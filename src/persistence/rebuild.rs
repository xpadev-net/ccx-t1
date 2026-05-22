use camino::Utf8Path;

use crate::error::CcxError;
use crate::persistence::jsonl::read_events_from_dir;
use crate::persistence::projector::apply_event_tx;
use crate::persistence::sqlite::{clear_dirty, open_db, run_migrations};

/// Rebuild the SQLite read model from scratch by replaying all events in `events.jsonl`.
///
/// Steps:
///   1. Acquire events.lock (exclusive) so no concurrent appends race the replay.
///   2. Drop all tables and re-run DDL migrations (clean slate).
///   3. Replay every event inside a single transaction.
///   4. Commit and clear the dirty marker.
pub fn rebuild(dir: &Utf8Path, project_id: &str) -> Result<(), CcxError> {
    use fd_lock::RwLock;
    use std::fs::OpenOptions;

    std::fs::create_dir_all(dir)?;

    // Step 1: hold the events.lock for the duration of the rebuild so no
    // concurrent append can add events between the snapshot and the replay.
    let lock_path = dir.join("events.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    let mut rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.write()?;

    // Step 2: read events under the lock.
    let events = read_events_from_dir(dir)?;

    // Step 3: wipe and reinitialise the DB.
    let mut conn = open_db(dir)?;
    wipe_tables(&conn)?;
    run_migrations(&conn)?;

    // Step 4: replay inside a single transaction for performance.
    {
        let tx = conn.transaction()?;
        for event in &events {
            apply_event_tx(&tx, event)?;
        }
        tx.commit()?;
    }

    // Step 5: clear the dirty flag.
    clear_dirty(dir, &conn, project_id)?;
    Ok(())
}

/// Result of a verify run.
#[derive(Debug)]
pub struct VerifyResult {
    pub consistent: bool,
    pub last_jsonl_event_id: Option<String>,
    pub last_applied_event_id: Option<String>,
    pub marker_present: bool,
}

/// Verify that the SQLite projection is up to date with `events.jsonl`.
///
/// Cheap check: compare the last event_id in the JSONL file with
/// `projects.last_applied_event_id`.  Also reports whether the dirty marker
/// file is present.
pub fn verify(dir: &Utf8Path, project_id: &str) -> Result<VerifyResult, CcxError> {
    use crate::persistence::sqlite::is_dirty;

    let events = read_events_from_dir(dir)?;
    let last_jsonl = events.last().map(|e| e.event_id.clone());

    let marker_present = is_dirty(dir);

    let conn = open_db(dir)?;
    let last_applied: Option<String> = conn
        .query_row(
            "SELECT last_applied_event_id FROM projects WHERE project_id = ?1",
            rusqlite::params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(None);

    let consistent = !marker_present && last_jsonl == last_applied;
    Ok(VerifyResult {
        consistent,
        last_jsonl_event_id: last_jsonl,
        last_applied_event_id: last_applied,
        marker_present,
    })
}

/// Drop all user tables so `run_migrations` starts from a clean slate.
fn wipe_tables(conn: &rusqlite::Connection) -> Result<(), CcxError> {
    conn.execute_batch(
        "
        DROP TABLE IF EXISTS merge_locks;
        DROP TABLE IF EXISTS write_leases;
        DROP TABLE IF EXISTS agent_sessions;
        DROP TABLE IF EXISTS work_executions;
        DROP TABLE IF EXISTS projects;
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{Actor, Event, EventData, ProjectRegisteredPayload};
    use crate::persistence::jsonl::append_event_to_dir;

    fn make_registered(project_id: &str) -> Event {
        Event::new(
            project_id,
            Actor::Controller,
            EventData::ProjectRegistered(ProjectRegisteredPayload {
                display_slug: "repo".into(),
                canonical_repo: "/tmp/repo".into(),
                task_source_file: "/tmp/repo/tasks.md".into(),
            }),
        )
    }

    #[test]
    fn rebuild_replays_events_and_clears_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let project_id = "01JTEST00000000000000000001";

        // Write two events to JSONL.
        let e1 = make_registered(project_id);
        let e2 = Event::new(
            project_id,
            Actor::Controller,
            EventData::TaskSourceFileChanged(
                crate::domain::event::TaskSourceFileChangedPayload {
                    task_source_file: "/tmp/repo/tasks2.md".into(),
                    new_hash: "abc".into(),
                },
            ),
        );
        append_event_to_dir(&dir, &e1).unwrap();
        append_event_to_dir(&dir, &e2).unwrap();

        // Mark dirty to simulate a prior projection failure.
        crate::persistence::sqlite::mark_dirty(&dir, project_id);
        assert!(crate::persistence::sqlite::is_dirty(&dir));

        rebuild(&dir, project_id).unwrap();

        // Dirty flag should be cleared.
        assert!(!crate::persistence::sqlite::is_dirty(&dir));

        // DB should reflect the replayed state.
        let conn = open_db(&dir).unwrap();
        let tsf: String = conn
            .query_row(
                "SELECT task_source_file FROM projects WHERE project_id = ?1",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tsf, "/tmp/repo/tasks2.md");
    }

    #[test]
    fn verify_detects_consistent_state() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let project_id = "01JTEST00000000000000000001";

        // append_event_to_dir auto-projects via try_project_event.
        let event = make_registered(project_id);
        append_event_to_dir(&dir, &event).unwrap();

        let result = verify(&dir, project_id).unwrap();
        assert!(result.consistent);
        assert_eq!(
            result.last_jsonl_event_id.as_deref(),
            Some(event.event_id.as_str())
        );
    }

    #[test]
    fn verify_detects_stale_db() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let project_id = "01JTEST00000000000000000001";

        // Append two events (both auto-projected), then roll back last_applied_event_id
        // to the first event to simulate a missed projection.
        let e1 = make_registered(project_id);
        let e2 = Event::new(
            project_id,
            Actor::System,
            EventData::TaskSourceFileChanged(
                crate::domain::event::TaskSourceFileChangedPayload {
                    task_source_file: "/tmp/repo/tasks2.md".into(),
                    new_hash: "xyz".into(),
                },
            ),
        );
        append_event_to_dir(&dir, &e1).unwrap();
        append_event_to_dir(&dir, &e2).unwrap();

        // Simulate missed projection by rolling back last_applied_event_id.
        let conn = open_db(&dir).unwrap();
        conn.execute(
            "UPDATE projects SET last_applied_event_id = ?1 WHERE project_id = ?2",
            rusqlite::params![e1.event_id, project_id],
        )
        .unwrap();

        let result = verify(&dir, project_id).unwrap();
        assert!(!result.consistent);
        assert_ne!(result.last_jsonl_event_id, result.last_applied_event_id);
    }
}

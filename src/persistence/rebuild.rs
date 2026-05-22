use camino::Utf8Path;
use fd_lock::RwLock;
use rusqlite::OptionalExtension;
use std::fs::OpenOptions;

use crate::error::CcxError;
use crate::persistence::jsonl::read_events_from_dir;
use crate::persistence::projector::apply_event_tx;
use crate::persistence::sqlite::{clear_dirty, open_db, run_migrations};

/// Rebuild the SQLite read model from scratch by replaying all events in `events.jsonl`.
///
/// The entire operation — DROP, CREATE, replay — is wrapped in a single SQLite
/// transaction so that a mid-replay failure leaves the original data intact.
pub fn rebuild(dir: &Utf8Path, project_id: &str) -> Result<(), CcxError> {
    std::fs::create_dir_all(dir)?;

    // Hold events.lock exclusively for the whole rebuild so no concurrent append
    // can add events between the JSONL snapshot and the replay.
    let lock_path = dir.join("events.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    let mut rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.write()?;

    let events = read_events_from_dir(dir)?;
    if events.is_empty() {
        tracing::warn!(
            project_id,
            dir = %dir,
            "rebuild: events.jsonl is absent or empty — 0 events replayed; DB will be empty"
        );
    }

    let mut conn = open_db(dir)?;

    // Single transaction: wipe tables → recreate schema → replay events.
    // If any step fails the transaction rolls back, leaving the original rows intact.
    {
        let tx = conn.transaction()?;
        wipe_tables(&tx)?;
        run_migrations(&tx)?;
        for event in &events {
            apply_event_tx(&tx, event)?;
        }
        tx.commit()?;
    }

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
/// Holds a shared read lock on `events.lock` for the duration of both the JSONL
/// read and the DB query to prevent a concurrent append from making the snapshot
/// appear inconsistent even when it is not.
pub fn verify(dir: &Utf8Path, project_id: &str) -> Result<VerifyResult, CcxError> {
    use crate::persistence::sqlite::is_dirty;

    let lock_path = dir.join("events.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    let mut rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.read()?;

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
        .optional()?;

    let consistent = !marker_present && last_jsonl == last_applied;
    Ok(VerifyResult {
        consistent,
        last_jsonl_event_id: last_jsonl,
        last_applied_event_id: last_applied,
        marker_present,
    })
}

/// Drop all user tables. Accepts `&Connection` or `&Transaction` via Deref coercion.
fn wipe_tables(conn: &rusqlite::Connection) -> Result<(), CcxError> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS merge_locks;
         DROP TABLE IF EXISTS write_leases;
         DROP TABLE IF EXISTS agent_sessions;
         DROP TABLE IF EXISTS work_executions;
         DROP TABLE IF EXISTS projects;",
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

        crate::persistence::sqlite::mark_dirty(&dir, project_id);
        assert!(crate::persistence::sqlite::is_dirty(&dir));

        rebuild(&dir, project_id).unwrap();

        assert!(!crate::persistence::sqlite::is_dirty(&dir));

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

        // Roll back last_applied_event_id to simulate a missed projection.
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

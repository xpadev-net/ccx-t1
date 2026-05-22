use camino::Utf8Path;
use rusqlite::{params, Connection};

use crate::domain::event::{Event, EventData};
use crate::error::CcxError;
use crate::persistence::sqlite::{mark_dirty, open_db};

/// Apply `event` to the SQLite read model inside a single transaction.
///
/// Every variant — including audit-only ones — updates `projects.last_applied_event_id`
/// so that `verify` can detect gaps between the JSONL log and the projection.
pub fn project_event(conn: &mut Connection, event: &Event) -> Result<(), CcxError> {
    let tx = conn.transaction()?;
    apply_event_tx(&tx, event)?;
    tx.commit()?;
    Ok(())
}

/// Attempt to project `event` into the SQLite DB located at `dir/state.sqlite`.
/// On failure, marks the DB dirty and logs a warning — returns `Ok(())` either way
/// because the JSONL write already succeeded and is the source of truth.
pub fn try_project_event(dir: &Utf8Path, event: &Event) {
    match open_db(dir).and_then(|mut conn| project_event(&mut conn, event)) {
        Ok(()) => {}
        Err(e) => {
            tracing::warn!(
                error = %e,
                project_id = %event.project_id,
                event_id = %event.event_id,
                "projector failed; marking sqlite dirty"
            );
            mark_dirty(dir, &event.project_id);
        }
    }
}

/// Apply `event` to the given transaction, including the `last_applied_event_id` bump.
/// Used by both `project_event` (one-event path) and rebuild (bulk-replay path).
pub fn apply_event_tx(tx: &rusqlite::Transaction<'_>, event: &Event) -> Result<(), CcxError> {
    apply_event_data(tx, event)?;
    tx.execute(
        "UPDATE projects SET last_applied_event_id = ?1 WHERE project_id = ?2",
        params![event.event_id, event.project_id],
    )?;
    Ok(())
}

fn apply_event_data(tx: &rusqlite::Transaction<'_>, event: &Event) -> Result<(), CcxError> {
    match &event.data {
        EventData::ProjectRegistered(p) => {
            tx.execute(
                "INSERT INTO projects (project_id, display_slug, canonical_repo, task_source_file, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    event.project_id,
                    p.display_slug,
                    p.canonical_repo,
                    p.task_source_file,
                    event.occurred_at
                ],
            )?;
        }

        EventData::TaskSourceFileChanged(p) => {
            tx.execute(
                "UPDATE projects SET task_source_file = ?1 WHERE project_id = ?2",
                params![p.task_source_file, event.project_id],
            )?;
        }

        EventData::WorkExecutionCreated(p) => {
            tx.execute(
                "INSERT INTO work_executions (
                    work_execution_id, project_id, state, branch_name, worktree_path,
                    task_file_path, source_path, selector_type, selector_value,
                    display_text, source_file_hash, selected_at
                 ) VALUES (?1, ?2, 'created', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    p.work_execution_id,
                    event.project_id,
                    p.branch_name,
                    p.worktree_path,
                    p.task_file_path,
                    p.source_path,
                    p.selector_type,
                    p.selector_value,
                    p.display_text,
                    p.source_file_hash,
                    event.occurred_at
                ],
            )?;
        }

        EventData::WorkExecutionTaskFileCreated(p) => {
            tx.execute(
                "UPDATE work_executions SET task_file_path = ?1 WHERE work_execution_id = ?2",
                params![p.task_file_path, p.work_execution_id],
            )?;
        }

        EventData::WorkExecutionStateChanged(p) => {
            tx.execute(
                "UPDATE work_executions SET state = ?1 WHERE work_execution_id = ?2",
                params![p.to.to_string(), p.work_execution_id],
            )?;
        }

        EventData::WorkExecutionTaskFileChanged(p) => {
            tx.execute(
                "UPDATE work_executions SET source_file_hash = ?1 WHERE work_execution_id = ?2",
                params![p.new_hash, p.work_execution_id],
            )?;
        }

        EventData::AgentSessionCreated(p) => {
            tx.execute(
                "INSERT INTO agent_sessions (
                    agent_session_id, project_id, work_execution_id, state, role,
                    attach_mode, cmux_tab_id, tmux_session_id, cwd,
                    started_at, last_heartbeat_at
                 ) VALUES (?1, ?2, ?3, 'starting', ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                params![
                    p.agent_session_id,
                    event.project_id,
                    p.work_execution_id,
                    p.role,
                    p.attach_mode,
                    p.cmux_tab_id,
                    p.tmux_session_id,
                    p.cwd,
                    event.occurred_at
                ],
            )?;
        }

        EventData::AgentSessionAttached(p) => {
            tx.execute(
                "UPDATE agent_sessions SET attach_mode = ?1 WHERE agent_session_id = ?2",
                params![p.attach_mode, p.agent_session_id],
            )?;
        }

        EventData::AgentSessionHeartbeat(p) => {
            tx.execute(
                "UPDATE agent_sessions SET last_heartbeat_at = ?1, pid = ?2, cwd = COALESCE(?3, cwd)
                 WHERE agent_session_id = ?4",
                params![
                    event.occurred_at,
                    p.pid.map(|v| v as i64),
                    p.cwd,
                    p.agent_session_id
                ],
            )?;
        }

        EventData::AgentSessionHung(p) => {
            tx.execute(
                "UPDATE agent_sessions SET state = 'hung' WHERE agent_session_id = ?1",
                params![p.agent_session_id],
            )?;
        }

        EventData::AgentSessionStopped(p) => {
            tx.execute(
                "UPDATE agent_sessions SET state = 'exited', exit_code = ?1 WHERE agent_session_id = ?2",
                params![p.exit_code, p.agent_session_id],
            )?;
        }

        EventData::AgentLifecycleStop(p) => {
            tx.execute(
                "UPDATE work_executions SET artifact_state = ?1, artifact_checked_at = ?2
                 WHERE work_execution_id = ?3",
                params![p.artifact_state, event.occurred_at, p.work_execution_id],
            )?;
        }

        EventData::WriteLeaseAcquired(p) => {
            tx.execute(
                "INSERT INTO write_leases (
                    write_lease_id, project_id, work_execution_id, worktree_path,
                    writer_agent_session_id, acquired_at, last_heartbeat_at, state
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 'active')",
                params![
                    p.write_lease_id,
                    event.project_id,
                    p.work_execution_id,
                    p.worktree_path,
                    p.writer_agent_session_id,
                    event.occurred_at
                ],
            )?;
        }

        EventData::WriteLeaseReleased(p) => {
            tx.execute(
                "UPDATE write_leases SET state = 'released' WHERE write_lease_id = ?1",
                params![p.write_lease_id],
            )?;
        }

        EventData::WriteLeaseStale(p) => {
            tx.execute(
                "UPDATE write_leases SET state = 'stale' WHERE write_lease_id = ?1",
                params![p.write_lease_id],
            )?;
        }

        EventData::WriteLeaseRevoked(p) => {
            tx.execute(
                "UPDATE write_leases SET state = 'revoked' WHERE write_lease_id = ?1",
                params![p.write_lease_id],
            )?;
        }

        EventData::PrOpened(p) => {
            tx.execute(
                "UPDATE work_executions SET pr_number = ?1, pr_url = ?2, head_commit = ?3
                 WHERE work_execution_id = ?4",
                params![
                    p.pr_number as i64,
                    p.pr_url,
                    p.head_commit,
                    p.work_execution_id
                ],
            )?;
        }

        EventData::PrHeadUpdated(p) => {
            tx.execute(
                "UPDATE work_executions SET head_commit = ?1 WHERE work_execution_id = ?2",
                params![p.head_commit, p.work_execution_id],
            )?;
        }

        EventData::MergeLockAcquired(p) => {
            tx.execute(
                "INSERT INTO merge_locks (
                    merge_lock_id, project_id, owner_agent_session_id, work_execution_id,
                    pr_number, acquired_at, last_heartbeat_at, state
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 'active')",
                params![
                    p.merge_lock_id,
                    event.project_id,
                    p.owner_agent_session_id,
                    p.work_execution_id,
                    p.pr_number as i64,
                    event.occurred_at
                ],
            )?;
        }

        EventData::MergeCompleted(p) => {
            tx.execute(
                "UPDATE merge_locks SET state = 'released' WHERE merge_lock_id = ?1",
                params![p.merge_lock_id],
            )?;
        }

        EventData::MergeFailed(p) => {
            tx.execute(
                "UPDATE merge_locks SET state = 'released' WHERE merge_lock_id = ?1",
                params![p.merge_lock_id],
            )?;
        }

        EventData::CanonicalSyncCompleted(p) => {
            tx.execute(
                "UPDATE work_executions SET sync_status = 'success', sync_warning = ?1
                 WHERE work_execution_id = ?2",
                params![p.sync_warning, p.work_execution_id],
            )?;
        }

        EventData::CanonicalSyncFailed(p) => {
            tx.execute(
                "UPDATE work_executions SET sync_status = 'aborted'
                 WHERE work_execution_id = ?1",
                params![p.work_execution_id],
            )?;
        }

        // Audit-only events — no state change beyond last_applied_event_id.
        EventData::AgentSessionPrompted(_)
        | EventData::GhReviewHookStarted(_)
        | EventData::GhReviewHookCompleted(_)
        | EventData::MergeStarted(_)
        | EventData::CleanupStarted(_)
        | EventData::CleanupCompleted(_)
        | EventData::UserIntervention(_)
        | EventData::WorktreeCreated(_)
        | EventData::BranchCreated(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{Actor, Event, EventData, ProjectRegisteredPayload};
    use crate::persistence::sqlite::open_db;

    fn project_registered(project_id: &str) -> Event {
        Event::new(
            project_id,
            Actor::Controller,
            EventData::ProjectRegistered(ProjectRegisteredPayload {
                display_slug: "test-repo".into(),
                canonical_repo: "/tmp/repo".into(),
                task_source_file: "/tmp/repo/tasks.md".into(),
            }),
        )
    }

    #[test]
    fn project_registered_inserts_row() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let mut conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";
        let event = project_registered(project_id);

        project_event(&mut conn, &event).unwrap();

        let slug: String = conn
            .query_row(
                "SELECT display_slug FROM projects WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(slug, "test-repo");

        let last_id: Option<String> = conn
            .query_row(
                "SELECT last_applied_event_id FROM projects WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_id.as_deref(), Some(event.event_id.as_str()));
    }

    #[test]
    fn audit_only_event_still_bumps_last_applied_event_id() {
        use crate::domain::event::{AgentSessionPromptedPayload, EventData};

        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let mut conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";

        // Seed the project row first.
        let seed = project_registered(project_id);
        project_event(&mut conn, &seed).unwrap();

        let audit_event = Event::new(
            project_id,
            Actor::Controller,
            EventData::AgentSessionPrompted(AgentSessionPromptedPayload {
                agent_session_id: "01JTEST00000000000000000002".into(),
                message_preview: "hello".into(),
            }),
        );
        project_event(&mut conn, &audit_event).unwrap();

        let last_id: Option<String> = conn
            .query_row(
                "SELECT last_applied_event_id FROM projects WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_id.as_deref(), Some(audit_event.event_id.as_str()));
    }
}

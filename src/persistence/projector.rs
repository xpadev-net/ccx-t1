use camino::Utf8Path;
use rusqlite::{params, Connection};

use crate::domain::event::{Event, EventData};
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;
use crate::persistence::sqlite::{mark_dirty, open_db};

/// Apply `event` to the SQLite read model inside a single transaction.
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
///
/// The `last_applied_event_id` UPDATE uses a forward-only condition (`< ?1`) so that
/// concurrent projections running after `events.lock` is released can never move the
/// pointer backwards — a stale commit returns 0 rows and triggers `mark_dirty`.
pub fn apply_event_tx(tx: &rusqlite::Transaction<'_>, event: &Event) -> Result<(), CcxError> {
    apply_event_data(tx, event)?;
    // Only advance last_applied_event_id; never move it backwards.
    // If rows == 0: either the project row is missing, or a concurrent projection
    // already committed a newer event_id — both are inconsistency signals.
    let rows = tx.execute(
        "UPDATE projects SET last_applied_event_id = ?1
         WHERE project_id = ?2
         AND (last_applied_event_id IS NULL OR last_applied_event_id < ?1)",
        params![event.event_id, event.project_id],
    )?;
    if rows == 0 {
        return Err(CcxError::Database(format!(
            "last_applied_event_id not advanced for project_id={} event={} \
             (project row missing or event arrived out of ULID order)",
            event.project_id, event.event_id
        )));
    }
    Ok(())
}

/// Returns an error when an entity-specific UPDATE affected no rows, indicating the
/// target row is absent (missed INSERT, out-of-order replay, or projection gap).
fn require_affected(rows: usize, entity: &str, id: &str) -> Result<(), CcxError> {
    if rows == 0 {
        Err(CcxError::Database(format!(
            "projector: UPDATE on {entity} id={id} matched 0 rows — row is missing or out of order"
        )))
    } else {
        Ok(())
    }
}

fn task_status_to_work_execution_state(status: &str) -> Option<WorkExecutionState> {
    match status {
        "assigned" => Some(WorkExecutionState::TaskFileCreated),
        "working" => Some(WorkExecutionState::Running),
        "pr_open" => Some(WorkExecutionState::PrOpen),
        "gate_check" => Some(WorkExecutionState::GateCheck),
        "review_fixing" => Some(WorkExecutionState::ReviewFixing),
        "merge_ready" => Some(WorkExecutionState::MergeReady),
        "returned" => Some(WorkExecutionState::Returned),
        "blocked" => Some(WorkExecutionState::Blocked),
        "failed" => Some(WorkExecutionState::Failed),
        "followup_required" => Some(WorkExecutionState::FollowupRequired),
        "merged" => Some(WorkExecutionState::Merged),
        _ => None,
    }
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
            let n = tx.execute(
                "UPDATE projects SET task_source_file = ?1 WHERE project_id = ?2",
                params![p.task_source_file, event.project_id],
            )?;
            require_affected(n, "project", &event.project_id)?;
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
            let n = tx.execute(
                "UPDATE work_executions SET task_file_path = ?1 WHERE work_execution_id = ?2",
                params![p.task_file_path, p.work_execution_id],
            )?;
            require_affected(n, "work_execution", &p.work_execution_id)?;
        }

        EventData::WorkExecutionStateChanged(p) => {
            let n = tx.execute(
                "UPDATE work_executions SET state = ?1 WHERE work_execution_id = ?2",
                params![p.to.to_string(), p.work_execution_id],
            )?;
            require_affected(n, "work_execution", &p.work_execution_id)?;
        }

        EventData::WorkExecutionTaskFileChanged(p) => {
            let n = if let Some(state) = p
                .new_status
                .as_deref()
                .and_then(task_status_to_work_execution_state)
            {
                tx.execute(
                    "UPDATE work_executions SET source_file_hash = ?1, state = ?2
                     WHERE work_execution_id = ?3",
                    params![p.new_hash, state.to_string(), p.work_execution_id],
                )?
            } else {
                tx.execute(
                    "UPDATE work_executions SET source_file_hash = ?1 WHERE work_execution_id = ?2",
                    params![p.new_hash, p.work_execution_id],
                )?
            };
            require_affected(n, "work_execution", &p.work_execution_id)?;
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
            let n = tx.execute(
                "UPDATE agent_sessions SET attach_mode = ?1 WHERE agent_session_id = ?2",
                params![p.attach_mode, p.agent_session_id],
            )?;
            require_affected(n, "agent_session", &p.agent_session_id)?;
        }

        EventData::AgentSessionHeartbeat(p) => {
            let n = tx.execute(
                "UPDATE agent_sessions SET last_heartbeat_at = ?1, pid = ?2, cwd = COALESCE(?3, cwd)
                 WHERE agent_session_id = ?4",
                params![
                    event.occurred_at,
                    p.pid.map(|v| v as i64),
                    p.cwd,
                    p.agent_session_id
                ],
            )?;
            require_affected(n, "agent_session", &p.agent_session_id)?;
        }

        EventData::AgentSessionHung(p) => {
            let n = tx.execute(
                "UPDATE agent_sessions SET state = 'hung' WHERE agent_session_id = ?1",
                params![p.agent_session_id],
            )?;
            require_affected(n, "agent_session", &p.agent_session_id)?;
        }

        EventData::AgentSessionStopped(p) => {
            let n = tx.execute(
                "UPDATE agent_sessions SET state = 'exited', exit_code = ?1 WHERE agent_session_id = ?2",
                params![p.exit_code, p.agent_session_id],
            )?;
            require_affected(n, "agent_session", &p.agent_session_id)?;
        }

        EventData::AgentLifecycleStop(p) => {
            let n = tx.execute(
                "UPDATE work_executions SET artifact_state = ?1, artifact_checked_at = ?2
                 WHERE work_execution_id = ?3",
                params![p.artifact_state, event.occurred_at, p.work_execution_id],
            )?;
            require_affected(n, "work_execution", &p.work_execution_id)?;
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
            let n = tx.execute(
                "UPDATE write_leases SET state = 'released' WHERE write_lease_id = ?1",
                params![p.write_lease_id],
            )?;
            require_affected(n, "write_lease", &p.write_lease_id)?;
        }

        EventData::WriteLeaseStale(p) => {
            let n = tx.execute(
                "UPDATE write_leases SET state = 'stale' WHERE write_lease_id = ?1",
                params![p.write_lease_id],
            )?;
            require_affected(n, "write_lease", &p.write_lease_id)?;
        }

        EventData::WriteLeaseRevoked(p) => {
            let n = tx.execute(
                "UPDATE write_leases SET state = 'revoked' WHERE write_lease_id = ?1",
                params![p.write_lease_id],
            )?;
            require_affected(n, "write_lease", &p.write_lease_id)?;
        }

        EventData::PrOpened(p) => {
            let n = tx.execute(
                "UPDATE work_executions SET pr_number = ?1, pr_url = ?2, head_commit = ?3
                 WHERE work_execution_id = ?4",
                params![
                    p.pr_number as i64,
                    p.pr_url,
                    p.head_commit,
                    p.work_execution_id
                ],
            )?;
            require_affected(n, "work_execution", &p.work_execution_id)?;
        }

        EventData::PrHeadUpdated(p) => {
            let n = tx.execute(
                "UPDATE work_executions SET head_commit = ?1 WHERE work_execution_id = ?2",
                params![p.head_commit, p.work_execution_id],
            )?;
            require_affected(n, "work_execution", &p.work_execution_id)?;
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
            let n = tx.execute(
                "UPDATE merge_locks SET state = 'released' WHERE merge_lock_id = ?1",
                params![p.merge_lock_id],
            )?;
            require_affected(n, "merge_lock", &p.merge_lock_id)?;
        }

        EventData::MergeFailed(p) => {
            let n = tx.execute(
                "UPDATE merge_locks SET state = 'released' WHERE merge_lock_id = ?1",
                params![p.merge_lock_id],
            )?;
            require_affected(n, "merge_lock", &p.merge_lock_id)?;
        }

        EventData::CanonicalSyncCompleted(p) => {
            let n = tx.execute(
                "UPDATE work_executions SET sync_status = 'success', sync_warning = ?1
                 WHERE work_execution_id = ?2",
                params![p.sync_warning, p.work_execution_id],
            )?;
            require_affected(n, "work_execution", &p.work_execution_id)?;
        }

        EventData::CanonicalSyncFailed(p) => {
            let n = tx.execute(
                "UPDATE work_executions SET sync_status = 'aborted'
                 WHERE work_execution_id = ?1",
                params![p.work_execution_id],
            )?;
            require_affected(n, "work_execution", &p.work_execution_id)?;
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
    use crate::domain::event::{
        Actor, Event, EventData, ProjectRegisteredPayload, WorkExecutionCreatedPayload,
        WorkExecutionTaskFileChangedPayload,
    };
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

    fn work_execution_created(project_id: &str, work_execution_id: &str) -> Event {
        Event::new(
            project_id,
            Actor::Controller,
            EventData::WorkExecutionCreated(WorkExecutionCreatedPayload {
                work_execution_id: work_execution_id.into(),
                branch_name: "ccx/test".into(),
                worktree_path: "/tmp/worktree".into(),
                task_file_path: "/tmp/task.md".into(),
                source_path: "/tmp/repo/tasks.md".into(),
                selector_type: "line".into(),
                selector_value: "1".into(),
                display_text: "test task".into(),
                source_file_hash: "initial-hash".into(),
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

    #[test]
    fn task_file_changed_updates_hash_and_state_from_status() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let mut conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";
        let work_execution_id = "01JTEST00000000000000000002";

        project_event(&mut conn, &project_registered(project_id)).unwrap();
        project_event(
            &mut conn,
            &work_execution_created(project_id, work_execution_id),
        )
        .unwrap();

        let changed = Event::new(
            project_id,
            Actor::System,
            EventData::WorkExecutionTaskFileChanged(WorkExecutionTaskFileChangedPayload {
                work_execution_id: work_execution_id.into(),
                new_hash: "task-hash-2".into(),
                new_status: Some("working".into()),
                status_changed: true,
                notification_priority: Default::default(),
            }),
        );
        project_event(&mut conn, &changed).unwrap();

        let (hash, state): (String, String) = conn
            .query_row(
                "SELECT source_file_hash, state FROM work_executions WHERE work_execution_id = ?1",
                params![work_execution_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(hash, "task-hash-2");
        assert_eq!(state, "running");
    }

    #[test]
    fn task_file_changed_without_status_leaves_state_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let mut conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";
        let work_execution_id = "01JTEST00000000000000000002";

        project_event(&mut conn, &project_registered(project_id)).unwrap();
        project_event(
            &mut conn,
            &work_execution_created(project_id, work_execution_id),
        )
        .unwrap();

        let changed = Event::new(
            project_id,
            Actor::System,
            EventData::WorkExecutionTaskFileChanged(WorkExecutionTaskFileChangedPayload {
                work_execution_id: work_execution_id.into(),
                new_hash: "task-hash-3".into(),
                new_status: None,
                status_changed: false,
                notification_priority: Default::default(),
            }),
        );
        project_event(&mut conn, &changed).unwrap();

        let (hash, state): (String, String) = conn
            .query_row(
                "SELECT source_file_hash, state FROM work_executions WHERE work_execution_id = ?1",
                params![work_execution_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(hash, "task-hash-3");
        assert_eq!(state, "created");
    }

    #[test]
    fn out_of_order_event_returns_error() {
        // Projecting an event whose event_id is lexicographically less than the
        // already-stored last_applied_event_id must return an error.
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let mut conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";

        // Seed with a "later" event id by manually setting last_applied_event_id.
        let seed = project_registered(project_id);
        project_event(&mut conn, &seed).unwrap();
        conn.execute(
            "UPDATE projects SET last_applied_event_id = '9ZZZZZZZZZZZZZZZZZZZZZZZZZ' WHERE project_id = ?1",
            params![project_id],
        )
        .unwrap();

        // Now try projecting an event with a smaller id — should fail.
        let late_event = Event::new(
            project_id,
            Actor::Controller,
            EventData::ProjectRegistered(ProjectRegisteredPayload {
                display_slug: "x".into(),
                canonical_repo: "/x".into(),
                task_source_file: "/x/t.md".into(),
            }),
        );
        // This will fail on the INSERT (UNIQUE constraint), but the test is about the
        // last_applied_event_id guard, so use an audit-only event to avoid that:
        use crate::domain::event::{AgentSessionPromptedPayload, EventData};
        let stale = Event::new(
            project_id,
            Actor::System,
            EventData::AgentSessionPrompted(AgentSessionPromptedPayload {
                agent_session_id: "01JTEST00000000000000000002".into(),
                message_preview: "stale".into(),
            }),
        );
        let result = project_event(&mut conn, &stale);
        assert!(result.is_err(), "stale event should be rejected");
    }
}

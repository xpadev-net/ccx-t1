use camino::Utf8PathBuf;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OptionalExtension};

use crate::agent_runtime::cmux_adapter::{make_adapter, CmuxAdapter};
use crate::domain::event::{Actor, Event, EventData, TaskSourceFileChangedPayload};
use crate::error::CcxError;
use crate::watcher::{open_notification_db, sha256_hex};

/// Per-project deduplication state for the source file watcher.
pub struct SourceWatcherState {
    pub last_seen_hash: Option<String>,
}

/// Pure observe step: hash the content and deduplicate.
///
/// Returns `Some(payload)` when the file content has changed since the last
/// call; `None` when it is identical to the previously seen version.
pub fn observe(
    content: &str,
    task_source_file: &str,
    state: &mut SourceWatcherState,
) -> Option<TaskSourceFileChangedPayload> {
    let hash = sha256_hex(content);
    if state.last_seen_hash.as_deref() == Some(hash.as_str()) {
        return None;
    }
    state.last_seen_hash = Some(hash.clone());
    Some(TaskSourceFileChangedPayload {
        task_source_file: task_source_file.to_string(),
        new_hash: hash,
    })
}

/// Watches the project's task source file for modifications and appends
/// `TaskSourceFileChanged` events to the project's JSONL audit log.
///
/// Drop this struct to stop watching.
pub struct SourceWatcher {
    _watcher: RecommendedWatcher,
}

fn notify_orchestrator_source_file_changed(
    conn: &Connection,
    cmux: &dyn CmuxAdapter,
    project_id: &str,
    task_source_file: &str,
) -> Result<bool, CcxError> {
    let cmux_tab_id: Option<String> = conn
        .query_row(
            "SELECT cmux_tab_id FROM agent_sessions
             WHERE project_id = ?1
               AND role = 'orchestrator'
               AND state IN ('starting', 'running', 'idle')
             ORDER BY started_at DESC
             LIMIT 1",
            rusqlite::params![project_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();

    let Some(cmux_tab_id) = cmux_tab_id else {
        return Ok(false);
    };

    cmux.notify_user(
        &cmux_tab_id,
        &format!("task source changed: {task_source_file}"),
        "info",
    )?;
    Ok(true)
}

fn notify_orchestrator_source_file_changed_best_effort(
    project_dir: &camino::Utf8Path,
    project_id: &str,
    task_source_file: &str,
) {
    let result = open_notification_db(project_dir).and_then(|conn| {
        let cmux = make_adapter();
        notify_orchestrator_source_file_changed(&conn, cmux.as_ref(), project_id, task_source_file)
            .map(|_| ())
    });
    if let Err(e) = result {
        tracing::warn!(error = %e, task_source_file, "source_watcher: orchestrator notify error");
    }
}

impl SourceWatcher {
    pub fn new(
        source_file: &camino::Utf8Path,
        project_id: String,
        project_dir: Utf8PathBuf,
    ) -> Result<Self, CcxError> {
        let mut state = SourceWatcherState {
            last_seen_hash: None,
        };
        let file = source_file.as_std_path().to_owned();
        let source_path = source_file.to_string();

        let mut watcher = notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                let ev = match res {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(error = %e, "source_watcher: notify error");
                        return;
                    }
                };
                if !ev.kind.is_modify() && !ev.kind.is_create() {
                    return;
                }
                let content = match std::fs::read_to_string(&file) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(path = %file.display(), error = %e, "source_watcher: read error");
                        return;
                    }
                };
                if let Some(payload) = observe(&content, &source_path, &mut state) {
                    let task_source_file = payload.task_source_file.clone();
                    let event = Event::new(
                        &project_id,
                        Actor::System,
                        EventData::TaskSourceFileChanged(payload),
                    );
                    if let Err(e) =
                        crate::persistence::jsonl::append_event_to_dir(&project_dir, &event)
                    {
                        tracing::warn!(error = %e, "source_watcher: append event error");
                    } else {
                        notify_orchestrator_source_file_changed_best_effort(
                            &project_dir,
                            &project_id,
                            &task_source_file,
                        );
                    }
                }
            },
        )?;

        watcher.watch(source_file.as_std_path(), RecursiveMode::NonRecursive)?;
        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::sqlite::open_db;
    use crate::watcher::test_support::{seed_project_and_orchestrator, SpyCmuxAdapter};

    fn state() -> SourceWatcherState {
        SourceWatcherState {
            last_seen_hash: None,
        }
    }

    #[test]
    fn first_observation_emits_event() {
        let mut s = state();
        let payload = observe("# tasks\n", "/repo/tasks.md", &mut s);
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.task_source_file, "/repo/tasks.md");
        assert!(!p.new_hash.is_empty());
    }

    #[test]
    fn identical_content_is_deduplicated() {
        let mut s = state();
        let content = "# tasks\ncontent\n";
        observe(content, "/repo/tasks.md", &mut s);
        let second = observe(content, "/repo/tasks.md", &mut s);
        assert!(second.is_none(), "identical content must be skipped");
    }

    #[test]
    fn changed_content_emits_new_event() {
        let mut s = state();
        observe("# v1\n", "/repo/tasks.md", &mut s);
        let payload = observe("# v2\n", "/repo/tasks.md", &mut s);
        assert!(payload.is_some());
    }

    #[test]
    fn hash_changes_with_content() {
        let mut s = state();
        let p1 = observe("# a\n", "/tasks.md", &mut s).unwrap();
        let p2 = observe("# b\n", "/tasks.md", &mut s).unwrap();
        assert_ne!(p1.new_hash, p2.new_hash);
    }

    #[test]
    fn source_file_path_preserved_in_payload() {
        let mut s = state();
        let path = "/home/user/project/z/tasks.md";
        let p = observe("content", path, &mut s).unwrap();
        assert_eq!(p.task_source_file, path);
    }

    #[test]
    fn no_initial_hash_means_first_call_always_emits() {
        let mut s = state();
        assert!(s.last_seen_hash.is_none());
        let p = observe("any content", "/tasks.md", &mut s);
        assert!(p.is_some());
        assert!(s.last_seen_hash.is_some());
    }

    #[test]
    fn source_change_notifies_active_orchestrator() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";
        seed_project_and_orchestrator(&conn, project_id, "tab-orch");
        let cmux = SpyCmuxAdapter::new();

        let notified = notify_orchestrator_source_file_changed(
            &conn,
            &cmux,
            project_id,
            "/tmp/repo/z/tasks.md",
        )
        .unwrap();

        assert!(notified);
        let notifications = cmux.notifications();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].0, "tab-orch");
        assert_eq!(notifications[0].2, "info");
        assert!(notifications[0].1.contains("/tmp/repo/z/tasks.md"));
    }

    #[test]
    fn missing_orchestrator_session_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_db(&dir).unwrap();
        let cmux = SpyCmuxAdapter::new();

        let notified = notify_orchestrator_source_file_changed(
            &conn,
            &cmux,
            "01JTEST00000000000000000001",
            "/tmp/repo/z/tasks.md",
        )
        .unwrap();

        assert!(!notified);
        assert!(cmux.notifications().is_empty());
    }

    #[test]
    fn null_orchestrator_tab_id_is_not_an_error() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let project_id = "01JTEST00000000000000000001";
        conn.execute_batch(
            "CREATE TABLE agent_sessions (
                agent_session_id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                state TEXT NOT NULL,
                role TEXT NOT NULL,
                cmux_tab_id TEXT,
                started_at TEXT NOT NULL
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_sessions (
                agent_session_id, project_id, state, role, cmux_tab_id, started_at
             ) VALUES ('01JTEST00000000000000000002', ?1, 'running', 'orchestrator', NULL, '2026-05-24T00:00:01Z')",
            rusqlite::params![project_id],
        )
        .unwrap();
        let cmux = SpyCmuxAdapter::new();

        let notified = notify_orchestrator_source_file_changed(
            &conn,
            &cmux,
            project_id,
            "/tmp/repo/z/tasks.md",
        )
        .unwrap();

        assert!(!notified);
        assert!(cmux.notifications().is_empty());
    }
}

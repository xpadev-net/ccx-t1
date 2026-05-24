use camino::Utf8PathBuf;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::agent_runtime::cmux_adapter::{make_adapter, CmuxAdapter};
use crate::domain::event::{
    Actor, Event, EventData, TaskFileChangePriority, WorkExecutionTaskFileChangedPayload,
};
use crate::error::CcxError;
use crate::watcher::front_matter::parse_front_matter;
use crate::watcher::sha256_hex;

/// Per-execution deduplication state for the task watcher.
pub struct TaskWatcherState {
    pub last_seen_hash: Option<String>,
    pub last_seen_status: Option<String>,
    pub has_seen_observation: bool,
}

/// Pure observe step: hash the content, deduplicate, parse front matter best-effort.
///
/// Returns `Some(payload)` when the content has changed since the last call;
/// `None` when the content is identical to the previously seen version.
pub fn observe(
    content: &str,
    work_execution_id: &str,
    state: &mut TaskWatcherState,
) -> Option<WorkExecutionTaskFileChangedPayload> {
    let hash = sha256_hex(content);
    if state.last_seen_hash.as_deref() == Some(hash.as_str()) {
        return None;
    }
    state.last_seen_hash = Some(hash.clone());
    let new_status = parse_front_matter(content).ok().and_then(|fm| fm.status);
    let status_changed = !state.has_seen_observation || new_status != state.last_seen_status;
    let notification_priority = if status_changed {
        TaskFileChangePriority::Normal
    } else {
        TaskFileChangePriority::Low
    };
    state.last_seen_status.clone_from(&new_status);
    state.has_seen_observation = true;
    Some(WorkExecutionTaskFileChangedPayload {
        work_execution_id: work_execution_id.to_string(),
        new_hash: hash,
        new_status,
        status_changed,
        notification_priority,
    })
}

/// Watches a single `task.md` file for modifications and appends
/// `WorkExecutionTaskFileChanged` events to the project's JSONL audit log.
///
/// Drop this struct to stop watching.
pub struct TaskWatcher {
    _watcher: RecommendedWatcher,
}

fn notify_orchestrator_task_file_changed(
    conn: &rusqlite::Connection,
    cmux: &dyn CmuxAdapter,
    project_id: &str,
    work_execution_id: &str,
    priority: TaskFileChangePriority,
) -> Result<bool, CcxError> {
    if priority == TaskFileChangePriority::Low {
        return Ok(false);
    }

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
        &format!("task.md changed for work execution {work_execution_id}"),
        "info",
    )?;
    Ok(true)
}

fn open_notification_db(project_dir: &camino::Utf8Path) -> Result<Connection, CcxError> {
    let conn = Connection::open_with_flags(
        project_dir.join("state.sqlite").as_std_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    Ok(conn)
}

fn notify_orchestrator_task_file_changed_best_effort(
    project_dir: &camino::Utf8Path,
    project_id: &str,
    work_execution_id: &str,
    priority: TaskFileChangePriority,
) {
    if priority == TaskFileChangePriority::Low {
        return;
    }

    let result = open_notification_db(project_dir).and_then(|conn| {
        let cmux = make_adapter();
        notify_orchestrator_task_file_changed(
            &conn,
            cmux.as_ref(),
            project_id,
            work_execution_id,
            priority,
        )
        .map(|_| ())
    });
    if let Err(e) = result {
        tracing::warn!(error = %e, work_execution_id, "task_watcher: orchestrator notify error");
    }
}

impl TaskWatcher {
    pub fn new(
        task_file: &camino::Utf8Path,
        project_id: String,
        work_execution_id: String,
        project_dir: Utf8PathBuf,
    ) -> Result<Self, CcxError> {
        let mut state = TaskWatcherState {
            last_seen_hash: None,
            last_seen_status: None,
            has_seen_observation: false,
        };
        let file = task_file.as_std_path().to_owned();

        let mut watcher = notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                let ev = match res {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(error = %e, "task_watcher: notify error");
                        return;
                    }
                };
                if !ev.kind.is_modify() && !ev.kind.is_create() {
                    return;
                }
                let content = match std::fs::read_to_string(&file) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(path = %file.display(), error = %e, "task_watcher: read error");
                        return;
                    }
                };
                if let Some(payload) = observe(&content, &work_execution_id, &mut state) {
                    let priority = payload.notification_priority;
                    let event = Event::new(
                        &project_id,
                        Actor::System,
                        EventData::WorkExecutionTaskFileChanged(payload),
                    );
                    if let Err(e) =
                        crate::persistence::jsonl::append_event_to_dir(&project_dir, &event)
                    {
                        tracing::warn!(error = %e, "task_watcher: append event error");
                    } else {
                        notify_orchestrator_task_file_changed_best_effort(
                            &project_dir,
                            &project_id,
                            &work_execution_id,
                            priority,
                        );
                    }
                }
            },
        )?;

        watcher.watch(task_file.as_std_path(), RecursiveMode::NonRecursive)?;
        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::cmux_adapter::AgentSessionSpec;
    use crate::persistence::sqlite::open_db;
    use std::sync::Mutex;

    fn state() -> TaskWatcherState {
        TaskWatcherState {
            last_seen_hash: None,
            last_seen_status: None,
            has_seen_observation: false,
        }
    }

    struct SpyCmuxAdapter {
        notifications: Mutex<Vec<(String, String, String)>>,
    }

    impl SpyCmuxAdapter {
        fn new() -> Self {
            Self {
                notifications: Mutex::new(vec![]),
            }
        }
    }

    impl CmuxAdapter for SpyCmuxAdapter {
        fn ensure_workspace(
            &self,
            _project_id: &str,
            _display_slug: &str,
            _canonical_repo: &str,
        ) -> Result<String, CcxError> {
            Ok("ws".into())
        }

        fn create_agent_tab(&self, _spec: &AgentSessionSpec) -> Result<String, CcxError> {
            Ok("tab".into())
        }

        fn close_tab(&self, _tab_id: &str) -> Result<(), CcxError> {
            Ok(())
        }

        fn notify_user(&self, tab_id: &str, message: &str, level: &str) -> Result<(), CcxError> {
            self.notifications.lock().unwrap().push((
                tab_id.to_string(),
                message.to_string(),
                level.to_string(),
            ));
            Ok(())
        }
    }

    fn seed_project_and_orchestrator(
        conn: &rusqlite::Connection,
        project_id: &str,
        cmux_tab_id: &str,
    ) {
        conn.execute(
            "INSERT INTO projects (
                project_id, display_slug, canonical_repo, task_source_file, created_at
             ) VALUES (?1, 'test', '/tmp/repo', '/tmp/repo/tasks.md', '2026-05-24T00:00:00Z')",
            rusqlite::params![project_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_sessions (
                agent_session_id, project_id, work_execution_id, state, role, attach_mode,
                cmux_tab_id, tmux_session_id, cwd, started_at, last_heartbeat_at
             ) VALUES (
                '01JTEST00000000000000000002', ?1, NULL, 'running', 'orchestrator', NULL,
                ?2, 'tmux-orch', '/tmp/repo', '2026-05-24T00:00:01Z', '2026-05-24T00:00:01Z'
             )",
            rusqlite::params![project_id, cmux_tab_id],
        )
        .unwrap();
    }

    #[test]
    fn first_observation_emits_event() {
        let mut s = state();
        let payload = observe("# task\n", "we-1", &mut s);
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.work_execution_id, "we-1");
        assert!(!p.new_hash.is_empty());
    }

    #[test]
    fn identical_content_is_deduplicated() {
        let mut s = state();
        let content = "---\nstatus: working\n---\n# task\n";
        observe(content, "we-1", &mut s);
        let second = observe(content, "we-1", &mut s);
        assert!(second.is_none(), "identical content must be skipped");
    }

    #[test]
    fn changed_content_emits_new_event() {
        let mut s = state();
        observe("# original\n", "we-1", &mut s);
        let payload = observe("# modified\n", "we-1", &mut s);
        assert!(payload.is_some());
    }

    #[test]
    fn status_is_extracted_from_front_matter() {
        let mut s = state();
        let content = "---\nstatus: pr_open\n---\n# task\n";
        let payload = observe(content, "we-1", &mut s).unwrap();
        assert_eq!(payload.new_status.as_deref(), Some("pr_open"));
        assert!(payload.status_changed);
        assert_eq!(
            payload.notification_priority,
            TaskFileChangePriority::Normal
        );
    }

    #[test]
    fn unchanged_status_lowers_notification_priority() {
        let mut s = state();
        observe("---\nstatus: working\n---\n# original\n", "we-1", &mut s);
        let payload = observe("---\nstatus: working\n---\n# modified\n", "we-1", &mut s).unwrap();
        assert!(!payload.status_changed);
        assert_eq!(payload.notification_priority, TaskFileChangePriority::Low);
    }

    #[test]
    fn first_observation_without_status_uses_normal_notification_priority() {
        let mut s = state();
        let payload = observe("# no front matter\n", "we-1", &mut s).unwrap();
        assert!(payload.status_changed);
        assert_eq!(
            payload.notification_priority,
            TaskFileChangePriority::Normal
        );
    }

    #[test]
    fn changed_status_uses_normal_notification_priority() {
        let mut s = state();
        observe("---\nstatus: assigned\n---\n# original\n", "we-1", &mut s);
        let payload = observe("---\nstatus: working\n---\n# modified\n", "we-1", &mut s).unwrap();
        assert!(payload.status_changed);
        assert_eq!(
            payload.notification_priority,
            TaskFileChangePriority::Normal
        );
    }

    #[test]
    fn missing_front_matter_yields_none_status() {
        let mut s = state();
        let payload = observe("# no front matter\n", "we-1", &mut s).unwrap();
        assert_eq!(payload.new_status, None);
    }

    #[test]
    fn malformed_front_matter_yields_none_status() {
        let mut s = state();
        let content = "---\nstatus: [unclosed\n---\n# task\n";
        let payload = observe(content, "we-1", &mut s).unwrap();
        assert_eq!(
            payload.new_status, None,
            "malformed YAML must be best-effort (no panic)"
        );
    }

    #[test]
    fn invalid_front_matter_status_yields_none_status() {
        let mut s = state();
        let content = "---\nstatus: merging\n---\n# task\n";
        let payload = observe(content, "we-1", &mut s).unwrap();
        assert_eq!(payload.new_status, None);
    }

    #[test]
    fn hash_is_deterministic() {
        let mut s1 = state();
        let mut s2 = state();
        let p1 = observe("# same\n", "we-1", &mut s1).unwrap();
        let p2 = observe("# same\n", "we-2", &mut s2).unwrap();
        assert_eq!(
            p1.new_hash, p2.new_hash,
            "SHA-256 must be content-dependent only"
        );
    }

    #[test]
    fn different_content_produces_different_hash() {
        let mut s = state();
        let p1 = observe("# a\n", "we-1", &mut s).unwrap();
        let p2 = observe("# b\n", "we-1", &mut s).unwrap();
        assert_ne!(p1.new_hash, p2.new_hash);
    }

    #[test]
    fn normal_priority_notifies_active_orchestrator() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";
        seed_project_and_orchestrator(&conn, project_id, "tab-orch");
        let cmux = SpyCmuxAdapter::new();

        let notified = notify_orchestrator_task_file_changed(
            &conn,
            &cmux,
            project_id,
            "01JTEST00000000000000000003",
            TaskFileChangePriority::Normal,
        )
        .unwrap();

        assert!(notified);
        let notifications = cmux.notifications.lock().unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].0, "tab-orch");
        assert_eq!(notifications[0].2, "info");
        assert!(notifications[0].1.contains("01JTEST00000000000000000003"));
    }

    #[test]
    fn low_priority_skips_orchestrator_notification() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_db(&dir).unwrap();
        let project_id = "01JTEST00000000000000000001";
        seed_project_and_orchestrator(&conn, project_id, "tab-orch");
        let cmux = SpyCmuxAdapter::new();

        let notified = notify_orchestrator_task_file_changed(
            &conn,
            &cmux,
            project_id,
            "01JTEST00000000000000000003",
            TaskFileChangePriority::Low,
        )
        .unwrap();

        assert!(!notified);
        assert!(cmux.notifications.lock().unwrap().is_empty());
    }

    #[test]
    fn missing_orchestrator_session_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_db(&dir).unwrap();
        let cmux = SpyCmuxAdapter::new();

        let notified = notify_orchestrator_task_file_changed(
            &conn,
            &cmux,
            "01JTEST00000000000000000001",
            "01JTEST00000000000000000003",
            TaskFileChangePriority::Normal,
        )
        .unwrap();

        assert!(!notified);
        assert!(cmux.notifications.lock().unwrap().is_empty());
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

        let notified = notify_orchestrator_task_file_changed(
            &conn,
            &cmux,
            project_id,
            "01JTEST00000000000000000003",
            TaskFileChangePriority::Normal,
        )
        .unwrap();

        assert!(!notified);
        assert!(cmux.notifications.lock().unwrap().is_empty());
    }

    #[test]
    fn notification_db_open_does_not_create_missing_database() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let result = open_notification_db(&dir);

        assert!(result.is_err());
        assert!(!dir.join("state.sqlite").exists());
    }
}

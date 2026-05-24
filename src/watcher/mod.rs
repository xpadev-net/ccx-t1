pub mod front_matter;
pub mod source_watcher;
pub mod task_watcher;

use sha2::{Digest, Sha256};

use crate::error::CcxError;

pub(crate) fn sha256_hex(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

pub(crate) fn open_notification_db(
    project_dir: &camino::Utf8Path,
) -> Result<rusqlite::Connection, CcxError> {
    let conn = rusqlite::Connection::open_with_flags(
        project_dir.join("state.sqlite").as_std_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    Ok(conn)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Mutex;

    use crate::agent_runtime::cmux_adapter::{AgentSessionSpec, CmuxAdapter};
    use crate::error::CcxError;

    pub(crate) struct SpyCmuxAdapter {
        notifications: Mutex<Vec<(String, String, String)>>,
    }

    impl SpyCmuxAdapter {
        pub(crate) fn new() -> Self {
            Self {
                notifications: Mutex::new(vec![]),
            }
        }

        pub(crate) fn notifications(
            &self,
        ) -> std::sync::MutexGuard<'_, Vec<(String, String, String)>> {
            self.notifications.lock().unwrap()
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

    pub(crate) fn seed_project_and_orchestrator(
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_db_open_does_not_create_missing_database() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let result = open_notification_db(&dir);

        assert!(result.is_err());
        assert!(!dir.join("state.sqlite").exists());
    }
}

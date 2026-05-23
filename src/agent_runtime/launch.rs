use std::collections::HashMap;
use std::path::PathBuf;

use camino::Utf8PathBuf;

use crate::agent_runtime::cmux_adapter::{AgentSessionSpec, CmuxAdapter};
use crate::agent_runtime::tmux_adapter::{session_name, session_target, TmuxAdapter};
use crate::domain::event::{Actor, AgentSessionCreatedPayload, Event, EventData};
use crate::error::CcxError;
use crate::persistence::jsonl::append_event_to_dir;

pub struct LaunchSpec {
    pub agent_session_id: String,
    pub project_id: String,
    pub project_dir: Utf8PathBuf,
    pub work_execution_id: Option<String>,
    pub role: String,
    pub attach_mode: Option<String>,
    pub cwd_path: PathBuf,
    pub worktree_path: Option<PathBuf>,
    pub envs: HashMap<String, String>,
    pub display_slug: String,
    pub canonical_repo: String,
}

#[derive(Debug)]
pub struct LaunchResult {
    pub tmux_session_id: String,
    pub cmux_workspace_id: String,
    pub cmux_tab_id: String,
}

/// Two-stage agent session launch:
/// 1. Create a background tmux session (`ccx-<session_id>`).
/// 2. Open a cmux tab that runs `tmux attach-session` against it (headless if cmux unavailable).
///
/// If stage 2 fails the tmux session is killed to avoid orphaned sessions.
/// If the event write fails both the tmux session and cmux tab are cleaned up.
/// Emits `AgentSessionCreated` on success.
pub fn launch_agent(
    spec: &LaunchSpec,
    tmux: &dyn TmuxAdapter,
    cmux: &dyn CmuxAdapter,
) -> Result<LaunchResult, CcxError> {
    let effective_cwd = spec.worktree_path.as_deref().unwrap_or(&spec.cwd_path);

    // Validate UTF-8 before allocating any resources so there is nothing to roll back.
    let cwd_str = effective_cwd
        .to_str()
        .ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!(
                "cwd path is not valid UTF-8: {effective_cwd:?}"
            ))
        })?
        .to_string();

    // Stage 1 — tmux session.
    tmux.create_session(&spec.agent_session_id, effective_cwd, &spec.envs)?;

    // Stage 2 — cmux workspace + tab. Kill tmux on failure to prevent orphaned sessions.
    let (cmux_workspace_id, cmux_tab_id) = match open_cmux_tab(spec, cmux) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = tmux.kill_session(&spec.agent_session_id);
            return Err(e);
        }
    };

    // Stage 3 — emit audit event. Roll back both resources on failure.
    let tmux_session_id = session_name(&spec.agent_session_id);
    let event = Event::new(
        &spec.project_id,
        Actor::Controller,
        EventData::AgentSessionCreated(AgentSessionCreatedPayload {
            agent_session_id: spec.agent_session_id.clone(),
            work_execution_id: spec.work_execution_id.clone(),
            role: spec.role.clone(),
            attach_mode: spec.attach_mode.clone(),
            cmux_tab_id: cmux_tab_id.clone(),
            tmux_session_id: tmux_session_id.clone(),
            cwd: cwd_str,
        }),
    );
    if let Err(e) = append_event_to_dir(&spec.project_dir, &event) {
        let _ = tmux.kill_session(&spec.agent_session_id);
        let _ = cmux.close_tab(&cmux_tab_id);
        return Err(e);
    }

    Ok(LaunchResult {
        tmux_session_id,
        cmux_workspace_id,
        cmux_tab_id,
    })
}

fn open_cmux_tab(spec: &LaunchSpec, cmux: &dyn CmuxAdapter) -> Result<(String, String), CcxError> {
    let workspace_id =
        cmux.ensure_workspace(&spec.project_id, &spec.display_slug, &spec.canonical_repo)?;

    let tab_spec = AgentSessionSpec {
        session_id: spec.agent_session_id.clone(),
        project_id: spec.project_id.clone(),
        cmux_workspace_id: workspace_id.clone(),
        role: spec.role.clone(),
        cwd_path: spec.cwd_path.clone(),
        work_execution_id: spec.work_execution_id.clone(),
        worktree_path: spec.worktree_path.clone(),
        envs: spec.envs.clone(),
        startup_command: format!("tmux attach-session -t {}", session_target(&spec.agent_session_id)),
    };
    let tab_id = cmux.create_agent_tab(&tab_spec)?;
    Ok((workspace_id, tab_id))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::cmux_adapter::HeadlessCmuxAdapter;
    use crate::domain::event::EventData;
    use crate::error::CcxError;
    use crate::persistence::jsonl::read_events_from_dir;
    use std::path::Path;
    use std::sync::Mutex;

    // --- mocks ---

    struct SpyTmuxAdapter {
        created: Mutex<Vec<String>>,
        killed: Mutex<Vec<String>>,
    }

    impl SpyTmuxAdapter {
        fn new() -> Self {
            Self { created: Mutex::new(vec![]), killed: Mutex::new(vec![]) }
        }
    }

    impl TmuxAdapter for SpyTmuxAdapter {
        fn create_session(
            &self,
            session_id: &str,
            _cwd: &Path,
            _envs: &HashMap<String, String>,
        ) -> Result<(), CcxError> {
            self.created.lock().unwrap().push(session_id.to_string());
            Ok(())
        }
        fn kill_session(&self, session_id: &str) -> Result<(), CcxError> {
            self.killed.lock().unwrap().push(session_id.to_string());
            Ok(())
        }
        fn session_exists(&self, _: &str) -> Result<bool, CcxError> { Ok(false) }
        fn get_pane_pid(&self, _: &str) -> Result<Option<u32>, CcxError> { Ok(None) }
        fn get_pane_cwd(&self, _: &str) -> Result<Option<String>, CcxError> { Ok(None) }
        fn send_keys(&self, _: &str, _: &str) -> Result<(), CcxError> { Ok(()) }
        fn send_literal(&self, _: &str, _: &str) -> Result<(), CcxError> { Ok(()) }
    }

    struct AlwaysFailCmuxAdapter;

    impl CmuxAdapter for AlwaysFailCmuxAdapter {
        fn ensure_workspace(&self, _: &str, _: &str, _: &str) -> Result<String, CcxError> {
            Err(CcxError::Other(anyhow::anyhow!("cmux unavailable")))
        }
        fn create_agent_tab(&self, _: &AgentSessionSpec) -> Result<String, CcxError> {
            Err(CcxError::Other(anyhow::anyhow!("cmux unavailable")))
        }
        fn close_tab(&self, _: &str) -> Result<(), CcxError> { Ok(()) }
        fn notify_user(&self, _: &str, _: &str, _: &str) -> Result<(), CcxError> { Ok(()) }
    }

    // --- helpers ---

    fn make_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        (tmp, dir)
    }

    fn make_spec(dir: &Utf8PathBuf) -> LaunchSpec {
        LaunchSpec {
            agent_session_id: "01JTEST00000000000000000003".into(),
            project_id: "01JTEST00000000000000000001".into(),
            project_dir: dir.clone(),
            work_execution_id: Some("01JTEST00000000000000000002".into()),
            role: "worker".into(),
            attach_mode: Some("writer".into()),
            cwd_path: PathBuf::from("/repos/myproject"),
            worktree_path: Some(PathBuf::from("/repos/myproject/.ccx/we-001")),
            envs: HashMap::new(),
            display_slug: "my-project".into(),
            canonical_repo: "/repos/myproject".into(),
        }
    }

    // --- tests ---

    #[test]
    fn launch_creates_tmux_session_cmux_tab_and_emits_event() {
        let (_tmp, dir) = make_dir();
        let spec = make_spec(&dir);
        let spy = SpyTmuxAdapter::new();

        let result = launch_agent(&spec, &spy, &HeadlessCmuxAdapter).unwrap();

        assert_eq!(*spy.created.lock().unwrap(), vec!["01JTEST00000000000000000003"]);
        assert!(spy.killed.lock().unwrap().is_empty(), "no kill on success");

        assert_eq!(result.tmux_session_id, "ccx-01JTEST00000000000000000003");
        assert!(result.cmux_tab_id.starts_with("headless-tab-"));
        assert!(result.cmux_workspace_id.starts_with("headless-ws-"));

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].data {
            EventData::AgentSessionCreated(p) => {
                assert_eq!(p.agent_session_id, "01JTEST00000000000000000003");
                assert_eq!(p.tmux_session_id, "ccx-01JTEST00000000000000000003");
                assert_eq!(p.role, "worker");
                assert_eq!(p.attach_mode.as_deref(), Some("writer"));
                assert_eq!(p.cwd, "/repos/myproject/.ccx/we-001");
            }
            other => panic!("expected AgentSessionCreated, got {other:?}"),
        }
    }

    #[test]
    fn launch_uses_cwd_when_no_worktree() {
        let (_tmp, dir) = make_dir();
        let mut spec = make_spec(&dir);
        spec.worktree_path = None;
        let spy = SpyTmuxAdapter::new();

        let _ = launch_agent(&spec, &spy, &HeadlessCmuxAdapter).unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        match &events[0].data {
            EventData::AgentSessionCreated(p) => assert_eq!(p.cwd, "/repos/myproject"),
            other => panic!("expected AgentSessionCreated, got {other:?}"),
        }
    }

    #[test]
    fn launch_kills_tmux_on_cmux_failure_and_emits_no_event() {
        let (_tmp, dir) = make_dir();
        let spec = make_spec(&dir);
        let spy = SpyTmuxAdapter::new();

        let err = launch_agent(&spec, &spy, &AlwaysFailCmuxAdapter).unwrap_err();
        assert!(err.to_string().contains("cmux unavailable"));

        assert_eq!(*spy.created.lock().unwrap(), vec!["01JTEST00000000000000000003"]);
        assert_eq!(*spy.killed.lock().unwrap(), vec!["01JTEST00000000000000000003"]);

        assert!(read_events_from_dir(&dir).unwrap().is_empty());
    }
}

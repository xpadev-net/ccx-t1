use std::path::PathBuf;

use camino::Utf8PathBuf;

use tracing::warn;

use crate::agent_runtime::prompt::send_to_tmux;
use crate::domain::event::{Actor, AgentLifecycleStopPayload, Event, EventData};
use crate::error::CcxError;
use crate::persistence::jsonl::append_event_to_dir;

pub struct LifecycleStopConfig {
    pub project_id: String,
    pub project_dir: Utf8PathBuf,
    pub agent_session_id: String,
    pub work_execution_id: String,
    /// Path to the task.md file for this work execution.
    pub task_file_path: PathBuf,
    /// If set, the lifecycle stop notification is forwarded to the orchestrator
    /// session via tmux (best-effort; failure does not abort the event write).
    pub orchestrator_session_id: Option<String>,
}

/// Validate the task file then emit an `AgentLifecycleStop` event.
///
/// `artifact_state` is `"ready"` when `task_file_path` exists and is non-empty;
/// `"invalid"` otherwise. The event is always emitted so the projector always
/// has an update to apply.
///
/// If `orchestrator_session_id` is set, a brief notification is injected into
/// that tmux session on a best-effort basis (failure is silently ignored).
pub fn handle_lifecycle_stop(config: &LifecycleStopConfig) -> Result<(), CcxError> {
    let artifact_state = validate_task_file(&config.task_file_path);

    let event = Event::new(
        &config.project_id,
        Actor::System,
        EventData::AgentLifecycleStop(AgentLifecycleStopPayload {
            agent_session_id: config.agent_session_id.clone(),
            work_execution_id: config.work_execution_id.clone(),
            artifact_state: artifact_state.to_string(),
        }),
    );
    append_event_to_dir(&config.project_dir, &event)?;

    if let Some(ref orch_id) = config.orchestrator_session_id {
        let msg = format!(
            "lifecycle_stop session={} work_execution={} artifact_state={}\n",
            config.agent_session_id, config.work_execution_id, artifact_state
        );
        // Best-effort — orchestrator tmux session may not be running.
        if let Err(e) = send_to_tmux(orch_id, &msg) {
            warn!("failed to notify orchestrator {orch_id}: {e}");
        }
    }

    Ok(())
}

/// Returns `"ready"` if the file exists and has at least one byte; `"invalid"` otherwise.
/// Unexpected I/O errors (EACCES, ELOOP, etc.) are logged and treated as `"invalid"`.
fn validate_task_file(path: &std::path::Path) -> &'static str {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > 0 => "ready",
        Ok(_) => "invalid",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "invalid",
        Err(e) => {
            warn!("unexpected error checking task file {:?}: {e}", path);
            "invalid"
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::EventData;
    use crate::persistence::jsonl::read_events_from_dir;
    use std::io::Write;

    fn make_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        (tmp, dir)
    }

    #[test]
    fn lifecycle_stop_ready_on_nonempty_task_file() {
        let (_tmp, dir) = make_dir();
        let mut task_file = tempfile::NamedTempFile::new().unwrap();
        task_file.write_all(b"## Task\ndo the thing\n").unwrap();

        let config = LifecycleStopConfig {
            project_id: "01JTEST00000000000000000001".into(),
            project_dir: dir.clone(),
            agent_session_id: "01JTEST00000000000000000002".into(),
            work_execution_id: "01JTEST00000000000000000003".into(),
            task_file_path: task_file.path().to_path_buf(),
            orchestrator_session_id: None,
        };
        handle_lifecycle_stop(&config).unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].data {
            EventData::AgentLifecycleStop(p) => {
                assert_eq!(p.agent_session_id, "01JTEST00000000000000000002");
                assert_eq!(p.work_execution_id, "01JTEST00000000000000000003");
                assert_eq!(p.artifact_state, "ready");
            }
            other => panic!("expected AgentLifecycleStop, got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_stop_invalid_on_missing_task_file() {
        let (_tmp, dir) = make_dir();

        let config = LifecycleStopConfig {
            project_id: "01JTEST00000000000000000001".into(),
            project_dir: dir.clone(),
            agent_session_id: "01JTEST00000000000000000002".into(),
            work_execution_id: "01JTEST00000000000000000003".into(),
            task_file_path: PathBuf::from("/does/not/exist/task.md"),
            orchestrator_session_id: None,
        };
        handle_lifecycle_stop(&config).unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].data {
            EventData::AgentLifecycleStop(p) => assert_eq!(p.artifact_state, "invalid"),
            other => panic!("expected AgentLifecycleStop, got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_stop_invalid_on_empty_task_file() {
        let (_tmp, dir) = make_dir();
        let task_file = tempfile::NamedTempFile::new().unwrap();
        // write nothing — 0 bytes

        let config = LifecycleStopConfig {
            project_id: "01JTEST00000000000000000001".into(),
            project_dir: dir.clone(),
            agent_session_id: "01JTEST00000000000000000002".into(),
            work_execution_id: "01JTEST00000000000000000003".into(),
            task_file_path: task_file.path().to_path_buf(),
            orchestrator_session_id: None,
        };
        handle_lifecycle_stop(&config).unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        match &events[0].data {
            EventData::AgentLifecycleStop(p) => assert_eq!(p.artifact_state, "invalid"),
            other => panic!("expected AgentLifecycleStop, got {other:?}"),
        }
    }
}

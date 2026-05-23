use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use camino::Utf8PathBuf;
use tracing::warn;

use crate::domain::event::{
    Actor, AgentSessionHeartbeatPayload, AgentSessionStoppedPayload, Event, EventData,
};
use crate::persistence::jsonl::append_event_to_dir;

use super::tmux_adapter::TmuxAdapter;

pub struct HeartbeatConfig {
    pub project_id: String,
    pub project_dir: Utf8PathBuf,
    pub agent_session_id: String,
    pub heartbeat_interval: Duration,
}

/// Spawn a background thread that polls the tmux session and appends
/// `AgentSessionHeartbeat` events until the session dies, then appends
/// `AgentSessionStopped` and exits.
///
/// Set `shutdown` to `true` to request early termination. The thread checks
/// the flag at the start of each iteration (after sleeping), so it stops
/// within one `heartbeat_interval`.
pub fn spawn_heartbeat(
    config: HeartbeatConfig,
    tmux: Arc<dyn TmuxAdapter>,
    shutdown: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || run_heartbeat_loop(config, tmux, shutdown))
}

fn run_heartbeat_loop(
    config: HeartbeatConfig,
    tmux: Arc<dyn TmuxAdapter>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        std::thread::sleep(config.heartbeat_interval);

        if shutdown.load(Ordering::Acquire) {
            // Emit a stopped event if the session already died before shutdown fired.
            // If the session is still alive the caller owns teardown and must emit the event.
            let still_alive = match tmux.session_exists(&config.agent_session_id) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "session_exists error on shutdown for {}: {e}",
                        config.agent_session_id
                    );
                    true // conservative: assume alive, caller owns teardown
                }
            };
            if !still_alive {
                let event = Event::new(
                    &config.project_id,
                    Actor::System,
                    EventData::AgentSessionStopped(AgentSessionStoppedPayload {
                        agent_session_id: config.agent_session_id.clone(),
                        exit_code: None,
                    }),
                );
                if let Err(e) = append_event_to_dir(&config.project_dir, &event) {
                    warn!(
                        "failed to append AgentSessionStopped for {}: {e}",
                        config.agent_session_id
                    );
                }
            }
            break;
        }

        let exists = match tmux.session_exists(&config.agent_session_id) {
            Ok(v) => v,
            Err(e) => {
                warn!("session_exists error for {}: {e}", config.agent_session_id);
                continue;
            }
        };

        if !exists {
            let event = Event::new(
                &config.project_id,
                Actor::System,
                EventData::AgentSessionStopped(AgentSessionStoppedPayload {
                    agent_session_id: config.agent_session_id.clone(),
                    exit_code: None,
                }),
            );
            if let Err(e) = append_event_to_dir(&config.project_dir, &event) {
                warn!("failed to append AgentSessionStopped for {}: {e}", config.agent_session_id);
            }
            break;
        }

        let pid = tmux.get_pane_pid(&config.agent_session_id).unwrap_or(None);
        let cwd = tmux.get_pane_cwd(&config.agent_session_id).unwrap_or(None);

        let event = Event::new(
            &config.project_id,
            Actor::System,
            EventData::AgentSessionHeartbeat(AgentSessionHeartbeatPayload {
                agent_session_id: config.agent_session_id.clone(),
                pid,
                cwd,
            }),
        );
        if let Err(e) = append_event_to_dir(&config.project_dir, &event) {
            warn!("failed to append AgentSessionHeartbeat for {}: {e}", config.agent_session_id);
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
    use crate::error::CcxError;
    use crate::persistence::jsonl::read_events_from_dir;
    use std::collections::{HashMap, VecDeque};
    use std::path::Path;
    use std::sync::Mutex;

    struct MockTmuxAdapter {
        // Use VecDeque so pop_front() matches left-to-right call order.
        exists_responses: Mutex<VecDeque<bool>>,
        pid: Option<u32>,
        cwd: Option<String>,
    }

    impl MockTmuxAdapter {
        fn new(exists_sequence: Vec<bool>, pid: Option<u32>, cwd: Option<String>) -> Self {
            Self {
                exists_responses: Mutex::new(exists_sequence.into()),
                pid,
                cwd,
            }
        }
    }

    impl TmuxAdapter for MockTmuxAdapter {
        fn create_session(
            &self,
            _session_id: &str,
            _cwd: &Path,
            _envs: &HashMap<String, String>,
        ) -> Result<(), CcxError> {
            Ok(())
        }
        fn kill_session(&self, _session_id: &str) -> Result<(), CcxError> {
            Ok(())
        }
        fn session_exists(&self, _session_id: &str) -> Result<bool, CcxError> {
            Ok(self.exists_responses.lock().unwrap().pop_front().unwrap_or(false))
        }
        fn get_pane_pid(&self, _session_id: &str) -> Result<Option<u32>, CcxError> {
            Ok(self.pid)
        }
        fn get_pane_cwd(&self, _session_id: &str) -> Result<Option<String>, CcxError> {
            Ok(self.cwd.clone())
        }
        fn send_keys(&self, _session_id: &str, _keys: &str) -> Result<(), CcxError> {
            Ok(())
        }
        fn send_literal(&self, _session_id: &str, _text: &str) -> Result<(), CcxError> {
            Ok(())
        }
    }

    fn make_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        (tmp, dir)
    }

    #[test]
    fn heartbeat_appends_events_then_stopped_on_session_death() {
        let (_tmp, dir) = make_dir();

        // Session reports alive for 2 iterations, then dead on the 3rd.
        let mock = Arc::new(MockTmuxAdapter::new(
            vec![true, true, false],
            Some(12345),
            Some("/repos/myproject".into()),
        ));
        let shutdown = Arc::new(AtomicBool::new(false));
        let config = HeartbeatConfig {
            project_id: "01JTEST00000000000000000001".into(),
            project_dir: dir.clone(),
            agent_session_id: "sess-001".into(),
            heartbeat_interval: Duration::from_millis(1),
        };

        let handle = spawn_heartbeat(config, mock, Arc::clone(&shutdown));
        handle.join().unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 3, "expected 2 heartbeats + 1 stopped");

        for event in &events[..2] {
            match &event.data {
                EventData::AgentSessionHeartbeat(p) => {
                    assert_eq!(p.agent_session_id, "sess-001");
                    assert_eq!(p.pid, Some(12345));
                    assert_eq!(p.cwd.as_deref(), Some("/repos/myproject"));
                }
                other => panic!("expected heartbeat, got {other:?}"),
            }
        }
        match &events[2].data {
            EventData::AgentSessionStopped(p) => {
                assert_eq!(p.agent_session_id, "sess-001");
                assert_eq!(p.exit_code, None);
            }
            other => panic!("expected stopped, got {other:?}"),
        }
    }

    #[test]
    fn shutdown_while_alive_emits_no_stopped_event() {
        let (_tmp, dir) = make_dir();

        // Session is still alive when shutdown fires → no AgentSessionStopped.
        let mock = Arc::new(MockTmuxAdapter::new(vec![true], None, None));
        let shutdown = Arc::new(AtomicBool::new(true));
        let config = HeartbeatConfig {
            project_id: "01JTEST00000000000000000001".into(),
            project_dir: dir.clone(),
            agent_session_id: "sess-002".into(),
            heartbeat_interval: Duration::from_millis(1),
        };

        let handle = spawn_heartbeat(config, mock, Arc::clone(&shutdown));
        handle.join().unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        assert!(
            !events.iter().any(|e| matches!(e.data, EventData::AgentSessionStopped(_))),
            "no stopped event expected when session is alive on shutdown"
        );
    }

    #[test]
    fn shutdown_while_dead_emits_stopped_event() {
        let (_tmp, dir) = make_dir();

        // Session is already dead when shutdown fires → AgentSessionStopped is emitted.
        let mock = Arc::new(MockTmuxAdapter::new(vec![false], None, None));
        let shutdown = Arc::new(AtomicBool::new(true));
        let config = HeartbeatConfig {
            project_id: "01JTEST00000000000000000001".into(),
            project_dir: dir.clone(),
            agent_session_id: "sess-003".into(),
            heartbeat_interval: Duration::from_millis(1),
        };

        let handle = spawn_heartbeat(config, mock, Arc::clone(&shutdown));
        handle.join().unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].data {
            EventData::AgentSessionStopped(p) => assert_eq!(p.agent_session_id, "sess-003"),
            other => panic!("expected stopped event, got {other:?}"),
        }
    }
}

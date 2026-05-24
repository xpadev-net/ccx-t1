use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use tracing::warn;

use crate::error::CcxError;

const SOCKET_PATH: &str = "/tmp/cmux.sock";
const RPC_IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

pub trait CmuxAdapter: Send + Sync {
    fn ensure_workspace(
        &self,
        project_id: &str,
        display_slug: &str,
        canonical_repo: &str,
    ) -> Result<String, CcxError>;

    fn create_agent_tab(&self, spec: &AgentSessionSpec) -> Result<String, CcxError>;

    fn close_tab(&self, tab_id: &str) -> Result<(), CcxError>;

    fn notify_user(&self, tab_id: &str, message: &str, level: &str) -> Result<(), CcxError>;
}

// ---------------------------------------------------------------------------
// AgentSessionSpec
// ---------------------------------------------------------------------------

pub struct AgentSessionSpec {
    pub session_id: String,
    pub project_id: String,
    pub cmux_workspace_id: String,
    pub role: String,
    pub cwd_path: PathBuf,
    pub work_execution_id: Option<String>,
    pub worktree_path: Option<PathBuf>,
    pub envs: HashMap<String, String>,
    pub startup_command: String,
}

// ---------------------------------------------------------------------------
// HeadlessCmuxAdapter — no-op fallback when cmux socket is unavailable
// ---------------------------------------------------------------------------

pub struct HeadlessCmuxAdapter;

impl CmuxAdapter for HeadlessCmuxAdapter {
    fn ensure_workspace(
        &self,
        project_id: &str,
        _display_slug: &str,
        _canonical_repo: &str,
    ) -> Result<String, CcxError> {
        Ok(format!("headless-ws-{project_id}"))
    }

    fn create_agent_tab(&self, spec: &AgentSessionSpec) -> Result<String, CcxError> {
        Ok(format!("headless-tab-{}", spec.session_id))
    }

    fn close_tab(&self, _tab_id: &str) -> Result<(), CcxError> {
        Ok(())
    }

    fn notify_user(&self, _tab_id: &str, message: &str, level: &str) -> Result<(), CcxError> {
        tracing::info!("[headless cmux notify] level={level} message={message}");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SocketCmuxAdapter — newline-delimited JSON-RPC 2.0 over Unix socket
// ---------------------------------------------------------------------------

pub struct SocketCmuxAdapter {
    socket_path: String,
}

impl SocketCmuxAdapter {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    fn send_rpc(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CcxError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static RPC_COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = RPC_COUNTER.fetch_add(1, Ordering::Relaxed).to_string();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| CcxError::Other(anyhow::anyhow!("cmux socket connect failed: {e}")))?;
        stream.set_read_timeout(Some(RPC_IO_TIMEOUT))?;
        stream.set_write_timeout(Some(RPC_IO_TIMEOUT))?;

        let mut writer = stream.try_clone()?;
        let mut msg = serde_json::to_string(&request)?;
        msg.push('\n');
        writer.write_all(msg.as_bytes())?;

        let mut reader = BufReader::new(&stream);
        let mut response_line = String::new();
        let n = reader
            .by_ref()
            .take(MAX_RESPONSE_BYTES)
            .read_line(&mut response_line)?;
        if n == 0 {
            return Err(CcxError::Other(anyhow::anyhow!(
                "cmux: server closed connection without sending a response"
            )));
        }

        let response: serde_json::Value = serde_json::from_str(response_line.trim())
            .map_err(|e| CcxError::Other(anyhow::anyhow!("cmux: malformed response: {e}")))?;

        if let Some(error) = response.get("error") {
            return Err(CcxError::Other(anyhow::anyhow!("cmux RPC error: {error}")));
        }

        Ok(response["result"].clone())
    }
}

impl CmuxAdapter for SocketCmuxAdapter {
    fn ensure_workspace(
        &self,
        project_id: &str,
        display_slug: &str,
        canonical_repo: &str,
    ) -> Result<String, CcxError> {
        let result = self.send_rpc(
            "workspace.create",
            serde_json::json!({
                "project_id": project_id,
                "name": format!("CCX: {display_slug}"),
                "cwd": canonical_repo,
            }),
        )?;
        let id = result["workspace_id"].as_str().ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!(
                "cmux: workspace.create response missing workspace_id: {result}"
            ))
        })?;
        Ok(id.to_string())
    }

    fn create_agent_tab(&self, spec: &AgentSessionSpec) -> Result<String, CcxError> {
        let cwd = spec.worktree_path.as_ref().unwrap_or(&spec.cwd_path);
        let cwd_str = cwd.to_str().ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!("cwd path is not valid UTF-8: {cwd:?}"))
        })?;
        let result = self.send_rpc(
            "surface.create",
            serde_json::json!({
                "workspace_id": spec.cmux_workspace_id,
                "title": format!("{} ({})", spec.role, spec.session_id),
                "cwd": cwd_str,
                "command": spec.startup_command,
                "envs": spec.envs,
            }),
        )?;
        let id = result["surface_id"].as_str().ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!(
                "cmux: surface.create response missing surface_id: {result}"
            ))
        })?;
        Ok(id.to_string())
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), CcxError> {
        self.send_rpc("surface.close", serde_json::json!({ "surface_id": tab_id }))?;
        Ok(())
    }

    fn notify_user(&self, tab_id: &str, message: &str, level: &str) -> Result<(), CcxError> {
        self.send_rpc(
            "ui.notify",
            serde_json::json!({
                "surface_id": tab_id,
                "level": level,
                "message": message,
            }),
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CliCmuxAdapter — raw JSON-RPC via `cmux rpc`
// ---------------------------------------------------------------------------

pub struct CliCmuxAdapter {
    cli_path: PathBuf,
}

impl CliCmuxAdapter {
    pub fn new(cli_path: impl Into<PathBuf>) -> Self {
        Self {
            cli_path: cli_path.into(),
        }
    }

    fn send_rpc(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CcxError> {
        let params_json = serde_json::to_string(&params)?;
        let output = self.run_rpc_process(method, params_json)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CcxError::Other(anyhow::anyhow!(
                "cmux CLI rpc {method} failed with status {}: {}",
                output.status,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let response: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|e| {
            CcxError::Other(anyhow::anyhow!(
                "cmux CLI rpc {method} returned malformed JSON: {e}"
            ))
        })?;

        if let Some(error) = response.get("error") {
            return Err(CcxError::Other(anyhow::anyhow!(
                "cmux CLI RPC error: {error}"
            )));
        }

        Ok(response.get("result").cloned().unwrap_or(response))
    }

    fn run_rpc_process(&self, method: &str, params_json: String) -> Result<Output, CcxError> {
        let mut child = Command::new(&self.cli_path)
            .arg("rpc")
            .arg(method)
            .arg(params_json)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                CcxError::Other(anyhow::anyhow!(
                    "cmux CLI failed to execute {}: {e}",
                    self.cli_path.display()
                ))
            })?;

        let pid = child.id();
        let (done_tx, done_rx) = mpsc::channel();
        let timed_out = Arc::new(AtomicBool::new(false));
        let timed_out_for_thread = Arc::clone(&timed_out);
        std::thread::spawn(move || {
            if done_rx.recv_timeout(RPC_IO_TIMEOUT).is_err() {
                timed_out_for_thread.store(true, Ordering::SeqCst);
                let _ = Command::new("kill")
                    .arg("-KILL")
                    .arg(pid.to_string())
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        });

        let output = child.wait_with_output().map_err(CcxError::from)?;
        let _ = done_tx.send(());

        if timed_out.load(Ordering::SeqCst) {
            return Err(CcxError::Other(anyhow::anyhow!(
                "cmux CLI rpc {method} timed out after {:?}",
                RPC_IO_TIMEOUT
            )));
        }

        Ok(output)
    }
}

impl CmuxAdapter for CliCmuxAdapter {
    fn ensure_workspace(
        &self,
        project_id: &str,
        display_slug: &str,
        canonical_repo: &str,
    ) -> Result<String, CcxError> {
        let result = self.send_rpc(
            "workspace.create",
            serde_json::json!({
                "project_id": project_id,
                "name": format!("CCX: {display_slug}"),
                "cwd": canonical_repo,
            }),
        )?;
        let id = result["workspace_id"].as_str().ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!(
                "cmux CLI: workspace.create response missing workspace_id: {result}"
            ))
        })?;
        Ok(id.to_string())
    }

    fn create_agent_tab(&self, spec: &AgentSessionSpec) -> Result<String, CcxError> {
        let cwd = spec.worktree_path.as_ref().unwrap_or(&spec.cwd_path);
        let cwd_str = cwd.to_str().ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!("cwd path is not valid UTF-8: {cwd:?}"))
        })?;
        let result = self.send_rpc(
            "surface.create",
            serde_json::json!({
                "workspace_id": spec.cmux_workspace_id,
                "title": format!("{} ({})", spec.role, spec.session_id),
                "cwd": cwd_str,
                "command": spec.startup_command,
                "envs": spec.envs,
            }),
        )?;
        let id = result["surface_id"].as_str().ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!(
                "cmux CLI: surface.create response missing surface_id: {result}"
            ))
        })?;
        Ok(id.to_string())
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), CcxError> {
        self.send_rpc("surface.close", serde_json::json!({ "surface_id": tab_id }))?;
        Ok(())
    }

    fn notify_user(&self, tab_id: &str, message: &str, level: &str) -> Result<(), CcxError> {
        self.send_rpc(
            "ui.notify",
            serde_json::json!({
                "surface_id": tab_id,
                "level": level,
                "message": message,
            }),
        )?;
        Ok(())
    }
}

pub struct CliFallbackCmuxAdapter {
    cli: CliCmuxAdapter,
    headless: HeadlessCmuxAdapter,
    mode: Mutex<CliFallbackMode>,
}

impl CliFallbackCmuxAdapter {
    pub fn new(cli_path: impl Into<PathBuf>) -> Self {
        Self {
            cli: CliCmuxAdapter::new(cli_path),
            headless: HeadlessCmuxAdapter,
            mode: Mutex::new(CliFallbackMode::Unknown),
        }
    }

    fn mode_guard(&self) -> std::sync::MutexGuard<'_, CliFallbackMode> {
        self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl CmuxAdapter for CliFallbackCmuxAdapter {
    fn ensure_workspace(
        &self,
        project_id: &str,
        display_slug: &str,
        canonical_repo: &str,
    ) -> Result<String, CcxError> {
        let mut mode = self.mode_guard();
        if *mode == CliFallbackMode::Headless {
            return self
                .headless
                .ensure_workspace(project_id, display_slug, canonical_repo);
        }

        match self
            .cli
            .ensure_workspace(project_id, display_slug, canonical_repo)
        {
            Ok(id) => {
                *mode = CliFallbackMode::Cli;
                Ok(id)
            }
            Err(e) => {
                if *mode == CliFallbackMode::Cli {
                    Err(e)
                } else {
                    warn!("cmux CLI workspace fallback failed, running headless: {e}");
                    *mode = CliFallbackMode::Headless;
                    self.headless
                        .ensure_workspace(project_id, display_slug, canonical_repo)
                }
            }
        }
    }

    fn create_agent_tab(&self, spec: &AgentSessionSpec) -> Result<String, CcxError> {
        let mut mode = self.mode_guard();
        if *mode == CliFallbackMode::Headless {
            return self.headless.create_agent_tab(spec);
        }

        match self.cli.create_agent_tab(spec) {
            Ok(id) => {
                *mode = CliFallbackMode::Cli;
                Ok(id)
            }
            Err(e) => {
                if *mode == CliFallbackMode::Cli {
                    Err(e)
                } else {
                    warn!("cmux CLI tab fallback failed, running headless: {e}");
                    *mode = CliFallbackMode::Headless;
                    self.headless.create_agent_tab(spec)
                }
            }
        }
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), CcxError> {
        let mut mode = self.mode_guard();
        if *mode == CliFallbackMode::Headless {
            return self.headless.close_tab(tab_id);
        }

        match self.cli.close_tab(tab_id) {
            Ok(()) => {
                *mode = CliFallbackMode::Cli;
                Ok(())
            }
            Err(e) => {
                if *mode == CliFallbackMode::Cli {
                    Err(e)
                } else {
                    warn!("cmux CLI close fallback failed, running headless: {e}");
                    *mode = CliFallbackMode::Headless;
                    self.headless.close_tab(tab_id)
                }
            }
        }
    }

    fn notify_user(&self, tab_id: &str, message: &str, level: &str) -> Result<(), CcxError> {
        let mut mode = self.mode_guard();
        if *mode == CliFallbackMode::Headless {
            return self.headless.notify_user(tab_id, message, level);
        }

        match self.cli.notify_user(tab_id, message, level) {
            Ok(()) => {
                *mode = CliFallbackMode::Cli;
                Ok(())
            }
            Err(e) => {
                if *mode == CliFallbackMode::Cli {
                    Err(e)
                } else {
                    warn!("cmux CLI notify fallback failed, running headless: {e}");
                    *mode = CliFallbackMode::Headless;
                    self.headless.notify_user(tab_id, message, level)
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CliFallbackMode {
    Unknown,
    Cli,
    Headless,
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn make_adapter() -> Box<dyn CmuxAdapter> {
    make_adapter_from(SOCKET_PATH)
}

pub fn make_adapter_from(socket_path: &str) -> Box<dyn CmuxAdapter> {
    let cli_path = cmux_cli_path();
    make_adapter_from_with_cli(socket_path, cli_path)
}

pub fn make_adapter_from_with_cli(
    socket_path: &str,
    cli_path: impl Into<PathBuf>,
) -> Box<dyn CmuxAdapter> {
    use std::os::unix::fs::FileTypeExt;
    let cli_path = cli_path.into();
    let is_socket = std::fs::metadata(socket_path)
        .map(|m| m.file_type().is_socket())
        .unwrap_or(false);
    if is_socket {
        Box::new(SocketCmuxAdapter::new(socket_path))
    } else if cmux_cli_available(&cli_path) {
        warn!("cmux socket unavailable at {socket_path} (missing or not a socket), falling back to cmux CLI");
        Box::new(CliFallbackCmuxAdapter::new(cli_path))
    } else {
        warn!(
            "cmux socket unavailable at {socket_path} (missing or not a socket), running headless"
        );
        Box::new(HeadlessCmuxAdapter)
    }
}

fn cmux_cli_path() -> PathBuf {
    std::env::var_os("CCX_CMUX_CLI")
        .or_else(|| std::env::var_os("CMUX_CLI_BIN"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cmux"))
}

fn cmux_cli_available(cli_path: &Path) -> bool {
    Command::new(cli_path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::thread;

    // Spawn a single-shot mock server. Reads one JSON-RPC request line, writes
    // `reply` as a response line, then returns the parsed request.
    fn spawn_mock(path: String, reply: serde_json::Value) -> thread::JoinHandle<serde_json::Value> {
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind mock socket");
        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut writer = stream.try_clone().expect("clone stream");
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).expect("read request");
            let mut resp = reply.to_string();
            resp.push('\n');
            writer.write_all(resp.as_bytes()).expect("write response");
            serde_json::from_str(request_line.trim()).unwrap_or(serde_json::Value::Null)
        })
    }

    fn sock_path(dir: &tempfile::TempDir) -> String {
        dir.path().join("cmux.sock").to_string_lossy().into_owned()
    }

    fn sample_agent_spec() -> AgentSessionSpec {
        let mut envs = HashMap::new();
        envs.insert("CCX_PROJECT_ID".into(), "proj-1".into());
        AgentSessionSpec {
            session_id: "sess-999".into(),
            project_id: "proj-1".into(),
            cmux_workspace_id: "ws-abc".into(),
            role: "worker".into(),
            cwd_path: PathBuf::from("/repos/myproject"),
            work_execution_id: Some("we-111".into()),
            worktree_path: Some(PathBuf::from("/repos/myproject/.ccx/we-111")),
            envs,
            startup_command: "tmux attach-session -t ccx-sess-999".into(),
        }
    }

    fn fake_cmux_script(dir: &tempfile::TempDir) -> (PathBuf, PathBuf) {
        let script = dir.path().join("cmux");
        let log = dir.path().join("calls.log");
        let script_body = format!(
            r#"#!/bin/sh
printf '%s|%s|%s\n' "$1" "$2" "$3" >> '{}'
if [ "$1" = "--version" ]; then
  exit 0
fi
if [ "$1" != "rpc" ]; then
  exit 2
fi
case "$2" in
  workspace.create)
    printf '{{"workspace_id":"ws-cli"}}\n'
    ;;
  surface.create)
    printf '{{"surface_id":"tab-cli"}}\n'
    ;;
  surface.close|ui.notify)
    printf '{{}}\n'
    ;;
  *)
    exit 3
    ;;
esac
"#,
            log.display()
        );
        fs::write(&script, script_body).unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        (script, log)
    }

    // --- HeadlessCmuxAdapter tests ---

    #[test]
    fn headless_ensure_workspace_returns_sentinel() {
        let adapter = HeadlessCmuxAdapter;
        let id = adapter
            .ensure_workspace("proj-abc", "my-repo", "/path/to/repo")
            .unwrap();
        assert_eq!(id, "headless-ws-proj-abc");
    }

    #[test]
    fn headless_create_tab_returns_sentinel() {
        let adapter = HeadlessCmuxAdapter;
        let spec = AgentSessionSpec {
            session_id: "sess-123".into(),
            project_id: "proj-abc".into(),
            cmux_workspace_id: "headless-ws-proj-abc".into(),
            role: "worker".into(),
            cwd_path: PathBuf::from("/tmp"),
            work_execution_id: None,
            worktree_path: None,
            envs: HashMap::new(),
            startup_command: "tmux attach-session -t ccx-sess-123".into(),
        };
        let id = adapter.create_agent_tab(&spec).unwrap();
        assert_eq!(id, "headless-tab-sess-123");
    }

    #[test]
    fn headless_close_and_notify_are_noop() {
        let adapter = HeadlessCmuxAdapter;
        adapter.close_tab("any-tab-id").unwrap();
        adapter.notify_user("any-tab-id", "hello", "info").unwrap();
    }

    // --- SocketCmuxAdapter tests ---
    //
    // Note: mock response "id" values are intentionally arbitrary strings.
    // send_rpc now uses an AtomicU64 counter for request IDs but does NOT
    // validate that the response ID echoes the request, so any id value in
    // the mock reply is accepted. If response-ID validation is ever added,
    // these fixtures will need updating to match the counter sequence.

    #[test]
    fn socket_adapter_sends_workspace_create() {
        let dir = tempfile::tempdir().unwrap();
        let path = sock_path(&dir);
        let server = spawn_mock(
            path.clone(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "ws-create-1",
                "result": { "workspace_id": "ws-abc" }
            }),
        );

        let adapter = SocketCmuxAdapter::new(&path);
        let ws_id = adapter
            .ensure_workspace("proj-1", "my-slug", "/repos/myproject")
            .unwrap();
        assert_eq!(ws_id, "ws-abc");

        let request = server.join().unwrap();
        assert_eq!(request["method"], "workspace.create");
        assert_eq!(request["params"]["project_id"], "proj-1");
        assert_eq!(request["params"]["name"], "CCX: my-slug");
        assert_eq!(request["params"]["cwd"], "/repos/myproject");
        assert_eq!(request["jsonrpc"], "2.0");
    }

    #[test]
    fn socket_adapter_sends_surface_create() {
        let dir = tempfile::tempdir().unwrap();
        let path = sock_path(&dir);
        let server = spawn_mock(
            path.clone(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tab-create-1",
                "result": { "surface_id": "tab-xyz" }
            }),
        );

        let adapter = SocketCmuxAdapter::new(&path);
        let spec = sample_agent_spec();
        let tab_id = adapter.create_agent_tab(&spec).unwrap();
        assert_eq!(tab_id, "tab-xyz");

        let request = server.join().unwrap();
        assert_eq!(request["method"], "surface.create");
        assert_eq!(request["params"]["workspace_id"], "ws-abc");
        assert_eq!(request["params"]["title"], "worker (sess-999)");
        assert_eq!(
            request["params"]["command"],
            "tmux attach-session -t ccx-sess-999"
        );
    }

    #[test]
    fn socket_adapter_sends_surface_close() {
        let dir = tempfile::tempdir().unwrap();
        let path = sock_path(&dir);
        let server = spawn_mock(
            path.clone(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tab-close-1",
                "result": {}
            }),
        );

        let adapter = SocketCmuxAdapter::new(&path);
        adapter.close_tab("tab-xyz").unwrap();

        let request = server.join().unwrap();
        assert_eq!(request["method"], "surface.close");
        assert_eq!(request["params"]["surface_id"], "tab-xyz");
    }

    #[test]
    fn socket_adapter_sends_ui_notify() {
        let dir = tempfile::tempdir().unwrap();
        let path = sock_path(&dir);
        let server = spawn_mock(
            path.clone(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "notify-1",
                "result": {}
            }),
        );

        let adapter = SocketCmuxAdapter::new(&path);
        adapter
            .notify_user("tab-xyz", "User intervention required", "warning")
            .unwrap();

        let request = server.join().unwrap();
        assert_eq!(request["method"], "ui.notify");
        assert_eq!(request["params"]["surface_id"], "tab-xyz");
        assert_eq!(request["params"]["level"], "warning");
        assert_eq!(request["params"]["message"], "User intervention required");
    }

    #[test]
    fn make_adapter_from_returns_headless_when_socket_absent() {
        let dir = tempfile::tempdir().unwrap();
        let absent = dir
            .path()
            .join("absent.sock")
            .to_string_lossy()
            .into_owned();
        let absent_cli = dir.path().join("missing-cmux");
        // Socket file does not exist — must fall back to headless
        let adapter = make_adapter_from_with_cli(&absent, absent_cli);
        let ws_id = adapter.ensure_workspace("proj-x", "slug", "/repo").unwrap();
        assert!(
            ws_id.starts_with("headless-"),
            "expected headless sentinel, got: {ws_id}"
        );
    }

    #[test]
    fn make_adapter_from_returns_socket_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = sock_path(&dir);
        let listener = UnixListener::bind(&path).unwrap();
        // Accept one connection and immediately drop it — causes instant EOF
        // on the client's read_line rather than waiting for RPC_IO_TIMEOUT.
        thread::spawn(move || {
            let _ = listener.accept();
        });
        let adapter = make_adapter_from(&path);
        let result = adapter.ensure_workspace("proj-x", "slug", "/repo");
        // EOF → "server closed connection" error, not a headless sentinel
        assert!(
            result.is_err() || !result.as_ref().unwrap().starts_with("headless-"),
            "socket adapter should not return a headless sentinel"
        );
    }

    #[test]
    fn socket_adapter_errors_on_missing_workspace_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = sock_path(&dir);
        // Response is missing workspace_id field
        let _server = spawn_mock(
            path.clone(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "ws-create-1",
                "result": { "some_other_field": "oops" }
            }),
        );

        let adapter = SocketCmuxAdapter::new(&path);
        let result = adapter.ensure_workspace("proj-1", "slug", "/repo");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("workspace_id"),
            "error should mention the missing field: {msg}"
        );
    }

    // --- CliCmuxAdapter tests ---

    #[test]
    fn cli_adapter_sends_workspace_create() {
        let dir = tempfile::tempdir().unwrap();
        let (script, log) = fake_cmux_script(&dir);

        let adapter = CliCmuxAdapter::new(script);
        let ws_id = adapter
            .ensure_workspace("proj-1", "my-slug", "/repos/myproject")
            .unwrap();

        assert_eq!(ws_id, "ws-cli");
        let calls = fs::read_to_string(log).unwrap();
        assert!(calls.contains("rpc|workspace.create|"));
        assert!(calls.contains("\"project_id\":\"proj-1\""));
        assert!(calls.contains("\"name\":\"CCX: my-slug\""));
        assert!(calls.contains("\"cwd\":\"/repos/myproject\""));
    }

    #[test]
    fn cli_adapter_sends_surface_create() {
        let dir = tempfile::tempdir().unwrap();
        let (script, log) = fake_cmux_script(&dir);

        let adapter = CliCmuxAdapter::new(script);
        let tab_id = adapter.create_agent_tab(&sample_agent_spec()).unwrap();

        assert_eq!(tab_id, "tab-cli");
        let calls = fs::read_to_string(log).unwrap();
        assert!(calls.contains("rpc|surface.create|"));
        assert!(calls.contains("\"workspace_id\":\"ws-abc\""));
        assert!(calls.contains("\"title\":\"worker (sess-999)\""));
        assert!(calls.contains("\"cwd\":\"/repos/myproject/.ccx/we-111\""));
    }

    #[test]
    fn cli_adapter_sends_surface_close_and_notify() {
        let dir = tempfile::tempdir().unwrap();
        let (script, log) = fake_cmux_script(&dir);

        let adapter = CliCmuxAdapter::new(script);
        adapter.close_tab("tab-cli").unwrap();
        adapter.notify_user("tab-cli", "hello", "info").unwrap();

        let calls = fs::read_to_string(log).unwrap();
        assert!(calls.contains("rpc|surface.close|"));
        assert!(calls.contains("\"surface_id\":\"tab-cli\""));
        assert!(calls.contains("rpc|ui.notify|"));
        assert!(calls.contains("\"message\":\"hello\""));
        assert!(calls.contains("\"level\":\"info\""));
    }

    #[test]
    fn make_adapter_from_uses_cli_when_socket_absent_and_cli_available() {
        let dir = tempfile::tempdir().unwrap();
        let absent = dir
            .path()
            .join("absent.sock")
            .to_string_lossy()
            .into_owned();
        let (script, _log) = fake_cmux_script(&dir);

        let adapter = make_adapter_from_with_cli(&absent, script);
        let ws_id = adapter.ensure_workspace("proj-x", "slug", "/repo").unwrap();

        assert_eq!(ws_id, "ws-cli");
    }

    #[test]
    fn cli_fallback_returns_headless_when_cli_rpc_fails() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("cmux");
        fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then exit 0; fi\nexit 42\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        let absent = dir
            .path()
            .join("absent.sock")
            .to_string_lossy()
            .into_owned();

        let adapter = make_adapter_from_with_cli(&absent, script);
        let ws_id = adapter.ensure_workspace("proj-x", "slug", "/repo").unwrap();

        assert_eq!(ws_id, "headless-ws-proj-x");
    }

    #[test]
    fn cli_fallback_does_not_switch_to_headless_after_cli_success() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("cmux");
        fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then exit 0; fi\nif [ \"$2\" = \"workspace.create\" ]; then printf '{\"workspace_id\":\"ws-cli\"}\\n'; exit 0; fi\nexit 42\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        let absent = dir
            .path()
            .join("absent.sock")
            .to_string_lossy()
            .into_owned();

        let adapter = make_adapter_from_with_cli(&absent, script);
        let ws_id = adapter.ensure_workspace("proj-x", "slug", "/repo").unwrap();
        let tab_result = adapter.create_agent_tab(&sample_agent_spec());

        assert_eq!(ws_id, "ws-cli");
        assert!(
            tab_result.is_err(),
            "adapter should not report a headless tab after establishing CLI state"
        );
    }
}

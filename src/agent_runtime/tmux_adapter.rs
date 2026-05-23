use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::error::CcxError;

pub fn session_name(session_id: &str) -> String {
    format!("ccx-{session_id}")
}

/// Validate env-var key/value pairs for tmux compatibility.
///
/// Keys must be non-empty and must not contain `=` (tmux splits on the first `=`
/// when parsing `-e KEY=VALUE` arguments). Values must not contain NUL bytes
/// (forbidden in POSIX env var values).
pub fn validate_env_vars(envs: &HashMap<String, String>) -> Result<(), CcxError> {
    for (k, v) in envs {
        if k.is_empty() {
            return Err(CcxError::Other(anyhow::anyhow!("env var key must not be empty")));
        }
        if k.contains('=') {
            return Err(CcxError::Other(anyhow::anyhow!(
                "env var key contains '=': {k:?}"
            )));
        }
        if k.contains('\0') {
            return Err(CcxError::Other(anyhow::anyhow!(
                "env var key {k:?} contains NUL byte"
            )));
        }
        if v.contains('\0') {
            return Err(CcxError::Other(anyhow::anyhow!(
                "env var value for {k:?} contains NUL byte"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

pub trait TmuxAdapter: Send + Sync {
    fn create_session(
        &self,
        session_id: &str,
        cwd: &Path,
        envs: &HashMap<String, String>,
    ) -> Result<(), CcxError>;

    fn kill_session(&self, session_id: &str) -> Result<(), CcxError>;

    fn session_exists(&self, session_id: &str) -> Result<bool, CcxError>;

    fn get_pane_pid(&self, session_id: &str) -> Result<Option<u32>, CcxError>;

    /// Returns the current working directory of the pane using tmux's
    /// `#{pane_current_path}` format. Works on macOS where /proc is absent.
    fn get_pane_cwd(&self, session_id: &str) -> Result<Option<String>, CcxError>;

    /// Send a tmux key sequence to the pane (e.g. `"Enter"`, `"C-c"`).
    fn send_keys(&self, session_id: &str, keys: &str) -> Result<(), CcxError>;

    /// Send literal text to the pane without key-name expansion (`-l` flag).
    fn send_literal(&self, session_id: &str, text: &str) -> Result<(), CcxError>;
}

// ---------------------------------------------------------------------------
// ShellTmuxAdapter — executes real tmux subprocesses
// ---------------------------------------------------------------------------

pub struct ShellTmuxAdapter;

impl TmuxAdapter for ShellTmuxAdapter {
    fn create_session(
        &self,
        session_id: &str,
        cwd: &Path,
        envs: &HashMap<String, String>,
    ) -> Result<(), CcxError> {
        validate_env_vars(envs)?;
        let name = session_name(session_id);
        let mut cmd = Command::new("tmux");
        cmd.args(["new-session", "-d", "-s", &name, "-c"]);
        cmd.arg(cwd);
        for (k, v) in envs {
            cmd.arg("-e");
            cmd.arg(format!("{k}={v}"));
        }
        let output = cmd.output()?;
        if !output.status.success() {
            return Err(CcxError::Other(anyhow::anyhow!(
                "tmux new-session failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    fn kill_session(&self, session_id: &str) -> Result<(), CcxError> {
        let name = session_name(session_id);
        let output = Command::new("tmux")
            .args(["kill-session", "-t", &name])
            .output()?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Treat "already gone" as success — caller only needs the session absent.
        if stderr.contains("can't find session") || stderr.contains("no server running") {
            return Ok(());
        }
        Err(CcxError::Other(anyhow::anyhow!(
            "tmux kill-session failed: {}",
            stderr.trim()
        )))
    }

    fn session_exists(&self, session_id: &str) -> Result<bool, CcxError> {
        let name = session_name(session_id);
        let output = Command::new("tmux")
            .args(["has-session", "-t", &name])
            .output()?;
        Ok(output.status.success())
    }

    fn get_pane_pid(&self, session_id: &str) -> Result<Option<u32>, CcxError> {
        let name = session_name(session_id);
        let output = Command::new("tmux")
            .args(["display-message", "-p", "-F", "#{pane_pid}", "-t", &name])
            .output()?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().parse::<u32>().ok())
    }

    fn get_pane_cwd(&self, session_id: &str) -> Result<Option<String>, CcxError> {
        let name = session_name(session_id);
        let output = Command::new("tmux")
            .args(["display-message", "-p", "-F", "#{pane_current_path}", "-t", &name])
            .output()?;
        if !output.status.success() {
            return Ok(None);
        }
        let s = String::from_utf8_lossy(&output.stdout);
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        Ok(Some(s.to_string()))
    }

    fn send_keys(&self, session_id: &str, keys: &str) -> Result<(), CcxError> {
        let name = session_name(session_id);
        let output = Command::new("tmux")
            .args(["send-keys", "-t", &name, keys])
            .output()?;
        if !output.status.success() {
            return Err(CcxError::Other(anyhow::anyhow!(
                "tmux send-keys failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    fn send_literal(&self, session_id: &str, text: &str) -> Result<(), CcxError> {
        let name = session_name(session_id);
        let output = Command::new("tmux")
            .args(["send-keys", "-l", "-t", &name, text])
            .output()?;
        if !output.status.success() {
            return Err(CcxError::Other(anyhow::anyhow!(
                "tmux send-keys -l failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_format() {
        assert_eq!(session_name("abc123"), "ccx-abc123");
        assert_eq!(
            session_name("01JTEST00000000000000000001"),
            "ccx-01JTEST00000000000000000001"
        );
    }

    #[test]
    fn validate_accepts_valid_envs() {
        let mut envs = HashMap::new();
        envs.insert("CCX_PROJECT_ID".into(), "proj-1".into());
        envs.insert("CCX_ROLE".into(), "worker".into());
        assert!(validate_env_vars(&envs).is_ok());
    }

    #[test]
    fn validate_rejects_empty_key() {
        let mut envs = HashMap::new();
        envs.insert(String::new(), "value".into());
        assert!(validate_env_vars(&envs).is_err());
    }

    #[test]
    fn validate_rejects_key_with_equals() {
        let mut envs = HashMap::new();
        envs.insert("CCX_KEY=BAD".into(), "value".into());
        let err = validate_env_vars(&envs).unwrap_err().to_string();
        assert!(err.contains('='), "error should mention '=': {err}");
    }

    #[test]
    fn validate_rejects_nul_in_key() {
        let mut envs = HashMap::new();
        envs.insert("CCX_KEY\0BAD".into(), "value".into());
        let err = validate_env_vars(&envs).unwrap_err().to_string();
        assert!(err.contains("NUL"), "error should mention NUL: {err}");
    }

    #[test]
    fn validate_rejects_nul_in_value() {
        let mut envs = HashMap::new();
        envs.insert("CCX_KEY".into(), "val\0ue".into());
        let err = validate_env_vars(&envs).unwrap_err().to_string();
        assert!(err.contains("NUL"), "error should mention NUL: {err}");
    }
}

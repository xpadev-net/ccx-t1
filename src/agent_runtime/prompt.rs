use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::agent_runtime::tmux_adapter::session_target;
use crate::error::CcxError;

pub enum PromptSource {
    Text(String),
    File(PathBuf),
    Stdin,
}

pub fn read_message(source: &PromptSource) -> Result<String, CcxError> {
    match source {
        PromptSource::Text(s) => Ok(s.clone()),
        PromptSource::File(path) => std::fs::read_to_string(path).map_err(|e| {
            CcxError::Other(anyhow::anyhow!("failed to read prompt file {:?}: {e}", path))
        }),
        PromptSource::Stdin => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).map_err(|e| {
                CcxError::Other(anyhow::anyhow!("failed to read stdin: {e}"))
            })?;
            Ok(buf)
        }
    }
}

/// Inject `text` into the tmux session identified by `session_id`.
///
/// Uses a named buffer (`ccx-prompt-{session_id}`) via `load-buffer` + `paste-buffer`
/// so multi-line text is delivered verbatim without tmux key-name expansion.
pub fn send_to_tmux(session_id: &str, text: &str) -> Result<(), CcxError> {
    let buffer = format!("ccx-prompt-{session_id}");
    let target = session_target(session_id);

    // Stage 1: load text into a named tmux buffer via stdin.
    // Pipe stderr so error output is captured for diagnostics.
    let mut load = Command::new("tmux")
        .args(["load-buffer", "-b", &buffer, "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CcxError::Other(anyhow::anyhow!("failed to spawn tmux load-buffer: {e}")))?;

    // Write text then drop stdin to signal EOF. Collect the write error without ?
    // so that wait_with_output() is always called (preventing a zombie child).
    let write_result: Result<(), CcxError> = match load.stdin.take() {
        None => Err(CcxError::Other(anyhow::anyhow!("tmux load-buffer stdin not available"))),
        Some(mut stdin) => stdin
            .write_all(text.as_bytes())
            .map_err(|e| CcxError::Other(anyhow::anyhow!("failed to write to tmux load-buffer: {e}"))),
    };
    // stdin handle is dropped here, signalling EOF to the child regardless of write_result.

    let output = load
        .wait_with_output()
        .map_err(|e| CcxError::Other(anyhow::anyhow!("tmux load-buffer wait failed: {e}")))?;
    if let Err(e) = write_result {
        // Buffer may have been partially loaded; clean it up before returning.
        let _ = delete_buffer(&buffer);
        return Err(e);
    }
    if !output.status.success() {
        let _ = delete_buffer(&buffer);
        return Err(CcxError::Other(anyhow::anyhow!(
            "tmux load-buffer failed (exit {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    // Stage 2: paste the buffer into the target pane.
    let paste = match Command::new("tmux")
        .args(["paste-buffer", "-b", &buffer, "-t", &target])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            let _ = delete_buffer(&buffer);
            return Err(CcxError::Other(anyhow::anyhow!(
                "failed to run tmux paste-buffer: {e}"
            )));
        }
    };
    if !paste.status.success() {
        let _ = delete_buffer(&buffer);
        return Err(CcxError::Other(anyhow::anyhow!(
            "tmux paste-buffer failed: {}",
            String::from_utf8_lossy(&paste.stderr).trim()
        )));
    }

    // Stage 3: clean up the named buffer (best-effort).
    let _ = delete_buffer(&buffer);
    Ok(())
}

fn delete_buffer(buffer: &str) -> Result<(), CcxError> {
    let output = Command::new("tmux")
        .args(["delete-buffer", "-b", buffer])
        .output()
        .map_err(|e| CcxError::Other(anyhow::anyhow!("tmux delete-buffer failed: {e}")))?;
    if !output.status.success() {
        return Err(CcxError::Other(anyhow::anyhow!(
            "tmux delete-buffer failed (exit {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn read_message_text() {
        let source = PromptSource::Text("hello world".into());
        assert_eq!(read_message(&source).unwrap(), "hello world");
    }

    #[test]
    fn read_message_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"file content\n").unwrap();
        let source = PromptSource::File(f.path().to_path_buf());
        assert_eq!(read_message(&source).unwrap(), "file content\n");
    }

    #[test]
    fn read_message_file_not_found() {
        let source = PromptSource::File(PathBuf::from("/does/not/exist/prompt.txt"));
        let err = read_message(&source).unwrap_err().to_string();
        assert!(
            err.contains("failed to read prompt file"),
            "unexpected error: {err}"
        );
    }
}

use std::path::Path;
use std::process::Command;

use crate::error::CcxError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewHookOutcome {
    Pass,
    Issues { stderr: String },
    Failure { exit_code: i32, stderr: String },
}

/// Map a gh-review-hook exit code to a ReviewHookOutcome.
/// 0 = pass, 2 = issues found, anything else = hook failure.
pub fn classify_hook_exit(exit_code: i32, stderr: String) -> ReviewHookOutcome {
    match exit_code {
        0 => ReviewHookOutcome::Pass,
        2 => ReviewHookOutcome::Issues { stderr },
        other => ReviewHookOutcome::Failure { exit_code: other, stderr },
    }
}

/// Run `gh-review-hook` in the given worktree directory and return the outcome.
pub fn run_review_hook(worktree_cwd: &Path) -> Result<ReviewHookOutcome, CcxError> {
    let output = Command::new("gh-review-hook")
        .current_dir(worktree_cwd)
        .output()
        .map_err(|e| CcxError::Other(anyhow::anyhow!("failed to run gh-review-hook: {e}")))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    Ok(classify_hook_exit(exit_code, stderr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_0_is_pass() {
        assert_eq!(classify_hook_exit(0, String::new()), ReviewHookOutcome::Pass);
    }

    #[test]
    fn exit_2_is_issues() {
        assert_eq!(
            classify_hook_exit(2, "some issues".into()),
            ReviewHookOutcome::Issues { stderr: "some issues".into() }
        );
    }

    #[test]
    fn exit_1_is_failure() {
        assert_eq!(
            classify_hook_exit(1, "internal error".into()),
            ReviewHookOutcome::Failure { exit_code: 1, stderr: "internal error".into() }
        );
    }

    #[test]
    fn exit_negative_is_failure() {
        assert_eq!(
            classify_hook_exit(-1, "signal".into()),
            ReviewHookOutcome::Failure { exit_code: -1, stderr: "signal".into() }
        );
    }

    // ── Level 3: fake external command tests ─────────────────────────────────

    // Tests that manipulate PATH must not run concurrently.
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn write_fake_hook(dir: &std::path::Path, exit_code: u8) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("gh-review-hook");
        std::fs::write(&path, format!("#!/bin/sh\nexit {exit_code}\n")).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
    }

    struct PathGuard(String);
    impl Drop for PathGuard {
        fn drop(&mut self) {
            unsafe { std::env::set_var("PATH", &self.0) };
        }
    }

    fn with_fake_path<F: FnOnce()>(bin_dir: &std::path::Path, f: F) {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", bin_dir.display(), original);
        let _restore = PathGuard(original);
        unsafe { std::env::set_var("PATH", &new_path) };
        f();
    }

    #[test]
    fn run_review_hook_exit_0_is_pass() {
        let tmp = tempfile::tempdir().unwrap();
        write_fake_hook(tmp.path(), 0);
        with_fake_path(tmp.path(), || {
            let result = run_review_hook(tmp.path()).unwrap();
            assert_eq!(result, ReviewHookOutcome::Pass);
        });
    }

    #[test]
    fn run_review_hook_exit_2_is_issues() {
        let tmp = tempfile::tempdir().unwrap();
        write_fake_hook(tmp.path(), 2);
        with_fake_path(tmp.path(), || {
            let result = run_review_hook(tmp.path()).unwrap();
            assert!(matches!(result, ReviewHookOutcome::Issues { .. }));
        });
    }

    #[test]
    fn run_review_hook_exit_1_is_failure() {
        let tmp = tempfile::tempdir().unwrap();
        write_fake_hook(tmp.path(), 1);
        with_fake_path(tmp.path(), || {
            let result = run_review_hook(tmp.path()).unwrap();
            assert!(matches!(result, ReviewHookOutcome::Failure { exit_code: 1, .. }));
        });
    }
}

use camino::Utf8Path;

use crate::error::CcxError;

/// Create a git worktree at `worktree_path` on a new branch `branch_name`,
/// then place an absolute symlink `.ccx-task.md` inside the worktree that
/// points to the canonical `task_md_path`.
pub fn create_worktree(
    repo: &Utf8Path,
    worktree_path: &Utf8Path,
    branch_name: &str,
    task_md_path: &Utf8Path,
) -> Result<(), CcxError> {
    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", branch_name, worktree_path.as_str()])
        .current_dir(repo)
        .output()
        .map_err(|e| CcxError::Git(format!("failed to run git worktree add: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CcxError::Git(format!(
            "git worktree add exited with {:?}: {}",
            output.status.code(),
            stderr.trim()
        )));
    }

    // Symlink must be absolute so it resolves correctly from any cwd.
    let symlink_path = worktree_path.join(".ccx-task.md");
    std::os::unix::fs::symlink(task_md_path.as_std_path(), symlink_path.as_std_path())
        .map_err(|e| CcxError::Io(e))?;

    Ok(())
}

/// Remove a git worktree and prune stale administrative files.
///
/// Passes `--force` to `git worktree remove`, which discards any uncommitted
/// changes in the worktree without error. Callers must ensure that any work
/// they want to preserve has been committed or stashed beforehand.
pub fn remove_worktree(repo: &Utf8Path, worktree_path: &Utf8Path) -> Result<(), CcxError> {
    let status = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", worktree_path.as_str()])
        .current_dir(repo)
        .status()
        .map_err(|e| CcxError::Git(format!("failed to run git worktree remove: {e}")))?;

    if !status.success() {
        return Err(CcxError::Git(format!(
            "git worktree remove exited with {:?}",
            status.code()
        )));
    }

    prune_worktrees(repo)
}

/// Prune stale git worktree administrative files.
pub fn prune_worktrees(repo: &Utf8Path) -> Result<(), CcxError> {
    let prune_status = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo)
        .status()
        .map_err(|e| CcxError::Git(format!("failed to run git worktree prune: {e}")))?;

    if !prune_status.success() {
        return Err(CcxError::Git(format!(
            "git worktree prune exited with {:?}",
            prune_status.code()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_worktree_creates_branch_and_symlink() {
        let tmp_repo = tempfile::tempdir().unwrap();
        let repo = crate::git::test_helpers::init_repo(&tmp_repo);

        let tmp_wt = tempfile::tempdir().unwrap();
        let wt_path = camino::Utf8PathBuf::try_from(tmp_wt.path().join("wt")).unwrap();

        let task_md = repo.join("README");

        create_worktree(&repo, &wt_path, "ccx/test-branch", &task_md).unwrap();

        assert!(wt_path.exists(), "worktree dir should exist");
        assert!(
            wt_path.join(".ccx-task.md").exists(),
            "symlink should exist"
        );

        let output = std::process::Command::new("git")
            .args(["worktree", "list"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("ccx/test-branch"),
            "branch should appear in worktree list"
        );
    }

    #[test]
    fn remove_worktree_removes_directory() {
        let tmp_repo = tempfile::tempdir().unwrap();
        let repo = crate::git::test_helpers::init_repo(&tmp_repo);

        let tmp_wt = tempfile::tempdir().unwrap();
        let wt_path = camino::Utf8PathBuf::try_from(tmp_wt.path().join("wt")).unwrap();

        let task_md = repo.join("README");

        create_worktree(&repo, &wt_path, "ccx/remove-test", &task_md).unwrap();
        assert!(wt_path.exists());

        remove_worktree(&repo, &wt_path).unwrap();
        assert!(!wt_path.exists(), "worktree dir should be removed");
    }
}

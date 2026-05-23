use camino::Utf8Path;

use crate::domain::event::{Actor, BranchCreatedPayload, Event, EventData, WorktreeCreatedPayload};
use crate::error::CcxError;
use crate::persistence::jsonl::append_event_to_dir;

/// Create a git worktree at `worktree_path` on a new branch `branch_name`,
/// then place an absolute symlink `.ccx-task.md` inside the worktree that
/// points to the canonical `task_md_path`.
///
/// Appends `BranchCreated` then `WorktreeCreated` events to the project log.
pub fn create_worktree(
    repo: &Utf8Path,
    worktree_path: &Utf8Path,
    branch_name: &str,
    project_id: &str,
    work_execution_id: &str,
    project_dir: &Utf8Path,
    task_md_path: &Utf8Path,
) -> Result<(), CcxError> {
    let status = std::process::Command::new("git")
        .args(["worktree", "add", "-b", branch_name, worktree_path.as_str()])
        .current_dir(repo)
        .status()
        .map_err(|e| CcxError::Git(format!("failed to run git worktree add: {e}")))?;

    if !status.success() {
        return Err(CcxError::Git(format!(
            "git worktree add exited with {:?}",
            status.code()
        )));
    }

    // Symlink must be absolute so it resolves correctly from any cwd.
    let symlink_path = worktree_path.join(".ccx-task.md");
    std::os::unix::fs::symlink(task_md_path.as_std_path(), symlink_path.as_std_path())
        .map_err(|e| CcxError::Io(e))?;

    let branch_event = Event::new(
        project_id,
        Actor::Controller,
        EventData::BranchCreated(BranchCreatedPayload {
            work_execution_id: work_execution_id.to_string(),
            branch_name: branch_name.to_string(),
        }),
    );
    append_event_to_dir(project_dir, &branch_event)?;

    let worktree_event = Event::new(
        project_id,
        Actor::Controller,
        EventData::WorktreeCreated(WorktreeCreatedPayload {
            work_execution_id: work_execution_id.to_string(),
            worktree_path: worktree_path.to_string(),
            branch_name: branch_name.to_string(),
        }),
    );
    append_event_to_dir(project_dir, &worktree_event)?;

    Ok(())
}

/// Remove a git worktree and prune stale administrative files.
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

    fn init_repo(dir: &camino::Utf8Path) {
        for cmd in [
            vec!["init"],
            vec!["config", "user.email", "test@test.com"],
            vec!["config", "user.name", "Test"],
        ] {
            std::process::Command::new("git")
                .args(&cmd)
                .current_dir(dir)
                .output()
                .unwrap();
        }
        std::fs::write(dir.join("README"), b"init").unwrap();
        for cmd in [vec!["add", "."], vec!["commit", "-m", "init"]] {
            std::process::Command::new("git")
                .args(&cmd)
                .current_dir(dir)
                .output()
                .unwrap();
        }
    }

    #[test]
    fn create_worktree_creates_branch_and_symlink() {
        let tmp_repo = tempfile::tempdir().unwrap();
        let repo = camino::Utf8PathBuf::try_from(tmp_repo.path().to_path_buf()).unwrap();
        init_repo(&repo);

        let tmp_wt = tempfile::tempdir().unwrap();
        let wt_path = camino::Utf8PathBuf::try_from(tmp_wt.path().join("wt")).unwrap();

        let tmp_proj = tempfile::tempdir().unwrap();
        let proj_dir = camino::Utf8PathBuf::try_from(tmp_proj.path().to_path_buf()).unwrap();

        let task_md = repo.join("README");
        let pid = "01JTEST00000000000000000001";
        let we_id = "01JTEST00000000000000000002";

        create_worktree(&repo, &wt_path, "ccx/test-branch", pid, we_id, &proj_dir, &task_md)
            .unwrap();

        assert!(wt_path.exists(), "worktree dir should exist");
        assert!(wt_path.join(".ccx-task.md").exists(), "symlink should exist");

        let output = std::process::Command::new("git")
            .args(["worktree", "list"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("ccx/test-branch"), "branch should appear in worktree list");
    }

    #[test]
    fn remove_worktree_removes_directory() {
        let tmp_repo = tempfile::tempdir().unwrap();
        let repo = camino::Utf8PathBuf::try_from(tmp_repo.path().to_path_buf()).unwrap();
        init_repo(&repo);

        let tmp_wt = tempfile::tempdir().unwrap();
        let wt_path = camino::Utf8PathBuf::try_from(tmp_wt.path().join("wt")).unwrap();

        let tmp_proj = tempfile::tempdir().unwrap();
        let proj_dir = camino::Utf8PathBuf::try_from(tmp_proj.path().to_path_buf()).unwrap();
        let task_md = repo.join("README");

        create_worktree(&repo, &wt_path, "ccx/remove-test", "pid", "we_id", &proj_dir, &task_md)
            .unwrap();
        assert!(wt_path.exists());

        remove_worktree(&repo, &wt_path).unwrap();
        assert!(!wt_path.exists(), "worktree dir should be removed");
    }
}

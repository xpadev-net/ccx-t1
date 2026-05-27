use std::process::Command;

use camino::Utf8PathBuf;
use rusqlite::Connection;
use serde::Deserialize;

use crate::domain::event::{
    generate_id, Actor, CanonicalSyncCompletedPayload, CanonicalSyncFailedPayload, Event,
    EventData, GhReviewHookCompletedPayload, GhReviewHookStartedPayload, MergeCompletedPayload,
    MergeFailedPayload, MergeLockAcquiredPayload, MergeStartedPayload,
    WorkExecutionStateChangedPayload,
};
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;
use crate::git::repo::{check_dirty, DirtyEntry};
use crate::git::review_hook::{run_review_hook, ReviewHookOutcome};
use crate::persistence::jsonl::{append_event_to_dir, locked_read_write};
use crate::persistence::sqlite::open_db;

// ---------------------------------------------------------------------------
// Pure data types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrViewData {
    pub state: String,
    pub mergeable: String,
    pub head_ref_oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// Sync completed cleanly; warning present only when task source file changed.
    Success { warning: Option<String> },
    /// Other tracked files in canonical repo were dirty — sync aborted.
    Aborted { reason: String },
}

// ---------------------------------------------------------------------------
// Pure functions (testable without I/O)
// ---------------------------------------------------------------------------

/// Parse `gh pr view --json state,mergeable,headRefOid` stdout.
pub fn parse_pr_view_json(stdout: &str) -> Result<PrViewData, CcxError> {
    serde_json::from_str(stdout)
        .map_err(|e| CcxError::Other(anyhow::anyhow!("failed to parse gh pr view output: {e}")))
}

/// Classify `git status --porcelain` entries after a canonical sync.
///
/// If files other than `task_source_relpath` are dirty, returns `Aborted`.
/// If only the task source file is dirty, returns `Success` with a warning.
/// If nothing is dirty, returns `Success` without a warning.
pub fn classify_dirty_entries(entries: &[DirtyEntry], task_source_relpath: &str) -> SyncOutcome {
    let other_dirty: Vec<&str> = entries
        .iter()
        .filter(|e| e.path != task_source_relpath)
        .map(|e| e.path.as_str())
        .collect();

    if !other_dirty.is_empty() {
        return SyncOutcome::Aborted {
            reason: format!("dirty files in canonical repo: {}", other_dirty.join(", ")),
        };
    }

    let task_source_dirty = entries.iter().any(|e| e.path == task_source_relpath);
    SyncOutcome::Success {
        warning: task_source_dirty
            .then(|| format!("task source file has changes: {task_source_relpath}")),
    }
}

// ---------------------------------------------------------------------------
// Merge orchestration
// ---------------------------------------------------------------------------

pub struct MergeConfig {
    pub project_id: String,
    pub project_dir: Utf8PathBuf,
    pub work_execution_id: String,
    pub owner_agent_session_id: String,
}

pub struct MergeOutcome {
    pub pr_number: u64,
    /// "success" or "aborted"
    pub sync_status: String,
    pub sync_warning: Option<String>,
}

pub fn execute_merge(config: &MergeConfig) -> Result<MergeOutcome, CcxError> {
    let conn = open_db(&config.project_dir)?;

    // Query work execution
    let (we_state, pr_number_opt, worktree_path, head_commit): (
        String,
        Option<i64>,
        String,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT state, pr_number, worktree_path, head_commit \
             FROM work_executions WHERE work_execution_id = ?1",
            rusqlite::params![config.work_execution_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| {
            if e == rusqlite::Error::QueryReturnedNoRows {
                CcxError::Other(anyhow::anyhow!(
                    "work execution not found: {}",
                    config.work_execution_id
                ))
            } else {
                CcxError::Other(anyhow::anyhow!("failed to query work execution: {e}"))
            }
        })?;

    if we_state != "merge_ready" {
        return Err(CcxError::Other(anyhow::anyhow!(
            "work execution {} is in state '{}', expected 'merge_ready'",
            config.work_execution_id,
            we_state
        )));
    }

    let pr_number = pr_number_opt
        .ok_or_else(|| {
            CcxError::Other(anyhow::anyhow!(
                "work execution {} has no PR number",
                config.work_execution_id
            ))
        })?
        as u64;

    // Query project
    let (canonical_repo, task_source_file): (String, String) = conn
        .query_row(
            "SELECT canonical_repo, task_source_file FROM projects WHERE project_id = ?1",
            rusqlite::params![config.project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| {
            if e == rusqlite::Error::QueryReturnedNoRows {
                CcxError::Other(anyhow::anyhow!("project not found: {}", config.project_id))
            } else {
                CcxError::Other(anyhow::anyhow!("failed to query project: {e}"))
            }
        })?;

    // Acquire merge lock (INSERT; UNIQUE index prevents concurrent merges per project)
    let merge_lock_id = generate_id().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let insert_result = conn.execute(
        "INSERT INTO merge_locks \
         (merge_lock_id, project_id, owner_agent_session_id, work_execution_id, \
          pr_number, acquired_at, last_heartbeat_at, state) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 'active')",
        rusqlite::params![
            merge_lock_id,
            config.project_id,
            config.owner_agent_session_id,
            config.work_execution_id,
            pr_number as i64,
            now,
        ],
    );

    match insert_result {
        Err(rusqlite::Error::SqliteFailure(ref e, _))
            if e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            let holder: String = conn
                .query_row(
                    "SELECT owner_agent_session_id FROM merge_locks \
                     WHERE project_id = ?1 AND state = 'active'",
                    rusqlite::params![config.project_id],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| "unknown".into());
            return Err(CcxError::Other(anyhow::anyhow!(
                "merge lock held by {holder}"
            )));
        }
        Err(e) => {
            return Err(CcxError::Other(anyhow::anyhow!(
                "failed to acquire merge lock: {e}"
            )));
        }
        Ok(_) => {}
    }

    // Emit MergeLockAcquired
    append_event_to_dir(
        &config.project_dir,
        &Event::new(
            &config.project_id,
            Actor::Controller,
            EventData::MergeLockAcquired(MergeLockAcquiredPayload {
                merge_lock_id: merge_lock_id.clone(),
                work_execution_id: config.work_execution_id.clone(),
                owner_agent_session_id: config.owner_agent_session_id.clone(),
                pr_number,
            }),
        ),
    )?;

    // Run gh-review-hook
    append_event_to_dir(
        &config.project_dir,
        &Event::new(
            &config.project_id,
            Actor::Controller,
            EventData::GhReviewHookStarted(GhReviewHookStartedPayload {
                work_execution_id: config.work_execution_id.clone(),
            }),
        ),
    )?;

    let hook_result = run_review_hook(std::path::Path::new(&worktree_path));
    let hook_exit_code = match &hook_result {
        Ok(ReviewHookOutcome::Pass) => 0,
        Ok(ReviewHookOutcome::Issues { .. }) => 2,
        Ok(ReviewHookOutcome::Failure { exit_code, .. }) => *exit_code,
        Err(_) => -1,
    };

    append_event_to_dir(
        &config.project_dir,
        &Event::new(
            &config.project_id,
            Actor::Controller,
            EventData::GhReviewHookCompleted(GhReviewHookCompletedPayload {
                work_execution_id: config.work_execution_id.clone(),
                exit_code: hook_exit_code,
            }),
        ),
    )?;

    match hook_result {
        Err(e) => {
            let reason = format!("gh-review-hook error: {e}");
            abort_merge(
                &conn,
                &config.project_dir,
                &config.project_id,
                &config.work_execution_id,
                &merge_lock_id,
                pr_number,
                &reason,
                false,
            )?;
            return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
        }
        Ok(ReviewHookOutcome::Issues { stderr }) => {
            let reason = format!("gh-review-hook reported issues: {stderr}");
            abort_merge(
                &conn,
                &config.project_dir,
                &config.project_id,
                &config.work_execution_id,
                &merge_lock_id,
                pr_number,
                &reason,
                false,
            )?;
            return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
        }
        Ok(ReviewHookOutcome::Failure { exit_code, stderr }) => {
            let reason = format!("gh-review-hook failed (exit {exit_code}): {stderr}");
            abort_merge(
                &conn,
                &config.project_dir,
                &config.project_id,
                &config.work_execution_id,
                &merge_lock_id,
                pr_number,
                &reason,
                false,
            )?;
            return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
        }
        Ok(ReviewHookOutcome::Pass) => {}
    }

    // Check PR state via gh pr view
    let pr_view_output = Command::new("gh")
        .args(["pr", "view", &pr_number.to_string(), "--json", "state,mergeable,headRefOid"])
        .current_dir(&canonical_repo)
        .output()
        .map_err(|e| {
            CcxError::Other(anyhow::anyhow!("failed to run gh pr view: {e}"))
        });

    match pr_view_output {
        Err(e) => {
            abort_merge(
                &conn,
                &config.project_dir,
                &config.project_id,
                &config.work_execution_id,
                &merge_lock_id,
                pr_number,
                &e.to_string(),
                false,
            )?;
            return Err(e);
        }
        Ok(output) if !output.status.success() => {
            let reason = format!(
                "gh pr view failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            abort_merge(
                &conn,
                &config.project_dir,
                &config.project_id,
                &config.work_execution_id,
                &merge_lock_id,
                pr_number,
                &reason,
                false,
            )?;
            return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
        }
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            match parse_pr_view_json(&stdout) {
                Err(e) => {
                    let reason = format!("failed to parse PR view: {e}");
                    abort_merge(
                        &conn,
                        &config.project_dir,
                        &config.project_id,
                        &config.work_execution_id,
                        &merge_lock_id,
                        pr_number,
                        &reason,
                        false,
                    )?;
                    return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
                }
                Ok(pr) if pr.state != "OPEN" => {
                    let reason = format!("PR #{pr_number} is not open (state={})", pr.state);
                    abort_merge(
                        &conn,
                        &config.project_dir,
                        &config.project_id,
                        &config.work_execution_id,
                        &merge_lock_id,
                        pr_number,
                        &reason,
                        false,
                    )?;
                    return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
                }
                Ok(pr) if pr.mergeable == "CONFLICTING" => {
                    let reason = format!("PR #{pr_number} has merge conflicts");
                    abort_merge(
                        &conn,
                        &config.project_dir,
                        &config.project_id,
                        &config.work_execution_id,
                        &merge_lock_id,
                        pr_number,
                        &reason,
                        false,
                    )?;
                    return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
                }
                Ok(pr) => {
                    // Validate that the PR head hasn't shifted since we recorded it.
                    if let Some(ref expected) = head_commit {
                        if pr.head_ref_oid != *expected {
                            let reason = format!(
                                "PR head shifted: expected {expected}, got {}",
                                pr.head_ref_oid
                            );
                            abort_merge(
                                &conn,
                                &config.project_dir,
                                &config.project_id,
                                &config.work_execution_id,
                                &merge_lock_id,
                                pr_number,
                                &reason,
                                false,
                            )?;
                            return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
                        }
                    }
                }
            }
        }
    }

    // Transition: MergeReady → Merging
    append_event_to_dir(
        &config.project_dir,
        &Event::new(
            &config.project_id,
            Actor::Controller,
            EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
                work_execution_id: config.work_execution_id.clone(),
                from: WorkExecutionState::MergeReady,
                to: WorkExecutionState::Merging,
            }),
        ),
    )?;

    // MergeStarted (audit)
    append_event_to_dir(
        &config.project_dir,
        &Event::new(
            &config.project_id,
            Actor::Controller,
            EventData::MergeStarted(MergeStartedPayload {
                merge_lock_id: merge_lock_id.clone(),
                work_execution_id: config.work_execution_id.clone(),
                pr_number,
            }),
        ),
    )?;

    // Execute: gh pr merge
    let merge_output = Command::new("gh")
        .args(["pr", "merge", &pr_number.to_string(), "--squash", "--delete-branch"])
        .current_dir(&canonical_repo)
        .output()
        .map_err(|e| CcxError::Other(anyhow::anyhow!("failed to run gh pr merge: {e}")));

    match merge_output {
        Err(e) => {
            abort_merge(
                &conn,
                &config.project_dir,
                &config.project_id,
                &config.work_execution_id,
                &merge_lock_id,
                pr_number,
                &e.to_string(),
                true,
            )?;
            return Err(e);
        }
        Ok(output) if !output.status.success() => {
            let reason = format!(
                "gh pr merge failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            abort_merge(
                &conn,
                &config.project_dir,
                &config.project_id,
                &config.work_execution_id,
                &merge_lock_id,
                pr_number,
                &reason,
                true,
            )?;
            return Err(CcxError::Other(anyhow::anyhow!("{reason}")));
        }
        Ok(_) => {}
    }

    // Transition: Merging → Merged (BEFORE canonical sync)
    append_event_to_dir(
        &config.project_dir,
        &Event::new(
            &config.project_id,
            Actor::Controller,
            EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
                work_execution_id: config.work_execution_id.clone(),
                from: WorkExecutionState::Merging,
                to: WorkExecutionState::Merged,
            }),
        ),
    )?;

    // MergeCompleted (releases lock conceptually)
    append_event_to_dir(
        &config.project_dir,
        &Event::new(
            &config.project_id,
            Actor::Controller,
            EventData::MergeCompleted(MergeCompletedPayload {
                merge_lock_id: merge_lock_id.clone(),
                work_execution_id: config.work_execution_id.clone(),
                pr_number,
            }),
        ),
    )?;

    // Release lock in DB
    let _ = conn.execute(
        "UPDATE merge_locks SET state = 'released' WHERE merge_lock_id = ?1",
        rusqlite::params![merge_lock_id],
    );

    // Canonical sync
    let (sync_status, sync_warning) = run_canonical_sync(
        &config.project_dir,
        &config.project_id,
        &config.work_execution_id,
        &canonical_repo,
        &task_source_file,
    );

    Ok(MergeOutcome {
        pr_number,
        sync_status,
        sync_warning,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Emit MergeFailed and release the merge lock. Called on every abort path.
fn abort_merge(
    conn: &Connection,
    project_dir: &camino::Utf8Path,
    project_id: &str,
    work_execution_id: &str,
    merge_lock_id: &str,
    pr_number: u64,
    reason: &str,
    transition_to_failed: bool,
) -> Result<(), CcxError> {
    let mut transition_error: Option<CcxError> = None;

    if transition_to_failed {
        transition_error = emit_merging_failed_transition(project_dir, project_id, work_execution_id).err();
    }

    let append_result = append_event_to_dir(
        project_dir,
        &Event::new(
            project_id,
            Actor::Controller,
            EventData::MergeFailed(MergeFailedPayload {
                merge_lock_id: merge_lock_id.to_string(),
                work_execution_id: work_execution_id.to_string(),
                pr_number,
                reason: reason.to_string(),
            }),
        ),
    );

    // Best-effort lock release — don't mask the event error
    let _ = conn.execute(
        "UPDATE merge_locks SET state = 'released' WHERE merge_lock_id = ?1",
        rusqlite::params![merge_lock_id],
    );

    if let Err(error) = append_result {
        return Err(error);
    }

    if let Some(error) = transition_error {
        return Err(error);
    }

    Ok(())
}

fn emit_merging_failed_transition(
    project_dir: &camino::Utf8Path,
    project_id: &str,
    work_execution_id: &str,
) -> Result<(), CcxError> {
    locked_read_write(project_dir, |events| {
        let in_merging_state = events.iter().rev().find_map(|event| match &event.data {
            EventData::WorkExecutionStateChanged(payload)
                if payload.work_execution_id == work_execution_id =>
            {
                Some(payload.to == WorkExecutionState::Merging)
            }
            _ => None,
        });

        match in_merging_state {
            Some(true) => Ok(Some(Event::new(
                project_id,
                Actor::Controller,
                EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
                    work_execution_id: work_execution_id.to_string(),
                    from: WorkExecutionState::Merging,
                    to: WorkExecutionState::Failed,
                }),
            ))),
            _ => Ok(None),
        }
    })?;

    Ok(())
}
/// Pull the canonical repo, check dirty state, and emit sync events.
/// Returns (sync_status, sync_warning) — never returns Err (sync failure
/// is recorded as an event; the merge itself already succeeded).
fn run_canonical_sync(
    project_dir: &camino::Utf8Path,
    project_id: &str,
    work_execution_id: &str,
    canonical_repo: &str,
    task_source_file: &str,
) -> (String, Option<String>) {
    let pull_output = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(canonical_repo)
        .output();

    match pull_output {
        Err(e) => {
            let reason = format!("git pull failed: {e}");
            emit_sync_failed(project_dir, project_id, work_execution_id, &reason);
            return ("aborted".into(), Some(reason));
        }
        Ok(output) if !output.status.success() => {
            let reason = format!(
                "git pull failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            emit_sync_failed(project_dir, project_id, work_execution_id, &reason);
            return ("aborted".into(), Some(reason));
        }
        Ok(_) => {}
    }

    // Compute task_source relative path for dirty-entry classification
    let repo_path = std::path::Path::new(canonical_repo);
    let src_path = std::path::Path::new(task_source_file);
    let task_source_relpath = src_path
        .strip_prefix(repo_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| task_source_file.to_string());

    let entries = match check_dirty(camino::Utf8Path::new(canonical_repo)) {
        Err(e) => {
            let reason = format!("git status failed: {e}");
            emit_sync_failed(project_dir, project_id, work_execution_id, &reason);
            return ("aborted".into(), Some(reason));
        }
        Ok(v) => v,
    };

    let outcome = classify_dirty_entries(&entries, &task_source_relpath);

    match outcome {
        SyncOutcome::Aborted { reason } => {
            emit_sync_failed(project_dir, project_id, work_execution_id, &reason);
            ("aborted".into(), Some(reason))
        }
        SyncOutcome::Success { warning } => {
            let _ = append_event_to_dir(
                project_dir,
                &Event::new(
                    project_id,
                    Actor::Controller,
                    EventData::CanonicalSyncCompleted(CanonicalSyncCompletedPayload {
                        work_execution_id: work_execution_id.to_string(),
                        sync_warning: warning.clone(),
                    }),
                ),
            );
            ("success".into(), warning)
        }
    }
}

fn emit_sync_failed(
    project_dir: &camino::Utf8Path,
    project_id: &str,
    work_execution_id: &str,
    reason: &str,
) {
    let _ = append_event_to_dir(
        project_dir,
        &Event::new(
            project_id,
            Actor::Controller,
            EventData::CanonicalSyncFailed(CanonicalSyncFailedPayload {
                work_execution_id: work_execution_id.to_string(),
                reason: reason.to_string(),
            }),
        ),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::repo::DirtyEntry;

    fn entry(path: &str) -> DirtyEntry {
        DirtyEntry { xy: " M".into(), path: path.into() }
    }

    #[test]
    fn classify_empty_is_success() {
        assert_eq!(
            classify_dirty_entries(&[], "tasks.md"),
            SyncOutcome::Success { warning: None }
        );
    }

    #[test]
    fn classify_only_task_source_is_success_with_warning() {
        let entries = vec![entry("z/tasks.md")];
        match classify_dirty_entries(&entries, "z/tasks.md") {
            SyncOutcome::Success { warning: Some(w) } => {
                assert!(w.contains("z/tasks.md"), "warning should mention file: {w}");
            }
            other => panic!("expected Success with warning, got {other:?}"),
        }
    }

    #[test]
    fn classify_other_file_is_aborted() {
        let entries = vec![entry("src/main.rs")];
        match classify_dirty_entries(&entries, "z/tasks.md") {
            SyncOutcome::Aborted { reason } => {
                assert!(reason.contains("src/main.rs"), "reason should mention file: {reason}");
            }
            other => panic!("expected Aborted, got {other:?}"),
        }
    }

    #[test]
    fn classify_task_source_and_other_is_aborted() {
        let entries = vec![entry("z/tasks.md"), entry("src/lib.rs")];
        assert!(matches!(
            classify_dirty_entries(&entries, "z/tasks.md"),
            SyncOutcome::Aborted { .. }
        ));
    }

    #[test]
    fn parse_pr_view_json_valid() {
        let json = r#"{"state":"OPEN","mergeable":"MERGEABLE","headRefOid":"abc123"}"#;
        let pr = parse_pr_view_json(json).unwrap();
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.mergeable, "MERGEABLE");
        assert_eq!(pr.head_ref_oid, "abc123");
    }

    #[test]
    fn parse_pr_view_json_invalid() {
        let err = parse_pr_view_json("not json").unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }
}

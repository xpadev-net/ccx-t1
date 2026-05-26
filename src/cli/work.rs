use camino::Utf8Path;
use clap::Args;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::{load_project_config, project_dir};
use crate::domain::event::{
    generate_id, Actor, BranchCreatedPayload, Event, EventData, WorkExecutionCreatedPayload,
    WorkExecutionStateChangedPayload, WorkExecutionTaskFileCreatedPayload, WorktreeCreatedPayload,
};
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;
use crate::git::github::{execute_merge, MergeConfig};
use crate::git::worktree::{create_worktree, remove_worktree};
use crate::persistence::jsonl::{append_events_to_dir, EventBatchAppendError};
use crate::work::cleanup::{run_cleanup, CleanupConfig};

// ---------------------------------------------------------------------------
// work create
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub source_path: String,
    /// e.g. "section_heading", "line_range", "whole_file"
    #[arg(long)]
    pub selector_type: String,
    #[arg(long)]
    pub selector_value: String,
    #[arg(long)]
    pub display_text: String,
    #[arg(long)]
    pub json: bool,
}

pub fn create(args: CreateArgs) -> Result<(), CcxError> {
    let config = load_project_config(&args.project_id)?;
    let project_dir = project_dir(&args.project_id)?;
    let we_id = generate_id().to_string();
    let branch = format!("ccx/{we_id}/{}", branch_slug(&args.display_text));
    let worktree = project_dir.join("worktrees").join(&we_id);
    let execution_dir = project_dir.join("work-executions").join(&we_id);
    let task_file = execution_dir.join("task.md");
    let source_hash = hash_file(&args.source_path)?;
    let now = chrono::Utc::now().to_rfc3339();
    let task_md = render_task_md(&TaskMdInput {
        project_id: &args.project_id,
        work_execution_id: &we_id,
        source_path: &args.source_path,
        selector_type: &args.selector_type,
        selector_value: &args.selector_value,
        display_text: &args.display_text,
        branch: &branch,
        updated_at: &now,
    })?;
    std::fs::create_dir_all(&execution_dir)?;
    if test_fail_after_execution_dir() {
        let error = CcxError::Other(anyhow::anyhow!(
            "simulated work create failure after execution dir"
        ));
        cleanup_create_artifacts(&config.canonical_repo, &worktree, &branch, &execution_dir);
        return Err(error);
    }
    if let Err(error) = std::fs::write(task_file.as_std_path(), task_md) {
        cleanup_create_artifacts(&config.canonical_repo, &worktree, &branch, &execution_dir);
        return Err(error.into());
    }
    if let Err(error) = create_worktree(&config.canonical_repo, &worktree, &branch, &task_file) {
        cleanup_create_artifacts(&config.canonical_repo, &worktree, &branch, &execution_dir);
        return Err(error);
    }

    let created = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::WorkExecutionCreated(WorkExecutionCreatedPayload {
            work_execution_id: we_id.clone(),
            branch_name: branch.clone(),
            worktree_path: worktree.to_string(),
            task_file_path: task_file.to_string(),
            source_path: args.source_path.clone(),
            selector_type: args.selector_type.clone(),
            selector_value: args.selector_value.clone(),
            display_text: args.display_text.clone(),
            source_file_hash: source_hash.clone(),
        }),
    );
    let task_file_created = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::WorkExecutionTaskFileCreated(WorkExecutionTaskFileCreatedPayload {
            work_execution_id: we_id.clone(),
            task_file_path: task_file.to_string(),
        }),
    );
    let state_changed = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
            work_execution_id: we_id.clone(),
            from: WorkExecutionState::Created,
            to: WorkExecutionState::TaskFileCreated,
        }),
    );
    let branch_created = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::BranchCreated(BranchCreatedPayload {
            work_execution_id: we_id.clone(),
            branch_name: branch.clone(),
        }),
    );
    let worktree_created = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::WorktreeCreated(WorktreeCreatedPayload {
            work_execution_id: we_id.clone(),
            worktree_path: worktree.to_string(),
            branch_name: branch.clone(),
        }),
    );
    if let Err(error) = append_events_to_dir(
        &project_dir,
        &[
            created,
            task_file_created,
            state_changed,
            branch_created,
            worktree_created,
        ],
    ) {
        match error {
            EventBatchAppendError::RolledBack(error) => {
                cleanup_create_artifacts(
                    &config.canonical_repo,
                    &worktree,
                    &branch,
                    &execution_dir,
                );
                return Err(error);
            }
            EventBatchAppendError::Indeterminate(error) => return Err(error),
        }
    }

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "work_execution_id": we_id,
                "branch_name": branch,
                "worktree_path": worktree.to_string(),
                "task_file_path": task_file.to_string(),
            }))?
        );
    } else {
        println!("work_execution_id: {we_id}");
        println!("branch_name:       {branch}");
        println!("worktree_path:     {worktree}");
        println!("task_file_path:    {task_file}");
    }
    Ok(())
}

struct TaskMdInput<'a> {
    project_id: &'a str,
    work_execution_id: &'a str,
    source_path: &'a str,
    selector_type: &'a str,
    selector_value: &'a str,
    display_text: &'a str,
    branch: &'a str,
    updated_at: &'a str,
}

#[derive(Serialize)]
struct TaskMdFrontMatter<'a> {
    project_id: &'a str,
    work_execution_id: &'a str,
    status: &'a str,
    source_path: &'a str,
    source_ref: String,
    branch: &'a str,
    pr_number: Option<u64>,
    pr_url: Option<&'a str>,
    head_commit: Option<&'a str>,
    gh_review_hook_exit_code: Option<i32>,
    current_writer_session_id: Option<&'a str>,
    updated_by: &'a str,
    updated_at: &'a str,
}

fn render_task_md(input: &TaskMdInput<'_>) -> Result<String, CcxError> {
    let front_matter = TaskMdFrontMatter {
        project_id: input.project_id,
        work_execution_id: input.work_execution_id,
        status: "assigned",
        source_path: input.source_path,
        source_ref: format!("{}:{}", input.selector_type, input.selector_value),
        branch: input.branch,
        pr_number: None,
        pr_url: None,
        head_commit: None,
        gh_review_hook_exit_code: None,
        current_writer_session_id: None,
        updated_by: "controller",
        updated_at: input.updated_at,
    };
    let yaml = serde_yaml::to_string(&front_matter)?;
    Ok(format!(
        r#"---
{yaml}---

# Work Item

## Original Task
{display_text}

## Instructions
Implement the selected task source item within the WorkExecution boundaries.

## Progress

## Pull Request

## Review / Gate

## Result

## Remaining Work

## Blockers
"#,
        display_text = input.display_text,
    ))
}

fn branch_slug(input: &str) -> String {
    let mut slug = String::new();
    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 32 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "work".to_string()
    } else {
        slug.to_string()
    }
}

fn hash_file(path: &str) -> Result<String, CcxError> {
    let content = std::fs::read(path)
        .map_err(|e| CcxError::Other(anyhow::anyhow!("failed to read source file {path}: {e}")))?;
    Ok(format!("{:x}", Sha256::digest(&content)))
}

fn cleanup_create_artifacts(
    repo: &Utf8Path,
    worktree: &Utf8Path,
    branch: &str,
    execution_dir: &Utf8Path,
) {
    let _ = remove_worktree(repo, worktree);
    let _ = std::process::Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(repo)
        .output();
    let _ = std::fs::remove_dir_all(execution_dir);
}

fn test_fail_after_execution_dir() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var("CCX_TEST_FAIL_WORK_CREATE_AFTER_EXECUTION_DIR").is_ok()
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

// ---------------------------------------------------------------------------
// work cleanup
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct CleanupArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub work_execution_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn cleanup(args: CleanupArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let cfg = load_project_config(&args.project_id)?;

    let config = CleanupConfig {
        project_id: args.project_id.clone(),
        project_dir: dir,
        work_execution_id: args.work_execution_id.clone(),
        cleanup_policy: cfg.cleanup_policy,
        keep_last_n: cfg.keep_last_n,
        keep_for_days: cfg.keep_for_days,
        canonical_repo: cfg.canonical_repo,
    };
    let result = run_cleanup(&config)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "work_execution_id": args.work_execution_id,
                "status": "cleaned_up",
                "removed_worktree": result.removed_worktree,
                "closed_sessions": result.closed_sessions,
            }))?
        );
    } else {
        println!("work_execution_id: {}", args.work_execution_id);
        println!("removed_worktree:  {}", result.removed_worktree);
        if result.closed_sessions.is_empty() {
            println!("closed_sessions:   none");
        } else {
            println!("closed_sessions:");
            for id in &result.closed_sessions {
                println!("  - {id}");
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// merge execute  (dispatched from top-level `ccx merge execute`)
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct MergeExecuteArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub work_execution_id: String,
    #[arg(long)]
    pub owner_agent_session_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn merge_execute(args: MergeExecuteArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;

    let config = MergeConfig {
        project_id: args.project_id.clone(),
        project_dir: dir,
        work_execution_id: args.work_execution_id.clone(),
        owner_agent_session_id: args.owner_agent_session_id.clone(),
    };

    let outcome = execute_merge(&config)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "work_execution_id": args.work_execution_id,
                "status": "merged",
                "sync_status": outcome.sync_status,
                "sync_warning": outcome.sync_warning,
            }))?
        );
    } else {
        println!("merged PR #{}", outcome.pr_number);
        if let Some(w) = &outcome.sync_warning {
            println!("sync warning: {w}");
        }
    }
    Ok(())
}

use clap::Args;
use sha2::{Digest, Sha256};

use crate::config::{load_project_config, project_dir};
use crate::domain::event::{
    generate_id, Actor, Event, EventData, WorkExecutionCreatedPayload,
    WorkExecutionStateChangedPayload, WorkExecutionTaskFileCreatedPayload,
};
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;
use crate::git::github::{execute_merge, MergeConfig};
use crate::git::worktree::create_worktree;
use crate::persistence::jsonl::append_event_to_dir;
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
    std::fs::create_dir_all(&execution_dir)?;
    let now = chrono::Utc::now().to_rfc3339();
    std::fs::write(
        task_file.as_std_path(),
        render_task_md(&TaskMdInput {
            project_id: &args.project_id,
            work_execution_id: &we_id,
            source_path: &args.source_path,
            selector_type: &args.selector_type,
            selector_value: &args.selector_value,
            display_text: &args.display_text,
            branch: &branch,
            updated_at: &now,
        }),
    )?;
    create_worktree(
        &config.canonical_repo,
        &worktree,
        &branch,
        &args.project_id,
        &we_id,
        &project_dir,
        &task_file,
    )?;

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
            source_file_hash: source_hash,
        }),
    );
    append_event_to_dir(&project_dir, &created)?;
    let task_file_created = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::WorkExecutionTaskFileCreated(WorkExecutionTaskFileCreatedPayload {
            work_execution_id: we_id.clone(),
            task_file_path: task_file.to_string(),
        }),
    );
    append_event_to_dir(&project_dir, &task_file_created)?;
    let state_changed = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
            work_execution_id: we_id.clone(),
            from: WorkExecutionState::Created,
            to: WorkExecutionState::TaskFileCreated,
        }),
    );
    append_event_to_dir(&project_dir, &state_changed)?;

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

fn render_task_md(input: &TaskMdInput<'_>) -> String {
    format!(
        r#"---
project_id: {project_id}
work_execution_id: {work_execution_id}
status: assigned
source_path: {source_path}
source_ref: {selector_type}:{selector_value}
branch: {branch}
pr_number:
pr_url:
head_commit:
gh_review_hook_exit_code:
current_writer_session_id:
updated_by: orchestrator
updated_at: {updated_at}
---

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
        project_id = input.project_id,
        work_execution_id = input.work_execution_id,
        source_path = input.source_path,
        selector_type = input.selector_type,
        selector_value = input.selector_value,
        branch = input.branch,
        updated_at = input.updated_at,
        display_text = input.display_text,
    )
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

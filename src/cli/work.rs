use clap::Args;

use crate::config::project_dir;
use crate::domain::event::generate_id;
use crate::error::CcxError;
use crate::git::github::{execute_merge, MergeConfig};

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
    let we_id = generate_id().to_string();
    let branch = format!("ccx/{we_id}");
    let worktree = format!("~/.ccx/projects/{}/worktrees/{we_id}", args.project_id);
    let task_file = format!(
        "~/.ccx/projects/{}/work-executions/{we_id}/task.md",
        args.project_id
    );

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "work_execution_id": we_id,
                "branch_name": branch,
                "worktree_path": worktree,
                "task_file_path": task_file,
            }))?
        );
    } else {
        println!("work_execution_id: {we_id}");
        println!("branch_name:       {branch}");
        println!("(skeleton — not yet implemented)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// work cleanup
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct CleanupArgs {
    #[arg(long)]
    pub work_execution_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn cleanup(args: CleanupArgs) -> Result<(), CcxError> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "work_execution_id": args.work_execution_id,
                "status": "cleaned_up",
                "removed_worktree": true,
                "closed_sessions": [],
            }))?
        );
    } else {
        println!("cleanup {} (skeleton — not yet implemented)", args.work_execution_id);
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

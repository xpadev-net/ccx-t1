use std::path::PathBuf;

use clap::Args;

use crate::agent_runtime::cmux_adapter::make_adapter;
use crate::agent_runtime::lifecycle::{handle_lifecycle_stop, LifecycleStopConfig};
use crate::agent_runtime::tmux_adapter::{ShellTmuxAdapter, TmuxAdapter};
use crate::config::project_dir;
use crate::domain::event::{generate_id, Actor, AgentSessionStoppedPayload, Event, EventData};
use crate::error::CcxError;
use crate::persistence::jsonl::append_event_to_dir;
use crate::persistence::sqlite::open_db;

// ---------------------------------------------------------------------------
// agent start-orchestrator
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct StartOrchestratorArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn start_orchestrator(args: StartOrchestratorArgs) -> Result<(), CcxError> {
    let session_id = generate_id().to_string();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "agent_session_id": session_id,
                "project_id": args.project_id,
                "role": "orchestrator",
                "status": "started",
            }))?
        );
    } else {
        println!("agent_session_id: {session_id}  (skeleton — not yet implemented)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent attach
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct AttachArgs {
    #[arg(long)]
    pub work_execution_id: String,
    /// worker | reviewer | diagnostic
    #[arg(long)]
    pub role: String,
    /// writer | reviewer | observer | diagnostic
    #[arg(long)]
    pub mode: String,
    #[arg(long)]
    pub json: bool,
}

pub fn attach(args: AttachArgs) -> Result<(), CcxError> {
    let session_id = generate_id().to_string();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "agent_session_id": session_id,
                "work_execution_id": args.work_execution_id,
                "role": args.role,
                "mode": args.mode,
                "status": "attached",
            }))?
        );
    } else {
        println!("agent_session_id: {session_id}  (skeleton — not yet implemented)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent prompt
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct PromptArgs {
    #[arg(long)]
    pub session_id: String,
    /// Inline message text (mutually exclusive with --message-file and --stdin)
    #[arg(long, conflicts_with_all = ["message_file", "stdin"])]
    pub message: Option<String>,
    /// Path to a file whose content is used as the prompt
    #[arg(long, conflicts_with_all = ["message", "stdin"])]
    pub message_file: Option<String>,
    /// Read prompt from stdin
    #[arg(long, conflicts_with_all = ["message", "message_file"])]
    pub stdin: bool,
    #[arg(long)]
    pub json: bool,
}

pub fn prompt(args: PromptArgs) -> Result<(), CcxError> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": args.session_id,
                "status": "sent",
            }))?
        );
    } else {
        println!("prompt sent to {} (skeleton — not yet implemented)", args.session_id);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent stop
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct StopArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn stop(args: StopArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let conn = open_db(&dir)?;

    let cmux_tab_id: String = conn
        .query_row(
            "SELECT cmux_tab_id FROM agent_sessions WHERE agent_session_id = ?1",
            rusqlite::params![args.session_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            CcxError::Other(anyhow::anyhow!(
                "agent session not found {}: {e}",
                args.session_id
            ))
        })?;

    // Write the event first so the record is durable before any destructive action.
    // If the event write fails, nothing has been destroyed and the caller can retry.
    let event = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::AgentSessionStopped(AgentSessionStoppedPayload {
            agent_session_id: args.session_id.clone(),
            exit_code: None,
        }),
    );
    append_event_to_dir(&dir, &event)?;

    let tmux = ShellTmuxAdapter;
    let kill_result = tmux.kill_session(&args.session_id);
    // close_tab is best-effort and runs unconditionally so the tab is cleaned up
    // even when kill_session returns an unexpected error.
    let cmux = make_adapter();
    let _ = cmux.close_tab(&cmux_tab_id);
    kill_result?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": args.session_id,
                "status": "stopped",
            }))?
        );
    } else {
        println!("stopped {}", args.session_id);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent notify
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct NotifyArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub message: String,
    /// info | warning
    #[arg(long, default_value = "info")]
    pub level: String,
    #[arg(long)]
    pub json: bool,
}

pub fn notify(args: NotifyArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let conn = open_db(&dir)?;

    let cmux_tab_id: String = conn
        .query_row(
            "SELECT cmux_tab_id FROM agent_sessions WHERE agent_session_id = ?1",
            rusqlite::params![args.session_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            CcxError::Other(anyhow::anyhow!(
                "agent session not found {}: {e}",
                args.session_id
            ))
        })?;

    let cmux = make_adapter();
    cmux.notify_user(&cmux_tab_id, &args.message, &args.level)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": args.session_id,
                "level": args.level,
                "status": "notified",
            }))?
        );
    } else {
        println!("[{}] {} -> {}", args.level, args.session_id, args.message);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent lifecycle-stop
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct LifecycleStopArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub agent_session_id: String,
    #[arg(long)]
    pub work_execution_id: String,
    /// Path to the task.md file for this work execution
    #[arg(long)]
    pub task_file: PathBuf,
    /// Raw agent session ID of the orchestrator to notify (optional)
    #[arg(long)]
    pub orchestrator_session_id: Option<String>,
    #[arg(long)]
    pub json: bool,
}

pub fn lifecycle_stop(args: LifecycleStopArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;

    let config = LifecycleStopConfig {
        project_id: args.project_id.clone(),
        project_dir: dir,
        agent_session_id: args.agent_session_id.clone(),
        work_execution_id: args.work_execution_id.clone(),
        task_file_path: args.task_file,
        orchestrator_session_id: args.orchestrator_session_id,
    };
    handle_lifecycle_stop(&config)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "agent_session_id": args.agent_session_id,
                "work_execution_id": args.work_execution_id,
                "status": "lifecycle_stop_recorded",
            }))?
        );
    } else {
        println!(
            "lifecycle_stop recorded for session {} work_execution {}",
            args.agent_session_id, args.work_execution_id
        );
    }
    Ok(())
}

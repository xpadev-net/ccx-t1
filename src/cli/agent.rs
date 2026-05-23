use clap::Args;

use crate::domain::event::generate_id;
use crate::error::CcxError;

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
    pub session_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn stop(args: StopArgs) -> Result<(), CcxError> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": args.session_id,
                "status": "stopped",
            }))?
        );
    } else {
        println!("stopped {} (skeleton — not yet implemented)", args.session_id);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent notify
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct NotifyArgs {
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
        println!(
            "[{}] {} -> {} (skeleton — not yet implemented)",
            args.level, args.session_id, args.message
        );
    }
    Ok(())
}

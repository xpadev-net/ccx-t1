use clap::Args;

use crate::domain::event::generate_id;
use crate::error::CcxError;

// ---------------------------------------------------------------------------
// lease acquire
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct AcquireArgs {
    #[arg(long)]
    pub work_execution_id: String,
    #[arg(long)]
    pub agent_session_id: String,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub json: bool,
}

pub fn acquire(args: AcquireArgs) -> Result<(), CcxError> {
    let lease_id = generate_id().to_string();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "write_lease_id": lease_id,
                "status": "acquired",
            }))?
        );
    } else {
        println!("write_lease_id: {lease_id}  (skeleton — not yet implemented)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// lease release
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ReleaseArgs {
    #[arg(long)]
    pub work_execution_id: String,
    #[arg(long)]
    pub agent_session_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn release(args: ReleaseArgs) -> Result<(), CcxError> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "write_lease_id": "unknown",
                "status": "released",
            }))?
        );
    } else {
        println!(
            "lease released for {} (skeleton — not yet implemented)",
            args.work_execution_id
        );
    }
    Ok(())
}

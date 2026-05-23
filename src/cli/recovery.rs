use clap::Args;

use crate::error::CcxError;

// ---------------------------------------------------------------------------
// recovery digest
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct DigestArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn digest(args: DigestArgs) -> Result<(), CcxError> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "project_id": args.project_id,
                "timestamp": timestamp,
                "diagnostics": {
                    "active_sessions": 0,
                    "stale_leases": [],
                    "orphaned_tmux_sessions": [],
                    "sqlite_dirty": false,
                },
            }))?
        );
    } else {
        println!("recovery digest for {} (skeleton — not yet implemented)", args.project_id);
    }
    Ok(())
}

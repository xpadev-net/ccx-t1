use clap::Args;

use crate::config::project_dir;
use crate::error::CcxError;
use crate::persistence::rebuild::{rebuild, verify};

#[derive(Debug, Args)]
pub struct RebuildArgs {
    #[arg(long)]
    pub project_id: String,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn run_rebuild(args: RebuildArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    rebuild(&dir, &args.project_id)?;
    println!("rebuild complete: {}", args.project_id);
    Ok(())
}

pub fn run_verify(args: VerifyArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let result = verify(&dir, &args.project_id)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "project_id": args.project_id,
                "consistent": result.consistent,
                "last_jsonl_event_id": result.last_jsonl_event_id,
                "last_applied_event_id": result.last_applied_event_id,
                "marker_present": result.marker_present,
            }))?
        );
    } else {
        println!("project_id:            {}", args.project_id);
        println!("consistent:            {}", result.consistent);
        println!(
            "last_jsonl_event_id:   {}",
            result.last_jsonl_event_id.as_deref().unwrap_or("none")
        );
        println!(
            "last_applied_event_id: {}",
            result.last_applied_event_id.as_deref().unwrap_or("none")
        );
        println!("marker_present:        {}", result.marker_present);
        if !result.consistent {
            eprintln!("warning: SQLite is out of sync — run `ccx db rebuild --project-id {}`", args.project_id);
        }
    }
    Ok(())
}

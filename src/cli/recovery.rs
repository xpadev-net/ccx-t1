use clap::Args;

use crate::breaker::evaluator::{evaluate, CircuitBreakerConfig};
use crate::config::project_dir;
use crate::error::CcxError;
use crate::recovery::digest::run_digest;

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
    let dir = project_dir(&args.project_id)?;
    let result = run_digest(&args.project_id, &dir)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("project_id:      {}", result.project_id);
        println!("timestamp:       {}", result.timestamp);
        println!("active_sessions: {}", result.diagnostics.active_sessions);
        println!("sqlite_dirty:    {}", result.diagnostics.sqlite_dirty);

        if result.diagnostics.orphaned_tmux_sessions.is_empty() {
            println!("orphaned_sessions: none");
        } else {
            println!("orphaned_sessions:");
            for id in &result.diagnostics.orphaned_tmux_sessions {
                println!("  - {id}");
            }
        }

        if result.diagnostics.stale_leases.is_empty() {
            println!("stale_leases: none");
        } else {
            println!("stale_leases:");
            for l in &result.diagnostics.stale_leases {
                println!(
                    "  - {} (we={}, stale={}s)",
                    l.write_lease_id, l.work_execution_id, l.stale_seconds
                );
            }
        }

        if result.diagnostics.stale_merge_locks.is_empty() {
            println!("stale_merge_locks: none");
        } else {
            println!("stale_merge_locks:");
            for l in &result.diagnostics.stale_merge_locks {
                println!(
                    "  - {} (we={}, stale={}s)",
                    l.merge_lock_id, l.work_execution_id, l.stale_seconds
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// recovery circuit-check
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct CircuitCheckArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub work_execution_id: String,
    /// Transition to Hold after this many Failed landings (default: 3).
    #[arg(long, default_value = "3")]
    pub max_retries: u32,
    /// Transition to Hold after this many hours since creation (default: 24).
    #[arg(long, default_value = "24")]
    pub max_hours: u64,
    #[arg(long)]
    pub json: bool,
}

pub fn circuit_check(args: CircuitCheckArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let config = CircuitBreakerConfig {
        project_id: args.project_id.clone(),
        work_execution_id: args.work_execution_id.clone(),
        max_retries: args.max_retries,
        max_hours: args.max_hours,
    };
    let result = evaluate(&config, &dir)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("work_execution_id: {}", result.work_execution_id);
        println!("triggered:         {}", result.triggered);
        println!("retry_count:       {}", result.retry_count);
        println!("elapsed_hours:     {:.1}", result.elapsed_hours);
        if let Some(reason) = &result.reason {
            println!("reason:            {reason}");
        }
    }
    Ok(())
}

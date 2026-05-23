mod agent_runtime;
mod breaker;
mod cli;
mod config;
mod domain;
mod error;
mod git;
mod persistence;
mod recovery;
mod watcher;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "ccx", about = "CCX Orchestrator Controller", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Project management
    Project {
        #[command(subcommand)]
        cmd: cli::ProjectCommand,
    },
    /// Database management
    Db {
        #[command(subcommand)]
        cmd: cli::DbCommand,
    },
    /// Work execution management
    Work {
        #[command(subcommand)]
        cmd: cli::WorkCommand,
    },
    /// Agent session management
    Agent {
        #[command(subcommand)]
        cmd: cli::AgentCommand,
    },
    /// Write lease management
    Lease {
        #[command(subcommand)]
        cmd: cli::LeaseCommand,
    },
    /// PR merge operations
    Merge {
        #[command(subcommand)]
        cmd: cli::MergeCommand,
    },
    /// Recovery and diagnostics
    Recovery {
        #[command(subcommand)]
        cmd: cli::RecoveryCommand,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        Command::Project { cmd } => cli::run_project(cmd),
        Command::Db { cmd } => cli::run_db(cmd),
        Command::Work { cmd } => cli::run_work(cmd),
        Command::Agent { cmd } => cli::run_agent(cmd),
        Command::Lease { cmd } => cli::run_lease(cmd),
        Command::Merge { cmd } => cli::run_merge(cmd),
        Command::Recovery { cmd } => cli::run_recovery(cmd),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

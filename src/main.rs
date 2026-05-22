mod cli;
mod config;
mod domain;
mod error;

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
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

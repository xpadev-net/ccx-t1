pub mod project;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Register a new project
    Register(project::RegisterArgs),
    /// List registered projects
    List(project::ListArgs),
    /// Show project status
    Status(project::StatusArgs),
}

pub fn run_project(cmd: ProjectCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        ProjectCommand::Register(args) => project::register(args),
        ProjectCommand::List(args) => project::list(args),
        ProjectCommand::Status(args) => project::status(args),
    }
}

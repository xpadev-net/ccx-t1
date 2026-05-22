pub mod db;
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

#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Rebuild the SQLite read model from the event log
    Rebuild(db::RebuildArgs),
    /// Verify that the SQLite projection is in sync with the event log
    Verify(db::VerifyArgs),
}

pub fn run_db(cmd: DbCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        DbCommand::Rebuild(args) => db::run_rebuild(args),
        DbCommand::Verify(args) => db::run_verify(args),
    }
}

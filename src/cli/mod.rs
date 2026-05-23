pub mod agent;
pub mod db;
pub mod lease;
pub mod project;
pub mod recovery;
pub mod work;

use clap::Subcommand;

// ---------------------------------------------------------------------------
// project
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Register a new project
    Register(project::RegisterArgs),
    /// List registered projects
    List(project::ListArgs),
    /// Open a project (launch ccx-cmux workspace)
    Open(project::OpenArgs),
    /// Show project status
    Status(project::StatusArgs),
}

pub fn run_project(cmd: ProjectCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        ProjectCommand::Register(args) => project::register(args),
        ProjectCommand::List(args) => project::list(args),
        ProjectCommand::Open(args) => project::open(args),
        ProjectCommand::Status(args) => project::status(args),
    }
}

// ---------------------------------------------------------------------------
// db
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// work
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum WorkCommand {
    /// Create a new work execution
    Create(work::CreateArgs),
    /// Clean up a work execution
    Cleanup(work::CleanupArgs),
}

pub fn run_work(cmd: WorkCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        WorkCommand::Create(args) => work::create(args),
        WorkCommand::Cleanup(args) => work::cleanup(args),
    }
}

// ---------------------------------------------------------------------------
// agent
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Start an orchestrator agent session for a project
    StartOrchestrator(agent::StartOrchestratorArgs),
    /// Attach a worker, reviewer, or diagnostic agent session
    Attach(agent::AttachArgs),
    /// Send a prompt to an agent session
    Prompt(agent::PromptArgs),
    /// Stop an agent session
    Stop(agent::StopArgs),
    /// Send a user notification for an agent session
    Notify(agent::NotifyArgs),
    /// Record an AgentLifecycleStop event (called by the agent harness on exit)
    LifecycleStop(agent::LifecycleStopArgs),
}

pub fn run_agent(cmd: AgentCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        AgentCommand::StartOrchestrator(args) => agent::start_orchestrator(args),
        AgentCommand::Attach(args) => agent::attach(args),
        AgentCommand::Prompt(args) => agent::prompt(args),
        AgentCommand::Stop(args) => agent::stop(args),
        AgentCommand::Notify(args) => agent::notify(args),
        AgentCommand::LifecycleStop(args) => agent::lifecycle_stop(args),
    }
}

// ---------------------------------------------------------------------------
// lease
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum LeaseCommand {
    /// Acquire a write lease for a work execution
    Acquire(lease::AcquireArgs),
    /// Release a write lease
    Release(lease::ReleaseArgs),
}

pub fn run_lease(cmd: LeaseCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        LeaseCommand::Acquire(args) => lease::acquire(args),
        LeaseCommand::Release(args) => lease::release(args),
    }
}

// ---------------------------------------------------------------------------
// merge
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum MergeCommand {
    /// Execute a PR merge and canonical repo sync
    Execute(work::MergeExecuteArgs),
}

pub fn run_merge(cmd: MergeCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        MergeCommand::Execute(args) => work::merge_execute(args),
    }
}

// ---------------------------------------------------------------------------
// recovery
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum RecoveryCommand {
    /// Generate a recovery / integrity digest for a project
    Digest(recovery::DigestArgs),
    /// Evaluate Circuit Breaker thresholds and auto-transition to Hold if exceeded
    CircuitCheck(recovery::CircuitCheckArgs),
}

pub fn run_recovery(cmd: RecoveryCommand) -> Result<(), crate::error::CcxError> {
    match cmd {
        RecoveryCommand::Digest(args) => recovery::digest(args),
        RecoveryCommand::CircuitCheck(args) => recovery::circuit_check(args),
    }
}

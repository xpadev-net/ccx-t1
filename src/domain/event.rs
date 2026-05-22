use std::sync::Mutex;
use ulid::{Generator, Ulid};
use std::collections::HashMap;

static ULID_GEN: Mutex<Option<Generator>> = Mutex::new(None);

/// Generate a monotonically increasing ULID. Safe for concurrent calls within the same millisecond.
pub fn generate_id() -> Ulid {
    let mut guard = ULID_GEN.lock().expect("ulid generator mutex poisoned");
    let generator = guard.get_or_insert_with(Generator::new);
    generator.generate().unwrap_or_else(|_| Ulid::new())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    ProjectRegistered,
    TaskSourceFileChanged,
    WorkExecutionCreated,
    WorkExecutionTaskFileCreated,
    WorkExecutionStateChanged,
    WorkExecutionTaskFileChanged,
    AgentSessionCreated,
    AgentSessionAttached,
    AgentSessionPrompted,
    AgentSessionHeartbeat,
    AgentSessionHung,
    AgentSessionStopped,
    AgentLifecycleStop,
    WriteLeaseAcquired,
    WriteLeaseReleased,
    WriteLeaseStale,
    WriteLeaseRevoked,
    PrOpened,
    PrHeadUpdated,
    GhReviewHookStarted,
    GhReviewHookCompleted,
    MergeLockAcquired,
    MergeStarted,
    MergeCompleted,
    MergeFailed,
    CanonicalSyncCompleted,
    CanonicalSyncFailed,
    CleanupStarted,
    CleanupCompleted,
    UserIntervention,
    WorktreeCreated,
    BranchCreated,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub event_id: String,
    pub event_type: EventType,
    pub timestamp: String,
    pub actor: Actor,
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Controller,
    Orchestrator,
    Worker,
    Reviewer,
    Diagnostic,
    System,
}

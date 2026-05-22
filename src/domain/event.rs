use std::sync::Mutex;
use ulid::{Generator, Ulid};

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

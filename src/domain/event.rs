use std::sync::Mutex;
use ulid::{Generator, Ulid};

use crate::domain::work_execution::WorkExecutionState;
use serde::{Deserialize, Serialize};

static ULID_GEN: Mutex<Option<Generator>> = Mutex::new(None);

/// Generate a monotonically increasing ULID. Safe for concurrent calls within the same millisecond.
pub fn generate_id() -> Ulid {
    let mut guard = ULID_GEN.lock().expect("ulid generator mutex poisoned");
    let generator = guard.get_or_insert_with(Generator::new);
    generator.generate().unwrap_or_else(|_| Ulid::new())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Controller,
    Orchestrator,
    Worker,
    Reviewer,
    Diagnostic,
    System,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Envelope wrapping any event written to events.jsonl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: String,
    pub project_id: String,
    pub occurred_at: String,
    pub actor: Actor,
    #[serde(flatten)]
    pub data: EventData,
}

impl Event {
    pub fn new(project_id: impl Into<String>, actor: Actor, data: EventData) -> Self {
        Self {
            event_id: generate_id().to_string(),
            project_id: project_id.into(),
            occurred_at: chrono::Utc::now().to_rfc3339(),
            actor,
            data,
        }
    }
}

// ---------------------------------------------------------------------------
// EventData — tagged union, serialises as {"event_type": "...", "payload": {...}}
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "payload", rename_all = "snake_case")]
pub enum EventData {
    ProjectRegistered(ProjectRegisteredPayload),
    TaskSourceFileChanged(TaskSourceFileChangedPayload),
    WorkExecutionCreated(WorkExecutionCreatedPayload),
    WorkExecutionTaskFileCreated(WorkExecutionTaskFileCreatedPayload),
    WorkExecutionStateChanged(WorkExecutionStateChangedPayload),
    WorkExecutionTaskFileChanged(WorkExecutionTaskFileChangedPayload),
    AgentSessionCreated(AgentSessionCreatedPayload),
    AgentSessionAttached(AgentSessionAttachedPayload),
    AgentSessionPrompted(AgentSessionPromptedPayload),
    AgentSessionHeartbeat(AgentSessionHeartbeatPayload),
    AgentSessionHung(AgentSessionHungPayload),
    AgentSessionStopped(AgentSessionStoppedPayload),
    AgentLifecycleStop(AgentLifecycleStopPayload),
    WriteLeaseAcquired(WriteLeaseAcquiredPayload),
    WriteLeaseReleased(WriteLeaseReleasedPayload),
    WriteLeaseStale(WriteLeaseStalePayload),
    WriteLeaseRevoked(WriteLeaseRevokedPayload),
    PrOpened(PrOpenedPayload),
    PrHeadUpdated(PrHeadUpdatedPayload),
    GhReviewHookStarted(GhReviewHookStartedPayload),
    GhReviewHookCompleted(GhReviewHookCompletedPayload),
    MergeLockAcquired(MergeLockAcquiredPayload),
    MergeStarted(MergeStartedPayload),
    MergeCompleted(MergeCompletedPayload),
    MergeFailed(MergeFailedPayload),
    CanonicalSyncCompleted(CanonicalSyncCompletedPayload),
    CanonicalSyncFailed(CanonicalSyncFailedPayload),
    CleanupStarted(CleanupStartedPayload),
    CleanupCompleted(CleanupCompletedPayload),
    UserIntervention(UserInterventionPayload),
    WorktreeCreated(WorktreeCreatedPayload),
    BranchCreated(BranchCreatedPayload),
}

impl EventData {
    pub fn event_type(&self) -> EventType {
        match self {
            Self::ProjectRegistered(_) => EventType::ProjectRegistered,
            Self::TaskSourceFileChanged(_) => EventType::TaskSourceFileChanged,
            Self::WorkExecutionCreated(_) => EventType::WorkExecutionCreated,
            Self::WorkExecutionTaskFileCreated(_) => EventType::WorkExecutionTaskFileCreated,
            Self::WorkExecutionStateChanged(_) => EventType::WorkExecutionStateChanged,
            Self::WorkExecutionTaskFileChanged(_) => EventType::WorkExecutionTaskFileChanged,
            Self::AgentSessionCreated(_) => EventType::AgentSessionCreated,
            Self::AgentSessionAttached(_) => EventType::AgentSessionAttached,
            Self::AgentSessionPrompted(_) => EventType::AgentSessionPrompted,
            Self::AgentSessionHeartbeat(_) => EventType::AgentSessionHeartbeat,
            Self::AgentSessionHung(_) => EventType::AgentSessionHung,
            Self::AgentSessionStopped(_) => EventType::AgentSessionStopped,
            Self::AgentLifecycleStop(_) => EventType::AgentLifecycleStop,
            Self::WriteLeaseAcquired(_) => EventType::WriteLeaseAcquired,
            Self::WriteLeaseReleased(_) => EventType::WriteLeaseReleased,
            Self::WriteLeaseStale(_) => EventType::WriteLeaseStale,
            Self::WriteLeaseRevoked(_) => EventType::WriteLeaseRevoked,
            Self::PrOpened(_) => EventType::PrOpened,
            Self::PrHeadUpdated(_) => EventType::PrHeadUpdated,
            Self::GhReviewHookStarted(_) => EventType::GhReviewHookStarted,
            Self::GhReviewHookCompleted(_) => EventType::GhReviewHookCompleted,
            Self::MergeLockAcquired(_) => EventType::MergeLockAcquired,
            Self::MergeStarted(_) => EventType::MergeStarted,
            Self::MergeCompleted(_) => EventType::MergeCompleted,
            Self::MergeFailed(_) => EventType::MergeFailed,
            Self::CanonicalSyncCompleted(_) => EventType::CanonicalSyncCompleted,
            Self::CanonicalSyncFailed(_) => EventType::CanonicalSyncFailed,
            Self::CleanupStarted(_) => EventType::CleanupStarted,
            Self::CleanupCompleted(_) => EventType::CleanupCompleted,
            Self::UserIntervention(_) => EventType::UserIntervention,
            Self::WorktreeCreated(_) => EventType::WorktreeCreated,
            Self::BranchCreated(_) => EventType::BranchCreated,
        }
    }
}

// ---------------------------------------------------------------------------
// Payload structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRegisteredPayload {
    pub display_slug: String,
    pub canonical_repo: String,
    pub task_source_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSourceFileChangedPayload {
    pub task_source_file: String,
    pub new_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkExecutionCreatedPayload {
    pub work_execution_id: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub task_file_path: String,
    pub source_path: String,
    pub selector_type: String,
    pub selector_value: String,
    pub display_text: String,
    pub source_file_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkExecutionTaskFileCreatedPayload {
    pub work_execution_id: String,
    pub task_file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkExecutionStateChangedPayload {
    pub work_execution_id: String,
    pub from: WorkExecutionState,
    pub to: WorkExecutionState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkExecutionTaskFileChangedPayload {
    pub work_execution_id: String,
    pub new_hash: String,
    pub new_status: Option<String>,
    #[serde(default = "default_status_changed")]
    pub status_changed: bool,
    #[serde(default)]
    pub notification_priority: TaskFileChangePriority,
}

fn default_status_changed() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskFileChangePriority {
    Low,
    Normal,
}

impl Default for TaskFileChangePriority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionCreatedPayload {
    pub agent_session_id: String,
    pub work_execution_id: Option<String>,
    pub role: String,
    pub attach_mode: Option<String>,
    pub cmux_tab_id: String,
    pub tmux_session_id: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionAttachedPayload {
    pub agent_session_id: String,
    pub attach_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionPromptedPayload {
    pub agent_session_id: String,
    pub message_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionHeartbeatPayload {
    pub agent_session_id: String,
    pub pid: Option<u32>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionHungPayload {
    pub agent_session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionStoppedPayload {
    pub agent_session_id: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLifecycleStopPayload {
    pub agent_session_id: String,
    pub work_execution_id: String,
    /// "ready" or "invalid"
    pub artifact_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteLeaseAcquiredPayload {
    pub write_lease_id: String,
    pub work_execution_id: String,
    pub worktree_path: String,
    pub writer_agent_session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteLeaseReleasedPayload {
    pub write_lease_id: String,
    pub work_execution_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteLeaseStalePayload {
    pub write_lease_id: String,
    pub work_execution_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteLeaseRevokedPayload {
    pub write_lease_id: String,
    pub work_execution_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrOpenedPayload {
    pub work_execution_id: String,
    pub pr_number: u64,
    pub pr_url: String,
    pub head_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrHeadUpdatedPayload {
    pub work_execution_id: String,
    pub pr_number: u64,
    pub head_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhReviewHookStartedPayload {
    pub work_execution_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhReviewHookCompletedPayload {
    pub work_execution_id: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeLockAcquiredPayload {
    pub merge_lock_id: String,
    pub work_execution_id: String,
    pub owner_agent_session_id: String,
    pub pr_number: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeStartedPayload {
    pub merge_lock_id: String,
    pub work_execution_id: String,
    pub pr_number: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeCompletedPayload {
    pub merge_lock_id: String,
    pub work_execution_id: String,
    pub pr_number: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeFailedPayload {
    pub merge_lock_id: String,
    pub work_execution_id: String,
    pub pr_number: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalSyncCompletedPayload {
    pub work_execution_id: String,
    pub sync_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalSyncFailedPayload {
    pub work_execution_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupStartedPayload {
    pub work_execution_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupCompletedPayload {
    pub work_execution_id: String,
    pub removed_worktree: bool,
    pub closed_sessions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInterventionPayload {
    pub work_execution_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeCreatedPayload {
    pub work_execution_id: String,
    pub worktree_path: String,
    pub branch_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchCreatedPayload {
    pub work_execution_id: String,
    pub branch_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_roundtrip() {
        let event = Event::new(
            "01JTEST00000000000000000001",
            Actor::Controller,
            EventData::ProjectRegistered(ProjectRegisteredPayload {
                display_slug: "Users-test-repo".into(),
                canonical_repo: "/Users/test/repo".into(),
                task_source_file: "/Users/test/repo/z/tasks.md".into(),
            }),
        );

        let json = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();

        assert_eq!(back.event_id, event.event_id);
        assert_eq!(back.project_id, "01JTEST00000000000000000001");
        assert!(matches!(back.data, EventData::ProjectRegistered(_)));
    }

    #[test]
    fn event_type_tag_in_json() {
        let event = Event::new(
            "01JTEST00000000000000000001",
            Actor::System,
            EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
                work_execution_id: "01JTEST00000000000000000002".into(),
                from: WorkExecutionState::Created,
                to: WorkExecutionState::TaskFileCreated,
            }),
        );

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event_type\":\"work_execution_state_changed\""));
        assert!(json.contains("\"payload\""));
    }

    #[test]
    fn event_type_serializes_as_snake_case() {
        let json = serde_json::to_string(&EventType::GhReviewHookCompleted).unwrap();
        assert_eq!(json, "\"gh_review_hook_completed\"");

        let back: EventType = serde_json::from_str("\"worktree_created\"").unwrap();
        assert_eq!(back, EventType::WorktreeCreated);
    }

    #[test]
    fn event_data_exposes_event_type() {
        let data = EventData::MergeFailed(MergeFailedPayload {
            merge_lock_id: "01JTEST00000000000000000003".into(),
            work_execution_id: "01JTEST00000000000000000002".into(),
            pr_number: 12,
            reason: "conflict".into(),
        });

        assert_eq!(data.event_type(), EventType::MergeFailed);
    }

    #[test]
    fn task_file_changed_payload_defaults_priority_metadata() {
        let json = r#"{
            "work_execution_id": "01JTEST00000000000000000002",
            "new_hash": "abc123",
            "new_status": "working"
        }"#;

        let payload: WorkExecutionTaskFileChangedPayload = serde_json::from_str(json).unwrap();
        assert!(payload.status_changed);
        assert_eq!(
            payload.notification_priority,
            TaskFileChangePriority::Normal
        );
    }
}

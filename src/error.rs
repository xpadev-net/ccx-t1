use crate::domain::work_execution::WorkExecutionState;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CcxError {
    #[error("invalid state transition: {from} -> {to}")]
    InvalidStateTransition {
        from: WorkExecutionState,
        to: WorkExecutionState,
    },

    #[error("write lease conflict: work execution already held by session {active_session_id}")]
    WriteLeaseConflict { active_session_id: String },

    #[error("merge lock conflict: another merge is already in progress for this project")]
    MergeLockConflict,

    #[error("task file conflict: concurrent edit detected on task.md")]
    TaskFileConflict,

    #[error("task source conflict: expected hash {expected_hash}, found {actual_hash}")]
    TaskSourceConflict {
        expected_hash: String,
        actual_hash: String,
    },

    #[error("project not found: {project_id}")]
    ProjectNotFound { project_id: String },

    #[error("config error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("git error: {0}")]
    Git(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl CcxError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CcxError::TaskSourceConflict { .. } => 2,
            _ => 1,
        }
    }
}

impl From<rusqlite::Error> for CcxError {
    fn from(e: rusqlite::Error) -> Self {
        CcxError::Database(e.to_string())
    }
}

impl From<notify::Error> for CcxError {
    fn from(e: notify::Error) -> Self {
        CcxError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    }
}

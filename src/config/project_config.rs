use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhReviewHook {
    pub command: String,
    pub timeout_seconds: u64,
}

impl Default for GhReviewHook {
    fn default() -> Self {
        Self {
            command: "./gh-review-hook".into(),
            timeout_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CleanupPolicy {
    Immediate,
    KeepLastN,
    KeepForDuration,
}

impl Default for CleanupPolicy {
    fn default() -> Self {
        Self::KeepLastN
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project_id: String,
    pub display_slug: String,
    pub canonical_repo: Utf8PathBuf,
    pub task_source_file: Utf8PathBuf,
    pub gh_review_hook: GhReviewHook,
    pub cleanup_policy: CleanupPolicy,
    pub created_at: String,
}

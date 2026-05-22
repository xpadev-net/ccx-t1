use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkExecutionState {
    Created,
    TaskFileCreated,
    Dispatched,
    Running,
    PrOpen,
    GateCheck,
    ReviewFixing,
    MergeReady,
    Merging,
    Merged,
    FollowupRequired,
    Returned,
    Blocked,
    Failed,
    Hold,
    Canceled,
    Superseded,
}

impl fmt::Display for WorkExecutionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{self:?}"));
        write!(f, "{s}")
    }
}

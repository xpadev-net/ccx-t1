use camino::Utf8PathBuf;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::domain::event::{Actor, Event, EventData, WorkExecutionTaskFileChangedPayload};
use crate::error::CcxError;
use crate::watcher::front_matter::parse_front_matter;
use crate::watcher::sha256_hex;

/// Per-execution deduplication state for the task watcher.
pub struct TaskWatcherState {
    pub last_seen_hash: Option<String>,
}

/// Pure observe step: hash the content, deduplicate, parse front matter best-effort.
///
/// Returns `Some(payload)` when the content has changed since the last call;
/// `None` when the content is identical to the previously seen version.
pub fn observe(
    content: &str,
    work_execution_id: &str,
    state: &mut TaskWatcherState,
) -> Option<WorkExecutionTaskFileChangedPayload> {
    let hash = sha256_hex(content);
    if state.last_seen_hash.as_deref() == Some(hash.as_str()) {
        return None;
    }
    state.last_seen_hash = Some(hash.clone());
    let new_status = parse_front_matter(content).ok().and_then(|fm| fm.status);
    Some(WorkExecutionTaskFileChangedPayload {
        work_execution_id: work_execution_id.to_string(),
        new_hash: hash,
        new_status,
    })
}

/// Watches a single `task.md` file for modifications and appends
/// `WorkExecutionTaskFileChanged` events to the project's JSONL audit log.
///
/// Drop this struct to stop watching.
pub struct TaskWatcher {
    _watcher: RecommendedWatcher,
}

impl TaskWatcher {
    pub fn new(
        task_file: &camino::Utf8Path,
        project_id: String,
        work_execution_id: String,
        project_dir: Utf8PathBuf,
    ) -> Result<Self, CcxError> {
        let mut state = TaskWatcherState { last_seen_hash: None };
        let file = task_file.as_std_path().to_owned();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let ev = match res {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "task_watcher: notify error");
                    return;
                }
            };
            if !ev.kind.is_modify() && !ev.kind.is_create() {
                return;
            }
            let content = match std::fs::read_to_string(&file) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %file.display(), error = %e, "task_watcher: read error");
                    return;
                }
            };
            if let Some(payload) = observe(&content, &work_execution_id, &mut state) {
                let event = Event::new(
                    &project_id,
                    Actor::System,
                    EventData::WorkExecutionTaskFileChanged(payload),
                );
                if let Err(e) = crate::persistence::jsonl::append_event_to_dir(&project_dir, &event) {
                    tracing::warn!(error = %e, "task_watcher: append event error");
                }
            }
        })?;

        watcher.watch(task_file.as_std_path(), RecursiveMode::NonRecursive)?;
        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> TaskWatcherState {
        TaskWatcherState { last_seen_hash: None }
    }

    #[test]
    fn first_observation_emits_event() {
        let mut s = state();
        let payload = observe("# task\n", "we-1", &mut s);
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.work_execution_id, "we-1");
        assert!(!p.new_hash.is_empty());
    }

    #[test]
    fn identical_content_is_deduplicated() {
        let mut s = state();
        let content = "---\nstatus: working\n---\n# task\n";
        observe(content, "we-1", &mut s);
        let second = observe(content, "we-1", &mut s);
        assert!(second.is_none(), "identical content must be skipped");
    }

    #[test]
    fn changed_content_emits_new_event() {
        let mut s = state();
        observe("# original\n", "we-1", &mut s);
        let payload = observe("# modified\n", "we-1", &mut s);
        assert!(payload.is_some());
    }

    #[test]
    fn status_is_extracted_from_front_matter() {
        let mut s = state();
        let content = "---\nstatus: pr_open\n---\n# task\n";
        let payload = observe(content, "we-1", &mut s).unwrap();
        assert_eq!(payload.new_status.as_deref(), Some("pr_open"));
    }

    #[test]
    fn missing_front_matter_yields_none_status() {
        let mut s = state();
        let payload = observe("# no front matter\n", "we-1", &mut s).unwrap();
        assert_eq!(payload.new_status, None);
    }

    #[test]
    fn malformed_front_matter_yields_none_status() {
        let mut s = state();
        let content = "---\nstatus: [unclosed\n---\n# task\n";
        let payload = observe(content, "we-1", &mut s).unwrap();
        assert_eq!(payload.new_status, None, "malformed YAML must be best-effort (no panic)");
    }

    #[test]
    fn hash_is_deterministic() {
        let mut s1 = state();
        let mut s2 = state();
        let p1 = observe("# same\n", "we-1", &mut s1).unwrap();
        let p2 = observe("# same\n", "we-2", &mut s2).unwrap();
        assert_eq!(p1.new_hash, p2.new_hash, "SHA-256 must be content-dependent only");
    }

    #[test]
    fn different_content_produces_different_hash() {
        let mut s = state();
        let p1 = observe("# a\n", "we-1", &mut s).unwrap();
        let p2 = observe("# b\n", "we-1", &mut s).unwrap();
        assert_ne!(p1.new_hash, p2.new_hash);
    }
}

use camino::Utf8PathBuf;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::domain::event::{Actor, Event, EventData, TaskSourceFileChangedPayload};
use crate::error::CcxError;
use crate::watcher::sha256_hex;

/// Per-project deduplication state for the source file watcher.
pub struct SourceWatcherState {
    pub last_seen_hash: Option<String>,
}

/// Pure observe step: hash the content and deduplicate.
///
/// Returns `Some(payload)` when the file content has changed since the last
/// call; `None` when it is identical to the previously seen version.
pub fn observe(
    content: &str,
    task_source_file: &str,
    state: &mut SourceWatcherState,
) -> Option<TaskSourceFileChangedPayload> {
    let hash = sha256_hex(content);
    if state.last_seen_hash.as_deref() == Some(hash.as_str()) {
        return None;
    }
    state.last_seen_hash = Some(hash.clone());
    Some(TaskSourceFileChangedPayload {
        task_source_file: task_source_file.to_string(),
        new_hash: hash,
    })
}

/// Watches the project's task source file for modifications and appends
/// `TaskSourceFileChanged` events to the project's JSONL audit log.
///
/// Drop this struct to stop watching.
pub struct SourceWatcher {
    _watcher: RecommendedWatcher,
}

impl SourceWatcher {
    pub fn new(
        source_file: &camino::Utf8Path,
        project_id: String,
        project_dir: Utf8PathBuf,
    ) -> Result<Self, CcxError> {
        let mut state = SourceWatcherState { last_seen_hash: None };
        let file = source_file.as_std_path().to_owned();
        let source_path = source_file.to_string().to_owned();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let ev = match res {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "source_watcher: notify error");
                    return;
                }
            };
            if !ev.kind.is_modify() && !ev.kind.is_create() {
                return;
            }
            let content = match std::fs::read_to_string(&file) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %file.display(), error = %e, "source_watcher: read error");
                    return;
                }
            };
            if let Some(payload) = observe(&content, &source_path, &mut state) {
                let event = Event::new(
                    &project_id,
                    Actor::System,
                    EventData::TaskSourceFileChanged(payload),
                );
                if let Err(e) = crate::persistence::jsonl::append_event_to_dir(&project_dir, &event) {
                    tracing::warn!(error = %e, "source_watcher: append event error");
                }
            }
        })?;

        watcher.watch(source_file.as_std_path(), RecursiveMode::NonRecursive)?;
        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SourceWatcherState {
        SourceWatcherState { last_seen_hash: None }
    }

    #[test]
    fn first_observation_emits_event() {
        let mut s = state();
        let payload = observe("# tasks\n", "/repo/tasks.md", &mut s);
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.task_source_file, "/repo/tasks.md");
        assert!(!p.new_hash.is_empty());
    }

    #[test]
    fn identical_content_is_deduplicated() {
        let mut s = state();
        let content = "# tasks\ncontent\n";
        observe(content, "/repo/tasks.md", &mut s);
        let second = observe(content, "/repo/tasks.md", &mut s);
        assert!(second.is_none(), "identical content must be skipped");
    }

    #[test]
    fn changed_content_emits_new_event() {
        let mut s = state();
        observe("# v1\n", "/repo/tasks.md", &mut s);
        let payload = observe("# v2\n", "/repo/tasks.md", &mut s);
        assert!(payload.is_some());
    }

    #[test]
    fn hash_changes_with_content() {
        let mut s = state();
        let p1 = observe("# a\n", "/tasks.md", &mut s).unwrap();
        let p2 = observe("# b\n", "/tasks.md", &mut s).unwrap();
        assert_ne!(p1.new_hash, p2.new_hash);
    }

    #[test]
    fn source_file_path_preserved_in_payload() {
        let mut s = state();
        let path = "/home/user/project/z/tasks.md";
        let p = observe("content", path, &mut s).unwrap();
        assert_eq!(p.task_source_file, path);
    }

    #[test]
    fn no_initial_hash_means_first_call_always_emits() {
        let mut s = state();
        assert!(s.last_seen_hash.is_none());
        let p = observe("any content", "/tasks.md", &mut s);
        assert!(p.is_some());
        assert!(s.last_seen_hash.is_some());
    }
}

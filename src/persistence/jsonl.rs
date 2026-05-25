use std::fs::{File, OpenOptions};
use std::io::Write;

use camino::Utf8Path;
use fd_lock::RwLock;

use crate::config::project_dir;
use crate::domain::event::Event;
use crate::error::CcxError;

/// Append a single `Event` as a JSON line to `events.jsonl` inside `dir`.
///
/// Uses `events.lock` (a separate sentinel file) for exclusive access so the
/// data file is always opened in append-only mode.
pub fn append_event_to_dir(dir: &Utf8Path, event: &Event) -> Result<(), CcxError> {
    append_events_to_dir(dir, std::slice::from_ref(event))
        .map_err(EventBatchAppendError::into_error)
}

#[derive(Debug)]
pub enum EventBatchAppendError {
    RolledBack(CcxError),
    Indeterminate(CcxError),
}

impl EventBatchAppendError {
    pub fn into_error(self) -> CcxError {
        match self {
            Self::RolledBack(error) | Self::Indeterminate(error) => error,
        }
    }
}

/// Append multiple `Event`s as JSON lines to `events.jsonl` inside `dir`
/// while holding one exclusive lock. Projection still runs after the durable
/// append, preserving JSONL as the source of truth.
pub fn append_events_to_dir(dir: &Utf8Path, events: &[Event]) -> Result<(), EventBatchAppendError> {
    let mut lines = Vec::with_capacity(events.len());
    for event in events {
        // Serialize to JSON (single line, no pretty-printing) before touching
        // the file so serialization failures cannot partially append a batch.
        let mut line = serde_json::to_string(event)
            .map_err(|error| EventBatchAppendError::RolledBack(CcxError::Json(error)))?;
        line.push('\n');
        lines.push(line);
    }

    std::fs::create_dir_all(dir)
        .map_err(|error| EventBatchAppendError::RolledBack(CcxError::Io(error)))?;

    let lock_path = dir.join("events.lock");
    let log_path = dir.join("events.jsonl");

    // Acquire exclusive lock on the sentinel file.
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| EventBatchAppendError::RolledBack(CcxError::Io(error)))?;
    let mut rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.write().map_err(|error| {
        EventBatchAppendError::RolledBack(CcxError::Io(std::io::Error::new(
            error.kind(),
            error.to_string(),
        )))
    })?;

    // Open the JSONL file in append mode (create if absent).
    let mut log_file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| EventBatchAppendError::RolledBack(CcxError::Io(error)))?;
    let original_len = log_file
        .metadata()
        .map_err(|error| EventBatchAppendError::RolledBack(CcxError::Io(error)))?
        .len();

    let fail_after_lines = test_fail_after_batch_lines();
    for (index, line) in lines.iter().enumerate() {
        if let Err(error) = log_file.write_all(line.as_bytes()) {
            return rollback_or_indeterminate(&mut log_file, original_len, CcxError::Io(error));
        }
        if fail_after_lines == Some(index + 1) {
            return rollback_or_indeterminate(
                &mut log_file,
                original_len,
                CcxError::Other(anyhow::anyhow!(
                    "simulated event batch append failure after {} lines",
                    index + 1
                )),
            );
        }
    }
    if let Err(error) = log_file.flush() {
        return rollback_or_indeterminate(&mut log_file, original_len, CcxError::Io(error));
    }
    if let Err(error) = log_file.sync_all() {
        return rollback_or_indeterminate(&mut log_file, original_len, CcxError::Io(error));
    }

    // Release the events.lock before the SQLite projection so concurrent appenders
    // are not blocked during DB I/O. JSONL durability is already guaranteed above.
    drop(_guard);

    // Best-effort SQLite projection — JSONL write already succeeded.
    for event in events {
        crate::persistence::projector::try_project_event(dir, event);
    }

    Ok(())
}

fn rollback_or_indeterminate(
    log_file: &mut File,
    original_len: u64,
    original_error: CcxError,
) -> Result<(), EventBatchAppendError> {
    if log_file.set_len(original_len).is_ok() && log_file.sync_all().is_ok() {
        Err(EventBatchAppendError::RolledBack(original_error))
    } else {
        Err(EventBatchAppendError::Indeterminate(original_error))
    }
}

fn test_fail_after_batch_lines() -> Option<usize> {
    #[cfg(debug_assertions)]
    {
        std::env::var("CCX_TEST_FAIL_EVENT_BATCH_AFTER_LINES")
            .ok()
            .and_then(|value| value.parse().ok())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// Execute an atomic read-evaluate-write operation on the event log.
///
/// Acquires the write lock, reads the current events, passes them to `f`,
/// and — if `f` returns `Some(event)` — appends that event before releasing
/// the lock. This guarantees that the check and the write are serialized with
/// respect to all other callers that go through `append_event_to_dir`.
///
/// Returns `true` if an event was actually appended, `false` if the closure
/// returned `None` (no-op / already-handled case).
///
/// The SQLite projection runs after the lock is released (best-effort, as usual).
pub fn locked_read_write<F>(dir: &Utf8Path, f: F) -> Result<bool, CcxError>
where
    F: FnOnce(&[Event]) -> Result<Option<Event>, CcxError>,
{
    std::fs::create_dir_all(dir)?;

    let lock_path = dir.join("events.lock");
    let log_path = dir.join("events.jsonl");

    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    let mut rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.write()?;

    // Read current events under the lock (std::fs::read_to_string does not
    // acquire the advisory lock again — it just opens the file for reading).
    let events = read_events_from_dir(dir)?;
    let maybe_event = f(&events)?;

    if let Some(ref event) = maybe_event {
        let mut line = serde_json::to_string(event)?;
        line.push('\n');
        let mut log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        log_file.write_all(line.as_bytes())?;
        log_file.flush()?;
        log_file.sync_all()?;
    }

    let written = maybe_event.is_some();

    // Release the write lock before the SQLite projection.
    drop(_guard);

    if let Some(event) = maybe_event {
        crate::persistence::projector::try_project_event(dir, &event);
    }
    Ok(written)
}

/// Append a single `Event` to the project's canonical `events.jsonl`.
pub fn append_event(project_id: &str, event: &Event) -> Result<(), CcxError> {
    let dir = project_dir(project_id)?;
    append_event_to_dir(&dir, event)
}

/// Read all events from the project's `events.jsonl`, in append order.
pub fn read_events(project_id: &str) -> Result<Vec<Event>, CcxError> {
    let dir = project_dir(project_id)?;
    read_events_from_dir(&dir)
}

/// Read all events from `events.jsonl` inside `dir`, in append order.
///
/// Note: this function does not hold the `events.lock` while reading.
/// Concurrent appenders use O_APPEND semantics, so complete JSON lines are
/// atomic for writes up to the OS page size, but a reader can theoretically
/// observe a partial last line if a very large event is mid-write. In that
/// case `serde_json::from_str` will return a parse error; callers should
/// treat `CcxError::Database` on the last line as a transient condition and
/// retry if needed.
pub fn read_events_from_dir(dir: &Utf8Path) -> Result<Vec<Event>, CcxError> {
    let log_path = dir.join("events.jsonl");

    let raw = match std::fs::read_to_string(&log_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut events = Vec::new();
    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: Event = serde_json::from_str(line).map_err(|e| {
            CcxError::Database(format!("events.jsonl:{}: invalid JSON: {e}", line_no + 1))
        })?;
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{Actor, EventData, ProjectRegisteredPayload};

    fn make_registered_event(id: &str, slug: &str) -> Event {
        Event::new(
            id,
            Actor::Controller,
            EventData::ProjectRegistered(ProjectRegisteredPayload {
                display_slug: slug.into(),
                canonical_repo: "/tmp/repo".into(),
                task_source_file: "/tmp/repo/tasks.md".into(),
            }),
        )
    }

    #[test]
    fn two_appends_produce_two_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let id = crate::domain::event::generate_id().to_string();

        let e1 = make_registered_event(&id, "repo-1");
        let e2 = make_registered_event(&id, "repo-2");

        append_event_to_dir(&dir, &e1).unwrap();
        append_event_to_dir(&dir, &e2).unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, e1.event_id);
        assert_eq!(events[1].event_id, e2.event_id);
    }

    #[test]
    fn roundtrip_preserves_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let id = crate::domain::event::generate_id().to_string();

        let event = Event::new(
            &id,
            Actor::Controller,
            EventData::ProjectRegistered(ProjectRegisteredPayload {
                display_slug: "slug".into(),
                canonical_repo: "/canonical".into(),
                task_source_file: "/canonical/tasks.md".into(),
            }),
        );

        append_event_to_dir(&dir, &event).unwrap();

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 1);

        let back = &events[0];
        assert_eq!(back.event_id, event.event_id);
        assert_eq!(back.project_id, id);
        assert!(matches!(back.data, EventData::ProjectRegistered(_)));
        if let EventData::ProjectRegistered(p) = &back.data {
            assert_eq!(p.display_slug, "slug");
            assert_eq!(p.canonical_repo, "/canonical");
        }
    }

    #[test]
    fn concurrent_appends_do_not_interleave() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let id = crate::domain::event::generate_id().to_string();
        let dir2 = dir.clone();
        let id2 = id.clone();

        std::thread::scope(|s| {
            let t1 = s.spawn(|| {
                for _ in 0..10 {
                    let e = make_registered_event(&id, "t1");
                    append_event_to_dir(&dir, &e).unwrap();
                }
            });
            let t2 = s.spawn(|| {
                for _ in 0..10 {
                    let e = make_registered_event(&id2, "t2");
                    append_event_to_dir(&dir2, &e).unwrap();
                }
            });
            t1.join().unwrap();
            t2.join().unwrap();
        });

        let events = read_events_from_dir(&dir).unwrap();
        assert_eq!(events.len(), 20);
    }

    #[test]
    fn locked_read_write_appends_event_and_returns_true() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let id = crate::domain::event::generate_id().to_string();

        let event = make_registered_event(&id, "test-slug");
        let written = locked_read_write(&dir, |_events| Ok(Some(event.clone()))).unwrap();
        assert!(written);

        let stored = read_events_from_dir(&dir).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].event_id, event.event_id);
    }

    #[test]
    fn locked_read_write_skips_when_none_returned() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let written = locked_read_write(&dir, |_events| Ok(None)).unwrap();
        assert!(!written);
        assert!(read_events_from_dir(&dir).unwrap().is_empty());
    }

    #[test]
    fn locked_read_write_closure_sees_prior_events() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let id = crate::domain::event::generate_id().to_string();

        append_event_to_dir(&dir, &make_registered_event(&id, "first")).unwrap();

        let mut seen_count = 0usize;
        locked_read_write(&dir, |events| {
            seen_count = events.len();
            Ok(None)
        })
        .unwrap();
        assert_eq!(seen_count, 1);
    }
}

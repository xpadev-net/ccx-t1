use crate::domain::event::Event;
use crate::error::CcxError;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;
use fd_lock::RwLock;

/// JsonlEventLog provides append-only JSONL logging with exclusive write locking
pub struct JsonlEventLog {
    path: std::path::PathBuf,
}

impl JsonlEventLog {
    /// Create a new JsonlEventLog at the specified path
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Append an event to the JSONL file with exclusive file locking
    pub fn append(&self, event: &Event) -> Result<(), CcxError> {
        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CcxError::IoError(e.to_string()))?;
        }

        // Serialize event to JSON
        let json = serde_json::to_string(&event)
            .map_err(|e| CcxError::SerializationError(e.to_string()))?;

        // Acquire write lock on the events.lock file
        let lock_path = self.path.with_extension("lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| CcxError::IoError(format!("Failed to open lock file: {}", e)))?;

        let mut lock = RwLock::new(lock_file);
        let _write_guard = lock
            .write()
            .map_err(|e| CcxError::IoError(format!("Failed to acquire write lock: {}", e)))?;

        // Open the JSONL file in append mode
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| CcxError::IoError(format!("Failed to open JSONL file: {}", e)))?;

        let mut writer = BufWriter::new(file);

        // Write JSON + newline
        writeln!(writer, "{}", json)
            .map_err(|e| CcxError::IoError(format!("Failed to write to JSONL: {}", e)))?;

        // Flush and sync to ensure durability
        writer.flush()
            .map_err(|e| CcxError::IoError(format!("Failed to flush JSONL: {}", e)))?;

        // Sync to disk
        if let Ok(file) = writer.into_inner() {
            file.sync_all()
                .map_err(|e| CcxError::IoError(format!("Failed to sync JSONL: {}", e)))?;
        }

        Ok(())
    }

    /// Read all events from the JSONL file
    pub fn read_all(&self) -> Result<Vec<Event>, CcxError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.path)
            .map_err(|e| CcxError::IoError(format!("Failed to read JSONL: {}", e)))?;

        let mut events = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let event: Event = serde_json::from_str(line)
                .map_err(|e| CcxError::SerializationError(format!("Failed to parse event: {}", e)))?;
            events.push(event);
        }

        Ok(events)
    }

    /// Get the path to the JSONL file
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{Actor, EventType, generate_id};
    use chrono::Utc;

    #[test]
    fn test_append_and_read() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let log_path = temp_dir.path().join("events.jsonl");
        let log = JsonlEventLog::new(&log_path);

        let event = Event {
            event_id: generate_id().to_string(),
            event_type: EventType::ProjectRegistered,
            timestamp: Utc::now().to_rfc3339(),
            actor: Actor::Controller,
            project_id: "test-project".to_string(),
            work_execution_id: None,
            agent_session_id: None,
            context: None,
        };

        log.append(&event).unwrap();

        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, event.event_id);
        assert_eq!(events[0].event_type, event.event_type);
    }

    #[test]
    fn test_multiple_appends() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let log_path = temp_dir.path().join("events.jsonl");
        let log = JsonlEventLog::new(&log_path);

        for i in 0..5 {
            let event = Event {
                event_id: generate_id().to_string(),
                event_type: EventType::ProjectRegistered,
                timestamp: Utc::now().to_rfc3339(),
                actor: Actor::Controller,
                project_id: format!("test-project-{}", i),
                work_execution_id: None,
                agent_session_id: None,
                context: None,
            };
            log.append(&event).unwrap();
        }

        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 5);
    }
}

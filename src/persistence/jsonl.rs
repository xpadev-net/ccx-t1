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
            std::fs::create_dir_all(parent)?;
        }

        // Serialize event to JSON
        let json = serde_json::to_string(&event)
            .map_err(|e| CcxError::SerializationError(e.to_string()))?;

        // Acquire write lock on the events.lock file
        let lock_path = self.path.with_extension("lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;

        let mut lock = RwLock::new(lock_file);
        let _write_guard = lock.write()
            .map_err(|_| CcxError::Other(anyhow::anyhow!("Failed to acquire write lock")))?;

        // Open the JSONL file in append mode
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let mut writer = BufWriter::new(file);

        // Write JSON + newline
        writeln!(writer, "{}", json)?;

        // Flush and sync to ensure durability
        writer.flush()?;

        // Sync to disk
        if let Ok(file) = writer.into_inner() {
            file.sync_all()?;
        }

        Ok(())
    }

    /// Read all events from the JSONL file
    pub fn read_all(&self) -> Result<Vec<Event>, CcxError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.path)?;

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
    use std::collections::HashMap;

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

    #[test]
    fn test_read_nonexistent_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let log_path = temp_dir.path().join("nonexistent.jsonl");
        let log = JsonlEventLog::new(&log_path);

        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_append_with_context() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let log_path = temp_dir.path().join("events.jsonl");
        let log = JsonlEventLog::new(&log_path);

        let mut context = HashMap::new();
        context.insert("key1".to_string(), serde_json::json!("value1"));
        context.insert("key2".to_string(), serde_json::json!(42));

        let event = Event {
            event_id: generate_id().to_string(),
            event_type: EventType::WorkExecutionCreated,
            timestamp: Utc::now().to_rfc3339(),
            actor: Actor::Controller,
            project_id: "test-project".to_string(),
            work_execution_id: Some("work-id".to_string()),
            agent_session_id: Some("session-id".to_string()),
            context: Some(context),
        };

        log.append(&event).unwrap();

        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].context.is_some());
        let ctx = events[0].context.as_ref().unwrap();
        assert_eq!(ctx.get("key1").unwrap().as_str(), Some("value1"));
        assert_eq!(ctx.get("key2").unwrap().as_i64(), Some(42));
    }

    #[test]
    fn test_lock_file_created() {
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

        let lock_path = log_path.with_extension("lock");
        assert!(lock_path.exists());
    }
}

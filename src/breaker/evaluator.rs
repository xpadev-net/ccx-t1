use camino::Utf8Path;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::domain::event::{Actor, Event, EventData, WorkExecutionStateChangedPayload};
use crate::domain::transition::validate_transition;
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;
use crate::persistence::jsonl::{append_event_to_dir, read_events_from_dir};
use crate::persistence::sqlite::open_db;

pub struct CircuitBreakerConfig {
    pub project_id: String,
    pub work_execution_id: String,
    /// Transition to Hold after this many `failed` landings.
    pub max_retries: u32,
    /// Transition to Hold after this many hours since the WE was created.
    pub max_hours: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CircuitBreakerResult {
    pub work_execution_id: String,
    pub triggered: bool,
    pub reason: Option<String>,
    pub retry_count: u32,
    pub elapsed_hours: f64,
}

/// Count how many times this work execution has landed in the `Failed` state
/// by scanning `WorkExecutionStateChanged` events in append order.
fn count_failures(events: &[Event], work_execution_id: &str) -> u32 {
    events
        .iter()
        .filter_map(|e| {
            if let EventData::WorkExecutionStateChanged(p) = &e.data {
                if p.work_execution_id == work_execution_id
                    && p.to == WorkExecutionState::Failed
                {
                    return Some(());
                }
            }
            None
        })
        .count() as u32
}

fn query_current_state(
    conn: &rusqlite::Connection,
    work_execution_id: &str,
) -> Result<WorkExecutionState, CcxError> {
    let state_str: String = conn
        .query_row(
            "SELECT state FROM work_executions WHERE work_execution_id = ?1",
            rusqlite::params![work_execution_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            if e == rusqlite::Error::QueryReturnedNoRows {
                CcxError::Other(anyhow::anyhow!(
                    "work execution not found: {work_execution_id}"
                ))
            } else {
                CcxError::Database(e.to_string())
            }
        })?;

    let state: WorkExecutionState = serde_json::from_value(serde_json::Value::String(state_str))
        .map_err(|e| CcxError::Database(format!("invalid state value: {e}")))?;
    Ok(state)
}

fn query_selected_at(
    conn: &rusqlite::Connection,
    work_execution_id: &str,
) -> Result<String, CcxError> {
    conn.query_row(
        "SELECT selected_at FROM work_executions WHERE work_execution_id = ?1",
        rusqlite::params![work_execution_id],
        |row| row.get(0),
    )
    .map_err(|e| {
        if e == rusqlite::Error::QueryReturnedNoRows {
            CcxError::Other(anyhow::anyhow!(
                "work execution not found: {work_execution_id}"
            ))
        } else {
            CcxError::Database(e.to_string())
        }
    })
}

fn elapsed_hours(selected_at: &str) -> Result<f64, CcxError> {
    let ts = chrono::DateTime::parse_from_rfc3339(selected_at)
        .map_err(|e| CcxError::Database(format!("invalid selected_at timestamp: {e}")))?;
    let now = Utc::now();
    let delta = now - ts.with_timezone(&Utc);
    Ok(delta.num_seconds() as f64 / 3600.0)
}

pub fn evaluate(config: &CircuitBreakerConfig, dir: &Utf8Path) -> Result<CircuitBreakerResult, CcxError> {
    let events = read_events_from_dir(dir)?;
    let retry_count = count_failures(&events, &config.work_execution_id);

    let conn = open_db(dir)?;
    let selected_at = query_selected_at(&conn, &config.work_execution_id)?;
    let elapsed = elapsed_hours(&selected_at)?;

    let (triggered, reason) = if retry_count >= config.max_retries {
        (
            true,
            Some(format!(
                "retry count {retry_count} >= threshold {}",
                config.max_retries
            )),
        )
    } else if elapsed >= config.max_hours as f64 {
        (
            true,
            Some(format!(
                "elapsed {elapsed:.1}h >= threshold {}h",
                config.max_hours
            )),
        )
    } else {
        (false, None)
    };

    if triggered {
        let current = query_current_state(&conn, &config.work_execution_id)?;
        // Skip if already in Hold (idempotent)
        if current != WorkExecutionState::Hold {
            validate_transition(current, WorkExecutionState::Hold)?;
            let event = Event::new(
                &config.project_id,
                Actor::Controller,
                EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
                    work_execution_id: config.work_execution_id.clone(),
                    from: current,
                    to: WorkExecutionState::Hold,
                }),
            );
            append_event_to_dir(dir, &event)?;
        }
    }

    Ok(CircuitBreakerResult {
        work_execution_id: config.work_execution_id.clone(),
        triggered,
        reason,
        retry_count,
        elapsed_hours: elapsed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{EventData, WorkExecutionStateChangedPayload};
    use crate::domain::work_execution::WorkExecutionState;

    fn make_event(
        project_id: &str,
        we_id: &str,
        from: WorkExecutionState,
        to: WorkExecutionState,
    ) -> Event {
        Event::new(
            project_id,
            Actor::Controller,
            EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
                work_execution_id: we_id.to_owned(),
                from,
                to,
            }),
        )
    }

    #[test]
    fn count_failures_counts_only_failed_landings() {
        let project_id = "01JTEST00000000000000000001";
        let we_id = "01JTEST00000000000000000002";
        let events = vec![
            make_event(project_id, we_id, WorkExecutionState::Running, WorkExecutionState::Failed),
            make_event(project_id, we_id, WorkExecutionState::Failed, WorkExecutionState::Dispatched),
            make_event(project_id, we_id, WorkExecutionState::Running, WorkExecutionState::Failed),
            // different WE — should not count
            make_event(project_id, "01JTEST00000000000000000003", WorkExecutionState::Running, WorkExecutionState::Failed),
        ];
        assert_eq!(count_failures(&events, we_id), 2);
    }

    #[test]
    fn elapsed_hours_returns_positive_for_past_timestamp() {
        let an_hour_ago = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let h = elapsed_hours(&an_hour_ago).unwrap();
        assert!(h >= 1.9 && h <= 2.1, "expected ~2h, got {h}");
    }
}

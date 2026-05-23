use camino::Utf8Path;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::domain::event::{Actor, Event, EventData, WorkExecutionStateChangedPayload};
use crate::domain::transition::validate_transition;
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;
use crate::persistence::jsonl::{locked_read_write, read_events_from_dir};
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
    /// `true` when a Hold event was actually written this invocation.
    /// `false` if the threshold was not met, or if the WE was already in Hold.
    pub triggered: bool,
    /// `true` when the retry or time threshold was met (regardless of prior Hold).
    pub threshold_met: bool,
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

/// Derive the current state from the JSONL event stream (source of truth).
/// Returns the `to` field of the last `WorkExecutionStateChanged` event for
/// the given WE.  Returns `None` when no state-change event exists yet.
fn current_state_from_events(events: &[Event], work_execution_id: &str) -> Option<WorkExecutionState> {
    events.iter().rev().find_map(|e| {
        if let EventData::WorkExecutionStateChanged(p) = &e.data {
            if p.work_execution_id == work_execution_id {
                return Some(p.to);
            }
        }
        None
    })
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
    // Clamp to 0 so future-dated selected_at (clock skew) does not produce a
    // negative elapsed time in the output.
    Ok(delta.num_seconds().max(0) as f64 / 3600.0)
}

pub fn evaluate(config: &CircuitBreakerConfig, dir: &Utf8Path) -> Result<CircuitBreakerResult, CcxError> {
    // Read events once for the failure count (uncontended read-only use).
    let events = read_events_from_dir(dir)?;
    let retry_count = count_failures(&events, &config.work_execution_id);

    let conn = open_db(dir)?;
    let selected_at = query_selected_at(&conn, &config.work_execution_id)?;
    let elapsed = elapsed_hours(&selected_at)?;

    let (threshold_met, reason) = if retry_count >= config.max_retries {
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

    // `triggered` is true only when a Hold event was actually written this call.
    // If the WE is already in Hold (prior invocation), locked_read_write returns
    // false — making repeated invocations distinguishable in the JSON output.
    let triggered = if threshold_met {
        let project_id = config.project_id.clone();
        let work_execution_id = config.work_execution_id.clone();
        locked_read_write(dir, |latest_events| {
            // Re-derive state from the freshly-read (under-lock) event stream.
            // Fall back to SQLite only when no state-change event exists yet.
            let current = match current_state_from_events(latest_events, &work_execution_id) {
                Some(s) => s,
                None => query_current_state(&conn, &work_execution_id)?,
            };
            if current == WorkExecutionState::Hold {
                return Ok(None); // already in Hold — idempotent no-op
            }
            validate_transition(current, WorkExecutionState::Hold)?;
            Ok(Some(Event::new(
                &project_id,
                Actor::Controller,
                EventData::WorkExecutionStateChanged(WorkExecutionStateChangedPayload {
                    work_execution_id: work_execution_id.clone(),
                    from: current,
                    to: WorkExecutionState::Hold,
                }),
            )))
        })?
    } else {
        false
    };

    Ok(CircuitBreakerResult {
        work_execution_id: config.work_execution_id.clone(),
        triggered,
        threshold_met,
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
    fn current_state_from_events_returns_last_transition() {
        let project_id = "01JTEST00000000000000000001";
        let we_id = "01JTEST00000000000000000002";
        let events = vec![
            make_event(project_id, we_id, WorkExecutionState::Running, WorkExecutionState::Failed),
            make_event(project_id, we_id, WorkExecutionState::Failed, WorkExecutionState::Dispatched),
            make_event(project_id, we_id, WorkExecutionState::Dispatched, WorkExecutionState::Hold),
        ];
        assert_eq!(
            current_state_from_events(&events, we_id),
            Some(WorkExecutionState::Hold)
        );
    }

    #[test]
    fn current_state_from_events_none_when_no_events() {
        let events: Vec<Event> = vec![];
        assert_eq!(
            current_state_from_events(&events, "01JTEST00000000000000000002"),
            None
        );
    }

    #[test]
    fn elapsed_hours_returns_positive_for_past_timestamp() {
        let an_hour_ago = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let h = elapsed_hours(&an_hour_ago).unwrap();
        assert!(h >= 1.9 && h <= 2.1, "expected ~2h, got {h}");
    }
}

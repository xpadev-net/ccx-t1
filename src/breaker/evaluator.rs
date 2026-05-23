use camino::Utf8Path;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::domain::event::{Actor, Event, EventData, WorkExecutionStateChangedPayload};
use crate::domain::transition::validate_transition;
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;
use crate::persistence::jsonl::locked_read_write;
use crate::persistence::sqlite::open_db;

pub struct CircuitBreakerConfig {
    pub project_id: String,
    pub work_execution_id: String,
    /// Transition to Hold after this many `failed` landings since last resume.
    pub max_retries: u32,
    /// Transition to Hold after this many hours since selected_at.
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

/// Count failures since the last Hold→resume transition so that an operator
/// who manually resumes a WE from Hold gets a fresh failure budget.
/// Falls back to counting all failures when no resume has occurred yet.
fn count_failures_since_last_resume(events: &[Event], work_execution_id: &str) -> u32 {
    // Find the position of the last "Hold → active-state" event.
    let resume_pos = events.iter().rposition(|e| {
        if let EventData::WorkExecutionStateChanged(p) = &e.data {
            p.work_execution_id == work_execution_id
                && p.from == WorkExecutionState::Hold
                && matches!(
                    p.to,
                    WorkExecutionState::Dispatched
                        | WorkExecutionState::Running
                        | WorkExecutionState::TaskFileCreated
                )
        } else {
            false
        }
    });

    let start = resume_pos.map(|i| i + 1).unwrap_or(0);
    events[start..]
        .iter()
        .filter(|e| {
            if let EventData::WorkExecutionStateChanged(p) = &e.data {
                p.work_execution_id == work_execution_id && p.to == WorkExecutionState::Failed
            } else {
                false
            }
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
    let conn = open_db(dir)?;
    let selected_at = query_selected_at(&conn, &config.work_execution_id)?;

    // All threshold evaluation and the conditional Hold write happen inside
    // locked_read_write so the retry count, elapsed time, threshold check,
    // and event append are all serialized against concurrent invocations.
    // Results are communicated back to the caller via mutable captures
    // (safe: FnOnce, single-threaded).
    let mut result_elapsed: f64 = 0.0;
    let mut result_retry_count: u32 = 0;
    let mut result_threshold_met = false;
    let mut result_reason: Option<String> = None;

    let project_id = config.project_id.clone();
    let work_execution_id = config.work_execution_id.clone();
    let max_retries = config.max_retries;
    let max_hours = config.max_hours;

    let triggered = locked_read_write(dir, |latest_events| {
        // Re-count under lock using the current event stream.
        let retry_count =
            count_failures_since_last_resume(latest_events, &work_execution_id);
        result_retry_count = retry_count;

        // Re-compute elapsed under lock so both threshold checks share the
        // same wall-clock snapshot, avoiding a stale pre-lock elapsed value.
        let elapsed = elapsed_hours(&selected_at)?;
        result_elapsed = elapsed;

        let (threshold_met, reason) = if retry_count >= max_retries {
            (
                true,
                Some(format!(
                    "retry count {retry_count} >= threshold {max_retries}"
                )),
            )
        } else if elapsed >= max_hours as f64 {
            (
                true,
                Some(format!(
                    "elapsed {elapsed:.1}h >= threshold {max_hours}h"
                )),
            )
        } else {
            (false, None)
        };
        result_threshold_met = threshold_met;
        result_reason = reason;

        if !threshold_met {
            return Ok(None);
        }

        // Idempotency: derive current state from JSONL (source of truth);
        // fall back to SQLite only when no state-change event exists yet.
        let current = match current_state_from_events(latest_events, &work_execution_id) {
            Some(s) => s,
            None => query_current_state(&conn, &work_execution_id)?,
        };
        if current == WorkExecutionState::Hold {
            return Ok(None); // already in Hold — no-op
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
    })?;

    Ok(CircuitBreakerResult {
        work_execution_id: config.work_execution_id.clone(),
        triggered,
        threshold_met: result_threshold_met,
        reason: result_reason,
        retry_count: result_retry_count,
        elapsed_hours: result_elapsed,
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
    fn count_failures_since_last_resume_counts_since_resume() {
        let project_id = "01JTEST00000000000000000001";
        let we_id = "01JTEST00000000000000000002";
        let events = vec![
            make_event(project_id, we_id, WorkExecutionState::Running, WorkExecutionState::Failed),
            make_event(project_id, we_id, WorkExecutionState::Failed, WorkExecutionState::Hold),
            // operator resumes — failure budget resets here
            make_event(project_id, we_id, WorkExecutionState::Hold, WorkExecutionState::Dispatched),
            make_event(project_id, we_id, WorkExecutionState::Running, WorkExecutionState::Failed),
            // different WE — must not count
            make_event(project_id, "01JTEST00000000000000000003", WorkExecutionState::Running, WorkExecutionState::Failed),
        ];
        // Only 1 failure since the resume, not 2
        assert_eq!(count_failures_since_last_resume(&events, we_id), 1);
    }

    #[test]
    fn count_failures_since_last_resume_no_resume_counts_all() {
        let project_id = "01JTEST00000000000000000001";
        let we_id = "01JTEST00000000000000000002";
        let events = vec![
            make_event(project_id, we_id, WorkExecutionState::Running, WorkExecutionState::Failed),
            make_event(project_id, we_id, WorkExecutionState::Failed, WorkExecutionState::Dispatched),
            make_event(project_id, we_id, WorkExecutionState::Running, WorkExecutionState::Failed),
        ];
        assert_eq!(count_failures_since_last_resume(&events, we_id), 2);
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
        let two_hours_ago = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let h = elapsed_hours(&two_hours_ago).unwrap();
        assert!(h >= 1.9 && h <= 2.1, "expected ~2h, got {h}");
    }
}

use std::process::Command;

use camino::Utf8Path;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::CcxError;
use crate::persistence::sqlite::{is_dirty, open_db};

/// A write lease that has not received a heartbeat within the stale threshold.
#[derive(Debug, Serialize, Deserialize)]
pub struct StaleLeaseEntry {
    pub write_lease_id: String,
    pub work_execution_id: String,
    pub last_heartbeat_at: String,
    pub stale_seconds: i64,
}

/// A merge lock that has not received a heartbeat within the stale threshold.
#[derive(Debug, Serialize, Deserialize)]
pub struct StaleLockEntry {
    pub merge_lock_id: String,
    pub work_execution_id: String,
    pub last_heartbeat_at: String,
    pub stale_seconds: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Diagnostics {
    pub active_sessions: usize,
    /// Agent session IDs that are active in SQLite but whose tmux session is gone.
    pub orphaned_tmux_sessions: Vec<String>,
    pub stale_leases: Vec<StaleLeaseEntry>,
    pub stale_merge_locks: Vec<StaleLockEntry>,
    pub sqlite_dirty: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryDigest {
    pub project_id: String,
    pub timestamp: String,
    pub diagnostics: Diagnostics,
}

/// Write leases stale after 5 minutes without a heartbeat.
const LEASE_STALE_SECS: i64 = 300;
/// Merge locks stale after 30 minutes without a heartbeat.
const LOCK_STALE_SECS: i64 = 1800;
/// Sentinel returned for corrupted/unparseable timestamps (~317 years).
/// Large enough to always trigger the stale threshold, small enough to be
/// interpretable in JSON output without overflow surprises.
const STALE_SECS_CORRUPTED: i64 = 10_000_000_000;

struct ActiveSession {
    agent_session_id: String,
}

fn tmux_session_exists(agent_session_id: &str) -> bool {
    let target = format!("=ccx-{agent_session_id}");
    match Command::new("tmux")
        .args(["has-session", "-t", &target])
        .output()
    {
        Ok(o) => o.status.success(),
        Err(e) => {
            // tmux binary unavailable or permission error: assume session alive
            // to avoid flooding the digest with false orphan reports.
            tracing::warn!(error = %e, agent_session_id, "tmux unavailable; skipping orphan check for session");
            true
        }
    }
}

fn query_active_sessions(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<ActiveSession>, CcxError> {
    // Include 'lost' — the spec designates it as a primary recovery signal that
    // should be cross-checked against the live tmux session.
    let mut stmt = conn.prepare(
        "SELECT agent_session_id FROM agent_sessions
         WHERE project_id = ?1 AND state IN ('starting', 'running', 'idle', 'hung', 'lost')",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![project_id], |row| {
            Ok(ActiveSession { agent_session_id: row.get(0)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn stale_seconds(last_heartbeat_at: &str, now: &DateTime<Utc>) -> i64 {
    match DateTime::parse_from_rfc3339(last_heartbeat_at) {
        // A corrupted timestamp is treated as maximally stale (STALE_SECS_CORRUPTED)
        // so it surfaces in the digest rather than being silently hidden.
        Err(_) => STALE_SECS_CORRUPTED,
        // Clamp negative values (future-dated timestamps / clock skew) to 0
        // to avoid false positives without hiding genuinely stale entries.
        Ok(ts) => (*now - ts.with_timezone(&Utc)).num_seconds().max(0),
    }
}

fn query_stale_leases(
    conn: &Connection,
    project_id: &str,
    now: &DateTime<Utc>,
) -> Result<Vec<StaleLeaseEntry>, CcxError> {
    let mut stmt = conn.prepare(
        "SELECT write_lease_id, work_execution_id, last_heartbeat_at
         FROM write_leases
         WHERE project_id = ?1 AND state = 'active'",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut stale = Vec::new();
    for (id, we_id, heartbeat) in rows {
        let secs = stale_seconds(&heartbeat, now);
        if secs >= LEASE_STALE_SECS {
            stale.push(StaleLeaseEntry {
                write_lease_id: id,
                work_execution_id: we_id,
                last_heartbeat_at: heartbeat,
                stale_seconds: secs,
            });
        }
    }
    Ok(stale)
}

fn query_stale_locks(
    conn: &Connection,
    project_id: &str,
    now: &DateTime<Utc>,
) -> Result<Vec<StaleLockEntry>, CcxError> {
    let mut stmt = conn.prepare(
        "SELECT merge_lock_id, work_execution_id, last_heartbeat_at
         FROM merge_locks
         WHERE project_id = ?1 AND state = 'active'",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut stale = Vec::new();
    for (id, we_id, heartbeat) in rows {
        let secs = stale_seconds(&heartbeat, now);
        if secs >= LOCK_STALE_SECS {
            stale.push(StaleLockEntry {
                merge_lock_id: id,
                work_execution_id: we_id,
                last_heartbeat_at: heartbeat,
                stale_seconds: secs,
            });
        }
    }
    Ok(stale)
}

pub fn run_digest(project_id: &str, dir: &Utf8Path) -> Result<RecoveryDigest, CcxError> {
    let conn = open_db(dir)?;
    let now = Utc::now();

    let active_sessions = query_active_sessions(&conn, project_id)?;
    let active_count = active_sessions.len();

    let orphaned: Vec<String> = active_sessions
        .iter()
        .filter(|s| !tmux_session_exists(&s.agent_session_id))
        .map(|s| s.agent_session_id.clone())
        .collect();

    let stale_leases = query_stale_leases(&conn, project_id, &now)?;
    let stale_merge_locks = query_stale_locks(&conn, project_id, &now)?;
    let sqlite_dirty = is_dirty(dir);

    Ok(RecoveryDigest {
        project_id: project_id.to_owned(),
        timestamp: now.to_rfc3339(),
        diagnostics: Diagnostics {
            active_sessions: active_count,
            orphaned_tmux_sessions: orphaned,
            stale_leases,
            stale_merge_locks,
            sqlite_dirty,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn digest_on_empty_db_returns_clean_state() {
        let tmp = tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let project_id = crate::domain::event::generate_id().to_string();
        let result = run_digest(&project_id, &dir).unwrap();
        assert_eq!(result.diagnostics.active_sessions, 0);
        assert!(result.diagnostics.orphaned_tmux_sessions.is_empty());
        assert!(result.diagnostics.stale_leases.is_empty());
        assert!(result.diagnostics.stale_merge_locks.is_empty());
        assert!(!result.diagnostics.sqlite_dirty);
    }

    #[test]
    fn stale_seconds_parses_rfc3339() {
        let then = Utc::now() - chrono::Duration::seconds(400);
        let secs = stale_seconds(&then.to_rfc3339(), &Utc::now());
        assert!(secs >= 399 && secs <= 401);
    }

    #[test]
    fn stale_seconds_corrupted_timestamp_is_sentinel() {
        let secs = stale_seconds("not-a-timestamp", &Utc::now());
        assert_eq!(secs, STALE_SECS_CORRUPTED);
        assert!(secs >= LEASE_STALE_SECS); // always triggers stale threshold
    }

    #[test]
    fn stale_seconds_future_timestamp_clamps_to_zero() {
        let future = (Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339();
        let secs = stale_seconds(&future, &Utc::now());
        assert_eq!(secs, 0);
    }
}

use camino::Utf8PathBuf;
use rusqlite::Connection;

use crate::agent_runtime::cmux_adapter::make_adapter;
use crate::agent_runtime::tmux_adapter::ShellTmuxAdapter;
use crate::agent_runtime::tmux_adapter::TmuxAdapter;
use crate::config::project_config::CleanupPolicy;
use crate::domain::event::{
    Actor, CleanupCompletedPayload, CleanupStartedPayload, Event, EventData,
};
use crate::error::CcxError;
use crate::git::worktree::remove_worktree;
use crate::persistence::jsonl::append_event_to_dir;
use crate::persistence::sqlite::open_db;

pub struct CleanupConfig {
    pub project_id: String,
    pub project_dir: Utf8PathBuf,
    pub work_execution_id: String,
    pub cleanup_policy: CleanupPolicy,
    pub keep_last_n: u32,
    pub keep_for_days: u64,
    pub canonical_repo: Utf8PathBuf,
}

pub struct CleanupResult {
    pub removed_worktree: bool,
    pub closed_sessions: Vec<String>,
}

struct SessionRow {
    agent_session_id: String,
    tmux_session_id: String,
    cmux_tab_id: String,
}

struct WeRow {
    worktree_path: String,
    selected_at: String,
}

fn query_we(conn: &Connection, project_id: &str, work_execution_id: &str) -> Result<WeRow, CcxError> {
    conn.query_row(
        "SELECT worktree_path, selected_at FROM work_executions
         WHERE work_execution_id = ?1 AND project_id = ?2",
        rusqlite::params![work_execution_id, project_id],
        |row| Ok(WeRow { worktree_path: row.get(0)?, selected_at: row.get(1)? }),
    )
    .map_err(|e| {
        if e == rusqlite::Error::QueryReturnedNoRows {
            CcxError::Other(anyhow::anyhow!("work execution not found: {work_execution_id}"))
        } else {
            CcxError::Database(e.to_string())
        }
    })
}

fn query_sessions(conn: &Connection, work_execution_id: &str) -> Result<Vec<SessionRow>, CcxError> {
    let mut stmt = conn.prepare(
        "SELECT agent_session_id, tmux_session_id, cmux_tab_id
         FROM agent_sessions
         WHERE work_execution_id = ?1
           AND state NOT IN ('exited', 'lost', 'detached')",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![work_execution_id], |row| {
            Ok(SessionRow {
                agent_session_id: row.get(0)?,
                tmux_session_id: row.get(1)?,
                cmux_tab_id: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Returns true if the worktree should be deleted given the policy.
fn should_remove_worktree(
    conn: &Connection,
    project_id: &str,
    work_execution_id: &str,
    selected_at: &str,
    policy: &CleanupPolicy,
    keep_last_n: u32,
    keep_for_days: u64,
) -> bool {
    match policy {
        CleanupPolicy::Immediate => true,

        CleanupPolicy::KeepLastN => {
            // Preserve the most recent `keep_last_n` WEs; remove everything else.
            // On DB error, preserve (return false) to avoid accidental deletion.
            let ids: Vec<String> = match conn
                .prepare(
                    "SELECT work_execution_id FROM work_executions
                     WHERE project_id = ?1
                     ORDER BY selected_at DESC
                     LIMIT ?2",
                )
                .and_then(|mut s| {
                    s.query_map(rusqlite::params![project_id, keep_last_n], |row| row.get(0))
                        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
                }) {
                Ok(v) => v,
                Err(_) => return false,
            };
            !ids.iter().any(|id| id == work_execution_id)
        }

        CleanupPolicy::KeepForDuration => {
            let age_secs = chrono::DateTime::parse_from_rfc3339(selected_at)
                .map(|ts| {
                    (chrono::Utc::now() - ts.with_timezone(&chrono::Utc)).num_seconds()
                })
                .unwrap_or(0);
            let threshold_secs = keep_for_days as i64 * 86_400;
            age_secs >= threshold_secs
        }
    }
}

pub fn run_cleanup(config: &CleanupConfig) -> Result<CleanupResult, CcxError> {
    let conn = open_db(&config.project_dir)?;

    let we = query_we(&conn, &config.project_id, &config.work_execution_id)?;
    let sessions = query_sessions(&conn, &config.work_execution_id)?;

    // Emit CleanupStarted before taking any destructive action.
    let start_event = Event::new(
        &config.project_id,
        Actor::Controller,
        EventData::CleanupStarted(CleanupStartedPayload {
            work_execution_id: config.work_execution_id.clone(),
        }),
    );
    append_event_to_dir(&config.project_dir, &start_event)?;

    // Kill tmux sessions and close cmux tabs (both best-effort).
    let tmux = ShellTmuxAdapter;
    let cmux = make_adapter();
    let mut closed_sessions = Vec::new();

    for s in &sessions {
        let tmux_ok = tmux.kill_session(&s.tmux_session_id).is_ok();
        let cmux_ok = cmux.close_tab(&s.cmux_tab_id).is_ok();
        if tmux_ok || cmux_ok {
            closed_sessions.push(s.agent_session_id.clone());
        }
    }

    // Decide whether to remove the worktree based on cleanup policy.
    let remove = should_remove_worktree(
        &conn,
        &config.project_id,
        &config.work_execution_id,
        &we.selected_at,
        &config.cleanup_policy,
        config.keep_last_n,
        config.keep_for_days,
    );

    let mut removed_worktree = false;
    if remove && !we.worktree_path.is_empty() {
        let wt_path = camino::Utf8PathBuf::from(&we.worktree_path);
        if wt_path.exists() {
            remove_worktree(&config.canonical_repo, &wt_path)?;
            removed_worktree = true;
        }
    }

    // Emit CleanupCompleted.
    let done_event = Event::new(
        &config.project_id,
        Actor::Controller,
        EventData::CleanupCompleted(CleanupCompletedPayload {
            work_execution_id: config.work_execution_id.clone(),
            removed_worktree,
            closed_sessions: closed_sessions.clone(),
        }),
    );
    append_event_to_dir(&config.project_dir, &done_event)?;

    Ok(CleanupResult { removed_worktree, closed_sessions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::project_config::CleanupPolicy;
    use tempfile::tempdir;

    fn open_test_db(dir: &camino::Utf8Path) -> Connection {
        crate::persistence::sqlite::open_db(dir).unwrap()
    }

    fn ensure_project(conn: &Connection, project_id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO projects
             (project_id, display_slug, canonical_repo, task_source_file, created_at)
             VALUES (?1, 'test', '/tmp/repo', '/tmp/repo/tasks.md', '2026-01-01T00:00:00Z')",
            rusqlite::params![project_id],
        )
        .unwrap();
    }

    fn insert_we(conn: &Connection, project_id: &str, we_id: &str, selected_at: &str) {
        ensure_project(conn, project_id);
        conn.execute(
            "INSERT INTO work_executions
             (work_execution_id, project_id, state, branch_name, worktree_path,
              task_file_path, source_path, selector_type, selector_value,
              display_text, source_file_hash, selected_at)
             VALUES (?1, ?2, 'merged', 'ccx/test', '', 'task.md',
                     'src.md', 'whole_file', '*', 'test', 'abc', ?3)",
            rusqlite::params![we_id, project_id, selected_at],
        )
        .unwrap();
    }

    #[test]
    fn immediate_policy_always_removes() {
        let tmp = tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_test_db(&dir);
        let pid = "01JTEST00000000000000000001";
        let we_id = "01JTEST00000000000000000002";
        insert_we(&conn, pid, we_id, "2026-01-01T00:00:00Z");
        assert!(should_remove_worktree(
            &conn, pid, we_id, "2026-01-01T00:00:00Z",
            &CleanupPolicy::Immediate, 5, 7
        ));
    }

    #[test]
    fn keep_last_n_preserves_recent_we() {
        let tmp = tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_test_db(&dir);
        let pid = "01JTEST00000000000000000001";
        // Insert 3 WEs; keep_last_n=5 means all are preserved.
        insert_we(&conn, pid, "01JTEST00000000000000000002", "2026-01-01T00:00:00Z");
        insert_we(&conn, pid, "01JTEST00000000000000000003", "2026-01-02T00:00:00Z");
        insert_we(&conn, pid, "01JTEST00000000000000000004", "2026-01-03T00:00:00Z");
        assert!(!should_remove_worktree(
            &conn, pid, "01JTEST00000000000000000002", "2026-01-01T00:00:00Z",
            &CleanupPolicy::KeepLastN, 5, 7
        ));
    }

    #[test]
    fn keep_last_n_removes_old_we() {
        let tmp = tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_test_db(&dir);
        let pid = "01JTEST00000000000000000001";
        insert_we(&conn, pid, "01JTEST00000000000000000002", "2026-01-01T00:00:00Z");
        insert_we(&conn, pid, "01JTEST00000000000000000003", "2026-01-02T00:00:00Z");
        insert_we(&conn, pid, "01JTEST00000000000000000004", "2026-01-03T00:00:00Z");
        // keep_last_n=2: the oldest WE (002) is outside the window.
        assert!(should_remove_worktree(
            &conn, pid, "01JTEST00000000000000000002", "2026-01-01T00:00:00Z",
            &CleanupPolicy::KeepLastN, 2, 7
        ));
    }

    #[test]
    fn keep_for_duration_removes_old_we() {
        let old_date = "2020-01-01T00:00:00Z"; // definitely older than 7 days
        let tmp = tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_test_db(&dir);
        assert!(should_remove_worktree(
            &conn, "pid", "we_id", old_date,
            &CleanupPolicy::KeepForDuration, 5, 7
        ));
    }

    #[test]
    fn keep_for_duration_preserves_recent_we() {
        let recent = chrono::Utc::now().to_rfc3339();
        let tmp = tempdir().unwrap();
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_test_db(&dir);
        assert!(!should_remove_worktree(
            &conn, "pid", "we_id", &recent,
            &CleanupPolicy::KeepForDuration, 5, 7
        ));
    }
}

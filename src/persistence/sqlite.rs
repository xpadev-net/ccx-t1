use camino::Utf8Path;
use rusqlite::Connection;

use crate::error::CcxError;

/// DDL migrations — idempotent (all CREATE TABLE IF NOT EXISTS).
/// Includes the full schema from spec section 2.4 plus `last_applied_event_id`.
const MIGRATIONS: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT PRIMARY KEY,
    display_slug TEXT NOT NULL,
    canonical_repo TEXT NOT NULL,
    task_source_file TEXT NOT NULL,
    sqlite_dirty INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    last_applied_event_id TEXT
);

CREATE TABLE IF NOT EXISTS work_executions (
    work_execution_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN (
        'created', 'task_file_created', 'dispatched', 'running', 'pr_open',
        'gate_check', 'review_fixing', 'merge_ready', 'merging', 'merged',
        'followup_required', 'returned', 'blocked', 'failed', 'hold', 'canceled', 'superseded'
    )),
    branch_name TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    task_file_path TEXT NOT NULL,
    pr_number INTEGER,
    pr_url TEXT,
    head_commit TEXT,
    source_path TEXT NOT NULL,
    selector_type TEXT NOT NULL,
    selector_value TEXT NOT NULL,
    display_text TEXT NOT NULL,
    source_file_hash TEXT NOT NULL,
    selected_at TEXT NOT NULL,
    artifact_state TEXT NOT NULL DEFAULT 'pending' CHECK(artifact_state IN ('pending', 'ready', 'invalid')),
    artifact_checked_at TEXT,
    sync_status TEXT NOT NULL DEFAULT 'pending' CHECK(sync_status IN ('pending', 'success', 'aborted')),
    sync_warning TEXT,
    FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS agent_sessions (
    agent_session_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    work_execution_id TEXT,
    state TEXT NOT NULL CHECK(state IN (
        'starting', 'running', 'idle', 'hung', 'stopping', 'exited', 'lost', 'detached'
    )),
    role TEXT NOT NULL CHECK(role IN ('orchestrator', 'worker', 'reviewer', 'diagnostic')),
    attach_mode TEXT CHECK(attach_mode IS NULL OR attach_mode IN ('writer', 'reviewer', 'observer', 'diagnostic')),
    cmux_tab_id TEXT NOT NULL,
    tmux_session_id TEXT NOT NULL,
    pid INTEGER,
    cwd TEXT NOT NULL,
    started_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL,
    exit_code INTEGER,
    FOREIGN KEY(project_id) REFERENCES projects(project_id),
    FOREIGN KEY(work_execution_id) REFERENCES work_executions(work_execution_id),
    CHECK(
        (role = 'orchestrator' AND work_execution_id IS NULL AND attach_mode IS NULL)
        OR
        (role <> 'orchestrator' AND work_execution_id IS NOT NULL AND attach_mode IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_orchestrator_per_project
ON agent_sessions(project_id)
WHERE role = 'orchestrator'
  AND state IN ('starting', 'running', 'idle');

CREATE TABLE IF NOT EXISTS write_leases (
    write_lease_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    work_execution_id TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    writer_agent_session_id TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active', 'stale', 'released', 'revoked')),
    FOREIGN KEY(project_id) REFERENCES projects(project_id),
    FOREIGN KEY(work_execution_id) REFERENCES work_executions(work_execution_id),
    FOREIGN KEY(writer_agent_session_id) REFERENCES agent_sessions(agent_session_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_write_lease_per_work_execution
ON write_leases(work_execution_id)
WHERE state = 'active';

CREATE TABLE IF NOT EXISTS merge_locks (
    merge_lock_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    owner_agent_session_id TEXT NOT NULL,
    work_execution_id TEXT NOT NULL,
    pr_number INTEGER NOT NULL,
    acquired_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active', 'stale', 'released')),
    FOREIGN KEY(project_id) REFERENCES projects(project_id),
    FOREIGN KEY(owner_agent_session_id) REFERENCES agent_sessions(agent_session_id),
    FOREIGN KEY(work_execution_id) REFERENCES work_executions(work_execution_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_merge_lock_per_project
ON merge_locks(project_id)
WHERE state = 'active';
"#;

/// Open (or create) the project `state.sqlite` database and run migrations.
pub fn open_db(dir: &Utf8Path) -> Result<Connection, CcxError> {
    std::fs::create_dir_all(dir)?;
    let db_path = dir.join("state.sqlite");
    let conn = Connection::open(db_path.as_std_path())?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    run_migrations(&conn)?;
    Ok(conn)
}

/// Run DDL migrations (idempotent).
pub fn run_migrations(conn: &Connection) -> Result<(), CcxError> {
    conn.execute_batch(MIGRATIONS)?;
    Ok(())
}

fn dirty_marker_path(dir: &Utf8Path) -> camino::Utf8PathBuf {
    dir.join("state").join("sqlite.dirty")
}

/// Mark the SQLite view as dirty.
/// Creates the `state/sqlite.dirty` marker file and, if the DB is reachable,
/// sets `projects.sqlite_dirty = 1` for the given project.
pub fn mark_dirty(dir: &Utf8Path, project_id: &str) {
    let marker = dirty_marker_path(dir);
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&marker, b"dirty") {
        tracing::warn!(
            error = %e,
            path = %marker,
            project_id,
            "mark_dirty: failed to write dirty marker file — dirty state may be lost"
        );
    }

    // Best-effort: update the DB column if the DB is reachable.
    if let Ok(conn) = Connection::open(dir.join("state.sqlite").as_std_path()) {
        let _ = conn.execute(
            "UPDATE projects SET sqlite_dirty = 1 WHERE project_id = ?1",
            rusqlite::params![project_id],
        );
    }
}

/// Clear the dirty flag: delete the marker file and reset `sqlite_dirty = 0`.
pub fn clear_dirty(dir: &Utf8Path, conn: &Connection, project_id: &str) -> Result<(), CcxError> {
    let marker = dirty_marker_path(dir);
    if marker.exists() {
        std::fs::remove_file(marker.as_std_path())?;
    }
    conn.execute(
        "UPDATE projects SET sqlite_dirty = 0 WHERE project_id = ?1",
        rusqlite::params![project_id],
    )?;
    Ok(())
}

/// Returns `true` if the marker file exists (the definitive dirty signal).
pub fn is_dirty(dir: &Utf8Path) -> bool {
    dirty_marker_path(dir).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir =
            camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_db(&dir).unwrap();
        // Running migrations a second time must not fail.
        run_migrations(&conn).unwrap();
    }

    #[test]
    fn dirty_flag_lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        let dir =
            camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let conn = open_db(&dir).unwrap();
        let fake_id = "01JTEST00000000000000000001";

        assert!(!is_dirty(&dir));
        mark_dirty(&dir, fake_id);
        assert!(is_dirty(&dir));
        // clear requires the project row to exist (UPDATE is a no-op if absent),
        // but the marker file is what matters.
        clear_dirty(&dir, &conn, fake_id).unwrap();
        assert!(!is_dirty(&dir));
    }
}

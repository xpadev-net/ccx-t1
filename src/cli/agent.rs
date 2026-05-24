use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use clap::Args;

use crate::agent_runtime::cmux_adapter::make_adapter;
use crate::agent_runtime::env_builder::{build_agent_envs, AgentEnvInput};
use crate::agent_runtime::launch::{launch_agent, LaunchResult, LaunchSpec};
use crate::agent_runtime::lifecycle::{handle_lifecycle_stop, LifecycleStopConfig};
use crate::agent_runtime::tmux_adapter::{ShellTmuxAdapter, TmuxAdapter};
use crate::config::{ccx_home, project_dir};
use crate::domain::event::{generate_id, Actor, AgentSessionStoppedPayload, Event, EventData};
use crate::error::CcxError;
use crate::persistence::jsonl::append_event_to_dir;
use crate::persistence::sqlite::open_db;

// ---------------------------------------------------------------------------
// agent start-orchestrator
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct StartOrchestratorArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn start_orchestrator(args: StartOrchestratorArgs) -> Result<(), CcxError> {
    let session_id = generate_id().to_string();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "agent_session_id": session_id,
                "project_id": args.project_id,
                "role": "orchestrator",
                "status": "started",
            }))?
        );
    } else {
        println!("agent_session_id: {session_id}  (skeleton — not yet implemented)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent attach
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct AttachArgs {
    #[arg(long)]
    pub project_id: Option<String>,
    #[arg(long)]
    pub work_execution_id: String,
    /// worker | reviewer | diagnostic
    #[arg(long)]
    pub role: String,
    /// writer | reviewer | observer | diagnostic
    #[arg(long)]
    pub mode: String,
    #[arg(long)]
    pub json: bool,
}

pub fn attach(args: AttachArgs) -> Result<(), CcxError> {
    let home = ccx_home()?;
    let cli_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok())
        .unwrap_or_else(|| "ccx".into());
    let tmux = ShellTmuxAdapter;
    let cmux = make_adapter();
    attach_with_adapters(args, &home, &cli_path, &tmux, cmux.as_ref())
}

fn attach_with_adapters(
    args: AttachArgs,
    home: &Utf8Path,
    cli_path: &str,
    tmux: &dyn TmuxAdapter,
    cmux: &dyn crate::agent_runtime::cmux_adapter::CmuxAdapter,
) -> Result<(), CcxError> {
    let session_id = generate_id().to_string();
    let resolved =
        resolve_attach_work_execution(home, args.project_id.as_deref(), &args.work_execution_id)?;
    let envs = build_agent_envs(&AgentEnvInput {
        project_id: &resolved.project_id,
        work_execution_id: Some(&args.work_execution_id),
        agent_session_id: &session_id,
        worktree_path: Some(resolved.worktree_path.as_str()),
        task_file: Some(resolved.task_file_path.as_str()),
        cli_path,
        canonical_repo: resolved.canonical_repo.as_str(),
        role: &args.role,
        attach_mode: Some(&args.mode),
    });

    let result = launch_agent(
        &LaunchSpec {
            agent_session_id: session_id.clone(),
            project_id: resolved.project_id.clone(),
            project_dir: resolved.project_dir,
            work_execution_id: Some(args.work_execution_id.clone()),
            role: args.role.clone(),
            attach_mode: Some(args.mode.clone()),
            cwd_path: PathBuf::from(resolved.canonical_repo.as_str()),
            worktree_path: Some(PathBuf::from(resolved.worktree_path.as_str())),
            envs,
            display_slug: resolved.display_slug,
            canonical_repo: resolved.canonical_repo,
        },
        tmux,
        cmux,
    )?;

    print_attach_result(args, &session_id, &result)
}

fn print_attach_result(
    args: AttachArgs,
    session_id: &str,
    result: &LaunchResult,
) -> Result<(), CcxError> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "agent_session_id": session_id,
                "work_execution_id": args.work_execution_id,
                "role": args.role,
                "mode": args.mode,
                "tmux_session_id": result.tmux_session_id,
                "cmux_workspace_id": result.cmux_workspace_id,
                "cmux_tab_id": result.cmux_tab_id,
                "status": "attached",
            }))?
        );
    } else {
        println!(
            "agent_session_id: {session_id}  tmux: {}  cmux_tab: {}",
            result.tmux_session_id, result.cmux_tab_id
        );
    }
    Ok(())
}

struct ResolvedAttachWorkExecution {
    project_id: String,
    project_dir: Utf8PathBuf,
    display_slug: String,
    canonical_repo: String,
    worktree_path: String,
    task_file_path: String,
}

fn resolve_attach_work_execution(
    home: &Utf8Path,
    project_id: Option<&str>,
    work_execution_id: &str,
) -> Result<ResolvedAttachWorkExecution, CcxError> {
    if let Some(project_id) = project_id {
        let project_dir = home.join("projects").join(project_id);
        if !project_dir.join("state.sqlite").exists() {
            return Err(CcxError::Other(anyhow::anyhow!(
                "project not found: {project_id}"
            )));
        }
        return query_attach_work_execution(project_dir, work_execution_id).and_then(|found| {
            found.ok_or_else(|| {
                CcxError::Other(anyhow::anyhow!(
                    "work execution not found: {work_execution_id}"
                ))
            })
        });
    }

    let projects_dir = home.join("projects");
    let entries = std::fs::read_dir(&projects_dir).map_err(|e| {
        CcxError::Other(anyhow::anyhow!(
            "failed to read projects directory {}: {e}",
            projects_dir
        ))
    })?;

    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let project_dir = match Utf8PathBuf::try_from(entry.path()) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !project_dir.join("state.sqlite").exists() {
            continue;
        }
        match query_attach_work_execution(project_dir, work_execution_id) {
            Ok(Some(found)) => return Ok(found),
            Ok(None) | Err(_) => continue,
        }
    }

    Err(CcxError::Other(anyhow::anyhow!(
        "work execution not found: {work_execution_id}"
    )))
}

fn query_attach_work_execution(
    project_dir: Utf8PathBuf,
    work_execution_id: &str,
) -> Result<Option<ResolvedAttachWorkExecution>, CcxError> {
    let conn = open_db(&project_dir)?;
    let mut stmt = conn.prepare(
        "SELECT p.project_id, p.display_slug, p.canonical_repo, w.worktree_path, w.task_file_path
           FROM work_executions w
           JOIN projects p ON p.project_id = w.project_id
          WHERE w.work_execution_id = ?1",
    )?;
    let mut rows = stmt.query(rusqlite::params![work_execution_id])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(ResolvedAttachWorkExecution {
            project_id: row.get(0)?,
            project_dir,
            display_slug: row.get(1)?,
            canonical_repo: row.get(2)?,
            worktree_path: row.get(3)?,
            task_file_path: row.get(4)?,
        }));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// agent prompt
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct PromptArgs {
    #[arg(long)]
    pub session_id: String,
    /// Inline message text (mutually exclusive with --message-file and --stdin)
    #[arg(long, conflicts_with_all = ["message_file", "stdin"])]
    pub message: Option<String>,
    /// Path to a file whose content is used as the prompt
    #[arg(long, conflicts_with_all = ["message", "stdin"])]
    pub message_file: Option<String>,
    /// Read prompt from stdin
    #[arg(long, conflicts_with_all = ["message", "message_file"])]
    pub stdin: bool,
    #[arg(long)]
    pub json: bool,
}

pub fn prompt(args: PromptArgs) -> Result<(), CcxError> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": args.session_id,
                "status": "sent",
            }))?
        );
    } else {
        println!(
            "prompt sent to {} (skeleton — not yet implemented)",
            args.session_id
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent stop
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct StopArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn stop(args: StopArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let conn = open_db(&dir)?;

    let cmux_tab_id: String = conn
        .query_row(
            "SELECT cmux_tab_id FROM agent_sessions WHERE agent_session_id = ?1",
            rusqlite::params![args.session_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let msg = if e == rusqlite::Error::QueryReturnedNoRows {
                format!("agent session not found: {}", args.session_id)
            } else {
                format!("failed to query agent session {}: {e}", args.session_id)
            };
            CcxError::Other(anyhow::anyhow!("{msg}"))
        })?;

    // Write the event first so the record is durable before any destructive action.
    // If the event write fails, nothing has been destroyed and the caller can retry.
    let event = Event::new(
        &args.project_id,
        Actor::Controller,
        EventData::AgentSessionStopped(AgentSessionStoppedPayload {
            agent_session_id: args.session_id.clone(),
            exit_code: None,
        }),
    );
    append_event_to_dir(&dir, &event)?;

    let tmux = ShellTmuxAdapter;
    let kill_result = tmux.kill_session(&args.session_id);
    // close_tab is best-effort and runs unconditionally so the tab is cleaned up
    // even when kill_session returns an unexpected error.
    let cmux = make_adapter();
    let _ = cmux.close_tab(&cmux_tab_id);
    kill_result?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": args.session_id,
                "status": "stopped",
            }))?
        );
    } else {
        println!("stopped {}", args.session_id);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent notify
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct NotifyArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub message: String,
    /// info | warning
    #[arg(long, default_value = "info")]
    pub level: String,
    #[arg(long)]
    pub json: bool,
}

pub fn notify(args: NotifyArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let conn = open_db(&dir)?;

    let cmux_tab_id: String = conn
        .query_row(
            "SELECT cmux_tab_id FROM agent_sessions WHERE agent_session_id = ?1",
            rusqlite::params![args.session_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let msg = if e == rusqlite::Error::QueryReturnedNoRows {
                format!("agent session not found: {}", args.session_id)
            } else {
                format!("failed to query agent session {}: {e}", args.session_id)
            };
            CcxError::Other(anyhow::anyhow!("{msg}"))
        })?;

    let cmux = make_adapter();
    cmux.notify_user(&cmux_tab_id, &args.message, &args.level)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": args.session_id,
                "level": args.level,
                "status": "notified",
            }))?
        );
    } else {
        println!("[{}] {} -> {}", args.level, args.session_id, args.message);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// agent lifecycle-stop
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct LifecycleStopArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub agent_session_id: String,
    #[arg(long)]
    pub work_execution_id: String,
    /// Path to the task.md file for this work execution
    #[arg(long)]
    pub task_file: PathBuf,
    /// Raw agent session ID of the orchestrator to notify (optional)
    #[arg(long)]
    pub orchestrator_session_id: Option<String>,
    #[arg(long)]
    pub json: bool,
}

pub fn lifecycle_stop(args: LifecycleStopArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;

    let config = LifecycleStopConfig {
        project_id: args.project_id.clone(),
        project_dir: dir,
        agent_session_id: args.agent_session_id.clone(),
        work_execution_id: args.work_execution_id.clone(),
        task_file_path: args.task_file,
        orchestrator_session_id: args.orchestrator_session_id,
    };
    handle_lifecycle_stop(&config)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "agent_session_id": args.agent_session_id,
                "work_execution_id": args.work_execution_id,
                "status": "lifecycle_stop_recorded",
            }))?
        );
    } else {
        println!(
            "lifecycle_stop recorded for session {} work_execution {}",
            args.agent_session_id, args.work_execution_id
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::cmux_adapter::HeadlessCmuxAdapter;
    use crate::error::CcxError;
    use crate::persistence::sqlite::open_db;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    struct CaptureTmuxAdapter {
        envs: Mutex<Option<HashMap<String, String>>>,
        cwd: Mutex<Option<PathBuf>>,
    }

    impl CaptureTmuxAdapter {
        fn new() -> Self {
            Self {
                envs: Mutex::new(None),
                cwd: Mutex::new(None),
            }
        }
    }

    impl TmuxAdapter for CaptureTmuxAdapter {
        fn create_session(
            &self,
            _session_id: &str,
            cwd: &Path,
            envs: &HashMap<String, String>,
        ) -> Result<(), CcxError> {
            *self.cwd.lock().unwrap() = Some(cwd.to_path_buf());
            *self.envs.lock().unwrap() = Some(envs.clone());
            Ok(())
        }

        fn kill_session(&self, _session_id: &str) -> Result<(), CcxError> {
            Ok(())
        }

        fn session_exists(&self, _session_id: &str) -> Result<bool, CcxError> {
            Ok(false)
        }

        fn get_pane_pid(&self, _session_id: &str) -> Result<Option<u32>, CcxError> {
            Ok(None)
        }

        fn get_pane_cwd(&self, _session_id: &str) -> Result<Option<String>, CcxError> {
            Ok(None)
        }

        fn send_keys(&self, _session_id: &str, _keys: &str) -> Result<(), CcxError> {
            Ok(())
        }

        fn send_literal(&self, _session_id: &str, _text: &str) -> Result<(), CcxError> {
            Ok(())
        }
    }

    fn seed_project(home: &Utf8Path) -> Result<(), CcxError> {
        let project_id = "01JTEST00000000000000000001";
        let project_dir = home.join("projects").join(project_id);
        std::fs::create_dir_all(&project_dir)?;
        let conn = open_db(&project_dir)?;
        conn.execute(
            "INSERT INTO projects (
                project_id, display_slug, canonical_repo, task_source_file, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                project_id,
                "my-project",
                "/repos/myproject",
                "/repos/myproject/tasks.md",
                "2026-05-25T00:00:00Z",
            ],
        )?;
        conn.execute(
            "INSERT INTO work_executions (
                work_execution_id, project_id, state, branch_name, worktree_path,
                task_file_path, source_path, selector_type, selector_value,
                display_text, source_file_hash, selected_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                "01JTEST00000000000000000002",
                project_id,
                "created",
                "codex/test",
                "/repos/myproject/.ccx/we-001",
                "/repos/myproject/.ccx/we-001/task.md",
                "/repos/myproject/tasks.md",
                "heading",
                "Task",
                "Task",
                "hash",
                "2026-05-25T00:00:00Z",
            ],
        )?;
        Ok(())
    }

    #[test]
    fn attach_injects_all_agent_envs_into_tmux_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        seed_project(&home).unwrap();
        let tmux = CaptureTmuxAdapter::new();

        attach_with_adapters(
            AttachArgs {
                project_id: Some("01JTEST00000000000000000001".into()),
                work_execution_id: "01JTEST00000000000000000002".into(),
                role: "worker".into(),
                mode: "writer".into(),
                json: true,
            },
            &home,
            "/usr/local/bin/ccx",
            &tmux,
            &HeadlessCmuxAdapter,
        )
        .unwrap();

        let envs = tmux.envs.lock().unwrap().clone().expect("tmux envs");
        assert_eq!(envs.len(), 9);
        assert_eq!(envs["CCX_PROJECT_ID"], "01JTEST00000000000000000001");
        assert_eq!(envs["CCX_WORK_EXECUTION_ID"], "01JTEST00000000000000000002");
        assert!(envs["CCX_AGENT_SESSION_ID"].len() >= 26);
        assert_eq!(envs["CCX_WORKTREE_PATH"], "/repos/myproject/.ccx/we-001");
        assert_eq!(
            envs["CCX_TASK_FILE"],
            "/repos/myproject/.ccx/we-001/task.md"
        );
        assert_eq!(envs["CCX_CLI"], "/usr/local/bin/ccx");
        assert_eq!(envs["CCX_CANONICAL_REPO"], "/repos/myproject");
        assert_eq!(envs["CCX_ROLE"], "worker");
        assert_eq!(envs["CCX_ATTACH_MODE"], "writer");

        let cwd = tmux.cwd.lock().unwrap().clone().expect("tmux cwd");
        assert_eq!(cwd, PathBuf::from("/repos/myproject/.ccx/we-001"));
    }
}

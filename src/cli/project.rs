use std::fs::{self, OpenOptions};
use std::io::ErrorKind;

use crate::config::project_config::{CleanupPolicy, GhReviewHook, ProjectConfig};
use crate::config::{ccx_home, project_dir};
use crate::domain::event::{Actor, Event, EventData, ProjectRegisteredPayload, generate_id};
use crate::error::CcxError;
use crate::persistence::jsonl::append_event;
use camino::Utf8PathBuf;
use clap::Args;
use fd_lock::RwLock;

/// Derive a display slug from a path: strip leading '/', replace '/' with '-'.
fn path_to_slug(path: &Utf8PathBuf) -> String {
    path.as_str()
        .trim_start_matches('/')
        .replace('/', "-")
}

#[derive(Debug, Args)]
pub struct RegisterArgs {
    #[arg(long)]
    pub canonical_repo: Utf8PathBuf,
    #[arg(long)]
    pub task_source_file: Utf8PathBuf,
}

pub fn register(args: RegisterArgs) -> Result<(), CcxError> {
    let project_id = generate_id().to_string();
    let display_slug = path_to_slug(&args.canonical_repo);
    let created_at = chrono::Utc::now().to_rfc3339();

    let config = ProjectConfig {
        project_id: project_id.clone(),
        display_slug,
        canonical_repo: args.canonical_repo,
        task_source_file: args.task_source_file,
        gh_review_hook: GhReviewHook::default(),
        cleanup_policy: CleanupPolicy::default(),
        created_at,
    };

    let home = ccx_home()?;
    fs::create_dir_all(&home)?;

    // Step 1: append the audit event FIRST so the event log is the source of truth.
    // If subsequent writes fail the event is orphaned, but the project can be
    // reconstructed or the partial state detected via reconciliation.
    let dir = project_dir(&project_id)?;
    fs::create_dir_all(&dir)?;
    let event = Event::new(
        &config.project_id,
        Actor::Controller,
        EventData::ProjectRegistered(ProjectRegisteredPayload {
            display_slug: config.display_slug.clone(),
            canonical_repo: config.canonical_repo.to_string(),
            task_source_file: config.task_source_file.to_string(),
        }),
    );
    append_event(&config.project_id, &event)?;

    // Step 2: persist project config.
    let config_path = dir.join("project.json");
    let json = serde_json::to_string_pretty(&config)?;
    fs::write(&config_path, json)?;

    // Step 3: update the global projects.json index under an exclusive lock.
    update_projects_index(&home, &config)?;

    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

/// Atomically update `<ccx_home>/projects.json` using an exclusive fd-lock.
fn update_projects_index(home: &Utf8PathBuf, config: &ProjectConfig) -> Result<(), CcxError> {
    let lock_path = home.join("projects.lock");
    let index_path = home.join("projects.json");

    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    let mut rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.write()?;

    let mut index: Vec<serde_json::Value> = match fs::read_to_string(&index_path) {
        Ok(raw) => serde_json::from_str(&raw)?,
        Err(e) if e.kind() == ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.into()),
    };

    index.push(serde_json::json!({
        "project_id": config.project_id,
        "display_slug": config.display_slug,
        "canonical_repo": config.canonical_repo,
    }));
    fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;
    Ok(())
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long)]
    pub json: bool,
}

pub fn list(args: ListArgs) -> Result<(), CcxError> {
    let index_path = ccx_home()?.join("projects.json");
    let projects: Vec<serde_json::Value> = match fs::read_to_string(&index_path) {
        Ok(raw) => serde_json::from_str(&raw)?,
        Err(e) if e.kind() == ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.into()),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&projects)?);
    } else {
        for p in &projects {
            println!(
                "{}\t{}\t{}",
                p["project_id"].as_str().unwrap_or(""),
                p["display_slug"].as_str().unwrap_or(""),
                p["canonical_repo"].as_str().unwrap_or(""),
            );
        }
    }
    Ok(())
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    pub project_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn status(args: StatusArgs) -> Result<(), CcxError> {
    let dir = project_dir(&args.project_id)?;
    let config_path = dir.join("project.json");
    let raw = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(CcxError::ProjectNotFound {
                project_id: args.project_id,
            })
        }
        Err(e) => return Err(e.into()),
    };
    let config: ProjectConfig = serde_json::from_str(&raw)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&config)?);
    } else {
        println!("project_id:  {}", config.project_id);
        println!("slug:        {}", config.display_slug);
        println!("repo:        {}", config.canonical_repo);
        println!("task_source: {}", config.task_source_file);
        println!("created_at:  {}", config.created_at);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests that set CCX_HOME must not run concurrently — env vars are process-global.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_path_to_slug() {
        let path = Utf8PathBuf::from("/Users/xpadev/src/myrepo");
        assert_eq!(path_to_slug(&path), "Users-xpadev-src-myrepo");
    }

    #[test]
    fn test_register_creates_project_json() {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("CCX_HOME", tmp.path().to_str().unwrap()) };

        let repo = Utf8PathBuf::from(tmp.path().join("repo").to_str().unwrap());
        let tasks = repo.join("z/tasks.md");
        fs::create_dir_all(&repo).unwrap();

        register(RegisterArgs {
            canonical_repo: repo.clone(),
            task_source_file: tasks,
        })
        .unwrap();

        let index_path = tmp.path().join("projects.json");
        assert!(index_path.exists());

        let raw = fs::read_to_string(&index_path).unwrap();
        let index: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(
            index[0]["display_slug"].as_str().unwrap(),
            path_to_slug(&repo)
        );

        unsafe { std::env::remove_var("CCX_HOME") };
    }

    #[test]
    fn test_register_appends_event_before_config() {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("CCX_HOME", tmp.path().to_str().unwrap()) };

        let repo = Utf8PathBuf::from(tmp.path().join("repo").to_str().unwrap());
        let tasks = repo.join("z/tasks.md");
        fs::create_dir_all(&repo).unwrap();

        register(RegisterArgs {
            canonical_repo: repo.clone(),
            task_source_file: tasks.clone(),
        })
        .unwrap();

        // Find the project_id from the index.
        let index_path = tmp.path().join("projects.json");
        let raw = fs::read_to_string(&index_path).unwrap();
        let index: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
        let project_id = index[0]["project_id"].as_str().unwrap().to_string();

        // events.jsonl must exist and contain exactly one ProjectRegistered event.
        let events_path = tmp
            .path()
            .join(format!("projects/{}/events.jsonl", project_id));
        assert!(events_path.exists(), "events.jsonl must be created");
        let events_raw = fs::read_to_string(&events_path).unwrap();
        let lines: Vec<&str> = events_raw.lines().collect();
        assert_eq!(lines.len(), 1, "exactly one event expected");
        assert!(
            lines[0].contains("project_registered"),
            "first event must be project_registered"
        );

        unsafe { std::env::remove_var("CCX_HOME") };
    }
}

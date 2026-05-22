use crate::config::project_config::{CleanupPolicy, GhReviewHook, ProjectConfig};
use crate::config::{ccx_home, project_dir};
use crate::domain::event::{
    Actor, Event, EventData, ProjectRegisteredPayload, generate_id,
};
use crate::error::CcxError;
use crate::persistence::jsonl::append_event;
use camino::Utf8PathBuf;
use clap::Args;
use std::fs;

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

    let dir = project_dir(&project_id)?;
    fs::create_dir_all(&dir)?;

    let config_path = dir.join("project.json");
    let json = serde_json::to_string_pretty(&config)?;
    fs::write(&config_path, json)?;

    // Also register in ccx_home index
    let index_path = ccx_home()?.join("projects.json");
    let mut index: Vec<serde_json::Value> = if index_path.exists() {
        let raw = fs::read_to_string(&index_path)?;
        serde_json::from_str(&raw)?
    } else {
        vec![]
    };
    index.push(serde_json::json!({
        "project_id": config.project_id,
        "display_slug": config.display_slug,
        "canonical_repo": config.canonical_repo,
    }));
    fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;

    // Append audit event.
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

    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long)]
    pub json: bool,
}

pub fn list(args: ListArgs) -> Result<(), CcxError> {
    let index_path = ccx_home()?.join("projects.json");
    let projects: Vec<serde_json::Value> = if index_path.exists() {
        let raw = fs::read_to_string(&index_path)?;
        serde_json::from_str(&raw)?
    } else {
        vec![]
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
    if !config_path.exists() {
        return Err(CcxError::ProjectNotFound {
            project_id: args.project_id,
        });
    }
    let raw = fs::read_to_string(&config_path)?;
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

    #[test]
    fn test_path_to_slug() {
        let path = Utf8PathBuf::from("/Users/xpadev/src/myrepo");
        assert_eq!(path_to_slug(&path), "Users-xpadev-src-myrepo");
    }

    #[test]
    fn test_register_creates_project_json() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: single-threaded test, no concurrent env reads
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
}

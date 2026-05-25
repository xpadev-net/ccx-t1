use std::fs::OpenOptions;
use std::io::Read;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use clap::Args;
use fd_lock::RwLock;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::project_config::ProjectConfig;
use crate::config::{load_project_config, project_dir};
use crate::error::CcxError;
use crate::git::repo::check_dirty;

#[derive(Debug, Args)]
pub struct ReadArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct WriteArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub expected_hash: String,
    /// Read replacement content from stdin
    #[arg(long)]
    pub stdin: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AppendArgs {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub expected_hash: String,
    /// Read appended content from stdin
    #[arg(long)]
    pub stdin: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Serialize)]
struct TaskSourceSnapshot {
    project_id: String,
    path: Utf8PathBuf,
    content: String,
    hash: String,
    mtime: String,
    warning: Option<TaskSourceWarning>,
}

#[derive(Debug, Serialize)]
struct TaskSourceWriteOutput {
    project_id: String,
    path: Utf8PathBuf,
    hash: String,
    mtime: String,
    bytes_written: usize,
    warning: Option<TaskSourceWarning>,
}

#[derive(Debug, Serialize)]
struct TaskSourceAppendOutput {
    project_id: String,
    path: Utf8PathBuf,
    hash: String,
    mtime: String,
    append_offset_bytes: usize,
    bytes_appended: usize,
    warning: Option<TaskSourceWarning>,
}

#[derive(Debug, Serialize)]
struct TaskSourceWarning {
    code: &'static str,
    message: String,
}

#[derive(Debug)]
struct LoadedTaskSource {
    content: String,
    hash: String,
    mtime: String,
}

pub fn read(args: ReadArgs) -> Result<(), CcxError> {
    let config = load_project_config(&args.project_id)?;
    let loaded = load_task_source_with_read_lock(&config)?;
    let warning = dirty_warning(&config)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&TaskSourceSnapshot {
                project_id: config.project_id,
                path: config.task_source_file,
                content: loaded.content,
                hash: loaded.hash,
                mtime: loaded.mtime,
                warning,
            })?
        );
    } else {
        print!("{}", loaded.content);
    }
    Ok(())
}

pub fn write(args: WriteArgs) -> Result<(), CcxError> {
    if !args.stdin {
        return Err(CcxError::Other(anyhow::anyhow!(
            "task-source write requires --stdin"
        )));
    }

    let config = load_project_config(&args.project_id)?;
    let content = read_stdin()?;
    let loaded = replace_task_source_with_lock(&config, &args.expected_hash, &content)?;
    let warning = dirty_warning(&config)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&TaskSourceWriteOutput {
                project_id: config.project_id,
                path: config.task_source_file,
                hash: loaded.hash,
                mtime: loaded.mtime,
                bytes_written: content.len(),
                warning,
            })?
        );
    } else {
        println!("updated {}", config.task_source_file);
    }
    Ok(())
}

pub fn append(args: AppendArgs) -> Result<(), CcxError> {
    if !args.stdin {
        return Err(CcxError::Other(anyhow::anyhow!(
            "task-source append requires --stdin"
        )));
    }

    let config = load_project_config(&args.project_id)?;
    let content = read_stdin()?;
    let (append_offset_bytes, loaded) =
        append_task_source_with_lock(&config, &args.expected_hash, &content)?;
    let warning = dirty_warning(&config)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&TaskSourceAppendOutput {
                project_id: config.project_id,
                path: config.task_source_file,
                hash: loaded.hash,
                mtime: loaded.mtime,
                append_offset_bytes,
                bytes_appended: content.len(),
                warning,
            })?
        );
    } else {
        println!("appended {}", config.task_source_file);
    }
    Ok(())
}

#[cfg(test)]
fn ensure_expected_hash(
    config: &ProjectConfig,
    expected_hash: &str,
) -> Result<LoadedTaskSource, CcxError> {
    let loaded = load_task_source(config)?;
    if loaded.hash != expected_hash {
        return Err(CcxError::TaskSourceConflict {
            expected_hash: expected_hash.to_owned(),
            actual_hash: loaded.hash,
        });
    }
    Ok(loaded)
}

fn load_task_source_with_read_lock(config: &ProjectConfig) -> Result<LoadedTaskSource, CcxError> {
    let lock_path = task_source_lock_path(config)?;
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    let rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.read()?;
    load_task_source(config)
}

fn replace_task_source_with_lock(
    config: &ProjectConfig,
    expected_hash: &str,
    content: &str,
) -> Result<LoadedTaskSource, CcxError> {
    with_task_source_lock(config, |current| {
        ensure_loaded_hash(&current, expected_hash)?;
        std::fs::write(&config.task_source_file, content.as_bytes())?;
        load_task_source(config)
    })
}

fn append_task_source_with_lock(
    config: &ProjectConfig,
    expected_hash: &str,
    content: &str,
) -> Result<(usize, LoadedTaskSource), CcxError> {
    with_task_source_lock(config, |current| {
        ensure_loaded_hash(&current, expected_hash)?;
        let append_offset_bytes = current.content.len();
        let mut next = current.content;
        next.push_str(content);
        std::fs::write(&config.task_source_file, next.as_bytes())?;
        Ok((append_offset_bytes, load_task_source(config)?))
    })
}

fn with_task_source_lock<T>(
    config: &ProjectConfig,
    f: impl FnOnce(LoadedTaskSource) -> Result<T, CcxError>,
) -> Result<T, CcxError> {
    let lock_path = task_source_lock_path(config)?;
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    let mut rw_lock = RwLock::new(lock_file);
    let _guard = rw_lock.write()?;
    let current = load_task_source(config)?;
    f(current)
}

fn task_source_lock_path(config: &ProjectConfig) -> Result<Utf8PathBuf, CcxError> {
    Ok(project_dir(&config.project_id)?.join("task-source.lock"))
}

fn ensure_loaded_hash(loaded: &LoadedTaskSource, expected_hash: &str) -> Result<(), CcxError> {
    if loaded.hash != expected_hash {
        return Err(CcxError::TaskSourceConflict {
            expected_hash: expected_hash.to_owned(),
            actual_hash: loaded.hash.clone(),
        });
    }
    Ok(())
}

fn load_task_source(config: &ProjectConfig) -> Result<LoadedTaskSource, CcxError> {
    let content = std::fs::read_to_string(&config.task_source_file).map_err(|e| {
        CcxError::Other(anyhow::anyhow!(
            "failed to read task source file {}: {e}",
            config.task_source_file
        ))
    })?;
    let metadata = std::fs::metadata(&config.task_source_file).map_err(|e| {
        CcxError::Other(anyhow::anyhow!(
            "failed to stat task source file {}: {e}",
            config.task_source_file
        ))
    })?;
    let modified = metadata.modified().map_err(|e| {
        CcxError::Other(anyhow::anyhow!(
            "failed to read task source mtime {}: {e}",
            config.task_source_file
        ))
    })?;
    let mtime = DateTime::<Utc>::from(modified).to_rfc3339();
    Ok(LoadedTaskSource {
        hash: sha256_hex(&content),
        content,
        mtime,
    })
}

fn read_stdin() -> Result<String, CcxError> {
    let mut content = String::new();
    std::io::stdin()
        .read_to_string(&mut content)
        .map_err(|e| CcxError::Other(anyhow::anyhow!("failed to read stdin: {e}")))?;
    Ok(content)
}

fn sha256_hex(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn dirty_warning(config: &ProjectConfig) -> Result<Option<TaskSourceWarning>, CcxError> {
    let inside = match task_source_is_inside_repo(&config.task_source_file, &config.canonical_repo)
    {
        Ok(inside) => inside,
        Err(_) => return Ok(None),
    };
    if !inside {
        return Ok(None);
    }

    let entries = match check_dirty(&config.canonical_repo) {
        Ok(entries) => entries,
        Err(CcxError::Git(_)) => return Ok(None),
        Err(e) => return Err(e),
    };
    if entries.is_empty() {
        return Ok(None);
    }

    Ok(Some(TaskSourceWarning {
        code: "task_source_in_canonical_repo_dirty",
        message: format!(
            "task source file is inside canonical repo {}; git working tree has {} dirty entr{}",
            config.canonical_repo,
            entries.len(),
            if entries.len() == 1 { "y" } else { "ies" }
        ),
    }))
}

fn task_source_is_inside_repo(
    task_source: &Utf8Path,
    canonical_repo: &Utf8Path,
) -> Result<bool, CcxError> {
    let task_source = task_source.canonicalize_utf8().map_err(|e| {
        CcxError::Other(anyhow::anyhow!(
            "failed to resolve task source file {}: {e}",
            task_source
        ))
    })?;
    let canonical_repo = canonical_repo.canonicalize_utf8().map_err(|e| {
        CcxError::Other(anyhow::anyhow!(
            "failed to resolve canonical repo {}: {e}",
            canonical_repo
        ))
    })?;
    Ok(task_source.starts_with(canonical_repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(repo: &Utf8Path, task_source_file: &Utf8Path) -> ProjectConfig {
        ProjectConfig {
            project_id: "01JTEST00000000000000000001".to_string(),
            display_slug: "test".to_string(),
            canonical_repo: repo.to_path_buf(),
            task_source_file: task_source_file.to_path_buf(),
            gh_review_hook: Default::default(),
            cleanup_policy: Default::default(),
            keep_last_n: 5,
            keep_for_days: 7,
            created_at: "2026-05-26T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn load_task_source_returns_content_hash_and_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Utf8PathBuf::try_from(tmp.path().join("repo")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        let file = repo.join("tasks.md");
        std::fs::write(&file, "hello").unwrap();

        let loaded = load_task_source(&config(&repo, &file)).unwrap();

        assert_eq!(loaded.content, "hello");
        assert_eq!(loaded.hash, sha256_hex("hello"));
        assert!(!loaded.mtime.is_empty());
    }

    #[test]
    fn expected_hash_rejects_stale_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Utf8PathBuf::try_from(tmp.path().join("repo")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        let file = repo.join("tasks.md");
        std::fs::write(&file, "hello").unwrap();

        let err = ensure_expected_hash(&config(&repo, &file), "stale")
            .expect_err("stale hash should fail");

        assert!(err.to_string().contains("task source conflict"));
    }

    #[test]
    fn missing_task_source_reports_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Utf8PathBuf::try_from(tmp.path().join("repo")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        let file = repo.join("missing.md");

        let err = load_task_source(&config(&repo, &file)).expect_err("missing file should fail");

        assert!(err.to_string().contains("failed to read task source file"));
        assert!(err.to_string().contains("missing.md"));
    }

    #[test]
    fn detects_task_source_inside_repo_without_string_prefix_false_positive() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Utf8PathBuf::try_from(tmp.path().join("repo")).unwrap();
        let sibling = Utf8PathBuf::try_from(tmp.path().join("repo-sibling")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let inside = repo.join("tasks.md");
        let outside = sibling.join("tasks.md");
        std::fs::write(&inside, "").unwrap();
        std::fs::write(&outside, "").unwrap();

        assert!(task_source_is_inside_repo(&inside, &repo).unwrap());
        assert!(!task_source_is_inside_repo(&outside, &repo).unwrap());
    }
}

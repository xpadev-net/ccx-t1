/// Level 3: E2E tests that exercise the compiled `ccx` binary with fake
/// external commands (`gh`, `gh-review-hook`) injected via PATH.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Absolute path to the `tests/fixtures/bin/` directory.
fn fixture_bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin")
}

/// Build a PATH string with the fixture bin dir prepended.
fn path_with_fixtures() -> String {
    let original = std::env::var("PATH").unwrap_or_default();
    format!("{}:{}", fixture_bin_dir().display(), original)
}

/// Run `ccx <args>` and return (exit_code, stdout, stderr).
fn run_ccx(args: &[&str], extra_env: &[(&str, &str)], ccx_home: &PathBuf) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_ccx");
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("PATH", path_with_fixtures())
        .env("CCX_HOME", ccx_home);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("failed to run ccx");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn run_ccx_with_stdin(
    args: &[&str],
    stdin: &str,
    extra_env: &[(&str, &str)],
    ccx_home: &PathBuf,
) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_ccx");
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("PATH", path_with_fixtures())
        .env("CCX_HOME", ccx_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("failed to spawn ccx");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .expect("failed to write stdin");
    let out = child.wait_with_output().expect("failed to run ccx");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn spawn_ccx_with_piped_stdin(args: &[&str], ccx_home: &PathBuf) -> std::process::Child {
    let bin = env!("CARGO_BIN_EXE_ccx");
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("PATH", path_with_fixtures())
        .env("CCX_HOME", ccx_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn().expect("failed to spawn ccx")
}

fn register_project(
    home: &PathBuf,
    repo: &std::path::Path,
    task_source: &std::path::Path,
) -> String {
    let (code, stdout, stderr) = run_ccx(
        &[
            "project",
            "register",
            "--canonical-repo",
            repo.to_str().unwrap(),
            "--task-source-file",
            task_source.to_str().unwrap(),
        ],
        &[],
        home,
    );
    assert_eq!(code, 0, "register failed: {stderr}");
    let project: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    project["project_id"].as_str().unwrap().to_string()
}

#[test]
fn project_register_and_list_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_path_buf();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let tasks = repo.join("tasks.md");
    std::fs::write(&tasks, b"").unwrap();

    let (code, stdout, _stderr) = run_ccx(
        &[
            "project",
            "register",
            "--canonical-repo",
            repo.to_str().unwrap(),
            "--task-source-file",
            tasks.to_str().unwrap(),
        ],
        &[],
        &home,
    );
    assert_eq!(code, 0, "register should succeed");
    assert!(
        stdout.contains("project_id"),
        "output should contain project_id"
    );

    let (code, stdout, _) = run_ccx(&["project", "list", "--json"], &[], &home);
    assert_eq!(code, 0, "list should succeed");
    let projects: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(projects.len(), 1, "one project should be listed");
}

#[test]
fn task_source_read_write_append_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("ccx-home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let tasks = repo.join("tasks.md");
    std::fs::write(&tasks, "one\n").unwrap();
    let project_id = register_project(&home, &repo, &tasks);

    let (code, stdout, stderr) = run_ccx(
        &["task-source", "read", "--project-id", &project_id, "--json"],
        &[],
        &home,
    );
    assert_eq!(code, 0, "read failed: {stderr}");
    let read_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(read_json["path"].as_str().unwrap(), tasks.to_str().unwrap());
    assert_eq!(read_json["content"].as_str().unwrap(), "one\n");
    let hash = read_json["hash"].as_str().unwrap().to_string();

    let (code, stdout, stderr) = run_ccx_with_stdin(
        &[
            "task-source",
            "write",
            "--project-id",
            &project_id,
            "--expected-hash",
            &hash,
            "--stdin",
            "--json",
        ],
        "two\n",
        &[],
        &home,
    );
    assert_eq!(code, 0, "write failed: {stderr}");
    assert_eq!(std::fs::read_to_string(&tasks).unwrap(), "two\n");
    let write_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(write_json["bytes_written"].as_u64(), Some(4));
    let hash = write_json["hash"].as_str().unwrap().to_string();

    let (code, stdout, stderr) = run_ccx_with_stdin(
        &[
            "task-source",
            "append",
            "--project-id",
            &project_id,
            "--expected-hash",
            &hash,
            "--stdin",
            "--json",
        ],
        "three\n",
        &[],
        &home,
    );
    assert_eq!(code, 0, "append failed: {stderr}");
    assert_eq!(std::fs::read_to_string(&tasks).unwrap(), "two\nthree\n");
    let append_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(append_json["append_offset"].as_u64(), Some(4));
    assert_eq!(append_json["bytes_appended"].as_u64(), Some(6));
}

#[test]
fn task_source_write_rejects_stale_expected_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("ccx-home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let tasks = repo.join("tasks.md");
    std::fs::write(&tasks, "one\n").unwrap();
    let project_id = register_project(&home, &repo, &tasks);

    let (code, _stdout, stderr) = run_ccx_with_stdin(
        &[
            "task-source",
            "write",
            "--project-id",
            &project_id,
            "--expected-hash",
            "stale",
            "--stdin",
            "--json",
        ],
        "two\n",
        &[],
        &home,
    );

    assert_ne!(code, 0);
    assert!(stderr.contains("task source conflict"));
    assert_eq!(std::fs::read_to_string(&tasks).unwrap(), "one\n");
}

#[test]
fn task_source_write_rechecks_hash_after_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("ccx-home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let tasks = repo.join("tasks.md");
    std::fs::write(&tasks, "one\n").unwrap();
    let project_id = register_project(&home, &repo, &tasks);
    let (code, stdout, stderr) = run_ccx(
        &["task-source", "read", "--project-id", &project_id, "--json"],
        &[],
        &home,
    );
    assert_eq!(code, 0, "read failed: {stderr}");
    let read_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let original_hash = read_json["hash"].as_str().unwrap().to_string();

    let mut child = spawn_ccx_with_piped_stdin(
        &[
            "task-source",
            "write",
            "--project-id",
            &project_id,
            "--expected-hash",
            &original_hash,
            "--stdin",
            "--json",
        ],
        &home,
    );
    std::fs::write(&tasks, "intervening\n").unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"two\n")
        .expect("failed to write stdin");
    let out = child.wait_with_output().expect("failed to run ccx");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success());
    assert!(stderr.contains("task source conflict"));
    assert_eq!(std::fs::read_to_string(&tasks).unwrap(), "intervening\n");
}

#[test]
fn task_source_append_rechecks_hash_after_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("ccx-home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let tasks = repo.join("tasks.md");
    std::fs::write(&tasks, "one\n").unwrap();
    let project_id = register_project(&home, &repo, &tasks);
    let (code, stdout, stderr) = run_ccx(
        &["task-source", "read", "--project-id", &project_id, "--json"],
        &[],
        &home,
    );
    assert_eq!(code, 0, "read failed: {stderr}");
    let read_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let original_hash = read_json["hash"].as_str().unwrap().to_string();

    let mut child = spawn_ccx_with_piped_stdin(
        &[
            "task-source",
            "append",
            "--project-id",
            &project_id,
            "--expected-hash",
            &original_hash,
            "--stdin",
            "--json",
        ],
        &home,
    );
    std::fs::write(&tasks, "intervening\n").unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"two\n")
        .expect("failed to write stdin");
    let out = child.wait_with_output().expect("failed to run ccx");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success());
    assert!(stderr.contains("task source conflict"));
    assert_eq!(std::fs::read_to_string(&tasks).unwrap(), "intervening\n");
}

#[test]
fn task_source_read_reports_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("ccx-home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let tasks = repo.join("tasks.md");
    std::fs::write(&tasks, "one\n").unwrap();
    let project_id = register_project(&home, &repo, &tasks);
    std::fs::remove_file(&tasks).unwrap();

    let (code, _stdout, stderr) = run_ccx(
        &["task-source", "read", "--project-id", &project_id, "--json"],
        &[],
        &home,
    );

    assert_ne!(code, 0);
    assert!(stderr.contains("failed to read task source file"));
}

#[test]
fn task_source_json_warns_when_canonical_repo_is_dirty() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("ccx-home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let tasks = repo.join("tasks.md");
    std::fs::write(&tasks, "one\n").unwrap();
    let project_id = register_project(&home, &repo, &tasks);

    let (code, stdout, stderr) = run_ccx(
        &["task-source", "read", "--project-id", &project_id, "--json"],
        &[],
        &home,
    );

    assert_eq!(code, 0, "read failed: {stderr}");
    let read_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        read_json["warning"]["code"].as_str(),
        Some("task_source_in_canonical_repo_dirty")
    );
}

#[test]
fn fake_gh_review_hook_exit_0_is_accepted() {
    let fake_hook = fixture_bin_dir().join("gh-review-hook");
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(&fake_hook)
        .current_dir(tmp.path())
        .env("FAKE_GH_REVIEW_HOOK_EXIT", "0")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn fake_gh_review_hook_exit_2_reports_issues() {
    let fake_hook = fixture_bin_dir().join("gh-review-hook");
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(&fake_hook)
        .current_dir(tmp.path())
        .env("FAKE_GH_REVIEW_HOOK_EXIT", "2")
        .env("FAKE_GH_REVIEW_HOOK_MSG", "lint errors found")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("lint errors found"));
}

#[test]
fn fake_gh_pr_view_returns_configured_json() {
    let fake_gh = fixture_bin_dir().join("gh");
    let tmp = tempfile::tempdir().unwrap();
    let json = r#"{"state":"MERGED","mergeable":"MERGEABLE","headRefOid":"deadbeef","number":42}"#;
    let out = Command::new(&fake_gh)
        .args(["pr", "view", "42", "--json", "state,mergeable,headRefOid"])
        .current_dir(tmp.path())
        .env("FAKE_GH_PR_VIEW_JSON", json)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("MERGED"), "state should be MERGED");
    assert!(stdout.contains("deadbeef"), "headRefOid should match");
}

#[test]
fn fake_gh_pr_view_non_zero_exit_on_error() {
    let fake_gh = fixture_bin_dir().join("gh");
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(&fake_gh)
        .args(["pr", "view", "99", "--json", "state,mergeable,headRefOid"])
        .current_dir(tmp.path())
        .env("FAKE_GH_PR_VIEW_EXIT", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

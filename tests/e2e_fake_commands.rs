/// Level 3: E2E tests that exercise the compiled `ccx` binary with fake
/// external commands (`gh`, `gh-review-hook`) injected via PATH.
use std::path::PathBuf;
use std::process::Command;

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

#[test]
fn project_register_and_list_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_path_buf();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let tasks = repo.join("tasks.md");

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
    assert!(stdout.contains("project_id"), "output should contain project_id");

    let (code, stdout, _) = run_ccx(&["project", "list", "--json"], &[], &home);
    assert_eq!(code, 0, "list should succeed");
    let projects: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(projects.len(), 1, "one project should be listed");
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

use camino::Utf8PathBuf;

/// Initialise a temporary git repository with one initial commit.
/// Returns the `Utf8PathBuf` pointing to the repo root.
pub fn init_repo(tmp: &tempfile::TempDir) -> Utf8PathBuf {
    let dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    for cmd in [
        vec!["init"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
    ] {
        let out = std::process::Command::new("git")
            .args(&cmd)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed: {}", cmd, String::from_utf8_lossy(&out.stderr));
    }
    std::fs::write(dir.join("README"), b"init").unwrap();
    for cmd in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        let out = std::process::Command::new("git")
            .args(&cmd)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed: {}", cmd, String::from_utf8_lossy(&out.stderr));
    }
    dir
}

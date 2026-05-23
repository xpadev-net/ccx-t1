use camino::Utf8Path;

use crate::error::CcxError;

/// One entry from `git status --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirtyEntry {
    /// Two-character XY status code (e.g. `" M"`, `"??"`, `"A "`, `"R "`).
    pub xy: String,
    /// File path. For renames, this is the new (destination) path.
    pub path: String,
}

/// Parse the stdout of `git status --porcelain` into a list of dirty entries.
///
/// Rename lines use the form `XY orig -> new`; only the new path is kept.
pub fn parse_porcelain(stdout: &str) -> Vec<DirtyEntry> {
    stdout
        .lines()
        .filter(|l| l.len() >= 4)
        .map(|l| {
            let xy = l[..2].to_string();
            let raw = &l[3..];
            // Rename: "orig -> new" — take only the destination path.
            let path = match raw.find(" -> ") {
                Some(pos) => raw[pos + 4..].to_string(),
                None => raw.to_string(),
            };
            DirtyEntry { xy, path }
        })
        .collect()
}

/// Run `git status --porcelain` in `repo` and return each dirty entry.
pub fn check_dirty(repo: &Utf8Path) -> Result<Vec<DirtyEntry>, CcxError> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .map_err(|e| CcxError::Git(format!("failed to run git status: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CcxError::Git(format!("git status failed: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_porcelain(&stdout))
}

/// Run `git reset --hard HEAD` in `repo`, discarding all uncommitted changes.
pub fn reset_hard(repo: &Utf8Path) -> Result<(), CcxError> {
    let status = std::process::Command::new("git")
        .args(["reset", "--hard", "HEAD"])
        .current_dir(repo)
        .status()
        .map_err(|e| CcxError::Git(format!("failed to run git reset: {e}")))?;

    if !status.success() {
        return Err(CcxError::Git(format!(
            "git reset --hard HEAD exited with {:?}",
            status.code()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stdout_returns_empty() {
        assert!(parse_porcelain("").is_empty());
    }

    #[test]
    fn whitespace_only_returns_empty() {
        assert!(parse_porcelain("   \n\n").is_empty());
    }

    #[test]
    fn modified_file_parsed() {
        let entries = parse_porcelain(" M src/main.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].xy, " M");
        assert_eq!(entries[0].path, "src/main.rs");
    }

    #[test]
    fn untracked_file_parsed() {
        let entries = parse_porcelain("?? new_file.txt\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].xy, "??");
        assert_eq!(entries[0].path, "new_file.txt");
    }

    #[test]
    fn staged_new_file_parsed() {
        let entries = parse_porcelain("A  src/new.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].xy, "A ");
        assert_eq!(entries[0].path, "src/new.rs");
    }

    #[test]
    fn rename_returns_new_path() {
        let entries = parse_porcelain("R  old_name.rs -> new_name.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].xy, "R ");
        assert_eq!(entries[0].path, "new_name.rs");
    }

    #[test]
    fn path_with_spaces_preserved() {
        let entries = parse_porcelain(" M path with spaces/file.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "path with spaces/file.rs");
    }

    #[test]
    fn multiple_entries_all_parsed() {
        let stdout = " M src/main.rs\n?? untracked.txt\nA  new.rs\n";
        let entries = parse_porcelain(stdout);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].xy, " M");
        assert_eq!(entries[0].path, "src/main.rs");
        assert_eq!(entries[1].xy, "??");
        assert_eq!(entries[1].path, "untracked.txt");
        assert_eq!(entries[2].xy, "A ");
        assert_eq!(entries[2].path, "new.rs");
    }

    #[test]
    fn short_lines_skipped() {
        // Lines shorter than 4 chars lack the required "XY " prefix.
        let entries = parse_porcelain("AB\n M src/main.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/main.rs");
    }

    #[test]
    fn deleted_file_parsed() {
        let entries = parse_porcelain("D  removed.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].xy, "D ");
        assert_eq!(entries[0].path, "removed.rs");
    }

    // ── Level 2: local git tests ──────────────────────────────────────────────

    fn init_repo(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        let dir = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        for cmd in [
            vec!["init"],
            vec!["config", "user.email", "test@test.com"],
            vec!["config", "user.name", "Test"],
        ] {
            std::process::Command::new("git")
                .args(&cmd)
                .current_dir(&dir)
                .output()
                .unwrap();
        }
        // Initial commit required for branches to resolve HEAD.
        std::fs::write(dir.join("README"), b"init").unwrap();
        for cmd in [vec!["add", "."], vec!["commit", "-m", "init"]] {
            std::process::Command::new("git")
                .args(&cmd)
                .current_dir(&dir)
                .output()
                .unwrap();
        }
        dir
    }

    #[test]
    fn check_dirty_clean_repo_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = init_repo(&tmp);
        let entries = check_dirty(&dir).unwrap();
        assert!(entries.is_empty(), "clean repo should have no dirty entries");
    }

    #[test]
    fn check_dirty_detects_modified_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = init_repo(&tmp);
        std::fs::write(dir.join("README"), b"modified").unwrap();
        let entries = check_dirty(&dir).unwrap();
        assert!(!entries.is_empty(), "modified file should appear as dirty");
        assert!(entries.iter().any(|e| e.path == "README"));
    }

    #[test]
    fn reset_hard_clears_modifications() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = init_repo(&tmp);
        std::fs::write(dir.join("README"), b"dirty").unwrap();
        assert!(!check_dirty(&dir).unwrap().is_empty());
        reset_hard(&dir).unwrap();
        assert!(check_dirty(&dir).unwrap().is_empty(), "dirty entries should be gone after reset");
    }
}

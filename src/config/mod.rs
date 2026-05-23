pub mod project_config;

use crate::config::project_config::ProjectConfig;
use crate::error::CcxError;
use camino::Utf8PathBuf;
use std::path::PathBuf;

/// Returns the CCX home directory: $CCX_HOME if set, otherwise ~/.ccx
pub fn ccx_home() -> Result<Utf8PathBuf, CcxError> {
    if let Ok(val) = std::env::var("CCX_HOME") {
        return Utf8PathBuf::try_from(PathBuf::from(val))
            .map_err(|e| CcxError::Config(format!("CCX_HOME is not valid UTF-8: {e}")));
    }
    let base = directories::BaseDirs::new()
        .ok_or_else(|| CcxError::Config("cannot determine home directory".into()))?;
    let home = base.home_dir().join(".ccx");
    Utf8PathBuf::try_from(home)
        .map_err(|e| CcxError::Config(format!("home path is not valid UTF-8: {e}")))
}

/// Rejects project IDs that could escape the projects directory via path traversal.
/// Project IDs are ULIDs (26 ASCII alphanumeric characters); anything else is invalid.
fn validate_project_id(id: &str) -> Result<(), CcxError> {
    id.parse::<ulid::Ulid>()
        .map(|_| ())
        .map_err(|_| CcxError::Config(format!("invalid project_id: {id:?}")))
}

/// Returns the directory for a specific project: <ccx_home>/projects/<project_id>
pub fn project_dir(project_id: &str) -> Result<Utf8PathBuf, CcxError> {
    validate_project_id(project_id)?;
    Ok(ccx_home()?.join("projects").join(project_id))
}

/// Load the project.json config for a given project_id.
pub fn load_project_config(project_id: &str) -> Result<ProjectConfig, CcxError> {
    let path = project_dir(project_id)?.join("project.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CcxError::ProjectNotFound { project_id: project_id.to_owned() }
        } else {
            CcxError::Io(e)
        }
    })?;
    Ok(serde_json::from_str(&raw)?)
}

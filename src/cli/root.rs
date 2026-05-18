use std::path::PathBuf;

use crate::storage;

/// Resolve a project root for command execution.
///
/// Strict resolution is used for interactive and mutating commands so we never
/// fall back to "." unless we can confirm a project marker exists in the path
/// hierarchy.
pub fn resolve_project_root(
    project_root: Option<PathBuf>,
    command_name: &str,
) -> Result<PathBuf, anyhow::Error> {
    let root = project_root.or_else(storage::find_project_root_strict);
    root.ok_or_else(|| {
        anyhow::anyhow!(
            "{command_name} requires a project root. Run inside a directory that contains \
            .git or .octocode, or pass --project-root explicitly."
        )
    })
}

/// Resolve a project root when markers are optional.
///
/// This preserves legacy behavior for commands that need a best-effort location.
pub fn resolve_project_root_or_cwd(project_root: Option<PathBuf>) -> PathBuf {
    project_root
        .or_else(storage::find_project_root_strict)
        .unwrap_or_else(|| PathBuf::from("."))
}

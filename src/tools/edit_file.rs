use std::path::Path;

use crate::workspace::apply::DiffResult;

/// Execute an `EditFile` tool call. Requires policy approval before calling.
pub fn edit_file(
    project_root: &Path,
    path: &str,
    old_string: &str,
    new_string: &str,
) -> Result<DiffResult, anyhow::Error> {
    let resolved = crate::workspace::paths::resolve_workspace_path(project_root, path).ok_or_else(|| {
        crate::workspace::paths::nearest_paths_hint(project_root, path).map_or_else(
            || anyhow::anyhow!("path not in workspace or protected: {path}"),
            |hint| anyhow::anyhow!("cannot edit {path} ({hint}) — not found, outside the workspace, or protected"),
        )
    })?;

    let relative = resolved
        .strip_prefix(project_root)
        .unwrap_or(&resolved)
        .to_string_lossy()
        .to_string();

    crate::workspace::apply::apply_edit(project_root, &relative, old_string, new_string)
}

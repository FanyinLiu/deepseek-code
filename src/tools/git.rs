use std::path::Path;

/// Execute `GitStatus` tool — read-only, auto-approved.
pub fn git_status(project_root: &Path) -> Result<String, anyhow::Error> {
    crate::workspace::git::git_status(project_root)
}

/// Execute `GitDiff` tool — read-only, auto-approved.
pub fn git_diff(
    project_root: &Path,
    staged: bool,
    path: Option<&str>,
) -> Result<String, anyhow::Error> {
    crate::workspace::git::git_diff(project_root, staged, path)
}

/// Execute `GitLog` tool — read-only, auto-approved.
pub fn git_log(project_root: &Path, count: usize) -> Result<String, anyhow::Error> {
    crate::workspace::git::git_log(project_root, count)
}

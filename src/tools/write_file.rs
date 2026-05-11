use std::path::Path;

use crate::workspace::apply::DiffResult;

/// Execute a `WriteFile` tool call. Requires policy approval before calling.
pub fn write_file(
    project_root: &Path,
    path: &str,
    content: &str,
) -> Result<DiffResult, anyhow::Error> {
    crate::workspace::apply::apply_write(project_root, path, content)
}

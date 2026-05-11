use std::path::Path;

/// List files and directories in a readable path.
pub fn list_dir(project_root: &Path, path: &str, recursive: bool) -> Result<String, anyhow::Error> {
    let dir = crate::workspace::paths::resolve_read_path(project_root, path)
        .ok_or_else(|| anyhow::anyhow!("path not found or unreadable: {path}"))?;

    let canonical_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let mut out = String::new();

    if recursive {
        for entry in walkdir::WalkDir::new(&dir).max_depth(5) {
            let entry = entry?;
            let relative = entry
                .path()
                .strip_prefix(&canonical_root)
                .unwrap_or(entry.path());
            out.push_str(&format!("{}\n", relative.display()));
        }
    } else {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let entry_path = entry.path().clone();
            let relative = entry_path
                .strip_prefix(&canonical_root)
                .unwrap_or(&entry_path);
            let file_type = if entry.file_type()?.is_dir() { "/" } else { "" };
            out.push_str(&format!("{}{}\n", relative.display(), file_type));
        }
    }

    Ok(out)
}

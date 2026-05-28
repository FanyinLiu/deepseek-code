use std::path::Path;

/// Generate a unified diff between original and modified text.
#[must_use]
pub fn unified_diff(original: &str, modified: &str, file_path: &str) -> String {
    let diff = similar::TextDiff::from_lines(original, modified);
    format!(
        "--- a/{}\n+++ b/{}\n{}",
        file_path,
        file_path,
        diff.unified_diff().context_radius(3)
    )
}

/// Generate a diff for a file against its current on-disk content.
pub fn diff_against_disk(file_path: &Path, new_content: &str) -> Result<String, anyhow::Error> {
    let original = if file_path.exists() {
        crate::storage::read_text_file_capped(file_path)?
    } else {
        String::new()
    };
    let path_str = file_path.to_string_lossy().to_string();
    Ok(unified_diff(&original, new_content, &path_str))
}

/// Generate a summary of changes: number of lines added/removed.
#[must_use]
pub fn diff_stats(diff_text: &str) -> DiffStats {
    let mut added = 0;
    let mut removed = 0;

    for line in diff_text.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }
    }

    DiffStats { added, removed }
}

#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
}

impl std::fmt::Display for DiffStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "+{}/-{}", self.added, self.removed)
    }
}

/// Truncate a diff for display in approval dialogs.
#[must_use]
pub fn truncate_diff(diff: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = diff.lines().collect();
    if lines.len() <= max_lines {
        return diff.to_string();
    }
    let mut out = lines[..max_lines].join("\n");
    out.push_str(&format!(
        "\n... ({} more lines, {} total)",
        lines.len() - max_lines,
        lines.len()
    ));
    out
}

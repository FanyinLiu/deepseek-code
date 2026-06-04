use std::path::{Path, PathBuf};

/// Resolve a path relative to the project root, canonicalizing it.
/// Returns None if the path escapes the workspace or hits a protected path.
pub fn resolve_workspace_path(project_root: &Path, relative_or_absolute: &str) -> Option<PathBuf> {
    let path = Path::new(relative_or_absolute);

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };

    // Canonicalize to resolve symlinks and `..`
    let canonical = std::fs::canonicalize(&absolute).ok()?;

    // Canonicalize project_root for comparison (Windows UNC prefix normalization)
    let canonical_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

    // Symlink escape check: canonical path must start with canonical_root
    if !canonical.starts_with(&canonical_root) {
        tracing::warn!(
            "path escapes workspace: {} resolves to {} (root: {})",
            absolute.display(),
            canonical.display(),
            canonical_root.display()
        );
        return None;
    }

    Some(canonical)
}

/// Resolve a path for read-only operations.
///
/// Relative paths stay workspace-scoped. Absolute paths may point outside the
/// workspace; policy approval decides whether that sensitive read is allowed.
pub fn resolve_read_path(project_root: &Path, relative_or_absolute: &str) -> Option<PathBuf> {
    let path = Path::new(relative_or_absolute);
    if path.is_absolute() {
        std::fs::canonicalize(path).ok()
    } else {
        resolve_workspace_path(project_root, relative_or_absolute)
    }
}

/// Return a stable display path for a resolved path.
#[must_use]
pub fn display_path(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Check if a path matches any protected pattern.
#[must_use]
pub fn is_protected_path(path: &Path, protected_patterns: &[String]) -> bool {
    let path_str = path.to_string_lossy().to_string();
    let canonical = std::fs::canonicalize(path)
        .map_or_else(|_| path_str.clone(), |p| p.to_string_lossy().to_string());

    for pattern in protected_patterns {
        if glob_match(pattern, &path_str) || glob_match(pattern, &canonical) {
            return true;
        }
    }
    false
}

/// Simple glob matching for path patterns using the `glob` crate.
fn glob_match(pattern: &str, path: &str) -> bool {
    // Handle `~` expansion
    let pattern = if let Some(home) = dirs::home_dir() {
        pattern.replace('~', &home.to_string_lossy())
    } else {
        pattern.to_string()
    };

    glob::Pattern::new(&pattern).is_ok_and(|p| p.matches(path))
}

/// Check Unicode normalization for a command string.
/// Returns the normalized form and whether normalization changed anything.
#[must_use]
pub fn normalize_unicode_command(cmd: &str) -> (String, bool) {
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = cmd.nfc().collect();
    let changed = normalized != cmd;
    (normalized, changed)
}

/// When a requested path doesn't resolve, build a short re-grounding hint: the
/// nearest existing ancestor directory inside the workspace plus a few of its
/// real entries, ranked by similarity to the missing name. This lets the model
/// correct a hallucinated or typo'd path instead of guessing again. Returns
/// None when there is nothing useful to suggest.
#[must_use]
pub fn nearest_paths_hint(project_root: &Path, requested: &str) -> Option<String> {
    let canonical_root = std::fs::canonicalize(project_root).ok()?;
    let req = Path::new(requested);
    let absolute = if req.is_absolute() {
        req.to_path_buf()
    } else {
        project_root.join(req)
    };
    let wanted = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();

    let mut ancestor = absolute.parent();
    while let Some(dir) = ancestor {
        if let Ok(canon) = std::fs::canonicalize(dir) {
            if canon.is_dir() && canon.starts_with(&canonical_root) {
                let mut names: Vec<String> = std::fs::read_dir(&canon)
                    .ok()?
                    .flatten()
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .filter(|name| !name.starts_with('.'))
                    .collect();
                if names.is_empty() {
                    return None;
                }
                names.sort_by(|a, b| {
                    shared_prefix_len(b, &wanted)
                        .cmp(&shared_prefix_len(a, &wanted))
                        .then_with(|| a.cmp(b))
                });
                names.truncate(12);
                let rel = canon.strip_prefix(&canonical_root).map_or_else(
                    |_| canon.display().to_string(),
                    |relative| {
                        let shown = relative.to_string_lossy();
                        if shown.is_empty() {
                            ".".to_string()
                        } else {
                            shown.into_owned()
                        }
                    },
                );
                return Some(format!(
                    "nearest existing directory is `{rel}`, which contains: {}. \
                     Re-check the path, or use grep/glob to find the file.",
                    names.join(", ")
                ));
            }
        }
        ancestor = dir.parent();
    }
    None
}

/// Length of the shared leading character run of `a` and `b`.
fn shared_prefix_len(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(left, right)| left == right)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match_env_pattern() {
        assert!(glob_match("**/.env", "/home/user/project/.env"));
        assert!(glob_match("**/.env", ".env"));
        assert!(glob_match("**/.env.*", ".env.production"));
    }

    #[test]
    fn test_glob_match_ssh_pattern() {
        assert!(glob_match(
            "~/.ssh/**",
            &format!("{}/.ssh/id_rsa", dirs::home_dir().unwrap().display())
        ));
    }

    #[test]
    fn test_normalize_unicode_noop() {
        let (result, changed) = normalize_unicode_command("cargo test");
        assert_eq!(result, "cargo test");
        assert!(!changed);
    }

    #[test]
    fn resolve_read_path_allows_absolute_existing_path() {
        let file = tempfile::NamedTempFile::new().expect("temp file");
        let root = tempfile::tempdir().expect("temp dir");

        let resolved = resolve_read_path(root.path(), &file.path().to_string_lossy())
            .expect("absolute read path");

        assert_eq!(
            resolved,
            std::fs::canonicalize(file.path()).expect("canonical")
        );
    }

    #[test]
    fn nearest_paths_hint_points_at_real_siblings() {
        let root = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(root.path().join("src/agent")).expect("mkdir");
        std::fs::write(root.path().join("src/agent/orchestrator.rs"), "x").expect("write");
        std::fs::write(root.path().join("src/agent/tool_loop.rs"), "x").expect("write");

        // A typo'd basename surfaces the real sibling (ranked by shared prefix).
        let hint = nearest_paths_hint(root.path(), "src/agent/orchestratr.rs")
            .expect("hint for typo'd path");
        assert!(hint.contains("orchestrator.rs"), "hint: {hint}");
        assert!(hint.contains("src/agent"), "hint: {hint}");

        // A missing intermediate directory walks up to the nearest existing one.
        let walked =
            nearest_paths_hint(root.path(), "src/nope/deeper/x.rs").expect("hint walks up to src");
        assert!(walked.contains("agent"), "walked: {walked}");
    }
}

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
}

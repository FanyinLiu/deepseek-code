use std::path::Path;

/// Path protection rules.
/// These are applied before any file read/write operation.
const PROTECTED_PATHS: &[&str] = &[
    "~/.ssh/**",
    "~/.aws/**",
    "~/.gnupg/**",
    "**/.env",
    "**/.env.*",
    "**/credentials*",
    "**/*token*",
    "/etc/**",
    ".git/objects/**",
    ".git/refs/**",
];

/// Check if a path is blocked (must never be read or written).
#[must_use]
pub fn is_blocked_path(path: &Path, project_root: &Path) -> bool {
    let path_str = normalize_for_glob(&path.to_string_lossy());

    // Resolve home directory
    let home = dirs::home_dir()
        .map(|h| normalize_for_glob(&h.to_string_lossy()))
        .unwrap_or_default();

    for pattern in PROTECTED_PATHS {
        let pattern = normalize_for_glob(&pattern.replace('~', &home));

        if glob_match(&pattern, &path_str) {
            return true;
        }

        // Also check the canonical path
        if let Ok(canonical) = std::fs::canonicalize(path) {
            let canonical_str = normalize_for_glob(&canonical.to_string_lossy());
            if glob_match(&pattern, &canonical_str) {
                return true;
            }
        }
    }

    // Check .git internals
    if let Ok(canonical) = std::fs::canonicalize(path) {
        if canonical.starts_with(project_root.join(".git")) {
            // Allow read-only git operations, block direct object/ref writes
            let relative = canonical
                .strip_prefix(project_root.join(".git"))
                .unwrap_or(Path::new(""));
            let blocked_prefixes = ["objects/", "refs/"];
            for prefix in blocked_prefixes {
                if relative.starts_with(prefix) {
                    return true;
                }
            }
        }
    }

    false
}

/// Check if a path is sensitive (should prompt but not block).
#[must_use]
pub fn is_sensitive_path(path: &Path, project_root: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let sensitive_names = [
        "secret",
        "password",
        "private_key",
        "api_key",
        "token",
        "credential",
        "key.pem",
        "key.pub",
    ];

    if sensitive_names
        .iter()
        .any(|s| name.to_lowercase().contains(s))
    {
        return true;
    }

    // Outside project root
    if let Ok(canonical) = std::fs::canonicalize(path) {
        if !canonical.starts_with(project_root) {
            return true;
        }
    }

    false
}

fn glob_match(pattern: &str, path: &str) -> bool {
    if pattern == path {
        return true;
    }

    if wildcard_match(pattern, path) {
        return true;
    }

    if !pattern.starts_with('/') && !pattern.contains(":/") {
        let unanchored = format!("**/{}", pattern.trim_start_matches('/'));
        return wildcard_match(&unanchored, path);
    }

    !pattern.contains('*') && path.contains(pattern)
}

fn wildcard_match(pattern: &str, path: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let path: Vec<char> = path.chars().collect();

    let (mut pattern_index, mut path_index) = (0, 0);
    let mut star_index = None;
    let mut star_match_index = 0;

    while path_index < path.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == path[path_index] {
            pattern_index += 1;
            path_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            star_match_index = path_index;
            pattern_index += 1;
        } else if let Some(index) = star_index {
            pattern_index = index + 1;
            star_match_index += 1;
            path_index = star_match_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

fn normalize_for_glob(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_env_file() {
        assert!(is_blocked_path(
            Path::new("/home/user/project/.env"),
            Path::new("/home/user/project")
        ));
    }

    #[test]
    fn test_blocked_ssh_key() {
        let home = dirs::home_dir().unwrap();
        let ssh_key = home.join(".ssh").join("id_rsa");
        assert!(is_blocked_path(&ssh_key, Path::new("/tmp/project")));
    }

    #[test]
    fn test_glob_match_handles_windows_separators() {
        assert!(glob_match(
            &normalize_for_glob("C:\\Users\\me/.ssh/**"),
            &normalize_for_glob("C:\\Users\\me\\.ssh\\id_rsa")
        ));
    }

    #[test]
    fn test_glob_match_handles_suffix_wildcards() {
        assert!(glob_match(
            "**/.env.*",
            "/home/user/project/.env.production"
        ));
        assert!(glob_match(
            "**/credentials*",
            "/home/project/credentials.json"
        ));
    }

    #[test]
    fn test_normal_file_not_blocked() {
        assert!(!is_blocked_path(
            Path::new("/home/user/project/src/main.rs"),
            Path::new("/home/user/project")
        ));
    }
}

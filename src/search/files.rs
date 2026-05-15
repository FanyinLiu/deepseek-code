use std::path::{Path, PathBuf};

/// Search for files by name (glob-like pattern) in the project, respecting .gitignore.
pub fn search_files(
    project_root: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let query_lower = query.to_lowercase();
    let glob_pattern = glob::Pattern::new(query).ok();

    for entry in ignore::Walk::new(project_root) {
        let entry = entry?;
        if entry.file_type().is_none_or(|t| t.is_dir()) {
            // Skip directories by default
            if !query.contains('/') {
                continue;
            }
        }

        let path = entry.path();
        let relative = path.strip_prefix(project_root).unwrap_or(path);
        let name = relative.to_string_lossy();

        if name_matches(&name, &query_lower, glob_pattern.as_ref()) {
            results.push(SearchMatch {
                path: relative.to_path_buf(),
                line_number: None,
                matched_text: name.to_string(),
                match_type: MatchType::FileName,
            });

            if results.len() >= limit {
                break;
            }
        }
    }

    Ok(results)
}

fn name_matches(name: &str, query: &str, glob_pattern: Option<&glob::Pattern>) -> bool {
    let name_lower = name.to_lowercase();
    if name_lower.contains(query) {
        return true;
    }
    // Use the glob crate for proper pattern matching.
    if let Some(pattern) = glob_pattern {
        if pattern.matches(name) {
            return true;
        }
        // If the pattern has no path separator, also match against the file name only
        // so that "*.rs" matches "src/main.rs".
        if !query.contains('/') && !query.contains('\\') {
            if let Some(file_name) = std::path::Path::new(name).file_name() {
                if pattern.matches(&file_name.to_string_lossy()) {
                    return true;
                }
            }
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line_number: Option<usize>,
    pub matched_text: String,
    pub match_type: MatchType,
}

#[derive(Debug, Clone)]
pub enum MatchType {
    FileName,
    CodeLine,
    Symbol,
    GitDiff,
    Session,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_matches_substring() {
        assert!(name_matches("src/main.rs", "main", None));
        assert!(!name_matches("src/lib.rs", "main", None));
    }

    #[test]
    fn test_name_matches_glob_suffix() {
        let pattern = glob::Pattern::new("*.rs").ok();
        assert!(name_matches("src/main.rs", "*.rs", pattern.as_ref()));
        assert!(!name_matches("src/main.txt", "*.rs", pattern.as_ref()));
    }

    #[test]
    fn test_name_matches_glob_prefix() {
        let pattern = glob::Pattern::new("foo*").ok();
        assert!(name_matches("foobar.rs", "foo*", pattern.as_ref()));
        assert!(!name_matches("barfoo.rs", "foo*", pattern.as_ref()));
    }

    #[test]
    fn test_name_matches_glob_complex() {
        let pattern = glob::Pattern::new("src/*.rs").ok();
        assert!(name_matches("src/main.rs", "src/*.rs", pattern.as_ref()));
        assert!(!name_matches("tests/main.rs", "src/*.rs", pattern.as_ref()));
    }

    #[test]
    fn search_files_respects_zero_limit() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("needle.rs"), "fn main() {}\n").expect("write file");

        let results = search_files(root.path(), "needle", 0).expect("search files");
        assert!(results.is_empty());
    }
}

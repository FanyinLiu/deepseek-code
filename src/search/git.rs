use std::path::Path;

use super::files::{MatchType, SearchMatch};

/// Search git diffs (staged or unstaged) for matching content.
pub fn search_git_diff(
    project_root: &Path,
    query: Option<&str>,
    staged: bool,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    let diff_content = crate::workspace::git::git_diff(project_root, staged, None)?;

    if diff_content.trim().is_empty() {
        return Ok(Vec::new());
    }

    let matches = match query {
        Some(q) => {
            // Filter diff hunks that match the query
            let q_lower = q.to_lowercase();
            diff_content
                .lines()
                .enumerate()
                .filter(|(_, line)| line.to_lowercase().contains(&q_lower))
                .map(|(i, line)| SearchMatch {
                    path: Path::new("").to_path_buf(), // Will be inferred from diff hunk
                    line_number: Some(i + 1),
                    matched_text: line.to_string(),
                    match_type: MatchType::GitDiff,
                })
                .collect()
        }
        None => {
            // Return all diff content as a single "result"
            vec![SearchMatch {
                path: Path::new("").to_path_buf(),
                line_number: None,
                matched_text: diff_content,
                match_type: MatchType::GitDiff,
            }]
        }
    };

    Ok(matches)
}

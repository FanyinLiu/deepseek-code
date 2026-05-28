use std::path::Path;

use super::files::{MatchType, SearchMatch};

/// Search through past session transcripts for relevant context.
pub fn search_sessions(
    base_path: &Path,
    query: &str,
    project_only: bool,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    let sessions_dir = base_path.join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let query_lower = query.to_lowercase();

    for entry in std::fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let project_dir = entry.path();

        for session_entry in std::fs::read_dir(&project_dir)? {
            let session_entry = session_entry?;
            if !session_entry.file_type()?.is_dir() {
                continue;
            }

            if project_only {
                // For project_only, we'd need to check if this is the current project
                // Simplified for now: include all
            }

            let session_dir = session_entry.path();
            let session_file = session_dir.join("session.json");

            if let Ok(content) = crate::storage::read_text_file_capped(&session_file) {
                if content.to_lowercase().contains(&query_lower) {
                    // Try to extract a meaningful snippet
                    if let Ok(session_json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let name = session_json["name"].as_str().unwrap_or("unnamed");
                        let msg_count = session_json["messages"]
                            .as_array()
                            .map_or(0, std::vec::Vec::len);

                        results.push(SearchMatch {
                            path: session_dir,
                            line_number: None,
                            matched_text: format!(
                                "Session '{name}' ({msg_count} messages) — matches query"
                            ),
                            match_type: MatchType::Session,
                        });
                    }

                    if results.len() >= limit {
                        return Ok(results);
                    }
                }
            }
        }
    }

    Ok(results)
}

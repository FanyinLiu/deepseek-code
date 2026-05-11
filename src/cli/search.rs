use std::path::PathBuf;

use crate::search::{self, SearchMatch};
use crate::storage;

/// Run the search command: local code search.
pub async fn search(
    query: String,
    code_only: bool,
    limit: usize,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));

    let mut all_results: Vec<SearchMatch> = Vec::new();

    if code_only {
        let results = search::search_code(&root, &query, None, false, limit)?;
        all_results.extend(results);
    } else {
        let files = search::search_files(&root, &query, limit / 2)?;
        let code = search::search_code(&root, &query, None, false, limit / 2)?;
        all_results.extend(files);
        all_results.extend(code);
    }

    if all_results.is_empty() {
        println!("No results found for: {query}");
        return Ok(());
    }

    println!("Found {} results for: {}\n", all_results.len(), query);

    for result in &all_results {
        match result.match_type {
            search::MatchType::FileName => {
                println!("📁 {}", result.path.display());
            }
            _ => {
                if let Some(line) = result.line_number {
                    println!(
                        "{}:{}: {}",
                        result.path.display(),
                        line,
                        result.matched_text
                    );
                } else {
                    println!("{}: {}", result.path.display(), result.matched_text);
                }
            }
        }
    }

    Ok(())
}

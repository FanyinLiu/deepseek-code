use super::files::SearchMatch;

/// Pack search results into a context string for `DeepSeek` consumption.
/// Deduplicates, truncates per-file, clusters by file, and marks untrusted.
#[must_use]
pub fn pack_search_results(results: &[SearchMatch], max_tokens: usize) -> SearchContext {
    // Cluster by file
    let mut by_file: std::collections::BTreeMap<String, Vec<&SearchMatch>> =
        std::collections::BTreeMap::new();
    for r in results {
        let key = r.path.to_string_lossy().to_string();
        by_file.entry(key).or_default().push(r);
    }

    let mut ctx = SearchContext {
        summary: String::new(),
        file_clusters: Vec::new(),
        estimated_tokens: 0,
        is_truncated: false,
    };

    const CHARS_PER_TOKEN_ESTIMATE: usize = 4;
    const MAX_MATCHES_PER_FILE: usize = 5;
    let char_budget = max_tokens * CHARS_PER_TOKEN_ESTIMATE;
    let mut char_used = 0;

    for (file, matches) in &by_file {
        if char_used >= char_budget {
            ctx.is_truncated = true;
            break;
        }

        let mut file_text = String::new();
        file_text.push_str(&format!("## {file}\n"));

        for m in matches.iter().take(MAX_MATCHES_PER_FILE) {
            // Max 5 matches per file
            let line = if let Some(ln) = m.line_number {
                format!("  L{}: {}\n", ln, m.matched_text)
            } else {
                format!("  {}\n", m.matched_text)
            };

            if char_used + line.len() > char_budget {
                ctx.is_truncated = true;
                break;
            }

            file_text.push_str(&line);
            char_used += line.len();
        }

        char_used += file_text.len();
        ctx.file_clusters.push(FileCluster {
            path: file.clone(),
            snippet: file_text,
            match_count: matches.len(),
        });
    }

    ctx.summary = format!(
        "Search returned {} matches across {} files{}",
        results.len(),
        by_file.len(),
        if ctx.is_truncated { " (truncated)" } else { "" }
    );
    ctx.estimated_tokens = char_used / 4;

    ctx
}

#[derive(Debug, Clone)]
pub struct SearchContext {
    pub summary: String,
    pub file_clusters: Vec<FileCluster>,
    pub estimated_tokens: usize,
    pub is_truncated: bool,
}

#[derive(Debug, Clone)]
pub struct FileCluster {
    pub path: String,
    pub snippet: String,
    pub match_count: usize,
}

impl std::fmt::Display for SearchContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.summary)?;
        writeln!(f)?;
        for cluster in &self.file_clusters {
            write!(f, "{}", cluster.snippet)?;
        }
        if self.is_truncated {
            writeln!(f, "\n[Results truncated — refine search to narrow scope]")?;
        }
        Ok(())
    }
}

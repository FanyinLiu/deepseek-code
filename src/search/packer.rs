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

        let mut file_text = format!("## {file}\n");
        let mut file_chars = file_text.chars().count();
        if char_used + file_chars > char_budget {
            ctx.is_truncated = true;
            break;
        }

        for m in matches.iter().take(MAX_MATCHES_PER_FILE) {
            // Max 5 matches per file
            let line = if let Some(ln) = m.line_number {
                format!("  L{}: {}\n", ln, m.matched_text)
            } else {
                format!("  {}\n", m.matched_text)
            };
            let line_chars = line.chars().count();

            if char_used + file_chars + line_chars > char_budget {
                ctx.is_truncated = true;
                break;
            }

            file_text.push_str(&line);
            file_chars += line_chars;
        }

        char_used += file_chars;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::files::MatchType;
    use std::path::PathBuf;

    fn search_match(path: &str, matched_text: &str) -> SearchMatch {
        SearchMatch {
            path: PathBuf::from(path),
            line_number: Some(1),
            matched_text: matched_text.into(),
            match_type: MatchType::CodeLine,
        }
    }

    #[test]
    fn packer_counts_snippet_chars_once() {
        let results = vec![search_match("a.rs", "汉字测试")];
        let ctx = pack_search_results(&results, 5);
        let actual_chars: usize = ctx
            .file_clusters
            .iter()
            .map(|cluster| cluster.snippet.chars().count())
            .sum();

        assert_eq!(ctx.file_clusters.len(), 1);
        assert_eq!(ctx.estimated_tokens, actual_chars / 4);
        assert!(!ctx.is_truncated);
    }

    #[test]
    fn packer_uses_character_budget_for_cjk() {
        let results = vec![search_match("a.rs", "汉字测试")];
        let ctx = pack_search_results(&results, 5);

        assert_eq!(ctx.file_clusters.len(), 1);
        assert!(ctx.file_clusters[0].snippet.contains("汉字测试"));
    }
}

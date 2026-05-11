use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Document {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct SearchIndex {
    documents: Vec<Document>,
    term_freq: HashMap<String, Vec<(usize, f32)>>,
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|s| s.to_lowercase())
        .filter(|s| s.len() >= 3)
        .collect()
}

impl SearchIndex {
    pub fn new() -> Self {
        Self {
            documents: Vec::new(),
            term_freq: HashMap::new(),
        }
    }

    pub fn add_document(&mut self, path: &str, content: &str) {
        let tokens = tokenize(content);
        let total_terms = tokens.len() as f32;
        if total_terms == 0.0 {
            self.documents.push(Document {
                path: path.to_string(),
                content: content.to_string(),
            });
            return;
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for token in &tokens {
            *counts.entry(token.clone()).or_insert(0) += 1;
        }

        let doc_idx = self.documents.len();
        self.documents.push(Document {
            path: path.to_string(),
            content: content.to_string(),
        });

        for (term, count) in counts {
            let tf = count as f32 / total_terms;
            self.term_freq.entry(term).or_default().push((doc_idx, tf));
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        let query_tokens = tokenize(query);
        if query_tokens.is_empty() || self.documents.is_empty() {
            return Vec::new();
        }

        let n = self.documents.len() as f32;
        let mut scores: HashMap<usize, f32> = HashMap::new();

        for term in query_tokens {
            let Some(postings) = self.term_freq.get(&term) else {
                continue;
            };
            let df = postings.len() as f32;
            if df == 0.0 {
                continue;
            }
            let idf = (n / df).ln();
            for &(doc_idx, tf) in postings {
                *scores.entry(doc_idx).or_insert(0.0) += tf * idf;
            }
        }

        let mut results: Vec<(usize, f32)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        results
            .into_iter()
            .map(|(idx, score)| (self.documents[idx].path.clone(), score))
            .collect()
    }
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_project_index(project_root: &Path) -> Result<SearchIndex, anyhow::Error> {
    let mut index = SearchIndex::new();
    let extensions: &[&str] = &["rs", "py", "js", "ts", "go", "java", "md", "toml"];

    let walker = ignore::WalkBuilder::new(project_root)
        .standard_filters(true)
        .build();

    for entry in walker {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !extensions.contains(&ext) {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let relative = path.strip_prefix(project_root).unwrap_or(path);
        index.add_document(&relative.to_string_lossy(), &content);
    }

    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_add_document() {
        let mut index = SearchIndex::new();
        index.add_document("foo.rs", "hello world");
        assert_eq!(index.documents.len(), 1);
        assert_eq!(index.documents[0].path, "foo.rs");
        assert_eq!(index.documents[0].content, "hello world");

        // "hello" and "world" should be indexed (length >= 3)
        assert!(index.term_freq.contains_key("hello"));
        assert!(index.term_freq.contains_key("world"));
        // "rs" is too short
        assert!(!index.term_freq.contains_key("rs"));
    }

    #[test]
    fn test_tfidf_search_basic() {
        let mut index = SearchIndex::new();
        index.add_document("a.rs", "the quick brown fox jumps over the lazy dog");
        index.add_document("b.rs", "the quick blue fox jumps over the sleepy cat");
        index.add_document("c.rs", "rust programming language");

        let results = index.search("quick brown fox", 2);
        assert_eq!(results.len(), 2);
        // a.rs should rank first because it contains "brown" (exclusive term)
        assert_eq!(results[0].0, "a.rs");
        assert!(results[0].1 > results[1].1);

        // Test that c.rs does not appear because it shares no terms
        let all_results = index.search("rust", 10);
        assert_eq!(all_results.len(), 1);
        assert_eq!(all_results[0].0, "c.rs");
    }
}

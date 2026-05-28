//! Persistent input history across sessions.
use std::path::{Path, PathBuf};

const MAX_HISTORY_ENTRIES: usize = 500;

/// Load input history from disk.
pub fn load_history() -> Vec<String> {
    let path = history_path();
    load_history_from(&path)
}

fn load_history_from(path: &Path) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }
    match crate::storage::read_text_file_capped(path) {
        Ok(content) => content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Save input history to disk.
pub fn save_history(history: &[String]) {
    let path = history_path();
    save_history_to(&path, history);
}

fn save_history_to(path: &Path, history: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let trimmed: Vec<&String> = history.iter().rev().take(MAX_HISTORY_ENTRIES).collect();
    let content: String = trimmed
        .iter()
        .rev()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = crate::storage::atomic::write_text_atomic(path, &content);
}

fn history_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("octocode")
        .join("input_history.txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_roundtrip() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("input_history.txt");
        let entries = vec!["hello".to_string(), "world".to_string()];
        save_history_to(&path, &entries);
        let loaded = load_history_from(&path);
        assert!(loaded.contains(&"hello".to_string()));
        assert!(loaded.contains(&"world".to_string()));
    }
}

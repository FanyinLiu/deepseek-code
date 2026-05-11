//! Parse @-mentions in user input and resolve them to file contents.
use std::path::Path;

/// Find all `@path` mentions in text and return the list of paths.
pub fn extract_mentions(text: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    for word in text.split_whitespace() {
        if let Some(path) = word.strip_prefix('@') {
            // Strip trailing punctuation
            let path = path.trim_end_matches(|c: char| c.is_ascii_punctuation());
            if !path.is_empty() {
                mentions.push(path.to_string());
            }
        }
    }
    mentions
}

/// Read mentioned files and build a context string to inject before the user message.
pub fn resolve_mentions(project_root: &Path, text: &str) -> String {
    let mentions = extract_mentions(text);
    if mentions.is_empty() {
        return String::new();
    }

    let mut context = String::from("📎 Referenced files:\n");
    for path in mentions {
        if let Some(full_path) =
            crate::workspace::paths::resolve_workspace_path(project_root, &path)
        {
            match std::fs::read_to_string(&full_path) {
                Ok(content) => {
                    context.push_str(&format!("\n--- {} ---\n{}", path, content));
                }
                Err(e) => {
                    context.push_str(&format!("\n--- {} ---\n[Error reading file: {}]", path, e));
                }
            }
        } else {
            context.push_str(&format!(
                "\n--- {} ---\n[Error: path outside workspace]",
                path
            ));
        }
    }
    context.push_str("\n---\n\n");
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mentions_basic() {
        let text = "Look at @src/main.rs and @lib/foo.py for details.";
        let m = extract_mentions(text);
        assert_eq!(m, vec!["src/main.rs", "lib/foo.py"]);
    }

    #[test]
    fn test_extract_mentions_with_punctuation() {
        let text = "Check @README.md, then @Cargo.toml!";
        let m = extract_mentions(text);
        assert_eq!(m, vec!["README.md", "Cargo.toml"]);
    }

    #[test]
    fn test_extract_mentions_none() {
        let text = "No mentions here.";
        let m = extract_mentions(text);
        assert!(m.is_empty());
    }
}

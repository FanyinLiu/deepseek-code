//! Parse @-mentions in user input and resolve them to file contents.
use std::path::Path;

/// Find all `@path` mentions in text and return the list of paths.
pub fn extract_mentions(text: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    let mut index = 0usize;
    while index < text.len() {
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        if ch != '@' || !is_mention_token_boundary(text, index) {
            index += ch.len_utf8();
            continue;
        }

        let after_at = index + ch.len_utf8();
        if text[after_at..].starts_with('"') {
            let path_start = after_at + 1;
            let path_end = text[path_start..]
                .find('"')
                .map_or(text.len(), |rel| path_start + rel);
            let path = &text[path_start..path_end];
            if !path.is_empty() {
                mentions.push(path.replace('\\', "/"));
            }
            index = if path_end < text.len() {
                path_end + 1
            } else {
                text.len()
            };
        } else {
            let rest = &text[after_at..];
            let path_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let path = rest[..path_len].trim_end_matches(|c: char| c.is_ascii_punctuation());
            if !path.is_empty() {
                mentions.push(path.replace('\\', "/"));
            }
            index = after_at + path_len;
        }
    }
    mentions
}

fn is_mention_token_boundary(text: &str, start: usize) -> bool {
    start == 0
        || text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
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
    fn test_extract_mentions_with_quoted_path() {
        let text = "Review @\"docs/My Plan.md\" and @src/lib.rs.";
        let m = extract_mentions(text);
        assert_eq!(m, vec!["docs/My Plan.md", "src/lib.rs"]);
    }

    #[test]
    fn test_extract_mentions_none() {
        let text = "No mentions here.";
        let m = extract_mentions(text);
        assert!(m.is_empty());
    }

    #[test]
    fn test_resolve_mentions_with_quoted_path() {
        let root = tempfile::tempdir().expect("tempdir");
        let docs = root.path().join("docs");
        std::fs::create_dir_all(&docs).expect("create docs");
        std::fs::write(docs.join("My Plan.md"), "ship it").expect("write file");

        let context = resolve_mentions(root.path(), "Read @\"docs/My Plan.md\"");

        assert!(context.contains("--- docs/My Plan.md ---"));
        assert!(context.contains("ship it"));
    }
}

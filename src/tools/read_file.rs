use std::path::Path;

/// Execute a `ReadFile` tool call.
/// For code files, automatically appends a structural overview (functions, classes, etc.)
/// before the content to help the model navigate large files.
pub fn read_file(
    project_root: &Path,
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, anyhow::Error> {
    let resolved = crate::workspace::paths::resolve_read_path(project_root, path)
        .ok_or_else(|| anyhow::anyhow!("path not found or unreadable: {path}"))?;

    let display_path = crate::workspace::paths::display_path(project_root, &resolved);

    let metadata = std::fs::metadata(&resolved)?;
    const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB
    if metadata.len() > MAX_FILE_SIZE {
        anyhow::bail!(
            "file too large ({} bytes, max {} MiB): {}",
            metadata.len(),
            MAX_FILE_SIZE / (1024 * 1024),
            display_path
        );
    }

    let content = std::fs::read_to_string(&resolved)?;

    // Extract code structure for supported languages
    let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");
    let structure = extract_structure(&content, ext);

    let lines: Vec<&str> = content.lines().collect();
    let start = offset.unwrap_or(0).min(lines.len());
    let end = limit.map_or(lines.len(), |l| (start + l).min(lines.len()));

    let selected = &lines[start..end];
    let mut out = format!("File: {display_path}\n");

    if let Some(structure_text) = structure {
        out.push_str(&structure_text);
    } else {
        out.push('\n');
    }

    for (i, line) in selected.iter().enumerate() {
        out.push_str(&format!("{:>6} | {}\n", start + i + 1, line));
    }

    if end < lines.len() {
        out.push_str(&format!(
            "\n... ({} more lines, {} total)\n",
            lines.len() - end,
            lines.len()
        ));
    }

    Ok(out)
}

/// Language-specific patterns for extracting code structure.
/// Each tuple is (line prefix after trimming, display kind).
fn language_patterns(ext: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match ext {
        "rs" => Some(&[
            ("fn ", "fn"),
            ("pub fn ", "fn"),
            ("async fn ", "fn"),
            ("pub async fn ", "fn"),
            ("struct ", "struct"),
            ("pub struct ", "struct"),
            ("enum ", "enum"),
            ("pub enum ", "enum"),
            ("trait ", "trait"),
            ("pub trait ", "trait"),
            ("impl ", "impl"),
            ("type ", "type"),
            ("pub type ", "type"),
            ("mod ", "mod"),
            ("pub mod ", "mod"),
            ("const ", "const"),
            ("pub const ", "const"),
            ("static ", "static"),
            ("pub static ", "static"),
            ("macro_rules! ", "macro"),
            ("use ", "use"),
            ("pub use ", "use"),
        ]),
        "py" | "pyi" => Some(&[("def ", "def"), ("async def ", "def"), ("class ", "class")]),
        "js" | "jsx" | "ts" | "tsx" => Some(&[
            ("function ", "function"),
            ("class ", "class"),
            ("const ", "const"),
            ("let ", "let"),
            ("export ", "export"),
        ]),
        "java" | "kt" => Some(&[
            ("class ", "class"),
            ("interface ", "interface"),
            ("enum ", "enum"),
            ("public class ", "class"),
            ("public interface ", "interface"),
            ("public enum ", "enum"),
            ("record ", "record"),
            ("public record ", "record"),
        ]),
        "go" => Some(&[
            ("func ", "func"),
            ("type ", "type"),
            ("var ", "var"),
            ("const ", "const"),
        ]),
        "c" | "cpp" | "h" | "hpp" | "cc" | "cxx" => Some(&[
            ("struct ", "struct"),
            ("enum ", "enum"),
            ("typedef ", "typedef"),
            ("class ", "class"),
            ("namespace ", "namespace"),
            ("#define ", "define"),
        ]),
        _ => None,
    }
}

/// Extract the first identifier after a prefix on a line.
fn extract_name(line: &str, prefix: &str) -> String {
    let after = &line[prefix.len()..];
    after
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '!')
        .next()
        .unwrap_or("?")
        .to_string()
}

/// Extract structural overview from a source file.
fn extract_structure(content: &str, ext: &str) -> Option<String> {
    let patterns = language_patterns(ext)?;
    let mut items = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        for &(prefix, kind) in patterns {
            if trimmed.starts_with(prefix) {
                let name = extract_name(trimmed, prefix);
                // Skip trivial entries (e.g. "use " with no module name)
                if name.is_empty() || name == "?" {
                    continue;
                }
                let snippet = trimmed.chars().take(72).collect::<String>();
                items.push(format!("{:>6} | {:<10} {}", i + 1, kind, snippet));
                break;
            }
        }
    }

    if items.is_empty() {
        return None;
    }

    let header = format!("  Structure ({} symbols):\n", items.len());
    Some(format!("{}  {}\n\n", header, items.join("\n  ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_structure_rust() {
        let content = r#"
use std::path::Path;

fn helper() {}

pub struct Foo {
    x: i32,
}

impl Foo {
    pub fn new() -> Self {}
}

enum Bar { A, B }

mod inner {
    fn private() {}
}
"#;
        let structure = extract_structure(content, "rs").unwrap();
        assert!(structure.contains("fn helper"));
        assert!(structure.contains("struct Foo"));
        assert!(structure.contains("impl Foo"));
        assert!(structure.contains("enum Bar"));
        assert!(structure.contains("mod inner"));
    }

    #[test]
    fn test_extract_structure_python() {
        let content = r#"
class MyClass:
    def method(self):
        pass

async def async_func():
    pass
"#;
        let structure = extract_structure(content, "py").unwrap();
        assert!(structure.contains("class MyClass"));
        assert!(structure.contains("def method"));
        assert!(structure.contains("def async_func"));
    }

    #[test]
    fn test_extract_structure_unsupported() {
        assert!(extract_structure("hello", "txt").is_none());
    }

    #[test]
    fn test_extract_structure_empty() {
        assert!(extract_structure("", "rs").is_none());
    }
}

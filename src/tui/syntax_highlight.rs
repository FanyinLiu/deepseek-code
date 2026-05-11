//! Syntax highlighting for code blocks using syntect.
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// Convert a syntect color to ratatui Color.
fn convert_color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Convert a syntect style to ratatui Style.
fn convert_style(s: SyntectStyle) -> Style {
    let mut style = Style::default().fg(convert_color(s.foreground));
    if s.font_style
        .contains(syntect::highlighting::FontStyle::BOLD)
    {
        style = style.add_modifier(ratatui::style::Modifier::BOLD);
    }
    if s.font_style
        .contains(syntect::highlighting::FontStyle::ITALIC)
    {
        style = style.add_modifier(ratatui::style::Modifier::ITALIC);
    }
    if s.font_style
        .contains(syntect::highlighting::FontStyle::UNDERLINE)
    {
        style = style.add_modifier(ratatui::style::Modifier::UNDERLINED);
    }
    style
}

/// Highlight a block of code and return ratatui Spans per line.
/// Each inner Vec is one line of styled spans.
pub fn highlight_code_block(code: &str, language: Option<&str>) -> Vec<Vec<Span<'static>>> {
    let ss = syntax_set();
    let ts = theme_set();

    let syntax = language
        .and_then(|lang| ss.find_syntax_by_token(lang))
        .or_else(|| ss.find_syntax_by_first_line(code))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let theme = &ts.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    for line in LinesWithEndings::from(code) {
        let stripped = line.strip_suffix('\n').unwrap_or(line);
        let regions = highlighter.highlight_line(stripped, ss).unwrap_or_default();
        let spans: Vec<Span<'static>> = regions
            .into_iter()
            .map(|(style, text)| Span::styled(text.to_string(), convert_style(style)))
            .collect();
        lines.push(spans);
    }
    lines
}

/// Guess language from a file path extension.
pub fn lang_from_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?;
    match ext {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" => Some("javascript"),
        "ts" => Some("typescript"),
        "jsx" => Some("jsx"),
        "tsx" => Some("tsx"),
        "go" => Some("go"),
        "java" => Some("java"),
        "kt" => Some("kotlin"),
        "cpp" | "cc" | "cxx" => Some("cpp"),
        "c" => Some("c"),
        "h" | "hpp" => Some("c++"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "scala" => Some("scala"),
        "sh" | "bash" => Some("bash"),
        "json" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "md" => Some("markdown"),
        "html" => Some("html"),
        "css" => Some("css"),
        "sql" => Some("sql"),
        "xml" => Some("xml"),
        _ => None,
    }
}

/// Simple markdown parser that splits text into plain text and code blocks.
#[derive(Debug, Clone)]
pub enum MarkdownBlock {
    Text(String),
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    Heading(u8, String),
    BlockQuote(String),
}

/// Parse markdown-like text into blocks for rendering.
/// This is a lightweight parser that only handles:
/// - ``` fenced code blocks
/// - # headings
/// - > blockquotes
/// - plain text
pub fn parse_markdown(text: &str) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut current_text = String::new();
    let mut in_code = false;
    let mut code_lang: Option<String> = None;
    let mut code_buffer = String::new();

    for line in text.lines() {
        if in_code {
            if line.trim_start().starts_with("```") {
                // End code block
                if !current_text.is_empty() {
                    blocks.push(MarkdownBlock::Text(current_text.trim().to_string()));
                    current_text.clear();
                }
                blocks.push(MarkdownBlock::CodeBlock {
                    language: code_lang.take(),
                    code: code_buffer.clone(),
                });
                code_buffer.clear();
                in_code = false;
            } else {
                code_buffer.push_str(line);
                code_buffer.push('\n');
            }
            continue;
        }

        if line.trim_start().starts_with("```") {
            // Start code block
            if !current_text.is_empty() {
                blocks.push(MarkdownBlock::Text(current_text.trim().to_string()));
                current_text.clear();
            }
            let tag = line.trim_start()[3..].trim();
            code_lang = if tag.is_empty() {
                None
            } else {
                Some(tag.to_string())
            };
            in_code = true;
            continue;
        }

        if let Some(heading) = line.strip_prefix("# ") {
            if !current_text.is_empty() {
                blocks.push(MarkdownBlock::Text(current_text.trim().to_string()));
                current_text.clear();
            }
            blocks.push(MarkdownBlock::Heading(1, heading.to_string()));
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            if !current_text.is_empty() {
                blocks.push(MarkdownBlock::Text(current_text.trim().to_string()));
                current_text.clear();
            }
            blocks.push(MarkdownBlock::Heading(2, heading.to_string()));
            continue;
        }
        if let Some(heading) = line.strip_prefix("### ") {
            if !current_text.is_empty() {
                blocks.push(MarkdownBlock::Text(current_text.trim().to_string()));
                current_text.clear();
            }
            blocks.push(MarkdownBlock::Heading(3, heading.to_string()));
            continue;
        }
        if let Some(quote) = line.strip_prefix("> ") {
            if !current_text.is_empty() {
                blocks.push(MarkdownBlock::Text(current_text.trim().to_string()));
                current_text.clear();
            }
            blocks.push(MarkdownBlock::BlockQuote(quote.to_string()));
            continue;
        }

        current_text.push_str(line);
        current_text.push('\n');
    }

    if in_code && !code_buffer.is_empty() {
        blocks.push(MarkdownBlock::CodeBlock {
            language: code_lang.take(),
            code: code_buffer,
        });
    } else if !current_text.is_empty() {
        blocks.push(MarkdownBlock::Text(current_text.trim().to_string()));
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_basic() {
        let text = "Hello\n\n```rust\nfn main() {}\n```\n\nWorld";
        let blocks = parse_markdown(text);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], MarkdownBlock::Text(t) if t == "Hello"));
        assert!(
            matches!(&blocks[1], MarkdownBlock::CodeBlock { language, .. } if language.as_deref() == Some("rust"))
        );
        assert!(matches!(&blocks[2], MarkdownBlock::Text(t) if t == "World"));
    }

    #[test]
    fn test_parse_markdown_heading() {
        let text = "# Title\n\nBody";
        let blocks = parse_markdown(text);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], MarkdownBlock::Heading(1, h) if h == "Title"));
    }

    #[test]
    fn test_highlight_rust() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = highlight_code_block(code, Some("rust"));
        assert_eq!(lines.len(), 3);
        // Each line should have at least one span
        assert!(!lines[0].is_empty());
    }
}

use std::path::Path;

use super::files::{MatchType, SearchMatch};

/// Search for symbol/identifier definitions using heuristic pattern matching.
/// v1 uses regex patterns; v2 may adopt tree-sitter for language-aware search.
pub fn search_symbols(
    project_root: &Path,
    symbol: &str,
    language: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    // Build language-specific patterns
    let patterns = symbol_patterns(symbol, language);

    let mut results = Vec::new();
    for pattern in &patterns {
        if results.len() >= limit {
            break;
        }
        let matches = super::code::search_code(
            project_root,
            pattern,
            language_glob(language),
            true, // symbols are case-sensitive
            limit - results.len(),
        )?;

        for mut m in matches {
            m.match_type = MatchType::Symbol;
            results.push(m);
        }
    }

    Ok(results)
}

/// Find references (call sites / usages) of a symbol across the codebase.
/// Uses heuristic regex patterns to find where the symbol is used.
pub fn find_references(
    project_root: &Path,
    symbol: &str,
    language: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    let mut results = Vec::new();

    // Build usage patterns (exclude definitions)
    let usage_patterns = reference_patterns(symbol, language);

    for pattern in &usage_patterns {
        if results.len() >= limit {
            break;
        }
        let matches = super::code::search_code(
            project_root,
            pattern,
            language_glob(language),
            true,
            limit - results.len(),
        )?;

        for mut m in matches {
            // Tag as reference rather than definition
            m.match_type = MatchType::CodeLine;
            results.push(m);
        }
    }

    Ok(results)
}

fn symbol_patterns(symbol: &str, language: Option<&str>) -> Vec<String> {
    let mut patterns = Vec::new();

    match language {
        Some("rust" | "rs") => {
            patterns.push(format!("fn {symbol}"));
            patterns.push(format!("struct {symbol}"));
            patterns.push(format!("enum {symbol}"));
            patterns.push(format!("trait {symbol}"));
            patterns.push(format!("impl {symbol}"));
            patterns.push(format!("mod {symbol}"));
            patterns.push(format!("const {symbol}"));
            patterns.push(format!("static {symbol}"));
        }
        Some("python" | "py") => {
            patterns.push(format!("def {symbol}"));
            patterns.push(format!("class {symbol}"));
        }
        Some("go") => {
            patterns.push(format!("func {symbol}"));
            patterns.push(format!("type {symbol}"));
        }
        Some("javascript" | "js" | "typescript" | "ts") => {
            patterns.push(format!("function {symbol}"));
            patterns.push(format!("class {symbol}"));
            patterns.push(format!("const {symbol}"));
            patterns.push(format!("export const {symbol}"));
        }
        _ => {
            // Generic: look for common definition patterns
            patterns.push(format!("fn {symbol}"));
            patterns.push(format!("def {symbol}"));
            patterns.push(format!("function {symbol}"));
            patterns.push(format!("class {symbol}"));
            patterns.push(format!("struct {symbol}"));
            patterns.push(format!("interface {symbol}"));
            patterns.push(format!("type {symbol}"));
        }
    }

    patterns
}

/// Patterns to find references/usages of a symbol.
fn reference_patterns(symbol: &str, language: Option<&str>) -> Vec<String> {
    let mut patterns = Vec::new();

    match language {
        Some("rust" | "rs") => {
            // Match calls like `symbol(...)` or `obj.symbol(...)` or `module::symbol`
            patterns.push(format!("{symbol}("));
            patterns.push(format!(".{symbol}"));
            patterns.push(format!("::{symbol}"));
            // Exclude definitions by not matching `fn symbol` etc.
        }
        Some("python" | "py") => {
            patterns.push(format!("{symbol}("));
            patterns.push(format!(".{symbol}"));
            patterns.push(format!("{symbol}."));
        }
        Some("go") => {
            patterns.push(format!("{symbol}("));
            patterns.push(format!(".{symbol}"));
        }
        Some("javascript" | "js" | "typescript" | "ts") => {
            patterns.push(format!("{symbol}("));
            patterns.push(format!(".{symbol}"));
            patterns.push(format!("{symbol}."));
            patterns.push(format!("new {symbol}"));
        }
        _ => {
            patterns.push(format!("{symbol}("));
            patterns.push(format!(".{symbol}"));
        }
    }

    patterns
}

fn language_glob(language: Option<&str>) -> Option<&str> {
    match language {
        Some("rust" | "rs") => Some("*.rs"),
        Some("python" | "py") => Some("*.py"),
        Some("go") => Some("*.go"),
        Some("javascript" | "js") => Some("*.{js,jsx}"),
        Some("typescript" | "ts") => Some("*.{ts,tsx}"),
        Some("java") => Some("*.java"),
        Some("c") => Some("*.{c,h}"),
        Some("cpp" | "c++") => Some("*.{cpp,hpp,cc,hh}"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_patterns_rust() {
        let p = symbol_patterns("foo", Some("rust"));
        assert!(p.contains(&"fn foo".to_string()));
        assert!(p.contains(&"struct foo".to_string()));
    }

    #[test]
    fn test_reference_patterns_rust() {
        let p = reference_patterns("foo", Some("rust"));
        assert!(p.contains(&"foo(".to_string()));
        assert!(p.contains(&".foo".to_string()));
    }
}

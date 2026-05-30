use std::path::PathBuf;

use crate::cli::resolve_project_root_or_cwd;
use crate::deepseek::fim::fim_request;
use crate::deepseek::DeepSeekModel;
use crate::provider::{build_provider, Provider};

/// Marker that splits the input into the FIM prefix and suffix.
const CURSOR_MARKER: &str = "<CURSOR>";

/// Split content into the FIM prefix/suffix at the first `<CURSOR>` marker.
/// Without a marker the whole input is the prefix (a plain continuation).
#[must_use]
pub fn split_at_cursor(content: &str) -> (&str, &str) {
    match content.split_once(CURSOR_MARKER) {
        Some((prefix, suffix)) => (prefix, suffix),
        None => (content, ""),
    }
}

/// Fill-in-the-middle completion (DeepSeek FIM, non-thinking lane).
///
/// Reads the file (or stdin), fills at the `<CURSOR>` marker, and prints the
/// completion. FIM is intentionally a single-file, non-agentic path, so it runs
/// on the fast Flash model rather than the chat/tool loop.
pub async fn complete(
    file: Option<PathBuf>,
    max_tokens: Option<u32>,
    json: bool,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root_or_cwd(None);
    let content = match &file {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };
    let (prefix, suffix) = split_at_cursor(&content);

    let api_key = crate::cli::login::resolve_api_key_non_interactive(Some(&root))?;
    let config = crate::storage::Config::load(Some(&root))?;
    let provider = build_provider(&config.provider, api_key);
    let client = provider.create_deepseek_client();

    let req = fim_request(&DeepSeekModel::Flash, prefix, suffix, max_tokens);
    let response = client.fim_completion(&req).await?;
    let text = response
        .choices
        .first()
        .map(|c| c.text.clone())
        .unwrap_or_default();

    if json {
        let out = serde_json::json!({ "completion": text });
        println!("{}", serde_json::to_string(&out)?);
    } else {
        println!("{text}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_uses_cursor_marker() {
        let (prefix, suffix) = split_at_cursor("fn add(a: i32) {<CURSOR>\n}");
        assert_eq!(prefix, "fn add(a: i32) {");
        assert_eq!(suffix, "\n}");
    }

    #[test]
    fn split_without_marker_is_pure_prefix() {
        let (prefix, suffix) = split_at_cursor("let x = ");
        assert_eq!(prefix, "let x = ");
        assert_eq!(suffix, "");
    }

    #[test]
    fn split_uses_only_the_first_marker() {
        let (prefix, suffix) = split_at_cursor("a<CURSOR>b<CURSOR>c");
        assert_eq!(prefix, "a");
        assert_eq!(suffix, "b<CURSOR>c");
    }
}

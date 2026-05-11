//! Web search using DuckDuckGo HTML (no API key required).
use reqwest;

/// A single search result.
#[derive(Debug, PartialEq)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Search the web via DuckDuckGo HTML and return a plain-text result list.
///
/// `limit` controls the max number of results. If `limit` is 0 it defaults to 10,
/// and it is clamped to a maximum of 20.
pub async fn web_search(query: &str, limit: usize) -> Result<String, anyhow::Error> {
    let limit = match limit {
        0 => 10,
        n => n.min(20),
    };

    let encoded = encode_query(query);
    let url = format!("https://html.duckduckgo.com/html/?q={encoded}");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("deepseek-code/0.1.0")
        .build()?;

    let response = client.get(&url).send().await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {status} for DuckDuckGo search");
    }

    let body = response.text().await?;
    let results = parse_ddg_results(&body);
    let trimmed: Vec<_> = results.into_iter().take(limit).collect();

    if trimmed.is_empty() {
        Ok("No results found.".to_string())
    } else {
        Ok(format_results(&trimmed))
    }
}

/// Percent-encode a query string for `application/x-www-form-urlencoded`.
fn encode_query(query: &str) -> String {
    let mut out = String::new();
    for byte in query.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", byte));
            }
        }
    }
    out
}

/// Parse DuckDuckGo HTML results using simple string scanning.
///
/// Looks for `.result__a`, `.result__snippet`, and `.result__url` classes.
fn parse_ddg_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut pos = 0;

    while let Some(a_idx) = html[pos..].find(r#"<a class="result__a""#) {
        let a_start = pos + a_idx;

        // Extract href.
        let href_tag_start = match html[a_start..].find("href=\"") {
            Some(i) => a_start + i + 6,
            None => {
                pos = a_start + 1;
                continue;
            }
        };
        let href_tag_end = match html[href_tag_start..].find('"') {
            Some(i) => href_tag_start + i,
            None => {
                pos = a_start + 1;
                continue;
            }
        };
        let href = html[href_tag_start..href_tag_end].to_string();

        // Extract title (text between > and </a>).
        let tag_open_end = match html[a_start..].find('>') {
            Some(i) => a_start + i + 1,
            None => {
                pos = a_start + 1;
                continue;
            }
        };
        let a_close = match html[tag_open_end..].find("</a>") {
            Some(i) => tag_open_end + i,
            None => {
                pos = a_start + 1;
                continue;
            }
        };
        let title = strip_tags(&html[tag_open_end..a_close]).trim().to_string();

        let after_a = a_close + 4;

        // Extract snippet after this <a>.
        let mut after_snippet = after_a;
        let snippet = if let Some(snippet_idx) = html[after_a..].find(r#"class="result__snippet""#)
        {
            let snippet_start_tag = after_a + snippet_idx;
            let text_start = match html[snippet_start_tag..].find('>') {
                Some(i) => snippet_start_tag + i + 1,
                None => {
                    pos = after_a;
                    continue;
                }
            };
            let text_end = if let Some(end_a) = html[text_start..].find("</a>") {
                if let Some(end_div) = html[text_start..].find("</div>") {
                    text_start + end_a.min(end_div)
                } else {
                    text_start + end_a
                }
            } else if let Some(end_div) = html[text_start..].find("</div>") {
                text_start + end_div
            } else {
                pos = after_a;
                continue;
            };
            after_snippet = text_end + 4;
            strip_tags(&html[text_start..text_end]).trim().to_string()
        } else {
            String::new()
        };

        // Extract displayed URL after snippet.
        let url = if let Some(url_idx) = html[after_snippet..].find(r#"class="result__url""#) {
            let url_start_tag = after_snippet + url_idx;
            if let Some(text_start) = html[url_start_tag..].find('>') {
                let text_start = url_start_tag + text_start + 1;
                if let Some(text_end) = html[text_start..].find("</a>") {
                    strip_tags(&html[text_start..text_start + text_end])
                        .trim()
                        .to_string()
                } else {
                    href
                }
            } else {
                href
            }
        } else {
            href
        };

        results.push(SearchResult {
            title,
            url,
            snippet,
        });

        pos = after_a;
    }

    results
}

/// Simple tag stripper and entity decoder.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Format a slice of results as a numbered list.
fn format_results(results: &[SearchResult]) -> String {
    let mut out = String::new();
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!(
            "{}. {}\n{}\n{}\n\n",
            i + 1,
            r.title,
            r.url,
            r.snippet
        ));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ddg_results_basic() {
        let html = r#"
<html>
<body>
<div class="result">
  <h2 class="result__title">
    <a class="result__a" href="https://example.com/page1">First <b>Result</b> Title</a>
  </h2>
  <a class="result__snippet" href="https://example.com/page1">This is the <b>first</b> result snippet.</a>
  <div class="result__extras">
    <a class="result__url" href="https://example.com/page1">example.com/page1</a>
  </div>
</div>
<div class="result">
  <h2 class="result__title">
    <a class="result__a" href="https://example.org/page2">Second Result Title</a>
  </h2>
  <div class="result__snippet">This is the second result snippet.</div>
  <div class="result__extras">
    <a class="result__url" href="https://example.org/page2">example.org/page2</a>
  </div>
</div>
</body>
</html>
"#;
        let results = parse_ddg_results(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Result Title");
        assert_eq!(results[0].url, "example.com/page1");
        assert_eq!(results[0].snippet, "This is the first result snippet.");
        assert_eq!(results[1].title, "Second Result Title");
        assert_eq!(results[1].url, "example.org/page2");
        assert_eq!(results[1].snippet, "This is the second result snippet.");
    }

    #[test]
    fn test_format_results() {
        let results = vec![
            SearchResult {
                title: "Title One".to_string(),
                url: "https://one.com".to_string(),
                snippet: "Snippet one.".to_string(),
            },
            SearchResult {
                title: "Title Two".to_string(),
                url: "https://two.com".to_string(),
                snippet: "Snippet two.".to_string(),
            },
        ];
        let formatted = format_results(&results);
        assert!(formatted.contains("1. Title One"));
        assert!(formatted.contains("https://one.com"));
        assert!(formatted.contains("Snippet one."));
        assert!(formatted.contains("2. Title Two"));
        assert!(formatted.contains("https://two.com"));
        assert!(formatted.contains("Snippet two."));
    }
}

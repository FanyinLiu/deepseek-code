//! GitHub PR integration helpers.

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;

const BASE_URL: &str = "https://api.github.com";
const USER_AGENT: &str = "octocode/0.1.0";
const MAX_GITHUB_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_GITHUB_DIFF_BYTES: usize = 2 * 1024 * 1024;
const MAX_GITHUB_ERROR_BODY_BYTES: usize = 64 * 1024;

fn build_client() -> Result<reqwest::Client, anyhow::Error> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(USER_AGENT),
    );
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        let auth = format!("Bearer {}", token);
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&auth) {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }
    }
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .default_headers(headers)
        .build()?)
}

/// List open or closed PRs for a repository.
pub async fn list_prs(owner: &str, repo: &str, state: &str) -> Result<String, anyhow::Error> {
    let client = build_client()?;
    let url = format!(
        "{}/repos/{}/{}/pulls?state={}",
        BASE_URL, owner, repo, state
    );
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = github_error_body(resp).await;
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let json = response_json_capped(resp, MAX_GITHUB_JSON_BYTES).await?;
    Ok(format_pr_list(&json))
}

/// Get details for a single PR.
pub async fn get_pr(owner: &str, repo: &str, number: u64) -> Result<String, anyhow::Error> {
    let client = build_client()?;
    let url = format!("{}/repos/{}/{}/pulls/{}", BASE_URL, owner, repo, number);
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = github_error_body(resp).await;
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let json = response_json_capped(resp, MAX_GITHUB_JSON_BYTES).await?;
    Ok(format_pr_detail(&json))
}

/// Get the diff for a PR.
pub async fn get_pr_diff(owner: &str, repo: &str, number: u64) -> Result<String, anyhow::Error> {
    let client = build_client()?;
    let url = format!("{}/repos/{}/{}/pulls/{}", BASE_URL, owner, repo, number);
    let resp = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github.diff")
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let text = github_error_body(resp).await;
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let body = response_text_capped(resp, MAX_GITHUB_DIFF_BYTES).await?;
    Ok(format!("# PR {} Diff\n\n```diff\n{}\n```", number, body))
}

/// Add a comment to a PR.
pub async fn comment_pr(
    owner: &str,
    repo: &str,
    number: u64,
    body: &str,
) -> Result<String, anyhow::Error> {
    if std::env::var("GITHUB_TOKEN").is_err() {
        anyhow::bail!("GITHUB_TOKEN environment variable is required to comment on PRs");
    }
    let client = build_client()?;
    let url = format!(
        "{}/repos/{}/{}/issues/{}/comments",
        BASE_URL, owner, repo, number
    );
    let payload = serde_json::json!({ "body": body });
    let resp = client.post(&url).json(&payload).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = github_error_body(resp).await;
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let json = response_json_capped(resp, MAX_GITHUB_JSON_BYTES).await?;
    Ok(format_comment_response(&json))
}

async fn github_error_body(response: reqwest::Response) -> String {
    response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES)
        .await
        .unwrap_or_else(|error| format!("[response body unavailable: {error}]"))
}

async fn response_json_capped(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Value, anyhow::Error> {
    let bytes = response_bytes_capped(response, max_bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn response_text_capped(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<String, anyhow::Error> {
    let bytes = response_bytes_capped(response, max_bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

async fn response_bytes_capped(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, anyhow::Error> {
    if let Some(content_length) = response.content_length() {
        if content_length > max_bytes as u64 {
            anyhow::bail!(
                "response too large: {content_length} bytes exceeds {max_bytes} byte limit"
            );
        }
    }
    collect_limited_body(response.bytes_stream(), max_bytes).await
}

async fn collect_limited_body<S, E>(stream: S, max_bytes: usize) -> Result<Vec<u8>, anyhow::Error>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    futures::pin_mut!(stream);

    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            anyhow::bail!("response too large: exceeds {max_bytes} byte limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn format_pr_list(value: &Value) -> String {
    let mut out = String::from("# Pull Requests\n\n");
    let Some(arr) = value.as_array() else {
        return out;
    };
    if arr.is_empty() {
        out.push_str("No pull requests found.\n");
        return out;
    }
    for pr in arr {
        let number = pr["number"].as_u64().unwrap_or(0);
        let title = pr["title"].as_str().unwrap_or("");
        let state = pr["state"].as_str().unwrap_or("");
        let user = pr["user"]["login"].as_str().unwrap_or("");
        let html_url = pr["html_url"].as_str().unwrap_or("");
        out.push_str(&format!("## PR #{} – {}\n", number, title));
        out.push_str(&format!("- **State:** {}\n", state));
        out.push_str(&format!("- **Author:** {}\n", user));
        out.push_str(&format!("- **URL:** {}\n\n", html_url));
    }
    out
}

fn format_pr_detail(value: &Value) -> String {
    let number = value["number"].as_u64().unwrap_or(0);
    let title = value["title"].as_str().unwrap_or("");
    let state = value["state"].as_str().unwrap_or("");
    let user = value["user"]["login"].as_str().unwrap_or("");
    let body = value["body"].as_str().unwrap_or("");
    let html_url = value["html_url"].as_str().unwrap_or("");
    let head = value["head"]["label"].as_str().unwrap_or("");
    let base = value["base"]["label"].as_str().unwrap_or("");

    format!(
        "# PR #{} – {}\n\n- **State:** {}\n- **Author:** {}\n- **Head:** {}\n- **Base:** {}\n- **URL:** {}\n\n## Description\n\n{}\n",
        number, title, state, user, head, base, html_url,
        if body.is_empty() { "*(No description provided)*" } else { body }
    )
}

fn format_comment_response(value: &Value) -> String {
    let id = value["id"].as_u64().unwrap_or(0);
    let html_url = value["html_url"].as_str().unwrap_or("");
    format!(
        "# Comment Created\n\n- **ID:** {}\n- **URL:** {}\n",
        id, html_url
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_pr_list() {
        let json = serde_json::json!([
            {
                "number": 42,
                "title": "Fix memory leak",
                "state": "open",
                "user": { "login": "alice" },
                "html_url": "https://github.com/owner/repo/pull/42"
            },
            {
                "number": 43,
                "title": "Update docs",
                "state": "closed",
                "user": { "login": "bob" },
                "html_url": "https://github.com/owner/repo/pull/43"
            }
        ]);
        let text = format_pr_list(&json);
        assert!(text.contains("PR #42"));
        assert!(text.contains("Fix memory leak"));
        assert!(text.contains("alice"));
        assert!(text.contains("PR #43"));
        assert!(text.contains("Update docs"));
        assert!(text.contains("bob"));
    }

    #[test]
    fn test_format_pr_list_empty() {
        let json = serde_json::json!([]);
        let text = format_pr_list(&json);
        assert!(text.contains("No pull requests found"));
    }

    #[test]
    fn test_format_pr_detail() {
        let json = serde_json::json!({
            "number": 7,
            "title": "Add caching",
            "state": "open",
            "user": { "login": "charlie" },
            "body": "This PR adds a cache layer.",
            "html_url": "https://github.com/owner/repo/pull/7",
            "head": { "label": "owner:feature/cache" },
            "base": { "label": "owner:main" }
        });
        let text = format_pr_detail(&json);
        assert!(text.contains("PR #7"));
        assert!(text.contains("Add caching"));
        assert!(text.contains("charlie"));
        assert!(text.contains("This PR adds a cache layer"));
        assert!(text.contains("owner:feature/cache"));
        assert!(text.contains("owner:main"));
    }

    #[test]
    fn test_format_pr_detail_no_body() {
        let json = serde_json::json!({
            "number": 8,
            "title": "Hotfix",
            "state": "closed",
            "user": { "login": "dave" },
            "body": null,
            "html_url": "https://github.com/owner/repo/pull/8",
            "head": { "label": "owner:hotfix" },
            "base": { "label": "owner:main" }
        });
        let text = format_pr_detail(&json);
        assert!(text.contains("*(No description provided)*"));
    }

    #[test]
    fn test_format_comment_response() {
        let json = serde_json::json!({
            "id": 12345,
            "html_url": "https://github.com/owner/repo/issues/1#issuecomment-12345"
        });
        let text = format_comment_response(&json);
        assert!(text.contains("12345"));
        assert!(text.contains("https://github.com/owner/repo/issues/1#issuecomment-12345"));
    }

    #[tokio::test]
    async fn test_collect_limited_body_rejects_large_response() {
        let chunks = futures::stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(b"12345")),
            Ok::<_, std::io::Error>(Bytes::from_static(b"67890")),
        ]);

        let error = collect_limited_body(chunks, 8)
            .await
            .expect_err("body over limit should fail");
        assert!(
            error.to_string().contains("response too large"),
            "unexpected error: {error}"
        );
    }
}

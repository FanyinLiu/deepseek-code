//! GitHub PR integration helpers.

use reqwest;
use serde_json::Value;

const BASE_URL: &str = "https://api.github.com";
const USER_AGENT: &str = "deepseek-code/0.1.0";

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
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let json: Value = resp.json().await?;
    Ok(format_pr_list(&json))
}

/// Get details for a single PR.
pub async fn get_pr(owner: &str, repo: &str, number: u64) -> Result<String, anyhow::Error> {
    let client = build_client()?;
    let url = format!("{}/repos/{}/{}/pulls/{}", BASE_URL, owner, repo, number);
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let json: Value = resp.json().await?;
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
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let body = resp.text().await?;
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
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub API error {}: {}", status, text);
    }
    let json: Value = resp.json().await?;
    Ok(format_comment_response(&json))
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
}

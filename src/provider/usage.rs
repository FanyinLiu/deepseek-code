use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ProviderConfig, ProviderKind};

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const MAX_PROVIDER_USAGE_JSON_BYTES: usize = 1024 * 1024;
const MAX_PROVIDER_USAGE_ERROR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderUsageSnapshot {
    pub provider: ProviderKind,
    pub display_name: String,
    pub updated_at: DateTime<Utc>,
    pub source: UsageSnapshotSource,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<UsageBalance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<UsageWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageSnapshotSource {
    ProviderApi,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageBalance {
    pub label: String,
    pub currency: String,
    pub total: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topped_up: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageWindow {
    pub label: String,
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<DateTime<Utc>>,
}

pub async fn fetch_usage_snapshot(
    config: &ProviderConfig,
    api_key: &str,
) -> Result<ProviderUsageSnapshot, anyhow::Error> {
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    match config.default {
        ProviderKind::DeepSeek => fetch_deepseek_balance(&client, config, api_key).await,
        ProviderKind::OpenRouter => fetch_openrouter_credits(&client, config, api_key).await,
        kind => Ok(unsupported_snapshot(kind)),
    }
}

fn unsupported_snapshot(kind: ProviderKind) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        provider: kind,
        display_name: kind.display_name().to_string(),
        updated_at: Utc::now(),
        source: UsageSnapshotSource::Unsupported,
        balances: Vec::new(),
        windows: Vec::new(),
        plan: None,
        error: Some(format!(
            "usage balance snapshots are not implemented for {} yet",
            kind.as_str()
        )),
    }
}

async fn fetch_deepseek_balance(
    client: &Client,
    config: &ProviderConfig,
    api_key: &str,
) -> Result<ProviderUsageSnapshot, anyhow::Error> {
    let url = usage_url(
        config.deepseek.base_url.as_deref(),
        "https://api.deepseek.com",
        "/user/balance",
        true,
    );
    let response = client.get(url).bearer_auth(api_key).send().await?;
    let response = require_success(response).await?;
    let payload: DeepSeekBalanceResponse =
        response_json_capped(response, MAX_PROVIDER_USAGE_JSON_BYTES).await?;
    let balances = payload
        .balance_infos
        .into_iter()
        .map(|info| UsageBalance {
            label: "balance".to_string(),
            currency: info.currency,
            total: parse_amount(&info.total_balance).unwrap_or_default(),
            used: None,
            remaining: parse_amount(&info.total_balance),
            granted: parse_amount(&info.granted_balance),
            topped_up: parse_amount(&info.topped_up_balance),
        })
        .collect::<Vec<_>>();

    Ok(ProviderUsageSnapshot {
        provider: ProviderKind::DeepSeek,
        display_name: ProviderKind::DeepSeek.display_name().to_string(),
        updated_at: Utc::now(),
        source: UsageSnapshotSource::ProviderApi,
        balances,
        windows: Vec::new(),
        plan: None,
        error: (!payload.is_available).then(|| "DeepSeek reports balance unavailable".to_string()),
    })
}

async fn fetch_openrouter_credits(
    client: &Client,
    config: &ProviderConfig,
    api_key: &str,
) -> Result<ProviderUsageSnapshot, anyhow::Error> {
    let url = usage_url(
        config.openrouter.base_url.as_deref(),
        "https://openrouter.ai/api/v1",
        "/credits",
        false,
    );
    let response = client.get(url).bearer_auth(api_key).send().await?;
    let response = require_success(response).await?;
    let payload: OpenRouterCreditsResponse =
        response_json_capped(response, MAX_PROVIDER_USAGE_JSON_BYTES).await?;
    let remaining = payload.data.total_credits - payload.data.total_usage;

    Ok(ProviderUsageSnapshot {
        provider: ProviderKind::OpenRouter,
        display_name: ProviderKind::OpenRouter.display_name().to_string(),
        updated_at: Utc::now(),
        source: UsageSnapshotSource::ProviderApi,
        balances: vec![UsageBalance {
            label: "credits".to_string(),
            currency: "USD".to_string(),
            total: payload.data.total_credits,
            used: Some(payload.data.total_usage),
            remaining: Some(remaining),
            granted: None,
            topped_up: None,
        }],
        windows: Vec::new(),
        plan: None,
        error: None,
    })
}

async fn require_success(response: reqwest::Response) -> Result<reqwest::Response, anyhow::Error> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response_text_capped(response, MAX_PROVIDER_USAGE_ERROR_BYTES)
        .await
        .unwrap_or_else(|error| format!("[response body unavailable: {error}]"));
    anyhow::bail!("provider usage endpoint returned HTTP {status}: {body}");
}

async fn response_json_capped<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<T, anyhow::Error> {
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

fn usage_url(base_url: Option<&str>, default_base_url: &str, path: &str, strip_v1: bool) -> String {
    let mut base = base_url.unwrap_or(default_base_url).trim_end_matches('/');
    if strip_v1 {
        base = base.strip_suffix("/v1").unwrap_or(base);
    }
    format!("{base}{path}")
}

fn parse_amount(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok()
}

#[derive(Debug, Deserialize)]
struct DeepSeekBalanceResponse {
    is_available: bool,
    #[serde(default)]
    balance_infos: Vec<DeepSeekBalanceInfo>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekBalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCreditsResponse {
    data: OpenRouterCreditsData,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCreditsData {
    total_credits: f64,
    total_usage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_usage_url_strips_v1_for_balance_endpoint() {
        assert_eq!(
            usage_url(
                Some("https://api.deepseek.com/v1"),
                "https://api.deepseek.com",
                "/user/balance",
                true,
            ),
            "https://api.deepseek.com/user/balance"
        );
    }

    #[test]
    fn unsupported_snapshot_is_explicit() {
        let snapshot = unsupported_snapshot(ProviderKind::Kimi);

        assert_eq!(snapshot.provider, ProviderKind::Kimi);
        assert_eq!(snapshot.source, UsageSnapshotSource::Unsupported);
        assert!(snapshot.error.expect("error").contains("kimi"));
    }

    #[tokio::test]
    async fn collect_limited_body_rejects_large_usage_response() {
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

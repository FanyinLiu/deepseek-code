use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ProviderConfig, ProviderKind};

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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
    let payload: DeepSeekBalanceResponse = response.json().await?;
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
    let payload: OpenRouterCreditsResponse = response.json().await?;
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
    let body = response.text().await.unwrap_or_default();
    anyhow::bail!("provider usage endpoint returned HTTP {status}: {body}");
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
}

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::deepseek::{CacheUsage, Usage};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageEvent {
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_miss_tokens: u64,
    pub source: UsageEventSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageEventSource {
    ModelResponse,
    Imported,
}

impl UsageEvent {
    #[must_use]
    pub fn from_model_response(provider: &str, model: &str, usage: &Usage) -> Self {
        let cache = CacheUsage::from_usage(usage);
        Self {
            timestamp: Utc::now(),
            provider: provider.to_string(),
            model: model.to_string(),
            prompt_tokens: u64::from(usage.prompt_tokens),
            completion_tokens: u64::from(usage.completion_tokens),
            total_tokens: u64::from(usage.total_tokens),
            cache_read_tokens: cache.prompt_cache_hit_tokens,
            cache_write_tokens: 0,
            cache_miss_tokens: cache.prompt_cache_miss_tokens,
            source: UsageEventSource::ModelResponse,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageStore {
    root: PathBuf,
}

impl UsageStore {
    #[must_use]
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            root: project_root.into(),
        }
    }

    #[must_use]
    pub fn events_path(&self) -> PathBuf {
        self.root
            .join(".octocode")
            .join("usage")
            .join("events.jsonl")
    }

    pub fn append(&self, event: &UsageEvent) -> Result<(), anyhow::Error> {
        let line = serde_json::to_string(event)?;
        crate::storage::atomic::append_jsonl_locked(&self.events_path(), &line)
    }

    pub fn load_events(&self) -> Result<Vec<UsageEvent>, anyhow::Error> {
        let path = self.events_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)?;
        let mut events = Vec::new();
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event = serde_json::from_str(trimmed).map_err(|error| {
                anyhow::anyhow!(
                    "invalid usage event at {}:{}: {error}",
                    path.display(),
                    index + 1
                )
            })?;
            events.push(event);
        }
        Ok(events)
    }

    pub fn summarize(&self, days: Option<u32>) -> Result<UsageSummary, anyhow::Error> {
        let now = Utc::now();
        let since = days.map(|days| now - Duration::days(i64::from(days)));
        Ok(UsageSummary::from_events(self.load_events()?, since))
    }
}

pub fn record_usage_event(
    project_root: &Path,
    provider: &str,
    model: &str,
    usage: &Usage,
) -> Result<(), anyhow::Error> {
    UsageStore::new(project_root).append(&UsageEvent::from_model_response(provider, model, usage))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsageTotals {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_miss_tokens: u64,
    pub cache_hit_rate: Option<f64>,
}

impl UsageTotals {
    fn add_event(&mut self, event: &UsageEvent) {
        self.requests += 1;
        self.prompt_tokens = self.prompt_tokens.saturating_add(event.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(event.completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(event.total_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(event.cache_read_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(event.cache_write_tokens);
        self.cache_miss_tokens = self
            .cache_miss_tokens
            .saturating_add(event.cache_miss_tokens);
        self.cache_hit_rate = cache_hit_rate(self.cache_read_tokens, self.cache_miss_tokens);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageModelSummary {
    pub provider: String,
    pub model: String,
    #[serde(flatten)]
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageSummary {
    pub since: Option<DateTime<Utc>>,
    pub generated_at: DateTime<Utc>,
    pub totals: UsageTotals,
    pub by_model: Vec<UsageModelSummary>,
}

impl UsageSummary {
    #[must_use]
    pub fn from_events(events: Vec<UsageEvent>, since: Option<DateTime<Utc>>) -> Self {
        let mut totals = UsageTotals::default();
        let mut by_model = BTreeMap::<(String, String), UsageTotals>::new();

        for event in events.iter().filter(|event| match since {
            Some(since) => event.timestamp >= since,
            None => true,
        }) {
            totals.add_event(event);
            by_model
                .entry((event.provider.clone(), event.model.clone()))
                .or_default()
                .add_event(event);
        }

        let by_model = by_model
            .into_iter()
            .map(|((provider, model), totals)| UsageModelSummary {
                provider,
                model,
                totals,
            })
            .collect();

        Self {
            since,
            generated_at: Utc::now(),
            totals,
            by_model,
        }
    }
}

#[must_use]
pub fn cache_hit_rate(cache_read_tokens: u64, cache_miss_tokens: u64) -> Option<f64> {
    let total = cache_read_tokens.saturating_add(cache_miss_tokens);
    (total > 0).then_some(cache_read_tokens as f64 / total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_groups_events_by_provider_and_model() {
        let now = Utc::now();
        let events = vec![
            UsageEvent {
                timestamp: now,
                provider: "deepseek".to_string(),
                model: "deepseek-v4-flash".to_string(),
                prompt_tokens: 100,
                completion_tokens: 25,
                total_tokens: 125,
                cache_read_tokens: 80,
                cache_write_tokens: 0,
                cache_miss_tokens: 20,
                source: UsageEventSource::ModelResponse,
            },
            UsageEvent {
                timestamp: now,
                provider: "openrouter".to_string(),
                model: "deepseek/deepseek-v4".to_string(),
                prompt_tokens: 50,
                completion_tokens: 10,
                total_tokens: 60,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cache_miss_tokens: 0,
                source: UsageEventSource::ModelResponse,
            },
        ];

        let summary = UsageSummary::from_events(events, None);

        assert_eq!(summary.totals.requests, 2);
        assert_eq!(summary.totals.total_tokens, 185);
        assert_eq!(summary.by_model.len(), 2);
        assert_eq!(summary.by_model[0].provider, "deepseek");
        assert_eq!(summary.by_model[0].totals.cache_hit_rate, Some(0.8));
    }
}

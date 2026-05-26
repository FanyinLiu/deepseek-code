use std::path::PathBuf;

use crate::cli::resolve_project_root;
use crate::provider::usage::ProviderUsageSnapshot;
use crate::storage::{self, UsageSummary, UsageTotals};

#[derive(Debug, Clone)]
pub enum UsageCommand {
    Summary { json: bool, days: Option<u32> },
    Balance { json: bool },
}

pub async fn usage(
    command: UsageCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "usage")?;
    match command {
        UsageCommand::Summary { json, days } => print_summary(&root, json, days),
        UsageCommand::Balance { json } => print_balance(&root, json).await,
    }
}

fn print_summary(
    root: &std::path::Path,
    json: bool,
    days: Option<u32>,
) -> Result<(), anyhow::Error> {
    let summary = storage::UsageStore::new(root).summarize(days)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_human_summary(&summary);
    }
    Ok(())
}

async fn print_balance(root: &std::path::Path, json: bool) -> Result<(), anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    let provider = config.provider.default;
    let Some(api_key) = storage::get_effective_api_key(Some(root)) else {
        anyhow::bail!(
            "No API key configured. Run `octo login --api-key <key>` or set {}.",
            storage::api_key_env_hint(provider)
        );
    };
    let snapshot = crate::provider::usage::fetch_usage_snapshot(&config.provider, &api_key).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        print_human_balance(&snapshot);
    }
    Ok(())
}

fn print_human_summary(summary: &UsageSummary) {
    println!("Octocode usage summary");
    if let Some(since) = summary.since {
        println!("Since: {}", since.to_rfc3339());
    }
    println!();
    print_totals("Total", &summary.totals);

    if summary.by_model.is_empty() {
        println!("No usage events recorded yet.");
        return;
    }

    println!();
    println!("By model:");
    for item in &summary.by_model {
        let label = format!("{}/{}", item.provider, item.model);
        print_totals(&label, &item.totals);
    }
}

fn print_totals(label: &str, totals: &UsageTotals) {
    println!(
        "{label}: requests={} tokens={} prompt={} completion={} cache_read={} cache_miss={} cache_hit={}",
        totals.requests,
        totals.total_tokens,
        totals.prompt_tokens,
        totals.completion_tokens,
        totals.cache_read_tokens,
        totals.cache_miss_tokens,
        format_hit_rate(totals.cache_hit_rate),
    );
}

fn print_human_balance(snapshot: &ProviderUsageSnapshot) {
    println!(
        "{} usage snapshot ({})",
        snapshot.display_name,
        snapshot.provider.as_str()
    );
    if let Some(error) = &snapshot.error {
        println!("Status: {error}");
    }
    if let Some(plan) = &snapshot.plan {
        println!("Plan: {plan}");
    }
    for balance in &snapshot.balances {
        let remaining = balance
            .remaining
            .map(format_amount)
            .unwrap_or_else(|| "-".to_string());
        let used = balance
            .used
            .map(format_amount)
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{}: remaining={} {} total={} used={}",
            balance.label,
            remaining,
            balance.currency,
            format_amount(balance.total),
            used,
        );
        if balance.granted.is_some() || balance.topped_up.is_some() {
            println!(
                "  granted={} topped_up={}",
                balance
                    .granted
                    .map(format_amount)
                    .unwrap_or_else(|| "-".to_string()),
                balance
                    .topped_up
                    .map(format_amount)
                    .unwrap_or_else(|| "-".to_string())
            );
        }
    }
    for window in &snapshot.windows {
        let reset = window
            .reset_at
            .map(|reset| reset.to_rfc3339())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{}: used={:.0}% left={:.0}% reset={}",
            window.label,
            window.used_percent,
            (100.0 - window.used_percent).max(0.0),
            reset,
        );
    }
    if snapshot.balances.is_empty() && snapshot.windows.is_empty() && snapshot.error.is_none() {
        println!("No usage data returned by provider.");
    }
}

fn format_hit_rate(value: Option<f64>) -> String {
    value
        .map(|rate| format!("{:.1}%", rate * 100.0))
        .unwrap_or_else(|| "-".to_string())
}

fn format_amount(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.4}")
    }
}

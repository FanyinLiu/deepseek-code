use super::models::{CacheUsage, Usage};

/// Extract cache usage from a Usage struct.
#[must_use]
pub fn extract_cache_usage(usage: &Usage) -> CacheUsage {
    CacheUsage::from_usage(usage)
}

/// Compute the estimated cost for a request based on token usage and cache hit rate.
/// Prices are approximate and should be updated from the official pricing page.
#[must_use]
pub fn estimate_cost(
    _prompt_tokens: u64,
    completion_tokens: u64,
    cache_hit_tokens: u64,
    cache_miss_tokens: u64,
    is_pro: bool,
) -> f64 {
    // DeepSeek V4 pricing (approximate, update from https://api-docs.deepseek.com/quick_start/pricing)
    let (prompt_price_per_m, completion_price_per_m, cache_hit_price_per_m) = if is_pro {
        (0.55, 2.19, 0.14) // V4 Pro
    } else {
        (0.27, 1.10, 0.07) // V4 Flash
    };

    let prompt_cost = (cache_miss_tokens as f64 / 1_000_000.0) * prompt_price_per_m;
    let cache_cost = (cache_hit_tokens as f64 / 1_000_000.0) * cache_hit_price_per_m;
    let completion_cost = (completion_tokens as f64 / 1_000_000.0) * completion_price_per_m;

    prompt_cost + cache_cost + completion_cost
}

/// Format cache hit rate as a human-readable percentage string.
#[must_use]
pub fn format_hit_rate(usage: &CacheUsage) -> String {
    format!("{:.0}%", usage.hit_rate() * 100.0)
}

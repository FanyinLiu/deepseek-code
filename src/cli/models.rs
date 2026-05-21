use std::path::PathBuf;

use serde::Serialize;

use crate::cli::resolve_project_root_or_cwd;
use crate::deepseek::DeepSeekModel;
use crate::provider::{
    build_provider, model_context_window_tokens, model_max_output_tokens, Provider,
    ProviderCapabilities, ProviderConfig, ProviderKind,
};
use crate::storage;

pub async fn models(json: bool, project_root: Option<PathBuf>) -> Result<(), anyhow::Error> {
    let root = resolve_project_root_or_cwd(project_root);
    let config = match storage::Config::load(Some(&root)) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("warning: failed to load config (using defaults): {e}");
            storage::Config::default()
        }
    };
    let payload = models_payload(&config.provider);
    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_models(&payload);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct ModelsPayload {
    active_provider: ProviderKind,
    providers: Vec<ProviderModelCapability>,
}

#[derive(Debug, Serialize)]
struct ProviderModelCapability {
    provider: ProviderKind,
    display_name: &'static str,
    active: bool,
    base_url: Option<String>,
    pro_model: String,
    flash_model: String,
    pro_context_window_tokens: Option<u64>,
    flash_context_window_tokens: Option<u64>,
    pro_max_output_tokens: Option<u64>,
    flash_max_output_tokens: Option<u64>,
    capabilities: ProviderCapabilities,
}

fn models_payload(config: &ProviderConfig) -> ModelsPayload {
    ModelsPayload {
        active_provider: config.default,
        providers: ProviderKind::all()
            .iter()
            .copied()
            .map(|kind| provider_payload(config, kind))
            .collect(),
    }
}

fn provider_payload(config: &ProviderConfig, kind: ProviderKind) -> ProviderModelCapability {
    let mut scoped = config.clone();
    scoped.default = kind;
    let provider = build_provider(&scoped, String::new());
    let capabilities = kind.capabilities();
    ProviderModelCapability {
        provider: kind,
        display_name: kind.display_name(),
        active: config.default == kind,
        base_url: provider.base_url().map(str::to_string),
        pro_model: provider.request_model_name(&DeepSeekModel::Pro),
        flash_model: provider.request_model_name(&DeepSeekModel::Flash),
        pro_context_window_tokens: model_context_window_tokens(kind, &DeepSeekModel::Pro),
        flash_context_window_tokens: model_context_window_tokens(kind, &DeepSeekModel::Flash),
        pro_max_output_tokens: model_max_output_tokens(kind, &DeepSeekModel::Pro),
        flash_max_output_tokens: model_max_output_tokens(kind, &DeepSeekModel::Flash),
        capabilities,
    }
}

fn print_models(payload: &ModelsPayload) {
    println!("Model providers");
    println!("Active: {}\n", payload.active_provider.as_str());
    println!(
        "{:<18} {:<7} {:<7} {:<11} {:<14} {:<24} Flash",
        "Provider", "Think", "Tools", "Reasoning", "Context", "Pro"
    );
    for item in &payload.providers {
        let marker = if item.active { "*" } else { " " };
        let thinking = if item.capabilities.thinking.supports_thinking {
            "yes"
        } else {
            "no"
        };
        let reasoning = if item.capabilities.thinking.supports_reasoning_content {
            "content"
        } else {
            "-"
        };
        let tools = if item.capabilities.supports_tool_calls {
            "yes"
        } else {
            "no"
        };
        let context = item
            .pro_context_window_tokens
            .map(format_tokens)
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{} {:<16} {:<7} {:<7} {:<11} {:<14} {:<24} {}",
            marker,
            item.provider.as_str(),
            thinking,
            tools,
            reasoning,
            context,
            item.pro_model,
            item.flash_model
        );
    }
    println!("\nBase URLs:");
    for item in &payload.providers {
        println!(
            "  {:<18} {}",
            item.provider.as_str(),
            item.base_url.as_deref().unwrap_or("(native default)")
        );
    }
    println!("\nThinking controls:");
    for item in &payload.providers {
        println!(
            "  {:<18} {}",
            item.provider.as_str(),
            item.capabilities.thinking.control_surface
        );
    }
    println!("\nRoutes:");
    for item in &payload.providers {
        println!(
            "  {:<18} {}",
            item.provider.as_str(),
            item.capabilities.routes.join(", ")
        );
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_payload_lists_all_provider_capabilities() {
        let payload = models_payload(&ProviderConfig::default());

        assert_eq!(payload.active_provider, ProviderKind::DeepSeek);
        assert_eq!(payload.providers.len(), ProviderKind::all().len());
        assert!(payload
            .providers
            .iter()
            .any(|provider| provider.provider == ProviderKind::Qwen
                && provider.capabilities.thinking.supports_thinking
                && provider.pro_model == "qwen3-coder-plus"));
        assert!(payload.providers.iter().any(|provider| provider.provider
            == ProviderKind::OpenAiCompatible
            && !provider.capabilities.thinking.supports_thinking));
    }
}

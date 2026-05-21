use std::path::PathBuf;

use serde::Serialize;

use crate::cli::resolve_project_root;
use crate::deepseek::DeepSeekModel;
use crate::provider::{
    build_provider, Provider, ProviderCapabilities, ProviderConfig, ProviderKind,
};
use crate::storage;

pub async fn models(json: bool, project_root: Option<PathBuf>) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "models")?;
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
        capabilities,
    }
}

fn print_models(payload: &ModelsPayload) {
    println!("Model providers");
    println!("Active: {}\n", payload.active_provider.as_str());
    println!(
        "{:<18} {:<7} {:<11} {:<10} {:<24} Flash",
        "Provider", "Think", "Reasoning", "Preserve", "Pro"
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
        let preserve = if item.capabilities.thinking.supports_preserved_reasoning {
            "yes"
        } else {
            "no"
        };
        println!(
            "{} {:<16} {:<7} {:<11} {:<10} {:<24} {}",
            marker,
            item.provider.as_str(),
            thinking,
            reasoning,
            preserve,
            item.pro_model,
            item.flash_model
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

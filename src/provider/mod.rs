//! Provider selection and model resolution.
//!
//! This first provider layer keeps DeepSeek as the only implemented backend,
//! while giving CLI, TUI, and task paths one shared place to resolve models and
//! construct clients.

use serde::{Deserialize, Serialize};

use crate::deepseek::{client::DeepSeekClient, DeepSeekModel};
use crate::storage::config::ModelConfig;

/// Supported provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[serde(rename = "deepseek")]
    #[default]
    DeepSeek,
    #[serde(rename = "openrouter")]
    OpenRouter,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl ProviderKind {
    /// Stable label for config and diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::OpenRouter => "openrouter",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}

/// Provider-specific endpoint and model-name overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderEndpointConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pro_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flash_model: Option<String>,
}

/// Provider-level configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    #[serde(default)]
    pub default: ProviderKind,
    #[serde(default)]
    pub deepseek: ProviderEndpointConfig,
    #[serde(default)]
    pub openrouter: ProviderEndpointConfig,
    #[serde(default, rename = "openai-compatible")]
    pub openai_compatible: ProviderEndpointConfig,
}

impl ProviderConfig {
    #[must_use]
    pub fn endpoint_for(&self, kind: ProviderKind) -> &ProviderEndpointConfig {
        match kind {
            ProviderKind::DeepSeek => &self.deepseek,
            ProviderKind::OpenRouter => &self.openrouter,
            ProviderKind::OpenAiCompatible => &self.openai_compatible,
        }
    }
}

/// Fully resolved model selection for one request/session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSelection {
    pub provider: ProviderKind,
    pub model: DeepSeekModel,
}

impl ModelSelection {
    /// Resolve a selection from config plus an optional CLI/TUI override.
    pub fn resolve(
        provider: &ProviderConfig,
        model_config: &ModelConfig,
        override_value: Option<&str>,
    ) -> Result<Self, ModelSelectionError> {
        let model = match override_value {
            Some(value) => parse_model(value)?,
            None => model_config.default.canonical(),
        };
        Ok(Self {
            provider: provider.default,
            model,
        })
    }
}

/// Provider abstraction used by the runtime.
pub trait Provider {
    fn kind(&self) -> ProviderKind;
    fn create_deepseek_client(&self) -> DeepSeekClient;
    fn request_model_name(&self, model: &DeepSeekModel) -> String;
}

/// DeepSeek-native provider adapter.
#[derive(Debug, Clone)]
pub struct DeepSeekProvider {
    kind: ProviderKind,
    api_key: String,
    base_url: Option<String>,
    pro_model: Option<String>,
    flash_model: Option<String>,
}

impl DeepSeekProvider {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            kind: ProviderKind::DeepSeek,
            api_key,
            base_url: None,
            pro_model: None,
            flash_model: None,
        }
    }

    #[must_use]
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }
}

impl Provider for DeepSeekProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn create_deepseek_client(&self) -> DeepSeekClient {
        let client = DeepSeekClient::new(self.api_key.clone());
        let client = match self.base_url.as_deref() {
            Some(base_url) => client.with_base_url(base_url),
            None => client,
        };
        client.with_model_names(
            Some(self.request_model_name(&DeepSeekModel::Pro)),
            Some(self.request_model_name(&DeepSeekModel::Flash)),
        )
    }

    fn request_model_name(&self, model: &DeepSeekModel) -> String {
        match model.canonical() {
            DeepSeekModel::Pro => self
                .pro_model
                .clone()
                .unwrap_or_else(|| default_request_model_name(self.kind, &DeepSeekModel::Pro)),
            DeepSeekModel::Flash => self
                .flash_model
                .clone()
                .unwrap_or_else(|| default_request_model_name(self.kind, &DeepSeekModel::Flash)),
            other => default_request_model_name(self.kind, &other),
        }
    }
}

/// Build the currently supported provider.
#[must_use]
pub fn build_provider(config: &ProviderConfig, api_key: String) -> DeepSeekProvider {
    let endpoint = config.endpoint_for(config.default);
    DeepSeekProvider {
        kind: config.default,
        api_key,
        base_url: endpoint
            .base_url
            .as_deref()
            .map(normalize_base_url)
            .or_else(|| default_base_url(config.default).map(str::to_string)),
        pro_model: endpoint.pro_model.clone(),
        flash_model: endpoint.flash_model.clone(),
    }
}

fn normalize_base_url(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}

fn default_base_url(kind: ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::DeepSeek => None,
        ProviderKind::OpenRouter => Some("https://openrouter.ai/api/v1"),
        ProviderKind::OpenAiCompatible => Some("https://api.openai.com/v1"),
    }
}

fn default_request_model_name(kind: ProviderKind, model: &DeepSeekModel) -> String {
    let canonical = model.canonical();
    match kind {
        ProviderKind::DeepSeek => canonical.to_string(),
        ProviderKind::OpenRouter => match canonical {
            DeepSeekModel::Pro => "deepseek/deepseek-v4-pro".to_string(),
            DeepSeekModel::Flash => "deepseek/deepseek-v4-flash".to_string(),
            other => other.to_string(),
        },
        ProviderKind::OpenAiCompatible => match canonical {
            DeepSeekModel::Pro => "deepseek-reasoner".to_string(),
            DeepSeekModel::Flash => "deepseek-chat".to_string(),
            other => other.to_string(),
        },
    }
}

/// Parse CLI/TUI model names and legacy aliases.
pub fn parse_model(value: &str) -> Result<DeepSeekModel, ModelSelectionError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pro" | "v4-pro" | "deepseek-v4-pro" => Ok(DeepSeekModel::Pro),
        "flash" | "v4-flash" | "deepseek-v4-flash" => Ok(DeepSeekModel::Flash),
        other => crate::deepseek::migration::migrate_model_name(other)
            .map(|model| model.canonical())
            .ok_or_else(|| ModelSelectionError::UnknownModel(value.to_string())),
    }
}

/// Model/provider selection errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelSelectionError {
    #[error("unknown model '{0}'")]
    UnknownModel(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::config::ModelConfig;

    #[test]
    fn provider_config_parses_deepseek() {
        let config: ProviderConfig =
            toml::from_str(r#"default = "deepseek""#).expect("provider parses");

        assert_eq!(config.default, ProviderKind::DeepSeek);
    }

    #[test]
    fn provider_config_parses_openrouter_and_openai_compatible() {
        let openrouter: ProviderConfig =
            toml::from_str(r#"default = "openrouter""#).expect("openrouter parses");
        let compatible: ProviderConfig =
            toml::from_str(r#"default = "openai-compatible""#).expect("compatible parses");

        assert_eq!(openrouter.default, ProviderKind::OpenRouter);
        assert_eq!(compatible.default, ProviderKind::OpenAiCompatible);
        assert_eq!(ProviderKind::OpenAiCompatible.as_str(), "openai-compatible");
    }

    #[test]
    fn provider_config_parses_endpoint_overrides() {
        let config: ProviderConfig = toml::from_str(
            r#"
default = "openai-compatible"

[openai-compatible]
base_url = "https://llm.example.com/v1/"
pro_model = "reasoning-pro"
flash_model = "chat-fast"
"#,
        )
        .expect("provider endpoint config parses");

        assert_eq!(config.default, ProviderKind::OpenAiCompatible);
        assert_eq!(
            config.openai_compatible.base_url.as_deref(),
            Some("https://llm.example.com/v1/")
        );
        assert_eq!(
            config.openai_compatible.pro_model.as_deref(),
            Some("reasoning-pro")
        );
        assert_eq!(
            config.openai_compatible.flash_model.as_deref(),
            Some("chat-fast")
        );
    }

    #[test]
    fn model_selection_uses_config_default_without_override() {
        let provider = ProviderConfig::default();
        let model_config = ModelConfig {
            default: DeepSeekModel::Pro,
            ..ModelConfig::default()
        };

        let selection =
            ModelSelection::resolve(&provider, &model_config, None).expect("selection resolves");

        assert_eq!(selection.model, DeepSeekModel::Pro);
    }

    #[test]
    fn model_selection_override_wins_over_config() {
        let provider = ProviderConfig::default();
        let model_config = ModelConfig {
            default: DeepSeekModel::Pro,
            ..ModelConfig::default()
        };

        let selection =
            ModelSelection::resolve(&provider, &model_config, Some("flash")).expect("override");

        assert_eq!(selection.model, DeepSeekModel::Flash);
    }

    #[test]
    fn parse_model_accepts_shared_aliases() {
        for alias in ["pro", "v4-pro", "deepseek-v4-pro"] {
            assert_eq!(parse_model(alias).expect(alias), DeepSeekModel::Pro);
        }
        for alias in ["flash", "v4-flash", "deepseek-v4-flash"] {
            assert_eq!(parse_model(alias).expect(alias), DeepSeekModel::Flash);
        }
    }

    #[test]
    fn provider_config_rejects_unknown_provider() {
        let err = toml::from_str::<ProviderConfig>(r#"default = "bad""#)
            .expect_err("unknown provider must fail");

        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn provider_factory_builds_deepseek_client() {
        let provider = build_provider(&ProviderConfig::default(), "sk-test".to_string());

        assert_eq!(provider.kind(), ProviderKind::DeepSeek);
        assert_eq!(provider.base_url(), None);
        assert_eq!(
            provider.request_model_name(&DeepSeekModel::LegacyReasoner),
            "deepseek-v4-pro"
        );
    }

    #[test]
    fn provider_factory_builds_openrouter_defaults() {
        let config = ProviderConfig {
            default: ProviderKind::OpenRouter,
            ..ProviderConfig::default()
        };
        let provider = build_provider(&config, "sk-test".to_string());

        assert_eq!(provider.kind(), ProviderKind::OpenRouter);
        assert_eq!(provider.base_url(), Some("https://openrouter.ai/api/v1"));
        assert_eq!(
            provider.request_model_name(&DeepSeekModel::Pro),
            "deepseek/deepseek-v4-pro"
        );
        assert_eq!(
            provider.request_model_name(&DeepSeekModel::Flash),
            "deepseek/deepseek-v4-flash"
        );
    }

    #[test]
    fn provider_factory_applies_openai_compatible_overrides() {
        let config = ProviderConfig {
            default: ProviderKind::OpenAiCompatible,
            openai_compatible: ProviderEndpointConfig {
                base_url: Some("https://llm.example.com/v1/".into()),
                pro_model: Some("reasoning-pro".into()),
                flash_model: Some("chat-fast".into()),
            },
            ..ProviderConfig::default()
        };
        let provider = build_provider(&config, "sk-test".to_string());

        assert_eq!(provider.kind(), ProviderKind::OpenAiCompatible);
        assert_eq!(provider.base_url(), Some("https://llm.example.com/v1"));
        assert_eq!(
            provider.request_model_name(&DeepSeekModel::Pro),
            "reasoning-pro"
        );
        assert_eq!(
            provider.request_model_name(&DeepSeekModel::Flash),
            "chat-fast"
        );
    }
}

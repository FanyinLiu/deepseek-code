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
}

impl ProviderKind {
    /// Stable label for config and diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
        }
    }
}

/// Provider-level configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    #[serde(default)]
    pub default: ProviderKind,
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
    api_key: String,
}

impl DeepSeekProvider {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl Provider for DeepSeekProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::DeepSeek
    }

    fn create_deepseek_client(&self) -> DeepSeekClient {
        DeepSeekClient::new(self.api_key.clone())
    }

    fn request_model_name(&self, model: &DeepSeekModel) -> String {
        model.canonical().to_string()
    }
}

/// Build the currently supported provider.
#[must_use]
pub fn build_provider(config: &ProviderConfig, api_key: String) -> DeepSeekProvider {
    match config.default {
        ProviderKind::DeepSeek => DeepSeekProvider::new(api_key),
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
        assert_eq!(
            provider.request_model_name(&DeepSeekModel::LegacyReasoner),
            "deepseek-v4-pro"
        );
    }
}

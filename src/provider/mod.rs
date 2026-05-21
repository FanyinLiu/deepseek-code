//! Provider selection and model resolution.
//!
//! The runtime still uses the DeepSeek-compatible chat client internally, but
//! provider presets make OpenAI-compatible Chinese model services first-class.

use std::{future::Future, pin::Pin};

use serde::{Deserialize, Serialize};

use crate::deepseek::{
    client::DeepSeekClient,
    errors::DeepSeekError,
    models::{ChatRequest, ChatResponse, StreamResult},
    DeepSeekModel, ThinkingWireFormat,
};
use crate::storage::config::ModelConfig;

const DEEPSEEK_V4_CONTEXT_WINDOW_TOKENS: u64 = 1_000_000;
const DEEPSEEK_V4_MAX_OUTPUT_TOKENS: u64 = 384_000;
const KIMI_K2_CONTEXT_WINDOW_TOKENS: u64 = 256_000;
const GLM_CONTEXT_WINDOW_TOKENS: u64 = 200_000;
const GLM_MAX_OUTPUT_TOKENS: u64 = 128_000;
const QIANFAN_ERNIE_CONTEXT_WINDOW_TOKENS: u64 = 128_000;
const STEPFUN_CONTEXT_WINDOW_TOKENS: u64 = 16_000;
const DOUBAO_SEED_CODE_CONTEXT_WINDOW_TOKENS: u64 = 256_000;
const DOUBAO_SEED_CODE_MAX_OUTPUT_TOKENS: u64 = 32_000;
const DEFAULT_AUTO_COMPACT_RATIO: f64 = 0.80;
const PROVIDER_PROFILE_LAST_VERIFIED: &str = "2026-05-21";

/// Supported provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[serde(rename = "deepseek")]
    #[default]
    DeepSeek,
    #[serde(rename = "qwen")]
    Qwen,
    #[serde(rename = "kimi")]
    Kimi,
    #[serde(rename = "zhipu")]
    Zhipu,
    #[serde(rename = "minimax")]
    Minimax,
    #[serde(rename = "tencent")]
    Tencent,
    #[serde(rename = "qianfan")]
    Qianfan,
    #[serde(rename = "stepfun")]
    Stepfun,
    #[serde(rename = "doubao")]
    Doubao,
    #[serde(rename = "openrouter")]
    OpenRouter,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl ProviderKind {
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::DeepSeek,
            Self::Qwen,
            Self::Kimi,
            Self::Zhipu,
            Self::Minimax,
            Self::Tencent,
            Self::Qianfan,
            Self::Stepfun,
            Self::Doubao,
            Self::OpenRouter,
            Self::OpenAiCompatible,
        ]
    }

    /// Stable label for config and diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::Qwen => "qwen",
            Self::Kimi => "kimi",
            Self::Zhipu => "zhipu",
            Self::Minimax => "minimax",
            Self::Tencent => "tencent",
            Self::Qianfan => "qianfan",
            Self::Stepfun => "stepfun",
            Self::Doubao => "doubao",
            Self::OpenRouter => "openrouter",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }

    #[must_use]
    pub fn from_config_value(value: &str) -> Option<Self> {
        Self::all()
            .iter()
            .copied()
            .find(|kind| kind.as_str() == value.trim())
    }

    #[must_use]
    pub fn allowed_values() -> String {
        Self::all()
            .iter()
            .map(|kind| kind.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::Qwen => "Qwen / DashScope",
            Self::Kimi => "Kimi / Moonshot",
            Self::Zhipu => "GLM / Zhipu",
            Self::Minimax => "MiniMax",
            Self::Tencent => "Tencent TokenHub",
            Self::Qianfan => "Baidu Qianfan",
            Self::Stepfun => "StepFun",
            Self::Doubao => "Doubao / Volcano Ark",
            Self::OpenRouter => "OpenRouter",
            Self::OpenAiCompatible => "OpenAI-compatible",
        }
    }

    #[must_use]
    pub fn capabilities(self) -> ProviderCapabilities {
        match self {
            Self::DeepSeek => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: true,
                    supports_preserved_reasoning: true,
                    wire_format: ThinkingWireFormat::DeepSeekNative,
                    control_surface: "thinking.type + thinking.effort",
                },
                Some(DEEPSEEK_V4_CONTEXT_WINDOW_TOKENS),
                Some(DEEPSEEK_V4_MAX_OUTPUT_TOKENS),
                CapabilityFlags {
                    supports_fim: true,
                    supports_prompt_cache: true,
                    cache_behavior: "automatic prefix cache; usage exposes hit/miss tokens",
                    routes: &["fast", "strong", "coding"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Qwen => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: false,
                    supports_preserved_reasoning: false,
                    wire_format: ThinkingWireFormat::DashScopeEnableThinking,
                    control_surface: "enable_thinking + thinking_budget",
                },
                None,
                None,
                CapabilityFlags {
                    routes: &["fast", "strong", "coding"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Kimi => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: true,
                    supports_preserved_reasoning: true,
                    wire_format: ThinkingWireFormat::NativeTypeOnly,
                    control_surface: "thinking.type",
                },
                Some(KIMI_K2_CONTEXT_WINDOW_TOKENS),
                None,
                CapabilityFlags {
                    supports_vision: true,
                    supports_prompt_cache: true,
                    cache_behavior: "automatic context cache; usage may expose cached_tokens",
                    routes: &["strong", "coding", "research_vision"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Zhipu => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: true,
                    supports_preserved_reasoning: true,
                    wire_format: ThinkingWireFormat::NativeTypeOnly,
                    control_surface: "thinking.type",
                },
                Some(GLM_CONTEXT_WINDOW_TOKENS),
                Some(GLM_MAX_OUTPUT_TOKENS),
                CapabilityFlags {
                    routes: &["strong", "coding"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Minimax => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: true,
                    supports_preserved_reasoning: true,
                    wire_format: ThinkingWireFormat::MiniMaxReasoningSplit,
                    control_surface: "reasoning_split + reasoning_details",
                },
                None,
                None,
                CapabilityFlags {
                    supports_prompt_cache: true,
                    cache_behavior: "automatic cache; no OpenAI prompt_cache_key contract",
                    routes: &["strong", "coding"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Tencent => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: false,
                    supports_preserved_reasoning: false,
                    wire_format: ThinkingWireFormat::Unsupported,
                    control_surface: "model-specific TokenHub options",
                },
                None,
                None,
                CapabilityFlags {
                    routes: &["fast", "strong"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Qianfan => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: true,
                    supports_preserved_reasoning: true,
                    wire_format: ThinkingWireFormat::QianfanThinking,
                    control_surface: "thinking.type + enable_thinking + thinking_budget",
                },
                Some(QIANFAN_ERNIE_CONTEXT_WINDOW_TOKENS),
                None,
                CapabilityFlags {
                    supports_responses_api: true,
                    requires_dedicated_adapter: true,
                    cache_behavior:
                        "provider-managed context; Responses API supports previous_response_id",
                    routes: &["strong", "enterprise"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Stepfun => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: false,
                    supports_reasoning_content: false,
                    supports_preserved_reasoning: false,
                    wire_format: ThinkingWireFormat::Unsupported,
                    control_surface: "standard v1 chat; Step-Plan uses separate endpoint",
                },
                Some(STEPFUN_CONTEXT_WINDOW_TOKENS),
                None,
                CapabilityFlags {
                    routes: &["fast"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::Doubao => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: true,
                    supports_reasoning_content: false,
                    supports_preserved_reasoning: false,
                    wire_format: ThinkingWireFormat::Unsupported,
                    control_surface: "endpoint/model override required",
                },
                Some(DOUBAO_SEED_CODE_CONTEXT_WINDOW_TOKENS),
                Some(DOUBAO_SEED_CODE_MAX_OUTPUT_TOKENS),
                CapabilityFlags {
                    supports_vision: true,
                    supports_prompt_cache: true,
                    requires_endpoint_override: true,
                    requires_model_override: true,
                    cache_behavior: "Volcano Ark endpoint-specific cache behavior",
                    routes: &["coding", "research_vision"],
                    ..CapabilityFlags::default()
                },
            ),
            Self::OpenRouter | Self::OpenAiCompatible => provider_capabilities(
                self,
                ProviderThinkingCapabilities {
                    supports_thinking: false,
                    supports_reasoning_content: false,
                    supports_preserved_reasoning: false,
                    wire_format: ThinkingWireFormat::Unsupported,
                    control_surface: "custom provider config required",
                },
                None,
                None,
                CapabilityFlags {
                    supports_responses_api: matches!(self, Self::OpenAiCompatible),
                    supports_structured_outputs: matches!(self, Self::OpenAiCompatible),
                    supports_tracing: matches!(self, Self::OpenAiCompatible),
                    supports_evals: matches!(self, Self::OpenAiCompatible),
                    cache_behavior: "depends on configured upstream provider",
                    routes: &["custom"],
                    ..CapabilityFlags::default()
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CapabilityFlags {
    supports_fim: bool,
    supports_responses_api: bool,
    supports_structured_outputs: bool,
    supports_tracing: bool,
    supports_evals: bool,
    supports_vision: bool,
    supports_prompt_cache: bool,
    supports_cache_control: bool,
    requires_dedicated_adapter: bool,
    requires_endpoint_override: bool,
    requires_model_override: bool,
    cache_behavior: &'static str,
    routes: &'static [&'static str],
}

impl Default for CapabilityFlags {
    fn default() -> Self {
        Self {
            supports_fim: false,
            supports_responses_api: false,
            supports_structured_outputs: false,
            supports_tracing: false,
            supports_evals: false,
            supports_vision: false,
            supports_prompt_cache: false,
            supports_cache_control: false,
            requires_dedicated_adapter: false,
            requires_endpoint_override: false,
            requires_model_override: false,
            cache_behavior: "no provider-specific cache contract declared",
            routes: &["fast", "strong"],
        }
    }
}

fn provider_capabilities(
    kind: ProviderKind,
    thinking: ProviderThinkingCapabilities,
    context_window_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    flags: CapabilityFlags,
) -> ProviderCapabilities {
    ProviderCapabilities {
        kind,
        display_name: kind.display_name(),
        thinking,
        supports_tool_calls: true,
        supports_json_output: true,
        supports_fim: flags.supports_fim,
        supports_responses_api: flags.supports_responses_api,
        supports_structured_outputs: flags.supports_structured_outputs,
        supports_tracing: flags.supports_tracing,
        supports_evals: flags.supports_evals,
        supports_vision: flags.supports_vision,
        supports_prompt_cache: flags.supports_prompt_cache,
        supports_cache_control: flags.supports_cache_control,
        requires_dedicated_adapter: flags.requires_dedicated_adapter,
        requires_endpoint_override: flags.requires_endpoint_override,
        requires_model_override: flags.requires_model_override,
        context_window_tokens,
        max_output_tokens,
        cache_behavior: flags.cache_behavior,
        routes: flags.routes,
        last_verified: PROVIDER_PROFILE_LAST_VERIFIED,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCapabilities {
    pub kind: ProviderKind,
    pub display_name: &'static str,
    pub thinking: ProviderThinkingCapabilities,
    pub supports_tool_calls: bool,
    pub supports_json_output: bool,
    pub supports_fim: bool,
    pub supports_responses_api: bool,
    pub supports_structured_outputs: bool,
    pub supports_tracing: bool,
    pub supports_evals: bool,
    pub supports_vision: bool,
    pub supports_prompt_cache: bool,
    /// Whether this provider needs explicit `cache_control` markers on the
    /// request payload to opt in to prompt caching with explicit breakpoints.
    /// DeepSeek and most OpenAI-compatible providers auto-cache by prefix,
    /// so this stays `false` for them.
    pub supports_cache_control: bool,
    pub requires_dedicated_adapter: bool,
    pub requires_endpoint_override: bool,
    pub requires_model_override: bool,
    pub context_window_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub cache_behavior: &'static str,
    pub routes: &'static [&'static str],
    pub last_verified: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextBudget {
    pub model_window_tokens: Option<u64>,
    pub model_window_source: &'static str,
    pub local_budget_tokens: u64,
    pub effective_budget_tokens: u64,
    pub auto_compact_threshold_tokens: u64,
}

impl ContextBudget {
    #[must_use]
    pub fn usage_percent(&self, used_tokens: u64) -> f64 {
        (used_tokens as f64 / self.effective_budget_tokens.max(1) as f64) * 100.0
    }

    #[must_use]
    pub fn next_action(&self, used_tokens: u64) -> &'static str {
        if used_tokens >= self.auto_compact_threshold_tokens {
            "auto compact should run before the next large turn"
        } else {
            "keep collecting context"
        }
    }
}

#[must_use]
pub fn context_budget_for(
    provider: ProviderKind,
    model: &DeepSeekModel,
    configured_local_budget: usize,
) -> ContextBudget {
    let local_budget_tokens = (configured_local_budget as u64).max(1);
    let model_window_tokens = model_context_window_tokens(provider, model);
    let effective_budget_tokens = model_window_tokens
        .map_or(local_budget_tokens, |window| {
            window.min(local_budget_tokens)
        })
        .max(1);
    let auto_compact_threshold_tokens =
        ((effective_budget_tokens as f64) * DEFAULT_AUTO_COMPACT_RATIO).round() as u64;
    ContextBudget {
        model_window_tokens,
        model_window_source: model_context_window_source(provider, model),
        local_budget_tokens,
        effective_budget_tokens,
        auto_compact_threshold_tokens: auto_compact_threshold_tokens.max(1),
    }
}

#[must_use]
pub fn model_context_window_tokens(provider: ProviderKind, model: &DeepSeekModel) -> Option<u64> {
    match (provider, model.canonical()) {
        (ProviderKind::DeepSeek, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(DEEPSEEK_V4_CONTEXT_WINDOW_TOKENS)
        }
        (ProviderKind::Kimi, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(KIMI_K2_CONTEXT_WINDOW_TOKENS)
        }
        (ProviderKind::Zhipu, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(GLM_CONTEXT_WINDOW_TOKENS)
        }
        (ProviderKind::Qianfan, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(QIANFAN_ERNIE_CONTEXT_WINDOW_TOKENS)
        }
        (ProviderKind::Stepfun, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(STEPFUN_CONTEXT_WINDOW_TOKENS)
        }
        (ProviderKind::Doubao, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(DOUBAO_SEED_CODE_CONTEXT_WINDOW_TOKENS)
        }
        _ => None,
    }
}

#[must_use]
pub fn model_context_window_source(provider: ProviderKind, model: &DeepSeekModel) -> &'static str {
    match (provider, model.canonical()) {
        (ProviderKind::DeepSeek, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            "DeepSeek API model details"
        }
        (ProviderKind::Kimi, DeepSeekModel::Pro | DeepSeekModel::Flash) => "Kimi API model details",
        (ProviderKind::Zhipu, DeepSeekModel::Pro | DeepSeekModel::Flash) => "GLM API model details",
        (ProviderKind::Qianfan, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            "Qianfan model details"
        }
        (ProviderKind::Stepfun, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            "StepFun text model details"
        }
        (ProviderKind::Doubao, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            "Volcano Ark Doubao Seed Code details"
        }
        _ => "not declared by provider preset; using local assembly budget",
    }
}

#[must_use]
pub fn model_max_output_tokens(provider: ProviderKind, model: &DeepSeekModel) -> Option<u64> {
    match (provider, model.canonical()) {
        (ProviderKind::DeepSeek, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(DEEPSEEK_V4_MAX_OUTPUT_TOKENS)
        }
        (ProviderKind::Zhipu, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(GLM_MAX_OUTPUT_TOKENS)
        }
        (ProviderKind::Doubao, DeepSeekModel::Pro | DeepSeekModel::Flash) => {
            Some(DOUBAO_SEED_CODE_MAX_OUTPUT_TOKENS)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderThinkingCapabilities {
    pub supports_thinking: bool,
    pub supports_reasoning_content: bool,
    pub supports_preserved_reasoning: bool,
    pub wire_format: ThinkingWireFormat,
    pub control_surface: &'static str,
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
    pub qwen: ProviderEndpointConfig,
    #[serde(default)]
    pub kimi: ProviderEndpointConfig,
    #[serde(default)]
    pub zhipu: ProviderEndpointConfig,
    #[serde(default)]
    pub minimax: ProviderEndpointConfig,
    #[serde(default)]
    pub tencent: ProviderEndpointConfig,
    #[serde(default)]
    pub qianfan: ProviderEndpointConfig,
    #[serde(default)]
    pub stepfun: ProviderEndpointConfig,
    #[serde(default)]
    pub doubao: ProviderEndpointConfig,
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
            ProviderKind::Qwen => &self.qwen,
            ProviderKind::Kimi => &self.kimi,
            ProviderKind::Zhipu => &self.zhipu,
            ProviderKind::Minimax => &self.minimax,
            ProviderKind::Tencent => &self.tencent,
            ProviderKind::Qianfan => &self.qianfan,
            ProviderKind::Stepfun => &self.stepfun,
            ProviderKind::Doubao => &self.doubao,
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
        client
            .with_model_names(
                Some(self.request_model_name(&DeepSeekModel::Pro)),
                Some(self.request_model_name(&DeepSeekModel::Flash)),
            )
            .with_thinking_wire_format(self.kind.capabilities().thinking.wire_format)
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
        ProviderKind::Qwen => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        ProviderKind::Kimi => Some("https://api.moonshot.ai/v1"),
        ProviderKind::Zhipu => Some("https://open.bigmodel.cn/api/paas/v4"),
        ProviderKind::Minimax => Some("https://api.minimaxi.com/v1"),
        ProviderKind::Tencent => Some("https://tokenhub.tencentmaas.com/v1"),
        ProviderKind::Qianfan => Some("https://qianfan.baidubce.com/v2"),
        ProviderKind::Stepfun => Some("https://api.stepfun.ai/v1"),
        ProviderKind::Doubao => Some("https://ark.cn-beijing.volces.com/api/v3"),
        ProviderKind::OpenRouter => Some("https://openrouter.ai/api/v1"),
        ProviderKind::OpenAiCompatible => Some("https://api.openai.com/v1"),
    }
}

fn default_request_model_name(kind: ProviderKind, model: &DeepSeekModel) -> String {
    let canonical = model.canonical();
    match kind {
        ProviderKind::DeepSeek => canonical.to_string(),
        ProviderKind::Qwen => match canonical {
            DeepSeekModel::Pro => "qwen3-coder-plus".to_string(),
            DeepSeekModel::Flash => "qwen3-coder-flash".to_string(),
            other => other.to_string(),
        },
        ProviderKind::Kimi => match canonical {
            DeepSeekModel::Pro => "kimi-k2.6".to_string(),
            DeepSeekModel::Flash => "kimi-k2.5".to_string(),
            other => other.to_string(),
        },
        ProviderKind::Zhipu => match canonical {
            DeepSeekModel::Pro => "glm-5.1".to_string(),
            DeepSeekModel::Flash => "glm-4.7-flashx".to_string(),
            other => other.to_string(),
        },
        ProviderKind::Minimax => match canonical {
            DeepSeekModel::Pro => "MiniMax-M2.7".to_string(),
            DeepSeekModel::Flash => "MiniMax-M2.7-highspeed".to_string(),
            other => other.to_string(),
        },
        ProviderKind::Tencent => match canonical {
            DeepSeekModel::Pro => "hunyuan-2.0-thinking".to_string(),
            DeepSeekModel::Flash => "hunyuan-2.0-instruct".to_string(),
            other => other.to_string(),
        },
        ProviderKind::Qianfan => match canonical {
            DeepSeekModel::Pro => "ernie-5.0-thinking-preview".to_string(),
            DeepSeekModel::Flash => "ernie-4.5-turbo-128k".to_string(),
            other => other.to_string(),
        },
        ProviderKind::Stepfun => match canonical {
            DeepSeekModel::Pro => "step-2-16k".to_string(),
            DeepSeekModel::Flash => "step-2-mini".to_string(),
            other => other.to_string(),
        },
        ProviderKind::Doubao => match canonical {
            DeepSeekModel::Pro => "configure-doubao-pro-model".to_string(),
            DeepSeekModel::Flash => "configure-doubao-flash-model".to_string(),
            other => other.to_string(),
        },
        ProviderKind::OpenRouter => match canonical {
            DeepSeekModel::Pro => "deepseek/deepseek-v4-pro".to_string(),
            DeepSeekModel::Flash => "deepseek/deepseek-v4-flash".to_string(),
            other => other.to_string(),
        },
        ProviderKind::OpenAiCompatible => match canonical {
            DeepSeekModel::Pro => "deepseek-v4-pro".to_string(),
            DeepSeekModel::Flash => "deepseek-v4-flash".to_string(),
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

/// Provider-neutral request wrapper used by new runtime code.
#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub chat: ChatRequest,
}

/// Provider-neutral non-streaming response wrapper.
#[derive(Debug, Clone)]
pub enum ModelResponse {
    Chat(ChatResponse),
}

/// Provider-neutral streaming result wrapper.
#[derive(Debug, Clone)]
pub enum ModelStream {
    Accumulated(StreamResult),
}

/// Runtime-facing model abstraction. Existing code can keep using
/// `DeepSeekClient` while new providers converge on this interface.
pub trait ModelClient {
    fn provider_kind(&self) -> ProviderKind;

    fn send<'a>(
        &'a self,
        request: &'a ModelRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ModelResponse, DeepSeekError>> + Send + 'a>>;

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ModelStream, DeepSeekError>> + Send + 'a>>;
}

impl ModelClient for DeepSeekClient {
    fn provider_kind(&self) -> ProviderKind {
        ProviderKind::DeepSeek
    }

    fn send<'a>(
        &'a self,
        request: &'a ModelRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ModelResponse, DeepSeekError>> + Send + 'a>> {
        Box::pin(async move { self.chat(&request.chat).await.map(ModelResponse::Chat) })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ModelStream, DeepSeekError>> + Send + 'a>> {
        Box::pin(async move {
            self.chat_stream_accumulated(&request.chat)
                .await
                .map(ModelStream::Accumulated)
        })
    }
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
    fn provider_config_parses_chinese_provider_presets() {
        let qwen: ProviderConfig = toml::from_str(r#"default = "qwen""#).expect("qwen parses");
        let kimi: ProviderConfig = toml::from_str(r#"default = "kimi""#).expect("kimi parses");
        let zhipu: ProviderConfig = toml::from_str(r#"default = "zhipu""#).expect("zhipu parses");
        let minimax: ProviderConfig =
            toml::from_str(r#"default = "minimax""#).expect("minimax parses");
        let tencent: ProviderConfig =
            toml::from_str(r#"default = "tencent""#).expect("tencent parses");
        let qianfan: ProviderConfig =
            toml::from_str(r#"default = "qianfan""#).expect("qianfan parses");
        let stepfun: ProviderConfig =
            toml::from_str(r#"default = "stepfun""#).expect("stepfun parses");
        let doubao: ProviderConfig =
            toml::from_str(r#"default = "doubao""#).expect("doubao parses");

        assert_eq!(qwen.default, ProviderKind::Qwen);
        assert_eq!(kimi.default, ProviderKind::Kimi);
        assert_eq!(zhipu.default, ProviderKind::Zhipu);
        assert_eq!(minimax.default, ProviderKind::Minimax);
        assert_eq!(tencent.default, ProviderKind::Tencent);
        assert_eq!(qianfan.default, ProviderKind::Qianfan);
        assert_eq!(stepfun.default, ProviderKind::Stepfun);
        assert_eq!(doubao.default, ProviderKind::Doubao);
        assert_eq!(ProviderKind::Qwen.as_str(), "qwen");
        assert_eq!(ProviderKind::Kimi.as_str(), "kimi");
        assert_eq!(ProviderKind::Zhipu.as_str(), "zhipu");
        assert_eq!(ProviderKind::Minimax.as_str(), "minimax");
        assert_eq!(ProviderKind::Tencent.as_str(), "tencent");
        assert_eq!(ProviderKind::Qianfan.as_str(), "qianfan");
        assert_eq!(ProviderKind::Stepfun.as_str(), "stepfun");
        assert_eq!(ProviderKind::Doubao.as_str(), "doubao");
    }

    #[test]
    fn provider_capabilities_describe_thinking_adapters() {
        assert_eq!(
            ProviderKind::Qwen.capabilities().thinking.wire_format,
            ThinkingWireFormat::DashScopeEnableThinking
        );
        assert_eq!(
            ProviderKind::Kimi.capabilities().thinking.wire_format,
            ThinkingWireFormat::NativeTypeOnly
        );
        assert!(
            ProviderKind::Zhipu
                .capabilities()
                .thinking
                .supports_preserved_reasoning
        );
        assert!(
            !ProviderKind::OpenAiCompatible
                .capabilities()
                .thinking
                .supports_thinking
        );
        assert_eq!(
            ProviderKind::Qianfan.capabilities().thinking.wire_format,
            ThinkingWireFormat::QianfanThinking
        );
        assert_eq!(
            ProviderKind::Minimax.capabilities().thinking.wire_format,
            ThinkingWireFormat::MiniMaxReasoningSplit
        );
        assert!(ProviderKind::Doubao.capabilities().requires_model_override);
        assert!(ProviderKind::Kimi
            .capabilities()
            .routes
            .contains(&"research_vision"));
        assert!(ProviderKind::Qianfan
            .capabilities()
            .routes
            .contains(&"enterprise"));
    }

    #[test]
    fn context_budget_caps_configured_budget_to_provider_window() {
        let budget = context_budget_for(ProviderKind::DeepSeek, &DeepSeekModel::Flash, 2_000_000);

        assert_eq!(budget.model_window_tokens, Some(1_000_000));
        assert_eq!(budget.effective_budget_tokens, 1_000_000);
        assert_eq!(budget.auto_compact_threshold_tokens, 800_000);
        assert_eq!(budget.usage_percent(100_000).round() as u64, 10);
    }

    #[test]
    fn context_budget_uses_local_budget_for_unknown_provider_window() {
        let budget = context_budget_for(
            ProviderKind::OpenAiCompatible,
            &DeepSeekModel::Flash,
            32_000,
        );

        assert_eq!(budget.model_window_tokens, None);
        assert_eq!(budget.effective_budget_tokens, 32_000);
        assert_eq!(budget.auto_compact_threshold_tokens, 25_600);
    }

    #[test]
    fn provider_profiles_declare_context_and_output_limits() {
        assert_eq!(
            model_context_window_tokens(ProviderKind::DeepSeek, &DeepSeekModel::Pro),
            Some(1_000_000)
        );
        assert_eq!(
            model_max_output_tokens(ProviderKind::DeepSeek, &DeepSeekModel::Flash),
            Some(384_000)
        );
        assert_eq!(
            model_context_window_tokens(ProviderKind::Zhipu, &DeepSeekModel::Pro),
            Some(200_000)
        );
        assert_eq!(
            model_max_output_tokens(ProviderKind::Doubao, &DeepSeekModel::Pro),
            Some(32_000)
        );
        assert_eq!(
            ProviderKind::Qwen.capabilities().last_verified,
            "2026-05-21"
        );
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
    fn provider_factory_builds_chinese_provider_defaults() {
        let qwen = build_provider(
            &ProviderConfig {
                default: ProviderKind::Qwen,
                ..ProviderConfig::default()
            },
            "sk-test".to_string(),
        );
        assert_eq!(qwen.kind(), ProviderKind::Qwen);
        assert_eq!(
            qwen.base_url(),
            Some("https://dashscope.aliyuncs.com/compatible-mode/v1")
        );
        assert_eq!(
            qwen.request_model_name(&DeepSeekModel::Pro),
            "qwen3-coder-plus"
        );
        assert_eq!(
            qwen.request_model_name(&DeepSeekModel::Flash),
            "qwen3-coder-flash"
        );

        let kimi = build_provider(
            &ProviderConfig {
                default: ProviderKind::Kimi,
                ..ProviderConfig::default()
            },
            "sk-test".to_string(),
        );
        assert_eq!(kimi.kind(), ProviderKind::Kimi);
        assert_eq!(kimi.base_url(), Some("https://api.moonshot.ai/v1"));
        assert_eq!(kimi.request_model_name(&DeepSeekModel::Pro), "kimi-k2.6");
        assert_eq!(kimi.request_model_name(&DeepSeekModel::Flash), "kimi-k2.5");

        let zhipu = build_provider(
            &ProviderConfig {
                default: ProviderKind::Zhipu,
                ..ProviderConfig::default()
            },
            "sk-test".to_string(),
        );
        assert_eq!(zhipu.kind(), ProviderKind::Zhipu);
        assert_eq!(
            zhipu.base_url(),
            Some("https://open.bigmodel.cn/api/paas/v4")
        );
        assert_eq!(zhipu.request_model_name(&DeepSeekModel::Pro), "glm-5.1");
        assert_eq!(
            zhipu.request_model_name(&DeepSeekModel::Flash),
            "glm-4.7-flashx"
        );

        let minimax = build_provider(
            &ProviderConfig {
                default: ProviderKind::Minimax,
                ..ProviderConfig::default()
            },
            "sk-test".to_string(),
        );
        assert_eq!(minimax.base_url(), Some("https://api.minimaxi.com/v1"));
        assert_eq!(
            minimax.request_model_name(&DeepSeekModel::Pro),
            "MiniMax-M2.7"
        );

        let qianfan = build_provider(
            &ProviderConfig {
                default: ProviderKind::Qianfan,
                ..ProviderConfig::default()
            },
            "sk-test".to_string(),
        );
        assert_eq!(qianfan.base_url(), Some("https://qianfan.baidubce.com/v2"));
        assert_eq!(
            qianfan.request_model_name(&DeepSeekModel::Pro),
            "ernie-5.0-thinking-preview"
        );

        let stepfun = build_provider(
            &ProviderConfig {
                default: ProviderKind::Stepfun,
                ..ProviderConfig::default()
            },
            "sk-test".to_string(),
        );
        assert_eq!(stepfun.base_url(), Some("https://api.stepfun.ai/v1"));
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

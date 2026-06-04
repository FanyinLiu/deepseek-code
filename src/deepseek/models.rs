use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Model identifiers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum DeepSeekModel {
    #[serde(rename = "deepseek-v4-pro")]
    Pro,
    #[serde(rename = "deepseek-v4-flash")]
    #[default]
    Flash,
    /// Legacy alias — only accepted during migration, never written to new configs.
    #[serde(rename = "deepseek-chat")]
    LegacyChat,
    /// Legacy alias — only accepted during migration, never written to new configs.
    #[serde(rename = "deepseek-reasoner")]
    LegacyReasoner,
}

impl std::fmt::Display for DeepSeekModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pro => write!(f, "deepseek-v4-pro"),
            Self::Flash => write!(f, "deepseek-v4-flash"),
            Self::LegacyChat => write!(f, "deepseek-chat"),
            Self::LegacyReasoner => write!(f, "deepseek-reasoner"),
        }
    }
}

impl DeepSeekModel {
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::LegacyChat | Self::LegacyReasoner)
    }

    #[must_use]
    pub fn canonical(&self) -> Self {
        match self {
            Self::LegacyChat => Self::Flash,
            Self::LegacyReasoner => Self::Pro,
            other => other.clone(),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Model capability
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone)]
pub struct ModelCapability {
    pub model: DeepSeekModel,
    pub supports_thinking: bool,
    pub supports_tool_calls: bool,
    pub supports_json_output: bool,
    pub supports_fim: bool,
    pub supports_prefix_completion: bool,
    pub default_lane: ExecutionLane,
}

impl ModelCapability {
    #[must_use]
    pub fn for_model(model: &DeepSeekModel) -> Self {
        match model {
            DeepSeekModel::Pro => Self {
                model: model.clone(),
                supports_thinking: true,
                supports_tool_calls: true,
                supports_json_output: true,
                supports_fim: false,
                supports_prefix_completion: true,
                default_lane: ExecutionLane::ChatThinking,
            },
            DeepSeekModel::Flash => Self {
                model: model.clone(),
                supports_thinking: true,
                supports_tool_calls: true,
                supports_json_output: true,
                supports_fim: true,
                supports_prefix_completion: true,
                default_lane: ExecutionLane::ChatNonThinking,
            },
            DeepSeekModel::LegacyChat => Self::for_model(&DeepSeekModel::Flash),
            DeepSeekModel::LegacyReasoner => Self::for_model(&DeepSeekModel::Pro),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionLane {
    #[default]
    ChatNonThinking,
    ChatThinking,
    PlanThinking,
    ToolLoopThinking,
    JsonOutput,
    FimNonThinking,
}

impl ExecutionLane {
    #[must_use]
    pub fn requires_thinking(&self) -> bool {
        matches!(
            self,
            Self::ChatThinking | Self::PlanThinking | Self::ToolLoopThinking
        )
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Chat message types (OpenAI ChatCompletions format)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ChatMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// API-facing content: either a plain string or an array of content parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

impl ChatMessageContent {
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn to_string_lossy(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<Vec<_>>()
                .join(""),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(s) => s.trim().is_empty(),
            Self::Parts(parts) => parts.is_empty(),
        }
    }
}

impl From<String> for ChatMessageContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for ChatMessageContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatContentPart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ChatImageUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tool definition (sent to API)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// API request
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling temperature. DeepSeek recommends 0.0 for coding/math; thinking
    /// turns ignore it, so it only affects non-thinking model calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[must_use]
pub fn estimate_chat_request_tokens(request: &ChatRequest) -> u64 {
    // Estimate in place over the structured request. Serializing the whole
    // request to a String first (the old approach) allocated a multi-hundred-KB
    // buffer every turn once the context window grew — wasteful for a rough
    // preview whose real value comes back in the API usage anyway.
    let mut total: u64 = 0;
    for msg in &request.messages {
        total = total.saturating_add(estimate_tokenish_count(&msg.role));
        if let Some(content) = &msg.content {
            total = total.saturating_add(estimate_content_tokens(content));
        }
        if let Some(reasoning) = &msg.reasoning_content {
            total = total.saturating_add(estimate_tokenish_count(reasoning));
        }
        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                total = total.saturating_add(estimate_tokenish_count(&tc.function.name));
                total = total.saturating_add(estimate_tokenish_count(&tc.function.arguments));
            }
        }
        if let Some(name) = &msg.name {
            total = total.saturating_add(estimate_tokenish_count(name));
        }
        if let Some(id) = &msg.tool_call_id {
            total = total.saturating_add(estimate_tokenish_count(id));
        }
    }
    if let Some(tools) = &request.tools {
        for tool in tools {
            total = total.saturating_add(estimate_tokenish_count(&tool.function.name));
            total = total.saturating_add(estimate_tokenish_count(&tool.function.description));
            // Tool schemas are small and bounded (a fixed roster), so a rough
            // estimate of the serialized params is cheap and keeps overhead.
            total = total.saturating_add(estimate_tokenish_count(
                &tool.function.parameters.to_string(),
            ));
        }
    }
    total.max(1)
}

fn estimate_content_tokens(content: &ChatMessageContent) -> u64 {
    match content {
        ChatMessageContent::Text(text) => estimate_tokenish_count(text),
        ChatMessageContent::Parts(parts) => parts
            .iter()
            .filter_map(|part| part.text.as_deref())
            .map(estimate_tokenish_count)
            .fold(0u64, u64::saturating_add),
    }
}

#[must_use]
pub fn estimate_tokenish_count(value: &str) -> u64 {
    if value.trim().is_empty() {
        return 0;
    }
    let chars = value.chars().count() as u64;
    let non_ascii = value.chars().filter(|c| !c.is_ascii()).count() as u64;
    let ascii = chars.saturating_sub(non_ascii);
    // DeepSeek's published estimate: ~0.3 tokens per English char, ~0.6 per
    // Chinese char (rounded up to stay slightly conservative). Counting every
    // non-ASCII char as a full token previously over-counted CJK by ~67%,
    // tripping auto-compaction far too early on Chinese sessions.
    let ascii_tokens = (ascii * 3).div_ceil(10);
    let cjk_tokens = (non_ascii * 3).div_ceil(5);
    ascii_tokens.saturating_add(cjk_tokens).max(1)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl ThinkingConfig {
    #[must_use]
    pub fn enabled() -> Self {
        Self {
            thinking_type: "enabled".into(),
            effort: None,
        }
    }

    #[must_use]
    pub fn disabled() -> Self {
        Self {
            thinking_type: "disabled".into(),
            effort: None,
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.thinking_type == "enabled"
    }

    #[must_use]
    pub fn with_effort(effort: &str) -> Self {
        Self {
            thinking_type: "enabled".into(),
            effort: Some(effort.to_string()),
        }
    }
}

/// Provider-specific wire format for thinking controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingWireFormat {
    /// Keep DeepSeek-native `thinking.type` plus optional `thinking.effort`.
    #[serde(rename = "deepseek_native")]
    DeepSeekNative,
    /// Keep `thinking.type`, but strip provider-unsupported effort knobs.
    NativeTypeOnly,
    /// DashScope/Qwen-compatible `enable_thinking` and optional budget fields.
    #[serde(rename = "dashscope_enable_thinking")]
    DashScopeEnableThinking,
    /// Qianfan-compatible thinking controls with nested and top-level flags.
    #[serde(rename = "qianfan_thinking")]
    QianfanThinking,
    /// MiniMax OpenAI-compatible reasoning split mode.
    #[serde(rename = "minimax_reasoning_split")]
    MiniMaxReasoningSplit,
    /// Remove thinking controls from the outbound request.
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
}

impl ResponseFormat {
    #[must_use]
    pub fn json_object() -> Self {
        Self {
            format_type: "json_object".into(),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// API response
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatResponseMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponseMessage {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<ChatMessageContent>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub reasoning_details: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_cache_miss_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

impl Usage {
    /// Estimate API cost in CNY (Chinese Yuan) based on DeepSeek pricing.
    /// Flash (V3): input 0.5/MTok (cache hit 0.1), output 2/MTok
    /// Pro (R1):   input 2/MTok   (cache hit 0.5), output 8/MTok
    #[must_use]
    pub fn estimate_cost_cny(&self, model: &DeepSeekModel) -> f64 {
        let (input_rate, cache_hit_rate, output_rate) = match model {
            DeepSeekModel::Pro => (2.0, 0.5, 8.0),
            _ => (0.5, 0.1, 2.0),
        };
        let hit = self.prompt_cache_hit_tokens.unwrap_or(0) as f64 / 1_000_000.0;
        let miss = self.prompt_cache_miss_tokens.unwrap_or(0) as f64 / 1_000_000.0;
        let completion = self.completion_tokens as f64 / 1_000_000.0;
        hit * cache_hit_rate + miss * input_rate + completion * output_rate
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Streaming types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: StreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamDelta {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub reasoning_details: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallDelta {
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<ToolCallFunctionDelta>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolCallFunctionDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// Final assembled result from streaming
#[derive(Debug, Clone, Default)]
pub struct StreamResult {
    pub content: String,
    pub reasoning_content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Cache usage
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Default)]
pub struct CacheUsage {
    pub prompt_cache_hit_tokens: u64,
    pub prompt_cache_miss_tokens: u64,
}

impl CacheUsage {
    #[must_use]
    pub fn from_usage(u: &Usage) -> Self {
        let fallback_hit = u
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens);
        let hit = u.prompt_cache_hit_tokens.or(fallback_hit);
        let miss = u
            .prompt_cache_miss_tokens
            .or_else(|| hit.map(|hit| u.prompt_tokens.saturating_sub(hit)));
        Self {
            prompt_cache_hit_tokens: u64::from(hit.unwrap_or(0)),
            prompt_cache_miss_tokens: u64::from(miss.unwrap_or(0)),
        }
    }

    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let total = self.prompt_cache_hit_tokens + self.prompt_cache_miss_tokens;
        if total == 0 {
            0.0
        } else {
            self.prompt_cache_hit_tokens as f64 / total as f64
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// DeepSeek-native request / response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone)]
pub struct DeepSeekRequest {
    pub model: DeepSeekModel,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub thinking: Option<ThinkingConfig>,
    pub response_format: Option<ResponseFormat>,
    pub stream: bool,
    pub lane: ExecutionLane,
}

#[derive(Debug, Clone)]
pub enum DeepSeekResponseEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ToolCallDelta(ToolCallDeltaAccumulated),
    Usage(Usage),
    Cache(CacheUsage),
    FinishReason(FinishReason),
}

#[derive(Debug, Clone)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Unknown(String),
}

impl From<String> for FinishReason {
    fn from(s: String) -> Self {
        match s.as_str() {
            "stop" => Self::Stop,
            "length" => Self::Length,
            "tool_calls" => Self::ToolCalls,
            "content_filter" => Self::ContentFilter,
            _ => Self::Unknown(s),
        }
    }
}

/// Accumulated tool call from streaming deltas
#[derive(Debug, Clone, Default)]
pub struct ToolCallDeltaAccumulated {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Protocol message — session persistence format
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub type MessageId = Uuid;
pub type TurnId = Uuid;
pub type SubTurnId = Uuid;
pub type SessionId = Uuid;
pub type ToolTurnId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    pub id: MessageId,
    pub role: Role,
    pub content: MessageContent,
    pub reasoning_content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResultRecord>,
    pub turn_id: TurnId,
    pub sub_turn_id: Option<SubTurnId>,
    pub visibility: MessageVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "tool")]
    Tool,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    MultiPart(Vec<ContentPart>),
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    pub part_type: String,
    pub text: Option<String>,
    pub image_url: Option<String>,
}

impl MessageContent {
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn to_string_lossy(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::MultiPart(parts) => parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<Vec<_>>()
                .join(""),
            Self::None => String::new(),
        }
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub tool_call_id: String,
    pub name: String,
    pub result: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageVisibility {
    UserVisible,
    InternalProtocolState,
    AuditOnly,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Reasoning state
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningState {
    pub mode: ThinkingMode,
    pub effort: ReasoningEffort,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<DeepSeekModel>,
    /// Ephemeral per-turn auto-tier override (auto mode only). Never persisted:
    /// the orchestrator sets it from the complexity assessment at turn start and
    /// clears it at turn end. An explicit `selected_model` pin always wins.
    #[serde(skip)]
    pub auto_tier_model: Option<DeepSeekModel>,
    pub active_tool_turn: Option<ToolTurnId>,
    pub preserved_assistant_messages: Vec<MessageId>,
    pub last_cleanup_at_turn: Option<TurnId>,
}

impl Default for ReasoningState {
    fn default() -> Self {
        Self {
            mode: ThinkingMode::Auto,
            effort: ReasoningEffort::default(),
            selected_model: None,
            auto_tier_model: None,
            active_tool_turn: None,
            preserved_assistant_messages: Vec::new(),
            last_cleanup_at_turn: None,
        }
    }
}

impl ReasoningState {
    /// Return the model selected for the current session, falling back to the
    /// legacy effort-based behavior for older sessions without a selected model.
    #[must_use]
    pub fn effective_model(&self) -> DeepSeekModel {
        if let Some(model) = &self.selected_model {
            return model.canonical();
        }
        if let Some(model) = &self.auto_tier_model {
            return model.canonical();
        }
        match self.effort {
            ReasoningEffort::Max | ReasoningEffort::High => DeepSeekModel::Pro,
            _ => DeepSeekModel::Flash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingMode {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "auto")]
    Auto,
}

impl std::fmt::Display for ThinkingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::On => write!(f, "on"),
            Self::Off => write!(f, "off"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReasoningEffort {
    #[serde(rename = "min")]
    Min,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "max")]
    #[default]
    Max,
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Min => write!(f, "min"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Max => write!(f, "max"),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Session
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub name: Option<String>,
    pub project_root: PathBuf,
    pub messages: Vec<ProtocolMessage>,
    pub reasoning_state: ReasoningState,
    pub tool_call_history: Vec<ToolCallRecord>,
    pub checkpoints: Vec<Checkpoint>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: SessionMetadata,
}

impl Session {
    /// Keep at most this many tool-call records in memory to avoid unbounded
    /// session growth during long tool-heavy runs.
    pub const MAX_TOOL_CALL_HISTORY: usize = 512;

    #[inline]
    pub fn push_tool_call_record(&mut self, record: ToolCallRecord) {
        self.tool_call_history.push(record);
        if self.tool_call_history.len() > Self::MAX_TOOL_CALL_HISTORY {
            let remove_count = self.tool_call_history.len() - Self::MAX_TOOL_CALL_HISTORY;
            self.tool_call_history.drain(0..remove_count);
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub total_tokens: u64,
    pub total_cost_estimate: f64,
    /// Cumulative prompt-cache hits across all turns in this session. Used by
    /// `/usage --all-time` to compute cross-session cache effectiveness.
    /// `#[serde(default)]` keeps old session.json files loadable.
    #[serde(default)]
    pub prompt_cache_hit_tokens: u64,
    #[serde(default)]
    pub prompt_cache_miss_tokens: u64,
    pub model_switches: Vec<ModelSwitchRecord>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSwitchRecord {
    pub from: DeepSeekModel,
    pub to: DeepSeekModel,
    pub at: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub result_summary: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub risk_level: String,
    pub approved: bool,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: Uuid,
    pub turn_id: TurnId,
    pub label: String,
    pub file_snapshot: Vec<FileSnapshot>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: PathBuf,
    pub content_hash: String,
    pub backup_path: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_model_overrides_legacy_effort_mapping() {
        let state = ReasoningState {
            effort: ReasoningEffort::Max,
            selected_model: Some(DeepSeekModel::Flash),
            ..ReasoningState::default()
        };

        assert_eq!(state.effective_model(), DeepSeekModel::Flash);
    }

    #[test]
    fn missing_selected_model_preserves_effort_fallback() {
        let state = ReasoningState {
            effort: ReasoningEffort::High,
            selected_model: None,
            ..ReasoningState::default()
        };

        assert_eq!(state.effective_model(), DeepSeekModel::Pro);
    }

    #[test]
    fn token_estimate_follows_deepseek_char_ratios() {
        // ~0.3 tokens per English char, ~0.6 per Chinese char.
        assert_eq!(estimate_tokenish_count(&"a".repeat(100)), 30);
        assert_eq!(estimate_tokenish_count(&"中".repeat(100)), 60);
        // Empty stays zero; a tiny input still rounds up to at least 1.
        assert_eq!(estimate_tokenish_count(""), 0);
        assert_eq!(estimate_tokenish_count("a"), 1);
    }

    #[test]
    fn push_tool_call_record_keeps_session_history_bounded() {
        let mut session = Session {
            id: SessionId::new_v4(),
            name: None,
            project_root: ".".into(),
            messages: Vec::new(),
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: SessionMetadata::default(),
        };

        for i in 0..Session::MAX_TOOL_CALL_HISTORY + 4 {
            session.push_tool_call_record(ToolCallRecord {
                id: format!("id-{i}"),
                name: "read_file".into(),
                arguments: format!("{{\"path\":\"/tmp/{i}\"}}"),
                result_summary: "ok".into(),
                exit_code: Some(0),
                duration_ms: 1,
                risk_level: "safe".into(),
                approved: true,
                at: Utc::now(),
            });
        }

        assert_eq!(
            session.tool_call_history.len(),
            Session::MAX_TOOL_CALL_HISTORY
        );
        assert_eq!(session.tool_call_history[0].id, "id-4");
        assert_eq!(
            session
                .tool_call_history
                .last()
                .expect("tool-call history exists")
                .id,
            format!("id-{}", Session::MAX_TOOL_CALL_HISTORY + 3)
        );
    }

    #[test]
    fn chat_request_token_estimate_counts_content_not_json_overhead() {
        let request = ChatRequest {
            model: "deepseek-v4-flash".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: Some(ChatMessageContent::Text("a".repeat(100))),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            tools: None,
            thinking: None,
            response_format: None,
            stream: true,
            max_tokens: None,
            temperature: None,
        };

        // ~0.3 tokens/char for the 100-char body plus a couple for the role —
        // it reflects the actual content, not an inflated JSON envelope.
        let estimate = estimate_chat_request_tokens(&request);
        assert!((30..=40).contains(&estimate), "got {estimate}");
    }

    #[test]
    fn explicit_pin_overrides_auto_tier() {
        let state = ReasoningState {
            effort: ReasoningEffort::Max,
            selected_model: Some(DeepSeekModel::Pro),
            auto_tier_model: Some(DeepSeekModel::Flash),
            ..ReasoningState::default()
        };

        assert_eq!(state.effective_model(), DeepSeekModel::Pro);
    }

    #[test]
    fn auto_tier_downgrades_unpinned_effort_default() {
        let state = ReasoningState {
            effort: ReasoningEffort::Max, // would otherwise resolve to Pro
            selected_model: None,
            auto_tier_model: Some(DeepSeekModel::Flash),
            ..ReasoningState::default()
        };

        assert_eq!(state.effective_model(), DeepSeekModel::Flash);
    }
}

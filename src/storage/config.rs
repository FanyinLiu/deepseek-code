use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::deepseek::{DeepSeekModel, ReasoningEffort, ThinkingMode};
use crate::policy::sandbox::{CommandSandboxConfig, CommandSandboxMode};
use crate::provider::{ProviderConfig, ProviderEndpointConfig, ProviderKind};

/// Full application configuration, resolved from layered sources.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: ModelConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub router: RouterConfig,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    pub profiles: std::collections::BTreeMap<String, ProfileConfig>,
    #[serde(default)]
    pub subagent: SubagentConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    #[serde(default = "default_default_model")]
    pub default: DeepSeekModel,
    #[serde(default = "default_heavy_model")]
    pub heavy: DeepSeekModel,
    #[serde(default = "default_thinking_mode")]
    pub thinking_mode: ThinkingMode,
    #[serde(default)]
    pub reasoning_effort: ReasoningEffort,
    #[serde(default)]
    pub allow_legacy_model_alias: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default = "default_lane")]
    pub default_lane: String,
    #[serde(default = "default_plan_lane")]
    pub plan_lane: String,
    #[serde(default = "default_fim_lane")]
    pub fim_lane: String,
    #[serde(default)]
    pub json_output_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_engine")]
    pub engine: String,
    #[serde(default)]
    pub use_deepseek_rerank: bool,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    #[serde(default)]
    pub include_git_diff: bool,
    #[serde(default)]
    pub include_sessions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub stable_prefix_enabled: bool,
    #[serde(default)]
    pub warn_on_low_cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub auto_approve_safe_read: bool,
    #[serde(default)]
    pub network_access: bool,
    #[serde(default = "default_command_timeout")]
    pub command_timeout_seconds: u64,
    #[serde(default)]
    pub require_approval_for_write: bool,
    #[serde(default)]
    pub require_approval_for_command: bool,
    #[serde(default)]
    pub block_protected_paths: bool,
    #[serde(default)]
    pub normalize_unicode_commands: bool,
    #[serde(default)]
    pub auto_mode: bool,
    #[serde(default)]
    pub autonomy_level: AutonomyLevel,
    #[serde(default)]
    pub command_sandbox: CommandSandboxConfig,
    /// Optional per-turn allowlist of tool names. `None` = no restriction;
    /// `Some(set)` = block any tool not in the set. Populated by custom
    /// slash commands declaring `allowed-tools:` in their frontmatter; not
    /// persisted to disk.
    #[serde(skip)]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialConfig {
    api_key: Option<String>,
    model: Option<PartialModelConfig>,
    execution: Option<PartialExecutionConfig>,
    search: Option<PartialSearchConfig>,
    cache: Option<PartialCacheConfig>,
    policy: Option<PartialPolicyConfig>,
    paths: Option<PathsConfig>,
    ui: Option<PartialUiConfig>,
    telemetry: Option<PartialTelemetryConfig>,
    provider: Option<PartialProviderConfig>,
    router: Option<PartialRouterConfig>,
    profiles: Option<std::collections::BTreeMap<String, ProfileConfig>>,
    subagent: Option<PartialSubagentConfig>,
    mcp: Option<PartialMcpConfig>,
    hooks: Option<PartialHooksConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialModelConfig {
    default: Option<DeepSeekModel>,
    heavy: Option<DeepSeekModel>,
    thinking_mode: Option<ThinkingMode>,
    reasoning_effort: Option<ReasoningEffort>,
    allow_legacy_model_alias: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialExecutionConfig {
    default_lane: Option<String>,
    plan_lane: Option<String>,
    fim_lane: Option<String>,
    json_output_enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialSearchConfig {
    engine: Option<String>,
    use_deepseek_rerank: Option<bool>,
    max_results: Option<usize>,
    max_context_tokens: Option<usize>,
    include_git_diff: Option<bool>,
    include_sessions: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialCacheConfig {
    stable_prefix_enabled: Option<bool>,
    warn_on_low_cache_hit: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialProviderConfig {
    default: Option<ProviderKind>,
    deepseek: Option<PartialProviderEndpointConfig>,
    qwen: Option<PartialProviderEndpointConfig>,
    kimi: Option<PartialProviderEndpointConfig>,
    zhipu: Option<PartialProviderEndpointConfig>,
    minimax: Option<PartialProviderEndpointConfig>,
    tencent: Option<PartialProviderEndpointConfig>,
    qianfan: Option<PartialProviderEndpointConfig>,
    stepfun: Option<PartialProviderEndpointConfig>,
    doubao: Option<PartialProviderEndpointConfig>,
    openrouter: Option<PartialProviderEndpointConfig>,
    #[serde(rename = "openai-compatible")]
    openai_compatible: Option<PartialProviderEndpointConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialProviderEndpointConfig {
    base_url: Option<String>,
    pro_model: Option<String>,
    flash_model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialTelemetryConfig {
    enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialRouterConfig {
    enabled: Option<bool>,
    conservative: Option<bool>,
    use_model_classifier: Option<bool>,
    simple_threshold: Option<u32>,
    confidence_threshold: Option<f64>,
    shadow_mode: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialSubagentConfig {
    enabled: Option<bool>,
    swarm_enabled: Option<bool>,
    max_parallel: Option<usize>,
    auto_decompose: Option<bool>,
    write_requires_approval: Option<bool>,
    command_requires_approval: Option<bool>,
    default_model: Option<String>,
    allow_custom_agents: Option<bool>,
    custom_agents_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialHooksConfig {
    user_prompt_submit: Option<Vec<String>>,
    pre_tool: Option<Vec<String>>,
    post_tool: Option<Vec<String>>,
    session_start: Option<Vec<String>>,
    session_end: Option<Vec<String>>,
    stop: Option<Vec<String>>,
    turn_stop: Option<Vec<String>>,
    subagent_stop: Option<Vec<String>>,
    pre_compact: Option<Vec<String>>,
    notification: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialMcpConfig {
    enabled: Option<bool>,
    servers: Option<std::collections::BTreeMap<String, PartialMcpServerEntryConfig>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialMcpServerEntryConfig {
    transport: Option<crate::mcp::client::McpTransport>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<std::collections::HashMap<String, String>>,
    url: Option<String>,
    headers: Option<std::collections::HashMap<String, String>>,
    include_tools: Option<Vec<String>>,
    exclude_tools: Option<Vec<String>>,
    trust: Option<bool>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialPolicyConfig {
    auto_approve_safe_read: Option<bool>,
    network_access: Option<bool>,
    command_timeout_seconds: Option<u64>,
    require_approval_for_write: Option<bool>,
    require_approval_for_command: Option<bool>,
    block_protected_paths: Option<bool>,
    normalize_unicode_commands: Option<bool>,
    auto_mode: Option<bool>,
    autonomy_level: Option<AutonomyLevel>,
    command_sandbox: Option<PartialCommandSandboxConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialCommandSandboxConfig {
    mode: Option<CommandSandboxMode>,
    network_access: Option<bool>,
    readable_roots: Option<Vec<PathBuf>>,
    writable_roots: Option<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialUiConfig {
    language: Option<String>,
    theme: Option<String>,
    motion: Option<String>,
    renderer: Option<String>,
    show_reasoning_summary: Option<bool>,
    show_raw_reasoning: Option<bool>,
    show_cache_hud: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AutonomyLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl AutonomyLevel {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    #[must_use]
    pub fn auto_workspace_writes(self) -> bool {
        matches!(self, Self::Low | Self::Medium | Self::High)
    }

    #[must_use]
    pub fn auto_local_commands(self) -> bool {
        matches!(self, Self::Medium | Self::High)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default)]
    pub user_prompt_submit: Vec<String>,
    #[serde(default)]
    pub pre_tool: Vec<String>,
    #[serde(default)]
    pub post_tool: Vec<String>,
    #[serde(default)]
    pub session_start: Vec<String>,
    #[serde(default)]
    pub session_end: Vec<String>,
    /// Legacy alias merged into `session_end` for backward compatibility.
    #[serde(default)]
    pub stop: Vec<String>,
    /// Fires when a turn is interrupted by user cancel / abort, distinct from
    /// session end. Kept separate from `stop` to avoid breaking legacy configs.
    #[serde(default)]
    pub turn_stop: Vec<String>,
    /// Fires after each subagent task completes (success or failure).
    #[serde(default)]
    pub subagent_stop: Vec<String>,
    /// Fires right before context compaction runs.
    #[serde(default)]
    pub pre_compact: Vec<String>,
    /// Fires on noteworthy async events (plan proposed, approval required,
    /// errors). Use to bridge octocode events to OS notifications.
    #[serde(default)]
    pub notification: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    #[serde(default = "default_protected_paths")]
    pub protected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_motion")]
    pub motion: String,
    #[serde(default = "default_renderer")]
    pub renderer: String,
    #[serde(default)]
    pub show_reasoning_summary: bool,
    #[serde(default)]
    pub show_raw_reasoning: bool,
    #[serde(default)]
    pub show_cache_hud: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelemetryConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_router_enabled")]
    pub enabled: bool,
    #[serde(default = "default_router_conservative")]
    pub conservative: bool,
    #[serde(default = "default_router_use_model")]
    pub use_model_classifier: bool,
    #[serde(default = "default_router_simple_threshold")]
    pub simple_threshold: u32,
    #[serde(default = "default_router_confidence_threshold")]
    pub confidence_threshold: f64,
    #[serde(default)]
    pub shadow_mode: bool,
}

/// Post-edit LSP diagnostics. Off by default and inert until a language server
/// is configured — it requires the server binary to be installed locally.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Language id → server command, e.g. `rust = ["rust-analyzer"]` or
    /// `python = ["pyright-langserver", "--stdio"]`. Languages with no entry
    /// are skipped.
    #[serde(default)]
    pub servers: std::collections::HashMap<String, Vec<String>>,
}

/// Subagent system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentConfig {
    #[serde(default = "default_subagent_enabled")]
    pub enabled: bool,
    #[serde(default = "default_subagent_swarm_enabled")]
    pub swarm_enabled: bool,
    #[serde(default = "default_subagent_max_parallel")]
    pub max_parallel: usize,
    #[serde(default = "default_subagent_auto_decompose")]
    pub auto_decompose: bool,
    #[serde(default = "default_subagent_write_requires_approval")]
    pub write_requires_approval: bool,
    #[serde(default = "default_subagent_command_requires_approval")]
    pub command_requires_approval: bool,
    #[serde(default = "default_subagent_default_model")]
    pub default_model: String,
    #[serde(default)]
    pub allow_custom_agents: bool,
    #[serde(default)]
    pub custom_agents_dir: Option<std::path::PathBuf>,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: default_router_enabled(),
            conservative: default_router_conservative(),
            use_model_classifier: default_router_use_model(),
            simple_threshold: default_router_simple_threshold(),
            confidence_threshold: default_router_confidence_threshold(),
            shadow_mode: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<DeepSeekModel>,
    #[serde(default)]
    pub thinking_mode: Option<ThinkingMode>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: default_default_model(),
            heavy: default_heavy_model(),
            thinking_mode: default_thinking_mode(),
            reasoning_effort: ReasoningEffort::default(),
            allow_legacy_model_alias: false,
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            default_lane: default_lane(),
            plan_lane: default_plan_lane(),
            fim_lane: default_fim_lane(),
            json_output_enabled: false,
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            engine: default_engine(),
            use_deepseek_rerank: true,
            max_results: default_max_results(),
            max_context_tokens: default_max_context_tokens(),
            include_git_diff: true,
            include_sessions: true,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            stable_prefix_enabled: true,
            warn_on_low_cache_hit: true,
        }
    }
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            auto_approve_safe_read: true,
            network_access: false,
            command_timeout_seconds: default_command_timeout(),
            require_approval_for_write: true,
            require_approval_for_command: true,
            block_protected_paths: true,
            normalize_unicode_commands: true,
            auto_mode: false,
            autonomy_level: AutonomyLevel::Off,
            command_sandbox: CommandSandboxConfig::default(),
            allowed_tools: None,
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            protected: default_protected_paths(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: default_language(),
            theme: default_theme(),
            motion: default_motion(),
            renderer: default_renderer(),
            show_reasoning_summary: true,
            show_raw_reasoning: false,
            show_cache_hud: true,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Default values
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn default_default_model() -> DeepSeekModel {
    DeepSeekModel::Flash
}
fn default_heavy_model() -> DeepSeekModel {
    DeepSeekModel::Pro
}
fn default_thinking_mode() -> ThinkingMode {
    ThinkingMode::Auto
}
fn default_lane() -> String {
    "chat_non_thinking".into()
}
fn default_plan_lane() -> String {
    "plan_thinking".into()
}
fn default_fim_lane() -> String {
    "fim_non_thinking".into()
}
fn default_engine() -> String {
    "local".into()
}
fn default_max_results() -> usize {
    50
}
fn default_max_context_tokens() -> usize {
    // 0 means "use the model's full context window" (see `context_budget_for`).
    // DeepSeek V4 is 1M tokens; the old fixed 12K cap stranded ~99% of it and
    // forced auto-compaction at ~10K. Set an explicit positive value to cap.
    0
}
fn default_command_timeout() -> u64 {
    120
}
fn default_language() -> String {
    "zh-CN".into()
}
fn default_theme() -> String {
    "auto".into()
}
fn default_motion() -> String {
    "subtle".into()
}
fn default_renderer() -> String {
    "auto".into()
}

fn default_router_enabled() -> bool {
    true
}
fn default_router_conservative() -> bool {
    true
}
fn default_router_use_model() -> bool {
    true
}
fn default_router_simple_threshold() -> u32 {
    40
}
fn default_router_confidence_threshold() -> f64 {
    0.75
}

fn default_protected_paths() -> Vec<String> {
    vec![
        "~/.ssh/**".into(),
        "~/.aws/**".into(),
        "~/.gnupg/**".into(),
        "**/.env".into(),
        "**/.env.*".into(),
        "**/credentials*".into(),
        "/etc/**".into(),
    ]
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Resolution — merge layered configs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Config {
    /// Load from the standard file paths:
    /// 1. ~/.octocode/config.toml (user-global)
    /// 2. ./.octocode/config.toml (project, checked in)
    /// 3. ./.octocode/local.toml (local override, gitignored)
    ///
    /// Later files override earlier ones.
    pub fn load(project_root: Option<&std::path::Path>) -> Result<Self, anyhow::Error> {
        let mut config = Config::default();

        // 1. User global
        if let Some(user_dir) = crate::storage::user_config_dir() {
            let global_path = user_dir.join("config.toml");
            if global_path.exists() {
                let content = crate::storage::read_text_file_capped(&global_path)?;
                let patch = parse_config_patch(&content)?;
                config = config.merge_with_config_patch(patch);
            }
        }

        // 2 & 3. Project-level
        if let Some(root) = project_root.map(normalize_project_root) {
            let project_config = root.join(".octocode").join("config.toml");
            if project_config.exists() {
                let content = crate::storage::read_text_file_capped(&project_config)?;
                let patch = parse_config_patch(&content)?;
                config = config.merge_with_config_patch(patch);
            }

            let local_config = root.join(".octocode").join("local.toml");
            if local_config.exists() {
                let content = crate::storage::read_text_file_capped(&local_config)?;
                let patch = parse_config_patch(&content)?;
                config = config.merge_with_config_patch(patch);
            }
        }

        Ok(config)
    }

    /// Persist the interactive settings surface to project-local config.
    ///
    /// This writes only non-secret UI/runtime knobs that can be changed from
    /// the TUI settings panel. It intentionally avoids serializing the resolved
    /// full config because the resolved config may include a user-global API key.
    pub fn save_project_local_settings(
        &self,
        project_root: &std::path::Path,
    ) -> Result<PathBuf, anyhow::Error> {
        let root = normalize_project_root(project_root);
        let config_dir = root.join(".octocode");
        std::fs::create_dir_all(&config_dir)?;
        let path = config_dir.join("local.toml");

        let mut value = if path.exists() {
            crate::storage::read_text_file_capped(&path)?
                .parse::<toml::Value>()
                .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()))
        } else {
            toml::Value::Table(toml::map::Map::new())
        };
        if !value.is_table() {
            value = toml::Value::Table(toml::map::Map::new());
        }

        set_toml_string(
            &mut value,
            "model",
            "default",
            self.model.default.to_string(),
        );
        set_toml_string(&mut value, "model", "heavy", self.model.heavy.to_string());
        set_toml_string(
            &mut value,
            "model",
            "thinking_mode",
            self.model.thinking_mode.to_string(),
        );
        set_toml_string(
            &mut value,
            "model",
            "reasoning_effort",
            self.model.reasoning_effort.to_string(),
        );
        set_toml_string(
            &mut value,
            "provider",
            "default",
            self.provider.default.as_str().to_string(),
        );
        set_toml_string(&mut value, "ui", "language", self.ui.language.clone());
        set_toml_string(&mut value, "ui", "theme", self.ui.theme.clone());
        set_toml_string(&mut value, "ui", "motion", self.ui.motion.clone());
        set_toml_string(&mut value, "ui", "renderer", self.ui.renderer.clone());
        set_toml_bool(
            &mut value,
            "ui",
            "show_reasoning_summary",
            self.ui.show_reasoning_summary,
        );
        set_toml_bool(
            &mut value,
            "ui",
            "show_raw_reasoning",
            self.ui.show_raw_reasoning,
        );
        set_toml_bool(&mut value, "ui", "show_cache_hud", self.ui.show_cache_hud);
        set_toml_string(
            &mut value,
            "policy",
            "autonomy_level",
            self.policy.autonomy_level.as_str().to_string(),
        );
        set_toml_bool(
            &mut value,
            "policy",
            "auto_approve_safe_read",
            self.policy.auto_approve_safe_read,
        );
        set_toml_bool(&mut value, "policy", "auto_mode", self.policy.auto_mode);
        set_toml_bool(
            &mut value,
            "policy",
            "require_approval_for_write",
            self.policy.require_approval_for_write,
        );
        set_toml_bool(
            &mut value,
            "policy",
            "require_approval_for_command",
            self.policy.require_approval_for_command,
        );
        set_toml_bool(
            &mut value,
            "policy",
            "network_access",
            self.policy.network_access,
        );
        set_toml_bool(
            &mut value,
            "policy",
            "block_protected_paths",
            self.policy.block_protected_paths,
        );
        set_toml_integer(
            &mut value,
            "policy",
            "command_timeout_seconds",
            self.policy.command_timeout_seconds as i64,
        );
        set_toml_bool(&mut value, "router", "enabled", self.router.enabled);
        set_toml_bool(
            &mut value,
            "router",
            "use_model_classifier",
            self.router.use_model_classifier,
        );
        set_toml_bool(&mut value, "mcp", "enabled", self.mcp.enabled);
        set_toml_bool(&mut value, "subagent", "enabled", self.subagent.enabled);
        set_toml_bool(
            &mut value,
            "subagent",
            "swarm_enabled",
            self.subagent.swarm_enabled,
        );
        set_toml_integer(
            &mut value,
            "subagent",
            "max_parallel",
            self.subagent.max_parallel as i64,
        );
        set_toml_bool(
            &mut value,
            "subagent",
            "auto_decompose",
            self.subagent.auto_decompose,
        );
        set_toml_bool(
            &mut value,
            "subagent",
            "allow_custom_agents",
            self.subagent.allow_custom_agents,
        );
        set_toml_bool(&mut value, "telemetry", "enabled", self.telemetry.enabled);

        crate::storage::atomic::write_text_atomic(&path, &toml::to_string_pretty(&value)?)?;
        Ok(path)
    }

    fn merge_with_config_patch(self, patch: PartialConfig) -> Self {
        let Self {
            api_key,
            model,
            execution,
            search,
            cache,
            policy,
            paths,
            ui,
            telemetry,
            provider,
            router,
            profiles,
            subagent,
            mcp,
            hooks,
            lsp,
        } = self;

        let search = match patch.search {
            Some(next) => search.merge_search(next),
            None => search,
        };
        let cache = match patch.cache {
            Some(next) => cache.merge_cache(next),
            None => cache,
        };
        let paths = match patch.paths {
            Some(next) => paths.merge_paths(next),
            None => paths,
        };
        let ui = ui.merge_ui(patch.ui);
        let telemetry = match patch.telemetry {
            Some(next) => telemetry.merge_telemetry(next),
            None => telemetry,
        };
        let router = match patch.router {
            Some(next) => router.merge_router(next),
            None => router,
        };
        let subagent = match patch.subagent {
            Some(next) => subagent.merge_subagent(next),
            None => subagent,
        };

        Self {
            api_key: patch.api_key.or(api_key),
            model: model.merge_model(patch.model),
            execution: execution.merge_execution(patch.execution),
            search,
            cache,
            policy: policy.merge_policy(patch.policy),
            paths,
            ui,
            telemetry,
            provider: provider.merge_provider(patch.provider),
            router,
            profiles: merge_profiles(profiles, patch.profiles),
            subagent,
            mcp: mcp.merge_mcp(patch.mcp),
            hooks: hooks.merge_hooks(patch.hooks),
            lsp,
        }
    }
}

fn toml_table_mut<'a>(
    root: &'a mut toml::Value,
    table: &str,
) -> &'a mut toml::map::Map<String, toml::Value> {
    let root_table = root
        .as_table_mut()
        .expect("root config value is normalized to a table");
    root_table
        .entry(table.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if !root_table.get(table).is_some_and(toml::Value::is_table) {
        root_table.insert(table.to_string(), toml::Value::Table(toml::map::Map::new()));
    }
    root_table
        .get_mut(table)
        .and_then(toml::Value::as_table_mut)
        .expect("table entry was just created")
}

fn set_toml_string(root: &mut toml::Value, table: &str, key: &str, value: String) {
    toml_table_mut(root, table).insert(key.to_string(), toml::Value::String(value));
}

fn set_toml_bool(root: &mut toml::Value, table: &str, key: &str, value: bool) {
    toml_table_mut(root, table).insert(key.to_string(), toml::Value::Boolean(value));
}

fn set_toml_integer(root: &mut toml::Value, table: &str, key: &str, value: i64) {
    toml_table_mut(root, table).insert(key.to_string(), toml::Value::Integer(value));
}

fn merge_profiles(
    mut base: std::collections::BTreeMap<String, ProfileConfig>,
    patch: Option<std::collections::BTreeMap<String, ProfileConfig>>,
) -> std::collections::BTreeMap<String, ProfileConfig> {
    let Some(patch) = patch else {
        return base;
    };

    for (name, profile_patch) in patch {
        match base.remove(&name) {
            Some(existing) => {
                base.insert(name, existing.merge_profile(profile_patch));
            }
            None => {
                base.insert(name, profile_patch);
            }
        }
    }
    base
}

fn parse_config_patch(content: &str) -> Result<PartialConfig, anyhow::Error> {
    Ok(toml::from_str(content)?)
}

#[cfg(test)]
fn parse_policy_patch(content: &str) -> Result<Option<PartialPolicyConfig>, anyhow::Error> {
    Ok(parse_config_patch(content)?.policy)
}

impl SearchConfig {
    fn merge_search(self, patch: PartialSearchConfig) -> Self {
        Self {
            engine: patch.engine.unwrap_or(self.engine),
            use_deepseek_rerank: patch
                .use_deepseek_rerank
                .unwrap_or(self.use_deepseek_rerank),
            max_results: patch.max_results.unwrap_or(self.max_results),
            max_context_tokens: patch.max_context_tokens.unwrap_or(self.max_context_tokens),
            include_git_diff: patch.include_git_diff.unwrap_or(self.include_git_diff),
            include_sessions: patch.include_sessions.unwrap_or(self.include_sessions),
        }
    }
}
impl CacheConfig {
    fn merge_cache(self, patch: PartialCacheConfig) -> Self {
        Self {
            stable_prefix_enabled: patch
                .stable_prefix_enabled
                .unwrap_or(self.stable_prefix_enabled),
            warn_on_low_cache_hit: patch
                .warn_on_low_cache_hit
                .unwrap_or(self.warn_on_low_cache_hit),
        }
    }
}

impl ModelConfig {
    fn merge_model(self, patch: Option<PartialModelConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            default: patch.default.unwrap_or(self.default),
            heavy: patch.heavy.unwrap_or(self.heavy),
            thinking_mode: patch.thinking_mode.unwrap_or(self.thinking_mode),
            reasoning_effort: patch.reasoning_effort.unwrap_or(self.reasoning_effort),
            allow_legacy_model_alias: patch
                .allow_legacy_model_alias
                .unwrap_or(self.allow_legacy_model_alias),
        }
    }
}

impl ExecutionConfig {
    fn merge_execution(self, patch: Option<PartialExecutionConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            default_lane: patch.default_lane.unwrap_or(self.default_lane),
            plan_lane: patch.plan_lane.unwrap_or(self.plan_lane),
            fim_lane: patch.fim_lane.unwrap_or(self.fim_lane),
            json_output_enabled: patch
                .json_output_enabled
                .unwrap_or(self.json_output_enabled),
        }
    }
}

impl ProviderConfig {
    fn merge_provider(self, patch: Option<PartialProviderConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            default: patch.default.unwrap_or(self.default),
            deepseek: self.deepseek.merge_provider_endpoint(patch.deepseek),
            qwen: self.qwen.merge_provider_endpoint(patch.qwen),
            kimi: self.kimi.merge_provider_endpoint(patch.kimi),
            zhipu: self.zhipu.merge_provider_endpoint(patch.zhipu),
            minimax: self.minimax.merge_provider_endpoint(patch.minimax),
            tencent: self.tencent.merge_provider_endpoint(patch.tencent),
            qianfan: self.qianfan.merge_provider_endpoint(patch.qianfan),
            stepfun: self.stepfun.merge_provider_endpoint(patch.stepfun),
            doubao: self.doubao.merge_provider_endpoint(patch.doubao),
            openrouter: self.openrouter.merge_provider_endpoint(patch.openrouter),
            openai_compatible: self
                .openai_compatible
                .merge_provider_endpoint(patch.openai_compatible),
        }
    }
}

impl ProviderEndpointConfig {
    fn merge_provider_endpoint(self, patch: Option<PartialProviderEndpointConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            base_url: patch.base_url.or(self.base_url),
            pro_model: patch.pro_model.or(self.pro_model),
            flash_model: patch.flash_model.or(self.flash_model),
        }
    }
}
impl PolicyConfig {
    fn merge_policy(self, patch: Option<PartialPolicyConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            auto_approve_safe_read: patch
                .auto_approve_safe_read
                .unwrap_or(self.auto_approve_safe_read),
            network_access: patch.network_access.unwrap_or(self.network_access),
            command_timeout_seconds: patch
                .command_timeout_seconds
                .unwrap_or(self.command_timeout_seconds),
            require_approval_for_write: patch
                .require_approval_for_write
                .unwrap_or(self.require_approval_for_write),
            require_approval_for_command: patch
                .require_approval_for_command
                .unwrap_or(self.require_approval_for_command),
            block_protected_paths: patch
                .block_protected_paths
                .unwrap_or(self.block_protected_paths),
            normalize_unicode_commands: patch
                .normalize_unicode_commands
                .unwrap_or(self.normalize_unicode_commands),
            auto_mode: patch.auto_mode.unwrap_or(self.auto_mode),
            autonomy_level: patch.autonomy_level.unwrap_or(self.autonomy_level),
            command_sandbox: self
                .command_sandbox
                .merge_command_sandbox(patch.command_sandbox),
            allowed_tools: self.allowed_tools,
        }
    }
}

impl CommandSandboxConfig {
    fn merge_command_sandbox(self, patch: Option<PartialCommandSandboxConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            mode: patch.mode.unwrap_or(self.mode),
            network_access: patch.network_access.unwrap_or(self.network_access),
            readable_roots: patch.readable_roots.unwrap_or(self.readable_roots),
            writable_roots: patch.writable_roots.unwrap_or(self.writable_roots),
        }
    }
}
impl PathsConfig {
    fn merge_paths(self, other: Self) -> Self {
        let mut protected = self.protected;
        for p in other.protected {
            if !protected.contains(&p) {
                protected.push(p);
            }
        }
        Self { protected }
    }
}
impl UiConfig {
    fn merge_ui(self, patch: Option<PartialUiConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            language: patch.language.unwrap_or(self.language),
            theme: patch.theme.unwrap_or(self.theme),
            motion: patch.motion.unwrap_or(self.motion),
            renderer: patch.renderer.unwrap_or(self.renderer),
            show_reasoning_summary: patch
                .show_reasoning_summary
                .unwrap_or(self.show_reasoning_summary),
            show_raw_reasoning: patch.show_raw_reasoning.unwrap_or(self.show_raw_reasoning),
            show_cache_hud: patch.show_cache_hud.unwrap_or(self.show_cache_hud),
        }
    }
}
impl TelemetryConfig {
    fn merge_telemetry(self, patch: PartialTelemetryConfig) -> Self {
        Self {
            enabled: patch.enabled.unwrap_or(self.enabled),
        }
    }
}
impl RouterConfig {
    fn merge_router(self, patch: PartialRouterConfig) -> Self {
        Self {
            enabled: patch.enabled.unwrap_or(self.enabled),
            conservative: patch.conservative.unwrap_or(self.conservative),
            use_model_classifier: patch
                .use_model_classifier
                .unwrap_or(self.use_model_classifier),
            simple_threshold: patch.simple_threshold.unwrap_or(self.simple_threshold),
            confidence_threshold: patch
                .confidence_threshold
                .unwrap_or(self.confidence_threshold),
            shadow_mode: patch.shadow_mode.unwrap_or(self.shadow_mode),
        }
    }
}

impl Default for SubagentConfig {
    fn default() -> Self {
        Self {
            enabled: default_subagent_enabled(),
            swarm_enabled: default_subagent_swarm_enabled(),
            max_parallel: default_subagent_max_parallel(),
            auto_decompose: default_subagent_auto_decompose(),
            write_requires_approval: default_subagent_write_requires_approval(),
            command_requires_approval: default_subagent_command_requires_approval(),
            default_model: default_subagent_default_model(),
            allow_custom_agents: false,
            custom_agents_dir: None,
        }
    }
}

impl SubagentConfig {
    fn merge_subagent(self, patch: PartialSubagentConfig) -> Self {
        Self {
            enabled: patch.enabled.unwrap_or(self.enabled),
            swarm_enabled: patch.swarm_enabled.unwrap_or(self.swarm_enabled),
            max_parallel: patch.max_parallel.unwrap_or(self.max_parallel),
            auto_decompose: patch.auto_decompose.unwrap_or(self.auto_decompose),
            write_requires_approval: patch
                .write_requires_approval
                .unwrap_or(self.write_requires_approval),
            command_requires_approval: patch
                .command_requires_approval
                .unwrap_or(self.command_requires_approval),
            default_model: patch.default_model.unwrap_or(self.default_model),
            allow_custom_agents: patch
                .allow_custom_agents
                .unwrap_or(self.allow_custom_agents),
            custom_agents_dir: patch.custom_agents_dir.or(self.custom_agents_dir),
        }
    }
}

impl ProfileConfig {
    fn merge_profile(self, patch: Self) -> Self {
        Self {
            api_key: patch.api_key.or(self.api_key),
            model: patch.model.or(self.model),
            thinking_mode: patch.thinking_mode.or(self.thinking_mode),
        }
    }
}

impl HooksConfig {
    fn merge_hooks(self, patch: Option<PartialHooksConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        Self {
            user_prompt_submit: patch.user_prompt_submit.unwrap_or(self.user_prompt_submit),
            pre_tool: patch.pre_tool.unwrap_or(self.pre_tool),
            post_tool: patch.post_tool.unwrap_or(self.post_tool),
            session_start: patch.session_start.unwrap_or(self.session_start),
            session_end: patch.session_end.unwrap_or(self.session_end),
            stop: patch.stop.unwrap_or(self.stop),
            turn_stop: patch.turn_stop.unwrap_or(self.turn_stop),
            subagent_stop: patch.subagent_stop.unwrap_or(self.subagent_stop),
            pre_compact: patch.pre_compact.unwrap_or(self.pre_compact),
            notification: patch.notification.unwrap_or(self.notification),
        }
    }
}

impl McpConfig {
    fn merge_mcp(self, patch: Option<PartialMcpConfig>) -> Self {
        let Some(patch) = patch else {
            return self;
        };
        let mut servers = self.servers;
        if let Some(server_patches) = patch.servers {
            for (name, server_patch) in server_patches {
                match servers.remove(&name) {
                    Some(existing) => {
                        servers.insert(name, existing.merge_mcp_server(server_patch));
                    }
                    None => {
                        if let Some(new_server) = server_patch.into_mcp_server() {
                            servers.insert(name, new_server);
                        }
                    }
                }
            }
        }
        Self {
            enabled: patch.enabled.unwrap_or(self.enabled),
            servers,
        }
    }
}

impl McpServerEntryConfig {
    fn merge_mcp_server(self, patch: PartialMcpServerEntryConfig) -> Self {
        Self {
            transport: patch.transport.unwrap_or(self.transport),
            command: patch.command.or(self.command),
            args: patch.args.unwrap_or(self.args),
            env: patch.env.or(self.env),
            url: patch.url.or(self.url),
            headers: patch.headers.or(self.headers),
            include_tools: patch.include_tools.unwrap_or(self.include_tools),
            exclude_tools: patch.exclude_tools.unwrap_or(self.exclude_tools),
            trust: patch.trust.unwrap_or(self.trust),
            timeout_ms: patch.timeout_ms.unwrap_or(self.timeout_ms),
        }
    }
}

impl PartialMcpServerEntryConfig {
    fn into_mcp_server(self) -> Option<McpServerEntryConfig> {
        let transport = self.transport.unwrap_or_default();
        if transport == crate::mcp::client::McpTransport::Stdio && self.command.is_none() {
            return None;
        }
        if transport != crate::mcp::client::McpTransport::Stdio && self.url.is_none() {
            return None;
        }
        Some(McpServerEntryConfig {
            transport,
            command: self.command,
            args: self.args.unwrap_or_default(),
            env: self.env,
            url: self.url,
            headers: self.headers,
            include_tools: self.include_tools.unwrap_or_default(),
            exclude_tools: self.exclude_tools.unwrap_or_default(),
            trust: self.trust.unwrap_or(false),
            timeout_ms: self.timeout_ms.unwrap_or_else(default_mcp_timeout_ms),
        })
    }
}

fn default_subagent_enabled() -> bool {
    true
}
fn default_subagent_swarm_enabled() -> bool {
    true
}
fn default_subagent_max_parallel() -> usize {
    4
}
fn default_subagent_auto_decompose() -> bool {
    true
}
fn default_subagent_write_requires_approval() -> bool {
    true
}
fn default_subagent_command_requires_approval() -> bool {
    true
}
fn default_subagent_default_model() -> String {
    "deepseek-v4-flash".to_string()
}

// ---------------------------------------------------------------------------
// MCP configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub servers: std::collections::BTreeMap<String, McpServerEntryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntryConfig {
    #[serde(default)]
    pub transport: crate::mcp::client::McpTransport,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub include_tools: Vec<String>,
    #[serde(default)]
    pub exclude_tools: Vec<String>,
    #[serde(default)]
    pub trust: bool,
    #[serde(default = "default_mcp_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_mcp_timeout_ms() -> u64 {
    crate::mcp::client::DEFAULT_MCP_TIMEOUT_MS
}

/// Find the project root by looking for a .octocode directory or .git directory.
#[must_use]
pub fn find_project_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    Some(find_project_root_from(&cwd))
}

/// Find the project root by looking for a .octocode directory or .git directory.
/// Returns `None` if no root markers can be found.
#[must_use]
pub fn find_project_root_strict() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    find_project_root_strict_from(&cwd)
}

fn find_project_root_strict_from(cwd: &Path) -> Option<PathBuf> {
    let home_dir = crate::storage::user_home_dir();
    for ancestor in cwd.ancestors() {
        if has_project_root_marker(ancestor, home_dir.as_deref()) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn find_project_root_from(cwd: &Path) -> PathBuf {
    let home_dir = crate::storage::user_home_dir();
    for ancestor in cwd.ancestors() {
        if has_project_root_marker(ancestor, home_dir.as_deref()) {
            return ancestor.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

fn has_project_root_marker(path: &Path, home_dir: Option<&Path>) -> bool {
    if path.join(".git").exists() {
        return true;
    }

    path.join(".octocode").is_dir() && !home_dir.is_some_and(|home| same_path(path, home))
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn normalize_project_root(project_root: &Path) -> &Path {
    let is_config_dir = project_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(".octocode"));

    if is_config_dir {
        project_root.parent().unwrap_or(project_root)
    } else {
        project_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_load_normalizes_project_config_dir_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("local.toml"), "api_key = \"sk-local\"\n")
            .expect("write local config");

        let loaded = Config::load(Some(&config_dir)).expect("load config");

        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
    }

    #[test]
    fn provider_config_parses_from_project_config() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[provider]
default = "deepseek"
"#,
        )
        .expect("write provider config");

        let loaded = Config::load(Some(root.path())).expect("load config");

        assert_eq!(
            loaded.provider.default,
            crate::provider::ProviderKind::DeepSeek
        );
    }

    #[test]
    fn save_project_local_settings_preserves_secrets_out_of_local_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut config = Config {
            api_key: Some("sk-global-secret".to_string()),
            ..Config::default()
        };
        config.provider.default = ProviderKind::OpenRouter;
        config.ui.language = "en-US".to_string();
        config.ui.theme = "dark".to_string();
        config.policy.require_approval_for_command = false;
        config.mcp.enabled = true;
        config.subagent.max_parallel = 6;

        let path = config
            .save_project_local_settings(root.path())
            .expect("settings save should succeed");
        let content = std::fs::read_to_string(path).expect("read saved config");

        assert!(!content.contains("sk-global-secret"));
        assert!(content.contains("[provider]"));
        assert!(content.contains("default = \"openrouter\""));
        assert!(content.contains("[ui]"));
        assert!(content.contains("language = \"en-US\""));
        assert!(content.contains("theme = \"dark\""));
        assert!(content.contains("[mcp]"));
        assert!(content.contains("enabled = true"));
        assert!(content.contains("[subagent]"));
        assert!(content.contains("max_parallel = 6"));
    }

    #[test]
    fn provider_layer_merges_endpoint_overrides() {
        let base = Config {
            provider: ProviderConfig {
                openrouter: ProviderEndpointConfig {
                    base_url: Some("https://old.example/v1".to_string()),
                    pro_model: Some("old-pro".to_string()),
                    flash_model: None,
                },
                ..ProviderConfig::default()
            },
            ..Config::default()
        };
        let patch = parse_config_patch(
            r#"
[provider]
default = "openrouter"

[provider.openrouter]
flash_model = "deepseek/deepseek-v4-flash"
"#,
        )
        .expect("parse patch");

        let merged = base.merge_with_config_patch(patch);

        assert_eq!(merged.provider.default, ProviderKind::OpenRouter);
        assert_eq!(
            merged.provider.openrouter.base_url.as_deref(),
            Some("https://old.example/v1")
        );
        assert_eq!(
            merged.provider.openrouter.pro_model.as_deref(),
            Some("old-pro")
        );
        assert_eq!(
            merged.provider.openrouter.flash_model.as_deref(),
            Some("deepseek/deepseek-v4-flash")
        );
    }

    #[test]
    fn provider_layer_merges_new_chinese_provider_overrides() {
        let base = Config {
            provider: ProviderConfig {
                qianfan: ProviderEndpointConfig {
                    base_url: Some("https://qianfan.baidubce.com/v2".to_string()),
                    pro_model: Some("ernie-5.0-thinking-preview".to_string()),
                    flash_model: None,
                },
                ..ProviderConfig::default()
            },
            ..Config::default()
        };
        let patch = parse_config_patch(
            r#"
[provider]
default = "qianfan"

[provider.qianfan]
flash_model = "ernie-4.5-turbo-128k"
"#,
        )
        .expect("parse patch");

        let merged = base.merge_with_config_patch(patch);

        assert_eq!(merged.provider.default, ProviderKind::Qianfan);
        assert_eq!(
            merged.provider.qianfan.base_url.as_deref(),
            Some("https://qianfan.baidubce.com/v2")
        );
        assert_eq!(
            merged.provider.qianfan.pro_model.as_deref(),
            Some("ernie-5.0-thinking-preview")
        );
        assert_eq!(
            merged.provider.qianfan.flash_model.as_deref(),
            Some("ernie-4.5-turbo-128k")
        );
    }

    #[test]
    fn ui_config_defaults_to_subtle_motion() {
        assert_eq!(Config::default().ui.motion, "subtle");
    }

    #[test]
    fn example_config_parses_and_covers_settings_surface() {
        let config: Config = toml::from_str(include_str!("../../.octocode/config.toml.example"))
            .expect("example config parses");

        assert_eq!(config.provider.default.as_str(), "deepseek");
        assert_eq!(config.ui.theme, Config::default().ui.theme);
        assert_eq!(config.ui.motion, Config::default().ui.motion);
        assert_eq!(config.ui.renderer, Config::default().ui.renderer);
        assert_eq!(config.policy.autonomy_level, AutonomyLevel::Off);
        assert_eq!(
            config.subagent.max_parallel,
            Config::default().subagent.max_parallel
        );
    }

    #[test]
    fn ui_layer_only_overrides_declared_fields() {
        let base = UiConfig {
            theme: "dark".to_string(),
            motion: "off".to_string(),
            ..UiConfig::default()
        };
        let patch = parse_config_patch(
            r#"
[ui]
renderer = "fullscreen"
"#,
        )
        .expect("parse patch")
        .ui;

        let merged = base.merge_ui(patch);

        assert_eq!(merged.theme, "dark");
        assert_eq!(merged.motion, "off");
        assert_eq!(merged.renderer, "fullscreen");
    }

    #[test]
    fn layered_config_sections_only_override_declared_fields() {
        let base = Config {
            model: ModelConfig {
                default: DeepSeekModel::Pro,
                heavy: DeepSeekModel::Pro,
                allow_legacy_model_alias: true,
                ..ModelConfig::default()
            },
            execution: ExecutionConfig {
                default_lane: "custom_chat".into(),
                plan_lane: "custom_plan".into(),
                ..ExecutionConfig::default()
            },
            search: SearchConfig {
                include_sessions: false,
                include_git_diff: false,
                use_deepseek_rerank: false,
                ..SearchConfig::default()
            },
            cache: CacheConfig {
                stable_prefix_enabled: false,
                warn_on_low_cache_hit: false,
            },
            router: RouterConfig {
                enabled: false,
                conservative: false,
                simple_threshold: 10,
                ..RouterConfig::default()
            },
            subagent: SubagentConfig {
                enabled: false,
                max_parallel: 2,
                allow_custom_agents: true,
                ..SubagentConfig::default()
            },
            ..Config::default()
        };
        let patch = parse_config_patch(
            r#"
[model]
thinking_mode = "on"

[execution]
json_output_enabled = true

[search]
max_results = 7

[cache]
warn_on_low_cache_hit = true

[router]
confidence_threshold = 0.5

[subagent]
default_model = "deepseek-v4-pro"
"#,
        )
        .expect("parse patch");

        let merged = base.merge_with_config_patch(patch);

        assert_eq!(merged.model.default, DeepSeekModel::Pro);
        assert_eq!(merged.model.heavy, DeepSeekModel::Pro);
        assert!(merged.model.allow_legacy_model_alias);
        assert_eq!(merged.model.thinking_mode, ThinkingMode::On);
        assert_eq!(merged.execution.default_lane, "custom_chat");
        assert_eq!(merged.execution.plan_lane, "custom_plan");
        assert!(merged.execution.json_output_enabled);
        assert!(!merged.search.include_sessions);
        assert!(!merged.search.include_git_diff);
        assert!(!merged.search.use_deepseek_rerank);
        assert_eq!(merged.search.max_results, 7);
        assert!(!merged.cache.stable_prefix_enabled);
        assert!(merged.cache.warn_on_low_cache_hit);
        assert!(!merged.router.enabled);
        assert!(!merged.router.conservative);
        assert_eq!(merged.router.simple_threshold, 10);
        assert_eq!(merged.router.confidence_threshold, 0.5);
        assert!(!merged.subagent.enabled);
        assert_eq!(merged.subagent.max_parallel, 2);
        assert!(merged.subagent.allow_custom_agents);
        assert_eq!(merged.subagent.default_model, "deepseek-v4-pro");
    }

    #[test]
    fn profiles_layer_merge_preserves_unspecified_fields() {
        let mut profiles = std::collections::BTreeMap::new();
        profiles.insert(
            "default".to_string(),
            ProfileConfig {
                api_key: Some("sk-base".to_string()),
                model: Some(DeepSeekModel::Flash),
                thinking_mode: Some(ThinkingMode::On),
            },
        );
        let base = Config {
            profiles,
            ..Config::default()
        };
        let patch = parse_config_patch(
            r#"
[profiles.default]
model = "deepseek-v4-pro"
"#,
        )
        .expect("parse patch");

        let merged = base.merge_with_config_patch(patch);
        let profile = merged.profiles.get("default").expect("profile");

        assert_eq!(profile.api_key.as_deref(), Some("sk-base"));
        assert_eq!(profile.model, Some(DeepSeekModel::Pro));
        assert_eq!(profile.thinking_mode, Some(ThinkingMode::On));
    }

    #[test]
    fn hooks_layer_partial_override_preserves_unspecified_lists() {
        let base = Config {
            hooks: HooksConfig {
                user_prompt_submit: Vec::new(),
                pre_tool: vec!["pre".to_string()],
                post_tool: vec!["post".to_string()],
                session_start: Vec::new(),
                session_end: Vec::new(),
                stop: Vec::new(),
                turn_stop: Vec::new(),
                subagent_stop: Vec::new(),
                pre_compact: Vec::new(),
                notification: Vec::new(),
            },
            ..Config::default()
        };
        let patch = parse_config_patch(
            r#"
[hooks]
stop = ["stop"]
session_end = ["end"]
"#,
        )
        .expect("parse patch");

        let merged = base.merge_with_config_patch(patch);

        assert_eq!(merged.hooks.pre_tool, ["pre"]);
        assert_eq!(merged.hooks.post_tool, ["post"]);
        assert_eq!(merged.hooks.session_end, ["end"]);
        assert_eq!(merged.hooks.stop, ["stop"]);
    }

    #[test]
    fn mcp_layer_partial_server_override_does_not_require_command() {
        let mut servers = std::collections::BTreeMap::new();
        servers.insert(
            "filesystem".to_string(),
            McpServerEntryConfig {
                transport: crate::mcp::client::McpTransport::Stdio,
                command: Some("mcp-filesystem".to_string()),
                args: vec!["--root".to_string(), ".".to_string()],
                env: None,
                url: None,
                headers: None,
                include_tools: vec!["read".to_string()],
                exclude_tools: Vec::new(),
                trust: false,
                timeout_ms: 1_000,
            },
        );
        let base = Config {
            mcp: McpConfig {
                enabled: true,
                servers,
            },
            ..Config::default()
        };
        let patch = parse_config_patch(
            r#"
[mcp.servers.filesystem]
timeout_ms = 2000
"#,
        )
        .expect("parse patch");

        let merged = base.merge_with_config_patch(patch);
        let server = merged.mcp.servers.get("filesystem").expect("server");

        assert_eq!(server.transport, crate::mcp::client::McpTransport::Stdio);
        assert_eq!(server.command.as_deref(), Some("mcp-filesystem"));
        assert_eq!(server.args, ["--root", "."]);
        assert_eq!(server.include_tools, ["read"]);
        assert_eq!(server.timeout_ms, 2_000);
    }

    #[test]
    fn mcp_layer_adds_http_server_without_command() {
        let patch = parse_config_patch(
            r#"
[mcp]
enabled = true

[mcp.servers.remote]
transport = "http"
url = "https://mcp.example.com/rpc"
headers = { Authorization = "Bearer token" }
timeout_ms = 1500
"#,
        )
        .expect("parse patch");

        let merged = Config::default().merge_with_config_patch(patch);
        let server = merged.mcp.servers.get("remote").expect("server");

        assert_eq!(server.transport, crate::mcp::client::McpTransport::Http);
        assert_eq!(server.command, None);
        assert_eq!(server.url.as_deref(), Some("https://mcp.example.com/rpc"));
        assert_eq!(
            server
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization"))
                .map(String::as_str),
            Some("Bearer token")
        );
        assert_eq!(server.timeout_ms, 1_500);
    }

    #[test]
    fn find_project_root_handles_git_file_worktree() {
        let root = tempfile::tempdir().expect("tempdir");
        let child = root.path().join("src").join("nested");
        std::fs::create_dir_all(&child).expect("create child");
        std::fs::write(
            root.path().join(".git"),
            "gitdir: ../main/.git/worktrees/wt\n",
        )
        .expect("write git file");

        assert_eq!(find_project_root_from(&child), root.path());
    }

    #[test]
    fn find_project_root_strict_requires_marker() {
        let root = tempfile::tempdir().expect("tempdir");
        let child = root.path().join("child");
        std::fs::create_dir_all(&child).expect("create child");

        assert_eq!(find_project_root_strict_from(&child), None);

        std::fs::write(child.join(".octocode"), "").expect("create marker file");
        assert_eq!(find_project_root_strict_from(&child), None);

        std::fs::remove_file(child.join(".octocode")).expect("remove marker file");
        std::fs::create_dir(child.join(".octocode")).expect("create marker directory");
        assert_eq!(find_project_root_strict_from(&child), Some(child.clone()));
    }

    #[test]
    fn policy_layer_only_overrides_declared_fields() {
        let base = PolicyConfig {
            network_access: true,
            require_approval_for_write: true,
            require_approval_for_command: true,
            block_protected_paths: true,
            normalize_unicode_commands: true,
            command_timeout_seconds: 120,
            autonomy_level: AutonomyLevel::Off,
            ..PolicyConfig::default()
        };
        let patch = PartialPolicyConfig {
            command_timeout_seconds: Some(60),
            ..PartialPolicyConfig::default()
        };

        let merged = base.merge_policy(Some(patch));

        assert!(merged.network_access);
        assert!(merged.require_approval_for_write);
        assert!(merged.require_approval_for_command);
        assert!(merged.block_protected_paths);
        assert!(merged.normalize_unicode_commands);
        assert_eq!(merged.command_timeout_seconds, 60);
        assert_eq!(merged.autonomy_level, AutonomyLevel::Off);
    }

    #[test]
    fn parse_policy_patch_tracks_missing_fields() {
        let patch = parse_policy_patch(
            r#"
[policy]
command_timeout_seconds = 60
"#,
        )
        .expect("parse patch")
        .expect("policy patch");

        assert_eq!(patch.command_timeout_seconds, Some(60));
        assert_eq!(patch.network_access, None);
        assert_eq!(patch.require_approval_for_write, None);
    }

    #[test]
    fn policy_autonomy_level_parses_and_merges() {
        let patch = parse_policy_patch(
            r#"
[policy]
autonomy_level = "medium"
"#,
        )
        .expect("parse patch")
        .expect("policy patch");

        let merged = PolicyConfig::default().merge_policy(Some(patch));
        assert_eq!(merged.autonomy_level, AutonomyLevel::Medium);
        assert!(merged.autonomy_level.auto_local_commands());
        assert!(merged.autonomy_level.auto_workspace_writes());
    }

    #[test]
    fn mcp_server_config_parses_hardening_fields() {
        let config: McpConfig = toml::from_str(
            r#"
enabled = true

[servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
include_tools = ["read_file", "list_directory"]
exclude_tools = ["write_file"]
trust = true
timeout_ms = 2500
"#,
        )
        .expect("MCP config should deserialize");

        let server = config
            .servers
            .get("filesystem")
            .expect("filesystem server should exist");

        assert_eq!(server.transport, crate::mcp::client::McpTransport::Stdio);
        assert_eq!(server.command.as_deref(), Some("npx"));
        assert_eq!(server.include_tools, vec!["read_file", "list_directory"]);
        assert_eq!(server.exclude_tools, vec!["write_file"]);
        assert!(server.trust);
        assert_eq!(server.timeout_ms, 2500);
    }

    #[test]
    fn mcp_server_config_defaults_hardening_fields() {
        let config: McpConfig = toml::from_str(
            r#"
enabled = true

[servers.filesystem]
command = "npx"
"#,
        )
        .expect("minimal MCP config should deserialize");

        let server = config
            .servers
            .get("filesystem")
            .expect("filesystem server should exist");

        assert_eq!(server.transport, crate::mcp::client::McpTransport::Stdio);
        assert_eq!(server.command.as_deref(), Some("npx"));
        assert!(server.include_tools.is_empty());
        assert!(server.exclude_tools.is_empty());
        assert!(!server.trust);
        assert_eq!(
            server.timeout_ms,
            crate::mcp::client::DEFAULT_MCP_TIMEOUT_MS
        );
    }

    #[test]
    fn mcp_server_config_parses_http_and_sse_transports() {
        let config: McpConfig = toml::from_str(
            r#"
enabled = true

[servers.remote]
transport = "http"
url = "https://mcp.example.com/rpc"
headers = { Authorization = "Bearer token", "X-MCP" = "1" }

[servers.events]
transport = "sse"
url = "https://mcp.example.com/events"
"#,
        )
        .expect("MCP remote config should deserialize");

        let remote = config.servers.get("remote").expect("remote server");
        let events = config.servers.get("events").expect("events server");

        assert_eq!(remote.transport, crate::mcp::client::McpTransport::Http);
        assert_eq!(remote.command, None);
        assert_eq!(remote.url.as_deref(), Some("https://mcp.example.com/rpc"));
        assert_eq!(
            remote
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-MCP"))
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(events.transport, crate::mcp::client::McpTransport::Sse);
        assert_eq!(events.command, None);
        assert_eq!(
            events.url.as_deref(),
            Some("https://mcp.example.com/events")
        );
    }

    #[test]
    fn policy_command_sandbox_merges_nested_config() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[policy.command_sandbox]
mode = "strict"
network_access = true
readable_roots = ["/usr/local"]
writable_roots = ["/tmp/octocode"]
"#,
        )
        .expect("write config");

        let loaded = Config::load(Some(root.path())).expect("load config");

        assert_eq!(
            loaded.policy.command_sandbox.mode,
            CommandSandboxMode::Strict
        );
        assert!(loaded.policy.command_sandbox.network_access);
        assert_eq!(
            loaded.policy.command_sandbox.readable_roots,
            vec![PathBuf::from("/usr/local")]
        );
        assert_eq!(
            loaded.policy.command_sandbox.writable_roots,
            vec![PathBuf::from("/tmp/octocode")]
        );
    }
}

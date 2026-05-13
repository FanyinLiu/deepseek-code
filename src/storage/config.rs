use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::deepseek::{DeepSeekModel, ReasoningEffort, ThinkingMode};

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
    pub router: RouterConfig,
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
    pub show_cache_hud: bool,
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
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialConfig {
    api_key: Option<String>,
    model: Option<ModelConfig>,
    execution: Option<ExecutionConfig>,
    search: Option<SearchConfig>,
    cache: Option<CacheConfig>,
    policy: Option<PartialPolicyConfig>,
    paths: Option<PathsConfig>,
    ui: Option<UiConfig>,
    telemetry: Option<TelemetryConfig>,
    router: Option<RouterConfig>,
    profiles: Option<std::collections::BTreeMap<String, ProfileConfig>>,
    subagent: Option<SubagentConfig>,
    mcp: Option<McpConfig>,
    hooks: Option<HooksConfig>,
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
    pub pre_tool: Vec<String>,
    #[serde(default)]
    pub post_tool: Vec<String>,
    #[serde(default)]
    pub stop: Vec<String>,
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
            show_cache_hud: true,
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
    12000
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
fn default_renderer() -> String {
    "classic".into()
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
    /// 1. ~/.deepseek-code/config.toml (user-global)
    /// 2. ./.deepseek-code/config.toml (project, checked in)
    /// 3. ./.deepseek-code/local.toml (local override, gitignored)
    ///
    /// Later files override earlier ones.
    pub fn load(project_root: Option<&std::path::Path>) -> Result<Self, anyhow::Error> {
        let mut config = Config::default();

        // 1. User global
        if let Some(user_dir) = dirs::home_dir() {
            let global_path = user_dir.join(".deepseek-code").join("config.toml");
            if global_path.exists() {
                let content = std::fs::read_to_string(&global_path)?;
                let patch = parse_config_patch(&content)?;
                config = config.merge_with_config_patch(patch);
            }
        }

        // 2 & 3. Project-level
        if let Some(root) = project_root.map(normalize_project_root) {
            let project_config = root.join(".deepseek-code").join("config.toml");
            if project_config.exists() {
                let content = std::fs::read_to_string(&project_config)?;
                let patch = parse_config_patch(&content)?;
                config = config.merge_with_config_patch(patch);
            }

            let local_config = root.join(".deepseek-code").join("local.toml");
            if local_config.exists() {
                let content = std::fs::read_to_string(&local_config)?;
                let patch = parse_config_patch(&content)?;
                config = config.merge_with_config_patch(patch);
            }
        }

        Ok(config)
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
            router,
            profiles,
            subagent,
            mcp,
            hooks,
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
        let ui = match patch.ui {
            Some(next) => ui.merge_ui(next),
            None => ui,
        };
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
            model: patch.model.unwrap_or(model),
            execution: patch.execution.unwrap_or(execution),
            search,
            cache,
            policy: policy.merge_policy(patch.policy),
            paths,
            ui,
            telemetry,
            router,
            profiles: patch.profiles.unwrap_or(profiles),
            subagent,
            mcp: patch.mcp.unwrap_or(mcp),
            hooks: patch.hooks.unwrap_or(hooks),
        }
    }
}

fn parse_config_patch(content: &str) -> Result<PartialConfig, anyhow::Error> {
    Ok(toml::from_str(content)?)
}

#[cfg(test)]
fn parse_policy_patch(content: &str) -> Result<Option<PartialPolicyConfig>, anyhow::Error> {
    Ok(parse_config_patch(content)?.policy)
}

impl SearchConfig {
    fn merge_search(self, other: Self) -> Self {
        other
    }
}
impl CacheConfig {
    fn merge_cache(self, other: Self) -> Self {
        other
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
    fn merge_ui(self, other: Self) -> Self {
        other
    }
}
impl TelemetryConfig {
    fn merge_telemetry(self, other: Self) -> Self {
        other
    }
}
impl RouterConfig {
    fn merge_router(self, other: Self) -> Self {
        other
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
    fn merge_subagent(self, other: Self) -> Self {
        Self {
            enabled: other.enabled,
            swarm_enabled: other.swarm_enabled,
            max_parallel: other.max_parallel,
            auto_decompose: other.auto_decompose,
            write_requires_approval: other.write_requires_approval,
            command_requires_approval: other.command_requires_approval,
            default_model: other.default_model,
            allow_custom_agents: other.allow_custom_agents,
            custom_agents_dir: other.custom_agents_dir.or(self.custom_agents_dir),
        }
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
    false
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
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Option<std::collections::HashMap<String, String>>,
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

/// Find the project root by looking for a .deepseek-code directory or .git directory.
#[must_use]
pub fn find_project_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    for ancestor in cwd.ancestors() {
        if ancestor.join(".deepseek-code").is_dir() || ancestor.join(".git").is_dir() {
            return Some(ancestor.to_path_buf());
        }
    }
    Some(cwd)
}

pub(crate) fn normalize_project_root(project_root: &Path) -> &Path {
    let is_config_dir = project_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(".deepseek-code"));

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
        let config_dir = root.path().join(".deepseek-code");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("local.toml"), "api_key = \"sk-local\"\n")
            .expect("write local config");

        let loaded = Config::load(Some(&config_dir)).expect("load config");

        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
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

        assert!(server.include_tools.is_empty());
        assert!(server.exclude_tools.is_empty());
        assert!(!server.trust);
        assert_eq!(
            server.timeout_ms,
            crate::mcp::client::DEFAULT_MCP_TIMEOUT_MS
        );
    }
}

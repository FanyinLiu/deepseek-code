use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashMap, VecDeque},
    hash::{Hash, Hasher},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crossterm::{
    cursor::{position as cursor_position, Hide, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, Event as CEvent,
        KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    style::{Attribute, SetAttribute},
    terminal::{Clear, ClearType},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal, TerminalOptions, Viewport,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const PAGE_SCROLL_LINES: usize = 20;
const MOUSE_SCROLL_LINES: usize = 5;
const TUI_FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);
const COMPLETION_PANEL_MAX_HEIGHT: u16 = 9;
const SHELL_HINT_PANEL_HEIGHT: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputState {
    text_hash: u64,
    cursor_pos: usize,
    input_height: u16,
    api_key_entry: bool,
    pending_options: usize,
    history_search_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TranscriptState {
    messages_len: usize,
    last_message_hash: u64,
    pending_user_hash: u64,
    queued_inputs_len: usize,
    stream_hash: u64,
    reasoning_hash: u64,
    scroll_offset: usize,
    plan_hash: u64,
    subagents_len: usize,
    subagents_hash: u64,
    swarm_hash: u64,
    todo_hash: u64,
    diffs_len: usize,
    diff_hash: u64,
    settings_open: bool,
    showing_welcome: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusState {
    render_epoch: u64,
    streaming_bucket: u64,
    idle_seconds: u64,
    notice_hash: u64,
    status_hash: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderDirtyState {
    input: InputState,
    transcript: TranscriptState,
    status: StatusState,
}

struct RuntimeRenderState<'a> {
    visible_subagents: &'a [subagent_cards::SubagentCard],
    elapsed_ms: u64,
}

#[derive(Debug, Clone, Default)]
struct RenderOptionState {
    slash_suggestions: Vec<(String, String)>,
    file_mention_suggestions: Vec<String>,
    history_options: Vec<String>,
    shell_hint_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderDirtyFlags {
    input: bool,
    transcript: bool,
    status: bool,
}

impl RenderDirtyState {
    fn diff(self, previous: Self) -> RenderDirtyFlags {
        RenderDirtyFlags {
            input: self.input != previous.input,
            transcript: self.transcript != previous.transcript,
            status: self.status != previous.status,
        }
    }
}

impl RenderDirtyFlags {
    fn any(self) -> bool {
        self.input || self.transcript || self.status
    }
}

fn stable_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn role_hash_tag(role: &Role) -> u8 {
    match role {
        Role::System => 0,
        Role::User => 1,
        Role::Assistant => 2,
        Role::Tool => 3,
    }
}

fn message_content_hash(content: &MessageContent) -> u64 {
    let mut hasher = DefaultHasher::new();
    match content {
        MessageContent::Text(text) => {
            0u8.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        MessageContent::MultiPart(parts) => {
            1u8.hash(&mut hasher);
            for part in parts {
                part.text.as_deref().hash(&mut hasher);
            }
        }
        MessageContent::None => {
            2u8.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn animated_token_count(target: u64, elapsed_ms: u64) -> u64 {
    if target == 0 {
        return 0;
    }
    let duration_ms = target.saturating_mul(2).clamp(12_000, 45_000);
    if elapsed_ms >= duration_ms {
        return target;
    }
    let frame_ms = elapsed_ms.saturating_add(80);
    let shown = target.saturating_mul(frame_ms).saturating_div(duration_ms);
    shown.clamp(1, target)
}

use crate::agent::orchestrator::{AgentEvent, DecisionKind, Orchestrator};
use crate::deepseek::{
    CacheUsage, ChatMessage, ChatMessageContent, ChatRequest, DeepSeekModel, MessageContent,
    MessageVisibility, ProtocolMessage, ReasoningEffort, ReasoningState, Role, Session, SessionId,
    SessionMetadata, ThinkingConfig, ThinkingMode, ToolCall, ToolCallFunction,
};
use crate::policy::ApprovalDisplay;
use crate::provider::{
    build_provider, context_budget_for, request_model_name_for_config, Provider, ProviderKind,
};
use crate::storage;

use super::{
    approval_popup, diff_viewer, file_tree, input, keybindings, layout, model_hint, motion,
    plan_tracker, render_core, screens, select_popup, settings_panel, status_bar, statusline,
    subagent_cards, theme, transcript_view, welcome,
};
use render_core::{render_canvas, render_jump_to_bottom_hint};

type ApprovalRequest = (ApprovalDisplay, tokio::sync::oneshot::Sender<bool>);

/// TUI application state.
pub struct TuiApp {
    pub input_text: String,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
    pub status_message: String,
    pub messages: Vec<ProtocolMessage>,
    pub model: DeepSeekModel,
    pub thinking_mode: ThinkingMode,
    /// Session-scoped approval mode (Claude Code-style). Mirrored to the
    /// orchestrator via Submit handler diff detection, mirroring how
    /// `model` is synced. `/mode` slash command reads and writes this.
    pub permission_mode: crate::policy::PermissionMode,
    pub cache: Option<CacheUsage>,
    pub total_tokens: u64,
    pub current_turn_tokens: u64,
    pub current_turn_input_tokens: u64,
    pub current_turn_output_tokens: u64,
    current_turn_usage_finalized: bool,
    input_token_animation_started: Option<std::time::Instant>,
    pub total_cost: f64,
    pub session_id: Option<SessionId>,
    pub session_name: Option<String>,
    pub welcome: welcome::WelcomeDashboardData,
    api_key_state: ApiKeyState,
    api_key_entry: Option<ApiKeyEntry>,
    pub activity_log: Vec<String>,
    pub current_task_title: String,
    pub pending_user_message: Option<String>,
    pub queued_inputs: VecDeque<String>,
    /// Set by custom slash commands whose frontmatter declares
    /// `allowed-tools:`. The TUI submit handler hands this off to the
    /// orchestrator when the queued prompt is next consumed, so the
    /// per-command allowlist applies only to that one turn.
    pub pending_allowed_tools: Option<Vec<String>>,
    pending_side_outputs: VecDeque<String>,
    pub stream_buffer: String,
    pub is_streaming: bool,
    pub stream_start: Option<std::time::Instant>,
    pub approval: Option<ApprovalRequest>,
    approval_queue: VecDeque<ApprovalRequest>,
    approval_selected_index: usize,
    pub session_auto_approve: bool,
    pub interaction_mode: InteractionMode,
    pub running: bool,
    exit_confirm_pending: bool,
    pub plan_steps: Vec<plan_tracker::PlanStepItem>,
    pub plan_current_step: usize,
    pub plan_total_steps: usize,
    pub plan_summary: Option<String>,
    pub plan_warnings: Vec<String>,
    pub subagents: Vec<subagent_cards::SubagentCard>,
    pub active_swarm: Option<SwarmViewState>,
    pub file_diffs: Vec<diff_viewer::FileDiffItem>,
    pub options_needed: Option<DecisionPrompt>,
    pub pending_options: Option<(String, Vec<String>)>,
    /// Rich payload paralleling `pending_options` when the ask_user tool
    /// supplied per-option descriptions, previews, or asked for multi-select.
    /// Present alongside `pending_options`; absent when the legacy text-only
    /// fallback applies.
    pub pending_question_rich: Option<PendingQuestionRich>,
    pub todo_summary: crate::tools::todo_state::TodoSummary,
    pub todo_items: Vec<crate::tools::todo_state::TodoBoardItem>,
    pub recent_tool_summaries: VecDeque<String>,
    pub last_user_question_summary: Option<String>,
    pub latest_compact_summary: Option<String>,
    pub latest_compact_reason: Option<String>,
    compact_notice: Option<String>,
    pub selected_option_index: usize,
    pub selected_multi_options: Vec<bool>,
    pub selected_slash_index: usize,
    pub selected_file_mention_index: usize,
    pub pending_images: Vec<String>,
    pub reasoning_buffer: String,
    pub current_turn_reasoning_tokens: u64,
    pub show_reasoning: bool,
    pub input_history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub draft_input: String,
    pub history_search_active: bool,
    pub history_search_draft: String,
    // Diff viewer interaction
    pub selected_diff: Option<usize>,
    pub diff_scroll: usize,
    pub diff_focused: bool,
    // File tree sidebar
    pub file_tree: file_tree::FileTree,
    pub show_file_tree: bool,
    pub file_tree_focused: bool,
    pub mcp_status: String,
    pub config: storage::Config,
    pub theme_mode: theme::ThemeMode,
    pub motion_level: motion::MotionLevel,
    ui_started_at: std::time::Instant,
    pub renderer_mode: RendererMode,
    pub settings_open: bool,
    pub settings_tab: settings_panel::SettingsTab,
    pub settings_selected: usize,
    slash_suggestion_cache: RefCell<SlashSuggestionCache>,
    slash_command_registry_cache: RefCell<SlashCommandRegistryCache>,
    keymap: keybindings::Keymap,
}

#[derive(Default)]
struct SlashSuggestionCache {
    prefix: String,
    language: String,
    registry_generation: u64,
    suggestions: Vec<(String, String)>,
}

impl SlashSuggestionCache {
    fn clear(&mut self) {
        self.prefix.clear();
        self.language.clear();
        self.registry_generation = 0;
        self.suggestions.clear();
    }
}

struct SlashCommandRegistryCache {
    root: PathBuf,
    fingerprint: Vec<PromptCommandFileFingerprint>,
    generation: u64,
    registry: crate::commands::CommandRegistry,
}

impl Default for SlashCommandRegistryCache {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            fingerprint: Vec::new(),
            generation: 0,
            registry: crate::commands::CommandRegistry::new(),
        }
    }
}

impl SlashCommandRegistryCache {
    fn refresh_for_root(&mut self, root: &Path) -> u64 {
        let fingerprint = prompt_command_file_fingerprint(root);
        if self.root != root || self.fingerprint != fingerprint {
            let mut registry = crate::commands::CommandRegistry::new();
            registry.load_prompt_commands(root);
            self.root = root.to_path_buf();
            self.fingerprint = fingerprint;
            self.registry = registry;
            self.generation = self.generation.saturating_add(1);
        }
        self.generation
    }

    fn registry(&self) -> &crate::commands::CommandRegistry {
        &self.registry
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PromptCommandFileFingerprint {
    path: PathBuf,
    len: u64,
    modified_nanos: u128,
}

fn prompt_command_file_fingerprint(project_root: &Path) -> Vec<PromptCommandFileFingerprint> {
    let mut files = Vec::new();
    if let Some(home) = crate::storage::user_home_dir() {
        collect_prompt_command_fingerprint(&home.join(".octocode").join("commands"), &mut files);
    }
    collect_prompt_command_fingerprint(
        &project_root.join(".octocode").join("commands"),
        &mut files,
    );
    files.sort();
    files
}

fn collect_prompt_command_fingerprint(dir: &Path, files: &mut Vec<PromptCommandFileFingerprint>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_prompt_command_fingerprint(&path, files);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        files.push(PromptCommandFileFingerprint {
            path,
            len: metadata.len(),
            modified_nanos,
        });
    }
}

pub struct DecisionPrompt {
    pub kind: DecisionKind,
    pub title: String,
    pub options: Vec<String>,
    pub respond: tokio::sync::oneshot::Sender<usize>,
}

#[derive(Debug, Clone)]
pub struct PendingQuestionRich {
    pub title: String,
    pub question: String,
    pub options: Vec<PendingQuestionOption>,
    pub multi_select: bool,
}

#[derive(Debug, Clone)]
pub struct PendingQuestionOption {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SwarmViewState {
    pub run_id: String,
    pub summary: String,
    pub total: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub status: String,
    pub cancel_requested: bool,
    pub detail_expanded: bool,
    task_statuses: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionMode {
    Ask,
    Plan,
    AutoReview,
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererMode {
    Classic,
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalAction {
    ApproveOnce,
    ApproveSession,
    Deny,
}

impl RendererMode {
    #[must_use]
    pub fn from_config(value: &str) -> Self {
        Self::from_config_for_environment(value, &TerminalEnvironment::current())
    }

    #[must_use]
    fn from_config_for_environment(value: &str, _env: &TerminalEnvironment) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            // Auto now prefers the inline viewport so the transcript stays in
            // the terminal's native scrollback. Users who
            // explicitly want the legacy full-screen mode can still ask for it.
            "" | "auto" | "default" => Self::Classic,
            "fullscreen" | "full-screen" | "alternate" | "alt" => Self::Fullscreen,
            _ => Self::Classic,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Fullscreen => "fullscreen",
        }
    }

    #[must_use]
    fn uses_inline_viewport(self) -> bool {
        self == Self::Classic
    }
}

impl InteractionMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Plan => "Plan",
            Self::AutoReview => "Auto review",
            Self::FullAccess => "Full access",
        }
    }

    #[must_use]
    fn next(self) -> Self {
        match self {
            Self::Ask => Self::Plan,
            Self::Plan => Self::AutoReview,
            Self::AutoReview => Self::FullAccess,
            Self::FullAccess => Self::Ask,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ApiKeyEntry {
    pending_prompt: Option<String>,
    saving: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiKeyState {
    Missing,
    Entering,
    Saving,
    Ready,
    Error,
}

impl ApiKeyState {
    fn from_welcome(status: &str) -> Self {
        if status == "ready" {
            Self::Ready
        } else {
            Self::Missing
        }
    }

    fn welcome_status(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing | Self::Entering | Self::Saving | Self::Error => "missing",
        }
    }

    fn is_ready(self) -> bool {
        self == Self::Ready
    }
}

struct TuiStartupData {
    config: storage::Config,
    config_loaded: bool,
    config_load_error: Option<String>,
    api_key_available: bool,
    probe_keyring: bool,
}

impl TuiStartupData {
    fn load(root: &Path) -> (Self, Option<String>) {
        let config_result = storage::Config::load(Some(root));
        let (config_loaded, config_load_error, config) = match config_result {
            Ok(cfg) => (true, None, cfg),
            Err(err) => (false, Some(err.to_string()), storage::Config::default()),
        };
        let provider = config.provider.default;
        let api_key = storage::get_api_key_without_keyring_for_provider(
            provider,
            storage::config_api_key(&config),
        );
        let api_key_available = api_key.is_some();
        let probe_keyring =
            !api_key_available && storage::get_env_api_key_for_provider(provider).is_none();

        (
            Self {
                config,
                config_loaded,
                config_load_error,
                api_key_available,
                probe_keyring,
            },
            api_key,
        )
    }

    fn preview(api_key_available: bool) -> Self {
        Self {
            config: storage::Config::default(),
            config_loaded: false,
            config_load_error: None,
            api_key_available,
            probe_keyring: false,
        }
    }
}

fn load_welcome_with_startup(
    root: &Path,
    model: DeepSeekModel,
    thinking: ThinkingMode,
    config: &storage::Config,
    config_loaded: bool,
    api_key_available: bool,
) -> welcome::WelcomeDashboardData {
    let cache_status = if config_loaded {
        if config.ui.show_cache_hud {
            "no turn yet"
        } else {
            "disabled"
        }
    } else {
        "unknown"
    };
    let recent_sessions = dirs::home_dir()
        .map(|home| storage::SessionStore::new(home.join(".octocode")).list(root))
        .and_then(Result::ok)
        .unwrap_or_default()
        .into_iter()
        .take(3)
        .map(|session| {
            let id = session.id.to_string();
            welcome::RecentSessionItem {
                label: session.name.unwrap_or_else(|| id.chars().take(8).collect()),
                updated_at: session.updated_at.format("%m-%d %H:%M").to_string(),
                message_count: session.message_count,
                tool_call_count: session.tool_call_count,
            }
        })
        .collect();

    welcome::WelcomeDashboardData {
        workspace_name: root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                if name == concat!("deepseek", "-", "code") {
                    "octocode"
                } else {
                    name
                }
            })
            .unwrap_or("workspace")
            .to_string(),
        workspace_path: root.to_path_buf(),
        model,
        thinking,
        api_key_status: if api_key_available {
            "ready"
        } else {
            "missing"
        },
        config_status: if config_loaded { "loaded" } else { "fallback" },
        cache_status,
        recent_sessions,
        skills: welcome_skill_items(),
        mcp_servers: welcome_mcp_servers(config),
        agents_md: welcome_agents_md(root),
        detected_language: detect_project_language(root),
        display_language: config.ui.language.clone(),
    }
}

fn welcome_skill_items() -> Vec<welcome::SkillItem> {
    vec![
        welcome::SkillItem {
            name: "read_file",
            description: "Read & explore files",
            available: true,
        },
        welcome::SkillItem {
            name: "edit_file",
            description: "Edit & patch code",
            available: true,
        },
        welcome::SkillItem {
            name: "write_file",
            description: "Create new files",
            available: true,
        },
        welcome::SkillItem {
            name: "search_code",
            description: "Search codebase",
            available: true,
        },
        welcome::SkillItem {
            name: "run_command",
            description: "Execute shell commands",
            available: true,
        },
        welcome::SkillItem {
            name: "git_workflow",
            description: "Git add, commit, diff",
            available: true,
        },
        welcome::SkillItem {
            name: "web_search",
            description: "DuckDuckGo search",
            available: true,
        },
        welcome::SkillItem {
            name: "github_pr",
            description: "GitHub PR ops",
            available: std::env::var("GITHUB_TOKEN").is_ok(),
        },
        welcome::SkillItem {
            name: "semantic_search",
            description: "TF-IDF code search",
            available: true,
        },
        welcome::SkillItem {
            name: "fetch_url",
            description: "Fetch web content",
            available: true,
        },
        welcome::SkillItem {
            name: "image_input",
            description: "Multimodal images",
            available: true,
        },
        welcome::SkillItem {
            name: "lsp",
            description: "LSP hover/definition",
            available: true,
        },
        welcome::SkillItem {
            name: "subagent",
            description: "Parallel subagents",
            available: true,
        },
        welcome::SkillItem {
            name: "mcp",
            description: "MCP external tools",
            available: false,
        },
    ]
}

fn welcome_mcp_servers(config: &storage::Config) -> Vec<welcome::McpServerItem> {
    if !config.mcp.enabled {
        return Vec::new();
    }
    config
        .mcp
        .servers
        .keys()
        .map(|name| welcome::McpServerItem {
            name: name.clone(),
            status: welcome::McpServerStatus::NotConfigured,
            tool_count: 0,
            error: None,
        })
        .collect()
}

fn welcome_agents_md(root: &Path) -> welcome::AgentsMdInfo {
    let path = root.join("AGENTS.md");
    if !path.exists() {
        return welcome::AgentsMdInfo {
            loaded: false,
            rule_count: 0,
            summary: "No AGENTS.md found in project root.".to_string(),
        };
    }

    match crate::storage::read_text_file_capped(&path) {
        Ok(content) => {
            let rule_count = content
                .lines()
                .filter(|line| {
                    line.starts_with("## ") || line.starts_with("- ") || line.starts_with("* ")
                })
                .count();
            let summary = content
                .lines()
                .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .map(|line| {
                    let trimmed = line.trim();
                    truncate_chars(trimmed, 60)
                })
                .unwrap_or_else(|| "Project agent preferences loaded.".to_string());
            welcome::AgentsMdInfo {
                loaded: true,
                rule_count,
                summary,
            }
        }
        Err(_) => welcome::AgentsMdInfo {
            loaded: false,
            rule_count: 0,
            summary: "Failed to read AGENTS.md.".to_string(),
        },
    }
}

fn detect_project_language(root: &Path) -> String {
    const MARKERS: &[(&str, &str)] = &[
        ("Cargo.toml", "Rust"),
        ("package.json", "Node/TypeScript"),
        ("pyproject.toml", "Python"),
        ("go.mod", "Go"),
        ("pom.xml", "Java"),
    ];
    for (marker, language) in MARKERS {
        if root.join(marker).exists() {
            return (*language).to_string();
        }
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if let Some(ext) = entry.path().extension() {
                if matches!(
                    ext.to_string_lossy().as_ref(),
                    "rs" | "py" | "js" | "go" | "ts" | "java" | "cpp" | "c"
                ) {
                    return "Mixed".to_string();
                }
            }
        }
    }
    "Unknown".to_string()
}

impl TuiApp {
    #[must_use]
    pub fn new(
        model: DeepSeekModel,
        thinking_mode: ThinkingMode,
        session_name: Option<String>,
        project_root: PathBuf,
    ) -> Self {
        let (startup, _) = TuiStartupData::load(&project_root);
        Self::new_with_startup(model, thinking_mode, session_name, project_root, startup)
    }

    fn new_with_startup(
        model: DeepSeekModel,
        thinking_mode: ThinkingMode,
        session_name: Option<String>,
        project_root: PathBuf,
        startup: TuiStartupData,
    ) -> Self {
        let theme_mode = theme::ThemeMode::from_config(&startup.config.ui.theme);
        let motion_level = motion::MotionLevel::from_config(&startup.config.ui.motion);
        let renderer_mode = RendererMode::from_config(&startup.config.ui.renderer);
        theme::set_active_theme(theme_mode);
        let welcome = load_welcome_with_startup(
            &project_root,
            model.clone(),
            thinking_mode.clone(),
            &startup.config,
            startup.config_loaded,
            startup.api_key_available,
        );

        let api_key_state = ApiKeyState::from_welcome(welcome.api_key_status);
        let api_key_missing = !api_key_state.is_ready();
        let config_load_error = startup.config_load_error.clone();
        let initial_activity = if let Some(err) = &config_load_error {
            vec![format!("config load failed (using defaults): {err}")]
        } else {
            Vec::new()
        };
        let todo_board = crate::tools::todo_state::load_todo_board(&project_root);
        let keymap = keybindings::Keymap::load_project(&project_root);

        Self {
            input_text: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            status_message: if let Some(err) = &config_load_error {
                format!("config error — using defaults: {err}")
            } else if api_key_missing {
                "Enter your provider API key to start".into()
            } else {
                "Ready".into()
            },
            messages: Vec::new(),
            model,
            thinking_mode,
            permission_mode: crate::policy::PermissionMode::Default,
            cache: None,
            total_tokens: 0,
            current_turn_tokens: 0,
            current_turn_input_tokens: 0,
            current_turn_output_tokens: 0,
            current_turn_usage_finalized: false,
            input_token_animation_started: None,
            total_cost: 0.0,
            session_id: None,
            session_name,
            welcome,
            api_key_state: if api_key_missing {
                ApiKeyState::Entering
            } else {
                api_key_state
            },
            api_key_entry: api_key_missing.then(ApiKeyEntry::default),
            activity_log: initial_activity,
            current_task_title: String::new(),
            pending_user_message: None,
            queued_inputs: VecDeque::new(),
            pending_allowed_tools: None,
            pending_side_outputs: VecDeque::new(),
            stream_buffer: String::new(),
            is_streaming: false,
            stream_start: None,
            approval: None,
            approval_queue: VecDeque::new(),
            approval_selected_index: 0,
            session_auto_approve: false,
            interaction_mode: InteractionMode::Ask,
            running: true,
            exit_confirm_pending: false,
            plan_steps: Vec::new(),
            plan_current_step: 0,
            plan_total_steps: 0,
            plan_summary: None,
            plan_warnings: Vec::new(),
            subagents: Vec::new(),
            active_swarm: None,
            file_diffs: Vec::new(),
            options_needed: None,
            pending_options: None,
            pending_question_rich: None,
            todo_summary: todo_board.summary,
            todo_items: todo_board.items,
            recent_tool_summaries: VecDeque::new(),
            last_user_question_summary: None,
            latest_compact_summary: None,
            latest_compact_reason: None,
            compact_notice: None,
            selected_option_index: 0,
            selected_multi_options: Vec::new(),
            selected_slash_index: 0,
            selected_file_mention_index: 0,
            pending_images: Vec::new(),
            reasoning_buffer: String::new(),
            current_turn_reasoning_tokens: 0,
            show_reasoning: false,
            input_history: crate::storage::input_history::load_history(),
            history_cursor: None,
            draft_input: String::new(),
            history_search_active: false,
            history_search_draft: String::new(),
            selected_diff: None,
            diff_scroll: 0,
            diff_focused: false,
            file_tree: file_tree::FileTree::new(project_root.clone()),
            show_file_tree: false,
            file_tree_focused: false,
            mcp_status: String::new(),
            config: startup.config.clone(),
            theme_mode,
            motion_level,
            ui_started_at: std::time::Instant::now(),
            renderer_mode,
            settings_open: false,
            settings_tab: settings_panel::SettingsTab::Model,
            settings_selected: 0,
            slash_suggestion_cache: RefCell::default(),
            slash_command_registry_cache: RefCell::default(),
            keymap,
        }
    }

    /// Derive the current app mode from live state for the status bar.
    fn current_mode(&self) -> status_bar::AppMode {
        let has_running_plan =
            !self.plan_steps.is_empty() && self.plan_current_step < self.plan_total_steps;
        let has_running_agents = self.subagents.iter().any(|c| c.status.is_active());

        if self.interaction_mode == InteractionMode::Plan || has_running_plan {
            status_bar::AppMode::Plan
        } else if self.interaction_mode == InteractionMode::AutoReview {
            status_bar::AppMode::Review
        } else if self.interaction_mode == InteractionMode::FullAccess
            || has_running_agents
            || self.is_streaming
        {
            status_bar::AppMode::Run
        } else {
            status_bar::AppMode::Chat
        }
    }

    fn plan_execution_has_started(&self) -> bool {
        self.plan_steps.iter().any(|step| {
            matches!(
                step.status,
                plan_tracker::PlanStepStatus::Running
                    | plan_tracker::PlanStepStatus::Done
                    | plan_tracker::PlanStepStatus::Failed
            )
        })
    }

    pub fn set_interaction_mode(&mut self, mode: InteractionMode) {
        self.interaction_mode = mode;
        self.session_auto_approve = mode == InteractionMode::FullAccess;
        // Mirror the 4-mode TUI selector into the 6-mode approval pipeline.
        // The Submit handler diffs `permission_mode` and forwards changes to
        // the orchestrator, so `/mode plan` actually starts blocking
        // mutating tool calls instead of just relabelling the status line.
        self.permission_mode = match mode {
            InteractionMode::Ask => crate::policy::PermissionMode::Default,
            InteractionMode::Plan => crate::policy::PermissionMode::Plan,
            InteractionMode::AutoReview => crate::policy::PermissionMode::AcceptEdits,
            InteractionMode::FullAccess => crate::policy::PermissionMode::Bypass,
        };
        self.status_message = format!("Mode: {}", mode.label());
        self.push_activity(format!("mode: {}", mode.label()));
    }

    fn cycle_interaction_mode(&mut self) {
        self.set_interaction_mode(self.interaction_mode.next());
    }

    pub fn set_theme_mode(&mut self, mode: theme::ThemeMode) {
        self.theme_mode = mode;
        self.config.ui.theme = mode.label().to_string();
        theme::set_active_theme(mode);
        self.status_message = format!("Theme set to {}", mode.label());
        self.push_activity(format!("theme: {}", mode.label()));
    }

    fn motion_frame(&self) -> motion::MotionFrame {
        motion::MotionFrame::new(
            self.motion_level,
            self.ui_started_at.elapsed().as_millis() as u64,
        )
    }

    fn stream_motion_frame(&self) -> motion::MotionFrame {
        motion::MotionFrame::new(
            self.motion_level,
            self.stream_start.map_or_else(
                || self.ui_started_at.elapsed().as_millis() as u64,
                |started| started.elapsed().as_millis() as u64,
            ),
        )
    }

    fn render_epoch(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        let mode_tag = match self.current_mode() {
            status_bar::AppMode::Chat => 0u8,
            status_bar::AppMode::Plan => 1,
            status_bar::AppMode::Run => 2,
            status_bar::AppMode::Review => 3,
        };
        let interaction_tag = match self.interaction_mode {
            InteractionMode::Ask => 0u8,
            InteractionMode::Plan => 1,
            InteractionMode::AutoReview => 2,
            InteractionMode::FullAccess => 3,
        };
        let model_tag = match self.model {
            DeepSeekModel::Pro => 0u8,
            DeepSeekModel::Flash => 1,
            DeepSeekModel::LegacyChat => 2,
            DeepSeekModel::LegacyReasoner => 3,
        };
        let thinking_tag = match self.thinking_mode {
            ThinkingMode::Auto => 0u8,
            ThinkingMode::On => 1,
            ThinkingMode::Off => 2,
        };
        let renderer_tag = match self.renderer_mode {
            RendererMode::Classic => 0u8,
            RendererMode::Fullscreen => 1,
        };
        let api_key_state_tag = match self.api_key_state {
            ApiKeyState::Missing => 0u8,
            ApiKeyState::Entering => 1,
            ApiKeyState::Saving => 2,
            ApiKeyState::Ready => 3,
            ApiKeyState::Error => 4,
        };

        mode_tag.hash(&mut hasher);
        interaction_tag.hash(&mut hasher);
        model_tag.hash(&mut hasher);
        thinking_tag.hash(&mut hasher);
        renderer_tag.hash(&mut hasher);
        api_key_state_tag.hash(&mut hasher);
        self.motion_level.hash(&mut hasher);
        self.is_streaming.hash(&mut hasher);
        self.messages.len().hash(&mut hasher);
        self.stream_buffer.len().hash(&mut hasher);
        self.reasoning_buffer.len().hash(&mut hasher);
        self.pending_user_message.is_some().hash(&mut hasher);
        self.pending_side_outputs.len().hash(&mut hasher);
        self.scroll_offset.hash(&mut hasher);
        self.status_message.len().hash(&mut hasher);
        self.current_task_title.hash(&mut hasher);
        self.current_turn_tokens.hash(&mut hasher);
        self.current_turn_input_tokens.hash(&mut hasher);
        self.current_turn_output_tokens.hash(&mut hasher);
        self.current_turn_usage_finalized.hash(&mut hasher);
        self.total_tokens.hash(&mut hasher);
        self.total_cost.to_bits().hash(&mut hasher);
        self.input_text.len().hash(&mut hasher);
        self.cursor_pos.hash(&mut hasher);
        self.plan_steps.len().hash(&mut hasher);
        plan_steps_hash(&self.plan_steps).hash(&mut hasher);
        self.plan_current_step.hash(&mut hasher);
        self.plan_total_steps.hash(&mut hasher);
        self.subagents.len().hash(&mut hasher);
        subagents_hash(&self.subagents).hash(&mut hasher);
        self.options_needed.is_some().hash(&mut hasher);
        self.pending_options
            .as_ref()
            .map(|(_, options)| options.len())
            .hash(&mut hasher);
        self.history_search_active.hash(&mut hasher);
        self.status_notice().hash(&mut hasher);
        self.todo_summary.total.hash(&mut hasher);
        self.todo_summary.pending.hash(&mut hasher);
        self.todo_summary.in_progress.hash(&mut hasher);
        self.todo_summary.completed.hash(&mut hasher);
        self.todo_summary.cancelled.hash(&mut hasher);
        self.todo_summary.active.is_some().hash(&mut hasher);
        self.todo_items.hash(&mut hasher);
        self.file_diffs.len().hash(&mut hasher);
        self.queued_inputs.len().hash(&mut hasher);
        self.show_file_tree.hash(&mut hasher);
        self.file_tree.nodes.len().hash(&mut hasher);
        self.file_tree.selected.hash(&mut hasher);
        self.file_tree.scroll_offset.hash(&mut hasher);
        self.settings_open.hash(&mut hasher);
        self.settings_selected.hash(&mut hasher);
        match self.settings_tab {
            settings_panel::SettingsTab::Model => 0u8,
            settings_panel::SettingsTab::Safety => 1,
            settings_panel::SettingsTab::Interface => 2,
            settings_panel::SettingsTab::Agents => 3,
        }
        .hash(&mut hasher);

        self.exit_confirm_pending.hash(&mut hasher);
        self.stream_start.is_some().hash(&mut hasher);
        self.current_turn_reasoning_tokens.hash(&mut hasher);
        self.is_chinese_ui().hash(&mut hasher);
        self.welcome.workspace_name.len().hash(&mut hasher);
        active_swarm_hash(self.active_swarm.as_ref()).hash(&mut hasher);

        hasher.finish()
    }

    fn render_dirty_state(&self) -> RenderDirtyState {
        RenderDirtyState {
            input: self.input_state(),
            transcript: self.transcript_state(),
            status: self.status_state(),
        }
    }

    fn runtime_render_state(&self) -> RuntimeRenderState<'_> {
        let visible_subagents: &[subagent_cards::SubagentCard] = if self
            .active_swarm
            .as_ref()
            .is_some_and(|swarm| !swarm.detail_expanded)
        {
            &[]
        } else {
            &self.subagents
        };
        RuntimeRenderState {
            visible_subagents,
            elapsed_ms: self.stream_motion_frame().elapsed_ms,
        }
    }

    fn render_option_state(&self) -> RenderOptionState {
        let slash_suggestions = if self.history_search_active {
            Vec::new()
        } else {
            self.slash_command_suggestions()
        };
        let file_mention_suggestions = if self.history_search_active {
            Vec::new()
        } else {
            self.file_mention_suggestions()
        };
        RenderOptionState {
            slash_suggestions,
            file_mention_suggestions,
            history_options: self.history_search_options(),
            shell_hint_active: self.shell_hint_active(),
        }
    }

    fn render_option_count(&self, state: &RenderOptionState) -> usize {
        self.options_needed
            .as_ref()
            .map(|decision| decision.options.len())
            .or_else(|| (!state.history_options.is_empty()).then_some(state.history_options.len()))
            .or_else(|| self.pending_options.as_ref().map(|(_, opts)| opts.len()))
            .or_else(|| state.shell_hint_active.then_some(3))
            .or_else(|| {
                (!state.slash_suggestions.is_empty()).then_some(state.slash_suggestions.len())
            })
            .or_else(|| {
                (!state.file_mention_suggestions.is_empty())
                    .then_some(state.file_mention_suggestions.len())
            })
            .unwrap_or(0)
    }

    fn render_option_height(&self, state: &RenderOptionState, chrome: usize, max: u16) -> u16 {
        let count = self.render_option_count(state);
        if count == 0 {
            return 0;
        }

        if self.options_needed.is_some() || self.pending_options.is_some() {
            return (count + chrome).min(max as usize) as u16;
        }

        if state.shell_hint_active {
            return SHELL_HINT_PANEL_HEIGHT.min(max);
        }

        let max = max.min(COMPLETION_PANEL_MAX_HEIGHT);
        (count + 2).min(max as usize) as u16
    }

    fn input_pending_options<'a>(&'a self, state: &'a RenderOptionState) -> Option<&'a [String]> {
        if self.history_search_active {
            Some(state.history_options.as_slice())
        } else {
            self.pending_options.as_ref().map(|(_, o)| o.as_slice())
        }
    }

    fn input_state(&self) -> InputState {
        InputState {
            text_hash: stable_hash(&self.input_text),
            cursor_pos: self.cursor_pos,
            input_height: self.input_height(),
            api_key_entry: self.api_key_entry.is_some(),
            pending_options: self
                .pending_options
                .as_ref()
                .map_or(0, |(_, opts)| opts.len()),
            history_search_active: self.history_search_active,
        }
    }

    fn transcript_state(&self) -> TranscriptState {
        let last_message_hash = self.messages.last().map_or(0, |message| {
            let visibility_tag = match message.visibility {
                MessageVisibility::UserVisible => 0u8,
                MessageVisibility::InternalProtocolState => 1,
                MessageVisibility::AuditOnly => 2,
            };
            stable_hash(&(
                &message.id,
                role_hash_tag(&message.role),
                message_content_hash(&message.content),
                message.tool_calls.len(),
                message.tool_results.len(),
                visibility_tag,
            ))
        });
        let plan_hash = stable_hash(&(
            self.plan_summary.as_deref(),
            plan_steps_hash(&self.plan_steps),
            self.plan_current_step,
            self.plan_total_steps,
            self.plan_warnings.len(),
        ));

        TranscriptState {
            messages_len: self.messages.len(),
            last_message_hash,
            pending_user_hash: stable_hash(&self.pending_user_message),
            queued_inputs_len: self.queued_inputs.len(),
            stream_hash: stable_hash(&self.stream_buffer),
            reasoning_hash: stable_hash(&(
                &self.reasoning_buffer,
                self.current_turn_reasoning_tokens,
                self.show_reasoning,
            )),
            scroll_offset: self.scroll_offset,
            plan_hash,
            subagents_len: self.subagents.len(),
            subagents_hash: subagents_hash(&self.subagents),
            swarm_hash: active_swarm_hash(self.active_swarm.as_ref()),
            todo_hash: stable_hash(&self.todo_items),
            diffs_len: self.file_diffs.len(),
            diff_hash: self.diff_state_hash(),
            settings_open: self.settings_open,
            showing_welcome: self.is_showing_welcome(),
        }
    }

    fn diff_state_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.diff_focused.hash(&mut hasher);
        self.selected_diff.hash(&mut hasher);
        self.diff_scroll.hash(&mut hasher);
        for diff in &self.file_diffs {
            diff.path.hash(&mut hasher);
            diff.stats.hash(&mut hasher);
            let status = match diff.status {
                diff_viewer::DiffStatus::Pending => 0u8,
                diff_viewer::DiffStatus::Accepted => 1,
                diff_viewer::DiffStatus::Rejected => 2,
            };
            status.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn status_state(&self) -> StatusState {
        let streaming_bucket = if self.is_streaming {
            self.stream_motion_frame().elapsed_ms / motion::MotionFrame::TICK_MS
        } else {
            0
        };
        let idle_seconds = if !self.is_streaming && self.ui_started_at.elapsed().as_secs() >= 3 {
            self.ui_started_at.elapsed().as_secs()
        } else {
            0
        };

        StatusState {
            render_epoch: self.render_epoch(),
            streaming_bucket,
            idle_seconds,
            notice_hash: stable_hash(&self.status_notice()),
            status_hash: stable_hash(&(
                &self.status_message,
                &self.current_task_title,
                self.current_turn_tokens,
                self.current_turn_input_tokens,
                self.current_turn_output_tokens,
                self.live_agent_tokens(),
            )),
        }
    }

    pub fn set_renderer_mode(&mut self, mode: RendererMode) {
        self.renderer_mode = mode;
        self.config.ui.renderer = mode.label().to_string();
        self.status_message = format!(
            "TUI renderer set to {}; restart octo to apply terminal mode",
            mode.label()
        );
        self.push_activity(format!("renderer: {}", mode.label()));
    }

    pub fn set_renderer_config(&mut self, value: &str) {
        let normalized = value.trim().to_ascii_lowercase();
        let stored = if normalized.is_empty() {
            "auto"
        } else {
            normalized.as_str()
        };
        self.renderer_mode = RendererMode::from_config(stored);
        self.config.ui.renderer = stored.to_string();
        self.status_message = format!(
            "TUI renderer set to {} (resolved to {}); restart octo to apply terminal mode",
            stored,
            self.renderer_mode.label()
        );
        self.push_activity(format!(
            "renderer: {} ({})",
            stored,
            self.renderer_mode.label()
        ));
    }

    fn input_placeholder(&self) -> &'static str {
        ""
    }

    fn api_key_input_placeholder(&self) -> &'static str {
        if self.is_chinese_ui() {
            "粘贴 API key"
        } else {
            "paste API key"
        }
    }

    fn is_chinese_ui(&self) -> bool {
        welcome::is_chinese_display_language(&self.config.ui.language)
    }

    fn screen_flags(&self) -> screens::ScreenFlags {
        screens::ScreenFlags {
            decision_open: self.options_needed.is_some(),
            approval_open: self.approval.is_some(),
            settings_open: self.settings_open,
            api_key_entry_open: self.api_key_entry.is_some()
                && !self.input_text.trim_start().starts_with('/'),
            history_search_open: self.history_search_active,
            diff_focused: self.diff_focused,
            file_tree_focused: self.file_tree_focused,
            showing_welcome: self.messages.is_empty()
                && self.stream_buffer.is_empty()
                && !self.is_streaming,
        }
    }

    pub fn active_screen(&self) -> screens::ActiveScreen {
        screens::active_screen(self.screen_flags())
    }

    fn key_action(&self, key: KeyEvent) -> Option<keybindings::KeyAction> {
        self.keymap.action_for(key)
    }

    fn key_is_exit(&self, key: KeyEvent) -> bool {
        self.key_action(key) == Some(keybindings::KeyAction::Exit)
    }

    fn key_is_interrupt(&self, key: KeyEvent) -> bool {
        self.key_action(key) == Some(keybindings::KeyAction::Interrupt)
    }

    pub fn open_settings_panel(&mut self) {
        self.settings_open = true;
        self.settings_selected = self
            .settings_selected
            .min(settings_panel::row_count(self.settings_tab).saturating_sub(1));
        self.status_message = if self.is_chinese_ui() {
            "设置：Enter/Space/←/→ 修改当前项".into()
        } else {
            "Settings: Enter/Space/←/→ edits selected value".into()
        };
    }

    fn handle_settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.settings_open = false;
                self.status_message = if self.is_chinese_ui() {
                    "设置已关闭".into()
                } else {
                    "Settings closed".into()
                };
            }
            KeyCode::Tab => {
                self.settings_tab = self.settings_tab.next();
                self.settings_selected = 0;
            }
            KeyCode::BackTab => {
                self.settings_tab = self.settings_tab.previous();
                self.settings_selected = 0;
            }
            KeyCode::Up => {
                self.settings_selected = self.settings_selected.saturating_sub(1);
            }
            KeyCode::Down => {
                let max = settings_panel::row_count(self.settings_tab).saturating_sub(1);
                self.settings_selected = (self.settings_selected + 1).min(max);
            }
            KeyCode::Left => {
                self.apply_selected_setting_change(-1);
            }
            KeyCode::Right | KeyCode::Enter => {
                self.apply_selected_setting_change(1);
            }
            KeyCode::Char(' ') => {
                self.apply_selected_setting_change(1);
            }
            _ => {}
        }
    }

    fn apply_selected_setting_change(&mut self, delta: i32) {
        let changed = match self.settings_tab {
            settings_panel::SettingsTab::Model => self.apply_model_setting_change(delta),
            settings_panel::SettingsTab::Safety => self.apply_safety_setting_change(delta),
            settings_panel::SettingsTab::Interface => self.apply_interface_setting_change(delta),
            settings_panel::SettingsTab::Agents => self.apply_agent_setting_change(delta),
        };

        if changed {
            self.persist_settings_change();
        } else {
            self.status_message = "Selected setting is informational".into();
        }
    }

    fn apply_model_setting_change(&mut self, delta: i32) -> bool {
        match self.settings_selected {
            0 => {
                self.config.provider.default =
                    cycle_provider_kind(self.config.provider.default, delta);
                self.status_message = format!(
                    "Provider default: {}",
                    self.config.provider.default.as_str()
                );
                true
            }
            1 => {
                self.model = cycle_model(&self.model, delta);
                self.config.model.default = self.model.clone();
                self.welcome.model = self.model.clone();
                self.status_message = format!("Active model: {}", self.model);
                true
            }
            2 => {
                self.config.model.default = cycle_model(&self.config.model.default, delta);
                self.status_message = format!("Default model: {}", self.config.model.default);
                true
            }
            3 => {
                self.config.model.heavy = cycle_model(&self.config.model.heavy, delta);
                self.status_message = format!("Heavy model: {}", self.config.model.heavy);
                true
            }
            4 => {
                self.thinking_mode = cycle_thinking_mode(&self.thinking_mode, delta);
                self.config.model.thinking_mode = self.thinking_mode.clone();
                self.welcome.thinking = self.thinking_mode.clone();
                self.status_message = format!("Active thinking: {}", self.thinking_mode);
                true
            }
            5 => {
                self.config.model.thinking_mode =
                    cycle_thinking_mode(&self.config.model.thinking_mode, delta);
                self.status_message =
                    format!("Default thinking: {}", self.config.model.thinking_mode);
                true
            }
            6 => {
                self.config.model.reasoning_effort =
                    cycle_reasoning_effort(&self.config.model.reasoning_effort, delta);
                self.status_message =
                    format!("Reasoning effort: {}", self.config.model.reasoning_effort);
                true
            }
            _ => false,
        }
    }

    fn apply_safety_setting_change(&mut self, delta: i32) -> bool {
        match self.settings_selected {
            0 => {
                self.config.policy.autonomy_level =
                    cycle_autonomy_level(self.config.policy.autonomy_level, delta);
                self.status_message = format!(
                    "Autonomy level: {}",
                    self.config.policy.autonomy_level.as_str()
                );
                true
            }
            1 => {
                self.config.policy.auto_approve_safe_read =
                    !self.config.policy.auto_approve_safe_read;
                self.status_message = format!(
                    "Safe reads: {}",
                    settings_on_off(self.config.policy.auto_approve_safe_read)
                );
                true
            }
            2 => {
                self.config.policy.auto_mode = !self.config.policy.auto_mode;
                self.status_message = format!(
                    "Auto mode: {}",
                    settings_on_off(self.config.policy.auto_mode)
                );
                true
            }
            3 => {
                self.config.policy.require_approval_for_write =
                    !self.config.policy.require_approval_for_write;
                self.status_message = format!(
                    "Write approval: {}",
                    settings_on_required(self.config.policy.require_approval_for_write)
                );
                true
            }
            4 => {
                self.config.policy.require_approval_for_command =
                    !self.config.policy.require_approval_for_command;
                self.status_message = format!(
                    "Command approval: {}",
                    settings_on_required(self.config.policy.require_approval_for_command)
                );
                true
            }
            5 => {
                self.config.policy.network_access = !self.config.policy.network_access;
                self.status_message = format!(
                    "Network access: {}",
                    settings_on_off(self.config.policy.network_access)
                );
                true
            }
            6 => {
                self.config.policy.block_protected_paths =
                    !self.config.policy.block_protected_paths;
                self.status_message = format!(
                    "Protected paths: {}",
                    settings_on_off(self.config.policy.block_protected_paths)
                );
                true
            }
            7 => {
                self.config.policy.command_timeout_seconds = cycle_command_timeout_seconds(
                    self.config.policy.command_timeout_seconds,
                    delta,
                );
                self.status_message = format!(
                    "Command timeout: {}s",
                    self.config.policy.command_timeout_seconds
                );
                true
            }
            _ => false,
        }
    }

    fn apply_interface_setting_change(&mut self, delta: i32) -> bool {
        match self.settings_selected {
            0 => {
                self.config.ui.language = cycle_ui_language(&self.config.ui.language, delta);
                self.welcome.display_language = self.config.ui.language.clone();
                let language_label = ui_language_display_name(&self.config.ui.language);
                self.status_message = if self.is_chinese_ui() {
                    format!("界面语言：{language_label}")
                } else {
                    format!("Display language: {language_label}")
                };
                self.push_activity(format!("language: {}", self.config.ui.language));
                true
            }
            1 => {
                let next = cycle_theme_mode(self.theme_mode, delta);
                self.set_theme_mode(next);
                true
            }
            2 => {
                self.motion_level = cycle_motion_level(self.motion_level, delta);
                self.config.ui.motion = self.motion_level.label().to_string();
                self.status_message = format!("Motion: {}", self.motion_level.label());
                self.push_activity(format!("motion: {}", self.motion_level.label()));
                true
            }
            3 => {
                let next = cycle_renderer_mode(self.renderer_mode, delta);
                self.set_renderer_mode(next);
                true
            }
            4 => {
                self.config.ui.show_reasoning_summary = !self.config.ui.show_reasoning_summary;
                self.status_message = format!(
                    "Reasoning summary: {}",
                    settings_on_off(self.config.ui.show_reasoning_summary)
                );
                true
            }
            5 => {
                self.config.ui.show_raw_reasoning = !self.config.ui.show_raw_reasoning;
                self.status_message = format!(
                    "Raw reasoning: {}",
                    settings_on_off(self.config.ui.show_raw_reasoning)
                );
                true
            }
            6 => {
                self.config.ui.show_cache_hud = !self.config.ui.show_cache_hud;
                self.status_message = format!(
                    "Cache HUD: {}",
                    settings_on_off(self.config.ui.show_cache_hud)
                );
                true
            }
            _ => false,
        }
    }

    fn apply_agent_setting_change(&mut self, delta: i32) -> bool {
        match self.settings_selected {
            0 => {
                self.config.router.enabled = !self.config.router.enabled;
                self.status_message =
                    format!("Router: {}", settings_on_off(self.config.router.enabled));
                true
            }
            1 => {
                self.config.router.use_model_classifier = !self.config.router.use_model_classifier;
                self.status_message = format!(
                    "Model classifier: {}",
                    settings_on_off(self.config.router.use_model_classifier)
                );
                true
            }
            2 => {
                self.config.subagent.enabled = !self.config.subagent.enabled;
                self.status_message = format!(
                    "Subagents: {}",
                    settings_on_off(self.config.subagent.enabled)
                );
                true
            }
            3 => {
                self.config.subagent.swarm_enabled = !self.config.subagent.swarm_enabled;
                self.status_message = format!(
                    "Swarm: {}",
                    settings_on_off(self.config.subagent.swarm_enabled)
                );
                true
            }
            4 => {
                let current = self.config.subagent.max_parallel.max(1);
                self.config.subagent.max_parallel = if delta < 0 {
                    current.saturating_sub(1).max(1)
                } else {
                    (current + 1).min(64)
                };
                self.status_message = format!(
                    "Max parallel subagents: {}",
                    self.config.subagent.max_parallel
                );
                true
            }
            5 => {
                self.config.subagent.auto_decompose = !self.config.subagent.auto_decompose;
                self.status_message = format!(
                    "Auto decompose: {}",
                    settings_on_off(self.config.subagent.auto_decompose)
                );
                true
            }
            6 => {
                self.config.subagent.allow_custom_agents =
                    !self.config.subagent.allow_custom_agents;
                self.status_message = format!(
                    "Custom agents: {}",
                    settings_on_off(self.config.subagent.allow_custom_agents)
                );
                true
            }
            7 => {
                self.config.mcp.enabled = !self.config.mcp.enabled;
                self.status_message = format!("MCP: {}", settings_on_off(self.config.mcp.enabled));
                true
            }
            9 => {
                self.config.telemetry.enabled = !self.config.telemetry.enabled;
                self.status_message = format!(
                    "Telemetry: {}",
                    settings_on_off(self.config.telemetry.enabled)
                );
                true
            }
            _ => false,
        }
    }

    fn persist_settings_change(&mut self) {
        match self
            .config
            .save_project_local_settings(&self.file_tree.root)
        {
            Ok(path) => {
                let changed = self.status_message.clone();
                self.status_message = format!("{changed} · saved {}", path.to_string_lossy());
            }
            Err(err) => {
                self.status_message =
                    format!("Setting applied in this session, but couldn't persist: {err}");
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, tx: &mpsc::UnboundedSender<TuiAction>) {
        if !is_actionable_key_event(key) {
            return;
        }
        if self.key_is_exit(key) {
            self.handle_exit_key();
            return;
        }
        self.clear_exit_confirmation();

        // Decision selection mode: arrow keys + Enter, with digit/letter shortcuts.
        if let Some(decision) = self.options_needed.take() {
            let DecisionPrompt {
                kind,
                title,
                options,
                respond,
            } = decision;
            match key.code {
                KeyCode::Up => {
                    self.move_option_selection_up(options.len());
                    self.options_needed = Some(DecisionPrompt {
                        kind,
                        title,
                        options,
                        respond,
                    });
                    return;
                }
                KeyCode::Down | KeyCode::Tab => {
                    self.move_option_selection_down(options.len());
                    self.options_needed = Some(DecisionPrompt {
                        kind,
                        title,
                        options,
                        respond,
                    });
                    return;
                }
                KeyCode::Enter => {
                    let idx = self
                        .selected_option_index
                        .min(options.len().saturating_sub(1));
                    let _ = respond.send(idx);
                    self.selected_option_index = 0;
                    return;
                }
                KeyCode::Char(c) => {
                    if let Some(idx) = option_shortcut_index(c, options.len()) {
                        let _ = respond.send(idx);
                        self.selected_option_index = 0;
                        return;
                    }
                }
                KeyCode::Esc => {
                    let _ = respond.send(usize::MAX);
                    self.selected_option_index = 0;
                    return;
                }
                _ => {}
            }
            self.options_needed = Some(DecisionPrompt {
                kind,
                title,
                options,
                respond,
            });
            return;
        }

        // Approval mode: keyboard selection only.
        if self.approval.is_some() {
            match key.code {
                KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
                    self.move_approval_selection_prev();
                }
                KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                    self.move_approval_selection_next();
                }
                KeyCode::Enter => {
                    self.complete_current_approval(self.selected_approval_action());
                }
                KeyCode::Char(c) => {
                    if let Some(action) = approval_action_from_shortcut(c) {
                        self.complete_current_approval(action);
                    }
                }
                KeyCode::Esc => {
                    self.complete_current_approval(ApprovalAction::Deny);
                }
                _ => {}
            }
            return;
        }

        if self.settings_open {
            self.handle_settings_key(key);
            return;
        }

        if self.api_key_entry.is_some() && !self.input_text.trim_start().starts_with('/') {
            self.handle_api_key_entry_key(key, tx);
            return;
        }

        if self.history_search_active {
            self.handle_history_search_key(key);
            return;
        }

        if self.key_action(key) == Some(keybindings::KeyAction::OpenSettings) {
            self.open_settings_panel();
            return;
        }

        if self.handle_pending_option_key(key, tx) {
            return;
        }

        if self.handle_slash_suggestion_key(key) {
            return;
        }

        if self.handle_file_mention_key(key) {
            return;
        }

        match key.code {
            KeyCode::Enter => {
                // File tree: read selected file
                if self.file_tree_focused {
                    if let Some(path) = self.file_tree.selected_path() {
                        if !self.file_tree.selected_is_dir() {
                            let relative = path
                                .strip_prefix(&self.file_tree.root)
                                .unwrap_or(&path)
                                .to_string_lossy()
                                .replace('\\', "/");
                            let _ = tx.send(TuiAction::Submit(format!("Read @{relative}")));
                            self.status_message = format!("Reading @{relative}");
                        }
                    }
                    return;
                }
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.insert_text_at_cursor("\n");
                } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.status_message = "Ctrl+Enter is not assigned; press Enter to send".into();
                } else {
                    let input = self.input_text.trim().to_string();
                    if !input.is_empty() {
                        // Save to input history
                        if self.input_history.is_empty()
                            || self.input_history.last().unwrap() != &input
                        {
                            self.input_history.push(input.clone());
                            crate::storage::input_history::save_history(&self.input_history);
                        }
                        self.history_cursor = None;
                        self.draft_input.clear();

                        // If options are pending, try to match selection
                        if let Some((_, options)) = self.pending_options.take() {
                            let is_multi = self
                                .pending_question_rich
                                .as_ref()
                                .map(|r| r.multi_select)
                                .unwrap_or(false);
                            self.pending_question_rich = None;
                            self.selected_multi_options.clear();
                            if is_multi {
                                if let Some(picks) = try_match_multi_options(&input, &options) {
                                    let labels =
                                        picks.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>();
                                    let _ = tx.send(TuiAction::Submit(format_pending_multi_reply(
                                        &picks,
                                    )));
                                    self.status_message = format!(
                                        "Selected {} options: {}",
                                        picks.len(),
                                        labels.join(", ")
                                    );
                                    return;
                                }
                            }
                            let choice = try_match_option(&input, &options);
                            if let Some((idx, text)) = choice {
                                let _ = tx.send(TuiAction::Submit(format_pending_option_reply(
                                    idx, &text,
                                )));
                                self.status_message = format!("Selected option {}", idx);
                                return;
                            }
                            // Not a valid option selection — treat as normal message
                        }

                        if self.is_streaming && !input.starts_with('/') {
                            self.queue_user_input(input);
                        } else {
                            let _ = tx.send(TuiAction::Submit(input));
                        }
                    }
                }
            }
            KeyCode::Esc if self.is_streaming => {
                let _ = tx.send(TuiAction::Interrupt);
                self.status_message = "Interrupt requested".into();
            }
            KeyCode::Backspace if self.cursor_pos > 0 => {
                let byte_idx = self
                    .input_text
                    .char_indices()
                    .nth(self.cursor_pos.saturating_sub(1))
                    .map_or(self.input_text.len(), |(i, _)| i);
                self.input_text.remove(byte_idx);
                self.cursor_pos -= 1;
            }
            KeyCode::Delete if self.cursor_pos < self.input_text.chars().count() => {
                let byte_idx = char_to_byte_idx(&self.input_text, self.cursor_pos);
                self.input_text.remove(byte_idx);
            }
            KeyCode::Left if self.cursor_pos > 0 => {
                self.cursor_pos -= 1;
            }
            KeyCode::Right if self.cursor_pos < self.input_text.chars().count() => {
                self.cursor_pos += 1;
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_newer(usize::MAX);
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_older(PAGE_SCROLL_LINES);
            }
            KeyCode::Home if self.input_text.is_empty() && !self.diff_focused => {
                self.scroll_older(PAGE_SCROLL_LINES);
            }
            KeyCode::End if self.input_text.is_empty() && !self.diff_focused => {
                self.scroll_newer(usize::MAX);
            }
            KeyCode::Home => {
                let (line, _) = line_and_col(&self.input_text, self.cursor_pos);
                self.cursor_pos = pos_of_line_col(&self.input_text, line, 0);
            }
            KeyCode::End => {
                let (line, _) = line_and_col(&self.input_text, self.cursor_pos);
                self.cursor_pos =
                    pos_of_line_col(&self.input_text, line, col_of_line(&self.input_text, line));
            }
            KeyCode::Up => {
                if self.diff_focused {
                    if let Some(idx) = self.selected_diff {
                        if idx > 0 {
                            self.selected_diff = Some(idx - 1);
                            self.diff_scroll = 0;
                        }
                    }
                } else if self.file_tree_focused {
                    self.file_tree.navigate_up();
                } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_older(3);
                } else if self.can_use_arrow_history() {
                    self.recall_previous_history();
                } else {
                    let (line, _) = line_and_col(&self.input_text, self.cursor_pos);
                    if line == 0 {
                        self.scroll_older(1);
                    } else {
                        self.move_cursor_up();
                    }
                }
            }
            KeyCode::Down => {
                if self.diff_focused {
                    if let Some(idx) = self.selected_diff {
                        if idx + 1 < self.file_diffs.len() {
                            self.selected_diff = Some(idx + 1);
                            self.diff_scroll = 0;
                        }
                    }
                } else if self.file_tree_focused {
                    self.file_tree.navigate_down();
                } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_newer(3);
                } else if self.history_cursor.is_some() {
                    self.recall_next_history();
                } else {
                    let total_lines = logical_line_count(&self.input_text);
                    let (line, _) = line_and_col(&self.input_text, self.cursor_pos);
                    if line + 1 >= total_lines {
                        self.scroll_newer(1);
                    } else {
                        self.move_cursor_down();
                    }
                }
            }
            KeyCode::PageDown => {
                if self.diff_focused {
                    self.diff_scroll = self.diff_scroll.saturating_add(PAGE_SCROLL_LINES);
                } else {
                    self.scroll_newer(PAGE_SCROLL_LINES);
                }
            }
            KeyCode::PageUp => {
                if self.diff_focused {
                    self.diff_scroll = self.diff_scroll.saturating_sub(PAGE_SCROLL_LINES);
                } else {
                    self.scroll_older(PAGE_SCROLL_LINES);
                }
            }
            KeyCode::Tab => {
                if let Some((_, options)) = &self.pending_options {
                    let current = self.input_text.trim().to_lowercase();
                    let matches: Vec<&String> = options
                        .iter()
                        .filter(|opt| opt.to_lowercase().starts_with(&current))
                        .collect();
                    if !matches.is_empty() {
                        let next = if matches.len() == 1 {
                            matches[0]
                        } else {
                            let current_pos =
                                matches.iter().position(|&m| m.to_lowercase() == current);
                            match current_pos {
                                Some(idx) if idx + 1 < matches.len() => matches[idx + 1],
                                _ => matches[0],
                            }
                        };
                        self.input_text = next.clone();
                        self.cursor_pos = self.input_text.chars().count();
                    }
                } else if self.try_complete_slash_command() {
                } else {
                    self.try_complete_file_mention();
                }
            }
            KeyCode::BackTab => {
                self.cycle_interaction_mode();
            }
            KeyCode::Char(c) => match c {
                _ if self.key_action(key) == Some(keybindings::KeyAction::OpenHistorySearch) => {
                    self.open_history_search();
                }
                'j' if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.insert_text_at_cursor("\n");
                }
                's' if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let input = self.input_text.trim().to_string();
                    if input.is_empty() {
                        self.status_message = "No input to inject".into();
                    } else if self.is_streaming {
                        self.queue_user_input(input);
                    } else {
                        let _ = tx.send(TuiAction::Submit(input));
                    }
                }
                'l' if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.messages.clear();
                    self.stream_buffer.clear();
                    self.reasoning_buffer.clear();
                    self.plan_steps.clear();
                    self.plan_current_step = 0;
                    self.plan_total_steps = 0;
                    self.plan_summary = None;
                    self.subagents.clear();
                    self.file_diffs.clear();
                    self.selected_diff = None;
                    self.diff_scroll = 0;
                    self.diff_focused = false;
                    self.pending_options = None;
                    self.pending_question_rich = None;
                    self.selected_multi_options.clear();
                    self.activity_log.clear();
                    crate::workspace::apply::clear_history();
                    self.status_message = "Screen cleared".into();
                }
                'p' if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.recall_previous_history();
                }
                'n' if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.recall_next_history();
                }
                c if c.eq_ignore_ascii_case(&'t') && self.input_text.is_empty() => {
                    self.show_reasoning = !self.show_reasoning;
                    self.status_message = if self.show_reasoning {
                        "Thinking panel expanded".into()
                    } else {
                        "Thinking panel collapsed".into()
                    };
                }
                'd' if self.input_text.is_empty() => {
                    if self.file_diffs.is_empty() {
                        self.diff_focused = false;
                        self.selected_diff = None;
                        self.diff_scroll = 0;
                        self.status_message = if self.is_chinese_ui() {
                            "没有可查看的 diff".into()
                        } else {
                            "No diffs to review".into()
                        };
                    } else {
                        self.diff_focused = !self.diff_focused;
                        self.file_tree_focused = false;
                        if self.diff_focused
                            && self
                                .selected_diff
                                .is_none_or(|idx| idx >= self.file_diffs.len())
                        {
                            self.selected_diff = Some(0);
                        }
                        self.diff_scroll = 0;
                        self.status_message = if self.diff_focused {
                            if self.is_chinese_ui() {
                                "Diff 焦点：↑↓ 文件 · PgUp/PgDn 滚动 · a 接受 · r 拒绝 · d 关闭"
                                    .into()
                            } else {
                                "Diff focus: ↑↓ file · PgUp/PgDn scroll · a accept · r reject · d close"
                                    .into()
                            }
                        } else {
                            "Diff focus OFF".into()
                        };
                    }
                }
                'f' if self.input_text.is_empty() => {
                    self.show_file_tree = !self.show_file_tree;
                    self.file_tree_focused = self.show_file_tree;
                    self.diff_focused = false;
                    self.diff_scroll = 0;
                    if self.show_file_tree {
                        self.file_tree.refresh();
                        self.status_message =
                            "File tree ON — ↑↓ navigate, Enter=read, →=expand".into();
                    } else {
                        self.status_message = "File tree OFF".into();
                    }
                }
                'j' if self.input_text.is_empty()
                    && !self.diff_focused
                    && !self.file_tree_focused =>
                {
                    self.scroll_newer(1);
                }
                'k' if self.input_text.is_empty()
                    && !self.diff_focused
                    && !self.file_tree_focused =>
                {
                    self.scroll_older(1);
                }
                'a' if self.input_text.is_empty() && self.diff_focused => {
                    if let Some(idx) = self.selected_diff {
                        if idx < self.file_diffs.len() {
                            self.file_diffs[idx].status = diff_viewer::DiffStatus::Accepted;
                            self.status_message = if self.is_chinese_ui() {
                                format!("已标记接受（未应用）：{}", self.file_diffs[idx].path)
                            } else {
                                format!(
                                    "Marked accepted (not applied): {}",
                                    self.file_diffs[idx].path
                                )
                            };
                        }
                    }
                }
                'r' if self.input_text.is_empty() && self.diff_focused => {
                    if let Some(idx) = self.selected_diff {
                        if idx < self.file_diffs.len() {
                            self.file_diffs[idx].status = diff_viewer::DiffStatus::Rejected;
                            self.status_message = if self.is_chinese_ui() {
                                format!("已标记拒绝（未应用）：{}", self.file_diffs[idx].path)
                            } else {
                                format!(
                                    "Marked rejected (not applied): {}",
                                    self.file_diffs[idx].path
                                )
                            };
                        }
                    }
                }
                'e' if self.input_text.is_empty() && self.file_tree_focused => {
                    self.file_tree.toggle_expand();
                }
                ']' if self.input_text.is_empty() => {
                    self.scroll_newer(10);
                }
                '[' if self.input_text.is_empty() => {
                    self.scroll_older(10);
                }
                _ => {
                    self.insert_text_at_cursor(&c.to_string());
                }
            },
            _ => {}
        }
    }

    fn handle_pending_option_key(
        &mut self,
        key: KeyEvent,
        tx: &mpsc::UnboundedSender<TuiAction>,
    ) -> bool {
        let Some((_, options)) = self.pending_options.as_ref() else {
            return false;
        };
        if !self.input_text.trim().is_empty() {
            return false;
        }
        let option_count = options.len();

        match key.code {
            KeyCode::Up => {
                self.move_option_selection_up(option_count);
                true
            }
            KeyCode::Down | KeyCode::Tab => {
                self.move_option_selection_down(option_count);
                true
            }
            KeyCode::Enter => {
                self.submit_selected_pending_option(tx);
                true
            }
            KeyCode::Esc => {
                self.pending_options = None;
                self.pending_question_rich = None;
                self.selected_multi_options.clear();
                self.selected_option_index = 0;
                self.status_message = "Option picker dismissed".into();
                true
            }
            KeyCode::Char(' ') if self.pending_question_is_multi_select() => {
                self.toggle_selected_multi_option(option_count);
                true
            }
            KeyCode::Char(c) => {
                if let Some(idx) = option_shortcut_index(c, option_count) {
                    self.selected_option_index = idx;
                    self.submit_selected_pending_option(tx);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn handle_slash_suggestion_key(&mut self, key: KeyEvent) -> bool {
        let suggestions = self.slash_command_suggestions();
        if suggestions.is_empty() {
            self.selected_slash_index = 0;
            return false;
        }

        match key.code {
            KeyCode::Up => {
                self.selected_slash_index = if self.selected_slash_index == 0 {
                    suggestions.len() - 1
                } else {
                    self.selected_slash_index - 1
                };
                true
            }
            KeyCode::Down => {
                self.selected_slash_index = (self.selected_slash_index + 1) % suggestions.len();
                true
            }
            KeyCode::PageUp => {
                self.selected_slash_index = self.selected_slash_index.saturating_sub(8);
                true
            }
            KeyCode::PageDown => {
                self.selected_slash_index =
                    (self.selected_slash_index + 8).min(suggestions.len() - 1);
                true
            }
            KeyCode::Tab => {
                self.complete_selected_slash_command(&suggestions);
                true
            }
            KeyCode::Enter => {
                let trimmed = self.input_text.trim();
                let exact = suggestions.iter().any(|(name, _)| name.as_str() == trimmed);
                if exact {
                    false
                } else {
                    self.complete_selected_slash_command(&suggestions);
                    true
                }
            }
            KeyCode::Esc => {
                self.selected_slash_index = 0;
                self.status_message = if self.is_chinese_ui() {
                    "命令建议已关闭".into()
                } else {
                    "Command suggestions dismissed".into()
                };
                false
            }
            _ => false,
        }
    }

    fn complete_selected_slash_command(&mut self, suggestions: &[(String, String)]) {
        if suggestions.is_empty() {
            return;
        }
        let idx = self.selected_slash_index.min(suggestions.len() - 1);
        self.input_text = format!("{} ", suggestions[idx].0);
        self.cursor_pos = self.input_text.chars().count();
        self.selected_slash_index = idx;
        self.status_message = if self.is_chinese_ui() {
            format!("命令：{}", suggestions[idx].0)
        } else {
            format!("Command: {}", suggestions[idx].0)
        };
    }

    fn slash_command_suggestions(&self) -> Vec<(String, String)> {
        let trimmed = self.input_text.trim();
        if self.settings_open || !trimmed.starts_with('/') || trimmed.contains(char::is_whitespace)
        {
            self.slash_suggestion_cache.borrow_mut().clear();
            return Vec::new();
        }

        let language = self.config.ui.language.as_str();
        let registry_generation = self
            .slash_command_registry_cache
            .borrow_mut()
            .refresh_for_root(&self.file_tree.root);
        {
            let cache = self.slash_suggestion_cache.borrow();
            if cache.prefix == trimmed
                && cache.language == language
                && cache.registry_generation == registry_generation
            {
                return cache.suggestions.clone();
            }
        }

        let suggestions = {
            let registry_cache = self.slash_command_registry_cache.borrow();
            registry_cache
                .registry()
                .match_prefix_all(trimmed)
                .into_iter()
                .map(|entry| match entry {
                    crate::commands::CommandEntry::Builtin(cmd) => {
                        let display_name = if cmd.name.starts_with(trimmed) {
                            cmd.name.to_string()
                        } else {
                            cmd.aliases
                                .iter()
                                .copied()
                                .find(|alias| alias.starts_with(trimmed))
                                .map(str::to_string)
                                .unwrap_or_else(|| cmd.name.to_string())
                        };
                        (
                            display_name,
                            format!(
                                "{} · {}",
                                crate::commands::localized_command_description(
                                    cmd.name,
                                    cmd.description,
                                    language,
                                ),
                                crate::commands::localized_command_usage(cmd.usage, language),
                            ),
                        )
                    }
                    crate::commands::CommandEntry::Prompt(cmd) => (
                        cmd.name.clone(),
                        format!("{} · {}", cmd.description, entry.usage()),
                    ),
                })
                .collect::<Vec<_>>()
        };
        {
            let mut cache = self.slash_suggestion_cache.borrow_mut();
            cache.prefix = trimmed.to_string();
            cache.language = language.to_string();
            cache.registry_generation = registry_generation;
            cache.suggestions = suggestions.clone();
        }
        suggestions
    }

    fn handle_file_mention_key(&mut self, key: KeyEvent) -> bool {
        let suggestions = self.file_mention_suggestions();
        if suggestions.is_empty() {
            self.selected_file_mention_index = 0;
            return false;
        }

        match key.code {
            KeyCode::Up => {
                self.selected_file_mention_index = if self.selected_file_mention_index == 0 {
                    suggestions.len() - 1
                } else {
                    self.selected_file_mention_index - 1
                };
                true
            }
            KeyCode::Down => {
                self.selected_file_mention_index =
                    (self.selected_file_mention_index + 1) % suggestions.len();
                true
            }
            KeyCode::Tab => {
                self.complete_selected_file_mention(&suggestions);
                true
            }
            KeyCode::Enter => {
                if self.current_file_mention_is_exact_match(&suggestions) {
                    false
                } else {
                    self.complete_selected_file_mention(&suggestions);
                    true
                }
            }
            KeyCode::Esc => {
                self.selected_file_mention_index = 0;
                self.status_message = "File suggestions dismissed".into();
                false
            }
            _ => false,
        }
    }

    fn file_mention_suggestions(&self) -> Vec<String> {
        if self.api_key_entry.is_some() || self.settings_open {
            return Vec::new();
        }
        let Some(mention) = mention_prefix_at_cursor(&self.input_text, self.cursor_pos) else {
            return Vec::new();
        };
        let recent = recent_mention_paths(&self.input_history);
        file_mention_candidates(
            &self.file_tree.mention_paths,
            &mention.prefix,
            8,
            &recent,
            mention.quoted,
        )
    }

    fn shell_hint_active(&self) -> bool {
        self.api_key_entry.is_none()
            && !self.settings_open
            && !self.history_search_active
            && self.input_text.trim_start().starts_with('!')
    }

    fn complete_selected_file_mention(&mut self, suggestions: &[String]) {
        if suggestions.is_empty() {
            return;
        }
        let idx = self
            .selected_file_mention_index
            .min(suggestions.len().saturating_sub(1));
        let Some(mention) = mention_prefix_at_cursor(&self.input_text, self.cursor_pos) else {
            return;
        };
        let byte_cursor = char_to_byte_idx(&self.input_text, self.cursor_pos);
        let completion = &suggestions[idx];
        let replacement = file_mention_replacement(completion, mention.quoted, true);
        self.input_text
            .replace_range(mention.token_start..byte_cursor, &replacement);
        self.cursor_pos =
            self.input_text[..mention.token_start].chars().count() + replacement.chars().count();
        self.selected_file_mention_index = idx;
        self.status_message = format!(
            "{} will attach file context",
            file_mention_display(completion, mention.quoted)
        );
    }

    fn current_file_mention_is_exact_match(&self, suggestions: &[String]) -> bool {
        let Some(mention) = mention_prefix_at_cursor(&self.input_text, self.cursor_pos) else {
            return false;
        };
        if mention.quoted {
            return false;
        }
        let prefix = mention.prefix.replace('\\', "/");
        suggestions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&prefix))
    }

    fn input_for_interaction_mode(&self, input: String) -> String {
        if input.trim_start().starts_with('/') {
            return input;
        }
        match self.interaction_mode {
            InteractionMode::Ask | InteractionMode::FullAccess => input,
            InteractionMode::Plan => format!("/plan {input}"),
            InteractionMode::AutoReview => format!("/review {input}"),
        }
    }

    fn plan_completion_report(&self) -> Option<String> {
        if self.plan_steps.is_empty()
            || !self.plan_steps.iter().all(|step| {
                matches!(
                    step.status,
                    plan_tracker::PlanStepStatus::Done | plan_tracker::PlanStepStatus::Failed
                )
            })
        {
            return None;
        }

        let use_chinese = self
            .plan_steps
            .iter()
            .any(|step| contains_cjk(&step.description));
        let done = self
            .plan_steps
            .iter()
            .filter(|step| step.status == plan_tracker::PlanStepStatus::Done)
            .count();
        let failed = self
            .plan_steps
            .iter()
            .filter(|step| step.status == plan_tracker::PlanStepStatus::Failed)
            .count();
        let mut out = String::new();
        if use_chinese {
            out.push_str("\n---\n任务完成\n\n");
            out.push_str(&format!("完成：{done}/{}", self.plan_steps.len()));
            if failed > 0 {
                out.push_str(&format!(" · 失败：{failed}"));
            }
            out.push_str("\n\n");
            out.push_str("| # | 任务 | 用时 | 结果 |\n|---|---|---|---|\n");
        } else {
            out.push_str("\n---\nTask complete\n\n");
            out.push_str(&format!("Completed: {done}/{}", self.plan_steps.len()));
            if failed > 0 {
                out.push_str(&format!(" · Failed: {failed}"));
            }
            out.push_str("\n\n");
            out.push_str("| # | Task | Duration | Result |\n|---|---|---|---|\n");
        }

        for (index, step) in self.plan_steps.iter().enumerate() {
            let status = match step.status {
                plan_tracker::PlanStepStatus::Done => "completed",
                plan_tracker::PlanStepStatus::Failed => "failed",
                plan_tracker::PlanStepStatus::Running => "in_progress",
                plan_tracker::PlanStepStatus::Pending => "pending",
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                index + 1,
                escape_table_cell(&step.description),
                format_duration_compact(step.elapsed_ms()),
                status
            ));
        }

        let total_duration_ms: u64 = self
            .plan_steps
            .iter()
            .filter_map(plan_tracker::PlanStepItem::elapsed_ms)
            .sum();
        if total_duration_ms > 0 {
            if use_chinese {
                out.push_str(&format!(
                    "\n总用时：{}\n",
                    plan_tracker::format_duration_compact(total_duration_ms)
                ));
            } else {
                out.push_str(&format!(
                    "\nTotal time: {}\n",
                    plan_tracker::format_duration_compact(total_duration_ms)
                ));
            }
        }

        let changed_files = self.changed_file_summaries();
        if !changed_files.is_empty() {
            if use_chinese {
                out.push_str("\n变更文件：\n");
            } else {
                out.push_str("\nChanged files:\n");
            }
            for file in changed_files {
                out.push_str(&format!("- {file}\n"));
            }
        } else if use_chinese {
            out.push_str("\n变更文件：无\n");
        } else {
            out.push_str("\nChanged files: none\n");
        }
        Some(out)
    }

    fn changed_file_summaries(&self) -> Vec<String> {
        let mut ordered = Vec::<(String, String, usize)>::new();
        for diff in &self.file_diffs {
            if let Some((_, stats, count)) =
                ordered.iter_mut().find(|(path, _, _)| path == &diff.path)
            {
                *stats = diff.stats.clone();
                *count += 1;
            } else {
                ordered.push((diff.path.clone(), diff.stats.clone(), 1));
            }
        }
        ordered
            .into_iter()
            .map(|(path, stats, count)| {
                if count > 1 {
                    format!("{path} {stats} · {count} updates")
                } else {
                    format!("{path} {stats}")
                }
            })
            .collect()
    }

    fn move_option_selection_up(&mut self, option_count: usize) {
        if option_count == 0 {
            self.selected_option_index = 0;
            return;
        }
        self.selected_option_index = if self.selected_option_index == 0 {
            option_count - 1
        } else {
            self.selected_option_index - 1
        };
    }

    fn move_option_selection_down(&mut self, option_count: usize) {
        if option_count == 0 {
            self.selected_option_index = 0;
            return;
        }
        self.selected_option_index = (self.selected_option_index + 1) % option_count;
    }

    fn pending_question_is_multi_select(&self) -> bool {
        self.pending_question_rich
            .as_ref()
            .is_some_and(|rich| rich.multi_select)
    }

    fn toggle_selected_multi_option(&mut self, option_count: usize) {
        if option_count == 0 {
            self.selected_multi_options.clear();
            self.selected_option_index = 0;
            return;
        }
        if self.selected_multi_options.len() != option_count {
            self.selected_multi_options.resize(option_count, false);
        }
        let idx = self.selected_option_index.min(option_count - 1);
        self.selected_option_index = idx;
        self.selected_multi_options[idx] = !self.selected_multi_options[idx];
        self.status_message = if self.selected_multi_options[idx] {
            format!("Selected option {}", idx + 1)
        } else {
            format!("Unselected option {}", idx + 1)
        };
    }

    fn submit_selected_pending_option(&mut self, tx: &mpsc::UnboundedSender<TuiAction>) {
        let Some((_, options)) = self.pending_options.take() else {
            return;
        };
        let is_multi = self.pending_question_is_multi_select();
        self.pending_question_rich = None;
        if options.is_empty() {
            self.selected_option_index = 0;
            self.selected_multi_options.clear();
            return;
        }
        let idx = self.selected_option_index.min(options.len() - 1);
        let selected = options[idx].clone();
        self.selected_option_index = 0;
        if is_multi {
            let mut picks = options
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    self.selected_multi_options
                        .get(*index)
                        .copied()
                        .unwrap_or(false)
                })
                .map(|(index, option)| (index + 1, option.clone()))
                .collect::<Vec<_>>();
            if picks.is_empty() {
                picks.push((idx + 1, selected));
            }
            self.selected_multi_options.clear();
            let labels = picks
                .iter()
                .map(|(_, label)| label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.status_message = format!("Selected {} options: {labels}", picks.len());
            let _ = tx.send(TuiAction::Submit(format_pending_multi_reply(&picks)));
            return;
        }
        self.selected_multi_options.clear();
        self.status_message = format!("Selected: {selected}");
        let _ = tx.send(TuiAction::Submit(format_pending_option_reply(
            idx + 1,
            &selected,
        )));
    }

    fn open_history_search(&mut self) {
        if self.input_history.is_empty() {
            self.status_message = "No input history yet".into();
            return;
        }
        self.history_search_active = true;
        self.history_search_draft = self.input_text.clone();
        self.input_text.clear();
        self.cursor_pos = 0;
        self.selected_option_index = 0;
        self.history_cursor = None;
        self.status_message =
            "History search: type query, Enter inserts, Esc restores draft".into();
    }

    fn close_history_search(&mut self, restore_draft: bool) {
        self.history_search_active = false;
        self.selected_option_index = 0;
        if restore_draft {
            self.input_text = self.history_search_draft.clone();
            self.cursor_pos = self.input_text.chars().count();
            self.status_message = "History search closed".into();
        }
        self.history_search_draft.clear();
    }

    fn handle_history_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_history_search(true),
            KeyCode::Enter => {
                let options = self.history_search_options();
                if options.is_empty() {
                    self.status_message = "No matching history entry".into();
                    return;
                }
                let idx = self.selected_option_index.min(options.len() - 1);
                self.input_text = options[idx].clone();
                self.cursor_pos = self.input_text.chars().count();
                self.history_search_active = false;
                self.history_search_draft.clear();
                self.selected_option_index = 0;
                self.status_message = "History entry inserted".into();
            }
            KeyCode::Up => {
                let count = self.history_search_options().len();
                self.move_option_selection_up(count);
            }
            KeyCode::Down | KeyCode::Tab => {
                let count = self.history_search_options().len();
                self.move_option_selection_down(count);
            }
            KeyCode::Backspace if self.cursor_pos > 0 => {
                let byte_idx = self
                    .input_text
                    .char_indices()
                    .nth(self.cursor_pos.saturating_sub(1))
                    .map_or(self.input_text.len(), |(i, _)| i);
                self.input_text.remove(byte_idx);
                self.cursor_pos -= 1;
                self.selected_option_index = 0;
            }
            KeyCode::Delete if self.cursor_pos < self.input_text.chars().count() => {
                let byte_idx = char_to_byte_idx(&self.input_text, self.cursor_pos);
                self.input_text.remove(byte_idx);
                self.selected_option_index = 0;
            }
            KeyCode::Left if self.cursor_pos > 0 => {
                self.cursor_pos -= 1;
            }
            KeyCode::Right if self.cursor_pos < self.input_text.chars().count() => {
                self.cursor_pos += 1;
            }
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'r' => {
                self.close_history_search(true);
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_text_at_cursor(&c.to_string());
                self.selected_option_index = 0;
            }
            _ => {}
        }
    }

    fn history_search_options(&self) -> Vec<String> {
        if !self.history_search_active {
            return Vec::new();
        }
        let query = self.input_text.trim().to_lowercase();
        let mut options = Vec::new();
        for entry in self.input_history.iter().rev() {
            if !query.is_empty() && !entry.to_lowercase().contains(&query) {
                continue;
            }
            if !options.iter().any(|existing| existing == entry) {
                options.push(entry.clone());
            }
            if options.len() >= 10 {
                break;
            }
        }
        options
    }

    fn recall_previous_history(&mut self) {
        if self.input_history.is_empty() {
            self.status_message = "No input history yet".into();
            return;
        }

        match self.history_cursor {
            None => {
                self.draft_input = self.input_text.clone();
                self.history_cursor = Some(self.input_history.len() - 1);
            }
            Some(cursor) if cursor > 0 => {
                self.history_cursor = Some(cursor - 1);
            }
            Some(_) => {}
        }

        if let Some(idx) = self.history_cursor {
            self.input_text = self.input_history[idx].clone();
            self.cursor_pos = self.input_text.chars().count();
            self.status_message = format!("History {}/{}", idx + 1, self.input_history.len());
        }
    }

    fn recall_next_history(&mut self) {
        let Some(idx) = self.history_cursor else {
            self.status_message = "History is at newest input".into();
            return;
        };

        if idx + 1 < self.input_history.len() {
            self.history_cursor = Some(idx + 1);
            self.input_text = self.input_history[idx + 1].clone();
            self.status_message = format!("History {}/{}", idx + 2, self.input_history.len());
        } else {
            self.history_cursor = None;
            self.input_text = self.draft_input.clone();
            self.status_message = "Back to draft input".into();
        }
        self.cursor_pos = self.input_text.chars().count();
    }

    fn can_use_arrow_history(&self) -> bool {
        logical_line_count(&self.input_text) == 1 && !self.input_history.is_empty()
    }

    fn try_complete_slash_command(&mut self) -> bool {
        if self.cursor_pos != self.input_text.chars().count() {
            return false;
        }
        let trimmed = self.input_text.trim();
        if !trimmed.starts_with('/') || trimmed.chars().any(char::is_whitespace) {
            return false;
        }

        let matches = {
            let mut registry_cache = self.slash_command_registry_cache.borrow_mut();
            registry_cache.refresh_for_root(&self.file_tree.root);
            registry_cache
                .registry()
                .match_prefix_all(trimmed)
                .into_iter()
                .map(|entry| {
                    let name = entry.name().to_string();
                    let description = match entry {
                        crate::commands::CommandEntry::Builtin(cmd) => {
                            crate::commands::localized_command_description(
                                cmd.name,
                                cmd.description,
                                &self.config.ui.language,
                            )
                            .to_string()
                        }
                        crate::commands::CommandEntry::Prompt(cmd) => cmd.description.clone(),
                    };
                    (name, description)
                })
                .collect::<Vec<_>>()
        };
        match matches.as_slice() {
            [(name, description)] => {
                self.input_text = format!("{name} ");
                self.cursor_pos = self.input_text.chars().count();
                self.status_message = format!("{name} — {description}");
            }
            [] => {
                self.status_message = if self.is_chinese_ui() {
                    format!("没有匹配的斜杠命令：{trimmed}")
                } else {
                    format!("No slash command matches {trimmed}")
                };
            }
            many => {
                let names = many
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                self.status_message = if self.is_chinese_ui() {
                    format!("斜杠命令：{names}")
                } else {
                    format!("Slash commands: {names}")
                };
            }
        }
        true
    }

    fn try_complete_file_mention(&mut self) -> bool {
        let Some(mention) = mention_prefix_at_cursor(&self.input_text, self.cursor_pos) else {
            return false;
        };
        let prefix = mention.prefix.clone();

        let recent = recent_mention_paths(&self.input_history);
        let candidates = file_mention_candidates(
            &self.file_tree.mention_paths,
            &prefix,
            8,
            &recent,
            mention.quoted,
        );
        if candidates.is_empty() {
            self.status_message = format!(
                "No file matches {}",
                file_mention_display(&prefix, mention.quoted)
            );
            return true;
        }

        let completion = if candidates.len() == 1 {
            candidates[0].clone()
        } else {
            let common = common_string_prefix(&candidates);
            if common.chars().count() > prefix.chars().count() {
                common
            } else {
                self.status_message = format!("Files: {}", candidates.join("  "));
                return true;
            }
        };

        let byte_cursor = char_to_byte_idx(&self.input_text, self.cursor_pos);
        let replacement =
            file_mention_replacement(&completion, mention.quoted, candidates.len() == 1);
        self.input_text
            .replace_range(mention.token_start..byte_cursor, &replacement);
        self.cursor_pos =
            self.input_text[..mention.token_start].chars().count() + replacement.chars().count();
        self.status_message = format!(
            "{} will attach file context",
            file_mention_display(&completion, mention.quoted)
        );
        true
    }

    fn handle_api_key_entry_key(&mut self, key: KeyEvent, tx: &mpsc::UnboundedSender<TuiAction>) {
        match key.code {
            KeyCode::Enter => {
                if self
                    .api_key_entry
                    .as_ref()
                    .is_some_and(|entry| entry.saving)
                {
                    self.status_message = "Saving API key...".into();
                    return;
                }

                let api_key = self.input_text.trim().to_string();
                match crate::cli::login::validate_api_key(&api_key) {
                    Ok(valid) => {
                        let pending_prompt = self.api_key_entry.as_mut().and_then(|entry| {
                            entry.saving = true;
                            entry.pending_prompt.clone()
                        });
                        self.set_api_key_state(ApiKeyState::Saving);
                        let _ = tx.send(TuiAction::SaveApiKey {
                            api_key: valid.to_string(),
                            pending_prompt,
                        });
                        self.status_message = "Saving API key...".into();
                    }
                    Err(error) => {
                        self.status_message = error.to_string();
                        self.stream_buffer =
                            "Paste a valid provider API key here, then press Enter.".into();
                    }
                }
            }
            KeyCode::Esc => {
                self.set_api_key_state(ApiKeyState::Missing);
                self.api_key_entry = None;
                self.clear_input_editor();
                self.status_message = "API key entry skipped; local commands still work".into();
            }
            KeyCode::Backspace if self.cursor_pos > 0 => {
                let byte_idx = self
                    .input_text
                    .char_indices()
                    .nth(self.cursor_pos.saturating_sub(1))
                    .map_or(self.input_text.len(), |(i, _)| i);
                self.input_text.remove(byte_idx);
                self.cursor_pos -= 1;
            }
            KeyCode::Delete if self.cursor_pos < self.input_text.chars().count() => {
                let byte_idx = char_to_byte_idx(&self.input_text, self.cursor_pos);
                self.input_text.remove(byte_idx);
            }
            KeyCode::Left if self.cursor_pos > 0 => {
                self.cursor_pos -= 1;
            }
            KeyCode::Right if self.cursor_pos < self.input_text.chars().count() => {
                self.cursor_pos += 1;
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
            }
            KeyCode::End => {
                self.cursor_pos = self.input_text.chars().count();
            }
            KeyCode::Char(c) => {
                self.insert_text_at_cursor(&c.to_string());
            }
            _ => {}
        }
    }

    fn input_height(&self) -> u16 {
        if self.api_key_entry.is_some() {
            return 1;
        }
        logical_line_count(&self.input_text).clamp(1, 5) as u16
    }

    fn move_cursor_up(&mut self) {
        let (line, col) = line_and_col(&self.input_text, self.cursor_pos);
        if line > 0 {
            let prev_line_len = col_of_line(&self.input_text, line - 1);
            let target_col = col.min(prev_line_len);
            self.cursor_pos = pos_of_line_col(&self.input_text, line - 1, target_col);
        }
    }

    fn move_cursor_down(&mut self) {
        let total_lines = logical_line_count(&self.input_text);
        let (line, col) = line_and_col(&self.input_text, self.cursor_pos);
        if line + 1 < total_lines {
            let next_line_len = col_of_line(&self.input_text, line + 1);
            let target_col = col.min(next_line_len);
            self.cursor_pos = pos_of_line_col(&self.input_text, line + 1, target_col);
        }
    }

    fn is_showing_welcome(&self) -> bool {
        self.messages.is_empty() && self.stream_buffer.is_empty() && !self.is_streaming
    }

    fn runtime_idle_title(&self) -> Option<String> {
        // Quiet by default when idle. No badge, no ticking counter —
        // the user can see they're idle without us announcing it every second.
        None
    }

    fn clear_input_editor(&mut self) {
        self.input_text.clear();
        self.cursor_pos = 0;
    }

    fn queue_user_input(&mut self, input: String) {
        let input = input.trim().to_string();
        if input.is_empty() {
            return;
        }
        self.queued_inputs.push_back(input);
        self.clear_input_editor();
        let count = self.queued_inputs.len();
        self.status_message = if count == 1 {
            "已排队：当前任务完成后自动发送。".into()
        } else {
            format!("已排队 {count} 条：将按顺序自动发送。")
        };
        self.push_activity(format!("queued follow-up: {count} pending"));
    }

    fn queued_user_message_refs(&self) -> Vec<&str> {
        self.queued_inputs.iter().map(String::as_str).collect()
    }

    fn insert_text_at_cursor(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let byte_idx = char_to_byte_idx(&self.input_text, self.cursor_pos);
        self.input_text.insert_str(byte_idx, &normalized);
        self.cursor_pos += normalized.chars().count();
    }

    fn handle_paste(&mut self, text: &str) {
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            return;
        }
        self.insert_text_at_cursor(text);
        let line_count = text.lines().count().max(1);
        self.status_message = if line_count > 6 || text.chars().count() > 600 {
            format!("已粘贴 {line_count} 行到输入框；按 Enter 发送，Ctrl+J 换行")
        } else {
            "Pasted into input; press Enter to send".into()
        };
    }

    fn show_local_output(&mut self, output: impl Into<String>) {
        let output = output.into();
        let status = output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Command complete")
            .to_string();
        self.clear_input_editor();
        self.is_streaming = false;
        self.stream_start = None;
        self.stream_buffer = output;
        self.pending_user_message = None;
        self.status_message = truncate_for_activity(&status, 120);
    }

    #[cfg(test)]
    fn show_local_error_keep_input(&mut self, output: impl Into<String>) {
        let output = output.into();
        let status = output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Command failed")
            .to_string();
        self.is_streaming = false;
        self.stream_start = None;
        self.stream_buffer = output;
        self.status_message = truncate_for_activity(&status, 120);
    }

    fn begin_api_key_entry(&mut self, pending_prompt: Option<String>) {
        self.set_api_key_state(ApiKeyState::Entering);
        self.api_key_entry = Some(ApiKeyEntry {
            pending_prompt,
            saving: false,
        });
        self.clear_input_editor();
        self.is_streaming = false;
        self.stream_start = None;
        self.stream_buffer = "Provider API key required.\n\nPaste your provider API key into the input below, then press Enter. It will be stored in the system keyring.".into();
        self.status_message = "Enter API key first".into();
    }

    fn set_api_key_state(&mut self, state: ApiKeyState) {
        self.api_key_state = state;
        self.welcome.api_key_status = state.welcome_status();
    }

    fn refresh_welcome(&mut self, root: &Path) {
        let (startup, _) = TuiStartupData::load(root);
        self.welcome = load_welcome_with_startup(
            root,
            self.model.clone(),
            self.thinking_mode.clone(),
            &startup.config,
            startup.config_loaded,
            startup.api_key_available || self.api_key_state.is_ready(),
        );
        self.welcome.api_key_status = self.api_key_state.welcome_status();
    }

    pub fn refresh_todo_summary(&mut self) {
        let board = crate::tools::todo_state::load_todo_board(&self.file_tree.root);
        self.todo_summary = board.summary;
        self.todo_items = board.items;
    }

    fn push_recent_tool_summary(&mut self, tool_name: &str, success: bool, summary: &str) {
        let status = if success { "ok" } else { "error" };
        let line = format!("{tool_name} [{status}]: {}", first_line(summary));
        if self
            .recent_tool_summaries
            .back()
            .is_some_and(|existing| existing == &line)
        {
            return;
        }
        self.recent_tool_summaries.push_back(line);
        while self.recent_tool_summaries.len() > 5 {
            self.recent_tool_summaries.pop_front();
        }
    }

    fn restore_recoverable_state_from_events(&mut self, events: &[crate::storage::SessionEvent]) {
        self.recent_tool_summaries.clear();
        let mut latest_user_index = None;
        let mut latest_question: Option<(usize, String, Vec<String>, String)> = None;
        let mut latest_compact: Option<(String, String)> = None;
        for (index, event) in events.iter().enumerate() {
            match &event.kind {
                crate::storage::SessionEventKind::UserMessage { .. } => {
                    latest_user_index = Some(index);
                }
                crate::storage::SessionEventKind::ToolCallFinished {
                    name,
                    success,
                    summary,
                    ..
                } => self.push_recent_tool_summary(name, *success, summary),
                crate::storage::SessionEventKind::UserQuestionRequested {
                    title,
                    options,
                    summary,
                    ..
                } => {
                    latest_question =
                        Some((index, title.clone(), options.clone(), summary.clone()));
                }
                crate::storage::SessionEventKind::ContextCompacted {
                    summary, reason, ..
                } => {
                    latest_compact = Some((summary.clone(), reason.clone()));
                }
                _ => {}
            }
        }

        if let Some((summary, reason)) = latest_compact {
            self.latest_compact_summary = Some(summary);
            self.latest_compact_reason = Some(reason);
        }

        if let Some((question_index, title, options, summary)) = latest_question {
            self.last_user_question_summary = Some(summary);
            if latest_user_index.is_none_or(|user_index| question_index > user_index)
                && !options.is_empty()
            {
                self.pending_options = Some((title, options));
                self.selected_option_index = 0;
                self.selected_multi_options.clear();
            }
        }
    }

    pub fn compact_local_context(
        &mut self,
        root: &Path,
        messages_to_keep: usize,
        reason: &str,
    ) -> crate::agent::compact::CompactSnapshot {
        let mut session = self.session_snapshot(root);
        let mut events = Vec::new();
        for line in &self.recent_tool_summaries {
            events.push(crate::storage::SessionEvent::new(
                session.id,
                None,
                crate::storage::SessionEventKind::ToolCallFinished {
                    tool_call_id: String::new(),
                    name: "tool".to_string(),
                    success: !line.contains("[error]"),
                    summary: line.clone(),
                    duration_ms: 0,
                    changed_files: Vec::new(),
                },
            ));
        }
        if let Some(summary) = &self.last_user_question_summary {
            let (title, options) = self
                .pending_options
                .as_ref()
                .map(|(title, options)| (title.clone(), options.clone()))
                .unwrap_or_else(|| ("pending question".to_string(), Vec::new()));
            events.push(crate::storage::SessionEvent::new(
                session.id,
                None,
                crate::storage::SessionEventKind::UserQuestionRequested {
                    tool_call_id: String::new(),
                    name: "ask_user".to_string(),
                    title,
                    options,
                    summary: summary.clone(),
                    descriptions: Vec::new(),
                    previews: Vec::new(),
                    multi_select: false,
                },
            ));
        }
        if let (Some(summary), Some(reason)) =
            (&self.latest_compact_summary, &self.latest_compact_reason)
        {
            events.push(crate::storage::SessionEvent::new(
                session.id,
                None,
                crate::storage::SessionEventKind::ContextCompacted {
                    before_tokens: 0,
                    after_tokens: 0,
                    before_messages: 0,
                    after_messages: 0,
                    retained_start: 0,
                    retained_count: 0,
                    summary: summary.clone(),
                    reason: reason.clone(),
                },
            ));
        }

        session.messages = self.messages.clone();
        let snapshot = crate::agent::compact::build_compact_snapshot(
            &session,
            Some(&events),
            messages_to_keep,
            reason,
        );
        self.messages = crate::agent::compact::retained_messages(&session, messages_to_keep);
        self.latest_compact_summary = Some(snapshot.summary.clone());
        self.latest_compact_reason = Some(snapshot.reason.clone());
        self.compact_notice = Some(if self.is_chinese_ui() {
            format!(
                "已压缩上下文：{} -> {} tokens",
                compact_token_label(snapshot.before_tokens),
                compact_token_label(snapshot.after_tokens)
            )
        } else {
            format!(
                "Context compacted: {} -> {} tokens",
                compact_token_label(snapshot.before_tokens),
                compact_token_label(snapshot.after_tokens)
            )
        });
        self.push_activity(format!(
            "compact: {} -> {} tokens, kept {} messages",
            snapshot.before_tokens, snapshot.after_tokens, snapshot.retained_count
        ));

        if let (Some(home), Some(session_id)) = (dirs::home_dir(), self.session_id) {
            let store = crate::storage::EventLogStore::new(home.join(".octocode"));
            let event = crate::storage::SessionEvent::new(
                session_id,
                None,
                crate::storage::SessionEventKind::ContextCompacted {
                    before_tokens: snapshot.before_tokens,
                    after_tokens: snapshot.after_tokens,
                    before_messages: snapshot.before_messages,
                    after_messages: snapshot.after_messages,
                    retained_start: snapshot.retained_start,
                    retained_count: snapshot.retained_count,
                    summary: snapshot.summary.clone(),
                    reason: snapshot.reason.clone(),
                },
            );
            if let Err(error) = store.append(root, &event) {
                self.push_activity(format!("compact event write failed: {error}"));
            }
        }

        snapshot
    }

    fn mark_api_key_ready_from_storage(&mut self, root: &Path) {
        self.set_api_key_state(ApiKeyState::Ready);
        self.api_key_entry = None;
        self.refresh_welcome(root);
        self.push_activity("api key loaded from storage");
        self.status_message = "API key loaded from saved config".into();
    }

    fn finish_api_key_save_success(
        &mut self,
        root: &Path,
        location: &storage::ApiKeyStoreLocation,
    ) {
        self.set_api_key_state(ApiKeyState::Ready);
        self.api_key_entry = None;
        self.clear_input_editor();
        self.stream_buffer.clear();
        self.refresh_welcome(root);
        self.push_activity("api key saved");
        self.status_message = location.user_message();
    }

    fn finish_api_key_save_error(&mut self, error: &anyhow::Error) {
        self.set_api_key_state(ApiKeyState::Error);
        self.status_message = format!("Couldn't save the API key just now: {error}");
        self.stream_buffer = format!(
            "Could not save API key.\n\n{error}\n\nTry again, or run `octo login --api-key <key>`."
        );
        if let Some(entry) = self.api_key_entry.as_mut() {
            entry.saving = false;
        } else {
            self.api_key_entry = Some(ApiKeyEntry::default());
        }
    }

    fn should_block_agent_turn_for_api_key(&self) -> bool {
        !self.api_key_state.is_ready()
    }

    fn begin_running_turn(&mut self, input: &str) {
        self.clear_input_editor();
        self.is_streaming = true;
        self.stream_start = Some(std::time::Instant::now());
        self.current_task_title = summarize_task_title(input);
        self.current_turn_input_tokens = 0;
        self.current_turn_output_tokens = 0;
        self.current_turn_reasoning_tokens = 0;
        self.current_turn_tokens = 0;
        self.current_turn_usage_finalized = false;
        self.input_token_animation_started = None;
        self.pending_user_message = Some(input.to_string());
        self.stream_buffer.clear();
        self.pending_options = None;
        self.pending_question_rich = None;
        self.selected_multi_options.clear();
        self.compact_notice = None;
        self.selected_option_index = 0;
        self.push_activity(format!("turn started: {}", self.current_task_title));
        self.status_message = "Running turn...".into();
    }

    fn add_output_token_estimate(&mut self, text: &str) {
        let delta = estimate_tokens(text);
        self.add_output_tokens(delta);
    }

    fn add_input_tokens(&mut self, delta: u64) {
        if delta == 0 {
            return;
        }
        if self.current_turn_input_tokens == 0 {
            self.input_token_animation_started = Some(std::time::Instant::now());
        }
        self.current_turn_input_tokens = self.current_turn_input_tokens.saturating_add(delta);
        self.current_turn_tokens = self
            .current_turn_input_tokens
            .saturating_add(self.current_turn_output_tokens);
    }

    fn add_output_tokens(&mut self, delta: u64) {
        if delta == 0 {
            return;
        }
        self.current_turn_output_tokens = self.current_turn_output_tokens.saturating_add(delta);
        self.current_turn_tokens = self
            .current_turn_input_tokens
            .saturating_add(self.current_turn_output_tokens);
    }

    fn cancel_running_work(&mut self) {
        let elapsed_ms = self
            .stream_start
            .map(|started| started.elapsed().as_millis() as u64);
        self.deny_pending_approvals();
        if let Some(decision) = self.options_needed.take() {
            let _ = decision.respond.send(usize::MAX);
        }
        self.pending_options = None;
        self.pending_question_rich = None;
        self.selected_multi_options.clear();
        self.selected_option_index = 0;
        self.is_streaming = false;
        self.stream_start = None;
        self.reasoning_buffer.clear();
        self.current_turn_reasoning_tokens = 0;
        for step in &mut self.plan_steps {
            if step.status == plan_tracker::PlanStepStatus::Running {
                step.transition_to(plan_tracker::PlanStepStatus::Failed);
            }
        }
        if !self.stream_buffer.ends_with('\n') && !self.stream_buffer.is_empty() {
            self.stream_buffer.push('\n');
        }
        match elapsed_ms {
            Some(ms) => self.stream_buffer.push_str(&format!(
                "\n* Cancelled after {}\n",
                plan_tracker::format_duration_compact(ms)
            )),
            None => self.stream_buffer.push_str("\n* Cancelled\n"),
        }
        self.push_activity("turn cancelled by Esc");
        self.status_message = "Interrupted — all running work stopped".into();
    }

    pub fn request_swarm_cancel(&mut self) {
        if let Some(swarm) = self.active_swarm.as_mut() {
            swarm.cancel_requested = true;
            swarm.status = "cancel_requested".to_string();
            let run_id = swarm.run_id.clone();
            self.status_message = format!("Swarm cancel requested: {run_id}");
            self.push_activity(format!("swarm {run_id} cancel requested"));
        } else {
            self.status_message = "No active swarm to cancel".into();
        }
    }

    fn handle_exit_key(&mut self) -> bool {
        if self.exit_confirm_pending {
            self.exit_confirm_pending = false;
            self.running = false;
            self.status_message = if self.is_chinese_ui() {
                "正在退出".into()
            } else {
                "Exiting".into()
            };
            true
        } else {
            self.exit_confirm_pending = true;
            self.status_message = self.exit_confirm_message();
            false
        }
    }

    fn clear_exit_confirmation(&mut self) {
        self.exit_confirm_pending = false;
    }

    fn exit_key_label(&self) -> String {
        self.keymap
            .binding_label(keybindings::KeyAction::Exit)
            .map(|label| format_keybinding_label(&label))
            .unwrap_or_else(|| "Ctrl+D".to_string())
    }

    fn exit_confirm_message(&self) -> String {
        let label = self.exit_key_label();
        if self.is_chinese_ui() {
            format!("再次按 {label} 退出")
        } else {
            format!("Press {label} again to exit")
        }
    }

    fn status_notice(&self) -> Option<&str> {
        if self.exit_confirm_pending && !self.status_message.trim().is_empty() {
            return Some(self.status_message.as_str());
        }
        self.compact_notice
            .as_deref()
            .filter(|notice| !notice.trim().is_empty())
    }

    fn enqueue_approval(
        &mut self,
        display: ApprovalDisplay,
        respond: tokio::sync::oneshot::Sender<bool>,
    ) {
        if self.approval.is_none() {
            self.approval = Some((display, respond));
            self.approval_selected_index = 0;
            return;
        }

        self.approval_queue.push_back((display, respond));
        let pending = self.approval_queue.len();
        self.push_activity(format!("approval queued: {pending} pending"));
        self.status_message = format!("Approval queued - {pending} pending");
    }

    fn activate_next_approval(&mut self) {
        if self.approval.is_some() {
            return;
        }

        if let Some(next) = self.approval_queue.pop_front() {
            self.approval = Some(next);
            self.approval_selected_index = 0;
            let pending = self.approval_queue.len();
            self.push_activity(format!("next approval ready: {pending} pending"));
            self.status_message = if pending == 0 {
                "Next approval ready".into()
            } else {
                format!("Next approval ready - {pending} pending")
            };
        }
    }

    fn move_approval_selection_prev(&mut self) {
        let count = approval_popup::APPROVAL_ACTION_COUNT;
        self.approval_selected_index = if self.approval_selected_index == 0 {
            count.saturating_sub(1)
        } else {
            self.approval_selected_index - 1
        };
    }

    fn move_approval_selection_next(&mut self) {
        let count = approval_popup::APPROVAL_ACTION_COUNT.max(1);
        self.approval_selected_index = (self.approval_selected_index + 1) % count;
    }

    fn selected_approval_action(&self) -> ApprovalAction {
        match self.approval_selected_index {
            1 => ApprovalAction::ApproveSession,
            2 => ApprovalAction::Deny,
            _ => ApprovalAction::ApproveOnce,
        }
    }

    fn complete_current_approval(&mut self, action: ApprovalAction) {
        let Some((_, respond)) = self.approval.take() else {
            return;
        };
        self.approval_selected_index = 0;
        match action {
            ApprovalAction::ApproveOnce => {
                let _ = respond.send(true);
                self.activate_next_approval();
            }
            ApprovalAction::ApproveSession => {
                self.session_auto_approve = true;
                let _ = respond.send(true);
                while let Some((_, queued_respond)) = self.approval_queue.pop_front() {
                    let _ = queued_respond.send(true);
                }
                self.status_message = "Session approvals enabled".into();
            }
            ApprovalAction::Deny => {
                let _ = respond.send(false);
                self.activate_next_approval();
            }
        }
    }

    fn deny_pending_approvals(&mut self) {
        self.approval_selected_index = 0;
        if let Some((_, respond)) = self.approval.take() {
            let _ = respond.send(false);
        }
        while let Some((_, respond)) = self.approval_queue.pop_front() {
            let _ = respond.send(false);
        }
    }

    fn session_snapshot(&self, root: &Path) -> Session {
        Session {
            id: self.session_id.unwrap_or_else(SessionId::new_v4),
            name: self.session_name.clone(),
            project_root: root.to_path_buf(),
            messages: self.messages.clone(),
            reasoning_state: ReasoningState {
                mode: self.thinking_mode.clone(),
                selected_model: Some(self.model.clone()),
                ..ReasoningState::default()
            },
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            metadata: SessionMetadata::default(),
        }
    }

    fn apply_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::UserMessage { .. } => {}
            AgentEvent::ContentDelta(text) => {
                self.add_output_token_estimate(&text);
                if self.plan_execution_has_started() && !text.trim().is_empty() {
                    self.push_activity(format!("plan output: {}", first_line(&text)));
                    return;
                }
                self.stream_buffer.push_str(&text);
            }
            AgentEvent::ReasoningDelta(text) => {
                self.add_output_token_estimate(&text);
                self.current_turn_reasoning_tokens = self
                    .current_turn_reasoning_tokens
                    .saturating_add(estimate_tokens(&text));
                self.reasoning_buffer.push_str(&text);
            }
            AgentEvent::TokenDelta {
                input_tokens,
                output_tokens,
            } => {
                self.add_input_tokens(input_tokens);
                self.add_output_tokens(output_tokens);
            }
            AgentEvent::ToolApprovalNeeded {
                tool_name,
                display,
                respond,
            } => {
                if self.session_auto_approve {
                    let _ = respond.send(true);
                    self.push_activity(format!("auto-approved: {tool_name}"));
                } else {
                    self.push_activity(format!("approval requested: {tool_name}"));
                    self.enqueue_approval(display, respond);
                }
            }
            AgentEvent::ToolStarted { .. } => {}
            AgentEvent::ToolExecuted {
                tool_name,
                success,
                summary,
            } => {
                let status = if success { "ok" } else { "error" };
                self.push_recent_tool_summary(&tool_name, success, &summary);
                if success && is_todo_state_tool(&tool_name) {
                    self.refresh_todo_summary();
                }
                self.push_activity(format!(
                    "tool {} [{}]: {}",
                    tool_name,
                    status,
                    first_line(&summary)
                ));
                self.status_message = format!("Tool {tool_name} [{status}]: {summary}");
            }
            AgentEvent::UserQuestionRequested {
                title,
                options,
                summary,
                descriptions,
                previews,
                multi_select,
            } => {
                self.last_user_question_summary = Some(summary.clone());
                let rich_opts: Vec<PendingQuestionOption> = options
                    .iter()
                    .enumerate()
                    .map(|(i, label)| PendingQuestionOption {
                        label: label.clone(),
                        description: descriptions.get(i).cloned().unwrap_or_default(),
                        preview: previews.get(i).cloned().flatten(),
                    })
                    .collect();
                let has_extras = multi_select
                    || rich_opts.iter().any(|o| o.preview.is_some())
                    || rich_opts.iter().any(|o| !o.description.is_empty());
                self.pending_question_rich = if has_extras {
                    Some(PendingQuestionRich {
                        title: title.clone(),
                        question: summary,
                        options: rich_opts,
                        multi_select,
                    })
                } else {
                    None
                };
                let option_count = options.len();
                self.pending_options = Some((title.clone(), options));
                self.selected_option_index = 0;
                self.selected_multi_options = if multi_select {
                    vec![false; option_count]
                } else {
                    Vec::new()
                };
                self.is_streaming = false;
                self.stream_start = None;
                self.status_message = format!(
                    "{}：{}",
                    if self.is_chinese_ui() {
                        "需要选择"
                    } else {
                        "Select an option"
                    },
                    title
                );
            }
            AgentEvent::ContextCompacted {
                summary,
                reason,
                before_tokens,
                after_tokens,
            } => {
                self.latest_compact_summary = Some(summary);
                self.latest_compact_reason = Some(reason.clone());
                self.compact_notice = Some(if self.is_chinese_ui() {
                    format!(
                        "已自动压缩上下文：{} -> {} tokens",
                        compact_token_label(before_tokens),
                        compact_token_label(after_tokens)
                    )
                } else {
                    format!(
                        "Context auto-compacted: {} -> {} tokens",
                        compact_token_label(before_tokens),
                        compact_token_label(after_tokens)
                    )
                });
                self.push_activity(format!(
                    "context compacted [{reason}]: {before_tokens} -> {after_tokens} tokens"
                ));
            }
            AgentEvent::HookExecuted { .. } => {}
            AgentEvent::StreamDone { usage, cache, .. } => {
                if let Some(u) = usage {
                    let turn_tokens = u64::from(u.total_tokens);
                    if self.current_turn_input_tokens == 0 && u.prompt_tokens > 0 {
                        self.input_token_animation_started = Some(std::time::Instant::now());
                    }
                    self.current_turn_input_tokens = self
                        .current_turn_input_tokens
                        .max(u64::from(u.prompt_tokens));
                    self.current_turn_output_tokens = self
                        .current_turn_output_tokens
                        .max(u64::from(u.completion_tokens));
                    self.current_turn_tokens = self.current_turn_tokens.max(turn_tokens).max(
                        self.current_turn_input_tokens
                            .saturating_add(self.current_turn_output_tokens),
                    );
                    self.current_turn_usage_finalized = true;
                    self.total_tokens = self.total_tokens.saturating_add(turn_tokens);
                    self.total_cost += u.estimate_cost_cny(&self.model);
                    if let Some(c) = cache {
                        let agg = self.cache.get_or_insert_with(CacheUsage::default);
                        agg.prompt_cache_hit_tokens = agg
                            .prompt_cache_hit_tokens
                            .saturating_add(c.prompt_cache_hit_tokens);
                        agg.prompt_cache_miss_tokens = agg
                            .prompt_cache_miss_tokens
                            .saturating_add(c.prompt_cache_miss_tokens);
                    }
                    self.push_activity(format!(
                        "usage: total={} prompt={} completion={} ¥{:.3}",
                        u.total_tokens,
                        u.prompt_tokens,
                        u.completion_tokens,
                        u.estimate_cost_cny(&self.model)
                    ));
                }
            }
            AgentEvent::Error(e) => {
                self.push_activity(format!("error: {e}"));
                // Lead softly — "snag" instead of "Error" keeps the status
                // bar from yelling at the user mid-flow.
                self.status_message = format!("Hit a snag: {e}");
            }
            AgentEvent::TurnComplete { total_tokens, .. } => {
                let turn_duration_ms = self
                    .stream_start
                    .map(|started| started.elapsed().as_millis() as u64);
                let had_hidden_reasoning = !self.reasoning_buffer.trim().is_empty();
                self.is_streaming = false;
                self.stream_start = None;
                self.reasoning_buffer.clear();
                self.current_turn_reasoning_tokens = 0;
                if let Some(duration_ms) = turn_duration_ms {
                    append_brewed_line(&mut self.stream_buffer, duration_ms);
                }
                if let Some(report) = self.plan_completion_report() {
                    if !self.stream_buffer.contains("Task complete")
                        && !self.stream_buffer.contains("任务完成")
                    {
                        if !self.stream_buffer.ends_with('\n') {
                            self.stream_buffer.push('\n');
                        }
                        self.stream_buffer.push_str(&report);
                    }
                }
                if self.stream_buffer.trim().is_empty()
                    && had_hidden_reasoning
                    && self.pending_options.is_none()
                    && self.options_needed.is_none()
                    && self.approval_queue.is_empty()
                {
                    self.stream_buffer = empty_visible_answer_notice(self.is_chinese_ui()).into();
                }
                if !self.current_turn_usage_finalized && total_tokens > 0 {
                    self.current_turn_tokens = total_tokens;
                    self.current_turn_output_tokens =
                        total_tokens.saturating_sub(self.current_turn_input_tokens);
                }
                let turn_tokens = if self.current_turn_tokens > 0 {
                    self.current_turn_tokens
                } else {
                    total_tokens
                };
                self.push_activity(format!(
                    "turn complete: {turn_tokens} tokens ¥{:.3}",
                    self.total_cost
                ));
                self.status_message =
                    format!("Done — {turn_tokens} tokens ¥{:.3}", self.total_cost);
            }
            AgentEvent::ComplexityAssessed { assessment } => {
                self.push_activity(format!("complexity: {}", assessment.display_summary()));
                self.status_message = assessment.explanation.clone();
            }
            AgentEvent::ClarificationNeeded { questions } => {
                self.is_streaming = false;
                self.stream_start = None;
                let text = questions.join("\n");
                self.push_activity(format!(
                    "clarification needed: {} questions",
                    questions.len()
                ));
                self.stream_buffer
                    .push_str(&format!("\n[需要澄清]\n{text}\n"));
                self.status_message = "需要澄清".into();
            }
            AgentEvent::PlanStepUpdate {
                index,
                total,
                description,
                status,
            } => {
                let had_started_plan_execution = self.plan_execution_has_started();
                let should_reset_plan = index == 0
                    && (self.plan_steps.is_empty()
                        || (self.plan_total_steps > 0
                            && self.plan_current_step >= self.plan_total_steps));
                if should_reset_plan {
                    // New plan started — clear any lingering completed plan from previous turn
                    self.plan_steps.clear();
                    self.plan_current_step = 0;
                    self.plan_total_steps = total;
                }
                while self.plan_steps.len() <= index {
                    self.plan_steps.push(plan_tracker::PlanStepItem::new(
                        "",
                        plan_tracker::PlanStepStatus::Pending,
                    ));
                }
                let mapped = match status {
                    crate::agent::orchestrator::PlanStepStatus::Pending => {
                        plan_tracker::PlanStepStatus::Pending
                    }
                    crate::agent::orchestrator::PlanStepStatus::Running => {
                        plan_tracker::PlanStepStatus::Running
                    }
                    crate::agent::orchestrator::PlanStepStatus::Done => {
                        plan_tracker::PlanStepStatus::Done
                    }
                    crate::agent::orchestrator::PlanStepStatus::Failed => {
                        plan_tracker::PlanStepStatus::Failed
                    }
                };
                self.plan_steps[index].description = summarize_plan_step(&description);
                self.plan_steps[index].transition_to(mapped);
                self.plan_total_steps = total;
                if !had_started_plan_execution && self.plan_execution_has_started() {
                    self.stream_buffer.clear();
                }
                if mapped == plan_tracker::PlanStepStatus::Running {
                    self.plan_current_step = index;
                } else if (mapped == plan_tracker::PlanStepStatus::Done
                    || mapped == plan_tracker::PlanStepStatus::Failed)
                    && index >= self.plan_current_step
                {
                    self.plan_current_step = index.saturating_add(1);
                }
                if self.plan_summary.is_none() {
                    self.plan_summary = self
                        .active_swarm
                        .as_ref()
                        .map(|swarm| swarm.summary.clone())
                        .or_else(|| Some("Plan".into()));
                }
                self.auto_complete_parent_plan_step();
            }
            AgentEvent::PlanStarted { summary, total } => {
                self.plan_steps.clear();
                self.plan_current_step = 0;
                self.plan_total_steps = total;
                self.plan_summary = Some(summary);
                self.plan_warnings.clear();
                self.status_message = format!("Plan ready — {total} steps");
            }
            AgentEvent::PlanCleared => {
                // Keep the completed plan visible on screen; clear it when the user starts a new turn
                self.plan_current_step = self.plan_total_steps;
            }
            AgentEvent::PlanReviewWarnings { warnings } => {
                self.plan_warnings = warnings;
            }
            AgentEvent::FileDiff { path, diff, stats } => {
                self.file_diffs
                    .push(diff_viewer::FileDiffItem::new(path, diff, stats));
            }
            AgentEvent::OptionsNeeded {
                kind,
                title,
                options,
                respond,
            } => {
                self.options_needed = Some(DecisionPrompt {
                    kind,
                    title,
                    options,
                    respond,
                });
                self.selected_option_index = 0;
                self.status_message = format!(
                    "Select with ↑↓ then Enter, or press 1–{}",
                    self.options_needed
                        .as_ref()
                        .map_or(0, |decision| decision.options.len())
                );
            }
            AgentEvent::SwarmStarted {
                run_id,
                summary,
                total,
            } => {
                self.active_swarm = Some(SwarmViewState {
                    run_id: run_id.clone(),
                    summary: summary.clone(),
                    total,
                    running: 0,
                    done: 0,
                    failed: 0,
                    cancelled: 0,
                    status: "running".to_string(),
                    cancel_requested: false,
                    detail_expanded: false,
                    task_statuses: HashMap::new(),
                });
                if self.plan_summary.is_none() {
                    self.plan_summary = Some(summary.clone());
                }
                self.push_activity(format!("swarm {run_id} started: {summary}"));
                self.status_message = format!("Swarm running: {total} agents");
            }
            AgentEvent::SwarmTaskUpdated {
                run_id,
                task_id,
                role,
                status,
                description,
            } => {
                if let Some(swarm) = self.active_swarm.as_mut() {
                    swarm.task_statuses.insert(task_id.clone(), status.clone());
                    recalculate_swarm_counts(swarm);
                    swarm.status = status.clone();
                }
                self.push_activity(format!(
                    "swarm {run_id} task {task_id} [{role}] {status}: {description}"
                ));
                self.status_message = format!("Swarm {role}: {status}");
            }
            AgentEvent::SwarmFinished {
                run_id,
                success,
                summary,
            } => {
                let status = if success { "ok" } else { "error" };
                if let Some(swarm) = self.active_swarm.as_mut() {
                    swarm.status = status.to_string();
                    swarm.running = 0;
                    if success {
                        let terminal = swarm
                            .done
                            .saturating_add(swarm.failed)
                            .saturating_add(swarm.cancelled);
                        if terminal < swarm.total {
                            swarm.done = swarm.done.saturating_add(swarm.total - terminal);
                        }
                        swarm.detail_expanded = false;
                    } else {
                        swarm.detail_expanded = true;
                    }
                }
                if success {
                    for step in &mut self.plan_steps {
                        if matches!(
                            step.status,
                            plan_tracker::PlanStepStatus::Pending
                                | plan_tracker::PlanStepStatus::Running
                        ) {
                            step.transition_to(plan_tracker::PlanStepStatus::Done);
                        }
                    }
                    self.plan_current_step = self.plan_total_steps.max(self.plan_steps.len());
                } else {
                    for step in &mut self.plan_steps {
                        if step.status == plan_tracker::PlanStepStatus::Running {
                            step.transition_to(plan_tracker::PlanStepStatus::Failed);
                        }
                    }
                }
                self.push_activity(format!("swarm {run_id} finished [{status}]"));
                self.status_message = truncate_for_activity(&summary, 120);
            }
            AgentEvent::SubagentStarted {
                agent_id,
                agent_type,
                description,
                is_background,
            } => {
                let tag = if is_background { " [bg]" } else { "" };
                self.push_activity(format!(
                    "subagent {agent_id} [{agent_type}]{tag} started: {description}"
                ));
                self.status_message = format!("Subagent running: {description}");
                let mut card =
                    subagent_cards::SubagentCard::new(&agent_id, &agent_type, &description);
                card.is_background = is_background;
                self.subagents.push(card);
            }
            AgentEvent::SubagentDelta { agent_id, content } => {
                self.add_output_token_estimate(&content);
                if let Some(card) = self.subagents.iter_mut().find(|c| c.agent_id == agent_id) {
                    card.apply_delta(content.clone());
                }
                self.status_message = format!(
                    "Subagent update: {}",
                    truncate_for_activity(content.trim(), 80)
                );
            }
            AgentEvent::SubagentCompleted { agent_id, result } => {
                let status = if result.success { "ok" } else { "error" };
                self.push_activity(format!("subagent {agent_id} completed [{status}]"));
                self.status_message = format!("Subagent done: {}", result.summary);
                if let Some(card) = self.subagents.iter_mut().find(|c| c.agent_id == agent_id) {
                    card.complete(&result);
                }
            }
            AgentEvent::SubagentToolApprovalNeeded {
                agent_id,
                tool_name,
                arguments,
                policy_decision,
                respond,
            } => {
                let short_id: String = agent_id.chars().take(8).collect();
                self.push_activity(format!("subagent {short_id} needs approval: {tool_name}"));
                if policy_decision.action == crate::policy::PolicyAction::Allow {
                    if let Some(card) = self.subagents.iter_mut().find(|c| c.agent_id == agent_id) {
                        card.apply_delta(format!("auto-approved safe tool: {tool_name}"));
                    }
                    let _ = respond.send(true);
                    self.push_activity(format!(
                        "subagent {short_id} auto-approved safe tool: {tool_name}"
                    ));
                    return;
                }
                if policy_decision.action == crate::policy::PolicyAction::Deny {
                    if let Some(card) = self.subagents.iter_mut().find(|c| c.agent_id == agent_id) {
                        card.mark_blocked(&policy_decision.reason);
                    }
                    let _ = respond.send(false);
                    self.status_message = format!(
                        "Subagent tool blocked: {}",
                        truncate_for_activity(&policy_decision.reason, 80)
                    );
                    return;
                }
                if let Some(card) = self.subagents.iter_mut().find(|c| c.agent_id == agent_id) {
                    card.mark_waiting_approval(&tool_name);
                }
                self.enqueue_approval(
                    subagent_tool_approval_display(
                        &agent_id,
                        &tool_name,
                        &arguments,
                        &policy_decision.display,
                    ),
                    respond,
                );
            }
        }
    }

    pub fn push_activity(&mut self, message: impl Into<String>) {
        self.activity_log
            .push(truncate_for_activity(&message.into(), 160));
        if self.activity_log.len() > 80 {
            let extra = self.activity_log.len() - 80;
            self.activity_log.drain(0..extra);
        }
    }

    fn scroll_older(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    fn scroll_newer(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.renderer_mode.uses_inline_viewport() {
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_older(MOUSE_SCROLL_LINES),
            MouseEventKind::ScrollDown => self.scroll_newer(MOUSE_SCROLL_LINES),
            _ => {}
        }
    }

    fn render(&self, f: &mut Frame) {
        theme::set_active_theme(self.theme_mode);
        let area = f.area();
        if self.renderer_mode == RendererMode::Classic {
            self.render_classic(f, area);
            return;
        }

        render_canvas(f, area);
        if self.should_render_minimal_runtime_surface() {
            self.render_minimal_runtime_surface(f, area);
            return;
        }
        let input_h = self.input_height();

        // If file tree is shown, split horizontally
        let (tree_area, main_area) = if self.show_file_tree && area.width > 40 {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(30), Constraint::Min(10)])
                .split(area);
            (Some(chunks[0]), chunks[1])
        } else {
            (None, area)
        };

        let (status_area, body_area, model_hint_area, divider_area, input_area, footer_area) =
            layout::app_layout(main_area, input_h);
        let render_options = self.render_option_state();
        // Reserve space for options panel when orchestrator presents choices.
        let options_h = self.render_option_height(&render_options, 5, 12);

        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(options_h)])
            .split(body_area);
        let content_area = body_chunks[0];
        let options_area = body_chunks[1];

        // Paint every major area explicitly. Some terminals keep their default
        // color in untouched cells, which makes light-theme text look washed out.
        render_canvas(f, status_area);
        render_canvas(f, body_area);
        render_canvas(f, content_area);
        render_canvas(f, options_area);
        render_canvas(f, model_hint_area);
        render_canvas(f, divider_area);
        render_canvas(f, input_area);
        render_canvas(f, footer_area);
        let showing_welcome = self.is_showing_welcome();
        let full_body_area = Rect::new(
            content_area.x,
            status_area.y,
            content_area.width,
            content_area
                .y
                .saturating_add(content_area.height)
                .saturating_sub(status_area.y),
        );

        // Render file tree sidebar
        if let Some(ta) = tree_area {
            file_tree::render_file_tree(f, ta, &self.file_tree);
        }

        // Status bar (minimal). Swarm progress is rendered in the transcript as
        // agent rows, so the status line focuses on live activity.
        if !showing_welcome {
            let stream_elapsed_ms = self.stream_motion_frame().elapsed_ms;
            let up_time_ms = self.ui_started_at.elapsed().as_millis() as u64;
            let idle_title = self.runtime_idle_title();
            let status_activity = if self.is_streaming {
                Some(status_bar::StatusActivity {
                    title: self.current_task_title.as_str(),
                    elapsed_ms: stream_elapsed_ms,
                    input_tokens: self.activity_input_tokens(),
                    tokens: self.current_turn_output_tokens,
                    agent_tokens: self.live_agent_tokens(),
                    thought_seconds: self.stream_start.map_or(0, |started| {
                        thought_seconds_from_reasoning(
                            &self.reasoning_buffer,
                            started.elapsed().as_secs(),
                        )
                    }),
                })
            } else {
                idle_title
                    .as_deref()
                    .map(|title| status_bar::StatusActivity {
                        title,
                        elapsed_ms: up_time_ms,
                        input_tokens: 0,
                        tokens: 0,
                        agent_tokens: 0,
                        thought_seconds: 0,
                    })
            };
            let status_motion = if self.is_streaming {
                self.stream_motion_frame()
            } else {
                motion::MotionFrame::disabled()
            };
            status_bar::render_status_bar(
                f,
                status_area,
                status_bar::StatusBarProps {
                    mode: self.current_mode(),
                    thinking: &self.thinking_mode,
                    activity: status_activity,
                    chinese: self.is_chinese_ui(),
                    motion: status_motion,
                },
            );
        }

        // Empty workbench state: welcome content lives in the same surface as
        // the composer/status UI, not in a separate startup page.
        if self.settings_open {
            settings_panel::render_settings_panel(
                f,
                content_area,
                settings_panel::SettingsPanelProps {
                    selected_tab: self.settings_tab,
                    selected_row: self.settings_selected,
                    active_model: &self.model,
                    active_thinking: &self.thinking_mode,
                    renderer: self.renderer_mode,
                    config: &self.config,
                    theme_label: self.theme_mode.label(),
                },
            );
        } else if showing_welcome {
            welcome::render_welcome(f, full_body_area, &self.welcome);
        } else {
            // Quiet terminal style: everything scrolls together in one stream.
            let runtime = self.runtime_render_state();
            self.render_transcript_surface(f, content_area, &runtime);
        }

        if self.scroll_offset > 0 {
            render_jump_to_bottom_hint(f, content_area);
        }

        // Options panel (between content and activity)
        if options_h > 0 {
            self.render_command_options(f, options_area, &render_options);
        }

        model_hint::render_composer_hint_with_motion(
            f,
            model_hint_area,
            &self.model,
            &self.thinking_mode,
            self.is_streaming.then(|| status_bar::StatusActivity {
                title: &self.current_task_title,
                elapsed_ms: self.stream_motion_frame().elapsed_ms,
                input_tokens: self.activity_input_tokens(),
                tokens: self.current_turn_output_tokens,
                agent_tokens: self.live_agent_tokens(),
                thought_seconds: self.stream_start.map_or(0, |started| {
                    thought_seconds_from_reasoning(
                        &self.reasoning_buffer,
                        started.elapsed().as_secs(),
                    )
                }),
            }),
            self.current_mode(),
            self.stream_motion_frame(),
        );
        if !self.is_streaming {
            if let Some(notice) = self.status_notice() {
                render_status_notice(f, model_hint_area, notice);
            }
        }

        // Divider line above the input.
        {
            let p = theme::palette();
            let line = "─".repeat(divider_area.width as usize);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    line,
                    Style::default().fg(p.divider).bg(p.canvas),
                ))),
                divider_area,
            );
        }

        // Input
        let opts_for_input = self.input_pending_options(&render_options);
        if self.api_key_entry.is_some() {
            input::render_api_key_input_with_motion_and_placeholder(
                f,
                input_area,
                &self.input_text,
                self.cursor_pos,
                self.motion_frame(),
                self.api_key_input_placeholder(),
            );
        } else {
            input::render_input_with_options(
                f,
                input_area,
                &self.input_text,
                self.cursor_pos,
                input::InputRenderOptions {
                    pending_options: opts_for_input,
                    motion: self.motion_frame(),
                    placeholder: self.input_placeholder(),
                    chinese: self.is_chinese_ui(),
                },
            );
        }
        self.render_powerline_footer(f, footer_area);

        // Approval popup (on top of everything)
        if let Some((ref approval, _)) = self.approval {
            approval_popup::render_approval_popup(
                f,
                approval_overlay_area(area, divider_area.y),
                approval,
                self.approval_selected_index,
            );
        }
    }

    fn render_transcript_surface(
        &self,
        f: &mut Frame,
        area: Rect,
        runtime: &RuntimeRenderState<'_>,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if self.should_show_diff_focus_panel() && area.height >= 8 {
            let diff_height = diff_focus_height(area.height);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(diff_height)])
                .split(area);
            self.render_transcript_only(f, chunks[0], runtime);
            self.render_diff_focus_panel(f, chunks[1]);
        } else {
            self.render_transcript_only(f, area, runtime);
        }
    }

    fn render_transcript_only(&self, f: &mut Frame, area: Rect, runtime: &RuntimeRenderState<'_>) {
        let queued_user_messages = self.queued_user_message_refs();
        transcript_view::render_transcript(
            f,
            area,
            transcript_view::TranscriptProps {
                messages: &self.messages,
                pending_user_message: self.pending_user_message.as_deref(),
                queued_user_messages: &queued_user_messages,
                scroll_offset: self.scroll_offset,
                plan_summary: self.plan_summary.as_deref(),
                plan_steps: &self.plan_steps,
                plan_current_step: self.plan_current_step,
                plan_total_steps: self.plan_total_steps,
                plan_warnings: &self.plan_warnings,
                todo_summary: &self.todo_summary,
                todo_items: &self.todo_items,
                subagents: runtime.visible_subagents,
                global_elapsed_ms: runtime.elapsed_ms,
                diffs: &self.file_diffs,
                selected_diff: self.selected_diff,
                is_streaming: self.is_streaming,
                show_streaming_placeholder: false,
                stream_buffer: &self.stream_buffer,
                reasoning_buffer: &self.reasoning_buffer,
                reasoning_elapsed_ms: self.reasoning_elapsed_ms(),
                reasoning_tokens: self.current_turn_reasoning_tokens,
                show_reasoning: self.show_reasoning,
                chinese: self.is_chinese_ui(),
            },
        );
    }

    fn should_show_diff_focus_panel(&self) -> bool {
        self.diff_focused
            && self
                .selected_diff
                .is_some_and(|idx| idx < self.file_diffs.len())
    }

    fn render_diff_focus_panel(&self, f: &mut Frame, area: Rect) {
        let Some(selected) = self.selected_diff else {
            return;
        };
        let Some(item) = self.file_diffs.get(selected) else {
            return;
        };
        if area.width == 0 || area.height < 3 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(2)])
            .split(area);
        let p = theme::palette();
        let hint = if self.is_chinese_ui() {
            "Diff 焦点：↑↓ 文件 · PgUp/PgDn 滚动 · a 接受 · r 拒绝 · d 关闭"
        } else {
            "Diff focus: ↑↓ file · PgUp/PgDn scroll · a accept · r reject · d close"
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                hint,
                Style::default().fg(p.secondary).bg(p.canvas),
            )]))
            .style(Style::default().bg(p.canvas)),
            chunks[0],
        );
        diff_viewer::render_diff_viewer(
            f,
            chunks[1],
            std::slice::from_ref(item),
            Some(0),
            self.diff_scroll,
        );
    }

    fn should_render_minimal_runtime_surface(&self) -> bool {
        !self.settings_open && self.api_key_entry.is_none() && !self.show_file_tree
    }

    fn render_minimal_runtime_surface(&self, f: &mut Frame, area: Rect) {
        let p = theme::palette();
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(p.canvas)),
            area,
        );

        let root = area.inner(Margin {
            horizontal: u16::from(area.width >= 70) * 3,
            vertical: u16::from(area.height >= 10),
        });
        let render_options = self.render_option_state();
        let options_h = self.render_option_height(&render_options, 5, 12);
        let prompt_h = self.minimal_runtime_prompt_height();
        let activity_h = u16::from(self.should_show_minimal_runtime_activity());
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(options_h),
                Constraint::Length(activity_h),
                Constraint::Length(1),
                Constraint::Length(prompt_h),
                Constraint::Length(2),
            ])
            .split(root);

        let runtime = self.runtime_render_state();
        self.render_minimal_runtime_content(f, rows[0], &runtime);
        if self.scroll_offset > 0 {
            render_jump_to_bottom_hint(f, rows[0]);
        }
        if options_h > 0 {
            self.render_command_options(f, rows[1], &render_options);
        }
        self.render_minimal_runtime_activity(f, rows[2]);
        if prompt_h > 0 {
            self.render_minimal_runtime_prompt(f, rows[4], &render_options);
        }
        self.render_powerline_footer(f, rows[5]);
        if let Some((ref approval, _)) = self.approval {
            approval_popup::render_approval_popup(
                f,
                approval_overlay_area(area, rows[3].y),
                approval,
                self.approval_selected_index,
            );
        }
    }

    fn minimal_runtime_prompt_height(&self) -> u16 {
        let line_count = self.input_text.lines().count().max(1) as u16;
        line_count.clamp(1, 3)
    }

    fn should_show_minimal_runtime_activity(&self) -> bool {
        self.is_streaming
            || self.status_notice().is_some()
            || self.ui_started_at.elapsed().as_secs() >= 3
    }

    fn render_minimal_runtime_activity(&self, f: &mut Frame, area: Rect) {
        if area.height == 0 || !self.should_show_minimal_runtime_activity() {
            return;
        }

        if let Some(notice) = self.status_notice() {
            if !self.is_streaming {
                render_status_notice(f, area, notice);
            }
            return;
        }

        if self.is_streaming {
            let mut activity = self.streaming_status_activity();
            activity.title = if self.is_chinese_ui() {
                "思考中"
            } else {
                "Thinking"
            };
            status_bar::render_status_bar(
                f,
                area,
                status_bar::StatusBarProps {
                    mode: self.current_mode(),
                    thinking: &self.thinking_mode,
                    activity: Some(activity),
                    chinese: self.is_chinese_ui(),
                    motion: self.stream_motion_frame(),
                },
            );
            return;
        }

        let up_time_ms = self.ui_started_at.elapsed().as_millis() as u64;
        let Some(idle_title) = self.runtime_idle_title() else {
            return;
        };
        status_bar::render_status_bar(
            f,
            area,
            status_bar::StatusBarProps {
                mode: self.current_mode(),
                thinking: &self.thinking_mode,
                activity: Some(status_bar::StatusActivity {
                    title: &idle_title,
                    elapsed_ms: up_time_ms,
                    input_tokens: 0,
                    tokens: 0,
                    agent_tokens: 0,
                    thought_seconds: 0,
                }),
                chinese: self.is_chinese_ui(),
                motion: self.stream_motion_frame(),
            },
        );
    }

    fn render_minimal_runtime_content(
        &self,
        f: &mut Frame,
        area: Rect,
        runtime: &RuntimeRenderState<'_>,
    ) {
        if area.height == 0 {
            return;
        }
        render_canvas(f, area);
        if self.is_showing_welcome() {
            welcome::render_welcome(f, area, &self.welcome);
            return;
        }
        self.render_transcript_surface(f, area, runtime);
    }

    fn render_minimal_runtime_prompt(
        &self,
        f: &mut Frame,
        area: Rect,
        render_options: &RenderOptionState,
    ) {
        if area.height == 0 {
            return;
        }

        let opts_for_input = self.input_pending_options(render_options);
        input::render_input_with_options(
            f,
            area,
            &self.input_text,
            self.cursor_pos,
            input::InputRenderOptions {
                pending_options: opts_for_input,
                motion: self.motion_frame(),
                placeholder: self.input_placeholder(),
                chinese: self.is_chinese_ui(),
            },
        );
    }

    fn render_command_options(
        &self,
        f: &mut Frame,
        area: Rect,
        render_options: &RenderOptionState,
    ) {
        let mut render_select_popup = false;
        let (kind, title, options) = if let Some(decision) = &self.options_needed {
            render_select_popup = true;
            (
                decision.kind,
                decision.title.as_str(),
                decision.options.as_slice(),
            )
        } else if self.history_search_active {
            (
                DecisionKind::Clarification,
                "History search",
                render_options.history_options.as_slice(),
            )
        } else if let Some((t, opts)) = &self.pending_options {
            render_select_popup = true;
            (DecisionKind::Clarification, t.as_str(), opts.as_slice())
        } else {
            (DecisionKind::Clarification, "", &[][..])
        };
        if !options.is_empty() {
            if render_select_popup {
                if let Some(rich) = self.pending_question_rich.as_ref() {
                    let rich_opts: Vec<select_popup::RichOption> = rich
                        .options
                        .iter()
                        .map(|o| select_popup::RichOption {
                            label: o.label.as_str(),
                            description: o.description.as_str(),
                            preview: o.preview.as_deref(),
                        })
                        .collect();
                    let state = select_popup::RichSelectState {
                        title: rich.title.as_str(),
                        question: rich.question.as_str(),
                        options: &rich_opts,
                        selected_index: self.selected_option_index,
                        multi_select: rich.multi_select,
                        checked: if rich.multi_select {
                            &self.selected_multi_options
                        } else {
                            &[]
                        },
                    };
                    select_popup::render_select_popup_rich(f, area, &state);
                } else {
                    select_popup::render_select_popup(
                        f,
                        area,
                        title,
                        options,
                        self.selected_option_index,
                    );
                }
            } else {
                plan_tracker::render_options_panel(
                    f,
                    area,
                    kind,
                    title,
                    options,
                    self.selected_option_index,
                );
            }
        } else if render_options.shell_hint_active {
            plan_tracker::render_shell_hint_panel(f, area, self.is_chinese_ui());
        } else if !render_options.slash_suggestions.is_empty() {
            plan_tracker::render_slash_command_panel(
                f,
                area,
                &render_options.slash_suggestions,
                self.selected_slash_index,
                self.is_chinese_ui(),
            );
        } else if !render_options.file_mention_suggestions.is_empty() {
            plan_tracker::render_file_mention_panel(
                f,
                area,
                &render_options.file_mention_suggestions,
                self.selected_file_mention_index,
                self.is_chinese_ui(),
            );
        }
    }

    fn render_classic(&self, f: &mut Frame, area: Rect) {
        render_canvas(f, area);
        if self.should_render_minimal_runtime_surface() {
            self.render_minimal_runtime_surface(f, area);
            return;
        }

        let input_h = self.input_height();
        let render_options = self.render_option_state();
        let options_h = self.render_option_height(&render_options, 4, 10);
        let activity_h = u16::from(self.should_show_minimal_runtime_activity());
        let inner = area.inner(Margin {
            horizontal: u16::from(area.width >= 60),
            vertical: 0,
        });
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(options_h),
                Constraint::Length(activity_h),
                Constraint::Length(1),
                Constraint::Length(input_h),
                Constraint::Length(1),
            ])
            .split(inner);
        let content_area = chunks[0];
        let options_area = chunks[1];
        let activity_area = chunks[2];
        let top_divider_area = chunks[3];
        let input_area = chunks[4];
        let _bottom_divider_area = chunks[5];

        let showing_welcome = self.is_showing_welcome();
        if self.settings_open {
            settings_panel::render_settings_panel(
                f,
                content_area,
                settings_panel::SettingsPanelProps {
                    selected_tab: self.settings_tab,
                    selected_row: self.settings_selected,
                    active_model: &self.model,
                    active_thinking: &self.thinking_mode,
                    renderer: self.renderer_mode,
                    config: &self.config,
                    theme_label: self.theme_mode.label(),
                },
            );
        } else if showing_welcome {
            welcome::render_welcome(f, content_area, &self.welcome);
        } else {
            let runtime = self.runtime_render_state();
            self.render_transcript_surface(f, content_area, &runtime);
        }
        if self.scroll_offset > 0 {
            render_jump_to_bottom_hint(f, content_area);
        }

        if options_h > 0 {
            self.render_command_options(f, options_area, &render_options);
        }

        if self.is_streaming {
            status_bar::render_status_bar(
                f,
                activity_area,
                status_bar::StatusBarProps {
                    mode: self.current_mode(),
                    thinking: &self.thinking_mode,
                    activity: Some(status_bar::StatusActivity {
                        title: &self.current_task_title,
                        elapsed_ms: self.stream_motion_frame().elapsed_ms,
                        input_tokens: self.activity_input_tokens(),
                        tokens: self.current_turn_output_tokens,
                        agent_tokens: self.live_agent_tokens(),
                        thought_seconds: self.stream_start.map_or(0, |started| {
                            thought_seconds_from_reasoning(
                                &self.reasoning_buffer,
                                started.elapsed().as_secs(),
                            )
                        }),
                    }),
                    chinese: self.is_chinese_ui(),
                    motion: self.stream_motion_frame(),
                },
            );
        } else if let Some(notice) = self.status_notice() {
            render_status_notice(f, activity_area, notice);
        } else if let Some(idle_title) = self.runtime_idle_title() {
            status_bar::render_status_bar(
                f,
                activity_area,
                status_bar::StatusBarProps {
                    mode: self.current_mode(),
                    thinking: &self.thinking_mode,
                    activity: Some(status_bar::StatusActivity {
                        title: &idle_title,
                        elapsed_ms: self.ui_started_at.elapsed().as_millis() as u64,
                        input_tokens: 0,
                        tokens: 0,
                        agent_tokens: 0,
                        thought_seconds: 0,
                    }),
                    chinese: self.is_chinese_ui(),
                    motion: motion::MotionFrame::disabled(),
                },
            );
        }

        let opts_for_input = self.input_pending_options(&render_options);
        if self.api_key_entry.is_some() {
            input::render_api_key_input_with_motion_and_placeholder(
                f,
                input_area,
                &self.input_text,
                self.cursor_pos,
                self.motion_frame(),
                self.api_key_input_placeholder(),
            );
        } else {
            input::render_input_with_options(
                f,
                input_area,
                &self.input_text,
                self.cursor_pos,
                input::InputRenderOptions {
                    pending_options: opts_for_input,
                    motion: self.motion_frame(),
                    placeholder: self.input_placeholder(),
                    chinese: self.is_chinese_ui(),
                },
            );
        }

        if let Some((ref approval, _)) = self.approval {
            approval_popup::render_approval_popup(
                f,
                approval_overlay_area(area, top_divider_area.y),
                approval,
                self.approval_selected_index,
            );
        }
    }

    fn render_powerline_footer(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let status = if self.is_streaming {
            "working"
        } else if self.api_key_state == ApiKeyState::Missing {
            "api needed"
        } else if self.total_tokens > 0 {
            "done"
        } else {
            "ready"
        };
        let status_label = if self.todo_summary.is_empty() {
            status.to_string()
        } else {
            format!("{status} · {}", self.todo_summary.compact_line())
        };
        let permissions = if self.session_auto_approve {
            "bypass permissions on"
        } else {
            "permissions ask"
        };
        let provider_label = self.config.provider.default.as_str();
        let model_label = request_model_name_for_config(&self.config.provider, &self.model);

        statusline::render_statusline(
            f,
            area,
            statusline::StatuslineProps {
                mode: self.current_mode(),
                provider: provider_label,
                model: &model_label,
                status: &status_label,
                tokens: if self.ui_started_at.elapsed().as_millis() < 3_000 {
                    0
                } else {
                    self.visible_context_tokens()
                },
                input_tokens: self.visible_input_tokens(),
                output_tokens: self.current_turn_output_tokens,
                agent_tokens: self.live_agent_tokens(),
                cost: self.total_cost,
                cache: self.cache.as_ref(),
                permissions,
                context_limit: Some(self.effective_context_budget_tokens()),
                chinese: self.is_chinese_ui(),
            },
        );
    }

    fn effective_context_budget_tokens(&self) -> u64 {
        // Display the model's actual context window when known, not the local
        // retrieval assembly cap. Falling back to the assembly budget only when
        // the provider doesn't declare a window keeps the statusline honest.
        crate::provider::model_context_window_tokens(self.config.provider.default, &self.model)
            .unwrap_or_else(|| {
                context_budget_for(
                    self.config.provider.default,
                    &self.model,
                    self.config.search.max_context_tokens,
                )
                .effective_budget_tokens
            })
    }

    fn streaming_status_activity(&self) -> status_bar::StatusActivity<'_> {
        status_bar::StatusActivity {
            title: &self.current_task_title,
            elapsed_ms: self.stream_motion_frame().elapsed_ms,
            input_tokens: self.activity_input_tokens(),
            tokens: self.current_turn_output_tokens,
            agent_tokens: self.live_agent_tokens(),
            thought_seconds: self.stream_start.map_or(0, |started| {
                thought_seconds_from_reasoning(&self.reasoning_buffer, started.elapsed().as_secs())
            }),
        }
    }

    fn reasoning_elapsed_ms(&self) -> u64 {
        self.stream_start
            .map_or(0, |started| started.elapsed().as_millis() as u64)
    }

    fn activity_input_tokens(&self) -> u64 {
        self.animated_input_tokens()
    }

    fn visible_input_tokens(&self) -> u64 {
        if self.is_streaming {
            self.animated_input_tokens()
        } else if self.current_turn_usage_finalized {
            self.current_turn_input_tokens
        } else {
            0
        }
    }

    fn animated_input_tokens(&self) -> u64 {
        let target = self.current_turn_input_tokens;
        if target == 0 {
            return 0;
        }
        if !self.is_streaming {
            return target;
        }
        let elapsed_ms = self
            .input_token_animation_started
            .or(self.stream_start)
            .map_or(0, |started| started.elapsed().as_millis() as u64);
        animated_token_count(target, elapsed_ms)
    }

    fn live_agent_tokens(&self) -> u64 {
        self.subagents
            .iter()
            .map(|card| card.token_usage)
            .sum::<u64>()
    }

    fn visible_context_tokens(&self) -> u64 {
        if self.is_streaming && !self.current_turn_usage_finalized {
            self.total_tokens.saturating_add(self.current_turn_tokens)
        } else {
            self.total_tokens
        }
    }

    fn auto_complete_parent_plan_step(&mut self) {
        let Some(parent_index) = self.infer_pending_parent_step_index() else {
            return;
        };
        let child_failed = self.plan_steps.iter().enumerate().any(|(idx, step)| {
            idx != parent_index && step.status == plan_tracker::PlanStepStatus::Failed
        });
        let status = if child_failed {
            plan_tracker::PlanStepStatus::Failed
        } else {
            plan_tracker::PlanStepStatus::Done
        };
        self.plan_steps[parent_index].transition_to(status);
    }

    fn infer_pending_parent_step_index(&self) -> Option<usize> {
        let mut pending_index = None;
        for (idx, step) in self.plan_steps.iter().enumerate() {
            if step.status != plan_tracker::PlanStepStatus::Pending {
                continue;
            }
            if pending_index.replace(idx).is_some() {
                return None;
            }
        }
        let pending_index = pending_index?;
        if self.plan_steps.iter().enumerate().any(|(idx, step)| {
            idx != pending_index && step.status == plan_tracker::PlanStepStatus::Running
        }) {
            return None;
        }
        let has_completed_child = self.plan_steps.iter().enumerate().any(|(idx, step)| {
            idx != pending_index && step.status == plan_tracker::PlanStepStatus::Done
        });
        if !has_completed_child {
            return None;
        }
        let all_children_terminal = self.plan_steps.iter().enumerate().all(|(idx, step)| {
            idx == pending_index
                || matches!(
                    step.status,
                    plan_tracker::PlanStepStatus::Done | plan_tracker::PlanStepStatus::Failed
                )
        });
        if !all_children_terminal {
            return None;
        }
        let pending_step = &self.plan_steps[pending_index];
        let matches_summary = self.plan_summary.as_deref().is_some_and(|summary| {
            normalized_plan_title(summary) == normalized_plan_title(&pending_step.description)
        });
        ((pending_index == 0) || matches_summary).then_some(pending_index)
    }
}

fn recalculate_swarm_counts(swarm: &mut SwarmViewState) {
    swarm.running = 0;
    swarm.done = 0;
    swarm.failed = 0;
    swarm.cancelled = 0;
    for status in swarm.task_statuses.values() {
        match status.as_str() {
            "running" => swarm.running += 1,
            "done" => swarm.done += 1,
            "failed" => swarm.failed += 1,
            "cancelled" => swarm.cancelled += 1,
            _ => {}
        }
    }
}

fn plan_steps_hash(steps: &[plan_tracker::PlanStepItem]) -> u64 {
    let mut hasher = DefaultHasher::new();
    steps.len().hash(&mut hasher);
    for step in steps {
        step.description.hash(&mut hasher);
        plan_step_status_tag(step.status).hash(&mut hasher);
        step.duration_ms.hash(&mut hasher);
    }
    hasher.finish()
}

fn plan_step_status_tag(status: plan_tracker::PlanStepStatus) -> u8 {
    match status {
        plan_tracker::PlanStepStatus::Pending => 0,
        plan_tracker::PlanStepStatus::Running => 1,
        plan_tracker::PlanStepStatus::Done => 2,
        plan_tracker::PlanStepStatus::Failed => 3,
    }
}

fn subagents_hash(cards: &[subagent_cards::SubagentCard]) -> u64 {
    let mut hasher = DefaultHasher::new();
    cards.len().hash(&mut hasher);
    for card in cards {
        card.agent_id.hash(&mut hasher);
        card.agent_type.hash(&mut hasher);
        card.description.hash(&mut hasher);
        subagent_status_tag(&card.status).hash(&mut hasher);
        card.last_update.hash(&mut hasher);
        card.summary.hash(&mut hasher);
        card.duration_ms.hash(&mut hasher);
        card.files_read.hash(&mut hasher);
        card.files_written.hash(&mut hasher);
        card.token_usage.hash(&mut hasher);
        card.is_background.hash(&mut hasher);
    }
    hasher.finish()
}

fn subagent_status_tag(status: &subagent_cards::SubagentCardStatus) -> u8 {
    match status {
        subagent_cards::SubagentCardStatus::Running => 0,
        subagent_cards::SubagentCardStatus::WaitingApproval => 1,
        subagent_cards::SubagentCardStatus::Retrying => 2,
        subagent_cards::SubagentCardStatus::Done => 3,
        subagent_cards::SubagentCardStatus::Failed => 4,
        subagent_cards::SubagentCardStatus::Blocked => 5,
        subagent_cards::SubagentCardStatus::Cancelled => 6,
        subagent_cards::SubagentCardStatus::Skipped => 7,
    }
}

fn active_swarm_hash(swarm: Option<&SwarmViewState>) -> u64 {
    swarm.map_or(0, |swarm| {
        stable_hash(&(
            swarm.run_id.as_str(),
            swarm.summary.as_str(),
            swarm.total,
            swarm.running,
            swarm.done,
            swarm.failed,
            swarm.cancelled,
            swarm.status.as_str(),
            swarm.cancel_requested,
            swarm.detail_expanded,
        ))
    })
}

/// Actions sent from TUI to the async task handler.
pub enum TuiAction {
    Submit(String),
    SaveApiKey {
        api_key: String,
        pending_prompt: Option<String>,
    },
    LocalToolResult {
        command: String,
        output: String,
        is_error: bool,
    },
    SideOutput {
        label: String,
        output: String,
        is_error: bool,
    },
    ApproveOnce,
    ApproveSession,
    Deny,
    Interrupt,
    ResumeSession,
    ShowTranscript,
}

fn approval_overlay_area(area: Rect, lower_boundary_y: u16) -> Rect {
    let bottom = lower_boundary_y
        .min(area.bottom())
        .max(area.y.saturating_add(1));
    Rect::new(area.x, area.y, area.width, bottom.saturating_sub(area.y))
}

/// Non-interactive states used by `preview-tui` for dynamic layout checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSnapshotScenario {
    Welcome,
    Workbench,
    Slash,
    Approval,
    Settings,
    Diff,
    History,
    FileMention,
}

/// Render the TUI into plain text for local visual inspection without opening
/// an interactive terminal window.
pub fn render_preview_snapshot(
    project_root: PathBuf,
    api_key_missing: bool,
    width: u16,
    height: u16,
    scenario: PreviewSnapshotScenario,
    theme_mode: theme::ThemeMode,
    elapsed_ms: u64,
) -> Result<String, anyhow::Error> {
    let startup = TuiStartupData::preview(!api_key_missing);
    let mut app = TuiApp::new_with_startup(
        DeepSeekModel::Flash,
        ThinkingMode::Auto,
        None,
        project_root,
        startup,
    );
    app.set_theme_mode(theme_mode);
    app.ui_started_at = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(elapsed_ms))
        .unwrap_or_else(std::time::Instant::now);
    if api_key_missing {
        app.set_api_key_state(ApiKeyState::Missing);
        app.api_key_entry = Some(ApiKeyEntry::default());
        app.status_message = "Enter your provider API key to start".into();
    } else {
        app.set_api_key_state(ApiKeyState::Ready);
        app.api_key_entry = None;
        app.status_message = "Ready".into();
    }

    match scenario {
        PreviewSnapshotScenario::Welcome => {}
        PreviewSnapshotScenario::Workbench => populate_workbench_preview(&mut app, elapsed_ms),
        PreviewSnapshotScenario::Slash => {
            seed_preview_transcript(
                &mut app,
                "打开命令面板",
                "输入 / 后展示可用命令和补全提示。",
            );
            app.input_text = "/".into();
            app.cursor_pos = app.input_text.chars().count();
            app.selected_slash_index = 1;
            app.status_message = "Slash command suggestions".into();
        }
        PreviewSnapshotScenario::Approval => {
            seed_preview_transcript(&mut app, "运行测试", "run_command 需要用户确认后才会执行。");
            let (tx, _rx) = tokio::sync::oneshot::channel();
            app.approval = Some((
                ApprovalDisplay {
                    title: "run_command".into(),
                    description: "Execute cargo test --all-features in the workspace".into(),
                    risk_level: crate::policy::RiskLevel::CommandExecution,
                    details:
                        "Command: cargo test --all-features\nScope: project workspace\nSource: main"
                            .into(),
                },
                tx,
            ));
            app.approval_selected_index = 0;
            app.status_message = "Approval requested".into();
        }
        PreviewSnapshotScenario::Settings => {
            set_preview_ready(&mut app);
            app.settings_open = true;
            app.settings_tab = settings_panel::SettingsTab::Safety;
            app.settings_selected = 3;
            app.status_message = "Settings preview".into();
        }
        PreviewSnapshotScenario::Diff => {
            seed_preview_transcript(
                &mut app,
                "查看 diff",
                "Diff focus 展开当前文件，仅标记接受或拒绝状态。",
            );
            app.file_diffs = vec![
                diff_viewer::FileDiffItem::new(
                    "src/tui/app.rs",
                    "--- a/src/tui/app.rs\n+++ b/src/tui/app.rs\n@@ -1,3 +1,4 @@\n fn render() {\n-    old_summary();\n+    render_diff_focus();\n+    render_key_hints();\n }",
                    "+2 -1",
                ),
                diff_viewer::FileDiffItem::new(
                    "tests/tui_cli_tests.rs",
                    "--- a/tests/tui_cli_tests.rs\n+++ b/tests/tui_cli_tests.rs\n@@ -10,2 +10,3 @@\n+assert!(stdout.contains(\"Diff focus\"));",
                    "+1 -0",
                ),
            ];
            app.selected_diff = Some(0);
            app.diff_focused = true;
            app.status_message = "Diff focus preview".into();
        }
        PreviewSnapshotScenario::History => {
            seed_preview_transcript(
                &mut app,
                "复用历史输入",
                "Ctrl-R 打开历史搜索并筛选最近命令。",
            );
            app.history_search_active = true;
            app.history_search_draft = "cargo".into();
            app.input_text = "cargo".into();
            app.cursor_pos = app.input_text.chars().count();
            app.input_history = vec![
                "octo doctor --no-network".into(),
                "cargo check --all-targets --all-features".into(),
                "cargo test --all-features".into(),
            ];
            app.status_message = "History search".into();
        }
        PreviewSnapshotScenario::FileMention => {
            seed_preview_transcript(&mut app, "引用文件", "@path 面板展示当前工作区文件补全。");
            app.input_text = "检查 @src/tui/".into();
            app.cursor_pos = app.input_text.chars().count();
            app.selected_file_mention_index = 1;
            app.status_message = "File mention suggestions".into();
        }
    }

    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| app.render(f))?;
    Ok(buffer_to_text(terminal.backend()))
}

fn set_preview_ready(app: &mut TuiApp) {
    app.api_key_entry = None;
    app.set_api_key_state(ApiKeyState::Ready);
}

fn preview_instant(elapsed_ms: u64) -> std::time::Instant {
    std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(elapsed_ms))
        .unwrap_or_else(std::time::Instant::now)
}

fn seed_preview_transcript(app: &mut TuiApp, user: &str, assistant: &str) {
    set_preview_ready(app);
    let turn_id = uuid::Uuid::new_v4();
    app.messages = vec![
        ProtocolMessage {
            id: uuid::Uuid::new_v4(),
            role: Role::User,
            content: MessageContent::from(user),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id,
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        },
        ProtocolMessage {
            id: uuid::Uuid::new_v4(),
            role: Role::Assistant,
            content: MessageContent::from(assistant),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id,
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        },
    ];
}

fn populate_workbench_preview(app: &mut TuiApp, elapsed_ms: u64) {
    set_preview_ready(app);
    app.is_streaming = true;
    app.show_reasoning = true;
    app.stream_start = Some(preview_instant(elapsed_ms));
    let turn_id = uuid::Uuid::new_v4();
    app.messages = vec![
        ProtocolMessage {
            id: uuid::Uuid::new_v4(),
            role: Role::User,
            content: MessageContent::from("整理系统运行流畅度"),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id,
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        },
        ProtocolMessage {
            id: uuid::Uuid::new_v4(),
            role: Role::Assistant,
            content: MessageContent::from("先查一下当前的开机自启项目。"),
            reasoning_content: None,
            tool_calls: vec![ToolCall {
                id: "preview_run_command".to_string(),
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name: "run_command".to_string(),
                    arguments: serde_json::json!({
                        "command": "Get-ItemProperty HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run"
                    })
                    .to_string(),
                },
            }],
            tool_results: Vec::new(),
            turn_id,
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        },
    ];
    app.stream_buffer = String::new();
    app.reasoning_buffer = "Map runtime state -> fan out agents -> synthesize UI changes".into();
    app.status_message = "Working across plan, agents, and tools".into();
    app.current_task_title = "优化多智能体任务控制台".into();
    app.total_tokens = 578;
    app.current_turn_tokens = 578;
    app.current_turn_input_tokens = 41;
    app.current_turn_output_tokens = 131;
    app.plan_summary = Some("优化多智能体任务控制台".into());
    app.plan_current_step = 1;
    app.plan_total_steps = 4;
    app.plan_steps = vec![
        plan_tracker::PlanStepItem::new(
            "检查当前 plan / agent 渲染路径",
            plan_tracker::PlanStepStatus::Done,
        )
        .with_duration_ms(1_200),
        plan_tracker::PlanStepItem::new(
            "重排 Mission Control 和 Agent Team",
            plan_tracker::PlanStepStatus::Running,
        ),
        plan_tracker::PlanStepItem::new(
            "对标主流智能体工作台（含 OpenCode / Manus）",
            plan_tracker::PlanStepStatus::Pending,
        ),
        plan_tracker::PlanStepItem::new(
            "预览截图并验证输入区",
            plan_tracker::PlanStepStatus::Pending,
        ),
    ];
    let mut explorer = subagent_cards::SubagentCard::new(
        "019e0c78-8006",
        "code-explorer",
        "Trace plan and agent render paths",
    );
    explorer.status = subagent_cards::SubagentCardStatus::Done;
    explorer.summary = Some("Located transcript, plan tracker, and subagent card paths".into());
    explorer.duration_ms = Some(1_250);
    explorer.files_read = 3;
    let mut reviewer = subagent_cards::SubagentCard::new(
        "019e0c78-7fe3",
        "code-reviewer",
        "Review multi-agent UI against 8 competitors",
    );
    reviewer.apply_delta("checking Mission Control density and task visibility");
    let mut planner = subagent_cards::SubagentCard::new(
        "019e0c78-81a4",
        "planner",
        "Plan next UI pass for real swarm runs",
    );
    planner.apply_delta("mapping agent lanes to plan steps");
    app.subagents = vec![explorer, reviewer, planner];
    app.push_activity("dynamic preview: workbench state");
}

fn buffer_to_text(backend: &ratatui::backend::TestBackend) -> String {
    let width = backend.buffer().area.width as usize;
    backend
        .buffer()
        .content()
        .chunks(width)
        .map(|line| {
            line_symbols_to_text(line.iter().map(ratatui::buffer::Cell::symbol))
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_symbols_to_text<'a>(symbols: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::new();
    let mut skip_cells = 0usize;
    for symbol in symbols {
        if skip_cells > 0 {
            skip_cells -= 1;
            continue;
        }
        text.push_str(symbol);
        let width = snapshot_symbol_width(symbol);
        skip_cells = width.saturating_sub(1);
    }
    text
}

fn snapshot_symbol_width(symbol: &str) -> usize {
    symbol
        .chars()
        .map(|ch| if is_wide_snapshot_char(ch) { 2 } else { 1 })
        .sum::<usize>()
        .max(1)
}

fn is_wide_snapshot_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115F
            | 0x2329..=0x232A
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
            | 0x20000..=0x3FFFD
    )
}

struct RunningTurn {
    events: mpsc::UnboundedReceiver<AgentEvent>,
    handle: JoinHandle<(Orchestrator, Option<String>)>,
    cancel_token: Arc<AtomicBool>,
}

#[derive(Default)]
struct RenderInvalidation {
    throttled_stream: bool,
    immediate: bool,
}

impl RenderInvalidation {
    fn observe_agent_event(&mut self, event: &AgentEvent) {
        if agent_event_can_throttle_render(event) {
            self.throttled_stream = true;
        } else {
            self.immediate = true;
        }
    }

    fn apply(self, force_render: &mut bool, pending_stream_draw: &mut bool) {
        if self.immediate {
            *force_render = true;
            *pending_stream_draw = false;
        } else if self.throttled_stream && !*force_render {
            *pending_stream_draw = true;
        }
    }
}

fn agent_event_can_throttle_render(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::ContentDelta(_)
            | AgentEvent::ReasoningDelta(_)
            | AgentEvent::TokenDelta { .. }
            | AgentEvent::SubagentDelta { .. }
    )
}

struct TerminalSession {
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalEnvironment {
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    term: Option<String>,
    /// Terminal embedded in another host that emulates a TTY but does not
    /// answer cursor-position probes (CSI 6 n). Controlled by explicit opt-in.
    embedded_host: bool,
}

/// Terminals that emulate a TTY but don't answer CSI 6 n cursor-position
/// probes can opt into the classic renderer with `OCTO_EMBEDDED_HOST=1`.
const EMBEDDED_HOST_ENV_VARS: &[&str] = &["OCTO_EMBEDDED_HOST"];

impl TerminalEnvironment {
    fn current() -> Self {
        Self {
            stdin_is_terminal: io::stdin().is_terminal(),
            stdout_is_terminal: io::stdout().is_terminal(),
            term: std::env::var("TERM").ok(),
            embedded_host: EMBEDDED_HOST_ENV_VARS
                .iter()
                .any(|var| std::env::var_os(var).is_some()),
        }
    }

    fn ensure_tui_supported(&self) -> Result<(), anyhow::Error> {
        if self.stdin_is_terminal && self.stdout_is_terminal {
            return Ok(());
        }

        let stdin = if self.stdin_is_terminal {
            "tty"
        } else {
            "not a tty"
        };
        let stdout = if self.stdout_is_terminal {
            "tty"
        } else {
            "not a tty"
        };
        let term = self.term.as_deref().unwrap_or("<unset>");
        anyhow::bail!(
            "TUI requires an interactive terminal (stdin: {stdin}, stdout: {stdout}, TERM: {term}). \
Run `octo` from a real terminal/PTY, or use `octo preview-tui` for a non-interactive snapshot."
        );
    }

    fn skips_cursor_position_probe(&self) -> bool {
        self.embedded_host
            || match self.term.as_deref() {
                Some(term) => {
                    let normalized = term.trim().to_ascii_lowercase();
                    normalized.is_empty() || normalized == "dumb"
                }
                None => true,
            }
    }
}

impl TerminalSession {
    fn enter(_renderer: RendererMode) -> Result<Self, anyhow::Error> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, Hide, EnableBracketedPaste,)?;
        stdout.flush()?;
        Ok(Self { active: true })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.active {
            let mut stdout = io::stdout();
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = execute!(
                stdout,
                DisableBracketedPaste,
                DisableMouseCapture,
                Show,
                SetAttribute(Attribute::Reset),
            );
            reset_terminal_scroll_region(&mut stdout);
            let _ = execute!(stdout, Clear(ClearType::CurrentLine));
            let _ = write!(stdout, "\x1b[0m\x1b[39m\x1b[49m");
            let _ = stdout.flush();
        }
    }
}

fn reset_terminal_scroll_region(stdout: &mut impl Write) {
    // Ratatui inline/fixed viewports may leave DECSTBM scroll margins active in
    // some terminals. Reset them before handing the screen back to the shell.
    let _ = write!(stdout, "\x1b[r\x1b[?7h");
}

fn classic_viewport_height() -> u16 {
    let terminal_height = crossterm::terminal::size()
        .map(|(_, height)| height)
        .unwrap_or(24);
    classic_viewport_height_for_terminal(terminal_height)
}

fn classic_viewport_height_for_terminal(terminal_height: u16) -> u16 {
    terminal_height
        .saturating_div(2)
        .saturating_add(2)
        .clamp(12, 22)
}

fn tui_terminal(
    renderer: RendererMode,
) -> Result<Terminal<CrosstermBackend<std::io::Stdout>>, anyhow::Error> {
    let backend = CrosstermBackend::new(std::io::stdout());
    if renderer.uses_inline_viewport() {
        if TerminalEnvironment::current().skips_cursor_position_probe() {
            return Ok(Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Fixed(full_terminal_viewport_area()),
                },
            )?);
        }
        let (width, height) = crossterm::terminal::size().unwrap_or((100, 24));
        match cursor_position() {
            Ok((_, cursor_y)) => {
                if let Some(area) =
                    classic_fixed_viewport_area_for_terminal(width, height, cursor_y)
                {
                    return Ok(Terminal::with_options(
                        backend,
                        TerminalOptions {
                            viewport: Viewport::Fixed(area),
                        },
                    )?);
                }
            }
            Err(_) => {
                return Ok(Terminal::with_options(
                    backend,
                    TerminalOptions {
                        viewport: Viewport::Fixed(full_terminal_viewport_area_for_terminal(
                            width, height,
                        )),
                    },
                )?);
            }
        }
        Ok(Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(classic_viewport_height()),
            },
        )?)
    } else {
        Ok(Terminal::new(backend)?)
    }
}

fn full_terminal_viewport_area() -> Rect {
    let (width, height) = crossterm::terminal::size().unwrap_or((100, 24));
    full_terminal_viewport_area_for_terminal(width, height)
}

fn full_terminal_viewport_area_for_terminal(width: u16, height: u16) -> Rect {
    Rect::new(0, 0, width.max(1), height.max(1))
}

fn classic_fixed_viewport_area_for_terminal(
    width: u16,
    height: u16,
    cursor_y: u16,
) -> Option<Rect> {
    let y = cursor_y.min(height.saturating_sub(1));
    let available = height.saturating_sub(y);
    if available < 18 {
        return None;
    }
    Some(Rect::new(0, y, width, available))
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or("")
}

fn compact_token_label(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn subagent_tool_approval_display(
    agent_id: &str,
    tool_name: &str,
    arguments: &str,
    policy_display: &ApprovalDisplay,
) -> ApprovalDisplay {
    let short_id: String = agent_id.chars().take(8).collect();
    let (target_label, target_value) = approval_target_summary(tool_name, arguments);
    let title = match policy_display.risk_level {
        crate::policy::RiskLevel::SensitiveRead => "子 agent 请求敏感读取",
        crate::policy::RiskLevel::WriteProject => "子 agent 请求写入项目",
        crate::policy::RiskLevel::GitMutation => "子 agent 请求 Git 修改",
        crate::policy::RiskLevel::CommandExecution => "子 agent 请求执行命令",
        crate::policy::RiskLevel::NetworkAccess => "子 agent 请求网络访问",
        crate::policy::RiskLevel::Blocked => "子 agent 请求已阻止",
        crate::policy::RiskLevel::SafeRead => "子 agent 请求读取",
    };

    let mut details = vec![
        format!("来源: 子 agent {short_id}"),
        format!("工具: {tool_name}"),
    ];
    if !target_value.trim().is_empty() {
        details.push(format!("{target_label}: {target_value}"));
    }
    if tool_name == "list_dir"
        && serde_json::from_str::<serde_json::Value>(arguments)
            .ok()
            .and_then(|value| value["recursive"].as_bool())
            .unwrap_or(false)
    {
        details.push("范围: 递归".to_string());
    }

    ApprovalDisplay {
        title: title.to_string(),
        description: approval_description_for_tool(tool_name, &policy_display.risk_level),
        risk_level: policy_display.risk_level.clone(),
        details: details.join("\n"),
    }
}

fn approval_description_for_tool(tool_name: &str, risk: &crate::policy::RiskLevel) -> String {
    match (tool_name, risk) {
        ("run_command", _) => "子 agent 需要执行本地命令".to_string(),
        ("read_file" | "list_dir", crate::policy::RiskLevel::SensitiveRead) => {
            "子 agent 需要读取工作区外或敏感路径".to_string()
        }
        ("read_file" | "list_dir", _) => "子 agent 需要读取项目文件".to_string(),
        ("write_file" | "edit_file" | "notebook_edit" | "apply_patch", _) => {
            "子 agent 需要修改项目文件".to_string()
        }
        ("fetch_url" | "web_search", _) => "子 agent 需要访问网络".to_string(),
        _ => "子 agent 需要执行工具调用".to_string(),
    }
}

fn approval_target_summary(tool_name: &str, arguments: &str) -> (&'static str, String) {
    let value = serde_json::from_str::<serde_json::Value>(arguments).unwrap_or_default();
    let target = match tool_name {
        "run_command" => value["command"].as_str(),
        "read_file" | "list_dir" | "write_file" | "edit_file" | "notebook_edit" => {
            value["path"].as_str()
        }
        "search_files" | "search_code" | "semantic_search" => value["query"].as_str(),
        "fetch_url" => value["url"].as_str(),
        _ => None,
    };
    let label = match tool_name {
        "run_command" => "命令",
        "fetch_url" => "网址",
        "search_files" | "search_code" | "semantic_search" => "查询",
        _ => "路径",
    };
    let summary = target
        .map(|value| truncate_for_activity(value, 160))
        .unwrap_or_else(|| truncate_for_activity(arguments, 160));
    (label, summary)
}

fn estimate_tokens(value: &str) -> u64 {
    if value.trim().is_empty() {
        return 0;
    }

    let mut ascii_chars = 0u64;
    let mut non_ascii_chars = 0u64;
    for ch in value.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_ascii() {
            ascii_chars += 1;
        } else {
            non_ascii_chars += 1;
        }
    }

    let ascii_tokens = ascii_chars.div_ceil(4);
    ascii_tokens.saturating_add(non_ascii_chars).max(1)
}

fn shell_command_from_input(input: &str) -> Result<Option<String>, &'static str> {
    let trimmed = input.trim();
    let Some(command) = trimmed.strip_prefix('!') else {
        return Ok(None);
    };

    let command = command.trim();
    if command.is_empty() {
        Err("Shell mode needs a command after !")
    } else {
        Ok(Some(command.to_string()))
    }
}

fn is_swarm_cancel_command(input: &str) -> bool {
    let mut parts = input.split_whitespace();
    matches!(parts.next(), Some("/swarm" | "/cluster")) && matches!(parts.next(), Some("cancel"))
}

fn shell_tool_arguments(command: &str) -> String {
    serde_json::json!({ "command": command }).to_string()
}

fn shell_tool_call(command: &str) -> crate::deepseek::ToolCall {
    crate::deepseek::ToolCall {
        id: format!("shell_{}", uuid::Uuid::new_v4()),
        call_type: "function".to_string(),
        function: crate::deepseek::ToolCallFunction {
            name: "run_command".to_string(),
            arguments: shell_tool_arguments(command),
        },
    }
}

fn start_local_shell_command(
    app: &mut TuiApp,
    root: &std::path::Path,
    command: String,
    action_tx: &mpsc::UnboundedSender<TuiAction>,
    auto_approve: bool,
) -> Option<JoinHandle<()>> {
    let policy_config = crate::storage::Config::load(Some(root))
        .map(|config| config.policy)
        .unwrap_or_default();
    let tool_call = shell_tool_call(&command);
    let decision = crate::policy::evaluate_tool(
        "run_command",
        &tool_call.function.arguments,
        root,
        &policy_config,
    );

    if matches!(decision.action, crate::policy::PolicyAction::Deny) {
        app.show_local_output(format!("$ {command}\n\nBlocked: {}", decision.reason));
        app.push_activity(format!("shell blocked: {command}"));
        return None;
    }

    let approval_rx = if !auto_approve
        && matches!(
            decision.action,
            crate::policy::PolicyAction::AskOnce | crate::policy::PolicyAction::AskSession
        ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.approval = Some((decision.display, tx));
        app.approval_selected_index = 0;
        Some(rx)
    } else {
        None
    };

    app.begin_running_turn(&format!("!{command}"));
    app.stream_buffer = format!("$ {command}\n");
    app.status_message = if approval_rx.is_some() {
        "Approval needed: run command".into()
    } else {
        "Running shell command...".into()
    };

    let root = root.to_path_buf();
    let action_tx = action_tx.clone();
    Some(tokio::spawn(async move {
        let approved = match approval_rx {
            Some(rx) => matches!(
                tokio::time::timeout(std::time::Duration::from_mins(1), rx).await,
                Ok(Ok(true))
            ),
            None => true,
        };

        let (output, is_error) = if approved {
            crate::tools::dispatch::execute_single_tool_with_config(
                &tool_call,
                &root,
                crate::tools::dispatch::ToolDispatchConfig::from_policy(&policy_config),
            )
            .await
        } else {
            ("Denied by user or timeout".to_string(), true)
        };

        let _ = action_tx.send(TuiAction::LocalToolResult {
            command,
            output,
            is_error,
        });
    }))
}

fn local_shell_auto_approve(app: &TuiApp, yolo_mode: bool) -> bool {
    yolo_mode
        || app.session_auto_approve
        || matches!(
            app.permission_mode,
            crate::policy::PermissionMode::Bypass | crate::policy::PermissionMode::Auto
        )
}

fn maybe_start_side_slash_task(
    input: &str,
    app: &TuiApp,
    client: &crate::deepseek::client::DeepSeekClient,
    action_tx: &mpsc::UnboundedSender<TuiAction>,
) {
    let mut parts = input.trim().splitn(2, ' ');
    let Some(command) = parts.next() else {
        return;
    };
    match command {
        "/btw" => {
            let question = parts.next().unwrap_or_default().trim();
            if question.is_empty() {
                return;
            }
            let client = client.clone();
            let language = app.config.ui.language.clone();
            let messages = app.messages.clone();
            let question = question.to_string();
            let action_tx = action_tx.clone();
            tokio::spawn(async move {
                let (output, is_error) =
                    run_side_question(client, language, messages, question.clone()).await;
                let _ = action_tx.send(TuiAction::SideOutput {
                    label: "/btw".to_string(),
                    output,
                    is_error,
                });
            });
        }
        "/recap" => {
            let client = client.clone();
            let language = app.config.ui.language.clone();
            let messages = app.messages.clone();
            let action_tx = action_tx.clone();
            tokio::spawn(async move {
                let (output, is_error) = run_flash_recap(client, language, messages).await;
                let _ = action_tx.send(TuiAction::SideOutput {
                    label: "/recap".to_string(),
                    output,
                    is_error,
                });
            });
        }
        _ => {}
    }
}

async fn run_side_question(
    client: crate::deepseek::client::DeepSeekClient,
    language: String,
    messages: Vec<ProtocolMessage>,
    question: String,
) -> (String, bool) {
    let chinese = welcome::is_chinese_display_language(&language);
    let transcript = side_job_transcript(&messages, 12);
    let system = if chinese {
        "你是 Octocode 的只读旁路助手。只回答用户的侧问，不调用工具，不要求改文件，不把回答写入主会话。回答要简洁、可执行。"
    } else {
        "You are Octocode's read-only side assistant. Answer the side question without tools, without editing files, and without writing to the main session. Be concise and actionable."
    };
    let user = if chinese {
        format!(
            "侧问:\n{question}\n\n最近可见主会话:\n{transcript}\n\n请基于这些可见信息回答；信息不足就明确说明。"
        )
    } else {
        format!(
            "Side question:\n{question}\n\nRecent visible main session:\n{transcript}\n\nAnswer from this visible context; say what is unknown when context is insufficient."
        )
    };
    let request = side_chat_request(system, &user, 700);
    match client.chat_with_retry(&request).await {
        Ok(response) => {
            let answer = response_text(response).unwrap_or_else(|| {
                if chinese {
                    "模型没有返回可见答案。".to_string()
                } else {
                    "The model returned no visible answer.".to_string()
                }
            });
            (
                format_side_job_output("btw", &question, &answer, chinese, false),
                false,
            )
        }
        Err(error) => {
            let fallback = if chinese {
                format!(
                    "Flash 旁路请求失败：{error}\n\n本地状态：不会写入主会话历史；工具未启用；主 turn 不受影响。"
                )
            } else {
                format!(
                    "Flash side request failed: {error}\n\nLocal state: main session history was not changed; tools were disabled; the main turn was not interrupted."
                )
            };
            (
                format_side_job_output("btw", &question, &fallback, chinese, true),
                true,
            )
        }
    }
}

async fn run_flash_recap(
    client: crate::deepseek::client::DeepSeekClient,
    language: String,
    messages: Vec<ProtocolMessage>,
) -> (String, bool) {
    let chinese = welcome::is_chinese_display_language(&language);
    let transcript = side_job_transcript(&messages, 18);
    let system = if chinese {
        "你是 Octocode 的会话摘要器。只总结可见会话，不调用工具，不改写主会话历史。输出短摘要、当前目标、已知进展、风险和下一步。"
    } else {
        "You summarize Octocode sessions. Summarize only visible messages, use no tools, and do not write to main history. Include current goal, progress, risks, and next step."
    };
    let user = if chinese {
        format!("最近可见主会话:\n{transcript}\n\n请生成手动会话摘要。")
    } else {
        format!("Recent visible main session:\n{transcript}\n\nCreate a manual session recap.")
    };
    let request = side_chat_request(system, &user, 900);
    match client.chat_with_retry(&request).await {
        Ok(response) => {
            let answer =
                response_text(response).unwrap_or_else(|| local_side_recap(&messages, chinese));
            (
                format_side_job_output("recap", "session", &answer, chinese, false),
                false,
            )
        }
        Err(error) => {
            let fallback = if chinese {
                format!(
                    "Flash 摘要失败：{error}\n\n{}",
                    local_side_recap(&messages, true)
                )
            } else {
                format!(
                    "Flash recap failed: {error}\n\n{}",
                    local_side_recap(&messages, false)
                )
            };
            (
                format_side_job_output("recap", "session", &fallback, chinese, true),
                true,
            )
        }
    }
}

fn side_chat_request(system: &str, user: &str, max_tokens: u32) -> ChatRequest {
    ChatRequest {
        model: DeepSeekModel::Flash.to_string(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some(ChatMessageContent::from(system)),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some(ChatMessageContent::from(user)),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ],
        tools: None,
        thinking: Some(ThinkingConfig::disabled()),
        response_format: None,
        stream: false,
        max_tokens: Some(max_tokens),
    }
}

fn response_text(response: crate::deepseek::ChatResponse) -> Option<String> {
    response.choices.into_iter().find_map(|choice| {
        choice
            .message
            .content
            .map(|content| content.to_string_lossy())
            .filter(|text| !text.trim().is_empty())
    })
}

fn side_job_transcript(messages: &[ProtocolMessage], limit: usize) -> String {
    let mut lines = messages
        .iter()
        .rev()
        .filter(|message| message.visibility == MessageVisibility::UserVisible)
        .filter(|message| matches!(message.role, Role::User | Role::Assistant))
        .filter_map(|message| {
            let text = message.content.to_string_lossy();
            let text = text.trim();
            (!text.is_empty())
                .then(|| format!("{}: {}", message.role, truncate_for_activity(text, 500)))
        })
        .take(limit)
        .collect::<Vec<_>>();
    lines.reverse();
    if lines.is_empty() {
        "(no visible messages)".to_string()
    } else {
        lines.join("\n")
    }
}

fn local_side_recap(messages: &[ProtocolMessage], chinese: bool) -> String {
    let transcript = side_job_transcript(messages, 8);
    if chinese {
        format!(
            "本地摘要：当前有 {} 条主会话消息。\n最近可见内容：\n{transcript}",
            messages.len()
        )
    } else {
        format!(
            "Local recap: the main session has {} messages.\nRecent visible content:\n{transcript}",
            messages.len()
        )
    }
}

fn format_side_job_output(
    kind: &str,
    title: &str,
    body: &str,
    chinese: bool,
    is_error: bool,
) -> String {
    let status = if is_error { "error" } else { "ready" };
    let header = if chinese {
        format!("◆ 管理器 {kind}  状态:{status}")
    } else {
        manager_side_header(kind, status)
    };
    let label = if chinese { "主题" } else { "topic" };
    format!(
        "{header}\n{label}     {}\nmode      read-only, tools disabled, no session-history write\n\n{}",
        truncate_for_activity(title, 160),
        body.trim()
    )
}

fn manager_side_header(name: &str, status: &str) -> String {
    format!("◆ manager {name}  status:{status}")
}

fn char_to_byte_idx(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(idx, _)| idx)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileMentionPrefix {
    token_start: usize,
    prefix: String,
    quoted: bool,
}

fn mention_prefix_at_cursor(text: &str, cursor_pos: usize) -> Option<FileMentionPrefix> {
    let byte_cursor = char_to_byte_idx(text, cursor_pos);
    let before_cursor = &text[..byte_cursor];
    if let Some(token_start) = quoted_mention_start(before_cursor) {
        let prefix_start = token_start + 2;
        return Some(FileMentionPrefix {
            token_start,
            prefix: before_cursor[prefix_start..].replace('\\', "/"),
            quoted: true,
        });
    }
    let token_start = before_cursor
        .rfind(char::is_whitespace)
        .map_or(0, |idx| idx + 1);
    let token = &before_cursor[token_start..];
    let prefix = token.strip_prefix('@')?;
    if prefix.starts_with('"') {
        return None;
    }
    Some(FileMentionPrefix {
        token_start,
        prefix: prefix.replace('\\', "/"),
        quoted: false,
    })
}

fn quoted_mention_start(before_cursor: &str) -> Option<usize> {
    let mut open_start = None;
    for (start, _) in before_cursor.match_indices("@\"") {
        if !is_mention_token_boundary(before_cursor, start) {
            continue;
        }
        let content = &before_cursor[start + 2..];
        if !content.contains('"') {
            open_start = Some(start);
        }
    }
    open_start
}

fn is_mention_token_boundary(text: &str, start: usize) -> bool {
    start == 0
        || text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
}

fn file_mention_replacement(path: &str, quoted: bool, complete: bool) -> String {
    match (quoted, complete) {
        (true, true) => format!("@\"{path}\" "),
        (true, false) => format!("@\"{path}"),
        (false, true) => format!("@{path} "),
        (false, false) => format!("@{path}"),
    }
}

fn file_mention_display(path: &str, quoted: bool) -> String {
    if quoted {
        format!("@\"{path}\"")
    } else {
        format!("@{path}")
    }
}

fn render_status_notice(f: &mut Frame, area: Rect, notice: &str) {
    if area.width == 0 || area.height == 0 || notice.trim().is_empty() {
        return;
    }
    let p = theme::palette();
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            notice.to_string(),
            Style::default().fg(p.warning).bg(p.canvas),
        )))
        .style(Style::default().fg(p.text).bg(p.canvas)),
        area,
    );
}

fn diff_focus_height(area_height: u16) -> u16 {
    if area_height <= 8 {
        return area_height.saturating_sub(3).max(3);
    }
    (area_height / 2).clamp(6, 16)
}

fn recent_mention_paths(history: &[String]) -> Vec<String> {
    let mut paths = Vec::new();
    for entry in history.iter().rev().take(80) {
        for path in crate::tools::mentions::extract_mentions(entry) {
            let path = path.replace('\\', "/");
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn file_mention_candidates(
    indexed_paths: &[String],
    prefix: &str,
    limit: usize,
    recent_paths: &[String],
    allow_whitespace: bool,
) -> Vec<String> {
    let prefix = prefix.replace('\\', "/").to_lowercase();
    let mut candidates = indexed_paths
        .iter()
        .filter(|path| allow_whitespace || !path.chars().any(char::is_whitespace))
        .filter(|path| file_mention_matches(path, &prefix))
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| {
        file_mention_score(a, &prefix, recent_paths)
            .cmp(&file_mention_score(b, &prefix, recent_paths))
            .then_with(|| a.cmp(b))
    });
    candidates.truncate(limit);
    candidates
}

fn file_mention_score(path: &str, prefix: &str, recent_paths: &[String]) -> (u8, usize) {
    let lower = path.to_lowercase();
    let basename = lower.rsplit('/').next().unwrap_or(&lower);
    if !prefix.ends_with('/') {
        let recent_rank = recent_paths
            .iter()
            .position(|recent| recent.eq_ignore_ascii_case(path));
        if let Some(rank) = recent_rank {
            return (0, rank);
        }
    }
    if lower == prefix {
        return (1, 0);
    }
    if lower.starts_with(prefix) {
        return (2, 0);
    }
    if basename.starts_with(prefix) {
        return (3, 0);
    }
    (4, lower.len())
}

fn file_mention_matches(rel: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let lower = rel.to_lowercase();
    lower.starts_with(prefix)
        || lower
            .rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with(prefix))
}

fn common_string_prefix(values: &[String]) -> String {
    let Some(first) = values.first() else {
        return String::new();
    };

    let mut prefix = first.clone();
    for value in values.iter().skip(1) {
        while !value.starts_with(&prefix) {
            if prefix.pop().is_none() {
                return String::new();
            }
        }
    }
    prefix
}

/// Returns (line_index, column_index) for the given character position in a multi-line string.
fn line_and_col(text: &str, pos: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if i == pos {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Returns the number of characters in the given line (0-based index).
fn col_of_line(text: &str, line_idx: usize) -> usize {
    let mut line = 0;
    let mut col = 0;
    for c in text.chars() {
        if line == line_idx {
            if c == '\n' {
                break;
            }
            col += 1;
        } else if c == '\n' {
            line += 1;
        }
    }
    col
}

/// Returns the character position at the given (line, col) in a multi-line string.
fn pos_of_line_col(text: &str, target_line: usize, target_col: usize) -> usize {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if line == target_line {
            if col == target_col || c == '\n' {
                return i;
            }
            col += 1;
        } else if c == '\n' {
            line += 1;
        }
    }
    text.chars().count()
}

fn is_actionable_key_event(key: KeyEvent) -> bool {
    match key.kind {
        KeyEventKind::Press => true,
        KeyEventKind::Repeat => false,
        KeyEventKind::Release => false,
    }
}

fn logical_line_count(text: &str) -> usize {
    if text.is_empty() {
        1
    } else {
        text.split('\n').count()
    }
}

fn truncate_for_activity(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

fn summarize_task_title(input: &str) -> String {
    let cleaned = input
        .trim()
        .trim_start_matches('!')
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '。'
                    | '.'
                    | ','
                    | '，'
                    | '?'
                    | '？'
                    | '!'
                    | '！'
            )
        });
    if cleaned.is_empty() {
        return "Working...".to_string();
    }

    if contains_cjk(cleaned) {
        summarize_cjk_task_title(cleaned)
    } else {
        summarize_latin_task_title(cleaned)
    }
}

fn summarize_cjk_task_title(input: &str) -> String {
    if let Some(title) = summarize_cjk_keywords(input) {
        return title;
    }

    if input.contains("吗")
        || input.contains("谁")
        || input.contains("什么")
        || input.contains("为什么")
        || input.contains("怎么")
        || input.contains("？")
        || input.contains('?')
    {
        "回答问题".to_string()
    } else {
        "处理请求".to_string()
    }
}

fn summarize_cjk_keywords(input: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    let has_cli = lower.contains("cli") || lower.contains("octocode");
    let has_api = lower.contains("api");

    let title = if input.contains("电脑里的文件")
        || input.contains("本机文件")
        || input.contains("本地文件")
        || input.contains("读取")
        || input.contains("读一下")
        || input.contains("打开文件")
    {
        "读取本地文件"
    } else if input.contains("项目结构")
        || input.contains("目录结构")
        || input.contains("文件结构")
        || input.contains("仓库")
    {
        "检查项目结构"
    } else if input.contains("搜索文件") || input.contains("搜索代码") || input.contains("查找")
    {
        "搜索项目内容"
    } else if input.contains("bug")
        || input.contains("报错")
        || input.contains("错误")
        || input.contains("排查")
    {
        "排查问题"
    } else if input.contains("流式") || input.contains("stream") {
        "修复流式输出"
    } else if input.contains("状态") || input.contains("关键词") || input.contains("提示") {
        "优化状态提示"
    } else if input.contains("推荐") || input.contains("选择") || input.contains("选项") {
        "优化选项选择"
    } else if input.contains("输入框") || input.contains("打字") || input.contains("光标") {
        "修复输入框"
    } else if input.contains("测试") && has_cli {
        "测试 CLI"
    } else if input.contains("测试") {
        "整理测试方案"
    } else if has_api || input.contains("密钥") {
        "修复 API 设置"
    } else if input.contains("界面") || lower.contains("ui") {
        "优化界面"
    } else if input.contains("计划") || lower.contains("plan") {
        "优化计划模式"
    } else if input.contains("智能体") || lower.contains("agent") {
        "优化多智能体"
    } else {
        return None;
    };

    Some(title.to_string())
}

fn summarize_latin_task_title(input: &str) -> String {
    let title_words = input
        .split_whitespace()
        .filter(|word| {
            !word
                .trim_matches(|ch: char| ch.is_ascii_punctuation())
                .is_empty()
        })
        .take(5)
        .collect::<Vec<_>>()
        .join(" ");
    trim_title_punctuation(&title_words)
}

fn trim_title_punctuation(title: &str) -> String {
    let trimmed = title
        .trim()
        .trim_matches(|ch: char| ch.is_ascii_punctuation())
        .trim_matches(|ch: char| matches!(ch, '。' | '，' | '、' | '；' | '：' | '？' | '！'))
        .trim();
    if trimmed.is_empty() {
        "Working...".to_string()
    } else {
        trimmed.to_string()
    }
}

fn summarize_plan_step(description: &str) -> String {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return "task".to_string();
    }
    if let Some(agent_step) = summarize_agent_plan_step(trimmed) {
        return agent_step;
    }
    if trimmed.starts_with("Read `")
        || trimmed.starts_with("Search `")
        || trimmed.starts_with("Edit `")
        || trimmed.starts_with("Run `")
        || trimmed.starts_with("Verify")
    {
        return truncate_chars(trimmed, 56);
    }
    if contains_cjk(trimmed) {
        summarize_cjk_keywords(trimmed).unwrap_or_else(|| truncate_chars(trimmed, 18))
    } else {
        let title = trimmed
            .split_whitespace()
            .take(6)
            .collect::<Vec<_>>()
            .join(" ");
        trim_title_punctuation(&title)
    }
}

fn summarize_agent_plan_step(description: &str) -> Option<String> {
    let rest = description.strip_prefix("agent ")?;
    let (role, task) = rest
        .split_once(" · ")
        .or_else(|| rest.split_once(" - "))
        .unwrap_or((rest, ""));
    let role_label = match role.trim() {
        "code-explorer" | "explorer" => "explorer",
        "code-reviewer" | "reviewer" => "reviewer",
        "general-purpose" | "planner" => "planner",
        "test-runner" | "tester" => "tester",
        "worker" => "worker",
        "verifier" => "verifier",
        other if !other.is_empty() => other,
        _ => "agent",
    };
    let task = task.trim();
    if task.is_empty() {
        Some(format!("agent {role_label}"))
    } else {
        Some(truncate_chars(&format!("agent {role_label} · {task}"), 64))
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn normalized_plan_title(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '·' | '-' | '_' | ':' | '：'))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn format_duration_compact(duration_ms: Option<u64>) -> String {
    duration_ms
        .map(plan_tracker::format_duration_compact)
        .unwrap_or_else(|| "—".to_string())
}

fn append_brewed_line(stream_buffer: &mut String, duration_ms: u64) {
    if stream_buffer.trim().is_empty() || stream_buffer.contains("Brewed for ") {
        return;
    }
    if !stream_buffer.ends_with('\n') {
        stream_buffer.push('\n');
    }
    stream_buffer.push('\n');
    stream_buffer.push_str(&format!(
        "* Brewed for {}",
        plan_tracker::format_duration_compact(duration_ms)
    ));
    stream_buffer.push('\n');
}

fn empty_visible_answer_notice(chinese: bool) -> &'static str {
    if chinese {
        "本轮模型只返回了隐藏推理内容，没有返回可显示的最终回答。Octocode 已隐藏 reasoning，所以之前看起来像没有输出。请重试，或切换模型/关闭 thinking 后再试。"
    } else {
        "The model returned hidden reasoning but no visible final answer. Octocode hides reasoning, so the turn looked blank. Retry, switch models, or disable thinking and try again."
    }
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x4E00..=0x9FFF
                | 0x3400..=0x4DBF
                | 0x20000..=0x2A6DF
                | 0x2A700..=0x2B73F
                | 0x2B740..=0x2B81F
                | 0x2B820..=0x2CEAF
        )
    })
}

fn thought_seconds_from_reasoning(reasoning_buffer: &str, elapsed_seconds: u64) -> u64 {
    if reasoning_buffer.trim().is_empty() {
        0
    } else {
        elapsed_seconds.max(1)
    }
}

fn option_shortcut_index(c: char, option_count: usize) -> Option<usize> {
    if option_count == 0 {
        return None;
    }
    if let Some(digit) = c.to_digit(10) {
        let idx = digit as usize;
        if idx > 0 && idx <= option_count {
            return Some(idx - 1);
        }
    }

    let upper = c.to_ascii_uppercase();
    if upper.is_ascii_alphabetic() {
        let idx = (upper as u8 - b'A') as usize;
        if idx < option_count {
            return Some(idx);
        }
    }
    None
}

fn approval_action_from_shortcut(c: char) -> Option<ApprovalAction> {
    match c.to_ascii_lowercase() {
        'a' | 'y' => Some(ApprovalAction::ApproveOnce),
        's' => Some(ApprovalAction::ApproveSession),
        'd' | 'n' => Some(ApprovalAction::Deny),
        _ => None,
    }
}

fn cycle_provider_kind(current: ProviderKind, delta: i32) -> ProviderKind {
    let values = ProviderKind::all();
    values[cycle_index(
        values
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0),
        values.len(),
        delta,
    )]
}

fn cycle_model(current: &DeepSeekModel, delta: i32) -> DeepSeekModel {
    let values = [DeepSeekModel::Flash, DeepSeekModel::Pro];
    values[cycle_index(
        values
            .iter()
            .position(|value| value == &current.canonical())
            .unwrap_or(0),
        values.len(),
        delta,
    )]
    .clone()
}

fn cycle_thinking_mode(current: &ThinkingMode, delta: i32) -> ThinkingMode {
    let values = [ThinkingMode::Auto, ThinkingMode::On, ThinkingMode::Off];
    values[cycle_index(
        values
            .iter()
            .position(|value| value == current)
            .unwrap_or(0),
        values.len(),
        delta,
    )]
    .clone()
}

fn cycle_reasoning_effort(current: &ReasoningEffort, delta: i32) -> ReasoningEffort {
    let values = [
        ReasoningEffort::Min,
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
        ReasoningEffort::Max,
    ];
    values[cycle_index(
        values
            .iter()
            .position(|value| value == current)
            .unwrap_or(0),
        values.len(),
        delta,
    )]
    .clone()
}

fn cycle_autonomy_level(
    current: storage::config::AutonomyLevel,
    delta: i32,
) -> storage::config::AutonomyLevel {
    let values = [
        storage::config::AutonomyLevel::Off,
        storage::config::AutonomyLevel::Low,
        storage::config::AutonomyLevel::Medium,
        storage::config::AutonomyLevel::High,
    ];
    values[cycle_index(
        values
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0),
        values.len(),
        delta,
    )]
}

fn cycle_theme_mode(current: theme::ThemeMode, delta: i32) -> theme::ThemeMode {
    let values = [
        theme::ThemeMode::Auto,
        theme::ThemeMode::Light,
        theme::ThemeMode::Dark,
        theme::ThemeMode::HighContrast,
    ];
    values[cycle_index(
        values
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0),
        values.len(),
        delta,
    )]
}

fn cycle_motion_level(current: motion::MotionLevel, delta: i32) -> motion::MotionLevel {
    let values = [motion::MotionLevel::Subtle, motion::MotionLevel::Off];
    values[cycle_index(
        values
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0),
        values.len(),
        delta,
    )]
}

fn cycle_renderer_mode(current: RendererMode, delta: i32) -> RendererMode {
    let values = [RendererMode::Classic, RendererMode::Fullscreen];
    values[cycle_index(
        values
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0),
        values.len(),
        delta,
    )]
}

fn cycle_ui_language(current: &str, delta: i32) -> String {
    let values = ["auto", "zh-CN", "en-US", "ja-JP"];
    values[cycle_index(
        values
            .iter()
            .position(|value| value.eq_ignore_ascii_case(current))
            .unwrap_or(1),
        values.len(),
        delta,
    )]
    .to_string()
}

fn ui_language_display_name(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "auto" | "" => "Automatic",
        "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" | "chinese" => "Chinese",
        "en" | "en-us" | "en-gb" | "english" => "English",
        "ja" | "ja-jp" | "jp" | "japanese" => "Japanese",
        value if value.starts_with("zh") => "Chinese",
        value if value.starts_with("en") => "English",
        value if value.starts_with("ja") => "Japanese",
        _ => "English",
    }
}

fn cycle_index(current: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    if delta < 0 {
        (current + len - 1) % len
    } else {
        (current + 1) % len
    }
}

fn cycle_command_timeout_seconds(current: u64, delta: i32) -> u64 {
    let values = [15, 30, 60, 120, 300, 600];
    values[cycle_index(
        values
            .iter()
            .position(|value| *value == current)
            .unwrap_or_else(|| {
                values
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, value)| value.abs_diff(current))
                    .map(|(idx, _)| idx)
                    .unwrap_or(3)
            }),
        values.len(),
        delta,
    )]
}

fn is_todo_state_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "todo_write" | "task_create" | "task_update" | "task_stop"
    )
}

fn settings_on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn settings_on_required(value: bool) -> &'static str {
    if value {
        "required"
    } else {
        "skipped"
    }
}

fn format_keybinding_label(value: &str) -> String {
    value
        .split_whitespace()
        .map(|stroke| {
            stroke
                .split('+')
                .map(|part| match part {
                    "ctrl" => "Ctrl".to_string(),
                    "alt" => "Alt".to_string(),
                    "shift" => "Shift".to_string(),
                    other => other.to_ascii_uppercase(),
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
fn is_exit_key(key: KeyEvent) -> bool {
    keybindings::Keymap::default().action_for(key) == Some(keybindings::KeyAction::Exit)
}

#[cfg(test)]
fn is_interrupt_key(key: KeyEvent) -> bool {
    keybindings::Keymap::default().action_for(key) == Some(keybindings::KeyAction::Interrupt)
}

/// Try to match user input against a list of options.
/// Returns `Some((1-based index, option text))` if matched.
fn try_match_option(input: &str, options: &[String]) -> Option<(usize, String)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Exact number match: "1", "2", ...
    if let Ok(idx) = trimmed.parse::<usize>() {
        if idx > 0 && idx <= options.len() {
            return Some((idx, options[idx - 1].clone()));
        }
    }

    // 2. Single-letter match: "A", "b", ... (case-insensitive)
    if trimmed.len() == 1 {
        let c = trimmed.chars().next().unwrap().to_ascii_uppercase();
        if c.is_ascii_alphabetic() {
            let idx = (c as u8 - b'A' + 1) as usize;
            if idx <= options.len() {
                return Some((idx, options[idx - 1].clone()));
            }
        }
    }

    // 3. Exact text match (case-insensitive)
    let input_lower = trimmed.to_lowercase();
    for (i, opt) in options.iter().enumerate() {
        if opt.to_lowercase() == input_lower {
            return Some((i + 1, opt.clone()));
        }
    }

    // 4. Prefix match (case-insensitive)
    for (i, opt) in options.iter().enumerate() {
        if opt.to_lowercase().starts_with(&input_lower) {
            return Some((i + 1, opt.clone()));
        }
    }

    None
}

fn format_pending_option_reply(index: usize, option: &str) -> String {
    format!("Selected option {index}: {option}")
}

/// Parse a multi-select reply like "1,3" or "alpha, beta" against the
/// available option labels. Returns the matched labels in input order, or
/// `None` if any token failed to match — caller falls back to single-match.
fn try_match_multi_options(input: &str, options: &[String]) -> Option<Vec<(usize, String)>> {
    let trimmed = input.trim();
    if !trimmed.contains(',') {
        return None;
    }
    let mut picks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for token in trimmed.split(',') {
        let tok = token.trim();
        if tok.is_empty() {
            continue;
        }
        let matched = try_match_option(tok, options)?;
        if seen.insert(matched.0) {
            picks.push(matched);
        }
    }
    if picks.is_empty() {
        None
    } else {
        Some(picks)
    }
}

fn format_pending_multi_reply(picks: &[(usize, String)]) -> String {
    let joined = picks
        .iter()
        .map(|(_, label)| label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("Selected options: {joined}")
}

fn apply_session_model_selection(
    session: &mut Session,
    requested_model: DeepSeekModel,
    has_model_override: bool,
    loaded_existing_session: bool,
    thinking_mode: ThinkingMode,
) -> DeepSeekModel {
    session.reasoning_state.mode = thinking_mode;
    if has_model_override || !loaded_existing_session {
        session.reasoning_state.selected_model = Some(requested_model.canonical());
    }
    session.reasoning_state.effective_model()
}

/// Run the TUI interactive session.
pub async fn run_tui(
    project_root: Option<PathBuf>,
    thinking: bool,
    model_override: Option<String>,
    session_id: Option<String>,
) -> Result<(), anyhow::Error> {
    let terminal_env = TerminalEnvironment::current();
    terminal_env.ensure_tui_supported()?;

    let root = crate::cli::resolve_project_root_or_cwd(project_root);
    let (startup, api_key) = TuiStartupData::load(&root);
    let renderer_mode = RendererMode::from_config(&startup.config.ui.renderer);
    let probe_keyring = startup.probe_keyring;
    let mut active_client =
        crate::deepseek::client::DeepSeekClient::new(api_key.unwrap_or_default());

    let model = match model_override.as_deref() {
        Some(value) => crate::provider::parse_model(value)
            .map_err(|error| anyhow::anyhow!("invalid model override: {error}"))?,
        None => startup.config.model.default.canonical(),
    };

    let thinking_mode = if thinking {
        ThinkingMode::On
    } else {
        ThinkingMode::Auto
    };

    // Load or create session
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let store = storage::SessionStore::new(home.join(".octocode"));

    let mut loaded_existing_session = false;
    let mut session = if let Some(ref sid) = session_id {
        let sid = uuid::Uuid::parse_str(sid)?;
        match store.load(&root, &sid) {
            Ok(session) => {
                loaded_existing_session = true;
                session
            }
            Err(_) => Session {
                id: SessionId::new_v4(),
                name: None,
                project_root: root.clone(),
                messages: Vec::new(),
                reasoning_state: ReasoningState::default(),
                tool_call_history: Vec::new(),
                checkpoints: Vec::new(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                metadata: SessionMetadata::default(),
            },
        }
    } else {
        Session {
            id: SessionId::new_v4(),
            name: None,
            project_root: root.clone(),
            messages: Vec::new(),
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            metadata: SessionMetadata::default(),
        }
    };
    let active_model = apply_session_model_selection(
        &mut session,
        model,
        model_override.is_some(),
        loaded_existing_session,
        thinking_mode.clone(),
    );

    let mut orchestrator = Some(Orchestrator::new(
        active_client.clone(),
        root.clone(),
        session,
    ));

    // Set up terminal
    let _terminal_session = TerminalSession::enter(renderer_mode)?;
    let mut terminal = tui_terminal(renderer_mode)?;

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<TuiAction>();
    let mut app = TuiApp::new_with_startup(
        active_model,
        thinking_mode,
        session_id,
        root.clone(),
        startup,
    );
    app.session_id = orchestrator.as_ref().map(|orch| orch.session.id);
    if let Some(ref orch) = orchestrator {
        app.mcp_status = orch.mcp_status();
    }
    app.refresh_todo_summary();
    if let Some(session_id) = orchestrator.as_ref().map(|orch| orch.session.id) {
        let event_store = storage::EventLogStore::new(home.join(".octocode"));
        if let Ok(events) = event_store.load(&root, &session_id) {
            app.restore_recoverable_state_from_events(&events);
        }
    }
    let mut running_turn: Option<RunningTurn> = None;
    let mut local_task: Option<JoinHandle<()>> = None;
    let mut keyring_task =
        probe_keyring.then(|| tokio::task::spawn_blocking(storage::get_keyring_api_key));
    let mut yolo_mode = false;
    let mut last_render_state = app.render_dirty_state();
    let mut force_render = true;
    let mut last_draw_at = std::time::Instant::now();
    let mut pending_stream_draw = false;

    // Main event loop
    let result: Result<(), anyhow::Error> = loop {
        if !app.running {
            break Ok(());
        }

        let new_render_state = app.render_dirty_state();
        let dirty = new_render_state.diff(last_render_state);
        let should_draw = force_render || dirty.any();

        if should_draw {
            let can_throttle = pending_stream_draw && !force_render;
            if !can_throttle || last_draw_at.elapsed() >= TUI_FRAME_INTERVAL {
                terminal
                    .draw(|f| app.render(f))
                    .map_err(|e| anyhow::anyhow!("TUI draw failed: {e}"))?;
                last_draw_at = std::time::Instant::now();
                force_render = false;
                pending_stream_draw = false;
                last_render_state = new_render_state;
            }
        }

        if let Some(running) = running_turn.as_mut() {
            let mut invalidation = RenderInvalidation::default();
            while let Ok(ev) = running.events.try_recv() {
                invalidation.observe_agent_event(&ev);
                app.apply_agent_event(ev);
            }
            invalidation.apply(&mut force_render, &mut pending_stream_draw);
        }

        if let Some(mut running) = running_turn.take_if(|running| running.handle.is_finished()) {
            let mut invalidation = RenderInvalidation::default();
            while let Ok(ev) = running.events.try_recv() {
                invalidation.observe_agent_event(&ev);
                app.apply_agent_event(ev);
            }
            invalidation.apply(&mut force_render, &mut pending_stream_draw);

            match running.handle.await {
                Ok((mut returned_orchestrator, run_error)) => {
                    returned_orchestrator.set_active_model(app.model.clone());
                    let completion_report = app.plan_completion_report();
                    app.messages = returned_orchestrator.session.messages.clone();
                    app.stream_buffer.clear();
                    if let Some(report) = completion_report {
                        app.stream_buffer = report;
                    }
                    app.is_streaming = false;
                    app.stream_start = None;
                    app.pending_user_message = None;
                    app.refresh_welcome(&root);
                    if let Some(error) = run_error {
                        app.status_message = format!("This turn didn't finish: {error}");
                    }
                    app.session_id = Some(returned_orchestrator.session.id);
                    orchestrator = Some(returned_orchestrator);
                    force_render = true;
                    pending_stream_draw = false;
                }
                Err(error) if error.is_cancelled() => {
                    app.is_streaming = false;
                    app.stream_start = None;
                    app.pending_user_message = None;
                    app.status_message = "Turn interrupted".into();
                    orchestrator = None;
                    force_render = true;
                    pending_stream_draw = false;
                }
                Err(error) => {
                    app.is_streaming = false;
                    app.stream_start = None;
                    app.pending_user_message = None;
                    app.status_message = format!("Turn task ended early: {error}");
                    force_render = true;
                    pending_stream_draw = false;
                }
            }
        }

        if running_turn.is_none() && !app.is_streaming {
            if let Some(output) = app.pending_side_outputs.pop_front() {
                app.show_local_output(output);
                force_render = true;
                pending_stream_draw = false;
            } else if let Some(queued) = app.queued_inputs.pop_front() {
                let remaining = app.queued_inputs.len();
                app.status_message = if remaining == 0 {
                    "发送已排队的消息".into()
                } else {
                    format!("发送已排队的消息；剩余 {remaining} 条")
                };
                let _ = action_tx.send(TuiAction::Submit(queued));
                force_render = true;
                pending_stream_draw = false;
            }
        }

        if local_task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            if let Some(handle) = local_task.take() {
                let _ = handle.await;
            }
        }

        if keyring_task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            if let Some(handle) = keyring_task.take() {
                if let Ok(Some(api_key)) = handle.await {
                    active_client = crate::deepseek::client::DeepSeekClient::new(api_key);
                    app.mark_api_key_ready_from_storage(&root);
                    app.status_message = "API key loaded from system keyring".into();
                    if let Some(orchestrator) = orchestrator.as_mut() {
                        orchestrator.client = active_client.clone();
                    }
                    force_render = true;
                    pending_stream_draw = false;
                }
            }
        }

        // Poll keyboard events (non-blocking)
        let poll_timeout = if pending_stream_draw {
            TUI_FRAME_INTERVAL
                .saturating_sub(last_draw_at.elapsed())
                .min(std::time::Duration::from_millis(50))
        } else {
            std::time::Duration::from_millis(50)
        };
        if event::poll(poll_timeout).map_err(|e| anyhow::anyhow!("TUI event poll failed: {e}"))? {
            match event::read().map_err(|e| anyhow::anyhow!("TUI event read failed: {e}"))? {
                CEvent::Key(key) => {
                    if !is_actionable_key_event(key) {
                        continue;
                    }
                    force_render = true;
                    pending_stream_draw = false;
                    if app.key_is_exit(key) {
                        if app.handle_exit_key() {
                            break Ok(());
                        }
                        continue;
                    }
                    if app.key_is_interrupt(key) {
                        app.clear_exit_confirmation();
                        if running_turn.is_some() || local_task.is_some() || app.is_streaming {
                            let _ = action_tx.send(TuiAction::Interrupt);
                        } else {
                            let exit_label = app.exit_key_label();
                            app.status_message = if app.is_chinese_ui() {
                                format!("按 {exit_label} 退出")
                            } else {
                                format!("Press {exit_label} to exit")
                            };
                        }
                        continue;
                    }
                    if key.code == KeyCode::Esc
                        && (running_turn.is_some() || local_task.is_some() || app.is_streaming)
                    {
                        let _ = action_tx.send(TuiAction::Interrupt);
                        continue;
                    }
                    app.handle_key(key, &action_tx);
                }
                CEvent::Paste(text) => {
                    force_render = true;
                    pending_stream_draw = false;
                    app.clear_exit_confirmation();
                    app.handle_paste(&text);
                }
                CEvent::Mouse(mouse) => {
                    force_render = true;
                    pending_stream_draw = false;
                    app.clear_exit_confirmation();
                    app.handle_mouse(mouse);
                }
                CEvent::Resize(width, height) => {
                    force_render = true;
                    pending_stream_draw = false;
                    let _ = terminal.resize(Rect::new(0, 0, width, height));
                }
                _ => {}
            }
        }

        // Drain TUI actions
        while let Ok(action) = action_rx.try_recv() {
            force_render = true;
            pending_stream_draw = false;
            match action {
                TuiAction::SaveApiKey {
                    api_key,
                    pending_prompt,
                } => match storage::store_api_key_with_project_fallback(&api_key, Some(&root)) {
                    Ok(location) => {
                        app.finish_api_key_save_success(&root, &location);
                        active_client =
                            crate::deepseek::client::DeepSeekClient::new(api_key.clone());
                        if let Some(orchestrator) = orchestrator.as_mut() {
                            orchestrator.client = active_client.clone();
                        }
                        if let Some(prompt) = pending_prompt {
                            let _ = action_tx.send(TuiAction::Submit(prompt));
                        }
                    }
                    Err(error) => {
                        app.finish_api_key_save_error(&error);
                    }
                },
                TuiAction::Submit(mut input) => {
                    // Hand any pending per-command allowlist over to the
                    // orchestrator before processing this input. The TUI
                    // sees both raw user keystrokes and rendered prompt
                    // commands here; only the second pass (after a slash
                    // command has populated `pending_allowed_tools`) carries
                    // an allowlist to consume.
                    let staged_allowed_tools = app.pending_allowed_tools.take();
                    if let Some(orchestrator) = orchestrator.as_mut() {
                        orchestrator.stage_allowed_tools(staged_allowed_tools);
                    }
                    // Slash commands
                    if input.trim().starts_with('/') {
                        let registry = {
                            let mut registry_cache = app.slash_command_registry_cache.borrow_mut();
                            registry_cache.refresh_for_root(&root);
                            registry_cache.registry().clone()
                        };
                        let mcp_status = app.mcp_status.clone();
                        let background_tasks = orchestrator
                            .as_ref()
                            .map(Orchestrator::background_tasks)
                            .unwrap_or_default();
                        let model_before = app.model.clone();
                        let permission_mode_before = app.permission_mode;
                        let mut ctx = crate::commands::CommandContext {
                            app: &mut app,
                            project_root: &root,
                            yolo_mode: &mut yolo_mode,
                            mcp_status: &mcp_status,
                            background_tasks: &background_tasks,
                        };
                        if let Some(result) = registry.execute(&input, &mut ctx) {
                            match result {
                                Ok(Some(msg)) => {
                                    let command_name = input.split_whitespace().next();
                                    if app.model != model_before {
                                        if let Some(orchestrator) = orchestrator.as_mut() {
                                            orchestrator.set_active_model(app.model.clone());
                                        }
                                    }
                                    if app.permission_mode != permission_mode_before {
                                        if let Some(orchestrator) = orchestrator.as_mut() {
                                            orchestrator.permission_mode = app.permission_mode;
                                            // Bypass/Auto imply legacy yolo;
                                            // any other mode resets yolo so the
                                            // user can dial back from bypass.
                                            orchestrator.yolo_mode = matches!(
                                                app.permission_mode,
                                                crate::policy::PermissionMode::Bypass
                                                    | crate::policy::PermissionMode::Auto
                                            );
                                            yolo_mode = orchestrator.yolo_mode;
                                        }
                                    }
                                    if matches!(
                                        input.split_whitespace().next(),
                                        Some("/compact" | "/compress")
                                    ) {
                                        if let Some(orchestrator) = orchestrator.as_mut() {
                                            orchestrator.session.messages = app.messages.clone();
                                            if let Some(home) = dirs::home_dir() {
                                                let store = storage::SessionStore::new(
                                                    home.join(".octocode"),
                                                );
                                                if let Err(e) = store.save(&orchestrator.session) {
                                                    app.push_activity(format!(
                                                        "warning: failed to persist session after compact: {e}"
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    if is_swarm_cancel_command(&input) {
                                        if let Some(running) = running_turn.as_ref() {
                                            running.cancel_token.store(true, Ordering::SeqCst);
                                        }
                                        app.request_swarm_cancel();
                                    }
                                    if matches!(command_name, Some("/btw" | "/recap")) {
                                        maybe_start_side_slash_task(
                                            &input,
                                            &app,
                                            &active_client,
                                            &action_tx,
                                        );
                                    }
                                    if running_turn.is_some() || app.is_streaming {
                                        let status = msg
                                            .lines()
                                            .find(|line| !line.trim().is_empty())
                                            .unwrap_or("Command complete")
                                            .to_string();
                                        app.clear_input_editor();
                                        app.status_message = truncate_for_activity(&status, 120);
                                        app.push_activity(format!(
                                            "command: {}",
                                            truncate_for_activity(&status, 80)
                                        ));
                                    } else {
                                        app.show_local_output(msg);
                                    }
                                    continue;
                                }
                                Ok(None) => {
                                    if matches!(
                                        input.split_whitespace().next(),
                                        Some("/settings" | "/set")
                                    ) {
                                        continue;
                                    }
                                    if let Some(forwarded) =
                                        crate::commands::forwarded_agent_input(&input)
                                    {
                                        input = forwarded;
                                    }
                                    // Command forwarded to agent (e.g. /fix, /explain, /review)
                                    // Fall through to normal processing
                                }
                                Err(e) => {
                                    app.show_local_output(e);
                                    continue;
                                }
                            }
                        }
                    }

                    match shell_command_from_input(&input) {
                        Ok(Some(command)) => {
                            if running_turn.is_some() || app.is_streaming {
                                app.status_message = "A turn is already running".into();
                                continue;
                            }
                            let auto_approve = local_shell_auto_approve(&app, yolo_mode);
                            local_task = start_local_shell_command(
                                &mut app,
                                &root,
                                command,
                                &action_tx,
                                auto_approve,
                            );
                            continue;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            app.show_local_output(error);
                            continue;
                        }
                    }

                    let input = app.input_for_interaction_mode(input);
                    if app.interaction_mode == InteractionMode::FullAccess {
                        yolo_mode = true;
                        app.session_auto_approve = true;
                    }

                    if running_turn.is_some() || app.is_streaming {
                        app.queue_user_input(input);
                        continue;
                    }

                    if app.should_block_agent_turn_for_api_key() {
                        if let Some(api_key) = storage::get_effective_api_key(Some(&root)) {
                            app.mark_api_key_ready_from_storage(&root);
                            active_client = build_provider(&app.config.provider, api_key.clone())
                                .create_deepseek_client();
                            if let Some(orchestrator) = orchestrator.as_mut() {
                                orchestrator.client = active_client.clone();
                            }
                        } else {
                            app.push_activity("send blocked: missing API key");
                            app.begin_api_key_entry(Some(input));
                            continue;
                        }
                    }

                    // Clear any lingering completed plan / subagents from a previous turn
                    if !app.plan_steps.is_empty() && app.plan_current_step >= app.plan_total_steps {
                        app.plan_steps.clear();
                        app.plan_current_step = 0;
                        app.plan_total_steps = 0;
                        app.plan_summary = None;
                    }
                    app.subagents.clear();
                    app.active_swarm = None;
                    app.file_diffs.clear();

                    let Some(mut turn_orchestrator) = orchestrator.take() else {
                        app.is_streaming = false;
                        app.stream_start = None;
                        app.status_message = "Agent is not ready; restart the TUI".into();
                        continue;
                    };
                    turn_orchestrator.yolo_mode = yolo_mode;
                    turn_orchestrator.set_active_model(app.model.clone());

                    let (ev_tx, ev_rx) = mpsc::unbounded_channel();
                    let cancel_token = Arc::new(AtomicBool::new(false));
                    turn_orchestrator.set_swarm_cancel_token(cancel_token.clone());
                    app.begin_running_turn(&input);

                    let images = std::mem::take(&mut app.pending_images);
                    let handle = tokio::spawn(async move {
                        if !turn_orchestrator.mcp_initialized() {
                            let config =
                                crate::storage::Config::load(Some(&turn_orchestrator.project_root))
                                    .unwrap_or_default();
                            turn_orchestrator.init_mcp(&config.mcp).await;
                        }

                        let run_error = turn_orchestrator
                            .run_turn_with_images(&input, &images, ev_tx)
                            .await
                            .err()
                            .map(|e| e.to_string());

                        let mut effective_error = run_error;
                        if effective_error.is_none() {
                            if let Some(home) = dirs::home_dir() {
                                let store = storage::SessionStore::new(home.join(".octocode"));
                                if let Err(e) = store.save(&turn_orchestrator.session) {
                                    effective_error =
                                        Some(format!("failed to persist session: {e}"));
                                }
                            }
                        }

                        (turn_orchestrator, effective_error)
                    });

                    running_turn = Some(RunningTurn {
                        events: ev_rx,
                        handle,
                        cancel_token,
                    });
                }
                TuiAction::LocalToolResult {
                    command,
                    output,
                    is_error,
                } => {
                    local_task = None;
                    let status = if is_error { "error" } else { "ok" };
                    app.is_streaming = false;
                    app.stream_start = None;
                    app.stream_buffer = format!("$ {command}\n\n{output}");
                    app.status_message = format!("Shell [{status}]: {}", first_line(&output));
                    app.push_activity(format!(
                        "shell {status}: {}",
                        truncate_for_activity(&command, 120)
                    ));
                }
                TuiAction::SideOutput {
                    label,
                    output,
                    is_error,
                } => {
                    let status = if is_error { "error" } else { "ok" };
                    if running_turn.is_some() || app.is_streaming {
                        app.pending_side_outputs.push_back(output);
                        app.status_message = format!("{label} answer ready after current turn");
                    } else {
                        app.show_local_output(output);
                    }
                    app.push_activity(format!("{label} [{status}] side output ready"));
                }
                TuiAction::Interrupt => {
                    if let Some(running) = running_turn.take() {
                        running.handle.abort();
                        let _ = running.handle.await;
                    }
                    if let Some(handle) = local_task.take() {
                        handle.abort();
                        let _ = handle.await;
                    }
                    app.cancel_running_work();
                    let replacement = Orchestrator::new(
                        active_client.clone(),
                        root.clone(),
                        app.session_snapshot(&root),
                    );
                    app.session_id = Some(replacement.session.id);
                    orchestrator = Some(replacement);
                }
                TuiAction::ResumeSession => {
                    app.status_message = "Use /resume command in CLI mode".into();
                }
                TuiAction::ShowTranscript => {
                    if let Some(orchestrator) = orchestrator.as_ref() {
                        app.status_message = format!(
                            "Session: {} | Messages: {} | Tool calls: {}",
                            orchestrator.session.id,
                            orchestrator.session.messages.len(),
                            orchestrator.session.tool_call_history.len()
                        );
                    } else {
                        app.status_message = "Session is busy with a running turn".into();
                    }
                }
                _ => {}
            }
        }

        let latest_state = app.render_dirty_state();
        if latest_state.diff(last_render_state).any() {
            if !pending_stream_draw {
                force_render = true;
            }
        } else {
            pending_stream_draw = false;
        }
    };

    let _ = terminal.show_cursor();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> TuiApp {
        test_app_with_root(PathBuf::from("D:/octocode"))
    }

    #[test]
    fn stream_delta_events_are_render_throttled() {
        assert!(agent_event_can_throttle_render(&AgentEvent::ContentDelta(
            "visible".into()
        )));
        assert!(agent_event_can_throttle_render(
            &AgentEvent::ReasoningDelta("hidden".into())
        ));
        assert!(agent_event_can_throttle_render(&AgentEvent::TokenDelta {
            input_tokens: 0,
            output_tokens: 1,
        }));
        assert!(agent_event_can_throttle_render(
            &AgentEvent::SubagentDelta {
                agent_id: "agent".into(),
                content: "delta".into(),
            }
        ));
        assert!(!agent_event_can_throttle_render(
            &AgentEvent::TurnComplete {
                session_id: SessionId::new_v4(),
                total_tokens: 1,
            }
        ));
        assert!(!agent_event_can_throttle_render(&AgentEvent::Error(
            "boom".into()
        )));
    }

    #[test]
    fn transcript_dirty_state_tracks_last_message_content_without_lossy_clone() {
        let mut app = test_app();
        let turn_id = uuid::Uuid::new_v4();
        app.messages.push(ProtocolMessage {
            id: uuid::Uuid::new_v4(),
            role: Role::Assistant,
            content: MessageContent::from("first"),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id,
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        });
        let before = app.render_dirty_state();

        app.messages.last_mut().expect("message").content = MessageContent::from("second");

        assert!(app.render_dirty_state().diff(before).transcript);
    }

    #[test]
    fn set_interaction_mode_maps_to_policy_permission_mode() {
        let mut app = test_app();

        app.set_interaction_mode(InteractionMode::Plan);
        assert_eq!(app.permission_mode, crate::policy::PermissionMode::Plan);

        app.set_interaction_mode(InteractionMode::AutoReview);
        assert_eq!(
            app.permission_mode,
            crate::policy::PermissionMode::AcceptEdits
        );

        app.set_interaction_mode(InteractionMode::FullAccess);
        assert_eq!(app.permission_mode, crate::policy::PermissionMode::Bypass);

        app.set_interaction_mode(InteractionMode::Ask);
        assert_eq!(app.permission_mode, crate::policy::PermissionMode::Default);
    }

    fn test_app_with_root(project_root: PathBuf) -> TuiApp {
        let mut app = TuiApp::new(DeepSeekModel::Flash, ThinkingMode::Auto, None, project_root);
        app.api_key_entry = None;
        app.set_api_key_state(ApiKeyState::Ready);
        app.status_message = "Ready".into();
        app
    }

    fn subagent_policy_decision(
        tool_name: &str,
        arguments: &str,
        project_root: &std::path::Path,
    ) -> crate::policy::PolicyDecision {
        crate::policy::evaluate_tool(
            tool_name,
            arguments,
            project_root,
            &crate::storage::config::PolicyConfig::default(),
        )
        .with_source(crate::policy::ToolCallSource::Subagent)
    }

    fn test_session(reasoning_state: ReasoningState) -> Session {
        Session {
            id: SessionId::new_v4(),
            name: None,
            project_root: PathBuf::from("D:/octocode"),
            messages: Vec::new(),
            reasoning_state,
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            metadata: SessionMetadata::default(),
        }
    }

    fn key(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::empty(), kind)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, kind)
    }

    #[test]
    fn session_snapshot_preserves_active_model() {
        let mut app = test_app();
        app.model = DeepSeekModel::Pro;

        let session = app.session_snapshot(Path::new("D:/octocode"));

        assert_eq!(
            session.reasoning_state.selected_model,
            Some(DeepSeekModel::Pro)
        );
        assert_eq!(
            session.reasoning_state.effective_model(),
            DeepSeekModel::Pro
        );
    }

    #[test]
    fn top_model_badge_renders_full_name_at_right_edge() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 1)).expect("terminal");

        terminal
            .draw(|f| {
                render_canvas(f, f.area());
                render_core::render_top_model_badge(f, f.area(), &DeepSeekModel::Pro);
            })
            .expect("draw");

        let line = line_symbols_to_text(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(ratatui::buffer::Cell::symbol),
        );
        assert!(line.ends_with("DeepSeek V4 Pro"));
    }

    #[test]
    fn tui_new_session_uses_requested_model() {
        let mut session = test_session(ReasoningState::default());

        let active_model = apply_session_model_selection(
            &mut session,
            DeepSeekModel::Pro,
            false,
            false,
            ThinkingMode::Auto,
        );

        assert_eq!(active_model, DeepSeekModel::Pro);
        assert_eq!(
            session.reasoning_state.selected_model,
            Some(DeepSeekModel::Pro)
        );
    }

    #[test]
    fn tui_resumed_session_preserves_selected_model_without_override() {
        let mut session = test_session(ReasoningState {
            selected_model: Some(DeepSeekModel::Pro),
            ..ReasoningState::default()
        });

        let active_model = apply_session_model_selection(
            &mut session,
            DeepSeekModel::Flash,
            false,
            true,
            ThinkingMode::Auto,
        );

        assert_eq!(active_model, DeepSeekModel::Pro);
        assert_eq!(
            session.reasoning_state.selected_model,
            Some(DeepSeekModel::Pro)
        );
    }

    #[test]
    fn tui_resumed_legacy_session_preserves_effort_fallback_without_override() {
        let mut session = test_session(ReasoningState {
            effort: crate::deepseek::ReasoningEffort::High,
            selected_model: None,
            ..ReasoningState::default()
        });

        let active_model = apply_session_model_selection(
            &mut session,
            DeepSeekModel::Flash,
            false,
            true,
            ThinkingMode::Auto,
        );

        assert_eq!(active_model, DeepSeekModel::Pro);
        assert_eq!(session.reasoning_state.selected_model, None);
    }

    #[test]
    fn tui_model_override_replaces_resumed_session_model() {
        let mut session = test_session(ReasoningState {
            selected_model: Some(DeepSeekModel::Pro),
            ..ReasoningState::default()
        });

        let active_model = apply_session_model_selection(
            &mut session,
            DeepSeekModel::Flash,
            true,
            true,
            ThinkingMode::Auto,
        );

        assert_eq!(active_model, DeepSeekModel::Flash);
        assert_eq!(
            session.reasoning_state.selected_model,
            Some(DeepSeekModel::Flash)
        );
    }

    #[test]
    fn renderer_mode_auto_uses_fullscreen_in_normal_terminals_and_classic_in_embedded() {
        let normal = TerminalEnvironment {
            stdin_is_terminal: true,
            stdout_is_terminal: true,
            term: Some("xterm-256color".to_string()),
            embedded_host: false,
        };
        let embedded = TerminalEnvironment {
            stdin_is_terminal: true,
            stdout_is_terminal: true,
            term: Some("xterm-256color".to_string()),
            embedded_host: true,
        };

        // Auto now defaults to Classic (inline viewport) for both normal and
        // probe-skipping terminals, so the transcript lives in scrollback.
        assert_eq!(
            RendererMode::from_config_for_environment("auto", &normal),
            RendererMode::Classic
        );
        assert_eq!(
            RendererMode::from_config_for_environment("", &normal),
            RendererMode::Classic
        );
        assert_eq!(
            RendererMode::from_config_for_environment("auto", &embedded),
            RendererMode::Classic
        );
        assert_eq!(
            RendererMode::from_config_for_environment("classic", &normal),
            RendererMode::Classic
        );
        assert_eq!(
            RendererMode::from_config_for_environment("fullscreen", &embedded),
            RendererMode::Fullscreen
        );
        assert_eq!(
            RendererMode::from_config_for_environment("alt", &normal),
            RendererMode::Fullscreen
        );
    }

    #[test]
    fn classic_viewport_stays_compact_in_tall_terminals() {
        assert_eq!(classic_viewport_height_for_terminal(24), 14);
        assert_eq!(classic_viewport_height_for_terminal(40), 22);
        assert_eq!(classic_viewport_height_for_terminal(80), 22);
    }

    #[test]
    fn classic_prefers_fixed_viewport_when_space_exists_below_prompt() {
        assert_eq!(
            classic_fixed_viewport_area_for_terminal(120, 40, 5),
            Some(Rect::new(0, 5, 120, 35))
        );
        assert_eq!(classic_fixed_viewport_area_for_terminal(120, 40, 30), None);
    }

    #[test]
    fn terminal_environment_rejects_non_tty_tui() {
        let env = TerminalEnvironment {
            stdin_is_terminal: false,
            stdout_is_terminal: true,
            term: Some("xterm-256color".to_string()),
            embedded_host: false,
        };

        let error = env
            .ensure_tui_supported()
            .expect_err("non-tty stdin should not start TUI")
            .to_string();

        assert!(error.contains("TUI requires an interactive terminal"));
        assert!(error.contains("stdin: not a tty"));
    }

    #[test]
    fn embedded_and_dumb_term_skip_cursor_position_probe() {
        let embedded = TerminalEnvironment {
            stdin_is_terminal: true,
            stdout_is_terminal: true,
            term: Some("xterm-256color".to_string()),
            embedded_host: true,
        };
        let dumb = TerminalEnvironment {
            stdin_is_terminal: true,
            stdout_is_terminal: true,
            term: Some("dumb".to_string()),
            embedded_host: false,
        };

        assert!(embedded.ensure_tui_supported().is_ok());
        assert!(embedded.skips_cursor_position_probe());
        assert!(dumb.ensure_tui_supported().is_ok());
        assert!(dumb.skips_cursor_position_probe());
    }

    #[test]
    fn release_key_events_do_not_duplicate_typing() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Release), &tx);

        assert_eq!(app.input_text, "a");
        assert_eq!(app.cursor_pos, 1);
    }

    #[test]
    fn welcome_digit_key_stays_literal_input() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();

        assert!(app.is_showing_welcome());
        app.handle_key(key(KeyCode::Char('1'), KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "1");
        assert_eq!(app.cursor_pos, 1);
        assert!(!app.status_message.contains("Loaded launch prompt"));
    }

    #[test]
    fn enter_while_streaming_queues_user_input() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_text = "下一步继续".to_string();
        app.cursor_pos = app.input_text.chars().count();
        app.is_streaming = true;

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.queued_inputs.len(), 1);
        assert_eq!(
            app.queued_inputs.front().map(String::as_str),
            Some("下一步继续")
        );
        assert!(app.input_text.is_empty());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn enter_while_streaming_keeps_followups_fifo() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.is_streaming = true;

        app.input_text = "第一条".to_string();
        app.cursor_pos = app.input_text.chars().count();
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        app.input_text = "第二条".to_string();
        app.cursor_pos = app.input_text.chars().count();
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(rx.try_recv().is_err());
        assert_eq!(app.queued_inputs.len(), 2);
        assert_eq!(app.queued_inputs.pop_front().as_deref(), Some("第一条"));
        assert_eq!(app.queued_inputs.pop_front().as_deref(), Some("第二条"));
        assert!(app.status_message.contains("2"));
    }

    #[test]
    fn single_digit_enter_while_streaming_is_queued_and_visible() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.is_streaming = true;
        app.input_text = "1".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(rx.try_recv().is_err());
        assert_eq!(app.queued_inputs.front().map(String::as_str), Some("1"));
        assert!(app.input_text.is_empty());
        assert!(app.status_message.contains("已排队"));
    }

    #[test]
    fn repeat_char_events_do_not_duplicate_typing() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Repeat), &tx);

        assert_eq!(app.input_text, "a");
        assert_eq!(app.cursor_pos, 1);
    }

    #[test]
    fn left_right_keys_allow_inserting_inside_existing_text() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "abcd".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Left, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Left, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char('X'), KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "abXcd");
        assert_eq!(app.cursor_pos, 3);

        app.handle_key(key(KeyCode::Right, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char('Y'), KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "abXcYd");
        assert_eq!(app.cursor_pos, 5);
    }

    #[test]
    fn home_end_and_delete_edit_at_cursor() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "你好世界".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Home, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char('新'), KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Right, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Delete, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::End, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char('!'), KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "新你世界!");
        assert_eq!(app.cursor_pos, app.input_text.chars().count());
    }

    #[test]
    fn paste_inserts_text_without_submitting() {
        let mut app = test_app();
        let (_tx, mut rx) = mpsc::unbounded_channel::<TuiAction>();

        app.handle_paste("开蜂群，审查 src/agent/swarm.rs\n");

        assert_eq!(app.input_text, "开蜂群，审查 src/agent/swarm.rs");
        assert_eq!(app.cursor_pos, app.input_text.chars().count());
        assert!(rx.try_recv().is_err());
        assert!(app.status_message.contains("press Enter"));
    }

    #[test]
    fn paste_can_insert_multiline_text_at_cursor() {
        let mut app = test_app();
        app.input_text = "ab".into();
        app.cursor_pos = 1;

        app.handle_paste("一\r\n二");

        assert_eq!(app.input_text, "a一\n二b");
        assert_eq!(app.cursor_pos, 4);
    }

    #[test]
    fn enter_submits_input_without_starting_turn_in_key_handler() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        match rx.try_recv().expect("submit action") {
            TuiAction::Submit(input) => assert_eq!(input, "hello"),
            _ => panic!("expected submit action"),
        }
        assert_eq!(app.input_text, "hello");
        assert_eq!(app.cursor_pos, 5);
        assert!(!app.is_streaming);
    }

    #[test]
    fn local_slash_commands_can_submit_without_api_key() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.set_api_key_state(ApiKeyState::Missing);
        app.api_key_entry = None;
        app.input_text = "/help".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        match rx.try_recv().expect("submit action") {
            TuiAction::Submit(input) => assert_eq!(input, "/help"),
            _ => panic!("expected submit action"),
        }
    }

    #[test]
    fn api_key_state_controls_agent_submit_gate() {
        let mut app = test_app();

        app.set_api_key_state(ApiKeyState::Ready);
        app.welcome.api_key_status = "missing";
        assert!(!app.should_block_agent_turn_for_api_key());

        app.set_api_key_state(ApiKeyState::Missing);
        app.welcome.api_key_status = "ready";
        assert!(app.should_block_agent_turn_for_api_key());
    }

    #[test]
    fn shell_command_parser_accepts_bang_prefix() {
        assert_eq!(
            shell_command_from_input(" ! cargo check ").expect("parse shell command"),
            Some("cargo check".to_string())
        );
        assert_eq!(
            shell_command_from_input("hello").expect("parse normal input"),
            None
        );
        assert!(shell_command_from_input("!   ").is_err());
    }

    #[test]
    fn shell_tool_call_uses_run_command_schema() {
        let call = shell_tool_call("cargo check");
        let args: serde_json::Value =
            serde_json::from_str(&call.function.arguments).expect("valid arguments");

        assert_eq!(call.function.name, "run_command");
        assert_eq!(args["command"], "cargo check");
    }

    #[test]
    fn task_title_summarizes_english_prompt() {
        assert_eq!(
            summarize_task_title("Fix the input color contrast and cursor rendering"),
            "Fix the input color contrast"
        );
    }

    #[test]
    fn task_title_summarizes_chinese_prompt() {
        assert_eq!(
            summarize_task_title("修复输入框颜色和光标显示，让它更像主流编程助手"),
            "修复输入框"
        );
    }

    #[test]
    fn plan_step_summary_preserves_swarm_agent_role_and_task() {
        assert_eq!(
            summarize_plan_step("agent explorer · 阅读并定位 src/agent/swarm.rs"),
            "agent explorer · 阅读并定位 src/agent/swarm.rs"
        );
        assert_eq!(
            summarize_plan_step("agent reviewer · 审查关键风险并汇总"),
            "agent reviewer · 审查关键风险并汇总"
        );
    }

    #[test]
    fn task_title_extracts_keywords_from_chinese_filler() {
        assert_eq!(
            summarize_task_title("我要进行测试 我新写了个 CLI"),
            "测试 CLI"
        );
        assert_eq!(
            summarize_task_title("这个流式输出怎么上面就卡住了呢"),
            "修复流式输出"
        );
        assert_eq!(
            summarize_task_title("你是否能够读取电脑里的文件？"),
            "读取本地文件"
        );
        assert_eq!(summarize_task_title("你是谁"), "回答问题");
    }

    #[test]
    fn begin_running_turn_sets_task_title() {
        let mut app = test_app();

        app.begin_running_turn("修复输入框颜色和光标显示");

        assert_eq!(app.current_task_title, "修复输入框");
        assert_eq!(
            app.pending_user_message.as_deref(),
            Some("修复输入框颜色和光标显示")
        );
        assert!(app.is_streaming);
    }

    #[test]
    fn running_turn_renders_transcript_instead_of_welcome() {
        let mut app = test_app();
        assert!(app.is_showing_welcome());

        app.begin_running_turn("苹果怎么截图");
        app.apply_agent_event(AgentEvent::ContentDelta(
            "可以按 Shift + Command + 4 选择区域截图。".to_string(),
        ));

        let backend = ratatui::backend::TestBackend::new(115, 33);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");
        let snapshot = buffer_to_text(terminal.backend());
        let compact_snapshot = snapshot.replace(' ', "");

        assert!(!app.is_showing_welcome());
        assert!(compact_snapshot.contains("苹果怎么截图"));
        assert!(snapshot.contains("Shift + Command + 4"));
        assert!(!snapshot.contains("What are we changing today?"));
        assert!(!snapshot.contains("Changelog"));
    }

    #[test]
    fn app_uses_inline_cursor_without_terminal_cursor() {
        let mut app = test_app();
        app.input_text = "测试".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let backend = ratatui::backend::TestBackend::new(115, 33);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");

        let snapshot = buffer_to_text(terminal.backend());
        assert!(snapshot.contains("测试▌"));
        terminal.backend_mut().assert_cursor_position((0, 0));
    }

    #[test]
    fn cancel_running_work_clears_modal_state_and_marks_running_plan_failed() {
        let mut app = test_app();
        let (approval_tx, _approval_rx) = tokio::sync::oneshot::channel();
        let (options_tx, _options_rx) = tokio::sync::oneshot::channel();
        app.begin_running_turn("执行计划");
        app.approval = Some((
            ApprovalDisplay {
                title: "run_command".into(),
                description: "cargo test".into(),
                risk_level: crate::policy::RiskLevel::CommandExecution,
                details: "test".into(),
            },
            approval_tx,
        ));
        app.options_needed = Some(DecisionPrompt {
            kind: DecisionKind::Clarification,
            title: "Choose".into(),
            options: vec!["A".into(), "B".into()],
            respond: options_tx,
        });
        app.plan_steps = vec![
            plan_tracker::PlanStepItem::new("one", plan_tracker::PlanStepStatus::Done),
            plan_tracker::PlanStepItem::new("two", plan_tracker::PlanStepStatus::Running),
        ];

        app.cancel_running_work();

        assert!(!app.is_streaming);
        assert!(app.approval.is_none());
        assert!(app.options_needed.is_none());
        assert_eq!(
            app.plan_steps[1].status,
            plan_tracker::PlanStepStatus::Failed
        );
        assert!(app.stream_buffer.contains("Cancelled"));
        assert_eq!(app.status_message, "Interrupted — all running work stopped");
    }

    #[test]
    fn assistant_lists_do_not_auto_create_option_picker() {
        let mut app = test_app();
        app.begin_running_turn("测试 CLI 推荐项");

        app.apply_agent_event(AgentEvent::ContentDelta(
            "Choose one:\n\n1. First\n2. Second\n".to_string(),
        ));

        assert!(app.pending_options.is_none());

        app.apply_agent_event(AgentEvent::TurnComplete {
            session_id: SessionId::new_v4(),
            total_tokens: 42,
        });

        assert!(app.pending_options.is_none());
        assert_eq!(app.status_message, "Done — 42 tokens ¥0.000");
    }

    #[test]
    fn activity_tokens_are_current_turn_not_session_total() {
        let mut app = test_app();
        app.total_tokens = 5_000;
        app.begin_running_turn("检查滚动和 token");

        assert_eq!(app.current_turn_input_tokens, 0);
        assert_eq!(app.current_turn_output_tokens, 0);
        assert_eq!(app.activity_input_tokens(), 0);
        assert_eq!(app.visible_input_tokens(), 0);

        app.apply_agent_event(AgentEvent::TokenDelta {
            input_tokens: 240,
            output_tokens: 0,
        });

        assert_eq!(app.current_turn_input_tokens, 240);
        assert!(app.activity_input_tokens() > 0);
        assert!(app.activity_input_tokens() < 240);
        assert!(app.visible_input_tokens() > 0);
        assert!(app.visible_input_tokens() < 240);

        let old_enough = std::time::Instant::now() - std::time::Duration::from_secs(14);
        app.stream_start = Some(old_enough);
        app.input_token_animation_started = Some(old_enough);
        assert_eq!(app.activity_input_tokens(), 240);
        assert_eq!(app.visible_input_tokens(), 240);

        app.apply_agent_event(AgentEvent::StreamDone {
            finish_reason: None,
            usage: Some(crate::deepseek::Usage {
                prompt_tokens: 100,
                completion_tokens: 23,
                total_tokens: 123,
                prompt_cache_hit_tokens: None,
                prompt_cache_miss_tokens: None,
                prompt_tokens_details: None,
            }),
            cache: None,
        });

        assert_eq!(app.current_turn_tokens, 263);
        assert_eq!(app.current_turn_input_tokens, 240);
        assert_eq!(app.current_turn_output_tokens, 23);
        assert_eq!(app.activity_input_tokens(), 240);
        assert_eq!(app.visible_input_tokens(), 240);
        assert_eq!(app.total_tokens, 5_123);
        assert!(app.total_cost > 0.0);
    }

    #[test]
    fn input_token_activity_counts_up_before_output_arrives() {
        assert_eq!(animated_token_count(0, 0), 0);
        assert_eq!(animated_token_count(7_200, 0), 40);
        assert!(animated_token_count(213, 5_000) < 213);
        assert!(animated_token_count(7_200, 2_000) < 7_200);
        assert!(animated_token_count(7_200, 8_000) < 7_200);
        assert_eq!(animated_token_count(7_200, 15_000), 7_200);
    }

    #[test]
    fn finalized_input_usage_still_animates_while_turn_is_running() {
        let mut app = test_app();
        app.begin_running_turn("处理请求");

        app.apply_agent_event(AgentEvent::StreamDone {
            finish_reason: None,
            usage: Some(crate::deepseek::Usage {
                prompt_tokens: 213,
                completion_tokens: 0,
                total_tokens: 213,
                prompt_cache_hit_tokens: None,
                prompt_cache_miss_tokens: None,
                prompt_tokens_details: None,
            }),
            cache: None,
        });

        assert_eq!(app.current_turn_input_tokens, 213);
        assert!(app.activity_input_tokens() > 0);
        assert!(app.activity_input_tokens() < 213);
        assert!(app.visible_input_tokens() > 0);
        assert!(app.visible_input_tokens() < 213);
    }

    #[test]
    fn stream_deltas_update_live_output_token_estimate() {
        let mut app = test_app();
        app.begin_running_turn("比较 CLI 工具");
        let input_tokens = app.current_turn_input_tokens;

        app.apply_agent_event(AgentEvent::ContentDelta(
            "这是一个正在流式输出的回答".to_string(),
        ));

        assert_eq!(app.current_turn_input_tokens, input_tokens);
        assert!(app.current_turn_output_tokens > 0);
        assert_eq!(app.activity_input_tokens(), input_tokens);
        assert_eq!(
            app.current_turn_tokens,
            app.current_turn_input_tokens + app.current_turn_output_tokens
        );
    }

    #[test]
    fn subagent_usage_does_not_pollute_live_activity_tokens() {
        let mut app = test_app();
        app.begin_running_turn("开蜂群，审查代码");

        app.apply_agent_event(AgentEvent::SubagentStarted {
            agent_id: "agent-1".to_string(),
            agent_type: "explorer".to_string(),
            description: "定位入口".to_string(),
            is_background: false,
        });
        app.apply_agent_event(AgentEvent::SubagentDelta {
            agent_id: "agent-1".to_string(),
            content: "正在读取 src/tui/app.rs".to_string(),
        });
        let after_delta = app.current_turn_output_tokens;
        assert!(after_delta > 0);

        app.apply_agent_event(AgentEvent::SubagentCompleted {
            agent_id: "agent-1".to_string(),
            result: crate::agent::subagent::SubagentResult {
                success: true,
                summary: "完成定位".to_string(),
                output: "完成定位".to_string(),
                tool_calls_used: vec!["read_file".to_string()],
                files_read: vec!["src/tui/app.rs".to_string()],
                files_written: Vec::new(),
                duration_ms: 1_000,
                token_usage: 16_160,
                error: None,
                started_at: chrono::Utc::now(),
                completed_at: chrono::Utc::now(),
                worktree: None,
            },
        });

        assert_eq!(app.current_turn_output_tokens, after_delta);
        assert_eq!(
            app.current_turn_tokens,
            app.current_turn_input_tokens + app.current_turn_output_tokens
        );
        let card = app.subagents.iter().find(|card| card.agent_id == "agent-1");
        assert_eq!(card.map(|card| card.token_usage), Some(16_160));
        assert_eq!(app.live_agent_tokens(), 16_160);
    }

    #[test]
    fn token_delta_updates_live_output_without_visible_text() {
        let mut app = test_app();
        app.begin_running_turn("开蜂群，审查代码");

        app.apply_agent_event(AgentEvent::TokenDelta {
            input_tokens: 0,
            output_tokens: 24,
        });

        assert_eq!(app.current_turn_output_tokens, 24);
        assert_eq!(app.current_turn_tokens, app.current_turn_input_tokens + 24);
        assert!(app.stream_buffer.is_empty());
    }

    #[test]
    fn reasoning_only_turn_shows_visible_notice() {
        let mut app = test_app();
        app.config.ui.language = "zh-CN".to_string();
        app.begin_running_turn("检查一下这个 CLI");

        app.apply_agent_event(AgentEvent::ReasoningDelta(
            "I should inspect the CLI before answering.".to_string(),
        ));
        app.apply_agent_event(AgentEvent::TurnComplete {
            session_id: app.session_id.unwrap_or_else(SessionId::new_v4),
            total_tokens: 654,
        });

        assert!(app.stream_buffer.contains("只返回了隐藏推理内容"));
        assert!(app.reasoning_buffer.is_empty());
        assert!(!app.is_streaming);
    }

    #[test]
    fn subagent_workspace_list_dir_is_auto_approved() {
        let root = tempfile::tempdir().expect("workspace");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let arguments = serde_json::json!({ "path": ".", "recursive": true }).to_string();

        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-12345678".to_string(),
            tool_name: "list_dir".to_string(),
            arguments: arguments.clone(),
            policy_decision: subagent_policy_decision("list_dir", &arguments, root.path()),
            respond: tx,
        });

        assert!(app.approval.is_none());
        assert!(matches!(rx.try_recv(), Ok(true)));
    }

    #[test]
    fn subagent_sensitive_read_approval_is_localized_and_summarized() {
        let root = tempfile::tempdir().expect("workspace");
        let outside = tempfile::NamedTempFile::new().expect("outside file");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let arguments = serde_json::json!({ "path": outside.path().to_string_lossy() }).to_string();

        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-abcdef123456".to_string(),
            tool_name: "read_file".to_string(),
            arguments: arguments.clone(),
            policy_decision: subagent_policy_decision("read_file", &arguments, root.path()),
            respond: tx,
        });

        let Some((display, _)) = app.approval.as_ref() else {
            panic!("expected approval");
        };
        assert_eq!(display.risk_level, crate::policy::RiskLevel::SensitiveRead);
        assert!(display.title.contains("子 agent"));
        assert!(display.description.contains("敏感路径"));
        assert!(display.details.contains("来源: 子 agent subagent"));
        assert!(display.details.contains("路径:"));
        assert!(!display.details.contains("Arguments"));
        assert!(!display.details.contains("{\"path\""));
    }

    #[test]
    fn subagent_approval_updates_card_to_waiting() {
        let root = tempfile::tempdir().expect("workspace");
        let outside = tempfile::NamedTempFile::new().expect("outside file");
        let mut app = test_app_with_root(root.path().to_path_buf());
        app.begin_running_turn("审查敏感文件读取");
        app.apply_agent_event(AgentEvent::SubagentStarted {
            agent_id: "subagent-waiting".to_string(),
            agent_type: "reviewer".to_string(),
            description: "检查配置".to_string(),
            is_background: false,
        });
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let arguments = serde_json::json!({ "path": outside.path().to_string_lossy() }).to_string();

        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-waiting".to_string(),
            tool_name: "read_file".to_string(),
            arguments: arguments.clone(),
            policy_decision: subagent_policy_decision("read_file", &arguments, root.path()),
            respond: tx,
        });

        let card = app
            .subagents
            .iter()
            .find(|card| card.agent_id == "subagent-waiting")
            .expect("subagent card");
        assert_eq!(
            card.status,
            subagent_cards::SubagentCardStatus::WaitingApproval
        );
        assert_eq!(
            card.last_update.as_deref(),
            Some("waiting approval for read_file")
        );

        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(100, 24)).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");
        let rendered = buffer_to_text(terminal.backend());
        assert!(rendered.contains("waiting"));
        assert!(rendered.contains("waiting approval for read_file"));
    }

    #[test]
    fn subagent_denied_tool_updates_card_to_blocked() {
        let mut app = test_app();
        app.begin_running_turn("执行子 agent");
        app.apply_agent_event(AgentEvent::SubagentStarted {
            agent_id: "subagent-blocked".to_string(),
            agent_type: "worker".to_string(),
            description: "写入受保护路径".to_string(),
            is_background: false,
        });
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let policy_decision = crate::policy::PolicyDecision::deny(
            "protected path",
            "blocked",
            "blocked",
            crate::policy::RiskLevel::Blocked,
            "path: .git/config",
        )
        .with_source(crate::policy::ToolCallSource::Subagent);

        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-blocked".to_string(),
            tool_name: "write_file".to_string(),
            arguments: r#"{"path":".git/config"}"#.to_string(),
            policy_decision,
            respond: tx,
        });

        assert!(matches!(rx.try_recv(), Ok(false)));
        let card = app
            .subagents
            .iter()
            .find(|card| card.agent_id == "subagent-blocked")
            .expect("subagent card");
        assert_eq!(card.status, subagent_cards::SubagentCardStatus::Blocked);
        assert_eq!(card.last_update.as_deref(), Some("blocked: protected path"));
        assert!(app.approval.is_none());
    }

    #[test]
    fn concurrent_subagent_approvals_are_queued() {
        let root = tempfile::tempdir().expect("workspace");
        let outside_one = tempfile::NamedTempFile::new().expect("outside file one");
        let outside_two = tempfile::NamedTempFile::new().expect("outside file two");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx_one, mut rx_one) = tokio::sync::oneshot::channel();
        let (tx_two, mut rx_two) = tokio::sync::oneshot::channel();
        let arguments_one =
            serde_json::json!({ "path": outside_one.path().to_string_lossy() }).to_string();
        let arguments_two =
            serde_json::json!({ "path": outside_two.path().to_string_lossy() }).to_string();

        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-one".to_string(),
            tool_name: "read_file".to_string(),
            arguments: arguments_one.clone(),
            policy_decision: subagent_policy_decision("read_file", &arguments_one, root.path()),
            respond: tx_one,
        });
        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-two".to_string(),
            tool_name: "read_file".to_string(),
            arguments: arguments_two.clone(),
            policy_decision: subagent_policy_decision("read_file", &arguments_two, root.path()),
            respond: tx_two,
        });

        assert!(app.approval.is_some());
        assert_eq!(app.approval_queue.len(), 1);
        assert!(matches!(
            rx_one.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            rx_two.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        let (action_tx, _action_rx) = mpsc::unbounded_channel();
        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Press), &action_tx);

        assert!(matches!(rx_one.try_recv(), Ok(true)));
        assert!(app.approval.is_some());
        assert_eq!(app.approval_queue.len(), 0);
        assert!(matches!(
            rx_two.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        app.handle_key(key(KeyCode::Char('d'), KeyEventKind::Press), &action_tx);
        assert!(matches!(rx_two.try_recv(), Ok(false)));
        assert!(app.approval.is_none());
    }

    #[test]
    fn approval_keyboard_selection_confirms_session_choice() {
        let mut app = test_app();
        let (approval_tx, mut approval_rx) = tokio::sync::oneshot::channel();
        let (action_tx, _action_rx) = mpsc::unbounded_channel();

        app.approval = Some((
            ApprovalDisplay {
                title: "Run Command".into(),
                description: "cargo test".into(),
                risk_level: crate::policy::RiskLevel::CommandExecution,
                details: "command: cargo test".into(),
            },
            approval_tx,
        ));

        app.handle_key(key(KeyCode::Right, KeyEventKind::Press), &action_tx);
        assert_eq!(app.approval_selected_index, 1);

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &action_tx);

        assert!(matches!(approval_rx.try_recv(), Ok(true)));
        assert!(app.session_auto_approve);
        assert!(app.approval.is_none());
    }

    #[test]
    fn approval_keyboard_selection_can_deny_with_enter_or_escape() {
        let mut app = test_app();
        let (approval_tx, mut approval_rx) = tokio::sync::oneshot::channel();
        let (action_tx, _action_rx) = mpsc::unbounded_channel();

        app.approval = Some((
            ApprovalDisplay {
                title: "Run Command".into(),
                description: "cat file".into(),
                risk_level: crate::policy::RiskLevel::CommandExecution,
                details: "command: cat file".into(),
            },
            approval_tx,
        ));

        app.handle_key(key(KeyCode::Right, KeyEventKind::Press), &action_tx);
        app.handle_key(key(KeyCode::Right, KeyEventKind::Press), &action_tx);
        assert_eq!(app.approval_selected_index, 2);

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &action_tx);

        assert!(matches!(approval_rx.try_recv(), Ok(false)));
        assert!(app.approval.is_none());

        let (approval_tx, mut approval_rx) = tokio::sync::oneshot::channel();
        app.approval = Some((
            ApprovalDisplay {
                title: "Run Command".into(),
                description: "cat file".into(),
                risk_level: crate::policy::RiskLevel::CommandExecution,
                details: "command: cat file".into(),
            },
            approval_tx,
        ));

        app.handle_key(key(KeyCode::Esc, KeyEventKind::Press), &action_tx);

        assert!(matches!(approval_rx.try_recv(), Ok(false)));
        assert!(app.approval.is_none());
    }

    #[test]
    fn approval_overlay_stays_above_input_boundary() {
        let area = Rect::new(0, 0, 100, 30);

        assert_eq!(approval_overlay_area(area, 24), Rect::new(0, 0, 100, 24));
    }

    #[test]
    fn visible_context_tokens_include_live_turn_estimate_until_usage_arrives() {
        let mut app = test_app();
        app.total_tokens = 5_000;
        app.begin_running_turn("比较 CLI 工具");
        app.apply_agent_event(AgentEvent::ContentDelta(
            "这是一个正在流式输出的回答".to_string(),
        ));

        assert_eq!(
            app.visible_context_tokens(),
            5_000 + app.current_turn_tokens
        );

        app.apply_agent_event(AgentEvent::StreamDone {
            finish_reason: None,
            usage: Some(crate::deepseek::Usage {
                prompt_tokens: 100,
                completion_tokens: 23,
                total_tokens: 123,
                prompt_cache_hit_tokens: None,
                prompt_cache_miss_tokens: None,
                prompt_tokens_details: None,
            }),
            cache: None,
        });

        assert_eq!(app.visible_context_tokens(), 5_123);
    }

    #[test]
    fn ctrl_arrow_scrolls_transcript_without_editing_input() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(
            modified_key(KeyCode::Up, KeyModifiers::CONTROL, KeyEventKind::Press),
            &tx,
        );

        assert_eq!(app.scroll_offset, 3);
        assert_eq!(app.input_text, "hello");
        assert_eq!(app.cursor_pos, 5);

        app.handle_key(
            modified_key(KeyCode::Down, KeyModifiers::CONTROL, KeyEventKind::Press),
            &tx,
        );

        assert_eq!(app.scroll_offset, 0);
        assert_eq!(app.input_text, "hello");
    }

    #[test]
    fn ctrl_end_jumps_to_latest_even_with_input_text() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "draft".to_string();
        app.cursor_pos = app.input_text.chars().count();
        app.scroll_older(12);

        app.handle_key(
            modified_key(KeyCode::End, KeyModifiers::CONTROL, KeyEventKind::Press),
            &tx,
        );

        assert_eq!(app.scroll_offset, 0);
        assert_eq!(app.input_text, "draft");
    }

    #[test]
    fn transcript_scroll_helpers_saturate_at_latest() {
        let mut app = test_app();

        app.scroll_older(8);
        app.scroll_newer(3);
        app.scroll_newer(99);

        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn jump_to_bottom_hint_renders_when_scrolled_up() {
        let mut app = test_app();
        app.begin_running_turn("检查滚动提示");
        app.apply_agent_event(AgentEvent::ContentDelta(
            "第一行\n第二行\n第三行\n第四行\n第五行\n第六行\n第七行\n第八行\n".to_string(),
        ));
        app.scroll_older(4);

        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(100, 24)).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");
        let rendered = buffer_to_text(terminal.backend());

        assert!(rendered.contains("Jump to bottom (Ctrl+End)"));
    }

    #[test]
    fn mouse_wheel_scrolls_transcript() {
        let mut app = test_app();
        app.renderer_mode = RendererMode::Fullscreen;

        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(app.scroll_offset, MOUSE_SCROLL_LINES);

        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn classic_renderer_leaves_mouse_wheel_to_terminal_scrollback() {
        let mut app = test_app();
        app.renderer_mode = RendererMode::Classic;

        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        });

        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn tab_completes_unique_slash_command() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "/doc".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Tab, KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "/doctor ");
        assert_eq!(app.cursor_pos, 8);
        assert_eq!(app.status_message, "命令：/doctor");
    }

    #[test]
    fn slash_suggestions_show_for_root_slash() {
        let mut app = test_app();
        app.input_text = "/".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let suggestions = app.slash_command_suggestions();

        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().all(|(name, _)| name.starts_with('/')));
        assert!(suggestions
            .iter()
            .any(|(name, desc)| name == "/agents" && desc.contains("智能体")));
        assert!(suggestions
            .iter()
            .filter(|(name, _)| name == "/doctor")
            .all(|(_, desc)| !desc.starts_with("命令 ·")));
        assert!(!suggestions
            .iter()
            .any(|(name, desc)| name == "/agents" && desc.contains("List built-in")));
    }

    #[test]
    fn slash_suggestion_registry_cache_refreshes_custom_command_edits() {
        let temp = tempfile::tempdir().expect("tempdir");
        let command_dir = temp.path().join(".octocode").join("commands");
        std::fs::create_dir_all(&command_dir).expect("mkdir commands");
        let command_path = command_dir.join("zz-cache-refresh.md");
        std::fs::write(
            &command_path,
            "---\ndescription: first cached description\n---\nDo $ARGUMENTS",
        )
        .expect("write command");

        let mut app = test_app_with_root(temp.path().to_path_buf());
        app.input_text = "/zz-cache".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let first = app.slash_command_suggestions();
        assert!(first.iter().any(|(name, desc)| {
            name == "/zz-cache-refresh" && desc.contains("first cached description")
        }));

        std::fs::write(
            &command_path,
            "---\ndescription: second cached description with more bytes\n---\nDo $ARGUMENTS",
        )
        .expect("rewrite command");

        let second = app.slash_command_suggestions();
        assert!(second.iter().any(|(name, desc)| {
            name == "/zz-cache-refresh" && desc.contains("second cached description")
        }));
    }

    #[test]
    fn slash_suggestion_enter_completes_partial_command() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "/doc".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "/doctor ");
    }

    #[test]
    fn exact_slash_command_can_submit_while_turn_is_running() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.is_streaming = true;
        app.input_text = "/model".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(matches!(
            rx.try_recv().expect("submit action"),
            TuiAction::Submit(input) if input == "/model"
        ));
    }

    #[test]
    fn normal_input_is_queued_while_turn_is_running() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.is_streaming = true;
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(rx.try_recv().is_err());
        assert_eq!(app.queued_inputs.front().map(String::as_str), Some("hello"));
        assert!(app.input_text.is_empty());
        assert!(app.status_message.contains("已排队"));
    }

    #[test]
    fn slash_suggestions_complete_matching_alias() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "/han".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "/handoff ");
        assert_eq!(app.cursor_pos, 9);
    }

    #[test]
    fn shift_tab_cycles_interaction_mode() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(key(KeyCode::BackTab, KeyEventKind::Press), &tx);
        assert_eq!(app.interaction_mode, InteractionMode::Plan);
        assert_eq!(
            app.input_for_interaction_mode("inspect workspace".into()),
            "/plan inspect workspace"
        );

        app.handle_key(key(KeyCode::BackTab, KeyEventKind::Press), &tx);
        assert_eq!(app.interaction_mode, InteractionMode::AutoReview);
        assert_eq!(
            app.input_for_interaction_mode("inspect workspace".into()),
            "/review inspect workspace"
        );
    }

    #[test]
    fn tab_completes_file_mention_from_workspace() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(src.join("lib.rs"), "pub fn demo() {}").expect("write file");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "check @src/l".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Tab, KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "check @src/lib.rs ");
        assert_eq!(app.cursor_pos, app.input_text.chars().count());
        assert!(app.status_message.contains("@src/lib.rs"));
    }

    #[test]
    fn file_mention_menu_uses_keyboard_selection() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(src.join("alpha.rs"), "pub fn alpha() {}").expect("write alpha");
        std::fs::write(src.join("beta.rs"), "pub fn beta() {}").expect("write beta");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "check @src/".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let suggestions = app.file_mention_suggestions();
        assert_eq!(suggestions, vec!["src/alpha.rs", "src/beta.rs"]);

        app.handle_key(key(KeyCode::Down, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "check @src/beta.rs ");
        assert_eq!(app.cursor_pos, app.input_text.chars().count());
        assert!(app.status_message.contains("@src/beta.rs"));
    }

    #[test]
    fn enter_submits_when_file_mention_is_exact() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(src.join("lib.rs"), "pub fn demo() {}").expect("write file");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_text = "review @src/lib.rs".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(matches!(
            rx.try_recv().expect("submit action"),
            TuiAction::Submit(input) if input == "review @src/lib.rs"
        ));
    }

    #[test]
    fn unquoted_file_mention_suggestions_skip_hidden_and_whitespace_paths() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        let docs = root.path().join("docs");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::create_dir_all(&docs).expect("create docs");
        std::fs::write(src.join("lib.rs"), "pub fn demo() {}").expect("write file");
        std::fs::write(root.path().join(".env"), "SECRET=1").expect("write hidden file");
        std::fs::write(docs.join("My Plan.md"), "# plan").expect("write spaced file");
        let mut app = test_app_with_root(root.path().to_path_buf());
        app.input_text = "check @".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let suggestions = app.file_mention_suggestions();

        assert!(suggestions.iter().any(|path| path == "src/lib.rs"));
        assert!(!suggestions.iter().any(|path| path == ".env"));
        assert!(!suggestions.iter().any(|path| path.contains("My Plan.md")));
    }

    #[test]
    fn quoted_file_mention_completes_paths_with_spaces() {
        let root = tempfile::tempdir().expect("tempdir");
        let docs = root.path().join("docs");
        std::fs::create_dir_all(&docs).expect("create docs");
        std::fs::write(docs.join("My Plan.md"), "# plan").expect("write spaced file");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "review @\"docs/My".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let suggestions = app.file_mention_suggestions();
        assert_eq!(suggestions, vec!["docs/My Plan.md"]);

        app.handle_key(key(KeyCode::Tab, KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "review @\"docs/My Plan.md\" ");
        assert_eq!(app.cursor_pos, app.input_text.chars().count());
        assert!(app.status_message.contains("@\"docs/My Plan.md\""));
    }

    #[test]
    fn arrow_keys_cycle_single_line_input_history() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_history = vec!["first".to_string(), "second".to_string()];
        app.input_text = "draft".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Up, KeyEventKind::Press), &tx);
        assert_eq!(app.input_text, "second");

        app.handle_key(key(KeyCode::Up, KeyEventKind::Press), &tx);
        assert_eq!(app.input_text, "first");

        app.handle_key(key(KeyCode::Down, KeyEventKind::Press), &tx);
        assert_eq!(app.input_text, "second");

        app.handle_key(key(KeyCode::Down, KeyEventKind::Press), &tx);
        assert_eq!(app.input_text, "draft");
        assert!(app.history_cursor.is_none());
    }

    #[test]
    fn ctrl_r_history_search_filters_and_inserts_without_submitting() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_history = vec![
            "run cargo test".to_string(),
            "fix tui approval".to_string(),
            "review docs".to_string(),
        ];
        app.input_text = "draft".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(
            modified_key(
                KeyCode::Char('r'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );
        assert!(app.history_search_active);
        assert!(app.input_text.is_empty());

        for ch in "tui".chars() {
            app.handle_key(key(KeyCode::Char(ch), KeyEventKind::Press), &tx);
        }
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(!app.history_search_active);
        assert_eq!(app.input_text, "fix tui approval");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn settings_panel_can_edit_and_persist_provider_default() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();

        app.open_settings_panel();
        app.settings_tab = settings_panel::SettingsTab::Model;
        app.settings_selected = 0;
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.config.provider.default, ProviderKind::Qwen);
        let local_config = std::fs::read_to_string(root.path().join(".octocode/local.toml"))
            .expect("settings should persist");
        assert!(local_config.contains("[provider]"));
        assert!(local_config.contains("default = \"qwen\""));
    }

    #[test]
    fn settings_panel_can_edit_and_persist_display_language() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();

        app.config.ui.language = "zh-CN".to_string();
        app.welcome.display_language = app.config.ui.language.clone();
        app.open_settings_panel();
        app.settings_tab = settings_panel::SettingsTab::Interface;
        app.settings_selected = 0;
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.config.ui.language, "en-US");
        assert_eq!(app.welcome.display_language, "en-US");
        let local_config = std::fs::read_to_string(root.path().join(".octocode/local.toml"))
            .expect("settings should persist");
        assert!(local_config.contains("[ui]"));
        assert!(local_config.contains("language = \"en-US\""));
    }

    #[test]
    fn settings_panel_can_edit_and_persist_safety_knobs() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();

        app.open_settings_panel();
        app.settings_tab = settings_panel::SettingsTab::Safety;
        app.settings_selected = 1;
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);
        app.settings_selected = 7;
        app.handle_key(key(KeyCode::Right, KeyEventKind::Press), &tx);

        assert!(!app.config.policy.auto_approve_safe_read);
        assert_eq!(app.config.policy.command_timeout_seconds, 300);
        let local_config = std::fs::read_to_string(root.path().join(".octocode/local.toml"))
            .expect("settings should persist");
        assert!(local_config.contains("auto_approve_safe_read = false"));
        assert!(local_config.contains("command_timeout_seconds = 300"));
    }

    #[test]
    fn settings_panel_can_edit_and_persist_interface_runtime_knobs() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();

        app.open_settings_panel();
        app.settings_tab = settings_panel::SettingsTab::Interface;
        app.settings_selected = 2;
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);
        app.settings_selected = 3;
        app.handle_key(key(KeyCode::Right, KeyEventKind::Press), &tx);

        assert_eq!(app.motion_level, motion::MotionLevel::Off);
        assert_eq!(app.config.ui.motion, "off");
        assert_eq!(app.renderer_mode, RendererMode::Fullscreen);
        assert_eq!(app.config.ui.renderer, "fullscreen");
        let local_config = std::fs::read_to_string(root.path().join(".octocode/local.toml"))
            .expect("settings should persist");
        assert!(local_config.contains("motion = \"off\""));
        assert!(local_config.contains("renderer = \"fullscreen\""));
    }

    #[test]
    fn settings_panel_can_edit_and_persist_agent_knobs() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();

        app.open_settings_panel();
        app.settings_tab = settings_panel::SettingsTab::Agents;
        app.settings_selected = 0;
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);
        app.settings_selected = 5;
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);
        app.settings_selected = 9;
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(!app.config.router.enabled);
        assert_ne!(
            app.config.subagent.auto_decompose,
            storage::Config::default().subagent.auto_decompose
        );
        assert_ne!(
            app.config.telemetry.enabled,
            storage::Config::default().telemetry.enabled
        );
        let local_config = std::fs::read_to_string(root.path().join(".octocode/local.toml"))
            .expect("settings should persist");
        assert!(local_config.contains("[router]"));
        assert!(local_config.contains("enabled = false"));
        assert!(local_config.contains("auto_decompose"));
        assert!(local_config.contains("[telemetry]"));
    }

    #[tokio::test]
    async fn local_shell_mode_requests_command_approval() {
        let mut app = test_app();
        let root = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = mpsc::unbounded_channel();

        start_local_shell_command(&mut app, root.path(), "echo hi".to_string(), &tx, false);

        assert!(app.approval.is_some());
        assert!(app.is_streaming);
        assert!(app.stream_buffer.contains("$ echo hi"));
        assert_eq!(app.status_message, "Approval needed: run command");
    }

    #[test]
    fn local_shell_auto_approve_follows_full_access_mode() {
        let mut app = test_app();

        assert!(!local_shell_auto_approve(&app, false));

        app.set_interaction_mode(InteractionMode::FullAccess);

        assert!(local_shell_auto_approve(&app, false));
    }

    #[tokio::test]
    async fn local_shell_mode_auto_approve_skips_command_approval() {
        let mut app = test_app();
        let root = tempfile::tempdir().expect("tempdir");
        let (tx, mut rx) = mpsc::unbounded_channel();

        let handle =
            start_local_shell_command(&mut app, root.path(), "echo hi".to_string(), &tx, true)
                .expect("local shell task");

        assert!(app.approval.is_none());
        assert!(app.is_streaming);
        assert_eq!(app.status_message, "Running shell command...");

        handle.await.expect("local shell task joins");
        let action = rx.try_recv().expect("local tool result");
        match action {
            TuiAction::LocalToolResult {
                command,
                output,
                is_error,
            } => {
                assert_eq!(command, "echo hi");
                assert!(!is_error, "{output}");
                assert!(output.contains("hi"));
            }
            _ => panic!("unexpected action"),
        }
    }

    #[test]
    fn local_shell_mode_blocks_dangerous_commands_without_running() {
        let mut app = test_app();
        let root = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = mpsc::unbounded_channel();

        start_local_shell_command(&mut app, root.path(), "rm -rf /".to_string(), &tx, false);

        assert!(app.approval.is_none());
        assert!(!app.is_streaming);
        assert!(app.stream_buffer.contains("Blocked"));
    }

    #[test]
    fn release_enter_does_not_submit() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Release), &tx);

        assert!(rx.try_recv().is_err());
        assert_eq!(app.input_text, "hello");
    }

    #[test]
    fn ctrl_enter_does_not_silently_swallow_input() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(
            modified_key(KeyCode::Enter, KeyModifiers::CONTROL, KeyEventKind::Press),
            &tx,
        );

        assert!(rx.try_recv().is_err());
        assert_eq!(app.input_text, "hello");
        assert!(app.status_message.contains("Ctrl+Enter"));
    }

    #[test]
    fn repeat_enter_does_not_submit() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Repeat), &tx);

        assert!(rx.try_recv().is_err());
        assert_eq!(app.input_text, "hello");
    }

    #[test]
    fn local_output_clears_input_without_staying_streaming() {
        let mut app = test_app();
        app.input_text = "/help".to_string();
        app.cursor_pos = app.input_text.chars().count();
        app.is_streaming = true;
        app.stream_start = Some(std::time::Instant::now());

        app.show_local_output("Available commands:\n/help");

        assert!(app.input_text.is_empty());
        assert_eq!(app.cursor_pos, 0);
        assert!(!app.is_streaming);
        assert!(app.stream_start.is_none());
        assert!(app.stream_buffer.contains("/help"));
        assert_eq!(app.status_message, "Available commands:");
    }

    #[test]
    fn local_error_keeps_input_for_retry() {
        let mut app = test_app();
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();
        app.is_streaming = true;
        app.stream_start = Some(std::time::Instant::now());

        app.show_local_error_keep_input("No API key configured.");

        assert_eq!(app.input_text, "hello");
        assert_eq!(app.cursor_pos, 5);
        assert!(!app.is_streaming);
        assert!(app.stream_start.is_none());
        assert_eq!(app.stream_buffer, "No API key configured.");
    }

    #[test]
    fn api_key_entry_sends_save_action_with_pending_prompt() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.begin_api_key_entry(Some("hello".to_string()));
        app.input_text = "sk-test-key".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        match rx.try_recv().expect("save action") {
            TuiAction::SaveApiKey {
                api_key,
                pending_prompt,
            } => {
                assert_eq!(api_key, "sk-test-key");
                assert_eq!(pending_prompt.as_deref(), Some("hello"));
            }
            _ => panic!("expected save api key action"),
        }
        assert_eq!(app.status_message, "Saving API key...");
    }

    #[test]
    fn api_key_entry_ignores_second_enter_while_saving() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.begin_api_key_entry(Some("hello".to_string()));
        app.input_text = "sk-test-key".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(matches!(
            rx.try_recv().expect("first save action"),
            TuiAction::SaveApiKey { .. }
        ));
        assert!(rx.try_recv().is_err());
        assert_eq!(app.status_message, "Saving API key...");
    }

    #[test]
    fn api_key_entry_rejects_invalid_key_without_submitting() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.begin_api_key_entry(None);
        app.input_text = "bad".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(rx.try_recv().is_err());
        assert!(app.api_key_entry.is_some());
        assert!(app.status_message.contains("API key looks too short"));
    }

    #[test]
    fn api_key_entry_allows_exact_local_slash_command_submit() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.begin_api_key_entry(None);
        app.input_text = "/exit".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        match rx.try_recv().expect("submit action") {
            TuiAction::Submit(input) => assert_eq!(input, "/exit"),
            _ => panic!("expected submit action"),
        }
    }

    #[test]
    fn api_key_entry_completes_partial_local_slash_command() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.begin_api_key_entry(None);
        app.input_text = "/he".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.input_text, "/help ");
    }

    #[test]
    fn api_key_save_success_keeps_session_ready_when_reload_is_missing() {
        let mut app = test_app();
        let root = tempfile::tempdir().expect("tempdir");
        app.set_api_key_state(ApiKeyState::Saving);
        app.api_key_entry = Some(ApiKeyEntry {
            pending_prompt: Some("hello".to_string()),
            saving: true,
        });
        app.input_text = "sk-test-key".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.finish_api_key_save_success(root.path(), &storage::ApiKeyStoreLocation::Keyring);

        assert_eq!(app.api_key_state, ApiKeyState::Ready);
        assert_eq!(app.welcome.api_key_status, "ready");
        assert!(app.api_key_entry.is_none());
        assert!(app.input_text.is_empty());
        assert!(!app.should_block_agent_turn_for_api_key());
    }

    #[test]
    fn api_key_save_error_returns_to_entry_without_ready_state() {
        let mut app = test_app();
        let error = anyhow::anyhow!("keyring unavailable");
        app.set_api_key_state(ApiKeyState::Saving);
        app.api_key_entry = Some(ApiKeyEntry {
            pending_prompt: Some("hello".to_string()),
            saving: true,
        });

        app.finish_api_key_save_error(&error);

        assert_eq!(app.api_key_state, ApiKeyState::Error);
        assert_eq!(app.welcome.api_key_status, "missing");
        assert!(app.api_key_entry.is_some());
        assert!(!app.api_key_entry.as_ref().expect("api entry").saving);
        assert!(app.stream_buffer.contains("Could not save API key"));
    }

    #[test]
    fn api_key_loaded_from_storage_unblocks_current_session() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = test_app_with_root(root.path().to_path_buf());
        app.set_api_key_state(ApiKeyState::Missing);
        app.welcome.api_key_status = "missing";
        app.api_key_entry = Some(ApiKeyEntry::default());

        app.mark_api_key_ready_from_storage(root.path());

        assert_eq!(app.api_key_state, ApiKeyState::Ready);
        assert_eq!(app.welcome.api_key_status, "ready");
        assert!(app.api_key_entry.is_none());
        assert!(!app.should_block_agent_turn_for_api_key());
        assert!(app.status_message.contains("loaded"));
    }

    #[test]
    fn api_key_entry_uses_single_input_row() {
        let mut app = test_app();
        app.begin_api_key_entry(None);

        assert_eq!(app.input_height(), 1);
    }

    #[test]
    fn multiline_input_height_keeps_composer_room() {
        let mut app = test_app();
        app.input_text = "hello\n".to_string();
        app.cursor_pos = app.input_text.chars().count();

        assert_eq!(app.input_height(), 2);
        assert_eq!(logical_line_count(&app.input_text), 2);
    }

    #[test]
    fn reasoning_is_hidden_by_default_until_toggled() {
        let app = test_app();

        assert!(!app.show_reasoning);
    }

    #[test]
    fn uppercase_t_toggles_thinking_panel_when_input_is_empty() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(key(KeyCode::Char('T'), KeyEventKind::Press), &tx);

        assert!(app.show_reasoning);
        assert_eq!(app.status_message, "Thinking panel expanded");

        app.handle_key(key(KeyCode::Char('t'), KeyEventKind::Press), &tx);

        assert!(!app.show_reasoning);
        assert_eq!(app.status_message, "Thinking panel collapsed");
    }

    #[test]
    fn reasoning_delta_updates_private_panel_state_not_stream_text() {
        let mut app = test_app();
        app.begin_running_turn("修复 Thinking 面板");

        app.apply_agent_event(AgentEvent::ReasoningDelta(
            "inspect layout and keep status near input".to_string(),
        ));

        assert!(app.reasoning_buffer.contains("inspect layout"));
        assert!(app.current_turn_reasoning_tokens > 0);
        assert!(app.stream_buffer.is_empty());
    }

    #[test]
    fn plan_started_sets_tracker_summary() {
        let mut app = test_app();

        app.apply_agent_event(AgentEvent::PlanStarted {
            summary: "Improve TUI basics".to_string(),
            total: 3,
        });
        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 3,
            description: "Inspect input loop".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Pending,
        });

        assert_eq!(app.plan_summary.as_deref(), Some("Improve TUI basics"));
        assert_eq!(app.plan_total_steps, 3);
        assert_eq!(app.plan_steps.len(), 1);
    }

    #[test]
    fn plan_execution_suppresses_narrative_stream_deltas() {
        let mut app = test_app();
        app.begin_running_turn("测试 CLI 功能");
        app.stream_buffer = "我会先列计划。".to_string();

        app.apply_agent_event(AgentEvent::PlanStarted {
            summary: "测试 CLI".to_string(),
            total: 2,
        });
        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 2,
            description: "读取 `input_history.txt` - 了解上下文".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Running,
        });
        app.apply_agent_event(AgentEvent::ContentDelta(
            "好的，我来按计划逐步执行。\n\nStep 1: 读取 input_history.txt。".to_string(),
        ));

        assert!(app.stream_buffer.is_empty());
        assert!(app.current_turn_output_tokens > 0);
        assert!(app
            .activity_log
            .iter()
            .any(|entry| entry.contains("plan output: 好的")));
    }

    #[test]
    fn pending_plan_still_shows_non_execution_messages() {
        let mut app = test_app();

        app.apply_agent_event(AgentEvent::PlanStarted {
            summary: "测试 CLI".to_string(),
            total: 1,
        });
        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 1,
            description: "读取文件".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Pending,
        });
        app.apply_agent_event(AgentEvent::ContentDelta(
            "已选择预览模式 - 只显示计划，不执行。\n".to_string(),
        ));

        assert!(app.stream_buffer.contains("已选择预览模式"));
    }

    #[test]
    fn swarm_finished_preserves_compact_final_summary() {
        let mut app = test_app();
        app.begin_running_turn("开蜂群，审查代码");
        app.apply_agent_event(AgentEvent::SwarmStarted {
            run_id: "swarm-1".to_string(),
            summary: "蜂群任务：审查代码".to_string(),
            total: 1,
        });
        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 1,
            description: "agent reviewer · 审查风险".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Running,
        });
        app.apply_agent_event(AgentEvent::SubagentStarted {
            agent_id: "agent-1".to_string(),
            agent_type: "reviewer".to_string(),
            description: "审查风险".to_string(),
            is_background: false,
        });
        app.apply_agent_event(AgentEvent::SubagentCompleted {
            agent_id: "agent-1".to_string(),
            result: crate::agent::subagent::SubagentResult {
                success: true,
                summary: "未发现阻断风险".to_string(),
                output: "未发现阻断风险".to_string(),
                tool_calls_used: vec!["read_file".to_string()],
                files_read: vec!["src/tui/app.rs".to_string()],
                files_written: Vec::new(),
                duration_ms: 1_000,
                token_usage: 128,
                error: None,
                started_at: chrono::Utc::now(),
                completed_at: chrono::Utc::now(),
                worktree: None,
            },
        });
        app.apply_agent_event(AgentEvent::SwarmFinished {
            run_id: "swarm-1".to_string(),
            success: true,
            summary: "蜂群已完成".to_string(),
        });
        app.apply_agent_event(AgentEvent::ContentDelta("蜂群任务已完成\n".to_string()));

        let swarm = app.active_swarm.as_ref().expect("swarm summary remains");
        assert_eq!(swarm.status, "ok");
        assert_eq!(swarm.running, 0);
        assert_eq!(swarm.done, 1);
        assert!(!swarm.detail_expanded);
        assert_eq!(app.plan_steps.len(), 1);
        assert_eq!(app.plan_steps[0].status, plan_tracker::PlanStepStatus::Done);
        assert_eq!(app.subagents.len(), 1);
        assert!(app
            .activity_log
            .iter()
            .any(|entry| entry.contains("plan output: 蜂群任务已完成")));

        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(100, 24)).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");
        let rendered = buffer_to_text(terminal.backend());
        assert!(rendered.contains("任务控制台"));
        assert!(rendered.contains("1 完成"));
        assert!(!rendered.contains("Agent Team"));
    }

    #[test]
    fn swarm_failure_keeps_detail_expanded() {
        let mut app = test_app();
        app.begin_running_turn("开蜂群，审查代码");
        app.apply_agent_event(AgentEvent::SwarmStarted {
            run_id: "swarm-1".to_string(),
            summary: "蜂群任务".to_string(),
            total: 1,
        });
        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 1,
            description: "agent reviewer · 审查风险".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Failed,
        });
        app.apply_agent_event(AgentEvent::SwarmFinished {
            run_id: "swarm-1".to_string(),
            success: false,
            summary: "蜂群未完成".to_string(),
        });

        let swarm = app.active_swarm.as_ref().expect("swarm detail remains");
        assert!(swarm.detail_expanded);
        assert!(!app.plan_steps.is_empty());
    }

    #[test]
    fn swarm_counts_track_latest_task_status_by_id() {
        let mut app = test_app();
        app.apply_agent_event(AgentEvent::SwarmStarted {
            run_id: "swarm-1".to_string(),
            summary: "蜂群任务".to_string(),
            total: 2,
        });
        app.apply_agent_event(AgentEvent::SwarmTaskUpdated {
            run_id: "swarm-1".to_string(),
            task_id: "task-a".to_string(),
            role: "explorer".to_string(),
            status: "running".to_string(),
            description: "定位入口".to_string(),
        });
        app.apply_agent_event(AgentEvent::SwarmTaskUpdated {
            run_id: "swarm-1".to_string(),
            task_id: "task-a".to_string(),
            role: "explorer".to_string(),
            status: "done".to_string(),
            description: "定位入口".to_string(),
        });
        app.apply_agent_event(AgentEvent::SwarmTaskUpdated {
            run_id: "swarm-1".to_string(),
            task_id: "task-b".to_string(),
            role: "reviewer".to_string(),
            status: "running".to_string(),
            description: "审查风险".to_string(),
        });

        let swarm = app.active_swarm.as_ref().expect("active swarm");
        assert_eq!(swarm.running, 1);
        assert_eq!(swarm.done, 1);
        assert_eq!(swarm.failed, 0);
    }

    #[test]
    fn runtime_render_state_controls_swarm_agent_visibility() {
        let mut app = test_app();
        app.subagents.push(subagent_cards::SubagentCard::new(
            "agent-1",
            "reviewer",
            "审查风险",
        ));
        app.apply_agent_event(AgentEvent::SwarmStarted {
            run_id: "swarm-1".to_string(),
            summary: "蜂群任务".to_string(),
            total: 1,
        });

        let collapsed = app.runtime_render_state();
        assert!(collapsed.visible_subagents.is_empty());

        app.active_swarm
            .as_mut()
            .expect("active swarm")
            .detail_expanded = true;
        let expanded = app.runtime_render_state();
        assert_eq!(expanded.visible_subagents.len(), 1);

        app.active_swarm = None;
        let standalone = app.runtime_render_state();
        assert_eq!(standalone.visible_subagents.len(), 1);
    }

    #[test]
    fn render_option_state_is_shared_by_history_and_command_panels() {
        let mut app = test_app();
        app.input_text = "/doc".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let command_state = app.render_option_state();
        assert!(!command_state.slash_suggestions.is_empty());
        assert!(app.render_option_height(&command_state, 5, 12) > 0);
        assert!(app.render_option_height(&command_state, 5, 12) <= COMPLETION_PANEL_MAX_HEIGHT);

        app.pending_options = Some((
            "Choose".to_string(),
            (1..=10).map(|idx| format!("Option {idx}")).collect(),
        ));
        assert_eq!(app.render_option_height(&command_state, 5, 12), 12);
        app.pending_options = None;

        app.history_search_active = true;
        app.input_history = vec!["cargo test".to_string(), "/doctor".to_string()];
        app.input_text = "cargo".to_string();

        let history_state = app.render_option_state();
        assert!(history_state.slash_suggestions.is_empty());
        assert_eq!(history_state.history_options, vec!["cargo test"]);
        assert_eq!(
            app.input_pending_options(&history_state),
            Some(history_state.history_options.as_slice())
        );
    }

    #[test]
    fn active_swarm_does_not_duplicate_status_in_top_bar() {
        let mut app = test_app();
        app.begin_running_turn("开蜂群，审查代码");
        app.apply_agent_event(AgentEvent::SwarmStarted {
            run_id: "swarm-1".to_string(),
            summary: "蜂群任务".to_string(),
            total: 1,
        });
        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 1,
            description: "agent explorer · 阅读并定位 src/agent/swarm.rs".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Running,
        });

        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(100, 24)).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");

        let rendered = buffer_to_text(terminal.backend());
        let compact_rendered = rendered.replace(' ', "");
        let first_line = rendered.lines().next().unwrap_or_default();
        assert!(!first_line.contains("swarm"));
        assert!(compact_rendered.contains("审查代码"));
        assert!(rendered.contains("○ agent explorer"));
        assert!(!rendered.contains("○  1. agent explorer"));
        assert!(!rendered.contains("蜂群计划："));
        assert!(!rendered.contains("蜂群任务："));
    }

    #[test]
    fn completed_plan_appends_structured_report() {
        let mut app = test_app();
        app.stream_buffer = "Done.".to_string();
        app.plan_steps = vec![
            plan_tracker::PlanStepItem::new(
                "搭建 Python REST API 项目",
                plan_tracker::PlanStepStatus::Done,
            ),
            plan_tracker::PlanStepItem::new("运行测试", plan_tracker::PlanStepStatus::Done),
        ];
        app.plan_total_steps = 2;
        app.plan_current_step = 2;

        app.apply_agent_event(AgentEvent::TurnComplete {
            session_id: SessionId::new_v4(),
            total_tokens: 42,
        });

        assert!(app.stream_buffer.contains("任务完成"));
        assert!(app.stream_buffer.contains("完成：2/2"));
        assert!(app.stream_buffer.contains("| # | 任务 | 用时 | 结果 |"));
        assert!(app.stream_buffer.contains("completed"));
        assert!(app.stream_buffer.contains("变更文件"));
        assert_eq!(app.plan_steps.len(), 2);
        assert_eq!(app.plan_total_steps, 2);
    }

    #[test]
    fn plan_step_duration_is_captured_when_completed() {
        let mut app = test_app();

        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 1,
            description: "Inspect files".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Running,
        });
        app.plan_steps[0].started_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(2_100));
        app.apply_agent_event(AgentEvent::PlanStepUpdate {
            index: 0,
            total: 1,
            description: "Inspect files".to_string(),
            status: crate::agent::orchestrator::PlanStepStatus::Done,
        });

        assert!(app.plan_steps[0].duration_ms.unwrap_or_default() >= 2_000);
    }

    #[test]
    fn parent_plan_step_auto_completes_when_children_are_done() {
        let mut app = test_app();
        app.plan_summary = Some("并行构建演示".to_string());
        app.plan_steps = vec![
            plan_tracker::PlanStepItem::new("并行构建演示", plan_tracker::PlanStepStatus::Pending),
            plan_tracker::PlanStepItem::new("下载前端依赖", plan_tracker::PlanStepStatus::Done),
            plan_tracker::PlanStepItem::new("下载后端依赖", plan_tracker::PlanStepStatus::Done),
            plan_tracker::PlanStepItem::new("拉取 Docker 镜像", plan_tracker::PlanStepStatus::Done),
        ];

        app.auto_complete_parent_plan_step();

        assert_eq!(app.plan_steps[0].status, plan_tracker::PlanStepStatus::Done);
    }

    #[test]
    fn normal_single_pending_step_is_not_auto_completed() {
        let mut app = test_app();
        app.plan_steps = vec![
            plan_tracker::PlanStepItem::new("Inspect files", plan_tracker::PlanStepStatus::Done),
            plan_tracker::PlanStepItem::new("Run tests", plan_tracker::PlanStepStatus::Pending),
        ];

        app.auto_complete_parent_plan_step();

        assert_eq!(
            app.plan_steps[1].status,
            plan_tracker::PlanStepStatus::Pending
        );
    }

    #[test]
    fn turn_complete_appends_brewed_duration_line() {
        let mut app = test_app();
        app.stream_buffer = "Finished the task.".to_string();
        app.stream_start =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(62_000));
        app.is_streaming = true;

        app.apply_agent_event(AgentEvent::TurnComplete {
            session_id: SessionId::new_v4(),
            total_tokens: 42,
        });

        assert!(app.stream_buffer.contains("* Brewed for 1m 2s"));
    }

    #[test]
    fn ctrl_d_is_exit_key_and_ctrl_c_is_interrupt_only() {
        let ctrl_d = modified_key(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );
        let ctrl_c = modified_key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );

        assert!(is_exit_key(ctrl_d));
        assert!(!is_interrupt_key(ctrl_d));
        assert!(!is_exit_key(ctrl_c));
        assert!(is_interrupt_key(ctrl_c));
    }

    #[test]
    fn active_screen_tracks_welcome_focus_and_modal_precedence() {
        let mut app = test_app();

        assert_eq!(app.active_screen(), screens::ActiveScreen::Welcome);

        app.stream_buffer = "previous answer".to_string();
        assert_eq!(app.active_screen(), screens::ActiveScreen::Workbench);

        app.diff_focused = true;
        assert_eq!(app.active_screen(), screens::ActiveScreen::DiffFocus);

        app.diff_focused = false;
        app.file_tree_focused = true;
        assert_eq!(app.active_screen(), screens::ActiveScreen::FileTree);

        app.settings_open = true;
        assert_eq!(app.active_screen(), screens::ActiveScreen::Settings);
    }

    #[test]
    fn project_keybindings_open_settings_at_runtime() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create .octocode");
        std::fs::write(
            config_dir.join("keybindings.toml"),
            r#"[keybindings]
open_settings = "ctrl+o"
exit = "ctrl+q"
"#,
        )
        .expect("write keybindings");

        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();
        let open_settings = modified_key(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );

        assert_eq!(
            app.key_action(open_settings),
            Some(keybindings::KeyAction::OpenSettings)
        );

        app.handle_key(open_settings, &tx);

        assert!(app.settings_open);
        assert_eq!(app.active_screen(), screens::ActiveScreen::Settings);
    }

    #[test]
    fn custom_exit_key_keeps_double_press_confirmation() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create .octocode");
        std::fs::write(
            config_dir.join("keybindings.toml"),
            r#"[keybindings]
exit = "ctrl+q"
"#,
        )
        .expect("write keybindings");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();
        let ctrl_q = modified_key(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );
        let ctrl_d = modified_key(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );

        assert!(app.key_is_exit(ctrl_q));
        assert!(!app.key_is_exit(ctrl_d));

        app.handle_key(ctrl_q, &tx);

        assert!(app.running);
        assert_eq!(app.status_message, "再次按 Ctrl+Q 退出");

        app.handle_key(ctrl_q, &tx);

        assert!(!app.running);
        assert_eq!(app.status_message, "正在退出");
    }

    #[test]
    fn ctrl_d_routed_to_handle_key_requires_second_press() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.input_text = "draft".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(
            modified_key(
                KeyCode::Char('d'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );

        assert!(app.running);
        assert_eq!(app.input_text, "draft");
        assert_eq!(app.cursor_pos, 5);
        assert_eq!(app.status_message, "再次按 Ctrl+D 退出");

        app.handle_key(
            modified_key(
                KeyCode::Char('d'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );

        assert!(!app.running);
        assert_eq!(app.status_message, "正在退出");
    }

    #[test]
    fn ctrl_d_exit_confirmation_resets_after_other_input() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(
            modified_key(
                KeyCode::Char('d'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );
        assert!(app.exit_confirm_pending);

        app.handle_key(key(KeyCode::Char('x'), KeyEventKind::Press), &tx);
        assert!(!app.exit_confirm_pending);

        app.handle_key(
            modified_key(
                KeyCode::Char('d'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );
        assert!(app.running);
        assert_eq!(app.status_message, "再次按 Ctrl+D 退出");
    }

    #[test]
    fn render_keeps_normal_footer_without_exit_prompt() {
        let app = test_app();
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(90, 24)).expect("terminal");

        terminal.draw(|f| app.render(f)).expect("draw");

        let rendered = buffer_to_text(terminal.backend());
        assert!(!rendered.contains("Press Ctrl-C again to exit"));
    }

    #[test]
    fn incomplete_plan_does_not_append_report() {
        let mut app = test_app();
        app.stream_buffer = "Working.".to_string();
        app.plan_steps = vec![plan_tracker::PlanStepItem::new(
            "Run checks",
            plan_tracker::PlanStepStatus::Running,
        )];
        app.plan_total_steps = 1;

        app.apply_agent_event(AgentEvent::TurnComplete {
            session_id: SessionId::new_v4(),
            total_tokens: 42,
        });

        assert!(!app.stream_buffer.contains("Task complete"));
    }

    #[test]
    fn subagent_delta_updates_matching_card() {
        let mut app = test_app();
        app.apply_agent_event(AgentEvent::SubagentStarted {
            agent_id: "agent-1".to_string(),
            agent_type: "worker".to_string(),
            description: "Improve TUI".to_string(),
            is_background: false,
        });

        app.apply_agent_event(AgentEvent::SubagentDelta {
            agent_id: "agent-1".to_string(),
            content: "checking input loop".to_string(),
        });

        assert_eq!(
            app.subagents[0].last_update.as_deref(),
            Some("checking input loop")
        );
        assert!(app.status_message.contains("Subagent update"));
    }

    #[test]
    fn diff_focus_expands_selected_diff_and_keeps_status_ui_only() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut app = test_app();
        app.config.ui.language = "en-US".to_string();
        app.welcome.display_language = app.config.ui.language.clone();
        app.stream_buffer = "Reviewing diffs".to_string();
        app.file_diffs = vec![
            diff_viewer::FileDiffItem::new(
                "src/a.rs",
                "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
                "+1 -1",
            ),
            diff_viewer::FileDiffItem::new(
                "src/b.rs",
                "diff --git a/src/b.rs b/src/b.rs\n--- a/src/b.rs\n+++ b/src/b.rs\n@@ -1 +1 @@\n-left\n+right\n",
                "+1 -1",
            ),
        ];
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(key(KeyCode::Char('d'), KeyEventKind::Press), &tx);

        assert!(app.diff_focused);
        assert_eq!(app.selected_diff, Some(0));
        assert_eq!(app.diff_scroll, 0);
        assert!(app.status_message.contains("Diff focus"));

        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(110, 30)).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");
        let snapshot = buffer_to_text(terminal.backend());
        assert!(snapshot.contains("Diff focus"));
        assert!(snapshot.contains("src/a.rs"));
        assert!(snapshot.contains("@@ -1 +1 @@"));
        assert!(snapshot.contains("-old"));
        assert!(snapshot.contains("+new"));

        app.handle_key(key(KeyCode::PageDown, KeyEventKind::Press), &tx);
        assert_eq!(app.diff_scroll, PAGE_SCROLL_LINES);

        app.handle_key(key(KeyCode::PageUp, KeyEventKind::Press), &tx);
        assert_eq!(app.diff_scroll, 0);

        app.handle_key(key(KeyCode::PageDown, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Down, KeyEventKind::Press), &tx);
        assert_eq!(app.selected_diff, Some(1));
        assert_eq!(app.diff_scroll, 0);

        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Press), &tx);
        assert_eq!(app.file_diffs[1].status, diff_viewer::DiffStatus::Accepted);
        assert_eq!(app.file_diffs[1].path, "src/b.rs");
        assert!(app.status_message.contains("not applied"));

        app.handle_key(key(KeyCode::Char('r'), KeyEventKind::Press), &tx);
        assert_eq!(app.file_diffs[1].status, diff_viewer::DiffStatus::Rejected);
        assert_eq!(app.file_diffs[0].status, diff_viewer::DiffStatus::Pending);
        assert!(app.status_message.contains("not applied"));

        app.handle_key(key(KeyCode::Up, KeyEventKind::Press), &tx);
        assert_eq!(app.selected_diff, Some(0));

        app.handle_key(key(KeyCode::Char('d'), KeyEventKind::Press), &tx);
        assert!(!app.diff_focused);
        assert_eq!(app.diff_scroll, 0);
    }

    #[test]
    fn diff_focus_without_diffs_reports_empty_state() {
        let mut app = test_app();
        app.config.ui.language = "en-US".to_string();
        app.welcome.display_language = app.config.ui.language.clone();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(key(KeyCode::Char('d'), KeyEventKind::Press), &tx);

        assert!(!app.diff_focused);
        assert_eq!(app.selected_diff, None);
        assert_eq!(app.diff_scroll, 0);
        assert_eq!(app.status_message, "No diffs to review");
    }

    #[test]
    fn preview_snapshot_missing_api_key_shows_full_onboarding() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/octocode"),
            true,
            120,
            28,
            PreviewSnapshotScenario::Welcome,
            theme::ThemeMode::Light,
            0,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("需要配置 API"));
        assert!(snapshot.contains("粘贴 API key"));
        assert!(!snapshot.contains("粘贴 API key..."));
        assert!(snapshot.contains("DeepSeek V4 Flash (auto)"));
        assert!(snapshot.contains("octo"));
        assert!(!snapshot.contains("octocode ·"));
        assert!(!snapshot.contains("Type below, or press 1-3"));
    }

    #[test]
    fn preview_snapshot_ready_shows_direct_input_hint() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/octocode"),
            false,
            120,
            28,
            PreviewSnapshotScenario::Welcome,
            theme::ThemeMode::Light,
            0,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("今天想从哪开始？"));
        assert!(snapshot.contains("下面随手写一句"));
        assert!(!snapshot.contains("按 1-3"));
        assert!(snapshot.contains(">"));
        assert!(snapshot.contains("上下文") || snapshot.contains("Context"));
        assert!(snapshot.contains("deepseek"));
        assert!(snapshot.contains("octo"));
        assert!(!snapshot.contains("octocode ·"));
    }

    #[test]
    fn preview_snapshot_dark_theme_renders_welcome() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/octocode"),
            false,
            120,
            28,
            PreviewSnapshotScenario::Welcome,
            theme::ThemeMode::Dark,
            0,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("Octocode") || snapshot.contains("OCTOCODE"));
        assert!(snapshot.contains("Context") || snapshot.contains("上下文"));
    }

    #[test]
    fn preview_snapshot_workbench_shows_active_plan_agents_and_reasoning() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/octocode"),
            false,
            120,
            30,
            PreviewSnapshotScenario::Workbench,
            theme::ThemeMode::Light,
            0,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("Mission Control"));
        assert!(snapshot.contains("Agent Team"));
        assert!(snapshot.contains("Review multi-agent UI"));
        assert!(!snapshot.contains("octo "));
        assert!(!snapshot.contains("OCTOCODE"));
        assert!(!snapshot.contains("Model:"));
    }

    #[test]
    fn preview_snapshot_workbench_compacts_agents_at_80_columns() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/octocode"),
            false,
            80,
            22,
            PreviewSnapshotScenario::Workbench,
            theme::ThemeMode::Light,
            0,
        )
        .expect("render snapshot");
        let lines: Vec<&str> = snapshot.lines().collect();

        assert!(snapshot.contains("智能体 3"));
        assert!(snapshot.contains("运行 2"));
        assert!(snapshot.contains("完成 1"));
        assert!(snapshot.contains("Review multi-agent UI"));
        assert!(!lines
            .iter()
            .any(|line| matches!(line.trim(), "W0" | "W1" | "tok")));
    }

    #[test]
    fn preview_snapshot_workbench_places_thinking_above_input() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/octocode"),
            false,
            120,
            30,
            PreviewSnapshotScenario::Workbench,
            theme::ThemeMode::Light,
            0,
        )
        .expect("render snapshot");
        let lines: Vec<&str> = snapshot.lines().collect();
        let thinking_lines: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| line.contains("↓ 131 token").then_some(idx))
            .collect();
        let input_idx = lines
            .iter()
            .position(|line| line.trim_start().starts_with('>'))
            .expect("input line");

        assert_eq!(thinking_lines.len(), 1);
        let thinking_idx = thinking_lines[0];
        assert!(thinking_idx < input_idx);
        assert!(input_idx.saturating_sub(thinking_idx) <= 2);
        assert!(snapshot.contains("deepseek"));
    }

    #[test]
    fn multi_option_reply_parses_comma_separated_numbers() {
        let opts = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let picks = try_match_multi_options("1, 3", &opts).expect("matched");
        assert_eq!(picks.len(), 2);
        assert_eq!(picks[0].1, "alpha");
        assert_eq!(picks[1].1, "gamma");
        let reply = format_pending_multi_reply(&picks);
        assert!(reply.contains("alpha"));
        assert!(reply.contains("gamma"));
    }

    #[test]
    fn multi_option_reply_dedupes_repeats() {
        let opts = vec!["alpha".to_string(), "beta".to_string()];
        let picks = try_match_multi_options("1, alpha, A", &opts).expect("matched");
        assert_eq!(picks.len(), 1);
    }

    #[test]
    fn multi_option_reply_returns_none_without_comma() {
        let opts = vec!["alpha".to_string(), "beta".to_string()];
        assert!(try_match_multi_options("1", &opts).is_none());
    }

    #[test]
    fn multi_option_reply_returns_none_on_bad_token() {
        let opts = vec!["alpha".to_string(), "beta".to_string()];
        assert!(try_match_multi_options("1, zzz", &opts).is_none());
    }

    #[test]
    fn preview_snapshot_text_skips_wide_char_continuation_cells() {
        let symbols = ["4", " ", "项", " ", "任", " ", "务", " "];

        assert_eq!(line_symbols_to_text(symbols), "4 项任务");
    }

    #[test]
    fn begin_running_turn_is_the_only_place_that_sets_streaming() {
        let mut app = test_app();
        app.input_text = "hello".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.begin_running_turn("hello");

        assert!(app.input_text.is_empty());
        assert_eq!(app.cursor_pos, 0);
        assert!(app.is_streaming);
        assert!(app.stream_start.is_some());
        assert_eq!(app.status_message, "Running turn...");
    }

    #[test]
    fn preview_no_bg() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/octocode"),
            false,
            100,
            28,
            PreviewSnapshotScenario::Workbench,
            theme::ThemeMode::Light,
            0,
        )
        .expect("render snapshot");
        println!("\n=== NO BG ===\n{snapshot}\n=============\n");
    }

    #[test]
    fn test_try_match_option_number() {
        let options = vec![
            "Quick fix".into(),
            "Full refactor".into(),
            "Add tests".into(),
        ];
        assert_eq!(
            try_match_option("2", &options),
            Some((2, "Full refactor".into()))
        );
        assert_eq!(
            try_match_option("1", &options),
            Some((1, "Quick fix".into()))
        );
        assert!(try_match_option("4", &options).is_none());
    }

    #[test]
    fn test_try_match_option_letter() {
        let options = vec!["Use JWT".into(), "Use Session".into(), "Use OAuth".into()];
        assert_eq!(try_match_option("A", &options), Some((1, "Use JWT".into())));
        assert_eq!(
            try_match_option("b", &options),
            Some((2, "Use Session".into()))
        );
        assert!(try_match_option("D", &options).is_none());
    }

    #[test]
    fn test_try_match_option_text() {
        let options = vec!["Quick fix".into(), "Full refactor".into()];
        assert_eq!(
            try_match_option("Quick fix", &options),
            Some((1, "Quick fix".into()))
        );
        assert_eq!(
            try_match_option("quick fix", &options),
            Some((1, "Quick fix".into()))
        );
        assert_eq!(
            try_match_option("Full", &options),
            Some((2, "Full refactor".into()))
        );
        assert!(try_match_option("Unknown", &options).is_none());
    }

    #[test]
    fn pending_option_picker_submits_selected_option_with_enter() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.pending_options = Some((
            "Choose a task".into(),
            vec![
                "Generate code".into(),
                "Review code".into(),
                "Debug issue".into(),
            ],
        ));

        app.handle_key(key(KeyCode::Down, KeyEventKind::Press), &tx);
        assert_eq!(app.selected_option_index, 1);

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);
        assert!(app.pending_options.is_none());
        assert_eq!(app.selected_option_index, 0);
        match rx.try_recv().expect("submit action") {
            TuiAction::Submit(input) => assert_eq!(input, "Selected option 2: Review code"),
            _ => panic!("expected submit action"),
        }
    }

    #[test]
    fn pending_multi_option_picker_toggles_and_submits_checked_options() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.pending_options = Some((
            "Choose tasks".into(),
            vec![
                "Generate code".into(),
                "Review code".into(),
                "Debug issue".into(),
            ],
        ));
        app.pending_question_rich = Some(PendingQuestionRich {
            title: "Choose tasks".into(),
            question: "Pick one or more".into(),
            options: vec![
                PendingQuestionOption {
                    label: "Generate code".into(),
                    description: String::new(),
                    preview: None,
                },
                PendingQuestionOption {
                    label: "Review code".into(),
                    description: String::new(),
                    preview: None,
                },
                PendingQuestionOption {
                    label: "Debug issue".into(),
                    description: String::new(),
                    preview: None,
                },
            ],
            multi_select: true,
        });
        app.selected_multi_options = vec![false; 3];

        app.handle_key(key(KeyCode::Down, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char(' '), KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Down, KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char(' '), KeyEventKind::Press), &tx);
        assert_eq!(app.selected_multi_options, vec![false, true, true]);

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(app.pending_options.is_none());
        assert!(app.pending_question_rich.is_none());
        assert!(app.selected_multi_options.is_empty());
        match rx.try_recv().expect("submit action") {
            TuiAction::Submit(input) => {
                assert_eq!(input, "Selected options: Review code, Debug issue");
            }
            _ => panic!("expected submit action"),
        }
    }

    #[test]
    fn ask_user_event_sets_pending_options_panel() {
        let mut app = test_app();

        app.apply_agent_event(AgentEvent::UserQuestionRequested {
            title: "Library · Which one?".into(),
            options: vec!["serde - standard".into(), "miniserde - small".into()],
            summary: "2 options for Library".into(),
            descriptions: Vec::new(),
            previews: Vec::new(),
            multi_select: false,
        });

        assert_eq!(
            app.pending_options
                .as_ref()
                .map(|(_, options)| options.len()),
            Some(2)
        );
        assert_eq!(
            app.last_user_question_summary.as_deref(),
            Some("2 options for Library")
        );
        assert!(!app.is_streaming);
    }

    #[test]
    fn ask_user_multi_event_initializes_checkbox_state() {
        let mut app = test_app();

        app.apply_agent_event(AgentEvent::UserQuestionRequested {
            title: "Pick checks".into(),
            options: vec!["unit".into(), "integration".into(), "smoke".into()],
            summary: "3 checks".into(),
            descriptions: Vec::new(),
            previews: Vec::new(),
            multi_select: true,
        });

        assert_eq!(app.selected_multi_options, vec![false, false, false]);
        assert!(app
            .pending_question_rich
            .as_ref()
            .is_some_and(|rich| rich.multi_select));
    }

    #[test]
    fn compact_event_sets_stable_notice() {
        let mut app = test_app();

        app.apply_agent_event(AgentEvent::ContextCompacted {
            summary: "[Context compact summary]\nrecent user goals:\n- fix cli".into(),
            reason: "auto threshold".into(),
            before_tokens: 12_000,
            after_tokens: 2_400,
        });

        assert_eq!(app.latest_compact_reason.as_deref(), Some("auto threshold"));
        assert!(app
            .latest_compact_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("fix cli")));
        assert!(app.status_notice().is_some_and(
            |notice| notice.contains("压缩上下文") || notice.contains("Context auto-compacted")
        ));
    }

    #[test]
    fn restore_recoverable_state_loads_latest_compact_summary() {
        let mut app = test_app();
        let session_id = SessionId::new_v4();
        let events = vec![crate::storage::SessionEvent::new(
            session_id,
            None,
            crate::storage::SessionEventKind::ContextCompacted {
                before_tokens: 10_000,
                after_tokens: 1_000,
                before_messages: 40,
                after_messages: 10,
                retained_start: 30,
                retained_count: 10,
                summary: "summary after compact".into(),
                reason: "manual /compact".into(),
            },
        )];

        app.restore_recoverable_state_from_events(&events);

        assert_eq!(
            app.latest_compact_summary.as_deref(),
            Some("summary after compact")
        );
        assert_eq!(
            app.latest_compact_reason.as_deref(),
            Some("manual /compact")
        );
    }

    #[test]
    fn pending_option_digit_submits_structured_selection() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.pending_options = Some(("Choose".into(), vec!["A".into(), "B".into()]));

        app.handle_key(key(KeyCode::Char('1'), KeyEventKind::Press), &tx);

        match rx.try_recv().expect("submit action") {
            TuiAction::Submit(input) => assert_eq!(input, "Selected option 1: A"),
            _ => panic!("expected submit action"),
        }
    }

    #[test]
    fn pending_option_picker_keeps_typing_as_custom_message() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.pending_options = Some(("Choose".into(), vec!["A".into(), "B".into()]));

        app.handle_key(key(KeyCode::Char('x'), KeyEventKind::Press), &tx);
        assert_eq!(app.input_text, "x");
        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        match rx.try_recv().expect("submit action") {
            TuiAction::Submit(input) => assert_eq!(input, "x"),
            _ => panic!("expected submit action"),
        }
    }

    #[test]
    fn todo_summary_loads_from_project_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        crate::tools::todo_state::write_todo_items(
            temp.path(),
            vec![
                serde_json::json!({"content":"Implement","active_form":"Implementing","status":"in_progress"}),
                serde_json::json!({"content":"Verify","status":"pending"}),
            ],
        )
        .expect("write todos");

        let app = test_app_with_root(temp.path().to_path_buf());

        assert_eq!(app.todo_summary.total, 2);
        assert_eq!(app.todo_summary.active.as_deref(), Some("Implementing"));
        assert_eq!(app.todo_items.len(), 2);
        assert_eq!(app.todo_items[0].display_text(), "Implementing");
    }

    #[test]
    fn task_tool_execution_refreshes_todo_board() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut app = test_app_with_root(temp.path().to_path_buf());

        crate::tools::task_todo::task_create(
            temp.path(),
            crate::tools::task_todo::TaskCreateArgs {
                id: Some("ui".to_string()),
                content: "Build board".to_string(),
                active_form: Some("Building board".to_string()),
                status: Some("in_progress".to_string()),
            },
        )
        .expect("create task");

        assert!(app.todo_items.is_empty());
        app.apply_agent_event(AgentEvent::ToolExecuted {
            tool_name: "task_create".to_string(),
            success: true,
            summary: "Task created".to_string(),
        });

        assert_eq!(app.todo_summary.total, 1);
        assert_eq!(app.todo_items.len(), 1);
        assert_eq!(app.todo_items[0].id, "ui");
        assert_eq!(app.todo_items[0].display_text(), "Building board");
    }

    #[test]
    fn option_shortcuts_are_zero_based_for_picker() {
        assert_eq!(option_shortcut_index('1', 3), Some(0));
        assert_eq!(option_shortcut_index('3', 3), Some(2));
        assert_eq!(option_shortcut_index('a', 3), Some(0));
        assert_eq!(option_shortcut_index('C', 3), Some(2));
        assert_eq!(option_shortcut_index('4', 3), None);
    }
}

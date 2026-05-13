use std::{
    collections::HashMap,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crossterm::{
    cursor::{position as cursor_position, Hide, MoveTo, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as CEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    style::{
        Color as CrosstermColor, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame, Terminal, TerminalOptions, Viewport,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const CTRL_C_CONFIRM_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);
const PAGE_SCROLL_LINES: usize = 20;
const MOUSE_SCROLL_LINES: usize = 5;

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
    CacheUsage, DeepSeekModel, MessageContent, MessageVisibility, ProtocolMessage, ReasoningState,
    Role, Session, SessionId, SessionMetadata, ThinkingMode, ToolCall, ToolCallFunction,
};
use crate::policy::ApprovalDisplay;
use crate::storage;

use super::{
    approval_popup, diff_viewer, file_tree, input, layout, model_hint, plan_tracker, status_bar,
    statusline, subagent_cards, theme, transcript_view, welcome,
};

/// TUI application state.
pub struct TuiApp {
    pub input_text: String,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
    pub status_message: String,
    pub messages: Vec<ProtocolMessage>,
    pub model: DeepSeekModel,
    pub thinking_mode: ThinkingMode,
    pub cache: Option<CacheUsage>,
    pub total_tokens: u64,
    pub current_turn_tokens: u64,
    pub current_turn_input_tokens: u64,
    pub current_turn_output_tokens: u64,
    current_turn_usage_finalized: bool,
    input_token_animation_started: Option<std::time::Instant>,
    pub total_cost: f64,
    pub session_name: Option<String>,
    pub welcome: welcome::WelcomeDashboardData,
    api_key_state: ApiKeyState,
    api_key_entry: Option<ApiKeyEntry>,
    pub activity_log: Vec<String>,
    pub current_task_title: String,
    pub pending_user_message: Option<String>,
    pub queued_input: Option<String>,
    pub stream_buffer: String,
    pub is_streaming: bool,
    pub stream_start: Option<std::time::Instant>,
    pub approval: Option<(ApprovalDisplay, tokio::sync::oneshot::Sender<bool>)>,
    pub session_auto_approve: bool,
    pub interaction_mode: InteractionMode,
    pub running: bool,
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
    pub selected_option_index: usize,
    pub selected_slash_index: usize,
    pub pending_images: Vec<String>,
    pub reasoning_buffer: String,
    pub show_reasoning: bool,
    pub input_history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub draft_input: String,
    // Diff viewer interaction
    pub selected_diff: Option<usize>,
    pub diff_scroll: usize,
    pub diff_focused: bool,
    // File tree sidebar
    pub file_tree: file_tree::FileTree,
    pub show_file_tree: bool,
    pub file_tree_focused: bool,
    pub mcp_status: String,
    pub theme_mode: theme::ThemeMode,
    pub renderer_mode: RendererMode,
    ctrl_c_exit_deadline: Option<std::time::Instant>,
}

pub struct DecisionPrompt {
    pub kind: DecisionKind,
    pub title: String,
    pub options: Vec<String>,
    pub respond: tokio::sync::oneshot::Sender<usize>,
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

impl RendererMode {
    #[must_use]
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
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
    fn uses_alternate_screen(self) -> bool {
        self == Self::Fullscreen
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

impl TuiApp {
    #[must_use]
    pub fn new(
        model: DeepSeekModel,
        thinking_mode: ThinkingMode,
        session_name: Option<String>,
        project_root: PathBuf,
    ) -> Self {
        let config = storage::Config::load(Some(&project_root)).unwrap_or_default();
        let theme_mode = theme::ThemeMode::from_config(&config.ui.theme);
        let renderer_mode = RendererMode::from_config(&config.ui.renderer);
        theme::set_active_theme(theme_mode);
        let welcome = welcome::WelcomeDashboardData::load(
            &project_root,
            model.clone(),
            thinking_mode.clone(),
        );

        let api_key_state = ApiKeyState::from_welcome(welcome.api_key_status);
        let api_key_missing = !api_key_state.is_ready();

        Self {
            input_text: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            status_message: if api_key_missing {
                "Enter your DeepSeek API key to start".into()
            } else {
                "Ready".into()
            },
            messages: Vec::new(),
            model,
            thinking_mode,
            cache: None,
            total_tokens: 0,
            current_turn_tokens: 0,
            current_turn_input_tokens: 0,
            current_turn_output_tokens: 0,
            current_turn_usage_finalized: false,
            input_token_animation_started: None,
            total_cost: 0.0,
            session_name,
            welcome,
            api_key_state: if api_key_missing {
                ApiKeyState::Entering
            } else {
                api_key_state
            },
            api_key_entry: api_key_missing.then(ApiKeyEntry::default),
            activity_log: Vec::new(),
            current_task_title: String::new(),
            pending_user_message: None,
            queued_input: None,
            stream_buffer: String::new(),
            is_streaming: false,
            stream_start: None,
            approval: None,
            session_auto_approve: false,
            interaction_mode: InteractionMode::Ask,
            running: true,
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
            selected_option_index: 0,
            selected_slash_index: 0,
            pending_images: Vec::new(),
            reasoning_buffer: String::new(),
            show_reasoning: false,
            input_history: crate::storage::input_history::load_history(),
            history_cursor: None,
            draft_input: String::new(),
            selected_diff: None,
            diff_scroll: 0,
            diff_focused: false,
            file_tree: file_tree::FileTree::new(project_root.clone()),
            show_file_tree: false,
            file_tree_focused: false,
            mcp_status: String::new(),
            theme_mode,
            renderer_mode,
            ctrl_c_exit_deadline: None,
        }
    }

    /// Derive the current app mode from live state for the status bar.
    fn current_mode(&self) -> status_bar::AppMode {
        let has_running_plan =
            !self.plan_steps.is_empty() && self.plan_current_step < self.plan_total_steps;
        let has_running_agents = self
            .subagents
            .iter()
            .any(|c| c.status == subagent_cards::SubagentCardStatus::Running);

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
        self.status_message = format!("Mode: {}", mode.label());
        self.push_activity(format!("mode: {}", mode.label()));
    }

    fn cycle_interaction_mode(&mut self) {
        self.set_interaction_mode(self.interaction_mode.next());
    }

    pub fn set_theme_mode(&mut self, mode: theme::ThemeMode) {
        self.theme_mode = mode;
        theme::set_active_theme(mode);
        self.status_message = format!("Theme set to {}", mode.label());
        self.push_activity(format!("theme: {}", mode.label()));
    }

    pub fn set_renderer_mode(&mut self, mode: RendererMode) {
        self.renderer_mode = mode;
        self.status_message = format!(
            "TUI renderer set to {}; restart ds to apply terminal mode",
            mode.label()
        );
        self.push_activity(format!("renderer: {}", mode.label()));
    }

    fn handle_key(&mut self, key: KeyEvent, tx: &mpsc::UnboundedSender<TuiAction>) {
        if !is_actionable_key_event(key) {
            return;
        }

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

        // Approval mode: only accept a/s/d
        if let Some((approval, respond)) = self.approval.take() {
            match key.code {
                KeyCode::Char('a') => {
                    let _ = respond.send(true);
                }
                KeyCode::Char('s') => {
                    self.session_auto_approve = true;
                    let _ = respond.send(true);
                }
                KeyCode::Char('d') | KeyCode::Esc => {
                    let _ = respond.send(false);
                }
                _ => {
                    self.approval = Some((approval, respond));
                }
            }
            return;
        }

        if self.api_key_entry.is_some() {
            self.handle_api_key_entry_key(key, tx);
            return;
        }

        if self.handle_pending_option_key(key, tx) {
            return;
        }

        if self.handle_slash_suggestion_key(key) {
            return;
        }

        let showing_welcome = self.is_showing_welcome();

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
                            let choice = try_match_option(&input, &options);
                            if let Some((idx, text)) = choice {
                                let _ = tx.send(TuiAction::Submit(text));
                                self.status_message = format!("Selected option {}", idx);
                                return;
                            }
                            // Not a valid option selection — treat as normal message
                        }

                        if self.is_streaming && !input.starts_with('/') {
                            self.queued_input = Some(input);
                            self.clear_input_editor();
                            self.status_message =
                                "已排队；当前回答结束后自动发送。Ctrl+S 可重新注入当前输入。"
                                    .into();
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
                self.scroll_newer(PAGE_SCROLL_LINES);
            }
            KeyCode::PageUp => {
                self.scroll_older(PAGE_SCROLL_LINES);
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
                'j' if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.insert_text_at_cursor("\n");
                }
                's' if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let input = self.input_text.trim().to_string();
                    if input.is_empty() {
                        self.status_message = "No input to inject".into();
                    } else if self.is_streaming {
                        self.queued_input = Some(input);
                        self.clear_input_editor();
                        self.status_message = "已注入队列；当前任务结束后发送".into();
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
                    self.pending_options = None;
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
                't' if self.input_text.is_empty() => {
                    self.show_reasoning = !self.show_reasoning;
                    self.status_message = if self.show_reasoning {
                        "Reasoning display ON".into()
                    } else {
                        "Reasoning display OFF".into()
                    };
                }
                'd' if self.input_text.is_empty() => {
                    if !self.file_diffs.is_empty() {
                        self.diff_focused = !self.diff_focused;
                        self.file_tree_focused = false;
                        if self.diff_focused && self.selected_diff.is_none() {
                            self.selected_diff = Some(0);
                        }
                        self.status_message = if self.diff_focused {
                            "Diff focus ON — ↑↓ navigate, a=accept, r=reject, d=exit".into()
                        } else {
                            "Diff focus OFF".into()
                        };
                    }
                }
                'f' if self.input_text.is_empty() => {
                    self.show_file_tree = !self.show_file_tree;
                    self.file_tree_focused = self.show_file_tree;
                    self.diff_focused = false;
                    if self.show_file_tree {
                        self.file_tree.refresh();
                        self.status_message =
                            "File tree ON — ↑↓ navigate, Enter=read, →=expand".into();
                    } else {
                        self.status_message = "File tree OFF".into();
                    }
                }
                '1' | '2' | '3' if showing_welcome && self.input_text.is_empty() => {
                    if let Some(prompt) =
                        welcome::suggested_prompt(c.to_digit(10).unwrap() as usize)
                    {
                        self.input_text = prompt.to_string();
                        self.cursor_pos = self.input_text.chars().count();
                        self.status_message = format!("Loaded launch prompt {c}");
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
                            self.status_message =
                                format!("Accepted: {}", self.file_diffs[idx].path);
                        }
                    }
                }
                'r' if self.input_text.is_empty() && self.diff_focused => {
                    if let Some(idx) = self.selected_diff {
                        if idx < self.file_diffs.len() {
                            self.file_diffs[idx].status = diff_viewer::DiffStatus::Rejected;
                            self.status_message =
                                format!("Rejected: {}", self.file_diffs[idx].path);
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
                self.selected_option_index = 0;
                self.status_message = "Option picker dismissed".into();
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
                self.status_message = "Command suggestions dismissed".into();
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
        self.status_message = format!("Command: {}", suggestions[idx].0);
    }

    fn slash_command_suggestions(&self) -> Vec<(String, String)> {
        let trimmed = self.input_text.trim();
        if self.api_key_entry.is_some()
            || !trimmed.starts_with('/')
            || trimmed.contains(char::is_whitespace)
        {
            return Vec::new();
        }

        let registry = crate::commands::CommandRegistry::new();
        registry
            .match_prefix(trimmed)
            .into_iter()
            .take(9)
            .map(|cmd| {
                let display_name = if cmd.name.starts_with(trimmed) {
                    cmd.name
                } else {
                    cmd.aliases
                        .iter()
                        .copied()
                        .find(|alias| alias.starts_with(trimmed))
                        .unwrap_or(cmd.name)
                };
                (display_name.to_string(), cmd.description.to_string())
            })
            .collect()
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

    fn submit_selected_pending_option(&mut self, tx: &mpsc::UnboundedSender<TuiAction>) {
        let Some((_, options)) = self.pending_options.take() else {
            return;
        };
        if options.is_empty() {
            self.selected_option_index = 0;
            return;
        }
        let idx = self.selected_option_index.min(options.len() - 1);
        let selected = options[idx].clone();
        self.selected_option_index = 0;
        self.status_message = format!("Selected: {selected}");
        let _ = tx.send(TuiAction::Submit(selected));
    }

    fn recall_previous_history(&mut self) {
        if self.input_history.is_empty() {
            self.status_message = "No input history yet".into();
            return;
        }

        if self.history_cursor.is_none() {
            self.draft_input = self.input_text.clone();
            self.history_cursor = Some(self.input_history.len() - 1);
        } else if self.history_cursor.unwrap() > 0 {
            self.history_cursor = Some(self.history_cursor.unwrap() - 1);
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

        let registry = crate::commands::CommandRegistry::new();
        let matches = registry.match_prefix(trimmed);
        match matches.as_slice() {
            [cmd] => {
                self.input_text = format!("{} ", cmd.name);
                self.cursor_pos = self.input_text.chars().count();
                self.status_message = format!("{} — {}", cmd.name, cmd.description);
            }
            [] => {
                self.status_message = format!("No slash command matches {trimmed}");
            }
            many => {
                let names = many
                    .iter()
                    .map(|cmd| cmd.name)
                    .collect::<Vec<_>>()
                    .join(" ");
                self.status_message = format!("Slash commands: {names}");
            }
        }
        true
    }

    fn try_complete_file_mention(&mut self) -> bool {
        let Some((token_start, prefix)) =
            mention_prefix_at_cursor(&self.input_text, self.cursor_pos)
        else {
            return false;
        };

        let candidates = file_mention_candidates(&self.file_tree.root, &prefix, 8);
        if candidates.is_empty() {
            self.status_message = format!("No file matches @{prefix}");
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
        let suffix = if candidates.len() == 1 { " " } else { "" };
        let replacement = format!("@{completion}{suffix}");
        self.input_text
            .replace_range(token_start..byte_cursor, &replacement);
        self.cursor_pos =
            self.input_text[..token_start].chars().count() + replacement.chars().count();
        self.status_message = format!("@{completion} will attach file context");
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
                            "API key should start with `sk-`. Paste it here, then press Enter."
                                .into();
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
            return 5; // bordered panel needs room for frame + content + hint
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

    fn clear_input_editor(&mut self) {
        self.input_text.clear();
        self.cursor_pos = 0;
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
        self.stream_buffer = "DeepSeek API key required.\n\nPaste your API key into the input below, then press Enter. It will be stored in the system keyring.".into();
        self.status_message = "Enter API key first".into();
    }

    fn set_api_key_state(&mut self, state: ApiKeyState) {
        self.api_key_state = state;
        self.welcome.api_key_status = state.welcome_status();
    }

    fn refresh_welcome(&mut self, root: &Path) {
        self.welcome = welcome::WelcomeDashboardData::load(
            root,
            self.model.clone(),
            self.thinking_mode.clone(),
        );
        self.welcome.api_key_status = self.api_key_state.welcome_status();
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
        self.status_message = format!("API key save failed: {error}");
        self.stream_buffer = format!(
            "Could not save API key.\n\n{error}\n\nTry again, or run `deepseek-code login --api-key sk-...`."
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
        self.current_turn_tokens = 0;
        self.current_turn_usage_finalized = false;
        self.input_token_animation_started = None;
        self.pending_user_message = Some(input.to_string());
        self.stream_buffer.clear();
        self.pending_options = None;
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
        if let Some((_, respond)) = self.approval.take() {
            let _ = respond.send(false);
        }
        if let Some(decision) = self.options_needed.take() {
            let _ = decision.respond.send(usize::MAX);
        }
        self.pending_options = None;
        self.selected_option_index = 0;
        self.is_streaming = false;
        self.stream_start = None;
        self.reasoning_buffer.clear();
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

    fn session_snapshot(&self, root: &Path) -> Session {
        Session {
            id: SessionId::new_v4(),
            name: self.session_name.clone(),
            project_root: root.to_path_buf(),
            messages: self.messages.clone(),
            reasoning_state: ReasoningState {
                mode: self.thinking_mode.clone(),
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
                    self.approval = Some((display, respond));
                }
            }
            AgentEvent::ToolExecuted {
                tool_name,
                success,
                summary,
            } => {
                let status = if success { "ok" } else { "error" };
                self.push_activity(format!(
                    "tool {} [{}]: {}",
                    tool_name,
                    status,
                    first_line(&summary)
                ));
                self.status_message = format!("Tool {tool_name} [{status}]: {summary}");
            }
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
                    self.cache = cache;
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
                self.status_message = format!("Error: {e}");
            }
            AgentEvent::TurnComplete { total_tokens, .. } => {
                let turn_duration_ms = self
                    .stream_start
                    .map(|started| started.elapsed().as_millis() as u64);
                self.is_streaming = false;
                self.stream_start = None;
                self.reasoning_buffer.clear();
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
                if success {
                    self.active_swarm = None;
                    self.subagents.clear();
                    self.plan_steps.clear();
                    self.plan_summary = None;
                    self.plan_warnings.clear();
                    self.plan_total_steps = 0;
                    self.plan_current_step = 0;
                } else if let Some(swarm) = self.active_swarm.as_mut() {
                    swarm.status = status.to_string();
                    swarm.running = 0;
                    swarm.detail_expanded = true;
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
                respond,
            } => {
                let short_id: String = agent_id.chars().take(8).collect();
                self.push_activity(format!("subagent {short_id} needs approval: {tool_name}"));
                let policy = storage::Config::load(Some(&self.file_tree.root))
                    .map(|config| config.policy)
                    .unwrap_or_default();
                let decision = crate::policy::evaluate_tool(
                    &tool_name,
                    &arguments,
                    &self.file_tree.root,
                    &policy,
                );
                if decision.action == crate::policy::PolicyAction::Allow {
                    let _ = respond.send(true);
                    self.push_activity(format!(
                        "subagent {short_id} auto-approved safe tool: {tool_name}"
                    ));
                    return;
                }
                if decision.action == crate::policy::PolicyAction::Deny {
                    let _ = respond.send(false);
                    self.status_message = format!(
                        "Subagent tool blocked: {}",
                        truncate_for_activity(&decision.reason, 80)
                    );
                    return;
                }
                // If another approval is already pending, auto-deny the new one to avoid
                // deadlock (parallel subagents compete for the single approval slot).
                if self.approval.is_some() {
                    let _ = respond.send(false);
                } else {
                    self.approval = Some((
                        subagent_tool_approval_display(
                            &agent_id,
                            &tool_name,
                            &arguments,
                            &decision.display,
                        ),
                        respond,
                    ));
                }
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

    fn handle_ctrl_c_exit_request(&mut self, now: std::time::Instant) -> bool {
        if self.ctrl_c_exit_prompt_active_at(now) {
            return true;
        }
        self.ctrl_c_exit_deadline = Some(now + CTRL_C_CONFIRM_WINDOW);
        self.status_message = "Press Ctrl-C again to exit".into();
        false
    }

    fn dismiss_ctrl_c_exit_prompt(&mut self) {
        self.ctrl_c_exit_deadline = None;
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

    fn ctrl_c_exit_prompt_active_at(&self, now: std::time::Instant) -> bool {
        self.ctrl_c_exit_deadline
            .is_some_and(|deadline| now <= deadline)
    }

    fn render(&self, f: &mut Frame) {
        theme::set_active_theme(self.theme_mode);
        let area = f.area();
        if self.renderer_mode == RendererMode::Classic {
            self.render_classic(f, area);
            return;
        }

        render_canvas(f, area);
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
        let slash_suggestions = self.slash_command_suggestions();

        // Reserve space for options panel when orchestrator presents choices.
        let options_count = self
            .options_needed
            .as_ref()
            .map(|decision| decision.options.len())
            .or_else(|| self.pending_options.as_ref().map(|(_, opts)| opts.len()))
            .or_else(|| (!slash_suggestions.is_empty()).then_some(slash_suggestions.len()))
            .unwrap_or(0);
        let options_h: u16 = if options_count > 0 {
            (options_count + 5).min(12) as u16
        } else {
            0
        };

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
        // agent rows, so the top activity line stays blank to avoid duplicate UI.
        if !showing_welcome {
            status_bar::render_status_bar(
                f,
                status_area,
                status_bar::StatusBarProps {
                    mode: self.current_mode(),
                    thinking: &self.thinking_mode,
                    activity: None,
                },
            );
        }

        // Welcome page: full body (only when empty)
        if showing_welcome {
            welcome::render_welcome(f, full_body_area, &self.welcome);
        } else {
            // Quiet terminal style: everything scrolls together in one stream.
            let elapsed = self
                .stream_start
                .map_or(0, |s| s.elapsed().as_millis() as u64);
            let empty_subagents: &[subagent_cards::SubagentCard] = &[];
            let visible_subagents = if self.active_swarm.is_some() {
                if self
                    .active_swarm
                    .as_ref()
                    .is_some_and(|swarm| swarm.detail_expanded)
                {
                    &self.subagents
                } else {
                    empty_subagents
                }
            } else {
                &self.subagents
            };
            transcript_view::render_transcript(
                f,
                content_area,
                transcript_view::TranscriptProps {
                    messages: &self.messages,
                    pending_user_message: self.pending_user_message.as_deref(),
                    scroll_offset: self.scroll_offset,
                    plan_summary: self.plan_summary.as_deref(),
                    plan_steps: &self.plan_steps,
                    plan_current_step: self.plan_current_step,
                    plan_total_steps: self.plan_total_steps,
                    plan_warnings: &self.plan_warnings,
                    subagents: visible_subagents,
                    global_elapsed_ms: elapsed,
                    diffs: &self.file_diffs,
                    selected_diff: self.selected_diff,
                    is_streaming: self.is_streaming,
                    stream_buffer: &self.stream_buffer,
                    reasoning_buffer: &self.reasoning_buffer,
                    show_reasoning: self.show_reasoning,
                },
            );
        }

        if self.scroll_offset > 0 {
            render_jump_to_bottom_hint(f, content_area);
        }

        // Options panel (between content and activity)
        if options_h > 0 {
            let (kind, title, options) = if let Some(decision) = &self.options_needed {
                (
                    decision.kind,
                    decision.title.as_str(),
                    decision.options.as_slice(),
                )
            } else if let Some((t, opts)) = &self.pending_options {
                (DecisionKind::Clarification, t.as_str(), opts.as_slice())
            } else {
                (DecisionKind::Clarification, "", &[][..])
            };
            if !options.is_empty() {
                plan_tracker::render_options_panel(
                    f,
                    options_area,
                    kind,
                    title,
                    options,
                    self.selected_option_index,
                );
            } else if !slash_suggestions.is_empty() {
                plan_tracker::render_slash_command_panel(
                    f,
                    options_area,
                    &slash_suggestions,
                    self.selected_slash_index,
                );
            }
        }

        model_hint::render_composer_hint(
            f,
            model_hint_area,
            &self.model,
            &self.thinking_mode,
            self.is_streaming.then(|| status_bar::StatusActivity {
                title: &self.current_task_title,
                elapsed_ms: self
                    .stream_start
                    .map_or(0, |started| started.elapsed().as_millis() as u64),
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
        );

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
        let opts_for_input = self.pending_options.as_ref().map(|(_, o)| o.as_slice());
        if self.api_key_entry.is_some() {
            input::render_api_key_input(f, input_area, &self.input_text, self.cursor_pos);
        } else {
            input::render_input(
                f,
                input_area,
                &self.input_text,
                self.cursor_pos,
                opts_for_input,
            );
        }
        let (cursor_x, cursor_y) = input::terminal_cursor_position(
            input_area,
            &self.input_text,
            self.cursor_pos,
            self.api_key_entry.is_some(),
        );
        f.set_cursor_position((cursor_x, cursor_y));

        if self.ctrl_c_exit_prompt_active_at(std::time::Instant::now()) {
            self.render_ctrl_c_exit_footer(f, footer_area);
        } else {
            self.render_powerline_footer(f, footer_area);
        }

        // Approval popup (on top of everything)
        if let Some((ref approval, _)) = self.approval {
            approval_popup::render_approval_popup(f, area, approval);
        }
    }

    fn render_classic(&self, f: &mut Frame, area: Rect) {
        render_canvas(f, area);

        let input_h = self.input_height();
        let slash_suggestions = self.slash_command_suggestions();
        let options_count = self
            .options_needed
            .as_ref()
            .map(|decision| decision.options.len())
            .or_else(|| self.pending_options.as_ref().map(|(_, opts)| opts.len()))
            .or_else(|| (!slash_suggestions.is_empty()).then_some(slash_suggestions.len()))
            .unwrap_or(0);
        let options_h: u16 = if options_count > 0 {
            (options_count + 4).min(10) as u16
        } else {
            0
        };
        let show_exit_prompt = self.ctrl_c_exit_prompt_active_at(std::time::Instant::now());
        let activity_h = u16::from(self.is_streaming || show_exit_prompt);
        let approval_h = self
            .approval
            .as_ref()
            .map_or(0, |(a, _)| approval_popup::inline_height(a));
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
                Constraint::Length(approval_h),
                Constraint::Length(input_h),
            ])
            .split(inner);
        let content_area = chunks[0];
        let options_area = chunks[1];
        let activity_area = chunks[2];
        let approval_area = chunks[3];
        let input_area = chunks[4];

        let showing_welcome = self.is_showing_welcome();
        if showing_welcome {
            welcome::render_classic_welcome(f, content_area, &self.welcome);
        } else {
            let elapsed = self
                .stream_start
                .map_or(0, |s| s.elapsed().as_millis() as u64);
            let empty_subagents: &[subagent_cards::SubagentCard] = &[];
            let visible_subagents = if self.active_swarm.is_some() {
                if self
                    .active_swarm
                    .as_ref()
                    .is_some_and(|swarm| swarm.detail_expanded)
                {
                    &self.subagents
                } else {
                    empty_subagents
                }
            } else {
                &self.subagents
            };
            transcript_view::render_transcript(
                f,
                content_area,
                transcript_view::TranscriptProps {
                    messages: &self.messages,
                    pending_user_message: self.pending_user_message.as_deref(),
                    scroll_offset: self.scroll_offset,
                    plan_summary: self.plan_summary.as_deref(),
                    plan_steps: &self.plan_steps,
                    plan_current_step: self.plan_current_step,
                    plan_total_steps: self.plan_total_steps,
                    plan_warnings: &self.plan_warnings,
                    subagents: visible_subagents,
                    global_elapsed_ms: elapsed,
                    diffs: &self.file_diffs,
                    selected_diff: self.selected_diff,
                    is_streaming: self.is_streaming,
                    stream_buffer: &self.stream_buffer,
                    reasoning_buffer: &self.reasoning_buffer,
                    show_reasoning: self.show_reasoning,
                },
            );
        }

        if self.scroll_offset > 0 {
            render_jump_to_bottom_hint(f, content_area);
        }

        if options_h > 0 {
            let (kind, title, options) = if let Some(decision) = &self.options_needed {
                (
                    decision.kind,
                    decision.title.as_str(),
                    decision.options.as_slice(),
                )
            } else if let Some((t, opts)) = &self.pending_options {
                (DecisionKind::Clarification, t.as_str(), opts.as_slice())
            } else {
                (DecisionKind::Clarification, "", &[][..])
            };
            if !options.is_empty() {
                plan_tracker::render_options_panel(
                    f,
                    options_area,
                    kind,
                    title,
                    options,
                    self.selected_option_index,
                );
            } else if !slash_suggestions.is_empty() {
                plan_tracker::render_slash_command_panel(
                    f,
                    options_area,
                    &slash_suggestions,
                    self.selected_slash_index,
                );
            }
        }

        if show_exit_prompt {
            render_classic_status_text(f, activity_area, "Press Ctrl-C again to exit");
        } else if self.is_streaming {
            status_bar::render_status_bar(
                f,
                activity_area,
                status_bar::StatusBarProps {
                    mode: self.current_mode(),
                    thinking: &self.thinking_mode,
                    activity: Some(status_bar::StatusActivity {
                        title: &self.current_task_title,
                        elapsed_ms: self
                            .stream_start
                            .map_or(0, |started| started.elapsed().as_millis() as u64),
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
                },
            );
        }

        if let Some((ref approval, _)) = self.approval {
            approval_popup::render_approval_inline(f, approval_area, approval);
        }

        let opts_for_input = self.pending_options.as_ref().map(|(_, o)| o.as_slice());
        if self.api_key_entry.is_some() {
            input::render_api_key_input(f, input_area, &self.input_text, self.cursor_pos);
        } else {
            input::render_input(
                f,
                input_area,
                &self.input_text,
                self.cursor_pos,
                opts_for_input,
            );
        }

        let (cursor_x, cursor_y) = input::terminal_cursor_position(
            input_area,
            &self.input_text,
            self.cursor_pos,
            self.api_key_entry.is_some(),
        );
        f.set_cursor_position((cursor_x, cursor_y));
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
        let permissions = if self.session_auto_approve {
            "bypass permissions on"
        } else {
            "permissions ask"
        };

        statusline::render_statusline(
            f,
            area,
            statusline::StatuslineProps {
                mode: self.current_mode(),
                status,
                tokens: self.visible_context_tokens(),
                input_tokens: self.visible_input_tokens(),
                output_tokens: self.current_turn_output_tokens,
                agent_tokens: self.live_agent_tokens(),
                cost: self.total_cost,
                cache: self.cache.as_ref(),
                permissions,
            },
        );
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

    fn render_ctrl_c_exit_footer(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let p = theme::palette();
        let lines = vec![
            Line::from(Span::styled(
                "─".repeat(area.width as usize),
                Style::default().fg(p.divider).bg(p.canvas),
            )),
            Line::from(vec![
                Span::styled("  ", Style::default().bg(p.canvas)),
                Span::styled(
                    "Press Ctrl-C again to exit",
                    Style::default()
                        .fg(p.dim)
                        .bg(p.canvas)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]),
        ];
        f.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().fg(p.text).bg(p.canvas)),
            area,
        );
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
        let pending_indices: Vec<usize> = self
            .plan_steps
            .iter()
            .enumerate()
            .filter_map(|(idx, step)| {
                (step.status == plan_tracker::PlanStepStatus::Pending).then_some(idx)
            })
            .collect();
        let [pending_index] = pending_indices.as_slice() else {
            return None;
        };
        if self.plan_steps.iter().enumerate().any(|(idx, step)| {
            idx != *pending_index && step.status == plan_tracker::PlanStepStatus::Running
        }) {
            return None;
        }
        let has_completed_child = self.plan_steps.iter().enumerate().any(|(idx, step)| {
            idx != *pending_index && step.status == plan_tracker::PlanStepStatus::Done
        });
        if !has_completed_child {
            return None;
        }
        let all_children_terminal = self.plan_steps.iter().enumerate().all(|(idx, step)| {
            idx == *pending_index
                || matches!(
                    step.status,
                    plan_tracker::PlanStepStatus::Done | plan_tracker::PlanStepStatus::Failed
                )
        });
        if !all_children_terminal {
            return None;
        }
        let pending_step = &self.plan_steps[*pending_index];
        let matches_summary = self.plan_summary.as_deref().is_some_and(|summary| {
            normalized_plan_title(summary) == normalized_plan_title(&pending_step.description)
        });
        ((*pending_index == 0) || matches_summary).then_some(*pending_index)
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
    ApproveOnce,
    ApproveSession,
    Deny,
    Interrupt,
    ResumeSession,
    ShowTranscript,
}

fn render_canvas(f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let line = " ".repeat(area.width as usize);
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled(line.clone(), Style::default().bg(p.canvas))))
        .collect::<Vec<_>>();
    f.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().fg(p.text).bg(p.canvas)),
        area,
    );
}

fn render_classic_status_text(f: &mut Frame, area: Rect, text: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme::palette();
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("* ", Style::default().fg(p.warning).bg(p.canvas)),
            Span::styled(
                text.to_string(),
                Style::default()
                    .fg(p.dim)
                    .bg(p.canvas)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]))
        .style(Style::default().fg(p.text).bg(p.canvas)),
        area,
    );
}

/// Non-interactive states used by `preview-tui` for dynamic layout checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSnapshotScenario {
    Welcome,
    Workbench,
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
) -> Result<String, anyhow::Error> {
    let mut app = TuiApp::new(DeepSeekModel::Flash, ThinkingMode::Auto, None, project_root);
    app.set_theme_mode(theme_mode);
    if api_key_missing {
        app.set_api_key_state(ApiKeyState::Missing);
        app.api_key_entry = Some(ApiKeyEntry::default());
        app.status_message = "Enter your DeepSeek API key to start".into();
    } else {
        app.set_api_key_state(ApiKeyState::Ready);
        app.api_key_entry = None;
        app.status_message = "Ready".into();
    }

    if scenario == PreviewSnapshotScenario::Workbench {
        app.api_key_entry = None;
        app.set_api_key_state(ApiKeyState::Ready);
        app.is_streaming = true;
        app.show_reasoning = true;
        app.stream_start = Some(std::time::Instant::now());
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
        app.reasoning_buffer =
            "Identify entry points -> inspect tests -> make the smallest safe fix".into();
        app.status_message = "Working across plan, agents, and tools".into();
        app.current_task_title = "整理系统运行流畅度".into();
        app.total_tokens = 578;
        app.current_turn_tokens = 578;
        app.plan_summary = Some("整理系统运行流畅度".into());
        app.plan_current_step = 1;
        app.plan_total_steps = 4;
        app.plan_steps = vec![
            plan_tracker::PlanStepItem::new("检查开机自启项目", plan_tracker::PlanStepStatus::Done)
                .with_duration_ms(1_200),
            plan_tracker::PlanStepItem::new("整理运行流程", plan_tracker::PlanStepStatus::Running),
            plan_tracker::PlanStepItem::new(
                "清理多余启动项",
                plan_tracker::PlanStepStatus::Pending,
            ),
            plan_tracker::PlanStepItem::new("验证启动速度", plan_tracker::PlanStepStatus::Pending),
        ];
        let mut explorer =
            subagent_cards::SubagentCard::new("019e0c78-8006", "explorer", "Trace TUI input loop");
        explorer.status = subagent_cards::SubagentCardStatus::Done;
        explorer.summary = Some("Input loop verified; repeat events ignored".into());
        explorer.duration_ms = Some(1_250);
        explorer.files_read = 3;
        let worker =
            subagent_cards::SubagentCard::new("019e0c78-7fe3", "worker", "Polish workbench UI");
        let mut worker = worker;
        worker.apply_delta("Streaming card metadata update");
        app.subagents = vec![explorer, worker];
        app.push_activity("dynamic preview: workbench state");
    }

    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| app.render(f))?;
    Ok(buffer_to_text(terminal.backend()))
}

fn buffer_to_text(backend: &ratatui::backend::TestBackend) -> String {
    let width = backend.buffer().area.width as usize;
    backend
        .buffer()
        .content()
        .chunks(width)
        .map(|line| {
            line.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct RunningTurn {
    events: mpsc::UnboundedReceiver<AgentEvent>,
    handle: JoinHandle<(Orchestrator, Option<String>)>,
    cancel_token: Arc<AtomicBool>,
}

struct TerminalSession {
    active: bool,
    renderer: RendererMode,
}

impl TerminalSession {
    fn enter(renderer: RendererMode) -> Result<Self, anyhow::Error> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        if renderer.uses_alternate_screen() {
            execute!(
                stdout,
                EnterAlternateScreen,
                Hide,
                MoveTo(0, 0),
                Clear(ClearType::Purge),
                Clear(ClearType::All),
                MoveTo(0, 0),
                EnableMouseCapture,
                EnableBracketedPaste
            )?;
        } else {
            execute!(stdout, Hide, EnableBracketedPaste)?;
        }
        stdout.flush()?;
        Ok(Self {
            active: true,
            renderer,
        })
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
                ResetColor,
                SetForegroundColor(CrosstermColor::Reset),
                SetBackgroundColor(CrosstermColor::Reset),
                SetAttribute(crossterm::style::Attribute::Reset),
                ResetColor,
                SetForegroundColor(CrosstermColor::Reset),
                SetBackgroundColor(CrosstermColor::Reset),
                SetAttribute(crossterm::style::Attribute::Reset)
            );
            if self.renderer.uses_alternate_screen() {
                let _ = execute!(stdout, LeaveAlternateScreen);
            }
            let _ = write!(stdout, "\x1b[0m\x1b[39m\x1b[49m");
            let _ = stdout.flush();
        }
    }
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
        if let Some(area) = classic_fixed_viewport_area() {
            return Ok(Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Fixed(area),
                },
            )?);
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

fn classic_fixed_viewport_area() -> Option<Rect> {
    let (width, height) = crossterm::terminal::size().ok()?;
    let (_, cursor_y) = cursor_position().ok()?;
    classic_fixed_viewport_area_for_terminal(width, height, cursor_y)
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
        ("write_file" | "edit_file" | "apply_patch", _) => "子 agent 需要修改项目文件".to_string(),
        ("fetch_url" | "web_search", _) => "子 agent 需要访问网络".to_string(),
        _ => "子 agent 需要执行工具调用".to_string(),
    }
}

fn approval_target_summary(tool_name: &str, arguments: &str) -> (&'static str, String) {
    let value = serde_json::from_str::<serde_json::Value>(arguments).unwrap_or_default();
    let target = match tool_name {
        "run_command" => value["command"].as_str(),
        "read_file" | "list_dir" | "write_file" | "edit_file" => value["path"].as_str(),
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

fn render_jump_to_bottom_hint(f: &mut Frame, area: Rect) {
    if area.width < 28 || area.height == 0 {
        return;
    }
    let label = " Jump to bottom (Ctrl+End) ↓ ";
    let width = label.chars().count() as u16;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(1);
    let p = theme::palette();
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label,
            Style::default().fg(p.inverse_text).bg(p.text),
        )))
        .style(Style::default().bg(p.canvas)),
        Rect::new(x, y, width.min(area.width), 1),
    );
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
    yolo_mode: bool,
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

    let approval_rx = if !yolo_mode
        && matches!(
            decision.action,
            crate::policy::PolicyAction::AskOnce | crate::policy::PolicyAction::AskSession
        ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.approval = Some((decision.display, tx));
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

fn char_to_byte_idx(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(idx, _)| idx)
}

fn mention_prefix_at_cursor(text: &str, cursor_pos: usize) -> Option<(usize, String)> {
    let byte_cursor = char_to_byte_idx(text, cursor_pos);
    let before_cursor = &text[..byte_cursor];
    let token_start = before_cursor
        .rfind(char::is_whitespace)
        .map_or(0, |idx| idx + 1);
    let token = &before_cursor[token_start..];
    let prefix = token.strip_prefix('@')?;
    Some((token_start, prefix.replace('\\', "/")))
}

fn file_mention_candidates(root: &Path, prefix: &str, limit: usize) -> Vec<String> {
    let prefix = prefix.replace('\\', "/").to_lowercase();
    let mut candidates = Vec::new();
    collect_file_mention_candidates(root, root, &prefix, limit, &mut candidates);
    candidates.sort();
    candidates.truncate(limit);
    candidates
}

fn collect_file_mention_candidates(
    root: &Path,
    dir: &Path,
    prefix: &str,
    limit: usize,
    candidates: &mut Vec<String>,
) {
    if candidates.len() >= limit {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        if candidates.len() >= limit {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        if file_type.is_dir() {
            if should_skip_mention_dir(&path) {
                continue;
            }
            collect_file_mention_candidates(root, &path, prefix, limit, candidates);
        } else if rel.to_lowercase().starts_with(prefix) {
            candidates.push(rel);
        }
    }
}

fn should_skip_mention_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | ".deepseek-code" | ".cache"
            )
        })
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
    let has_cli = lower.contains("cli") || lower.contains("ds");
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

/// Run the TUI interactive session.
pub async fn run_tui(
    project_root: Option<PathBuf>,
    thinking: bool,
    model_override: Option<String>,
    session_id: Option<String>,
) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));
    let api_key = storage::get_effective_api_key(Some(&root));
    let config = storage::Config::load(Some(&root)).unwrap_or_default();
    let renderer_mode = RendererMode::from_config(&config.ui.renderer);
    let mut active_client =
        crate::deepseek::client::DeepSeekClient::new(api_key.unwrap_or_default());

    let model = match model_override.as_deref() {
        Some("pro" | "v4-pro") => DeepSeekModel::Pro,
        Some("flash" | "v4-flash") | None => DeepSeekModel::Flash,
        Some(other) => {
            crate::deepseek::migration::migrate_model_name(other).unwrap_or(DeepSeekModel::Flash)
        }
    };

    let thinking_mode = if thinking {
        ThinkingMode::On
    } else {
        ThinkingMode::Auto
    };

    // Load or create session
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let store = storage::SessionStore::new(home.join(".deepseek-code"));

    let session = if let Some(ref sid) = session_id {
        let sid = uuid::Uuid::parse_str(sid)?;
        store.load(&root, &sid).unwrap_or_else(|_| Session {
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
        })
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

    let mut orchestrator = Some(Orchestrator::new(
        active_client.clone(),
        root.clone(),
        session,
    ));

    // Set up terminal
    let _terminal_session = TerminalSession::enter(renderer_mode)?;
    let mut terminal = tui_terminal(renderer_mode)?;
    if renderer_mode.uses_alternate_screen() {
        terminal.clear()?;
    }

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<TuiAction>();
    let mut app = TuiApp::new(model, thinking_mode, session_id, root.clone());
    if let Some(ref orch) = orchestrator {
        app.mcp_status = orch.mcp_status();
    }
    let mut running_turn: Option<RunningTurn> = None;
    let mut local_task: Option<JoinHandle<()>> = None;
    let mut yolo_mode = false;

    // Main event loop
    let result: Result<(), anyhow::Error> = loop {
        if !app.running {
            break Ok(());
        }

        // Draw
        terminal
            .draw(|f| app.render(f))
            .map_err(|e| anyhow::anyhow!("TUI draw failed: {e}"))?;

        if let Some(running) = running_turn.as_mut() {
            while let Ok(ev) = running.events.try_recv() {
                app.apply_agent_event(ev);
            }
        }

        if running_turn
            .as_ref()
            .is_some_and(|running| running.handle.is_finished())
        {
            let mut running = running_turn.take().unwrap();
            while let Ok(ev) = running.events.try_recv() {
                app.apply_agent_event(ev);
            }

            match running.handle.await {
                Ok((returned_orchestrator, run_error)) => {
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
                        app.status_message = format!("Turn failed: {error}");
                    }
                    orchestrator = Some(returned_orchestrator);
                }
                Err(error) if error.is_cancelled() => {
                    app.is_streaming = false;
                    app.stream_start = None;
                    app.pending_user_message = None;
                    app.status_message = "Turn interrupted".into();
                    orchestrator = None;
                }
                Err(error) => {
                    app.is_streaming = false;
                    app.stream_start = None;
                    app.pending_user_message = None;
                    app.status_message = format!("Turn task failed: {error}");
                }
            }
        }

        if running_turn.is_none() && !app.is_streaming {
            if let Some(queued) = app.queued_input.take() {
                let _ = action_tx.send(TuiAction::Submit(queued));
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

        // Poll keyboard events (non-blocking)
        if event::poll(std::time::Duration::from_millis(50))
            .map_err(|e| anyhow::anyhow!("TUI event poll failed: {e}"))?
        {
            match event::read().map_err(|e| anyhow::anyhow!("TUI event read failed: {e}"))? {
                CEvent::Key(key) => {
                    if !is_actionable_key_event(key) {
                        continue;
                    }
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        if app.handle_ctrl_c_exit_request(std::time::Instant::now()) {
                            break Ok(());
                        }
                        continue;
                    }
                    if key.code == KeyCode::Esc
                        && (running_turn.is_some() || local_task.is_some() || app.is_streaming)
                    {
                        let _ = action_tx.send(TuiAction::Interrupt);
                        continue;
                    }
                    app.dismiss_ctrl_c_exit_prompt();
                    app.handle_key(key, &action_tx);
                }
                CEvent::Paste(text) => {
                    app.dismiss_ctrl_c_exit_prompt();
                    app.handle_paste(&text);
                }
                CEvent::Mouse(mouse) => {
                    app.dismiss_ctrl_c_exit_prompt();
                    app.handle_mouse(mouse);
                }
                _ => {}
            }
        }

        // Drain TUI actions
        while let Ok(action) = action_rx.try_recv() {
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
                TuiAction::Submit(input) => {
                    // Slash commands
                    let registry = crate::commands::CommandRegistry::new();
                    let mcp_status = app.mcp_status.clone();
                    let background_tasks = orchestrator
                        .as_ref()
                        .map(Orchestrator::background_tasks)
                        .unwrap_or_default();
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
                                if is_swarm_cancel_command(&input) {
                                    if let Some(running) = running_turn.as_ref() {
                                        running.cancel_token.store(true, Ordering::SeqCst);
                                    }
                                    app.request_swarm_cancel();
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
                                // Command forwarded to agent (e.g. /fix, /explain, /review)
                                // Fall through to normal processing
                            }
                            Err(e) => {
                                app.show_local_output(e);
                                continue;
                            }
                        }
                    }

                    match shell_command_from_input(&input) {
                        Ok(Some(command)) => {
                            if running_turn.is_some() || app.is_streaming {
                                app.status_message = "A turn is already running".into();
                                continue;
                            }
                            local_task = start_local_shell_command(
                                &mut app, &root, command, &action_tx, yolo_mode,
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

                    if running_turn.is_some() {
                        app.is_streaming = true;
                        app.stream_start = Some(std::time::Instant::now());
                        app.status_message = "A turn is already running".into();
                        continue;
                    }

                    if app.should_block_agent_turn_for_api_key() {
                        if let Some(api_key) = storage::get_effective_api_key(Some(&root)) {
                            app.mark_api_key_ready_from_storage(&root);
                            active_client =
                                crate::deepseek::client::DeepSeekClient::new(api_key.clone());
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

                        if run_error.is_none() {
                            if let Some(home) = dirs::home_dir() {
                                let store = storage::SessionStore::new(home.join(".deepseek-code"));
                                let _ = store.save(&turn_orchestrator.session);
                            }
                        }

                        (turn_orchestrator, run_error)
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
                    orchestrator = Some(Orchestrator::new(
                        active_client.clone(),
                        root.clone(),
                        app.session_snapshot(&root),
                    ));
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
    };

    let _ = terminal.show_cursor();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> TuiApp {
        test_app_with_root(PathBuf::from("D:/deepseek-code"))
    }

    fn test_app_with_root(project_root: PathBuf) -> TuiApp {
        let mut app = TuiApp::new(DeepSeekModel::Flash, ThinkingMode::Auto, None, project_root);
        app.api_key_entry = None;
        app.set_api_key_state(ApiKeyState::Ready);
        app.status_message = "Ready".into();
        app
    }

    fn key(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::empty(), kind)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, kind)
    }

    #[test]
    fn renderer_mode_defaults_to_classic_and_parses_fullscreen() {
        assert_eq!(RendererMode::from_config(""), RendererMode::Classic);
        assert_eq!(RendererMode::from_config("classic"), RendererMode::Classic);
        assert_eq!(
            RendererMode::from_config("fullscreen"),
            RendererMode::Fullscreen
        );
        assert_eq!(RendererMode::from_config("alt"), RendererMode::Fullscreen);
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
    fn release_key_events_do_not_duplicate_typing() {
        let mut app = test_app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Press), &tx);
        app.handle_key(key(KeyCode::Char('a'), KeyEventKind::Release), &tx);

        assert_eq!(app.input_text, "a");
        assert_eq!(app.cursor_pos, 1);
    }

    #[test]
    fn enter_while_streaming_queues_user_input() {
        let mut app = test_app();
        let (tx, mut rx) = mpsc::unbounded_channel();
        app.input_text = "下一步继续".to_string();
        app.cursor_pos = app.input_text.chars().count();
        app.is_streaming = true;

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert_eq!(app.queued_input.as_deref(), Some("下一步继续"));
        assert!(app.input_text.is_empty());
        assert!(rx.try_recv().is_err());
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
            summarize_task_title("修复输入框颜色和光标显示，让它更像 Claude Code"),
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
    fn app_places_terminal_cursor_in_input_area_for_ime() {
        let mut app = test_app();
        app.input_text = "测试".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let backend = ratatui::backend::TestBackend::new(115, 33);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|f| app.render(f)).expect("draw");

        terminal.backend_mut().assert_cursor_position((7, 32));
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
    fn subagent_workspace_list_dir_is_auto_approved() {
        let root = tempfile::tempdir().expect("workspace");
        let mut app = test_app_with_root(root.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::oneshot::channel();

        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-12345678".to_string(),
            tool_name: "list_dir".to_string(),
            arguments: serde_json::json!({ "path": ".", "recursive": true }).to_string(),
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

        app.apply_agent_event(AgentEvent::SubagentToolApprovalNeeded {
            agent_id: "subagent-abcdef123456".to_string(),
            tool_name: "read_file".to_string(),
            arguments: serde_json::json!({ "path": outside.path().to_string_lossy() }).to_string(),
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
        assert_eq!(app.status_message, "Command: /doctor");
    }

    #[test]
    fn slash_suggestions_show_for_root_slash() {
        let mut app = test_app();
        app.input_text = "/".to_string();
        app.cursor_pos = app.input_text.chars().count();

        let suggestions = app.slash_command_suggestions();

        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().all(|(name, _)| name.starts_with('/')));
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
        assert_eq!(app.queued_input.as_deref(), Some("hello"));
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
        app.input_text = "not-a-key".to_string();
        app.cursor_pos = app.input_text.chars().count();

        app.handle_key(key(KeyCode::Enter, KeyEventKind::Press), &tx);

        assert!(rx.try_recv().is_err());
        assert!(app.api_key_entry.is_some());
        assert!(app.status_message.contains("API key should start"));
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
    fn api_key_entry_uses_five_input_rows() {
        let mut app = test_app();
        app.begin_api_key_entry(None);

        assert_eq!(app.input_height(), 5);
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
    fn swarm_finished_clears_live_task_ui_before_final_output() {
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
        app.apply_agent_event(AgentEvent::SwarmFinished {
            run_id: "swarm-1".to_string(),
            success: true,
            summary: "蜂群已完成".to_string(),
        });
        app.apply_agent_event(AgentEvent::ContentDelta("蜂群任务已完成\n".to_string()));

        assert!(app.active_swarm.is_none());
        assert!(app.plan_steps.is_empty());
        assert!(app.subagents.is_empty());
        assert!(app.stream_buffer.contains("蜂群任务已完成"));
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
        assert!(compact_rendered.contains("1个agent"));
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
    fn ctrl_c_requires_second_press_before_exit() {
        let mut app = test_app();
        let now = std::time::Instant::now();

        assert!(!app.handle_ctrl_c_exit_request(now));
        assert!(app.ctrl_c_exit_prompt_active_at(now));
        assert_eq!(app.status_message, "Press Ctrl-C again to exit");
        assert!(app.handle_ctrl_c_exit_request(now + std::time::Duration::from_secs(1)));
    }

    #[test]
    fn ctrl_c_prompt_expires_and_can_be_dismissed() {
        let mut app = test_app();
        let now = std::time::Instant::now();

        assert!(!app.handle_ctrl_c_exit_request(now));
        assert!(!app.ctrl_c_exit_prompt_active_at(now + std::time::Duration::from_secs(3)));
        assert!(!app.handle_ctrl_c_exit_request(now + std::time::Duration::from_secs(3)));
        app.dismiss_ctrl_c_exit_prompt();
        assert!(!app.ctrl_c_exit_prompt_active_at(now + std::time::Duration::from_secs(3)));
    }

    #[test]
    fn ctrl_c_prompt_replaces_powerline_footer() {
        let mut app = test_app();
        app.ctrl_c_exit_deadline =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(90, 24)).expect("terminal");

        terminal.draw(|f| app.render(f)).expect("draw");

        let rendered = buffer_to_text(terminal.backend());
        assert!(rendered.contains("Press Ctrl-C again to exit"));
        assert!(!rendered.contains("ds-code"));
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
    fn preview_snapshot_missing_api_key_shows_full_onboarding() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/deepseek-code"),
            true,
            120,
            28,
            PreviewSnapshotScenario::Welcome,
            theme::ThemeMode::Light,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("API setup required"));
        assert!(snapshot.contains("Paste your API key"));
        assert!(snapshot.contains("paste API key"));
        assert!(snapshot.contains("DeepSeek V4 Flash (auto)"));
        assert!(!snapshot.contains("Type below, or press 1-3"));
    }

    #[test]
    fn preview_snapshot_ready_shows_starters() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/deepseek-code"),
            false,
            120,
            28,
            PreviewSnapshotScenario::Welcome,
            theme::ThemeMode::Light,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("What are we changing today?"));
        assert!(snapshot.contains("Type below, or press 1-3"));
        assert!(snapshot.contains("api:ready"));
        assert!(snapshot.contains("DeepSeek V4 Flash (auto)"));
    }

    #[test]
    fn preview_snapshot_dark_theme_renders_welcome() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/deepseek-code"),
            false,
            120,
            28,
            PreviewSnapshotScenario::Welcome,
            theme::ThemeMode::Dark,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("deepseek-code"));
        assert!(snapshot.contains("api:ready"));
    }

    #[test]
    fn preview_snapshot_workbench_shows_active_plan_agents_and_reasoning() {
        let snapshot = render_preview_snapshot(
            PathBuf::from("D:/deepseek-code"),
            false,
            120,
            30,
            PreviewSnapshotScenario::Workbench,
            theme::ThemeMode::Light,
        )
        .expect("render snapshot");

        assert!(snapshot.contains("Identify entry points"));
        assert!(snapshot.contains("agents"));
        assert!(snapshot.contains("Trace TUI input loop"));
        assert!(!snapshot.contains("ds-code"));
        assert!(!snapshot.contains("Model:"));
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
            PathBuf::from("D:/deepseek-code"),
            false,
            100,
            28,
            PreviewSnapshotScenario::Workbench,
            theme::ThemeMode::Light,
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
            TuiAction::Submit(input) => assert_eq!(input, "Review code"),
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
    fn option_shortcuts_are_zero_based_for_picker() {
        assert_eq!(option_shortcut_index('1', 3), Some(0));
        assert_eq!(option_shortcut_index('3', 3), Some(2));
        assert_eq!(option_shortcut_index('a', 3), Some(0));
        assert_eq!(option_shortcut_index('C', 3), Some(2));
        assert_eq!(option_shortcut_index('4', 3), None);
    }
}

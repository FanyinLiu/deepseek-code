use std::path::{Path, PathBuf};

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::{
    deepseek::{DeepSeekModel, ThinkingMode},
    storage::{self, SessionStore},
    tui::{ascii_art, theme},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeDashboardData {
    pub workspace_name: String,
    pub workspace_path: PathBuf,
    pub model: DeepSeekModel,
    pub thinking: ThinkingMode,
    pub api_key_status: &'static str,
    pub config_status: &'static str,
    pub cache_status: &'static str,
    pub recent_sessions: Vec<RecentSessionItem>,
    pub skills: Vec<SkillItem>,
    pub mcp_servers: Vec<McpServerItem>,
    pub agents_md: AgentsMdInfo,
    pub detected_language: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillItem {
    pub name: &'static str,
    pub description: &'static str,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerItem {
    pub name: String,
    pub status: McpServerStatus,
    pub tool_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerStatus {
    Connected,
    Failed,
    NotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsMdInfo {
    pub loaded: bool,
    pub rule_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentSessionItem {
    pub label: String,
    pub updated_at: String,
    pub message_count: usize,
    pub tool_call_count: usize,
}

fn detect_project_language(root: &Path) -> String {
    let indicators: &[(&str, &str)] = &[
        ("Cargo.toml", "Rust"),
        ("package.json", "JavaScript/Node"),
        ("go.mod", "Go"),
        ("pyproject.toml", "Python"),
        ("setup.py", "Python"),
        ("requirements.txt", "Python"),
        ("Pipfile", "Python"),
        ("Gemfile", "Ruby"),
        ("pom.xml", "Java"),
        ("build.gradle", "Java/Kotlin"),
        ("composer.json", "PHP"),
        ("CMakeLists.txt", "C/C++"),
        ("Makefile", "C/C++"),
        ("stack.yaml", "Haskell"),
        ("elm.json", "Elm"),
    ];
    for (pattern, lang) in indicators {
        let path = root.join(pattern);
        if path.exists() {
            return lang.to_string();
        }
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if let Some(ext) = entry.path().extension() {
                let ext_str = ext.to_string_lossy();
                if matches!(
                    ext_str.as_ref(),
                    "rs" | "py" | "js" | "go" | "ts" | "java" | "cpp" | "c"
                ) {
                    return "Mixed".to_string();
                }
            }
        }
    }
    "Unknown".to_string()
}

fn load_mcp_status(root: &Path) -> (Vec<McpServerItem>, bool) {
    let mut servers = Vec::new();
    let mut any_available = false;

    if let Ok(config) = crate::storage::Config::load(Some(root)) {
        if config.mcp.enabled {
            for name in config.mcp.servers.keys() {
                servers.push(McpServerItem {
                    name: name.clone(),
                    status: McpServerStatus::NotConfigured,
                    tool_count: 0,
                    error: None,
                });
                any_available = true;
            }
        }
    }

    (servers, any_available)
}

fn load_agents_md(root: &Path) -> AgentsMdInfo {
    let path = root.join("AGENTS.md");
    if !path.exists() {
        return AgentsMdInfo {
            loaded: false,
            rule_count: 0,
            summary: "No AGENTS.md found in project root.".to_string(),
        };
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let rule_count = content
                .lines()
                .filter(|l| l.starts_with("## ") || l.starts_with("- ") || l.starts_with("* "))
                .count();
            let summary = content
                .lines()
                .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .map(|l| {
                    let trimmed = l.trim();
                    if trimmed.len() > 60 {
                        format!("{}...", &trimmed[..60])
                    } else {
                        trimmed.to_string()
                    }
                })
                .unwrap_or_else(|| "Project agent preferences loaded.".to_string());
            AgentsMdInfo {
                loaded: true,
                rule_count,
                summary,
            }
        }
        Err(_) => AgentsMdInfo {
            loaded: false,
            rule_count: 0,
            summary: "Failed to read AGENTS.md.".to_string(),
        },
    }
}

impl WelcomeDashboardData {
    pub fn load(root: &Path, model: DeepSeekModel, thinking: ThinkingMode) -> Self {
        let api_key = storage::get_effective_api_key(Some(root));
        let config = storage::Config::load(Some(root));
        let cache_status = match config.as_ref() {
            Ok(config) if config.ui.show_cache_hud => "no turn yet",
            Ok(_) => "disabled",
            Err(_) => "unknown",
        };

        let recent_sessions = dirs::home_dir()
            .map(|home| SessionStore::new(home.join(".deepseek-code")).list(root))
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .take(3)
            .map(|session| {
                let id = session.id.to_string();
                RecentSessionItem {
                    label: session.name.unwrap_or_else(|| id.chars().take(8).collect()),
                    updated_at: session.updated_at.format("%m-%d %H:%M").to_string(),
                    message_count: session.message_count,
                    tool_call_count: session.tool_call_count,
                }
            })
            .collect();

        Self::from_parts(
            root,
            model,
            thinking,
            api_key.is_some(),
            config.is_ok(),
            cache_status,
            recent_sessions,
        )
    }

    fn from_parts(
        root: &Path,
        model: DeepSeekModel,
        thinking: ThinkingMode,
        has_api_key: bool,
        config_loaded: bool,
        cache_status: &'static str,
        recent_sessions: Vec<RecentSessionItem>,
    ) -> Self {
        let recent_sessions = recent_sessions.into_iter().take(3).collect();
        let detected_language = detect_project_language(root);

        let skills = vec![
            SkillItem {
                name: "read_file",
                description: "Read & explore files",
                available: true,
            },
            SkillItem {
                name: "edit_file",
                description: "Edit & patch code",
                available: true,
            },
            SkillItem {
                name: "write_file",
                description: "Create new files",
                available: true,
            },
            SkillItem {
                name: "search_code",
                description: "Search codebase",
                available: true,
            },
            SkillItem {
                name: "run_command",
                description: "Execute shell commands",
                available: true,
            },
            SkillItem {
                name: "git_workflow",
                description: "Git add, commit, diff",
                available: true,
            },
            SkillItem {
                name: "web_search",
                description: "DuckDuckGo search",
                available: true,
            },
            SkillItem {
                name: "github_pr",
                description: "GitHub PR ops",
                available: std::env::var("GITHUB_TOKEN").is_ok(),
            },
            SkillItem {
                name: "semantic_search",
                description: "TF-IDF code search",
                available: true,
            },
            SkillItem {
                name: "fetch_url",
                description: "Fetch web content",
                available: true,
            },
            SkillItem {
                name: "image_input",
                description: "Multimodal images",
                available: true,
            },
            SkillItem {
                name: "lsp",
                description: "LSP hover/definition",
                available: true,
            },
            SkillItem {
                name: "subagent",
                description: "Parallel subagents",
                available: true,
            },
            SkillItem {
                name: "mcp",
                description: "MCP external tools",
                available: false,
            },
        ];

        let (mcp_servers, _mcp_available) = load_mcp_status(root);
        let agents_md = load_agents_md(root);

        Self {
            workspace_name: root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("workspace")
                .to_string(),
            workspace_path: root.to_path_buf(),
            model,
            thinking,
            api_key_status: if has_api_key { "ready" } else { "missing" },
            config_status: if config_loaded { "loaded" } else { "fallback" },
            cache_status,
            recent_sessions,
            skills,
            mcp_servers,
            agents_md,
            detected_language,
        }
    }
}

#[must_use]
pub fn suggested_prompt(index: usize) -> Option<&'static str> {
    match index {
        1 => Some("Inspect workspace for next fix"),
        2 => Some("Find TUI entry points"),
        3 => Some("Run checks and summarize"),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  RENDER
// ═══════════════════════════════════════════════════════════════════════════════

pub fn render_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    if area.width < 54 || area.height < 14 {
        render_compact_welcome(f, area, data);
        return;
    }

    f.render_widget(Paragraph::new("").style(welcome_bg()), area);

    let inner = area.inner(Margin {
        horizontal: welcome_horizontal_margin(area.width),
        vertical: u16::from(area.height >= 22),
    });

    if inner.width < 96 || inner.height < 22 {
        render_stacked_welcome(f, inner, data);
        return;
    }

    render_split_welcome(f, inner, data);
}

pub fn render_classic_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let cwd = truncate_chars(
        &data.workspace_path.display().to_string(),
        area.width as usize,
    );

    let mut lines = vec![
        Line::from(vec![
            Span::styled("✻ ", welcome_accent()),
            Span::styled("DS-CODE", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled("  ·  ", welcome_muted()),
            Span::styled(model_label(&data.model).to_string(), welcome_muted()),
        ]),
        Line::from(""),
    ];

    if data.api_key_status != "ready" {
        lines.push(Line::from(vec![Span::styled(
            "  Paste your API key below, then press Enter.",
            welcome_text(),
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("  cwd: {cwd}"),
            welcome_muted(),
        )]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "  /help for commands  ·  press 1-3 for starters",
            welcome_muted(),
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("  cwd: {cwd}"),
            welcome_muted(),
        )]));
    }

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        area,
    );
}

fn welcome_horizontal_margin(width: u16) -> u16 {
    match width {
        0..=72 => 1,
        73..=110 => 2,
        _ => 3,
    }
}

fn render_split_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(54),
            Constraint::Length(1),
            Constraint::Percentage(46),
        ])
        .split(area);

    render_identity(f, columns[0], data);
    render_divider(f, columns[1]);
    render_actions(f, columns[2], data);
}

fn render_stacked_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let identity_height = if area.height >= 20 { 8 } else { 7 }.min(area.height);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(identity_height),
            Constraint::Length(u16::from(area.height >= 22)),
            Constraint::Min(1),
        ])
        .split(area);

    render_stacked_identity(f, chunks[0], data);
    if chunks[1].height > 0 {
        render_horizontal_divider(f, chunks[1]);
    }
    render_actions(f, chunks[2], data);
}

// ── Left: Identity ──
fn render_stacked_identity(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let mut lines = ascii_art::WELCOME_WORDMARK
        .iter()
        .map(|line| Line::from(Span::styled(*line, welcome_logo())))
        .collect::<Vec<_>>();
    lines.push(Line::from(vec![
        Span::styled("Tip: ", welcome_label().add_modifier(Modifier::BOLD)),
        Span::styled(
            "Use /init to teach DeepSeek Code this workspace",
            welcome_text().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(capability_line(data));

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_identity(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let logo_height = ascii_art::WELCOME_WORDMARK.len() as u16;
    let top_pad = if area.height >= 24 { 2 } else { 0 };
    let shortcut_height = if area.height >= 14 { 3 } else { 2 };
    let capability_height = if area.height >= 11 { 2 } else { 1 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_pad),
            Constraint::Length(logo_height.saturating_add(1)),
            Constraint::Length(2),
            Constraint::Length(shortcut_height),
            Constraint::Length(capability_height),
            Constraint::Min(0),
        ])
        .split(area);

    let lines = ascii_art::WELCOME_WORDMARK
        .iter()
        .map(|line| Line::from(Span::styled(*line, welcome_logo())))
        .collect::<Vec<_>>();

    let paragraph = Paragraph::new(Text::from(lines))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, chunks[1]);

    let prompt = Paragraph::new(Text::from(vec![Line::from(vec![
        Span::styled("Tip: ", welcome_label().add_modifier(Modifier::BOLD)),
        Span::styled(
            "Use /init to teach DeepSeek Code this workspace",
            welcome_text().add_modifier(Modifier::BOLD),
        ),
    ])]))
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });
    f.render_widget(prompt, chunks[2]);

    let shortcuts = vec![
        Line::from(vec![
            Span::styled("shift+tab", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled(" switch mode  ·  ", welcome_muted()),
            Span::styled("ctrl+n", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled(" switch model", welcome_muted()),
        ]),
        Line::from(vec![
            Span::styled("ctrl+l", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled(" autonomy  ·  ", welcome_muted()),
            Span::styled("tab", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled(" thinking", welcome_muted()),
        ]),
    ];
    f.render_widget(
        Paragraph::new(Text::from(shortcuts))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[3],
    );

    render_capability_line(f, chunks[4], data);
}

// ── Right: Actions ──
fn render_actions(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    if data.api_key_status == "missing" {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Min(1),
            ])
            .split(area);
        render_api_key_setup(f, chunks[0]);
        render_context(f, chunks[1], data);
        render_invitation_and_footer(f, chunks[2], data);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(1)])
            .split(area);
        render_context(f, chunks[0], data);
        render_invitation_and_footer(f, chunks[1], data);
    }
}

fn render_divider(f: &mut Frame, area: Rect) {
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled("|", welcome_accent())))
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(Text::from(lines)).style(welcome_bg()), area);
}

fn render_horizontal_divider(f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let line = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Text::from(vec![Line::from(Span::styled(
            line,
            welcome_accent(),
        ))]))
        .style(welcome_bg()),
        area,
    );
}

fn render_invitation_and_footer(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let (title, hint, footer) = if data.api_key_status == "missing" {
        (
            "Connect DeepSeek first",
            "Paste your API key below.",
            "enter save key · ctrl+c close",
        )
    } else {
        (
            "What are we changing today?",
            "Type below, or press 1-3 to load a starter.",
            "1-3 starters · / commands · ds features · ds agent · ds mission",
        )
    };
    let lines = vec![
        Line::from(vec![Span::styled(
            title,
            welcome_text().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(hint, welcome_muted())]),
        Line::from(""),
        Line::from(vec![Span::styled(footer, welcome_accent())]),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_api_key_setup(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(vec![Span::styled(
            "API setup required",
            welcome_accent().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("1 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("Paste your API key in the input line", welcome_text()),
        ]),
        Line::from(vec![
            Span::styled("2 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("Press Enter to save it", welcome_text()),
        ]),
        Line::from(vec![
            Span::styled("3 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("Continue with your coding task", welcome_text()),
        ]),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_context(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let agents = if data.agents_md.loaded {
        format!("{} rules", data.agents_md.rule_count)
    } else {
        "none".to_string()
    };
    let recent = data
        .recent_sessions
        .first()
        .map_or_else(|| "none yet".to_string(), |s| s.label.clone());

    let lines = vec![
        context_line(
            "workspace",
            format!("{}  ·  {}", data.workspace_name, data.detected_language),
        ),
        context_line(
            "model",
            format!(
                "{}  ·  think:{}",
                data.model,
                data.thinking.to_string().to_lowercase()
            ),
        ),
        context_line(
            "state",
            format!(
                "api:{}  ·  config:{}",
                data.api_key_status, data.config_status
            ),
        ),
        context_line(
            "memory",
            format!("AGENTS.md {}  ·  recent:{}", agents, recent),
        ),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_compact_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    f.render_widget(Paragraph::new("").style(welcome_bg()), area);

    if data.api_key_status == "missing" {
        render_compact_api_onboarding(f, area, data);
        return;
    }

    let headline = "What are we changing today?";
    let footer = vec![
        key_span("1-3"),
        action_span(" starters   "),
        key_span("ds"),
        action_span(" features/agent/mission   "),
        key_span("enter"),
        action_span(" send"),
    ];
    let mut lines = Vec::new();
    lines.extend([
        Line::from(vec![Span::styled(
            "DeepSeek Code",
            welcome_text().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(headline, welcome_muted())]),
        Line::from(""),
        Line::from(vec![
            Span::styled(&data.workspace_name, welcome_text()),
            Span::styled(" · ", welcome_muted()),
            Span::styled(data.model.to_string(), welcome_muted()),
        ]),
        Line::from(""),
        Line::from(footer),
    ]);

    let paragraph = Paragraph::new(Text::from(lines))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

fn render_compact_api_onboarding(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 0,
    });
    let lines = vec![
        Line::from(vec![
            Span::styled("DeepSeek Code", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled("  ·  ", welcome_muted()),
            Span::styled(
                "API setup required",
                welcome_accent().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![Span::styled(
            "Paste your API key in the input line below, then press Enter.",
            welcome_muted(),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("1 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("paste key", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled("   2 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("enter to save", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled("   3 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("start coding", welcome_text().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("workspace ", welcome_muted()),
            Span::styled(&data.workspace_name, welcome_text()),
            Span::styled("  ·  model ", welcome_muted()),
            Span::styled(data.model.to_string(), welcome_text()),
        ]),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        inner,
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn model_label(model: &DeepSeekModel) -> &'static str {
    match model {
        DeepSeekModel::Pro => "DeepSeek V4 Pro",
        DeepSeekModel::Flash => "DeepSeek V4 Flash",
        DeepSeekModel::LegacyChat => "DeepSeek Chat",
        DeepSeekModel::LegacyReasoner => "DeepSeek Reasoner",
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

fn context_line(label: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10} "), welcome_muted()),
        Span::styled(value.into(), welcome_text()),
    ])
}

fn key_span(key: &str) -> Span<'static> {
    Span::styled(
        format!(" {key} "),
        welcome_text().add_modifier(Modifier::BOLD),
    )
}

fn action_span(text: &str) -> Span<'static> {
    Span::styled(text.to_string(), welcome_muted())
}

fn render_capability_line(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let lines = vec![capability_line(data)];
    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Center),
        area,
    );
}

fn capability_line(data: &WelcomeDashboardData) -> Line<'static> {
    let skills_available = data.skills.iter().filter(|skill| skill.available).count();
    let mcp_count = data.mcp_servers.len();
    let agents_mark = if data.agents_md.loaded { "+" } else { "x" };
    Line::from(vec![
        Span::styled("skills ", welcome_muted()),
        Span::styled(
            format!("({skills_available}) "),
            welcome_text().add_modifier(Modifier::BOLD),
        ),
        Span::styled("+  ", welcome_ok()),
        Span::styled("MCP ", welcome_muted()),
        Span::styled(
            format!("({mcp_count}) "),
            welcome_text().add_modifier(Modifier::BOLD),
        ),
        Span::styled(if mcp_count > 0 { "+  " } else { "x  " }, welcome_warn()),
        Span::styled("AGENTS.md ", welcome_muted()),
        Span::styled(
            agents_mark,
            if data.agents_md.loaded {
                welcome_ok()
            } else {
                welcome_warn()
            },
        ),
    ])
}

fn welcome_bg() -> Style {
    let p = theme::palette();
    Style::default().fg(p.text).bg(p.canvas)
}

fn welcome_logo() -> Style {
    welcome_bg().fg(theme::palette().text)
}

fn welcome_text() -> Style {
    welcome_bg().fg(welcome_fg())
}

fn welcome_muted() -> Style {
    welcome_bg().fg(theme::palette().dim)
}

fn welcome_label() -> Style {
    welcome_bg().fg(theme::palette().text)
}

fn welcome_accent() -> Style {
    welcome_bg().fg(theme::palette().accent)
}

fn welcome_ok() -> Style {
    welcome_bg().fg(theme::palette().success)
}

fn welcome_warn() -> Style {
    welcome_bg().fg(theme::palette().danger)
}

fn welcome_fg() -> Color {
    theme::palette().text
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn load_data_with_no_sessions_keeps_recent_empty() {
        let data = WelcomeDashboardData::from_parts(
            Path::new("D:/deepseek-code"),
            DeepSeekModel::Flash,
            ThinkingMode::Auto,
            false,
            true,
            "no turn yet",
            Vec::new(),
        );
        assert!(data.recent_sessions.is_empty());
        assert_eq!(data.api_key_status, "missing");
    }

    #[test]
    fn load_data_keeps_only_three_recent_sessions() {
        let now = Utc::now();
        let sessions = (0..5)
            .map(|index| RecentSessionItem {
                label: format!("session-{index}"),
                updated_at: (now - Duration::minutes(index))
                    .format("%m-%d %H:%M")
                    .to_string(),
                message_count: index as usize,
                tool_call_count: (index * 2) as usize,
            })
            .collect();
        let data = WelcomeDashboardData::from_parts(
            Path::new("D:/deepseek-code"),
            DeepSeekModel::Pro,
            ThinkingMode::On,
            true,
            true,
            "no turn yet",
            sessions,
        );
        assert_eq!(data.recent_sessions.len(), 3);
        assert_eq!(data.recent_sessions[0].label, "session-0");
        assert_eq!(data.api_key_status, "ready");
    }

    #[test]
    fn suggested_prompts_are_fixed() {
        assert_eq!(suggested_prompt(1), Some("Inspect workspace for next fix"));
        assert_eq!(suggested_prompt(2), Some("Find TUI entry points"));
        assert_eq!(suggested_prompt(3), Some("Run checks and summarize"));
        assert_eq!(suggested_prompt(4), None);
    }

    #[test]
    fn wide_render_contains_quiet_welcome_sections() {
        let data = test_data(Vec::new(), true);
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("DeepSeek Code"));
        assert!(rendered.contains("What are we changing today?"));
        assert!(rendered.contains("starters"));
        assert!(rendered.contains("ds features"));
        assert!(rendered.contains("ds agent"));
        assert!(rendered.contains("ds mission"));
        assert!(rendered.contains("workspace"));
        assert!(rendered.contains("memory"));
        assert!(!rendered.contains("Project"));
    }

    #[test]
    fn medium_render_stacks_without_losing_welcome_actions() {
        let data = test_data(Vec::new(), true);
        let mut terminal = Terminal::new(TestBackend::new(90, 24)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("DeepSeek Code"));
        assert!(rendered.contains("What are we changing today?"));
        assert!(rendered.contains("workspace"));
        assert!(rendered.contains("starters"));
    }

    #[test]
    fn missing_api_key_render_shows_setup_instead_of_starters() {
        let data = test_data(Vec::new(), false);
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("DeepSeek Code"));
        assert!(rendered.contains("Connect DeepSeek first"));
        assert!(rendered.contains("Paste your API key"));
        assert!(rendered.contains("enter"));
        assert!(!rendered.contains("Type below, or press 1-3"));
    }

    #[test]
    fn compact_render_contains_title_and_shortcuts() {
        let data = test_data(Vec::new(), true);
        let mut terminal = Terminal::new(TestBackend::new(50, 12)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw compact welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("DeepSeek Code"));
        assert!(rendered.contains("1-3"));
        assert!(rendered.contains("enter"));
    }

    #[test]
    fn compact_missing_api_key_render_shows_setup_hint() {
        let data = test_data(Vec::new(), false);
        let mut terminal = Terminal::new(TestBackend::new(50, 12)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw compact welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("API setup required"));
        assert!(rendered.contains("enter to save"));
        assert!(!rendered.contains("starters"));
    }

    fn test_data(
        recent_sessions: Vec<RecentSessionItem>,
        has_api_key: bool,
    ) -> WelcomeDashboardData {
        WelcomeDashboardData::from_parts(
            Path::new("D:/deepseek-code"),
            DeepSeekModel::Flash,
            ThinkingMode::Auto,
            has_api_key,
            true,
            "no turn yet",
            recent_sessions,
        )
    }

    fn buffer_text(backend: &TestBackend) -> String {
        backend
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }
}

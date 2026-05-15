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
    pub mascot_lines: Option<Vec<String>>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WelcomeLoadStatus {
    has_api_key: bool,
    config_loaded: bool,
    cache_status: &'static str,
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

fn load_mcp_status(config: Option<&storage::Config>) -> (Vec<McpServerItem>, bool) {
    let mut servers = Vec::new();
    let mut any_available = false;

    if let Some(config) = config {
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

fn load_project_mascot(root: &Path) -> Option<Vec<String>> {
    let path = root.join(".deepseek-code").join("mascot.txt");
    let content = std::fs::read_to_string(path).ok()?;
    let lines = content
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!lines.is_empty()).then_some(lines)
}

impl WelcomeDashboardData {
    pub fn load(root: &Path, model: DeepSeekModel, thinking: ThinkingMode) -> Self {
        let config_result = storage::Config::load(Some(root));
        let has_api_key = storage::get_api_key(
            config_result
                .as_ref()
                .ok()
                .and_then(storage::config_api_key),
        )
        .is_some();
        let config_loaded = config_result.is_ok();
        let config = config_result.ok();

        Self::load_with_startup(
            root,
            model,
            thinking,
            config.as_ref(),
            config_loaded,
            has_api_key,
        )
    }

    pub fn load_with_startup(
        root: &Path,
        model: DeepSeekModel,
        thinking: ThinkingMode,
        config: Option<&storage::Config>,
        config_loaded: bool,
        has_api_key: bool,
    ) -> Self {
        let cache_status = match config {
            Some(config) if config.ui.show_cache_hud => "no turn yet",
            Some(_) => "disabled",
            None => "unknown",
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
            WelcomeLoadStatus {
                has_api_key,
                config_loaded,
                cache_status,
            },
            recent_sessions,
            config,
        )
    }

    fn from_parts(
        root: &Path,
        model: DeepSeekModel,
        thinking: ThinkingMode,
        status: WelcomeLoadStatus,
        recent_sessions: Vec<RecentSessionItem>,
        config: Option<&storage::Config>,
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

        let (mcp_servers, _mcp_available) = load_mcp_status(config);
        let agents_md = load_agents_md(root);
        let mascot_lines = load_project_mascot(root);

        Self {
            workspace_name: root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("workspace")
                .to_string(),
            workspace_path: root.to_path_buf(),
            model,
            thinking,
            api_key_status: if status.has_api_key {
                "ready"
            } else {
                "missing"
            },
            config_status: if status.config_loaded {
                "loaded"
            } else {
                "fallback"
            },
            cache_status: status.cache_status,
            recent_sessions,
            skills,
            mcp_servers,
            agents_md,
            detected_language,
            mascot_lines,
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
    if area.width < 70 || area.height < 18 {
        render_compact_welcome(f, area, data);
        return;
    }
    if area.width < 110 {
        render_focused_welcome(f, area, data);
        return;
    }

    f.render_widget(Paragraph::new("").style(welcome_bg()), area);

    let inner = area.inner(Margin {
        horizontal: 4,
        vertical: 1,
    });

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55),
            Constraint::Length(1),
            Constraint::Percentage(45),
        ])
        .split(inner);

    render_identity(f, columns[0], data);
    render_divider(f, columns[1]);
    render_actions(f, columns[2], data);
}

fn render_focused_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    f.render_widget(Paragraph::new("").style(welcome_bg()), area);
    let inner = area.inner(Margin {
        horizontal: 4,
        vertical: 1,
    });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Min(1),
        ])
        .split(inner);

    render_product_mark(f, chunks[0]);

    let tips = vec![
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled(
                "Ask questions, edit files, or run commands.",
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled("Be specific for the best results.", welcome_muted()),
        ]),
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled("/help for more information.", welcome_muted()),
        ]),
    ];
    f.render_widget(
        Paragraph::new(Text::from(tips))
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[1],
    );

    if data.api_key_status == "missing" {
        render_api_key_setup(f, chunks[2]);
    } else {
        render_context(f, chunks[2], data);
    }

    render_invitation_and_footer(f, chunks[3], data);
}

// ── Left: Identity ──
fn render_identity(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(7),
            Constraint::Length(4),
            Constraint::Min(1),
        ])
        .split(area);

    render_product_mark(f, chunks[1]);

    let shortcuts = vec![
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled(
                "Ask questions, edit files, or run commands.",
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled(
                "Plan, approvals, and agents appear only when needed.",
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled(
                "/help for commands, @path for files, ! for shell.",
                welcome_muted(),
            ),
        ]),
    ];
    f.render_widget(
        Paragraph::new(Text::from(shortcuts))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[2],
    );

    render_capability_line(f, chunks[3], data);
}

// ── Right: Actions ──
fn render_actions(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Min(1),
        ])
        .split(area);

    render_release_header(f, chunks[0]);
    if data.api_key_status == "missing" {
        render_api_key_setup(f, chunks[1]);
    } else {
        render_changelog(f, chunks[1]);
    }
    render_context(f, chunks[2], data);
    render_invitation_and_footer(f, chunks[3], data);
}

fn render_divider(f: &mut Frame, area: Rect) {
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled("|", welcome_accent())))
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(Text::from(lines)).style(welcome_bg()), area);
}

fn render_release_header(f: &mut Frame, area: Rect) {
    let lines = vec![Line::from(vec![
        Span::styled(
            "Ready surface ",
            welcome_accent().add_modifier(Modifier::BOLD),
        ),
        Span::styled("v0.1.0", welcome_text().add_modifier(Modifier::BOLD)),
        Span::styled("        quiet until work starts", welcome_muted()),
    ])];
    f.render_widget(Paragraph::new(Text::from(lines)).style(welcome_bg()), area);
}

fn render_changelog(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Start",
            welcome_accent().add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("1 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled(
                "New conversation",
                welcome_text().add_modifier(Modifier::BOLD),
            ),
            Span::styled(" - describe the change", welcome_muted()),
        ]),
        Line::from(vec![
            Span::styled("2 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled(
                "Inspect workspace",
                welcome_text().add_modifier(Modifier::BOLD),
            ),
            Span::styled(" - read files before editing", welcome_muted()),
        ]),
        Line::from(vec![
            Span::styled("3 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("Run checks", welcome_text().add_modifier(Modifier::BOLD)),
            Span::styled(" - verify before summary", welcome_muted()),
        ]),
    ];
    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .wrap(Wrap { trim: false }),
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
            "1-3 starters · plan/agents/approvals appear on demand",
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
            "API setup",
            welcome_accent().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("1 ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("Paste API key in the input line", welcome_text()),
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

    if area.width >= 70 && area.height >= 12 {
        render_compact_brand_welcome(f, area, data);
        return;
    }

    let headline = "What are we changing today?";
    let footer = vec![
        key_span("1-3"),
        action_span(" starters   "),
        key_span("enter"),
        action_span(" send"),
    ];
    let lines = vec![
        Line::from(vec![Span::styled(
            format!("{}  DeepSeek Code", ascii_art::WHALE_TINY),
            welcome_badge(),
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
    ];

    let paragraph = Paragraph::new(Text::from(lines))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

fn render_compact_brand_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    render_product_mark(f, chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "What are we changing today?",
            welcome_muted(),
        )]))
        .style(welcome_bg())
        .alignment(Alignment::Center),
        chunks[1],
    );

    let workspace = Line::from(vec![
        Span::styled(&data.workspace_name, welcome_text()),
        Span::styled(" · ", welcome_muted()),
        Span::styled(data.model.to_string(), welcome_muted()),
    ]);
    f.render_widget(
        Paragraph::new(workspace)
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[2],
    );

    let footer = Line::from(vec![
        key_span("1-3"),
        action_span(" starters   "),
        key_span("enter"),
        action_span(" send"),
    ]);
    f.render_widget(
        Paragraph::new(footer)
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[3],
    );
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

fn render_product_mark(f: &mut Frame, area: Rect) {
    if area.width < 48 || area.height < 6 {
        let lines = vec![
            Line::from(vec![Span::styled(ascii_art::WHALE_TINY, welcome_badge())]),
            Line::from(vec![Span::styled(
                "DeepSeek Code",
                welcome_text().add_modifier(Modifier::BOLD),
            )]),
        ];
        f.render_widget(
            Paragraph::new(Text::from(lines))
                .style(welcome_bg())
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let whale_width = ascii_art::WELCOME_WHALE
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let title_width = 30;
    let lockup_width = whale_width + 4 + title_width;
    let left_x = area.x + ((area.width as usize).saturating_sub(lockup_width) / 2) as u16;
    let title_x = left_x + whale_width as u16 + 4;
    let title_rows: [Vec<Span<'static>>; 6] = [
        vec![],
        vec![
            Span::styled("DeepSeek ", welcome_accent().add_modifier(Modifier::BOLD)),
            Span::styled("Code", welcome_brand_alt().add_modifier(Modifier::BOLD)),
        ],
        vec![Span::styled("Your AI Coding Partner", welcome_muted())],
        vec![],
        vec![Span::styled("Think Deeper. Code Smarter.", welcome_muted())],
        vec![],
    ];

    for (idx, whale) in ascii_art::WELCOME_WHALE.iter().enumerate() {
        let row_y = area.y + idx as u16;
        if row_y >= area.y + area.height {
            break;
        }

        let indent = whale.chars().take_while(|ch| *ch == ' ').count() as u16;
        let whale_body = whale.trim_start();
        if !whale_body.is_empty() {
            let whale_area = Rect::new(
                left_x + indent,
                row_y,
                area.right().saturating_sub(left_x + indent),
                1,
            );
            f.render_widget(
                Paragraph::new(whale_body.to_string())
                    .style(welcome_accent().add_modifier(Modifier::BOLD)),
                whale_area,
            );
        }

        if let Some(row) = title_rows.get(idx).filter(|row| !row.is_empty()) {
            let title_area = Rect::new(title_x, row_y, area.right().saturating_sub(title_x), 1);
            f.render_widget(
                Paragraph::new(Line::from(row.clone())).style(welcome_bg()),
                title_area,
            );
        }
    }
}

fn render_capability_line(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let skills_available = data.skills.iter().filter(|skill| skill.available).count();
    let mcp_count = data.mcp_servers.len();
    let agents_mark = if data.agents_md.loaded { "+" } else { "x" };
    let lines = vec![Line::from(vec![
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
    ])];
    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Center),
        area,
    );
}

fn welcome_bg() -> Style {
    let p = theme::palette();
    Style::default().fg(p.text).bg(p.canvas)
}

fn welcome_text() -> Style {
    welcome_bg().fg(welcome_fg())
}

fn welcome_muted() -> Style {
    welcome_bg().fg(theme::palette().dim)
}

fn welcome_accent() -> Style {
    welcome_bg().fg(theme::palette().accent)
}

fn welcome_brand_alt() -> Style {
    welcome_bg().fg(theme::palette().warning)
}

fn welcome_badge() -> Style {
    welcome_bg()
        .fg(theme::palette().info)
        .add_modifier(Modifier::BOLD)
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
            WelcomeLoadStatus {
                has_api_key: false,
                config_loaded: true,
                cache_status: "no turn yet",
            },
            Vec::new(),
            None,
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
            WelcomeLoadStatus {
                has_api_key: true,
                config_loaded: true,
                cache_status: "no turn yet",
            },
            sessions,
            None,
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
    fn project_mascot_loads_from_fixed_text_file_without_scaling() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".deepseek-code");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("mascot.txt"), "  ██  \n ████ \n").expect("write mascot");

        let data =
            WelcomeDashboardData::load(root.path(), DeepSeekModel::Flash, ThinkingMode::Auto);

        assert_eq!(
            data.mascot_lines,
            Some(vec!["  ██".to_string(), " ████".to_string()])
        );
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
        assert!(rendered.contains("workspace"));
        assert!(rendered.contains("memory"));
        assert!(!rendered.contains("Project"));
    }

    #[test]
    fn missing_api_key_render_shows_setup_instead_of_starters() {
        let data = test_data(Vec::new(), false);
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("Connect DeepSeek first"));
        assert!(rendered.contains("Paste API key"));
        assert!(rendered.contains("enter"));
        assert!(!rendered.contains("What are we changing today?"));
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
            WelcomeLoadStatus {
                has_api_key,
                config_loaded: true,
                cache_status: "no turn yet",
            },
            recent_sessions,
            None,
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

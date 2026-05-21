use std::path::{Path, PathBuf};

use crate::{
    deepseek::{DeepSeekModel, ThinkingMode},
    storage::{self, SessionStore},
    tui::theme,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
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
    pub display_language: String,
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
                    truncate_chars(trimmed, 60)
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

fn truncate_chars(value: &str, max_chars: usize) -> String {
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

impl WelcomeDashboardData {
    pub fn load(root: &Path, model: DeepSeekModel, thinking: ThinkingMode) -> Self {
        let config_result = storage::Config::load(Some(root));
        let has_api_key = config_result
            .as_ref()
            .ok()
            .and_then(|config| {
                storage::get_api_key_for_provider(
                    config.provider.default,
                    storage::config_api_key(config),
                )
            })
            .or_else(|| storage::get_api_key(None))
            .is_some();
        let config_loaded = config_result.is_ok();
        let config = config_result.ok();
        let cache_status = match config.as_ref() {
            Some(config) if config.ui.show_cache_hud => "no turn yet",
            Some(_) => "disabled",
            None => "unknown",
        };

        let recent_sessions = dirs::home_dir()
            .map(|home| SessionStore::new(home.join(".octocode")).list(root))
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

        Self::from_parts_with_config(
            root,
            model,
            thinking,
            WelcomeLoadStatus {
                has_api_key,
                config_loaded,
                cache_status,
            },
            recent_sessions,
            config.as_ref(),
        )
    }

    #[cfg(test)]
    fn from_parts(
        root: &Path,
        model: DeepSeekModel,
        thinking: ThinkingMode,
        has_api_key: bool,
        config_loaded: bool,
        cache_status: &'static str,
        recent_sessions: Vec<RecentSessionItem>,
    ) -> Self {
        Self::from_parts_with_config(
            root,
            model,
            thinking,
            WelcomeLoadStatus {
                has_api_key,
                config_loaded,
                cache_status,
            },
            recent_sessions,
            None,
        )
    }

    fn from_parts_with_config(
        root: &Path,
        model: DeepSeekModel,
        thinking: ThinkingMode,
        status: WelcomeLoadStatus,
        recent_sessions: Vec<RecentSessionItem>,
        config: Option<&storage::Config>,
    ) -> Self {
        let recent_sessions = recent_sessions.into_iter().take(3).collect();
        let detected_language = detect_project_language(root);
        let display_language = config
            .map(|config| config.ui.language.clone())
            .unwrap_or_else(|| storage::Config::default().ui.language);

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

        Self {
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
            display_language,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiLanguage {
    EnUs,
    ZhCn,
}

pub fn is_chinese_display_language(value: &str) -> bool {
    ui_language_from_config(value) == UiLanguage::ZhCn
}

fn display_language(data: &WelcomeDashboardData) -> UiLanguage {
    ui_language_from_config(&data.display_language)
}

fn ui_language_from_config(value: &str) -> UiLanguage {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "auto" | "" => auto_ui_language(),
        "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" | "chinese" => UiLanguage::ZhCn,
        "en" | "en-us" | "en-gb" | "english" => UiLanguage::EnUs,
        "ja" | "ja-jp" | "jp" | "japanese" => UiLanguage::EnUs,
        value if value.starts_with("zh") => UiLanguage::ZhCn,
        value if value.starts_with("ja") => UiLanguage::EnUs,
        _ => UiLanguage::EnUs,
    }
}

fn auto_ui_language() -> UiLanguage {
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.to_ascii_lowercase().replace('_', "-"))
        .find_map(|value| {
            if value.starts_with("zh") {
                Some(UiLanguage::ZhCn)
            } else if value.starts_with("en") || value.starts_with("ja") {
                Some(UiLanguage::EnUs)
            } else {
                None
            }
        })
        .unwrap_or(UiLanguage::EnUs)
}

fn tr(lang: UiLanguage, en: &'static str, zh: &'static str) -> &'static str {
    match lang {
        UiLanguage::EnUs => en,
        UiLanguage::ZhCn => zh,
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
    if area.width < 110 || area.height < 18 {
        render_compact_welcome(f, area, data);
        return;
    }

    f.render_widget(Paragraph::new("").style(welcome_bg()), area);

    let inner = area.inner(Margin {
        horizontal: welcome_horizontal_margin(area.width),
        vertical: u16::from(area.height >= 22),
    });

    if inner.width < 96 || inner.height < 20 {
        render_compact_welcome(f, inner, data);
        return;
    }

    render_split_welcome(f, inner, data);
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
            Constraint::Percentage(55),
            Constraint::Length(1),
            Constraint::Percentage(45),
        ])
        .split(area);

    render_identity(f, columns[0], data);
    render_divider(f, columns[1]);
    render_actions(f, columns[2], data);
}

fn render_identity(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let lang = display_language(data);
    if data.api_key_status == "missing" {
        render_octo_with_bubble(f, area, data, lang);
        return;
    }

    let product_mark_height = if area.height >= 23 { 3 } else { 2 };
    // When the welcome area is generous, perch the pixel octopus pet above
    // the wordmark. 10 rows for the sprite + 1 gap; needs ~28 rows total.
    let show_pet = area.height >= 28;
    let pet_height = if show_pet { 10 } else { 0 };
    let pet_gap = if show_pet { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(pet_height),
            Constraint::Length(pet_gap),
            Constraint::Length(product_mark_height),
            Constraint::Length(4),
            Constraint::Min(1),
        ])
        .split(area);

    if show_pet {
        render_octopus_pet(f, chunks[1]);
    }
    render_product_mark(f, chunks[3], lang);

    let shortcuts = vec![
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled(
                tr(
                    lang,
                    "Ask questions, edit files, or run commands.",
                    "可以提问、改文件或运行命令。",
                ),
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled(
                tr(
                    lang,
                    "Plan, approvals, and agents appear only when needed.",
                    "计划、审批和智能体只在需要时出现。",
                ),
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("◇ ", welcome_accent()),
            Span::styled(
                tr(
                    lang,
                    "/help for commands, @path for files, !cmd for shell, /agents on demand.",
                    "/help 看命令，@path 引文件，!cmd 跑终端，/agents 调智能体。",
                ),
                welcome_muted(),
            ),
        ]),
    ];
    f.render_widget(
        Paragraph::new(Text::from(shortcuts))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[4],
    );

    render_capability_line(f, chunks[5], data);
}

// ── Right: Actions ──
fn render_actions(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    if data.api_key_status == "missing" {
        render_api_key_setup(f, area, data);
    } else {
        let lang = display_language(data);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(7),
                Constraint::Length(5),
                Constraint::Length(4),
                Constraint::Min(1),
            ])
            .split(area);
        render_release_header(f, chunks[0], lang);
        render_changelog(f, chunks[1], lang);
        render_quick_access(f, chunks[2], data);
        render_context(f, chunks[3], data);
        render_invitation_and_footer(f, chunks[4], data);
    }
}

fn render_divider(f: &mut Frame, area: Rect) {
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled("|", welcome_accent())))
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(Text::from(lines)).style(welcome_bg()), area);
}

fn render_release_header(f: &mut Frame, area: Rect, lang: UiLanguage) {
    let lines = vec![Line::from(vec![
        Span::styled(
            tr(lang, "Ready surface ", "工作台就绪 "),
            welcome_accent().add_modifier(Modifier::BOLD),
        ),
        Span::styled("v0.1.0", welcome_text().add_modifier(Modifier::BOLD)),
        Span::styled(
            tr(
                lang,
                "        quiet until work starts",
                "        开始工作前保持安静",
            ),
            welcome_muted(),
        ),
    ])];
    f.render_widget(Paragraph::new(Text::from(lines)).style(welcome_bg()), area);
}

fn render_changelog(f: &mut Frame, area: Rect, lang: UiLanguage) {
    let lines = vec![
        Line::from(Span::styled(
            tr(lang, "Start", "开始"),
            welcome_accent().add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                tr(lang, "New conversation", "新对话"),
                welcome_text().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                tr(lang, " - describe the change", " - 描述要改什么"),
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                tr(lang, "Inspect workspace", "检查工作区"),
                welcome_text().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                tr(lang, " - read files before editing", " - 先读文件再编辑"),
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                tr(lang, "Run checks", "运行检查"),
                welcome_text().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                tr(lang, " - verify before summary", " - 验证后再总结"),
                welcome_muted(),
            ),
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
    let lang = display_language(data);
    let (title, hint, footer) = if data.api_key_status == "missing" {
        (
            tr(lang, "Connect a provider first", "先连接模型提供方"),
            tr(
                lang,
                "Paste your API key below.",
                "把 API key 粘贴到底部输入行。",
            ),
            tr(
                lang,
                "enter save key · ctrl+d twice to exit",
                "enter 保存密钥 · ctrl+d 连按两次退出",
            ),
        )
    } else {
        (
            tr(
                lang,
                "Get started: Describe your next step",
                "今天要改什么？",
            ),
            tr(
                lang,
                "Type the work directly, then press Enter.",
                "直接输入要做的事，按 Enter 发送。",
            ),
            tr(
                lang,
                "/help · @path · !cmd · ctrl+d twice to exit",
                "/help · @path · !cmd · ctrl+d 连按两次退出",
            ),
        )
    };
    let model = format!("{} ({})", model_label(&data.model), data.thinking);
    let lines = vec![
        Line::from(vec![Span::styled(
            title,
            welcome_text().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(hint, welcome_muted())]),
        Line::from(""),
        Line::from(vec![Span::styled(footer, welcome_accent())]),
        Line::from(vec![Span::styled(model, welcome_muted())]),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_api_key_setup(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let lang = display_language(data);
    let inner = area.inner(Margin {
        horizontal: u16::from(area.width >= 42),
        vertical: u16::from(area.height >= 14),
    });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            tr(lang, "API setup required", "需要配置 API"),
            welcome_accent().add_modifier(Modifier::BOLD),
        )]))
        .style(welcome_bg())
        .wrap(Wrap { trim: true }),
        chunks[0],
    );

    f.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![Span::styled(
                tr(
                    lang,
                    "Paste your API key in the input line below.",
                    "把 API key 粘贴到底部输入行。",
                ),
                welcome_text(),
            )]),
            Line::from(vec![Span::styled(
                tr(
                    lang,
                    "Octocode saves it, then opens the normal workbench.",
                    "保存后会进入正常工作台。",
                ),
                welcome_muted(),
            )]),
        ]))
        .style(welcome_bg())
        .wrap(Wrap { trim: true }),
        chunks[1],
    );

    let lines = vec![
        Line::from(vec![Span::styled(
            tr(
                lang,
                "Paste key into the input line.",
                "把密钥粘贴到输入行。",
            ),
            welcome_text().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            tr(lang, "Press Enter to save.", "按 Enter 保存。"),
            welcome_text().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            tr(
                lang,
                "The workbench opens after saving.",
                "保存后进入工作台。",
            ),
            welcome_muted(),
        )]),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .wrap(Wrap { trim: true }),
        chunks[2],
    );

    let model = format!("{} ({})", model_label(&data.model), data.thinking);
    f.render_widget(
        Paragraph::new(Text::from(vec![
            context_line(
                tr(lang, "workspace", "工作区"),
                pretty_workspace(&data.workspace_path),
            ),
            context_line(tr(lang, "model", "模型"), model),
        ]))
        .style(welcome_bg())
        .wrap(Wrap { trim: true }),
        chunks[3],
    );
}

fn render_context(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let lang = display_language(data);
    let agents = if data.agents_md.loaded {
        if lang == UiLanguage::ZhCn {
            format!("{} 条规则", data.agents_md.rule_count)
        } else {
            format!("{} rules", data.agents_md.rule_count)
        }
    } else {
        tr(lang, "none", "无").to_string()
    };
    let recent = data.recent_sessions.first().map_or_else(
        || tr(lang, "none yet", "暂无").to_string(),
        |s| s.label.clone(),
    );

    let lines = vec![
        context_line(
            tr(lang, "workspace", "工作区"),
            format!("{}  ·  {}", data.workspace_name, data.detected_language),
        ),
        context_line(
            tr(lang, "model", "模型"),
            format!(
                "{}  ·  {}:{}",
                data.model,
                tr(lang, "think", "思考"),
                data.thinking.to_string().to_lowercase()
            ),
        ),
        context_line(
            tr(lang, "state", "状态"),
            format!(
                "api:{}  ·  config:{}",
                data.api_key_status, data.config_status
            ),
        ),
        context_line(
            tr(lang, "memory", "记忆"),
            format!(
                "AGENTS.md {}  ·  {}:{}",
                agents,
                tr(lang, "recent", "最近"),
                recent
            ),
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
    let lang = display_language(data);
    f.render_widget(Paragraph::new("").style(welcome_bg()), area);

    if data.api_key_status == "missing" {
        render_compact_api_welcome(f, area, data);
        return;
    }

    if area.width >= 70 && area.height >= 12 {
        render_compact_brand_welcome(f, area, data);
        return;
    }

    let headline = tr(
        lang,
        "Describe your next step to start.",
        "请输入下一步要做的事。",
    );
    let footer = vec![
        key_span("enter"),
        action_span(tr(lang, " send   ", " 发送   ")),
        key_span("/help"),
        action_span(tr(lang, " commands", " 命令")),
    ];
    let mut lines = Vec::new();
    lines.extend([
        Line::from(vec![Span::styled(
            "OCTOCODE".to_string(),
            welcome_text().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(headline, welcome_muted())]),
        Line::from(""),
        Line::from(vec![
            Span::styled("octo", welcome_accent()),
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

fn render_compact_brand_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let lang = display_language(data);
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    render_product_mark(f, chunks[0], lang);

    f.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![Span::styled(
                tr(
                    lang,
                    "Get started: Describe your next step",
                    "今天要改什么？",
                ),
                welcome_muted(),
            )]),
            Line::from(vec![Span::styled(
                tr(
                    lang,
                    "Type the work directly, then press Enter.",
                    "直接输入要做的事，按 Enter 发送。",
                ),
                welcome_muted(),
            )]),
        ]))
        .style(welcome_bg())
        .alignment(Alignment::Center),
        chunks[1],
    );

    let workspace = Line::from(vec![
        Span::styled(tr(lang, "workspace ", "工作区 "), welcome_muted()),
        Span::styled(&data.workspace_name, welcome_text()),
        Span::styled(" · ", welcome_muted()),
        Span::styled(
            format!("{}  ·  api:{}", data.model, data.api_key_status),
            welcome_muted(),
        ),
    ]);
    f.render_widget(
        Paragraph::new(workspace)
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[2],
    );

    let footer = Line::from(vec![
        key_span("enter"),
        action_span(tr(lang, " send   ", " 发送   ")),
        key_span("/help"),
        action_span(tr(lang, " commands", " 命令")),
    ]);
    f.render_widget(
        Paragraph::new(footer)
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[3],
    );
}

fn render_compact_api_welcome(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let lang = display_language(data);
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: u16::from(area.height >= 12),
    });

    let mark_rows = match inner.height {
        0..=5 => 2,
        6..=10 => 4,
        _ => 3,
    };
    let show_steps = inner.height >= 10;
    let show_hint = inner.height >= 8;
    let show_context = inner.height >= 9;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(mark_rows),
            Constraint::Length(2),
            Constraint::Length(u16::from(show_hint)),
            Constraint::Length(u16::from(show_steps) * 2),
            Constraint::Min(1),
        ])
        .split(inner);

    render_product_mark(f, chunks[0], lang);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                tr(lang, "API setup required", "需要配置 API"),
                welcome_accent().add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  ", welcome_muted()),
            Span::styled(
                tr(
                    lang,
                    "connect once, then start coding",
                    "连接一次，然后开始编码",
                ),
                welcome_muted(),
            ),
        ]))
        .style(welcome_bg())
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true }),
        chunks[1],
    );

    if show_hint {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                tr(
                    lang,
                    "Paste your API key below, then press Enter.",
                    "粘贴 API key 后按 Enter 保存。",
                ),
                welcome_text(),
            )]))
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
            chunks[2],
        );
    }

    if show_steps {
        let steps = Line::from(vec![Span::styled(
            tr(
                lang,
                "Paste key below, then press Enter.",
                "在下方粘贴密钥，然后按 Enter。",
            ),
            welcome_muted(),
        )]);
        f.render_widget(
            Paragraph::new(steps)
                .style(welcome_bg())
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            chunks[3],
        );
    }

    if show_context {
        let model = format!("{} ({})", model_label(&data.model), data.thinking);
        let workspace = Line::from(vec![
            Span::styled("octo", welcome_accent()),
            Span::styled(" · ", welcome_muted()),
            Span::styled(model, welcome_muted()),
        ]);
        f.render_widget(
            Paragraph::new(workspace)
                .style(welcome_bg())
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            chunks[4],
        );
    }
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

fn pretty_workspace(path: &Path) -> String {
    let display = path.display().to_string();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.display().to_string();
        if let Some(rest) = display.strip_prefix(&home_str) {
            let trimmed = rest.trim_start_matches(['/', '\\']);
            if trimmed.is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", trimmed.replace('\\', "/"));
        }
    }
    display
}

fn render_octo_with_bubble(
    f: &mut Frame,
    area: Rect,
    data: &WelcomeDashboardData,
    lang: UiLanguage,
) {
    // Need at least 14 rows + 60 cols for the pet + bubble side-by-side to
    // breathe. Below that, degrade to the wordmark only.
    if area.height < 14 || area.width < 60 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(if area.height >= 6 { 3 } else { 2 }),
                Constraint::Min(0),
            ])
            .split(area);
        render_product_mark(f, chunks[1], lang);
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(20)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(12),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(cols[0]);
    render_octopus_pet(f, left[1]);
    render_product_mark(f, left[3], lang);

    let lines = compose_octo_dialogue(data, lang);
    let bubble_inner = lines.len() as u16;
    let bubble_outer = bubble_inner
        .saturating_add(2)
        .min(area.height.saturating_sub(2));
    let pad_top = area.height.saturating_sub(bubble_outer) / 2;
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(pad_top),
            Constraint::Length(bubble_outer),
            Constraint::Min(0),
        ])
        .split(cols[1]);
    render_speech_bubble(f, right[1], lines);
}

fn compose_octo_dialogue(data: &WelcomeDashboardData, lang: UiLanguage) -> Vec<Line<'static>> {
    let p = theme::palette();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let greeting = tr(lang, "Hi, I'm Octo.", "嗨，我是 Octo。");
    lines.push(Line::from(Span::styled(
        greeting.to_string(),
        Style::default().fg(p.text).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        tr(
            lang,
            "You haven't set an API key yet.",
            "你还没配 API key。",
        )
        .to_string(),
        Style::default().fg(p.text),
    )));
    lines.push(Line::from(Span::styled(
        tr(
            lang,
            "Paste it into the input row to start.",
            "粘贴到下方输入行就能开工。",
        )
        .to_string(),
        Style::default().fg(p.dim),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            tr(lang, "workspace ", "工作区   ").to_string(),
            Style::default().fg(p.dim),
        ),
        Span::styled(
            pretty_workspace(&data.workspace_path),
            Style::default().fg(p.text),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            tr(lang, "model     ", "模型     ").to_string(),
            Style::default().fg(p.dim),
        ),
        Span::styled(
            format!("{} ({})", model_label(&data.model), data.thinking),
            Style::default().fg(p.dim),
        ),
    ]));
    lines
}

fn render_speech_bubble(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    let p = theme::palette();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.assistant).bg(p.canvas))
        .style(Style::default().bg(p.canvas));
    let para = Paragraph::new(Text::from(lines))
        .block(block)
        .style(Style::default().bg(p.canvas))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn render_octopus_pet(f: &mut Frame, area: Rect) {
    use crate::tui::ascii_art::WELCOME_OCTO_GHOST;
    let p = theme::palette();
    // Tentacle tips: 60% blend of body toward canvas/dim to read as shadow.
    let tip = p.dim;
    let lines: Vec<Line> = WELCOME_OCTO_GHOST
        .iter()
        .map(|row| {
            let spans: Vec<Span> = row
                .chars()
                .map(|ch| {
                    let (block, fg, bg) = match ch {
                        'B' => ("██", p.assistant, p.canvas),
                        'D' => ("██", tip, p.canvas),
                        'E' => ("██", p.canvas, p.canvas),
                        _ => ("  ", p.canvas, p.canvas),
                    };
                    Span::styled(block, Style::default().fg(fg).bg(bg))
                })
                .collect();
            Line::from(spans)
        })
        .collect();
    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Center),
        area,
    );
}

fn render_product_mark(f: &mut Frame, area: Rect, lang: UiLanguage) {
    let workspace_line = tr(lang, "Octocode Workbench", "Octocode 工作台");
    let subtitle = tr(
        lang,
        "Run with `octo` from this workspace.",
        "使用主命令 octo。",
    );
    let mut lines = vec![Line::from(vec![
        Span::styled("OCTOCODE", welcome_accent().add_modifier(Modifier::BOLD)),
        Span::styled(" · ", welcome_muted()),
        Span::styled(workspace_line, welcome_text()),
    ])];

    if area.height >= 2 {
        lines.push(Line::from(Span::styled(subtitle, welcome_muted())));
    }

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
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

fn render_quick_access(f: &mut Frame, area: Rect, data: &WelcomeDashboardData) {
    let lang = display_language(data);
    let lines = vec![
        Line::from(vec![
            Span::styled(tr(lang, "Quick start", "快速开始"), welcome_accent()),
            Span::styled(" : ", welcome_muted()),
            Span::styled(
                tr(
                    lang,
                    "run octo, then /help for commands",
                    "运行 octo 后用 /help 查看命令",
                ),
                welcome_text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("/agents", welcome_accent()),
            Span::styled(
                tr(lang, "  list / switch agents", "  查看 / 切换智能体"),
                welcome_muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("/model", welcome_accent()),
            Span::styled(tr(lang, "  switch model", "  切换模型"), welcome_muted()),
        ]),
        Line::from(vec![
            Span::styled("@path", welcome_accent()),
            Span::styled(tr(lang, "  mention files", "  引用文件"), welcome_muted()),
        ]),
        Line::from(vec![
            Span::styled("!command", welcome_accent()),
            Span::styled(
                tr(lang, "  shell quick action", "  快速执行终端命令"),
                welcome_muted(),
            ),
        ]),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines))
            .style(welcome_bg())
            .wrap(Wrap { trim: true }),
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

fn welcome_text() -> Style {
    welcome_bg().fg(welcome_fg())
}

fn welcome_muted() -> Style {
    welcome_bg().fg(theme::palette().dim)
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
            Path::new("D:/octocode"),
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
            Path::new("D:/octocode"),
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
        assert!(rendered.contains("OCTO"));
        assert!(rendered.contains("Describe your next step"));
        assert!(rendered.contains("/help"));
        assert!(rendered.contains("workspace"));
        assert!(rendered.contains("memory"));
        assert!(!rendered.contains("Project"));
    }

    #[test]
    fn wide_render_uses_simple_header() {
        let data = test_data(Vec::new(), true);
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        let compact = rendered.replace(' ', "");

        assert!(rendered.contains("Octocode") || rendered.contains("OCTOCODE"));
        assert!(compact.contains("octo"));
    }

    #[test]
    fn medium_render_stacks_without_losing_welcome_actions() {
        let data = test_data(Vec::new(), true);
        let mut terminal = Terminal::new(TestBackend::new(90, 24)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("OCTO"));
        assert!(rendered.contains("Get started"));
        assert!(rendered.contains("workspace"));
        assert!(rendered.contains("/help"));
    }

    #[test]
    fn missing_api_key_render_shows_setup_instead_of_starters() {
        let data = test_data(Vec::new(), false);
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("OCTO"));
        assert!(rendered.contains("API setup required"));
        assert!(rendered.contains("Paste your API key"));
        assert!(rendered.contains("Press Enter"));
        assert!(!rendered.contains("Type below, or press 1-3"));
        assert!(!rendered.contains("Quick start"));
    }

    #[test]
    fn compact_render_contains_title_and_shortcuts() {
        let data = test_data(Vec::new(), true);
        let mut terminal = Terminal::new(TestBackend::new(50, 12)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw compact welcome");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("OCTO"));
        assert!(!rendered.contains("1-3"));
        assert!(rendered.contains("enter"));
        assert!(rendered.contains("/help"));
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
        assert!(rendered.contains("press Enter"));
        assert!(!rendered.contains("starters"));
    }

    #[test]
    fn chinese_language_renders_api_setup_copy() {
        let mut data = test_data(Vec::new(), false);
        data.display_language = "zh-CN".to_string();
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).expect("create test terminal");
        terminal
            .draw(|f| render_welcome(f, f.area(), &data))
            .expect("draw welcome");
        let rendered = buffer_text(terminal.backend());
        let compact = rendered.replace(' ', "");

        assert!(compact.contains("需要配置API"));
        assert!(compact.contains("密钥粘贴"));
        assert!(compact.contains("按Enter保存"));
        assert!(!rendered.contains("API setup required"));
    }

    fn test_data(
        recent_sessions: Vec<RecentSessionItem>,
        has_api_key: bool,
    ) -> WelcomeDashboardData {
        let mut data = WelcomeDashboardData::from_parts(
            Path::new("D:/octocode"),
            DeepSeekModel::Flash,
            ThinkingMode::Auto,
            has_api_key,
            true,
            "no turn yet",
            recent_sessions,
        );
        data.display_language = "en-US".to_string();
        data
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

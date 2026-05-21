use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::deepseek::{DeepSeekModel, ThinkingMode};
use crate::storage;
use crate::tui::{app::RendererMode, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Model,
    Safety,
    Interface,
    Agents,
}

impl SettingsTab {
    pub const ALL: [Self; 4] = [Self::Model, Self::Safety, Self::Interface, Self::Agents];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Model => "Model",
            Self::Safety => "Safety",
            Self::Interface => "Interface",
            Self::Agents => "Agents",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    #[must_use]
    pub fn previous(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

pub struct SettingsPanelProps<'a> {
    pub selected_tab: SettingsTab,
    pub selected_row: usize,
    pub active_model: &'a DeepSeekModel,
    pub active_thinking: &'a ThinkingMode,
    pub renderer: RendererMode,
    pub config: &'a storage::Config,
    pub theme_label: &'a str,
}

#[must_use]
pub fn row_count(tab: SettingsTab) -> usize {
    match tab {
        SettingsTab::Model => 7,
        SettingsTab::Safety => 8,
        SettingsTab::Interface => 7,
        SettingsTab::Agents => 10,
    }
}

pub fn render_settings_panel(f: &mut Frame, area: Rect, props: SettingsPanelProps<'_>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let chinese = crate::tui::welcome::is_chinese_display_language(&props.config.ui.language);
    let mut lines = vec![
        Line::from(""),
        tab_line(props.selected_tab, chinese),
        Line::from(""),
        Line::from(vec![Span::styled(
            if chinese {
                "当前设置"
            } else {
                "Current settings"
            },
            Style::default()
                .fg(p.text)
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];
    lines.extend(settings_rows(&props));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("↑↓", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(
            if chinese {
                " 导航   "
            } else {
                " navigate   "
            },
            Style::default().fg(p.muted).bg(p.canvas),
        ),
        Span::styled("Tab", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(
            if chinese {
                " 切换页签   "
            } else {
                " switch tab   "
            },
            Style::default().fg(p.muted).bg(p.canvas),
        ),
        Span::styled("Enter/Space", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(
            if chinese { " 修改   " } else { " edit   " },
            Style::default().fg(p.muted).bg(p.canvas),
        ),
        Span::styled("Esc", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(
            if chinese { " 关闭" } else { " close" },
            Style::default().fg(p.muted).bg(p.canvas),
        ),
    ]));

    let block = Block::default()
        .title(if chinese { " 设置 " } else { " Settings " })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.divider).bg(p.canvas))
        .style(Style::default().fg(p.text).bg(p.canvas));
    let panel = Paragraph::new(Text::from(lines))
        .block(block)
        .style(Style::default().fg(p.text).bg(p.canvas))
        .wrap(Wrap { trim: false });
    f.render_widget(panel, area);
}

fn tab_line(selected: SettingsTab, chinese: bool) -> Line<'static> {
    let p = theme::palette();
    let mut spans = Vec::new();
    for tab in SettingsTab::ALL {
        let active = tab == selected;
        spans.push(Span::styled(
            if active { "● " } else { "○ " },
            Style::default()
                .fg(if active { p.accent } else { p.muted })
                .bg(p.canvas),
        ));
        spans.push(Span::styled(
            tab_label(tab, chinese),
            Style::default()
                .fg(if active { p.accent } else { p.secondary })
                .bg(p.canvas)
                .add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        spans.push(Span::styled("   ", Style::default().bg(p.canvas)));
    }
    Line::from(spans)
}

fn settings_rows(props: &SettingsPanelProps<'_>) -> Vec<Line<'static>> {
    let chinese = crate::tui::welcome::is_chinese_display_language(&props.config.ui.language);
    match props.selected_tab {
        SettingsTab::Model => rows(
            &[
                (
                    label("Provider", "提供方", chinese),
                    props.config.provider.default.as_str().to_string(),
                ),
                (
                    label("Active model", "当前模型", chinese),
                    format!("{}", props.active_model),
                ),
                (
                    label("Default model", "默认模型", chinese),
                    format!("{}", props.config.model.default),
                ),
                (
                    label("Heavy model", "复杂任务模型", chinese),
                    format!("{}", props.config.model.heavy),
                ),
                (
                    label("Active thinking", "当前思考模式", chinese),
                    format!("{}", props.active_thinking),
                ),
                (
                    label("Default thinking", "默认思考模式", chinese),
                    format!("{}", props.config.model.thinking_mode),
                ),
                (
                    label("Reasoning effort", "推理强度", chinese),
                    format!("{}", props.config.model.reasoning_effort),
                ),
            ],
            props.selected_row,
        ),
        SettingsTab::Safety => rows(
            &[
                (
                    label("Autonomy level", "自主级别", chinese),
                    props.config.policy.autonomy_level.as_str().to_string(),
                ),
                (
                    label("Safe reads", "安全读取", chinese),
                    on_off(props.config.policy.auto_approve_safe_read, chinese),
                ),
                (
                    label("Auto mode", "自动模式", chinese),
                    on_off(props.config.policy.auto_mode, chinese),
                ),
                (
                    label("Write approval", "写入审批", chinese),
                    required(props.config.policy.require_approval_for_write, chinese),
                ),
                (
                    label("Command approval", "命令审批", chinese),
                    required(props.config.policy.require_approval_for_command, chinese),
                ),
                (
                    label("Network access", "网络访问", chinese),
                    on_off(props.config.policy.network_access, chinese),
                ),
                (
                    label("Protected paths", "受保护路径", chinese),
                    format!(
                        "{} ({})",
                        on_off(props.config.policy.block_protected_paths, chinese),
                        props.config.paths.protected.len()
                    ),
                ),
                (
                    label("Command timeout", "命令超时", chinese),
                    format!("{}s", props.config.policy.command_timeout_seconds),
                ),
            ],
            props.selected_row,
        ),
        SettingsTab::Interface => rows(
            &[
                (
                    label("Language", "语言", chinese),
                    props.config.ui.language.clone(),
                ),
                (
                    label("Theme", "主题", chinese),
                    props.theme_label.to_string(),
                ),
                (
                    label("Motion", "动画", chinese),
                    props.config.ui.motion.clone(),
                ),
                (
                    label("Renderer", "渲染器", chinese),
                    props.renderer.label().to_string(),
                ),
                (
                    label("Reasoning summary", "思考摘要", chinese),
                    on_off(props.config.ui.show_reasoning_summary, chinese),
                ),
                (
                    label("Raw reasoning", "原始思考", chinese),
                    on_off(props.config.ui.show_raw_reasoning, chinese),
                ),
                (
                    label("Cache HUD", "缓存 HUD", chinese),
                    on_off(props.config.ui.show_cache_hud, chinese),
                ),
            ],
            props.selected_row,
        ),
        SettingsTab::Agents => rows(
            &[
                (
                    label("Router", "路由器", chinese),
                    on_off(props.config.router.enabled, chinese),
                ),
                (
                    label("Model classifier", "模型分类器", chinese),
                    on_off(props.config.router.use_model_classifier, chinese),
                ),
                (
                    label("Subagents", "子智能体", chinese),
                    on_off(props.config.subagent.enabled, chinese),
                ),
                (
                    label("Swarm", "群组", chinese),
                    on_off(props.config.subagent.swarm_enabled, chinese),
                ),
                (
                    label("Max parallel", "最大并发", chinese),
                    props.config.subagent.max_parallel.to_string(),
                ),
                (
                    label("Auto decompose", "自动拆解", chinese),
                    on_off(props.config.subagent.auto_decompose, chinese),
                ),
                (
                    label("Custom agents", "自定义智能体", chinese),
                    on_off(props.config.subagent.allow_custom_agents, chinese),
                ),
                (
                    "MCP",
                    format!(
                        "{} ({})",
                        on_off(props.config.mcp.enabled, chinese),
                        props.config.mcp.servers.len()
                    ),
                ),
                (
                    label("Hooks", "Hooks", chinese),
                    hook_count(&props.config.hooks).to_string(),
                ),
                (
                    label("Telemetry", "遥测", chinese),
                    on_off(props.config.telemetry.enabled, chinese),
                ),
            ],
            props.selected_row,
        ),
    }
}

fn tab_label(tab: SettingsTab, chinese: bool) -> &'static str {
    if !chinese {
        return tab.label();
    }
    match tab {
        SettingsTab::Model => "模型",
        SettingsTab::Safety => "安全",
        SettingsTab::Interface => "界面",
        SettingsTab::Agents => "智能体",
    }
}

fn label(english: &'static str, chinese_label: &'static str, chinese: bool) -> &'static str {
    if chinese {
        chinese_label
    } else {
        english
    }
}

fn on_off(value: bool, chinese: bool) -> String {
    match (value, chinese) {
        (true, true) => "开启",
        (false, true) => "关闭",
        (true, false) => "on",
        (false, false) => "off",
    }
    .to_string()
}

fn required(value: bool, chinese: bool) -> String {
    match (value, chinese) {
        (true, true) => "需要",
        (false, true) => "跳过",
        (true, false) => "required",
        (false, false) => "skipped",
    }
    .to_string()
}

fn hook_count(hooks: &storage::config::HooksConfig) -> usize {
    hooks.user_prompt_submit.len()
        + hooks.pre_tool.len()
        + hooks.post_tool.len()
        + hooks.session_start.len()
        + hooks.session_end.len()
        + hooks.stop.len()
}

fn rows(values: &[(&str, String)], selected_row: usize) -> Vec<Line<'static>> {
    let p = theme::palette();
    values
        .iter()
        .enumerate()
        .map(|(idx, (label, value))| {
            let selected = idx == selected_row.min(values.len().saturating_sub(1));
            let label_style = Style::default()
                .fg(if selected { p.accent } else { p.text })
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD);
            Line::from(vec![
                Span::styled(
                    if selected { "> " } else { "  " },
                    Style::default()
                        .fg(if selected { p.accent } else { p.muted })
                        .bg(p.canvas),
                ),
                Span::styled(format!("{label:<24}"), label_style),
                Span::styled(value.clone(), Style::default().fg(p.text).bg(p.canvas)),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn settings_panel_renders_tabs_and_selected_section() {
        let mut config = storage::Config::default();
        config.ui.language = "en-US".to_string();
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).expect("terminal");
        terminal
            .draw(|f| {
                render_settings_panel(
                    f,
                    f.area(),
                    SettingsPanelProps {
                        selected_tab: SettingsTab::Interface,
                        selected_row: 3,
                        active_model: &DeepSeekModel::Flash,
                        active_thinking: &ThinkingMode::Auto,
                        renderer: RendererMode::Classic,
                        config: &config,
                        theme_label: "light",
                    },
                );
            })
            .expect("draw");
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();

        assert!(rendered.contains("Model"));
        assert!(rendered.contains("Safety"));
        assert!(rendered.contains("Interface"));
        assert!(rendered.contains("Agents"));
        assert!(rendered.contains("Renderer"));
        assert!(rendered.contains("classic"));
        assert!(!rendered.contains("Sound"));
        assert!(!rendered.contains("Statusline mode"));
    }

    #[test]
    fn settings_panel_shows_policy_values_from_config() {
        let mut config = storage::Config::default();
        config.ui.language = "en-US".to_string();
        config.policy.autonomy_level = storage::config::AutonomyLevel::Medium;
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).expect("terminal");
        terminal
            .draw(|f| {
                render_settings_panel(
                    f,
                    f.area(),
                    SettingsPanelProps {
                        selected_tab: SettingsTab::Safety,
                        selected_row: 0,
                        active_model: &DeepSeekModel::Flash,
                        active_thinking: &ThinkingMode::Auto,
                        renderer: RendererMode::Classic,
                        config: &config,
                        theme_label: "auto",
                    },
                );
            })
            .expect("draw");
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();

        assert!(rendered.contains("Autonomy level"));
        assert!(rendered.contains("medium"));
        assert!(!rendered.contains("Autonomy level        high"));
    }
}

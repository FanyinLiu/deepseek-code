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
    let mut lines = vec![
        Line::from(""),
        tab_line(props.selected_tab),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Current settings",
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
        Span::styled(" navigate   ", Style::default().fg(p.muted).bg(p.canvas)),
        Span::styled("Tab", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(" switch tab   ", Style::default().fg(p.muted).bg(p.canvas)),
        Span::styled("Esc", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(" close", Style::default().fg(p.muted).bg(p.canvas)),
    ]));

    let block = Block::default()
        .title(" Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.divider).bg(p.canvas))
        .style(Style::default().fg(p.text).bg(p.canvas));
    let panel = Paragraph::new(Text::from(lines))
        .block(block)
        .style(Style::default().fg(p.text).bg(p.canvas))
        .wrap(Wrap { trim: false });
    f.render_widget(panel, area);
}

fn tab_line(selected: SettingsTab) -> Line<'static> {
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
            tab.label(),
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
    match props.selected_tab {
        SettingsTab::Model => rows(
            &[
                (
                    "Provider",
                    props.config.provider.default.as_str().to_string(),
                ),
                ("Active model", format!("{}", props.active_model)),
                ("Default model", format!("{}", props.config.model.default)),
                ("Heavy model", format!("{}", props.config.model.heavy)),
                ("Active thinking", format!("{}", props.active_thinking)),
                (
                    "Default thinking",
                    format!("{}", props.config.model.thinking_mode),
                ),
                (
                    "Reasoning effort",
                    format!("{}", props.config.model.reasoning_effort),
                ),
            ],
            props.selected_row,
        ),
        SettingsTab::Safety => rows(
            &[
                (
                    "Autonomy level",
                    props.config.policy.autonomy_level.as_str().to_string(),
                ),
                (
                    "Safe reads",
                    on_off(props.config.policy.auto_approve_safe_read),
                ),
                ("Auto mode", on_off(props.config.policy.auto_mode)),
                (
                    "Write approval",
                    required(props.config.policy.require_approval_for_write),
                ),
                (
                    "Command approval",
                    required(props.config.policy.require_approval_for_command),
                ),
                ("Network access", on_off(props.config.policy.network_access)),
                (
                    "Protected paths",
                    format!(
                        "{} ({})",
                        on_off(props.config.policy.block_protected_paths),
                        props.config.paths.protected.len()
                    ),
                ),
                (
                    "Command timeout",
                    format!("{}s", props.config.policy.command_timeout_seconds),
                ),
            ],
            props.selected_row,
        ),
        SettingsTab::Interface => rows(
            &[
                ("Language", props.config.ui.language.clone()),
                ("Theme", props.theme_label.to_string()),
                ("Motion", props.config.ui.motion.clone()),
                ("Renderer", props.renderer.label().to_string()),
                (
                    "Reasoning summary",
                    on_off(props.config.ui.show_reasoning_summary),
                ),
                ("Raw reasoning", on_off(props.config.ui.show_raw_reasoning)),
                ("Cache HUD", on_off(props.config.ui.show_cache_hud)),
            ],
            props.selected_row,
        ),
        SettingsTab::Agents => rows(
            &[
                ("Router", on_off(props.config.router.enabled)),
                (
                    "Model classifier",
                    on_off(props.config.router.use_model_classifier),
                ),
                ("Subagents", on_off(props.config.subagent.enabled)),
                ("Swarm", on_off(props.config.subagent.swarm_enabled)),
                (
                    "Max parallel",
                    props.config.subagent.max_parallel.to_string(),
                ),
                (
                    "Auto decompose",
                    on_off(props.config.subagent.auto_decompose),
                ),
                (
                    "Custom agents",
                    on_off(props.config.subagent.allow_custom_agents),
                ),
                (
                    "MCP",
                    format!(
                        "{} ({})",
                        on_off(props.config.mcp.enabled),
                        props.config.mcp.servers.len()
                    ),
                ),
                ("Hooks", hook_count(&props.config.hooks).to_string()),
                ("Telemetry", on_off(props.config.telemetry.enabled)),
            ],
            props.selected_row,
        ),
    }
}

fn on_off(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

fn required(value: bool) -> String {
    if value { "required" } else { "skipped" }.to_string()
}

fn hook_count(hooks: &storage::config::HooksConfig) -> usize {
    hooks.pre_tool.len() + hooks.post_tool.len() + hooks.stop.len()
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
                        config: &storage::Config::default(),
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

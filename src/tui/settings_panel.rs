use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::deepseek::{DeepSeekModel, ThinkingMode};
use crate::tui::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    SessionDefaults,
    TaskDefaults,
    Preferences,
    Sound,
}

impl SettingsTab {
    pub const ALL: [Self; 4] = [
        Self::SessionDefaults,
        Self::TaskDefaults,
        Self::Preferences,
        Self::Sound,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SessionDefaults => "Session Defaults",
            Self::TaskDefaults => "Task Defaults",
            Self::Preferences => "Preferences",
            Self::Sound => "Sound",
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
    pub model: &'a DeepSeekModel,
    pub thinking: &'a ThinkingMode,
    pub theme_label: &'a str,
    pub motion_label: &'a str,
}

#[must_use]
pub fn row_count(tab: SettingsTab) -> usize {
    match tab {
        SettingsTab::SessionDefaults | SettingsTab::TaskDefaults => 5,
        SettingsTab::Preferences => 10,
        SettingsTab::Sound => 3,
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
            "Filter settings...",
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
        Span::styled("Enter", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(" select   ", Style::default().fg(p.muted).bg(p.canvas)),
        Span::styled("Tab", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(" switch tab   ", Style::default().fg(p.muted).bg(p.canvas)),
        Span::styled("Esc", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(" cancel", Style::default().fg(p.muted).bg(p.canvas)),
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
        SettingsTab::SessionDefaults => rows(
            &[
                ("Default model", format!("{}", props.model)),
                ("Thinking mode", format!("{}", props.thinking)),
                ("Interaction mode", "auto".to_string()),
                ("Autonomy level", "high".to_string()),
                ("Compaction limit", "1M".to_string()),
            ],
            props.selected_row,
        ),
        SettingsTab::TaskDefaults => rows(
            &[
                ("Orchestrator model", format!("{} / auto", props.model)),
                ("Worker model", format!("{} / auto", props.model)),
                ("Verifier model", format!("{} / auto", props.model)),
                ("Skip review", "off".to_string()),
                ("Skip user tests", "off".to_string()),
            ],
            props.selected_row,
        ),
        SettingsTab::Preferences => rows(
            &[
                ("Diff display mode", "GitHub".to_string()),
                ("Theme", props.theme_label.to_string()),
                ("Tool result display", "Compact".to_string()),
                ("Cursor style", "Inline block".to_string()),
                ("Prompt precache", "on".to_string()),
                ("Motion", props.motion_label.to_string()),
                ("Hooks", "enabled".to_string()),
            ],
            props.selected_row,
        ),
        SettingsTab::Sound => rows(
            &[
                ("Completion sound", "FX-OK01".to_string()),
                ("Waiting input sound", "FX-ACK01".to_string()),
                ("Play sound", "always".to_string()),
            ],
            props.selected_row,
        ),
    }
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
                        selected_tab: SettingsTab::Preferences,
                        selected_row: 3,
                        model: &DeepSeekModel::Flash,
                        thinking: &ThinkingMode::Auto,
                        theme_label: "light",
                        motion_label: "subtle",
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

        assert!(rendered.contains("Session Defaults"));
        assert!(rendered.contains("Task Defaults"));
        assert!(rendered.contains("Preferences"));
        assert!(rendered.contains("Sound"));
        assert!(rendered.contains("Tool result display"));
        assert!(rendered.contains("Motion"));
        assert!(rendered.contains("subtle"));
    }
}

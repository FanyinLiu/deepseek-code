use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::{DeepSeekModel, ThinkingMode};
use crate::tui::{status_bar, theme};

#[must_use]
pub fn model_hint_text(model: &DeepSeekModel, thinking: &ThinkingMode) -> String {
    format!("{} ({})", model_display_name(model), thinking)
}

pub fn render_model_hint(
    f: &mut Frame,
    area: Rect,
    model: &DeepSeekModel,
    thinking: &ThinkingMode,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let line = Line::from(vec![Span::styled(
        model_hint_text(model, thinking),
        Style::default()
            .fg(p.text)
            .bg(p.canvas)
            .add_modifier(Modifier::BOLD),
    )]);
    f.render_widget(
        Paragraph::new(line)
            .style(Style::default().fg(p.text).bg(p.canvas))
            .alignment(Alignment::Right),
        area,
    );
}

pub fn render_composer_hint(
    f: &mut Frame,
    area: Rect,
    model: &DeepSeekModel,
    thinking: &ThinkingMode,
    activity: Option<status_bar::StatusActivity<'_>>,
    mode: status_bar::AppMode,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let model_text = model_hint_text(model, thinking);
    let mut activity_text = activity
        .as_ref()
        .map(|activity| status_bar::activity_hint_text(activity, thinking))
        .unwrap_or_default();
    let width = area.width as usize;
    let right_len = display_width(&model_text);
    if !activity_text.is_empty() && display_width(&activity_text) + right_len + 1 > width {
        let max_left = width.saturating_sub(right_len + 2);
        activity_text = truncate_display_width(&activity_text, max_left);
    }
    let gap = hint_gap(width, display_width(&activity_text), right_len);

    let mut spans = Vec::new();
    if !activity_text.is_empty() {
        spans.push(Span::styled(
            activity_text,
            Style::default()
                .fg(mode.color())
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if gap > 0 {
        spans.push(Span::styled(" ".repeat(gap), Style::default().bg(p.canvas)));
    }
    spans.push(Span::styled(
        model_text,
        Style::default()
            .fg(p.text)
            .bg(p.canvas)
            .add_modifier(Modifier::BOLD),
    ));

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().fg(p.text).bg(p.canvas)),
        area,
    );
}

fn hint_gap(width: usize, left_len: usize, right_len: usize) -> usize {
    if left_len == 0 {
        width.saturating_sub(right_len)
    } else {
        width.saturating_sub(left_len + right_len).max(1)
    }
}

fn truncate_display_width(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_string();
    }
    let mut out = String::new();
    let target = max_width.saturating_sub(1);
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = char_display_width(ch);
        if used + ch_width > target {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

fn display_width(value: &str) -> usize {
    value.chars().map(char_display_width).sum()
}

fn char_display_width(ch: char) -> usize {
    if matches!(
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
            | 0x20000..=0x3FFFD
    ) {
        2
    } else {
        1
    }
}

fn model_display_name(model: &DeepSeekModel) -> &'static str {
    match model {
        DeepSeekModel::Pro => "DeepSeek V4 Pro",
        DeepSeekModel::Flash => "DeepSeek V4 Flash",
        DeepSeekModel::LegacyChat => "DeepSeek Chat",
        DeepSeekModel::LegacyReasoner => "DeepSeek Reasoner",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn model_hint_formats_model_and_thinking_mode() {
        assert_eq!(
            model_hint_text(&DeepSeekModel::Pro, &ThinkingMode::Auto),
            "DeepSeek V4 Pro (auto)"
        );
    }

    #[test]
    fn model_hint_uses_theme_colors() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).expect("terminal");
        terminal
            .draw(|f| render_model_hint(f, f.area(), &DeepSeekModel::Flash, &ThinkingMode::Auto))
            .expect("draw");

        let cell = terminal.backend().buffer().cell((22, 0)).expect("cell");
        assert_eq!(cell.fg, theme::LIGHT_PALETTE.text);
        assert_eq!(cell.bg, theme::LIGHT_PALETTE.canvas);
    }

    #[test]
    fn composer_hint_places_activity_next_to_input_context() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(96, 1)).expect("terminal");
        terminal
            .draw(|f| {
                render_composer_hint(
                    f,
                    f.area(),
                    &DeepSeekModel::Flash,
                    &ThinkingMode::On,
                    Some(status_bar::StatusActivity {
                        title: "Fix input colors",
                        elapsed_ms: 2_000,
                        input_tokens: 41,
                        tokens: 578,
                        agent_tokens: 0,
                        thought_seconds: 2,
                    }),
                    status_bar::AppMode::Chat,
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
        assert!(rendered.contains("⠴ Fix input colors... (2s · ↓ 578 tokens · thought for 2s"));
        assert!(!rendered.contains("↑ 41 tokens"));
        assert!(rendered.contains("DeepSeek V4 Flash (on)"));
    }
}

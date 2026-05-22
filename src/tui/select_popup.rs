use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::tui::theme;

/// Rich question payload for `render_select_popup_rich`. Mirrors
/// `tools::ask_user::PendingUserQuestion` but the popup module stays
/// decoupled from `tools::` so it can be used in tests in isolation.
pub struct RichOption<'a> {
    pub label: &'a str,
    pub description: &'a str,
    pub preview: Option<&'a str>,
}

pub struct RichSelectState<'a> {
    pub title: &'a str,
    pub question: &'a str,
    pub options: &'a [RichOption<'a>],
    pub selected_index: usize,
    pub multi_select: bool,
    /// Same length as `options` when `multi_select` is true; ignored otherwise.
    pub checked: &'a [bool],
}

pub fn render_select_popup(
    f: &mut Frame,
    area: Rect,
    title: &str,
    options: &[String],
    selected_index: usize,
) {
    if area.width < 24 || area.height < 4 {
        return;
    }

    let p = theme::palette();
    let popup_width = area.width.min(80).min(area.width.saturating_sub(2).max(24));
    let popup_height = (options.len() as u16 + 4)
        .clamp(6, 18)
        .min(area.height.saturating_sub(2).max(4));
    let popup_area = centered_bottom_rect(area, popup_width, popup_height);
    let selected_index = selected_index.min(options.len().saturating_sub(1));

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        title.to_string(),
        Style::default().fg(p.text).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for (index, option) in options.iter().enumerate() {
        let marker = if index == selected_index { ">" } else { " " };
        let text = format!("{marker} {}", option);
        let style = if index == selected_index {
            Style::default()
                .fg(p.inverse_text)
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.text)
        };
        lines.push(Line::from(Span::styled(text, style)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ select · Enter confirm · Esc cancel",
        Style::default().fg(p.dim),
    )));

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(p.canvas))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.divider)),
        );

    f.render_widget(Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

/// Rich popup: option list on the left, preview pane on the right when the
/// focused option has a preview. When `multi_select` is true, options render
/// with `[x]` / `[ ]` markers and the footer advertises Space to toggle.
pub fn render_select_popup_rich(f: &mut Frame, area: Rect, state: &RichSelectState) {
    if area.width < 40 || area.height < 6 || state.options.is_empty() {
        return;
    }

    let p = theme::palette();
    let popup_width = area
        .width
        .min(110)
        .min(area.width.saturating_sub(2).max(40));
    let popup_height = ((state.options.len() as u16).saturating_add(6))
        .clamp(10, 22)
        .min(area.height.saturating_sub(2).max(6));
    let popup_area = centered_bottom_rect(area, popup_width, popup_height);
    let selected = state.selected_index.min(state.options.len() - 1);

    f.render_widget(Clear, popup_area);
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.divider))
        .style(Style::default().bg(p.canvas));
    let inner = outer.inner(popup_area);
    f.render_widget(outer, popup_area);

    let has_preview = state
        .options
        .iter()
        .any(|o| o.preview.map(|s| !s.trim().is_empty()).unwrap_or(false));

    let (left_area, preview_area) = if has_preview && inner.width > 60 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        state.title.to_string(),
        Style::default().fg(p.text).add_modifier(Modifier::BOLD),
    )));
    if !state.question.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            state.question.to_string(),
            Style::default().fg(p.dim),
        )));
    }
    lines.push(Line::from(""));

    for (i, opt) in state.options.iter().enumerate() {
        let marker = if state.multi_select {
            let checked = state.checked.get(i).copied().unwrap_or(false);
            if checked {
                "[x]"
            } else {
                "[ ]"
            }
        } else if i == selected {
            " > "
        } else {
            "   "
        };
        let line_style = if i == selected {
            Style::default()
                .fg(p.inverse_text)
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.text)
        };
        lines.push(Line::from(Span::styled(
            format!("{marker} {}", opt.label),
            line_style,
        )));
        if !opt.description.trim().is_empty() {
            lines.push(Line::from(Span::styled(
                format!("      {}", opt.description),
                Style::default().fg(p.dim),
            )));
        }
    }
    lines.push(Line::from(""));
    let footer = if state.multi_select {
        "↑/↓ move · Space toggle · Enter confirm · Esc cancel"
    } else {
        "↑/↓ move · Enter confirm · Esc cancel"
    };
    lines.push(Line::from(Span::styled(footer, Style::default().fg(p.dim))));

    let left_para = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(p.canvas))
        .wrap(Wrap { trim: false });
    f.render_widget(left_para, left_area);

    if let Some(area) = preview_area {
        let preview_text = state
            .options
            .get(selected)
            .and_then(|o| o.preview)
            .unwrap_or("(no preview)");
        let preview_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(p.divider));
        let inner_preview = preview_block.inner(area);
        f.render_widget(preview_block, area);
        let para = Paragraph::new(preview_text)
            .style(Style::default().fg(p.text).bg(p.canvas))
            .wrap(Wrap { trim: false });
        f.render_widget(para, inner_preview);
    }
}

fn centered_bottom_rect(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height);
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn select_popup_marks_selected_option() {
        let options = vec!["Alpha".to_string(), "Beta".to_string()];
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("terminal");

        terminal
            .draw(|f| render_select_popup(f, f.area(), "Choose", &options, 1))
            .expect("draw");

        let selected_cells = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.symbol() == "B" && cell.bg == theme::palette().accent)
            .count();

        assert!(selected_cells >= 1);
    }

    fn collect_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area().height {
            for x in 0..buf.area().width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn rich_popup_shows_preview_for_focused_option() {
        let opts = vec![
            RichOption {
                label: "Alpha",
                description: "first option",
                preview: Some("preview-A"),
            },
            RichOption {
                label: "Beta",
                description: "second option",
                preview: Some("preview-B"),
            },
        ];
        let state = RichSelectState {
            title: "Pick one",
            question: "Which one?",
            options: &opts,
            selected_index: 1,
            multi_select: false,
            checked: &[],
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).expect("terminal");
        terminal
            .draw(|f| render_select_popup_rich(f, f.area(), &state))
            .expect("draw");
        let text = collect_text(&terminal);
        assert!(text.contains("Pick one"));
        assert!(text.contains("Alpha"));
        assert!(text.contains("Beta"));
        assert!(
            text.contains("preview-B"),
            "preview for selected should show"
        );
        assert!(!text.contains("preview-A"));
    }

    #[test]
    fn rich_popup_multi_select_shows_checkboxes() {
        let opts = vec![
            RichOption {
                label: "Alpha",
                description: "",
                preview: None,
            },
            RichOption {
                label: "Beta",
                description: "",
                preview: None,
            },
        ];
        let state = RichSelectState {
            title: "Pick any",
            question: "",
            options: &opts,
            selected_index: 0,
            multi_select: true,
            checked: &[true, false],
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).expect("terminal");
        terminal
            .draw(|f| render_select_popup_rich(f, f.area(), &state))
            .expect("draw");
        let text = collect_text(&terminal);
        assert!(text.contains("[x] Alpha"));
        assert!(text.contains("[ ] Beta"));
        assert!(text.contains("Space toggle"));
    }
}

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::theme;

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
}

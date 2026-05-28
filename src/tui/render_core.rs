use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::DeepSeekModel;

use super::{model_hint, theme};

pub fn render_canvas(f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let line = " ".repeat(area.width as usize);
    let line_style = Style::default().bg(p.canvas);
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled(line.as_str(), line_style)))
        .collect::<Vec<_>>();
    f.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().fg(p.text).bg(p.canvas)),
        area,
    );
}

pub fn render_top_model_badge(f: &mut Frame, area: Rect, model: &DeepSeekModel) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let label = model_hint::model_display_name(model);
    let label_width = label.len() as u16;
    if area.width < label_width {
        return;
    }
    let p = theme::palette();
    let badge_area = Rect::new(
        area.right().saturating_sub(label_width),
        area.y,
        label_width,
        1,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label.to_string(),
            Style::default()
                .fg(p.dim)
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        )))
        .style(Style::default().fg(p.dim).bg(p.canvas)),
        badge_area,
    );
}

pub fn render_classic_divider(f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme::palette();
    let line = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            line,
            Style::default().fg(p.dim).bg(p.canvas),
        )))
        .style(Style::default().fg(p.text).bg(p.canvas)),
        area,
    );
}

pub fn render_jump_to_bottom_hint(f: &mut Frame, area: Rect) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_handles_zero_area() {
        let backend = ratatui::backend::TestBackend::new(10, 3);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");

        terminal
            .draw(|f| render_canvas(f, Rect::new(0, 0, 0, 0)))
            .expect("draw");
    }
}

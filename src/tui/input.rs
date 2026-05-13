use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::tui::theme;

/// Quiet terminal input: no border, just a clean prompt. The visible cursor
/// comes from the terminal itself (positioned by the caller via
/// [`terminal_cursor_position`]); we never paint a glyph for it.
pub fn render_input(f: &mut Frame, area: Rect, input_text: &str, _pending_options: Option<&[String]>) {
    let lines: Vec<Line> = if input_text.is_empty() {
        vec![Line::from(vec![prompt_span()])]
    } else {
        input_text
            .split('\n')
            .enumerate()
            .map(|(i, line)| {
                let leading = if i == 0 {
                    prompt_span()
                } else {
                    Span::styled("  ", input_style())
                };
                Line::from(vec![leading, Span::styled(line.to_string(), input_style())])
            })
            .collect()
    };

    let p = theme::palette();
    let input = Paragraph::new(lines).style(Style::default().bg(p.canvas).fg(p.text));
    f.render_widget(input, area);
}

pub fn render_api_key_input(f: &mut Frame, area: Rect, input_text: &str) {
    let display = mask_secret(input_text);
    let lines = if display.is_empty() {
        vec![Line::from(vec![
            prompt_span(),
            Span::styled("paste API key...", muted_style()),
        ])]
    } else {
        vec![Line::from(vec![
            prompt_span(),
            Span::styled(display, input_style()),
        ])]
    };

    let p = theme::palette();
    let input = Paragraph::new(lines).style(Style::default().bg(p.canvas).fg(p.text));
    f.render_widget(input, area);
}

pub fn terminal_cursor_position(
    area: Rect,
    input_text: &str,
    cursor_position: usize,
    secret: bool,
) -> (u16, u16) {
    let cursor_position = cursor_position.min(input_text.chars().count());
    let (line_idx, prefix) = cursor_line_prefix(input_text, cursor_position);
    let display_prefix = if secret {
        "•".repeat(prefix.chars().count())
    } else {
        prefix
    };
    let x = area
        .x
        .saturating_add(2)
        .saturating_add(display_width(&display_prefix) as u16)
        .min(area.x.saturating_add(area.width.saturating_sub(1)));
    let y = area
        .y
        .saturating_add(line_idx as u16)
        .min(area.y.saturating_add(area.height.saturating_sub(1)));
    (x, y)
}

fn cursor_line_prefix(text: &str, cursor_position: usize) -> (usize, String) {
    let mut line_idx = 0usize;
    let mut prefix = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx == cursor_position {
            break;
        }
        if ch == '\n' {
            line_idx += 1;
            prefix.clear();
        } else {
            prefix.push(ch);
        }
    }
    (line_idx, prefix)
}

fn mask_secret(input_text: &str) -> String {
    "•".repeat(input_text.chars().count())
}

fn prompt_span() -> Span<'static> {
    let p = theme::palette();
    Span::styled("› ", Style::default().fg(p.accent).bg(p.canvas))
}

fn input_style() -> Style {
    let p = theme::palette();
    Style::default().fg(p.text).bg(p.canvas)
}

fn muted_style() -> Style {
    let p = theme::palette();
    Style::default().fg(p.muted).bg(p.canvas)
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

fn char_display_width(ch: char) -> usize {
    match ch {
        '\u{1100}'..='\u{115F}'
        | '\u{2329}'..='\u{232A}'
        | '\u{2E80}'..='\u{A4CF}'
        | '\u{AC00}'..='\u{D7A3}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FE10}'..='\u{FE19}'
        | '\u{FE30}'..='\u{FE6F}'
        | '\u{FF00}'..='\u{FF60}'
        | '\u{FFE0}'..='\u{FFE6}' => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn empty_input_renders_just_the_prompt_chevron() {
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        terminal
            .draw(|f| render_input(f, f.area(), "", None))
            .expect("draw");

        let rendered = buffer_text(terminal.backend());
        assert!(rendered.starts_with("› "));
        assert!(!rendered.contains('▌'));
    }

    #[test]
    fn typed_input_renders_without_visible_cursor_glyph() {
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        terminal
            .draw(|f| render_input(f, f.area(), "ask", None))
            .expect("draw");

        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("› ask"));
        assert!(!rendered.contains('▌'));
    }

    #[test]
    fn mask_secret_keeps_length_without_leaking_text() {
        assert_eq!(mask_secret("sk-secret"), "•••••••••");
        assert_eq!(mask_secret("密钥"), "••");
    }

    #[test]
    fn api_key_input_empty_render_shows_setup_hint() {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        terminal
            .draw(|f| render_api_key_input(f, f.area(), ""))
            .expect("draw");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("› "));
        assert!(rendered.contains("paste API key"));
        assert!(!rendered.contains('▌'));
    }

    #[test]
    fn api_key_input_masks_secret_render() {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        terminal
            .draw(|f| render_api_key_input(f, f.area(), "sk-secret"))
            .expect("draw");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("•••••••••"));
        assert!(!rendered.contains("sk-secret"));
    }

    #[test]
    fn normal_input_uses_dark_ink_on_canvas_background() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        terminal
            .draw(|f| render_input(f, f.area(), "hello", None))
            .expect("draw");
        let cell = terminal.backend().buffer().cell((2, 0)).expect("cell");
        assert_eq!(cell.fg, theme::LIGHT_PALETTE.text);
        assert_eq!(cell.bg, theme::LIGHT_PALETTE.canvas);
    }

    #[test]
    fn multiline_input_continuation_uses_theme_colors() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("terminal");
        terminal
            .draw(|f| render_input(f, f.area(), "hello\nworld", None))
            .expect("draw");
        let cell = terminal.backend().buffer().cell((2, 1)).expect("cell");
        assert_eq!(cell.fg, theme::LIGHT_PALETTE.text);
        assert_eq!(cell.bg, theme::LIGHT_PALETTE.canvas);
    }

    #[test]
    fn api_key_hint_uses_muted_theme_colors() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        terminal
            .draw(|f| render_api_key_input(f, f.area(), ""))
            .expect("draw");
        let cell = terminal.backend().buffer().cell((4, 0)).expect("cell");
        assert!(
            (cell.fg == theme::LIGHT_PALETTE.muted && cell.bg == theme::LIGHT_PALETTE.canvas)
                || (cell.fg == theme::DARK_PALETTE.muted && cell.bg == theme::DARK_PALETTE.canvas)
        );
    }

    #[test]
    fn dark_theme_input_uses_dark_palette_text_on_canvas() {
        theme::set_active_theme(theme::ThemeMode::Dark);
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        terminal
            .draw(|f| render_input(f, f.area(), "hello", None))
            .expect("draw");
        let cell = terminal.backend().buffer().cell((2, 0)).expect("cell");
        assert_eq!(cell.fg, theme::DARK_PALETTE.text);
        assert_eq!(cell.bg, theme::DARK_PALETTE.canvas);
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn terminal_cursor_position_counts_cjk_display_width() {
        let area = Rect::new(10, 4, 40, 3);
        assert_eq!(
            terminal_cursor_position(area, "是现在", 3, false),
            (10 + 2 + 6, 4)
        );
    }

    #[test]
    fn terminal_cursor_position_tracks_current_multiline_prefix() {
        let area = Rect::new(3, 7, 40, 3);
        assert_eq!(
            terminal_cursor_position(area, "hello\n是啊", 8, false),
            (3 + 2 + 4, 8)
        );
    }

    #[test]
    fn terminal_cursor_position_masks_api_key_width() {
        let area = Rect::new(0, 0, 40, 1);
        assert_eq!(terminal_cursor_position(area, "密钥", 2, true), (4, 0));
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

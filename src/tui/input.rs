use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::tui::{motion::MotionFrame, theme};

pub struct InputRenderOptions<'a> {
    pub pending_options: Option<&'a [String]>,
    pub motion: MotionFrame,
    pub placeholder: &'a str,
    pub chinese: bool,
}

/// Quiet terminal input: no border, just a clean prompt.
pub fn render_input(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_position: usize,
    pending_options: Option<&[String]>,
) {
    render_input_with_motion(
        f,
        area,
        input_text,
        cursor_position,
        pending_options,
        MotionFrame::disabled(),
    );
}

pub fn render_input_with_motion(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_position: usize,
    pending_options: Option<&[String]>,
    motion: MotionFrame,
) {
    render_input_with_motion_and_placeholder(
        f,
        area,
        input_text,
        cursor_position,
        pending_options,
        motion,
        &input_placeholder(),
    );
}

pub fn render_input_with_motion_and_placeholder(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_position: usize,
    pending_options: Option<&[String]>,
    motion: MotionFrame,
    placeholder: &str,
) {
    render_input_with_options(
        f,
        area,
        input_text,
        cursor_position,
        InputRenderOptions {
            pending_options,
            motion,
            placeholder,
            chinese: false,
        },
    );
}

pub fn render_input_with_options(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_position: usize,
    options: InputRenderOptions<'_>,
) {
    let lines: Vec<Line> = if input_text.is_empty() {
        vec![Line::from(vec![
            prompt_span(),
            cursor_span(options.motion),
            Span::styled("  ", muted_style()),
            Span::styled(options.placeholder.to_string(), muted_style()),
            render_context_suggestions(options.pending_options, options.chinese),
        ])]
    } else {
        let mut line_start = 0usize;
        input_text
            .split('\n')
            .enumerate()
            .map(|(i, line)| {
                let line_len = line.chars().count();
                let cursor_in_line =
                    cursor_position >= line_start && cursor_position <= line_start + line_len;
                let local_cursor =
                    cursor_in_line.then_some(cursor_position.saturating_sub(line_start));
                let mut spans = Vec::new();
                if i == 0 {
                    spans.push(prompt_span());
                } else {
                    spans.push(Span::styled("  ", input_style()));
                }
                spans.extend(edit_spans(line, local_cursor, options.motion));
                line_start += line_len + 1;
                Line::from(spans)
            })
            .collect()
    };

    let p = theme::palette();
    let input = Paragraph::new(lines).style(Style::default().bg(p.canvas).fg(p.text));
    f.render_widget(input, area);
}

fn input_placeholder() -> String {
    String::new()
}

fn render_context_suggestions(pending_options: Option<&[String]>, chinese: bool) -> Span<'static> {
    if pending_options.is_some() {
        let opts = pending_options.unwrap_or(&[]);
        if opts.is_empty() {
            return Span::styled(
                if chinese {
                    "  (选项列表已激活)"
                } else {
                    "  (option list active)"
                },
                muted_style().add_modifier(Modifier::ITALIC),
            );
        }
        let shown = opts.iter().take(3).cloned().collect::<Vec<_>>().join(" · ");
        let mut label = format!("  {shown}");
        if opts.len() > 3 {
            label.push_str(&format!(" (+{})", opts.len() - 3));
        }
        return Span::styled(
            format!("  {label}"),
            muted_style().add_modifier(Modifier::ITALIC),
        );
    }

    let _ = chinese;
    Span::styled("", muted_style().add_modifier(Modifier::ITALIC))
}

pub fn render_api_key_input(f: &mut Frame, area: Rect, input_text: &str, cursor_position: usize) {
    render_api_key_input_with_motion(
        f,
        area,
        input_text,
        cursor_position,
        MotionFrame::disabled(),
    );
}

pub fn render_api_key_input_with_motion(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_position: usize,
    motion: MotionFrame,
) {
    render_api_key_input_with_motion_and_placeholder(
        f,
        area,
        input_text,
        cursor_position,
        motion,
        "paste API key",
    );
}

pub fn render_api_key_input_with_motion_and_placeholder(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_position: usize,
    motion: MotionFrame,
    placeholder: &str,
) {
    let display = mask_secret(input_text);
    let cursor = cursor_position.min(display.chars().count());
    let lines = if display.is_empty() {
        vec![Line::from(vec![
            prompt_span(),
            cursor_span(motion),
            Span::styled(" ", input_style()),
            Span::styled(placeholder.to_string(), muted_style()),
        ])]
    } else {
        vec![Line::from({
            let mut spans = vec![prompt_span()];
            spans.extend(edit_spans(&display, Some(cursor), motion));
            spans
        })]
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
    Span::styled(
        "> ",
        Style::default()
            .fg(p.accent)
            .bg(p.canvas)
            .add_modifier(Modifier::BOLD),
    )
}

fn input_style() -> Style {
    let p = theme::palette();
    Style::default().fg(p.text).bg(p.canvas)
}

fn muted_style() -> Style {
    let p = theme::palette();
    Style::default().fg(p.muted).bg(p.canvas)
}

fn edit_spans(line: &str, cursor: Option<usize>, motion: MotionFrame) -> Vec<Span<'static>> {
    let Some(cursor) = cursor else {
        return vec![Span::styled(line.to_string(), input_style())];
    };

    let mut spans = Vec::new();
    let before: String = line.chars().take(cursor).collect();
    let at = line.chars().nth(cursor);
    let after: String = line
        .chars()
        .skip(cursor + usize::from(at.is_some()))
        .collect();

    if !before.is_empty() {
        spans.push(Span::styled(before, input_style()));
    }

    spans.push(cursor_span(motion));

    if let Some(ch) = at {
        spans.push(Span::styled(ch.to_string(), input_style()));
    }

    if !after.is_empty() {
        spans.push(Span::styled(after, input_style()));
    }

    spans
}

fn cursor_span(motion: MotionFrame) -> Span<'static> {
    Span::styled(
        motion.cursor(),
        Style::default()
            .fg(theme::palette().accent)
            .bg(theme::palette().canvas),
    )
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
    fn empty_normal_input_draws_visible_cursor() {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        terminal
            .draw(|f| render_input(f, f.area(), "", 0, None))
            .expect("draw");

        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("> ▌"));
        assert!(!rendered.contains("type message"));
        assert!(!rendered.contains("/help"));
    }

    #[test]
    fn empty_input_with_pending_options_shows_option_preview() {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        let options: Vec<String> = ["help", "agents", "model", "context"]
            .into_iter()
            .map(String::from)
            .collect();
        terminal
            .draw(|f| render_input(f, f.area(), "", 0, Some(&options)))
            .expect("draw");

        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("help · agents · model"));
        assert!(rendered.contains("(+1)"));
    }

    #[test]
    fn typed_input_uses_inline_cursor_marker() {
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        terminal
            .draw(|f| render_input(f, f.area(), "ask", 3, None))
            .expect("draw");

        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("> ask▌"));
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
            .draw(|f| render_api_key_input(f, f.area(), "", 0))
            .expect("draw");
        let rendered = buffer_text(terminal.backend());
        assert!(rendered.contains("> ▌"));
        assert!(rendered.contains("paste API key"));
    }

    #[test]
    fn api_key_input_masks_secret_render() {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        terminal
            .draw(|f| render_api_key_input(f, f.area(), "sk-secret", 9))
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
            .draw(|f| render_input(f, f.area(), "hello", 5, None))
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
            .draw(|f| render_input(f, f.area(), "hello\nworld", 11, None))
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
            .draw(|f| render_api_key_input(f, f.area(), "", 0))
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
            .draw(|f| render_input(f, f.area(), "hello", 5, None))
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

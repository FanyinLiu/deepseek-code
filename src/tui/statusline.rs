use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::CacheUsage;
use crate::tui::{status_bar::AppMode, theme};

const CONTEXT_LIMIT_TOKENS: u64 = 1_000_000;

pub struct StatuslineProps<'a> {
    pub mode: AppMode,
    pub status: &'a str,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: f64,
    pub cache: Option<&'a CacheUsage>,
    pub permissions: &'a str,
}

pub fn render_statusline(f: &mut Frame, area: Rect, props: StatuslineProps<'_>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let divider = "─".repeat(area.width as usize);
    let lines = vec![
        Line::from(Span::styled(
            divider,
            Style::default().fg(p.divider).bg(p.canvas),
        )),
        Line::from(statusline_row(&props, p.canvas, area.width)),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().fg(p.text).bg(p.canvas)),
        area,
    );
}

fn statusline_row(props: &StatuslineProps<'_>, canvas: Color, width: u16) -> Vec<Span<'static>> {
    let colors = statusline_colors();
    let compact = width < 112;
    let narrow = width < 88;
    let mut spans = vec![Span::styled("  ", Style::default().bg(canvas))];
    push_chip(
        &mut spans,
        " ds-code ".to_string(),
        colors.project_bg,
        colors.project_fg,
    );
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        format!(" {} ", props.mode.label()),
        colors.mode_bg,
        colors.dark_fg,
    );
    if !narrow {
        push_gap(&mut spans, canvas);
        push_chip(
            &mut spans,
            " web:on ".to_string(),
            colors.web_bg,
            colors.dark_fg,
        );
    }
    if props.input_tokens > 0 || props.output_tokens > 0 {
        push_gap(&mut spans, canvas);
        push_chip(
            &mut spans,
            format!(" in {} ", compact_number(props.input_tokens)),
            colors.input_bg,
            colors.dark_fg,
        );
        push_gap(&mut spans, canvas);
        push_chip(
            &mut spans,
            format!(" tok {} ", compact_number(props.output_tokens)),
            colors.tokens_bg,
            colors.dark_fg,
        );
    } else {
        push_gap(&mut spans, canvas);
        push_chip(
            &mut spans,
            format!(" tok {} ", compact_number(props.tokens)),
            colors.tokens_bg,
            colors.dark_fg,
        );
    }
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        format!(" ¥{:.3} ", props.cost),
        colors.cost_bg,
        colors.dark_fg,
    );
    if let Some(cache) = props.cache.filter(|_| !compact) {
        push_gap(&mut spans, canvas);
        push_chip(
            &mut spans,
            format!(" cache {:.0}% ", cache.hit_rate() * 100.0),
            colors.cache_bg,
            colors.dark_fg,
        );
    }
    if !narrow {
        push_gap(&mut spans, canvas);
        push_chip(
            &mut spans,
            " tools ✓ ".to_string(),
            colors.tools_bg,
            colors.dark_fg,
        );
    }
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        format!(" {} ", compact_permissions(props.permissions)),
        colors.permissions_bg,
        colors.light_fg,
    );
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        context_segment_for_width(props.tokens, width),
        colors.ctx_bg,
        colors.dark_fg,
    );
    spans
}

fn context_segment_for_width(tokens: u64, width: u16) -> String {
    if width >= 112 {
        context_segment(tokens)
    } else if width >= 88 {
        compact_context_segment(tokens)
    } else {
        tiny_context_segment(tokens)
    }
}

fn context_segment(tokens: u64) -> String {
    let ratio = (tokens as f64 / CONTEXT_LIMIT_TOKENS as f64).clamp(0.0, 1.0);
    format!(
        " {}/{} ({:.1}%) ",
        compact_number(tokens),
        context_limit_label(),
        ratio * 100.0,
    )
}

fn compact_context_segment(tokens: u64) -> String {
    let ratio = (tokens as f64 / CONTEXT_LIMIT_TOKENS as f64).clamp(0.0, 1.0);
    format!(
        " {}/{} ({:.1}%) ",
        compact_number(tokens),
        context_limit_label(),
        ratio * 100.0,
    )
}

fn tiny_context_segment(tokens: u64) -> String {
    let ratio = (tokens as f64 / CONTEXT_LIMIT_TOKENS as f64).clamp(0.0, 1.0);
    format!(
        " {}/{} {:.0}% ",
        compact_number(tokens),
        context_limit_label(),
        ratio * 100.0
    )
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn context_limit_label() -> &'static str {
    "1M"
}

fn compact_permissions(permissions: &str) -> &'static str {
    if permissions.contains("bypass") {
        "bypass"
    } else {
        "ask"
    }
}

fn push_chip(spans: &mut Vec<Span<'static>>, label: String, bg: Color, fg: Color) {
    spans.push(Span::styled(
        label,
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    ));
}

fn push_gap(spans: &mut Vec<Span<'static>>, canvas: Color) {
    spans.push(Span::styled(" ", Style::default().bg(canvas)));
}

#[derive(Clone, Copy)]
struct StatuslineColors {
    project_bg: Color,
    project_fg: Color,
    ctx_bg: Color,
    input_bg: Color,
    mode_bg: Color,
    web_bg: Color,
    tokens_bg: Color,
    cost_bg: Color,
    cache_bg: Color,
    tools_bg: Color,
    permissions_bg: Color,
    dark_fg: Color,
    light_fg: Color,
}

fn statusline_colors() -> StatuslineColors {
    StatuslineColors {
        project_bg: Color::Rgb(36, 38, 42),
        project_fg: Color::Rgb(230, 230, 220),
        ctx_bg: Color::Rgb(244, 206, 22),
        input_bg: Color::Rgb(255, 184, 77),
        mode_bg: Color::Rgb(87, 142, 214),
        web_bg: Color::Rgb(118, 184, 124),
        tokens_bg: Color::Rgb(102, 204, 204),
        cost_bg: Color::Rgb(188, 139, 216),
        cache_bg: Color::Rgb(144, 184, 104),
        tools_bg: Color::Rgb(111, 191, 113),
        permissions_bg: Color::Rgb(198, 72, 82),
        dark_fg: Color::Rgb(20, 20, 18),
        light_fg: Color::Rgb(255, 244, 230),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn statusline_renders_colored_metadata_chips() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Chat,
                        status: "ready",
                        tokens: 128,
                        input_tokens: 0,
                        output_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
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
        assert!(!rendered.contains("Model:"));
        assert!(rendered.contains("ds-code"));
        assert!(rendered.contains("chat"));
        assert!(rendered.contains("128/1M (0.0%)"));
        assert!(rendered.contains("tok 128"));
        assert!(rendered.contains("ask"));
        let chip = terminal.backend().buffer().cell((2, 1)).expect("chip");
        assert_ne!(chip.bg, theme::palette().canvas);
    }

    #[test]
    fn context_segment_has_progress_and_compact_limit() {
        let segment = context_segment(126_300);

        assert_eq!(segment.trim(), "126.3k/1M (12.6%)");
    }

    #[test]
    fn statusline_keeps_permission_visible_when_compact() {
        let mut terminal = Terminal::new(TestBackend::new(96, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Chat,
                        status: "ready",
                        tokens: 128,
                        input_tokens: 0,
                        output_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
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
        assert!(rendered.contains("ask"));
        assert!(rendered.contains("128/1M (0.0%)"));
    }

    #[test]
    fn statusline_renders_live_input_and_output_tokens() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Run,
                        status: "working",
                        tokens: 5_700,
                        input_tokens: 742,
                        output_tokens: 131,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
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
        assert!(rendered.contains("in 742"));
        assert!(rendered.contains("tok 131"));
        assert!(!rendered.contains("out 131"));
    }
}

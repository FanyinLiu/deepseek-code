use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::{CacheUsage, DeepSeekModel};
use crate::tui::{status_bar::AppMode, theme};

const CONTEXT_LIMIT_TOKENS: u64 = 1_000_000;

pub struct StatuslineProps<'a> {
    pub mode: AppMode,
    pub model: &'a DeepSeekModel,
    pub status: &'a str,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub agent_tokens: u64,
    pub cost: f64,
    pub cache: Option<&'a CacheUsage>,
    pub permissions: &'a str,
    pub context_limit: Option<u64>,
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
    let colors = statusline_colors(theme::palette());
    let narrow = width < 88;
    let mut spans = vec![Span::styled("  ", Style::default().bg(canvas))];
    let context_limit = props.context_limit.unwrap_or(CONTEXT_LIMIT_TOKENS);
    push_chip(
        &mut spans,
        " octocode ".to_string(),
        colors.project_bg,
        colors.project_fg,
    );
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        format!(" {} ", props.mode.label()),
        colors.mode_bg,
        colors.mode_fg,
    );
    if !narrow {
        push_gap(&mut spans, canvas);
        push_chip(
            &mut spans,
            format!(" {} ", compact_model_label(props.model)),
            colors.model_bg,
            colors.model_fg,
        );
    }
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        format!(" {} ", props.status),
        colors.ctx_bg,
        colors.ctx_fg,
    );
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        format!(" {} ", compact_permissions(props.permissions)),
        colors.permissions_bg,
        colors.permissions_fg,
    );
    push_gap(&mut spans, canvas);
    push_chip(
        &mut spans,
        context_segment_for_width(props.tokens, width, context_limit),
        colors.ctx_bg,
        colors.ctx_fg,
    );
    spans
}

fn compact_model_label(model: &DeepSeekModel) -> &'static str {
    match model {
        DeepSeekModel::Pro => "V4 Pro",
        DeepSeekModel::Flash => "V4 Flash",
        DeepSeekModel::LegacyChat => "Chat",
        DeepSeekModel::LegacyReasoner => "Reasoner",
    }
}

fn context_segment_for_width(tokens: u64, width: u16, context_limit: u64) -> String {
    if width >= 112 {
        context_segment(tokens, context_limit)
    } else if width >= 88 {
        compact_context_segment(tokens, context_limit)
    } else {
        tiny_context_segment(tokens, context_limit)
    }
}

fn context_segment(tokens: u64, context_limit: u64) -> String {
    let limit = context_limit.max(1);
    let ratio = (tokens as f64 / limit as f64).clamp(0.0, 1.0);
    format!(
        " {}/{} ({:.1}%) ",
        compact_number(tokens),
        context_limit_label(context_limit),
        ratio * 100.0,
    )
}

fn compact_context_segment(tokens: u64, context_limit: u64) -> String {
    let limit = context_limit.max(1);
    let ratio = (tokens as f64 / limit as f64).clamp(0.0, 1.0);
    format!(
        " {}/{} ({:.1}%) ",
        compact_number(tokens),
        context_limit_label(context_limit),
        ratio * 100.0,
    )
}

fn tiny_context_segment(tokens: u64, context_limit: u64) -> String {
    let limit = context_limit.max(1);
    let ratio = (tokens as f64 / limit as f64).clamp(0.0, 1.0);
    format!(
        " {}/{} {:.0}% ",
        compact_number(tokens),
        context_limit_label(context_limit),
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

fn context_limit_label(context_limit: u64) -> &'static str {
    if context_limit >= 1_000_000 {
        "1M"
    } else if context_limit >= 1_000 {
        "1K"
    } else {
        "tok"
    }
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
    ctx_fg: Color,
    model_bg: Color,
    model_fg: Color,
    mode_bg: Color,
    mode_fg: Color,
    permissions_bg: Color,
    permissions_fg: Color,
}

fn statusline_colors(p: theme::ThemePalette) -> StatuslineColors {
    StatuslineColors {
        project_bg: p.accent,
        project_fg: p.inverse_text,
        ctx_bg: p.surface_alt,
        ctx_fg: p.text,
        model_bg: p.info,
        model_fg: p.inverse_text,
        mode_bg: p.surface_alt,
        mode_fg: p.text,
        permissions_bg: p.danger,
        permissions_fg: p.inverse_text,
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
                        model: &DeepSeekModel::Pro,
                        status: "ready",
                        tokens: 128,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
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
        assert!(rendered.contains("octocode"));
        assert!(rendered.contains("chat"));
        assert!(rendered.contains("V4 Pro"));
        assert!(rendered.contains("128/1M (0.0%)"));
        assert!(!rendered.contains("tok "));
        assert!(!rendered.contains("↑"));
        assert!(!rendered.contains("↓"));
        assert!(!rendered.contains("web:on"));
        assert!(!rendered.contains("¥"));
        assert!(rendered.contains("ask"));
        let chip = terminal.backend().buffer().cell((2, 1)).expect("chip");
        assert_ne!(chip.bg, theme::palette().canvas);
    }

    #[test]
    fn context_segment_has_progress_and_compact_limit() {
        let segment = context_segment(126_300, 1_000_000);

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
                        model: &DeepSeekModel::Flash,
                        status: "ready",
                        tokens: 128,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
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
        assert!(rendered.contains("V4 Flash"));
        assert!(rendered.contains("128/1M (0.0%)"));
    }

    #[test]
    fn statusline_keeps_live_tokens_out_of_the_footer() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Run,
                        model: &DeepSeekModel::Flash,
                        status: "working",
                        tokens: 5_700,
                        input_tokens: 742,
                        output_tokens: 131,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
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
        assert!(rendered.contains("5.7k/1M"));
        assert!(!rendered.contains("↑ 742 tokens"));
        assert!(!rendered.contains("↓ 131 tokens"));
        assert!(!rendered.contains("tok 131"));
    }

    #[test]
    fn statusline_omits_token_directions() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Run,
                        model: &DeepSeekModel::Flash,
                        status: "working",
                        tokens: 5_700,
                        input_tokens: 54,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
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
        assert!(rendered.contains("5.7k/1M"));
        assert!(!rendered.contains("↑ 54 tokens"));
        assert!(!rendered.contains("↓ 0"));
        assert!(!rendered.contains("tok "));
    }

    #[test]
    fn statusline_hides_token_chip_until_usage_exists() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Chat,
                        model: &DeepSeekModel::Flash,
                        status: "ready",
                        tokens: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.0,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
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
        assert!(!rendered.contains("tok "));
        assert!(!rendered.contains("tokens"));
        assert!(!rendered.contains("↑"));
        assert!(!rendered.contains("↓"));
    }

    #[test]
    fn statusline_renders_agent_tokens_separately() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Run,
                        model: &DeepSeekModel::Flash,
                        status: "working",
                        tokens: 16_160,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 16_160,
                        cost: 0.0,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
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
        assert!(rendered.contains("16.2k/1M"));
        assert!(!rendered.contains("agent 16.2k tokens"));
        assert!(!rendered.contains("↓ 16.2k"));
    }
}

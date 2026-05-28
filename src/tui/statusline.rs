use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::CacheUsage;
use crate::tui::{status_bar::AppMode, theme};

const DEFAULT_CONTEXT_BUDGET_TOKENS: u64 = 12_000;

pub struct StatuslineProps<'a> {
    pub mode: AppMode,
    pub provider: &'a str,
    pub model: &'a str,
    pub status: &'a str,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub agent_tokens: u64,
    pub cost: f64,
    pub cache: Option<&'a CacheUsage>,
    pub permissions: &'a str,
    pub context_limit: Option<u64>,
    pub chinese: bool,
}

pub fn render_statusline(f: &mut Frame, area: Rect, props: StatuslineProps<'_>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let line_area = Rect {
        y: area.y + area.height.saturating_sub(1),
        height: 1,
        ..area
    };
    f.render_widget(
        Paragraph::new(Line::from(statusline_row(&props, p.canvas, area.width)))
            .style(Style::default().fg(p.text).bg(p.canvas)),
        line_area,
    );
}

fn statusline_row(props: &StatuslineProps<'_>, canvas: Color, width: u16) -> Vec<Span<'static>> {
    let colors = statusline_colors(theme::palette());
    let mut spans = Vec::new();
    let context_limit = props.context_limit.unwrap_or(DEFAULT_CONTEXT_BUDGET_TOKENS);
    let narrow = width < 88;

    push_label(
        &mut spans,
        label(props.chinese, "Context", "上下文"),
        colors.label,
    );
    push_text(
        &mut spans,
        context_value_for_width(props.tokens, width, context_limit),
        colors.text,
    );
    push_sep(&mut spans, canvas, colors.sep);
    push_label(
        &mut spans,
        label(props.chinese, "Mode", "模式"),
        colors.label,
    );
    push_text(&mut spans, props.mode.label().to_string(), colors.text);
    if !narrow {
        push_sep(&mut spans, canvas, colors.sep);
        push_label(
            &mut spans,
            label(props.chinese, "Model", "模型"),
            colors.label,
        );
        push_text(
            &mut spans,
            model_value(props.provider, props.model, width),
            colors.text,
        );
    }
    if !narrow {
        if let Some(cache) = props.cache {
            if cache.prompt_cache_hit_tokens + cache.prompt_cache_miss_tokens > 0 {
                push_sep(&mut spans, canvas, colors.sep);
                push_label(
                    &mut spans,
                    label(props.chinese, "Hit", "命中率"),
                    colors.label,
                );
                let rate = cache.hit_rate() * 100.0;
                let fg = if rate >= 80.0 {
                    colors.permission
                } else if rate >= 50.0 {
                    colors.text
                } else {
                    colors.sep
                };
                push_text(&mut spans, format!("{:.0}%", rate), fg);
            }
        }
    }
    push_sep(&mut spans, canvas, colors.sep);
    push_label(
        &mut spans,
        label(props.chinese, "State", "状态"),
        colors.label,
    );
    push_text(&mut spans, props.status.to_string(), colors.text);
    push_sep(&mut spans, canvas, colors.sep);
    push_label(
        &mut spans,
        label(props.chinese, "Permissions", "权限"),
        colors.label,
    );
    push_text(
        &mut spans,
        compact_permissions(props.permissions).to_string(),
        colors.permission,
    );
    spans
}

fn label(chinese: bool, en: &'static str, zh: &'static str) -> &'static str {
    if chinese {
        zh
    } else {
        en
    }
}

fn model_value(provider: &str, model: &str, width: u16) -> String {
    let model = if width < 112 {
        compact_model_name(model)
    } else {
        model.to_string()
    };
    let provider = if width < 112 {
        compact_provider_name(provider)
    } else {
        provider.to_string()
    };
    if provider.is_empty() {
        model
    } else {
        format!("{provider}/{model}")
    }
}

fn compact_model_name(model: &str) -> String {
    model
        .strip_prefix("deepseek-v4-")
        .or_else(|| model.strip_prefix("deepseek-"))
        .or_else(|| model.strip_prefix("qwen-"))
        .or_else(|| model.strip_prefix("kimi-"))
        .unwrap_or(model)
        .to_string()
}

fn compact_provider_name(provider: &str) -> String {
    match provider {
        "deepseek" => "ds",
        "openai-compatible" => "oai",
        "openrouter" => "or",
        other => other,
    }
    .to_string()
}

fn context_value_for_width(tokens: u64, width: u16, context_limit: u64) -> String {
    if width >= 112 {
        context_value(tokens, context_limit)
    } else if width >= 88 {
        compact_context_value(tokens, context_limit)
    } else {
        tiny_context_value(tokens, context_limit)
    }
}

fn context_value(tokens: u64, context_limit: u64) -> String {
    let limit = context_limit.max(1);
    let ratio = (tokens as f64 / limit as f64).clamp(0.0, 1.0);
    format!(
        "{}/{} ({:.1}%)",
        compact_number(tokens),
        context_limit_label(context_limit),
        ratio * 100.0,
    )
}

fn compact_context_value(tokens: u64, context_limit: u64) -> String {
    let limit = context_limit.max(1);
    let ratio = (tokens as f64 / limit as f64).clamp(0.0, 1.0);
    format!(
        "{}/{} ({:.1}%)",
        compact_number(tokens),
        context_limit_label(context_limit),
        ratio * 100.0,
    )
}

fn tiny_context_value(tokens: u64, context_limit: u64) -> String {
    let limit = context_limit.max(1);
    let ratio = (tokens as f64 / limit as f64).clamp(0.0, 1.0);
    format!(
        "{}/{} {:.0}%",
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

fn context_limit_label(context_limit: u64) -> String {
    if context_limit >= 1_000_000 {
        compact_scaled_label(context_limit, 1_000_000, "M")
    } else if context_limit >= 1_000 {
        compact_scaled_label(context_limit, 1_000, "K")
    } else {
        format!("{}tok", context_limit.max(1))
    }
}

fn compact_scaled_label(value: u64, unit: u64, suffix: &str) -> String {
    if value.is_multiple_of(unit) {
        format!("{}{suffix}", value / unit)
    } else {
        format!("{:.1}{suffix}", value as f64 / unit as f64)
    }
}

fn compact_permissions(permissions: &str) -> &'static str {
    // Match the CLI vocabulary (`--tool-approval ask`) and avoid the
    // imperative "confirm" — the system is asking, not commanding.
    if permissions.contains("bypass") {
        "auto"
    } else {
        "ask"
    }
}

fn push_label(spans: &mut Vec<Span<'static>>, label: &'static str, fg: Color) {
    spans.push(Span::styled(
        format!("{label} "),
        Style::default().fg(fg).bg(theme::palette().canvas),
    ));
}

fn push_text(spans: &mut Vec<Span<'static>>, text: String, fg: Color) {
    spans.push(Span::styled(
        text,
        Style::default().fg(fg).bg(theme::palette().canvas),
    ));
}

fn push_sep(spans: &mut Vec<Span<'static>>, canvas: Color, fg: Color) {
    spans.push(Span::styled("  ·  ", Style::default().fg(fg).bg(canvas)));
}

#[derive(Clone, Copy)]
struct StatuslineColors {
    label: Color,
    text: Color,
    sep: Color,
    permission: Color,
}

fn statusline_colors(p: theme::ThemePalette) -> StatuslineColors {
    StatuslineColors {
        label: p.assistant,
        text: p.text,
        sep: p.muted,
        permission: p.warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn statusline_renders_plain_metadata() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Chat,
                        provider: "deepseek",
                        model: "deepseek-v4-pro",
                        status: "ready",
                        tokens: 128,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: false,
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
        assert!(!rendered.contains("octo"));
        assert!(rendered.contains("Mode chat"));
        assert!(rendered.contains("Model deepseek/deepseek-v4-pro"));
        assert!(rendered.contains("Context 128/12K (1.1%)"));
        assert!(!rendered.contains("tok "));
        assert!(!rendered.contains("↑"));
        assert!(!rendered.contains("↓"));
        assert!(!rendered.contains("web:on"));
        assert!(!rendered.contains("¥"));
        assert!(rendered.contains("ask"));
        let status_cell = terminal.backend().buffer().cell((2, 1)).expect("cell");
        assert_eq!(status_cell.bg, theme::palette().canvas);
    }

    #[test]
    fn context_segment_has_progress_and_compact_limit() {
        let segment = context_value(126_300, 1_000_000);

        assert_eq!(segment.trim(), "126.3k/1M (12.6%)");

        let segment = context_value(1_200, 12_000);
        assert_eq!(segment.trim(), "1.2k/12K (10.0%)");
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
                        provider: "deepseek",
                        model: "deepseek-v4-flash",
                        status: "ready",
                        tokens: 128,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: false,
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
        assert!(rendered.contains("ds/flash"));
        assert!(rendered.contains("Context 128/12K (1.1%)"));
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
                        provider: "deepseek",
                        model: "deepseek-v4-flash",
                        status: "working",
                        tokens: 5_700,
                        input_tokens: 742,
                        output_tokens: 131,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: false,
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
        assert!(rendered.contains("Context 5.7k/12K"));
        assert!(!rendered.contains("↑ 742 tokens"));
        assert!(!rendered.contains("↓ 131 tokens"));
        assert!(!rendered.contains("tok 131"));
    }

    #[test]
    fn statusline_renders_cache_hit_rate_when_available() {
        let mut terminal = Terminal::new(TestBackend::new(120, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Run,
                        provider: "deepseek",
                        model: "deepseek-v4-flash",
                        status: "working",
                        tokens: 5_700,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: Some(&CacheUsage {
                            prompt_cache_hit_tokens: 800,
                            prompt_cache_miss_tokens: 200,
                        }),
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: false,
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
        assert!(rendered.contains("Hit 80%"));
    }

    #[test]
    fn statusline_localizes_cache_hit_rate_label() {
        let mut terminal = Terminal::new(TestBackend::new(160, 2)).expect("terminal");
        terminal
            .draw(|f| {
                render_statusline(
                    f,
                    f.area(),
                    StatuslineProps {
                        mode: AppMode::Run,
                        provider: "deepseek",
                        model: "deepseek-v4-flash",
                        status: "working",
                        tokens: 5_700,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: Some(&CacheUsage {
                            prompt_cache_hit_tokens: 900,
                            prompt_cache_miss_tokens: 100,
                        }),
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: true,
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
        assert!(rendered.contains("命 中 率"));
        assert!(rendered.contains("90%"));
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
                        provider: "deepseek",
                        model: "deepseek-v4-flash",
                        status: "working",
                        tokens: 5_700,
                        input_tokens: 54,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.001,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: false,
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
        assert!(rendered.contains("Context 5.7k/12K"));
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
                        provider: "deepseek",
                        model: "deepseek-v4-flash",
                        status: "ready",
                        tokens: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 0,
                        cost: 0.0,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: false,
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
                        provider: "deepseek",
                        model: "deepseek-v4-flash",
                        status: "working",
                        tokens: 16_160,
                        input_tokens: 0,
                        output_tokens: 0,
                        agent_tokens: 16_160,
                        cost: 0.0,
                        cache: None,
                        permissions: "permissions ask",
                        context_limit: None,
                        chinese: false,
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
        assert!(rendered.contains("Context 16.2k/12K"));
        assert!(!rendered.contains("agent 16.2k tokens"));
        assert!(!rendered.contains("↓ 16.2k"));
    }
}

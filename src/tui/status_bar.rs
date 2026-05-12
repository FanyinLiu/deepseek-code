use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::ThinkingMode;
use crate::tui::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Chat,
    Plan,
    Run,
    Review,
}

impl AppMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Plan => "plan",
            Self::Run => "run",
            Self::Review => "review",
        }
    }

    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Chat => theme::DROID_ACCENT,
            Self::Plan => theme::ACCENT_BLUE,
            Self::Run => theme::ACCENT_GREEN,
            Self::Review => theme::ACCENT_PURPLE,
        }
    }
}

pub struct StatusBarProps<'a> {
    pub mode: AppMode,
    pub thinking: &'a ThinkingMode,
    pub activity: Option<StatusActivity<'a>>,
}

pub struct StatusActivity<'a> {
    pub title: &'a str,
    pub elapsed_ms: u64,
    pub input_tokens: u64,
    pub tokens: u64,
    pub agent_tokens: u64,
    pub thought_seconds: u64,
}

pub(crate) fn activity_hint_text(activity: &StatusActivity<'_>, thinking: &ThinkingMode) -> String {
    let spinner = activity_spinner(activity.elapsed_ms);
    let title = stable_activity_title(activity.title);
    let elapsed = format_elapsed(activity.elapsed_ms / 1_000);
    let effort = thinking_effort_label(thinking);

    if activity.input_tokens > 0 || activity.tokens > 0 || activity.agent_tokens > 0 {
        let token_label = activity_token_label(
            activity.input_tokens,
            activity.tokens,
            activity.agent_tokens,
        );
        format!("{spinner} {title}... ({elapsed} · {token_label} · thinking with {effort} effort)")
    } else {
        format!("{spinner} {title}... ({elapsed} · thinking with {effort} effort)")
    }
}

fn activity_token_label(input_tokens: u64, output_tokens: u64, agent_tokens: u64) -> String {
    let mut parts = Vec::new();
    if input_tokens > 0 {
        parts.push(format!("↑ {}", token_count_label(input_tokens)));
    }
    if output_tokens > 0 {
        parts.push(format!("↓ {}", token_count_label(output_tokens)));
    }
    if agent_tokens > 0 {
        parts.push(format!("agent {}", token_count_label(agent_tokens)));
    }
    parts.join(" · ")
}

fn token_count_label(tokens: u64) -> String {
    let unit = if tokens == 1 { "token" } else { "tokens" };
    format!("{} {unit}", compact_number(tokens))
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

fn stable_activity_title(title: &str) -> &str {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        "Working"
    } else {
        trimmed
    }
}

fn format_elapsed(elapsed_seconds: u64) -> String {
    let hours = elapsed_seconds / 3600;
    let minutes = (elapsed_seconds % 3600) / 60;
    let seconds = elapsed_seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn activity_spinner(elapsed_ms: u64) -> &'static str {
    const GLYPHS: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let idx = ((elapsed_ms / 80) as usize) % GLYPHS.len();
    GLYPHS[idx]
}

fn thinking_effort_label(thinking: &ThinkingMode) -> &'static str {
    match thinking {
        ThinkingMode::Auto => "auto",
        ThinkingMode::On => "high",
        ThinkingMode::Off => "no",
    }
}

/// Quiet activity line. Idle state intentionally renders blank.
pub fn render_status_bar(f: &mut Frame, area: Rect, props: StatusBarProps<'_>) {
    let p = theme::palette();
    let Some(activity) = props.activity else {
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(p.canvas)),
            area,
        );
        return;
    };

    let line = Line::from(vec![Span::styled(
        activity_hint_text(&activity, props.thinking),
        Style::default()
            .fg(props.mode.color())
            .bg(p.canvas)
            .add_modifier(Modifier::BOLD),
    )]);

    let status = Paragraph::new(line)
        .style(Style::default().bg(p.canvas))
        .alignment(Alignment::Left);

    f.render_widget(status, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_hint_uses_english_task_title() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 6_000,
                input_tokens: 42,
                tokens: 578,
                agent_tokens: 0,
                thought_seconds: 2,
            },
            &ThinkingMode::Auto,
        );

        assert_eq!(
            hint,
            "⠴ Fix input colors... (6s · ↑ 42 tokens · ↓ 578 tokens · thinking with auto effort)"
        );
    }

    #[test]
    fn activity_hint_uses_chinese_task_title() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "修复输入颜色",
                elapsed_ms: 6_000,
                input_tokens: 42,
                tokens: 578,
                agent_tokens: 0,
                thought_seconds: 2,
            },
            &ThinkingMode::Auto,
        );

        assert_eq!(
            hint,
            "⠴ 修复输入颜色... (6s · ↑ 42 tokens · ↓ 578 tokens · thinking with auto effort)"
        );
    }

    #[test]
    fn elapsed_time_compacts_into_minutes_and_hours() {
        assert_eq!(format_elapsed(59), "59s");
        assert_eq!(format_elapsed(60), "1m 0s");
        assert_eq!(format_elapsed(170), "2m 50s");
        assert_eq!(format_elapsed(3661), "1h 1m 1s");
    }

    #[test]
    fn activity_hint_shows_still_thinking_with_token_count() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 1_000,
                input_tokens: 0,
                tokens: 663,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::On,
        );

        assert_eq!(
            hint,
            "⠹ Fix input colors... (1s · ↓ 663 tokens · thinking with high effort)"
        );
    }

    #[test]
    fn activity_hint_omits_empty_token_directions() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 1_000,
                input_tokens: 11,
                tokens: 0,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
        );

        assert!(hint.contains("↑ 11 tokens"));
        assert!(!hint.contains("↓ 0"));
    }

    #[test]
    fn activity_hint_shows_agent_tokens_without_direction() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "优化多智能体",
                elapsed_ms: 38_000,
                input_tokens: 0,
                tokens: 0,
                agent_tokens: 16_160,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
        );

        assert!(hint.contains("agent 16.2k tokens"));
        assert!(!hint.contains("↑"));
        assert!(!hint.contains("↓"));
    }

    #[test]
    fn activity_hint_uses_stable_title_across_elapsed_seconds() {
        let first = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 1_000,
                input_tokens: 1,
                tokens: 100,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
        );
        let second = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 2_000,
                input_tokens: 1,
                tokens: 100,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
        );

        assert!(first.contains("Fix input colors..."));
        assert!(second.contains("Fix input colors..."));
        assert!(!first.contains("Undulating"));
        assert!(!second.contains("Reticulating"));
    }
}

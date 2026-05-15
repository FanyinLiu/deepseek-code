use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::ThinkingMode;
use crate::tui::{motion, theme};

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
    pub motion_level: motion::MotionLevel,
}

pub struct StatusActivity<'a> {
    pub title: &'a str,
    pub elapsed_ms: u64,
    pub tokens: u64,
    pub thought_seconds: u64,
}

pub(crate) fn activity_hint_text(
    activity: &StatusActivity<'_>,
    thinking: &ThinkingMode,
    motion_level: motion::MotionLevel,
) -> String {
    let frame = motion::MotionFrame::new(motion_level, activity.elapsed_ms);
    let spinner = frame.running_icon();
    let title = stable_activity_title(activity.title);
    let elapsed = format_elapsed(activity.elapsed_ms / 1_000);
    let effort = thinking_effort_label(thinking);

    if activity.tokens > 0 {
        format!(
            "{spinner} {title}{} ({elapsed} · ↓ {} tokens · thinking with {effort} effort)",
            frame.dots(),
            activity.tokens
        )
    } else {
        format!(
            "{spinner} {title}{} ({elapsed} · thinking with {effort} effort)",
            frame.dots()
        )
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
        activity_hint_text(&activity, props.thinking, props.motion_level),
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
    use crate::tui::motion::MotionLevel;

    #[test]
    fn activity_hint_uses_english_task_title() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 6_000,
                tokens: 578,
                thought_seconds: 2,
            },
            &ThinkingMode::Auto,
            MotionLevel::Off,
        );

        assert_eq!(
            hint,
            "* Fix input colors... (6s · ↓ 578 tokens · thinking with auto effort)"
        );
    }

    #[test]
    fn activity_hint_uses_chinese_task_title() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "修复输入颜色",
                elapsed_ms: 6_000,
                tokens: 578,
                thought_seconds: 2,
            },
            &ThinkingMode::Auto,
            MotionLevel::Off,
        );

        assert_eq!(
            hint,
            "* 修复输入颜色... (6s · ↓ 578 tokens · thinking with auto effort)"
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
                tokens: 663,
                thought_seconds: 0,
            },
            &ThinkingMode::On,
            MotionLevel::Off,
        );

        assert_eq!(
            hint,
            "* Fix input colors... (1s · ↓ 663 tokens · thinking with high effort)"
        );
    }

    #[test]
    fn activity_hint_uses_stable_title_across_elapsed_seconds() {
        let first = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 1_000,
                tokens: 100,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
            MotionLevel::Off,
        );
        let second = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 2_000,
                tokens: 100,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
            MotionLevel::Off,
        );

        assert!(first.contains("Fix input colors..."));
        assert!(second.contains("Fix input colors..."));
        assert!(!first.contains("Undulating"));
        assert!(!second.contains("Reticulating"));
    }
}

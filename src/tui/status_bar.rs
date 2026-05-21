use std::env;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::deepseek::ThinkingMode;
use crate::tui::{motion, theme};

// "Bloom" spinner: dot → ring → bullseye → fisheye → solid disc, then
// plays back in reverse for a 12-frame breathing/blooming cycle.
//
// Every glyph lives in the Geometric Shapes (U+25xx) or Mathematical
// Operators (U+22xx) blocks, neither of which has emoji presentation in
// Unicode — so terminals like Windows Terminal / VS Code can't override
// our `assistant` accent color the way they do with the U+27xx dingbats
// (✶ ✻ ✽ etc.) available in some terminals. We keep a flowering rhythm.
// rhythm without losing the colour.
const STATUS_SPINNER_FRAMES: [&str; 6] = ["·", "∘", "○", "◎", "◉", "●"];
const GHOSTTY_STATUS_SPINNER_FRAMES: [&str; 6] = ["·", "∘", "○", "◎", "◉", "●"];

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
        let palette = theme::palette();
        match self {
            Self::Chat | Self::Plan => palette.accent,
            Self::Run => palette.success,
            Self::Review => palette.info,
        }
    }
}

pub struct StatusBarProps<'a> {
    pub mode: AppMode,
    pub thinking: &'a ThinkingMode,
    pub activity: Option<StatusActivity<'a>>,
    pub chinese: bool,
    pub motion: motion::MotionFrame,
}

pub struct StatusActivity<'a> {
    pub title: &'a str,
    pub elapsed_ms: u64,
    pub input_tokens: u64,
    pub tokens: u64,
    pub agent_tokens: u64,
    pub thought_seconds: u64,
}

#[cfg(test)]
pub(crate) fn activity_hint_text(activity: &StatusActivity<'_>, thinking: &ThinkingMode) -> String {
    activity_hint_text_with_motion(activity, thinking, motion::MotionFrame::disabled())
}

pub(crate) fn activity_hint_text_with_motion(
    activity: &StatusActivity<'_>,
    thinking: &ThinkingMode,
    frame: motion::MotionFrame,
) -> String {
    activity_hint_text_with_motion_localized(activity, thinking, frame, false)
}

pub(crate) fn activity_hint_text_with_motion_localized(
    activity: &StatusActivity<'_>,
    thinking: &ThinkingMode,
    _frame: motion::MotionFrame,
    chinese: bool,
) -> String {
    let title = stable_activity_title(activity.title, chinese);
    let elapsed = format_elapsed(activity.elapsed_ms / 1_000);
    let state = activity_state_label(activity, thinking, chinese);
    let is_idle = title.starts_with("Idle for ") || title.starts_with("空闲");
    // Idle keeps the standalone ✻ (spinner is off). Active turns let the
    // animated spinner glyph itself act as the leading mark; the body just
    // gets the U+2026 ellipsis.
    let prefix = if is_idle { "✻ " } else { "" };
    let dots = if is_idle { "" } else { "…" };

    if is_idle {
        return format!("{prefix}{title}");
    }

    if let Some(token_label) =
        activity_token_label_with_motion(activity, activity.elapsed_ms, chinese)
    {
        if token_label.is_empty() {
            format!("{prefix}{title}{dots} ({elapsed} · {state})")
        } else {
            format!("{prefix}{title}{dots} ({elapsed} · {token_label} · {state})")
        }
    } else {
        format!("{prefix}{title}{dots} ({elapsed} · {state})")
    }
}

fn activity_state_label(
    activity: &StatusActivity<'_>,
    thinking: &ThinkingMode,
    chinese: bool,
) -> String {
    if activity.thought_seconds > 0 && activity.tokens > 0 {
        return if chinese { "推理中" } else { "reasoning" }.to_string();
    }

    let elapsed_seconds = activity.elapsed_ms / 1_000;
    let effort = thinking_effort_label(thinking, chinese);
    let phase = if elapsed_seconds >= 90 {
        if chinese {
            "接近完成"
        } else {
            "almost done thinking"
        }
    } else if chinese {
        "思考中"
    } else {
        "thinking"
    };
    if matches!(thinking, ThinkingMode::Auto) {
        return phase.to_string();
    }
    if chinese {
        format!("{phase} · {effort}")
    } else {
        format!("{phase} with {effort} effort")
    }
}

fn activity_token_label(
    input_tokens: u64,
    output_tokens: u64,
    agent_tokens: u64,
    chinese: bool,
) -> String {
    if output_tokens > 0 {
        return format!("↓ {}", token_count_label(output_tokens, chinese));
    }
    if input_tokens > 0 {
        return format!("↑ {}", token_count_label(input_tokens, chinese));
    }
    if agent_tokens > 0 {
        return format!(
            "{} {}",
            if chinese { "智能体" } else { "agent" },
            token_count_label(agent_tokens, chinese)
        );
    }
    String::new()
}

fn activity_token_label_with_motion(
    activity: &StatusActivity<'_>,
    elapsed_ms: u64,
    chinese: bool,
) -> Option<String> {
    let active_reasoning = activity.thought_seconds > 0 && activity.tokens > 0;
    if elapsed_ms < 3_000 && !active_reasoning {
        return None;
    }

    let label = activity_token_label(
        activity.input_tokens,
        activity.tokens,
        activity.agent_tokens,
        chinese,
    );

    if label.is_empty() {
        Some(String::new())
    } else {
        Some(label)
    }
}

fn status_spinner_icon(frame: motion::MotionFrame) -> &'static str {
    if !frame.level.is_enabled() {
        // Static "rest" pose matching the largest frame of the bloom spinner.
        return "●";
    }

    status_spinner_frame(frame.elapsed_ms, is_ghostty_like_terminal())
}

pub(crate) fn status_spinner_index(elapsed_ms: u64) -> usize {
    let tick = (elapsed_ms / motion::MotionFrame::TICK_MS) as usize;
    let tick = tick % 12;
    if tick < 6 {
        tick
    } else {
        12 - tick - 1
    }
}

fn status_spinner_frame(elapsed_ms: u64, ghostty: bool) -> &'static str {
    let idx = status_spinner_index(elapsed_ms);
    if ghostty {
        GHOSTTY_STATUS_SPINNER_FRAMES[idx]
    } else {
        STATUS_SPINNER_FRAMES[idx]
    }
}

fn is_ghostty_like_terminal() -> bool {
    env::var("GHOSTTY").is_ok()
        || env::var("TERM_PROGRAM").is_ok_and(|term| term.eq_ignore_ascii_case("ghostty"))
        || env::var("TERM").is_ok_and(|term| term.to_ascii_lowercase().contains("ghostty"))
}

fn token_count_label(tokens: u64, chinese: bool) -> String {
    let unit = if !chinese && tokens != 1 {
        "tokens"
    } else {
        "token"
    };
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

fn stable_activity_title(title: &str, chinese: bool) -> &str {
    let trimmed = title.trim();
    if !trimmed.is_empty() {
        clear_fallback_verb();
        return trimmed;
    }
    cached_fallback_verb(chinese)
}

thread_local! {
    static FALLBACK_VERB: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

fn cached_fallback_verb(chinese: bool) -> &'static str {
    FALLBACK_VERB.with(|slot| {
        if let Some(v) = slot.get() {
            return v;
        }
        let v = crate::tui::spinner_verbs::pick_verb(
            chinese,
            crate::tui::spinner_verbs::ambient_nonce(),
        );
        slot.set(Some(v));
        v
    })
}

fn clear_fallback_verb() {
    FALLBACK_VERB.with(|slot| slot.set(None));
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

fn thinking_effort_label(thinking: &ThinkingMode, chinese: bool) -> &'static str {
    match (thinking, chinese) {
        (ThinkingMode::Auto, true) => "自动强度",
        (ThinkingMode::On, true) => "高强度",
        (ThinkingMode::Off, true) => "无思考",
        (ThinkingMode::Auto, false) => "auto",
        (ThinkingMode::On, false) => "high",
        (ThinkingMode::Off, false) => "no",
    }
}

/// Activity line. Idle state stays minimal but visible with a subtle status token.
pub fn render_status_bar(f: &mut Frame, area: Rect, props: StatusBarProps<'_>) {
    let p = theme::palette();
    let Some(activity) = props.activity else {
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(p.canvas)),
            area,
        );
        return;
    };

    let text = if props.chinese {
        activity_hint_text_with_motion_localized(&activity, props.thinking, props.motion, true)
    } else {
        activity_hint_text_with_motion(&activity, props.thinking, props.motion)
    };
    let is_idle = is_idle_activity_title(activity.title, &text);
    let hint_color = activity_line_color(props.motion, &activity, p);
    let line_style = Style::default().fg(hint_color).bg(p.canvas);
    let line = if is_idle {
        Line::from(vec![Span::styled(text, line_style)])
    } else {
        // Split "Title… (3s · ...)" into shimmering title + static metadata.
        // The animated spinner glyph plays the role of the leading mark.
        let (title_part, tail_part) = match text.find(" (") {
            Some(idx) => text.split_at(idx),
            None => (text.as_str(), ""),
        };
        let mut spans = Vec::with_capacity(title_part.chars().count() + 2);
        spans.push(Span::styled(
            format!("{} ", status_spinner_icon(props.motion)),
            line_style,
        ));
        if props.motion.level.is_enabled() {
            spans.extend(theme::shimmer_text(
                title_part,
                props.motion.elapsed_ms,
                hint_color,
                p.assistant_shimmer,
                p.canvas,
            ));
        } else {
            spans.push(Span::styled(title_part.to_string(), line_style));
        }
        if !tail_part.is_empty() {
            spans.push(Span::styled(tail_part.to_string(), line_style));
        }
        Line::from(spans)
    };

    let status = Paragraph::new(line)
        .style(Style::default().bg(p.canvas))
        .alignment(Alignment::Left);

    f.render_widget(status, area);
}

fn is_idle_activity_title(title: &str, text: &str) -> bool {
    title.starts_with("Idle for ")
        || title.starts_with("空闲")
        || text.starts_with("✻ Idle for")
        || text.starts_with("✻ 空闲")
}

fn activity_line_color(
    frame: motion::MotionFrame,
    activity: &StatusActivity<'_>,
    p: theme::ThemePalette,
) -> Color {
    let is_stalled =
        frame.elapsed_ms >= 5_000 && activity.tokens == 0 && activity.input_tokens == 0;
    if !is_stalled {
        return p.dim;
    }

    let ratio = ((frame.elapsed_ms.saturating_sub(5_000)) as f64) / 10_000.0;
    theme::blend_colors(p.permission, p.permission_shimmer, ratio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_hint_uses_english_task_title() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 40_000,
                input_tokens: 42,
                tokens: 578,
                agent_tokens: 0,
                thought_seconds: 2,
            },
            &ThinkingMode::Auto,
        );

        assert_eq!(hint, "Fix input colors… (40s · ↓ 578 tokens · reasoning)");
    }

    #[test]
    fn activity_hint_uses_chinese_task_title() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "修复输入颜色",
                elapsed_ms: 40_000,
                input_tokens: 42,
                tokens: 578,
                agent_tokens: 0,
                thought_seconds: 2,
            },
            &ThinkingMode::Auto,
        );

        assert_eq!(hint, "修复输入颜色… (40s · ↓ 578 tokens · reasoning)");
    }

    #[test]
    fn activity_hint_localizes_chinese_status_copy() {
        let hint = activity_hint_text_with_motion_localized(
            &StatusActivity {
                title: "修复输入颜色",
                elapsed_ms: 40_000,
                input_tokens: 42,
                tokens: 578,
                agent_tokens: 0,
                thought_seconds: 2,
            },
            &ThinkingMode::Auto,
            motion::MotionFrame::new(motion::MotionLevel::Subtle, 40_000),
            true,
        );

        assert_eq!(hint, "修复输入颜色… (40s · ↓ 578 token · 推理中)");
    }

    #[test]
    fn activity_hint_uses_one_directional_token_slot() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "优化多智能体",
                elapsed_ms: 40_000,
                input_tokens: 7_200,
                tokens: 572,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
        );

        assert!(hint.contains("↓ 572 tokens"));
        assert!(!hint.contains("↑ 7.2k tokens"));
        assert!(!hint.contains("· ↑"));
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
                elapsed_ms: 40_000,
                input_tokens: 0,
                tokens: 663,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::On,
        );

        assert_eq!(
            hint,
            "Fix input colors… (40s · ↓ 663 tokens · thinking with high effort)"
        );
    }

    #[test]
    fn activity_hint_status_changes_with_elapsed_time() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 96_000,
                input_tokens: 0,
                tokens: 663,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::On,
        );

        assert_eq!(
            hint,
            "Fix input colors… (1m 36s · ↓ 663 tokens · almost done thinking with high effort)"
        );
    }

    #[test]
    fn activity_hint_omits_empty_token_directions() {
        let hint = activity_hint_text(
            &StatusActivity {
                title: "Fix input colors",
                elapsed_ms: 40_000,
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

        assert!(first.contains("Fix input colors…"));
        assert!(second.contains("Fix input colors…"));
        assert!(!first.contains("Undulating"));
        assert!(!second.contains("Reticulating"));
    }

    #[test]
    fn activity_hint_hides_tokens_before_delay_window() {
        let hint = activity_hint_text(
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

        assert_eq!(hint, "Fix input colors… (2s · thinking)");
    }

    #[test]
    fn idle_activity_does_not_claim_thinking() {
        let hint = activity_hint_text_with_motion_localized(
            &StatusActivity {
                title: "空闲 8s",
                elapsed_ms: 8_000,
                input_tokens: 0,
                tokens: 0,
                agent_tokens: 0,
                thought_seconds: 0,
            },
            &ThinkingMode::Auto,
            motion::MotionFrame::disabled(),
            true,
        );

        assert_eq!(hint, "✻ 空闲 8s");
        assert!(!hint.contains("思考中"));
    }

    #[test]
    fn activity_spinner_falls_back_for_ghostty_like_terminal() {
        let previous = env::var("TERM_PROGRAM").ok();
        env::set_var("TERM_PROGRAM", "ghostty");

        let icon = status_spinner_icon(motion::MotionFrame::new(
            motion::MotionLevel::Subtle,
            motion::MotionFrame::TICK_MS,
        ));

        assert!(matches!(icon, "·" | "∘" | "○" | "◎" | "◉" | "●"));

        if let Some(previous) = previous {
            env::set_var("TERM_PROGRAM", previous);
        } else {
            env::remove_var("TERM_PROGRAM");
        }
    }

    #[test]
    fn status_spinner_uses_6_forward_frames_then_reverse() {
        let frames: Vec<&str> = (0..12)
            .map(|step| status_spinner_frame(step * motion::MotionFrame::TICK_MS, false))
            .collect();

        assert_eq!(
            frames,
            vec!["·", "∘", "○", "◎", "◉", "●", "●", "◉", "◎", "○", "∘", "·"]
        );
    }

    #[test]
    fn status_spinner_off_mode_is_static() {
        assert_eq!(
            status_spinner_icon(motion::MotionFrame::new(motion::MotionLevel::Off, 0)),
            status_spinner_icon(motion::MotionFrame::new(motion::MotionLevel::Off, 10_000))
        );
    }

    #[test]
    fn stalled_activity_color_blends_after_threshold() {
        let p = theme::LIGHT_PALETTE;
        let activity = StatusActivity {
            title: "Thinking",
            elapsed_ms: 6_000,
            input_tokens: 0,
            tokens: 0,
            agent_tokens: 0,
            thought_seconds: 0,
        };

        assert_eq!(
            activity_line_color(
                motion::MotionFrame::new(motion::MotionLevel::Subtle, 4_999),
                &activity,
                p
            ),
            p.dim
        );
        assert_ne!(
            activity_line_color(
                motion::MotionFrame::new(motion::MotionLevel::Subtle, 10_000),
                &activity,
                p
            ),
            p.dim
        );
    }
}

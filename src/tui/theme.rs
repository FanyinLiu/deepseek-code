//! Quiet terminal palette and style helpers.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
#[cfg(test)]
use std::cell::Cell;
use std::env;
#[cfg(not(test))]
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
    HighContrast,
}

impl ThemeMode {
    #[must_use]
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" | "system" | "" => Self::Auto,
            "light" | "bright" => Self::Light,
            "dark" | "terminal" => Self::Dark,
            "high-contrast" | "high_contrast" | "contrast" | "hc" => Self::HighContrast,
            _ => Self::Auto,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::HighContrast => "high-contrast",
        }
    }

    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Auto => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::HighContrast,
            Self::HighContrast => Self::Auto,
        }
    }
}

#[cfg(not(test))]
static ACTIVE_THEME: AtomicU8 = AtomicU8::new(3);

#[cfg(test)]
thread_local! {
    static ACTIVE_THEME: Cell<u8> = const { Cell::new(3) };
}

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub canvas: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub input: Color,
    pub text: Color,
    pub secondary: Color,
    pub dim: Color,
    pub muted: Color,
    pub divider: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub info: Color,
    pub inverse_text: Color,
    pub assistant: Color,
    pub assistant_shimmer: Color,
    pub inactive: Color,
    pub user_message_background: Color,
    pub message_actions_background: Color,
    pub selection_bg: Color,
    pub permission: Color,
    pub permission_shimmer: Color,
    pub fast_mode_running: Color,
    pub fast_mode_idle: Color,
}

pub const LIGHT_PALETTE: ThemePalette = ThemePalette {
    // Use terminal default bg so the inline TUI doesn't paint a visible box.
    canvas: Color::Reset,
    surface: Color::Rgb(238, 241, 245),
    surface_alt: Color::Rgb(228, 232, 238),
    input: Color::Rgb(240, 243, 248),
    text: Color::Rgb(30, 34, 40),
    secondary: Color::Rgb(22, 50, 82),
    dim: Color::Rgb(54, 71, 96),
    muted: Color::Rgb(88, 92, 98),
    divider: Color::Rgb(168, 176, 191),
    accent: Color::Rgb(12, 92, 194),
    success: Color::Rgb(40, 148, 78),
    warning: Color::Rgb(168, 116, 0),
    danger: Color::Rgb(194, 42, 46),
    info: Color::Rgb(11, 111, 186),
    inverse_text: Color::Rgb(255, 255, 255),
    assistant: Color::Rgb(215, 119, 87),
    assistant_shimmer: Color::Rgb(235, 159, 127),
    inactive: Color::Rgb(109, 121, 142),
    user_message_background: Color::Rgb(248, 250, 252),
    message_actions_background: Color::Rgb(236, 241, 247),
    selection_bg: Color::Rgb(220, 228, 237),
    permission: Color::Rgb(196, 111, 0),
    permission_shimmer: Color::Rgb(237, 149, 39),
    fast_mode_running: Color::Rgb(39, 122, 255),
    fast_mode_idle: Color::Rgb(146, 167, 190),
};

pub const DARK_PALETTE: ThemePalette = ThemePalette {
    canvas: Color::Reset,
    surface: Color::Rgb(22, 28, 38),
    surface_alt: Color::Rgb(28, 34, 46),
    input: Color::Rgb(20, 24, 33),
    text: Color::Rgb(229, 235, 244),
    secondary: Color::Rgb(173, 190, 215),
    dim: Color::Rgb(64, 94, 132),
    muted: Color::Rgb(80, 86, 96),
    divider: Color::Rgb(56, 69, 88),
    accent: Color::Rgb(120, 183, 255),
    success: Color::Rgb(103, 215, 132),
    warning: Color::Rgb(255, 205, 123),
    danger: Color::Rgb(255, 133, 147),
    info: Color::Rgb(110, 184, 230),
    inverse_text: Color::Rgb(11, 14, 18),
    assistant: Color::Rgb(77, 107, 254),
    assistant_shimmer: Color::Rgb(137, 157, 255),
    inactive: Color::Rgb(132, 146, 170),
    user_message_background: Color::Rgb(24, 31, 42),
    message_actions_background: Color::Rgb(30, 38, 51),
    selection_bg: Color::Rgb(44, 56, 74),
    permission: Color::Rgb(255, 168, 84),
    permission_shimmer: Color::Rgb(255, 200, 129),
    fast_mode_running: Color::Rgb(125, 188, 255),
    fast_mode_idle: Color::Rgb(119, 137, 167),
};

pub const HIGH_CONTRAST_PALETTE: ThemePalette = ThemePalette {
    canvas: Color::Reset,
    surface: Color::Rgb(18, 18, 18),
    surface_alt: Color::Rgb(34, 34, 34),
    input: Color::Rgb(0, 0, 0),
    text: Color::Rgb(255, 255, 255),
    secondary: Color::Rgb(226, 226, 226),
    dim: Color::Rgb(178, 178, 178),
    muted: Color::Rgb(138, 138, 138),
    divider: Color::Rgb(220, 220, 220),
    accent: Color::Rgb(255, 209, 102),
    success: Color::Rgb(82, 255, 145),
    warning: Color::Rgb(255, 215, 0),
    danger: Color::Rgb(255, 93, 93),
    info: Color::Rgb(111, 200, 255),
    inverse_text: Color::Rgb(0, 0, 0),
    assistant: Color::Rgb(255, 237, 153),
    assistant_shimmer: Color::Rgb(255, 255, 255),
    inactive: Color::Rgb(182, 182, 182),
    user_message_background: Color::Rgb(32, 32, 32),
    message_actions_background: Color::Rgb(48, 48, 48),
    selection_bg: Color::Rgb(78, 78, 78),
    permission: Color::Rgb(255, 200, 64),
    permission_shimmer: Color::Rgb(255, 234, 149),
    fast_mode_running: Color::Rgb(110, 255, 205),
    fast_mode_idle: Color::Rgb(190, 190, 190),
};

#[must_use]
pub fn shimmer(base: Color) -> Color {
    let (r, g, b) = match rgb_components(base) {
        Some(value) => value,
        None => return base,
    };

    let (h, s, v) = rgb_to_hsv(r, g, b);
    hsv_to_rgb(h, s, (v + 0.20).clamp(0.0, 1.0))
}

/// Build a left-to-right "scanning shimmer" — a 2-char-wide bright band moves
/// across `text` once per cycle, then rests 20 chars before sweeping again.
///
/// Tuned for a 150 ms per-character sweep, with a 20-character rest period.
/// Peak char uses `shimmer`, neighbours blend 60% toward `shimmer`, and the rest
/// stays at `base`.
#[must_use]
pub fn shimmer_text(
    text: &str,
    elapsed_ms: u64,
    base: Color,
    shimmer: Color,
    bg: Color,
) -> Vec<Span<'static>> {
    const STEP_MS: u64 = 150;
    const REST_CHARS: i64 = 20;

    let chars: Vec<char> = text.chars().collect();
    let width = chars.len();
    if width == 0 {
        return Vec::new();
    }

    let cycle = width as i64 + REST_CHARS;
    let tick = (elapsed_ms / STEP_MS) as i64;
    // Band sweeps from rightmost edge inward, then loops once it walks off the
    // left side and out into the rest region.
    let glimmer = (width as i64 + REST_CHARS / 2) - tick.rem_euclid(cycle);

    chars
        .into_iter()
        .enumerate()
        .map(|(i, ch)| {
            let dist = (i as i64 - glimmer).abs();
            let color = match dist {
                0 => shimmer,
                1 => blend_colors(base, shimmer, 0.6),
                _ => base,
            };
            Span::styled(ch.to_string(), Style::default().fg(color).bg(bg))
        })
        .collect()
}

#[must_use]
pub fn blend_colors(base: Color, overlay: Color, t: f64) -> Color {
    let (base, overlay) = match (rgb_components(base), rgb_components(overlay)) {
        (Some(base), Some(overlay)) => (base, overlay),
        _ => return base,
    };

    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = (
        base.0 + (overlay.0 - base.0) * t,
        base.1 + (overlay.1 - base.1) * t,
        base.2 + (overlay.2 - base.2) * t,
    );
    Color::Rgb(
        (r * 255.0).clamp(0.0, 255.0).round() as u8,
        (g * 255.0).clamp(0.0, 255.0).round() as u8,
        (b * 255.0).clamp(0.0, 255.0).round() as u8,
    )
}

#[must_use]
const fn rgb_components(color: Color) -> Option<(f64, f64, f64)> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };
    Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0))
}

#[must_use]
fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let delta = max - min;
    let mut h = if delta.abs() < f64::EPSILON {
        0.0
    } else if (max - r).abs() < f64::EPSILON {
        ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    h /= 6.0;
    let s = if max.abs() < f64::EPSILON {
        0.0
    } else {
        delta / max
    };
    (h, s, max)
}

#[must_use]
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> Color {
    if s <= f64::EPSILON {
        let c = (v * 255.0).round();
        let value = c.clamp(0.0, 255.0) as u8;
        return Color::Rgb(value, value, value);
    }

    let h_prime = (h * 6.0) % 6.0;
    let c = v * s;
    let x = c * (1.0 - (h_prime.rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = match (h_prime as i32).rem_euclid(6) {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let r = ((r1 + m) * 255.0).round() as u8;
    let g = ((g1 + m) * 255.0).round() as u8;
    let b = ((b1 + m) * 255.0).round() as u8;
    Color::Rgb(r.clamp(0, 255), g.clamp(0, 255), b.clamp(0, 255))
}

pub fn set_active_theme(mode: ThemeMode) {
    #[cfg(not(test))]
    ACTIVE_THEME.store(
        match mode {
            ThemeMode::Light => 0,
            ThemeMode::Dark => 1,
            ThemeMode::HighContrast => 2,
            ThemeMode::Auto => 3,
        },
        Ordering::Relaxed,
    );

    #[cfg(test)]
    ACTIVE_THEME.with(|theme| {
        theme.set(match mode {
            ThemeMode::Light => 0,
            ThemeMode::Dark => 1,
            ThemeMode::HighContrast => 2,
            ThemeMode::Auto => 3,
        });
    });
}

#[must_use]
pub fn active_theme() -> ThemeMode {
    #[cfg(not(test))]
    match ACTIVE_THEME.load(Ordering::Relaxed) {
        1 => ThemeMode::Dark,
        2 => ThemeMode::HighContrast,
        3 => detect_terminal_theme(),
        _ => ThemeMode::Light,
    }

    #[cfg(test)]
    {
        ACTIVE_THEME.with(|theme| match theme.get() {
            1 => ThemeMode::Dark,
            2 => ThemeMode::HighContrast,
            3 => ThemeMode::Dark,
            _ => ThemeMode::Light,
        })
    }
}

#[must_use]
pub fn detect_terminal_theme() -> ThemeMode {
    let override_theme = env::var("DEEPSEEK_CODE_THEME").ok();
    let colorfgbg = env::var("COLORFGBG").ok();
    detect_terminal_theme_from_values(override_theme.as_deref(), colorfgbg.as_deref())
}

fn detect_terminal_theme_from_values(
    override_theme: Option<&str>,
    colorfgbg: Option<&str>,
) -> ThemeMode {
    if let Some(value) = override_theme {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" | "bright" => return ThemeMode::Light,
            "dark" | "terminal" => return ThemeMode::Dark,
            "high-contrast" | "high_contrast" | "contrast" | "hc" => {
                return ThemeMode::HighContrast;
            }
            _ => {}
        }
    }

    if let Some(value) = colorfgbg {
        if let Some(light_bg) = colorfgbg_prefers_light_background(value) {
            return if light_bg {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            };
        }
    }

    ThemeMode::Light
}

fn colorfgbg_prefers_light_background(value: &str) -> Option<bool> {
    let bg = value
        .rsplit(';')
        .find_map(|part| part.parse::<u16>().ok())?;
    if matches!(bg, 7 | 15 | 244..=255) {
        return Some(true);
    }
    if (16..=231).contains(&bg) {
        let cube = bg - 16;
        let r = cube / 36;
        let g = (cube % 36) / 6;
        let b = cube % 6;
        return Some(r + g + b >= 10);
    }
    Some(false)
}

#[must_use]
pub fn palette() -> ThemePalette {
    match active_theme() {
        ThemeMode::Auto => DARK_PALETTE,
        ThemeMode::Light => LIGHT_PALETTE,
        ThemeMode::Dark => DARK_PALETTE,
        ThemeMode::HighContrast => HIGH_CONTRAST_PALETTE,
    }
}

// ═══════════════════════════════════════════════════════════
//  Style Helpers
// ═══════════════════════════════════════════════════════════

pub fn style_primary() -> Style {
    let p = palette();
    Style::default().fg(p.text).bg(p.canvas)
}

pub fn style_secondary() -> Style {
    let p = palette();
    Style::default().fg(p.secondary).bg(p.canvas)
}

pub fn style_dim() -> Style {
    let p = palette();
    Style::default().fg(p.dim).bg(p.canvas)
}

pub fn style_user() -> Style {
    let p = palette();
    Style::default().fg(p.text).bg(p.canvas)
}

pub fn style_assistant() -> Style {
    let p = palette();
    Style::default().fg(p.text).bg(p.canvas)
}

pub fn style_input() -> Style {
    let p = palette();
    Style::default().fg(p.text).bg(p.input)
}

pub fn style_status_ok() -> Style {
    let p = palette();
    Style::default().fg(p.success).bg(p.canvas)
}

pub fn style_status_warn() -> Style {
    let p = palette();
    Style::default().fg(p.warning).bg(p.canvas)
}

pub fn style_status_err() -> Style {
    let p = palette();
    Style::default().fg(p.danger).bg(p.canvas)
}

pub fn style_accent() -> Style {
    let p = palette();
    Style::default()
        .fg(p.accent)
        .bg(p.canvas)
        .add_modifier(Modifier::BOLD)
}

/// Card container style (subtle border, elevated background)
pub fn style_card() -> Style {
    Style::default().bg(palette().surface)
}

/// Card with focus/hover state
pub fn style_card_active() -> Style {
    Style::default().bg(palette().surface_alt)
}

/// Title inside a card
pub fn style_card_title() -> Style {
    let p = palette();
    Style::default()
        .fg(p.accent)
        .bg(p.surface)
        .add_modifier(Modifier::BOLD)
}

/// Subtitle / label inside a card
pub fn style_card_label() -> Style {
    let p = palette();
    Style::default().fg(p.dim).bg(p.surface)
}

/// Value / content inside a card
pub fn style_card_value() -> Style {
    let p = palette();
    Style::default().fg(p.secondary).bg(p.surface)
}

/// Border for unfocused containers
pub fn border_style() -> Style {
    Style::default().fg(palette().divider)
}

/// Border for focused/hovered containers
pub fn border_style_focus() -> Style {
    Style::default().fg(palette().accent)
}

/// Divider line style
pub fn divider_style() -> Style {
    Style::default().fg(palette().divider)
}

/// Muted tag / badge
pub fn style_badge() -> Style {
    let p = palette();
    Style::default().fg(p.muted).bg(p.surface)
}

/// Success badge
pub fn style_badge_ok() -> Style {
    let p = palette();
    Style::default()
        .fg(p.inverse_text)
        .bg(p.success)
        .add_modifier(Modifier::BOLD)
}

/// Warning badge
pub fn style_badge_warn() -> Style {
    let p = palette();
    Style::default()
        .fg(p.inverse_text)
        .bg(p.warning)
        .add_modifier(Modifier::BOLD)
}

/// Error badge
pub fn style_badge_err() -> Style {
    let p = palette();
    Style::default()
        .fg(p.inverse_text)
        .bg(p.danger)
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shimmer_text_produces_one_span_per_char_and_peaks_inside_band() {
        let base = Color::Rgb(100, 100, 100);
        let shimmer = Color::Rgb(255, 255, 255);
        let bg = Color::Rgb(0, 0, 0);

        let spans = shimmer_text("hello", 0, base, shimmer, bg);
        assert_eq!(spans.len(), 5);

        // At least one char in the visible region must hit the shimmer peak
        // somewhere during the cycle; rest of the chars stay at base.
        let cycle_ms = (5 + 20) * 150;
        let mut saw_peak = false;
        for t in (0..cycle_ms).step_by(150) {
            let spans = shimmer_text("hello", t as u64, base, shimmer, bg);
            if spans.iter().any(|s| s.style.fg == Some(shimmer)) {
                saw_peak = true;
                break;
            }
        }
        assert!(saw_peak, "shimmer band never reaches a visible char");
    }

    #[test]
    fn shimmer_text_empty_input_yields_empty_vec() {
        let spans = shimmer_text(
            "",
            0,
            Color::Rgb(0, 0, 0),
            Color::Rgb(255, 255, 255),
            Color::Rgb(0, 0, 0),
        );
        assert!(spans.is_empty());
    }

    #[test]
    fn parses_theme_aliases() {
        assert_eq!(ThemeMode::from_config("auto"), ThemeMode::Auto);
        assert_eq!(ThemeMode::from_config("system"), ThemeMode::Auto);
        assert_eq!(ThemeMode::from_config(""), ThemeMode::Auto);
        assert_eq!(ThemeMode::from_config("dark"), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_config("terminal"), ThemeMode::Dark);
        assert_eq!(
            ThemeMode::from_config("high-contrast"),
            ThemeMode::HighContrast
        );
        assert_eq!(ThemeMode::from_config("hc"), ThemeMode::HighContrast);
        assert_eq!(ThemeMode::from_config("light"), ThemeMode::Light);
        assert_eq!(ThemeMode::from_config("unknown"), ThemeMode::Auto);
    }

    #[test]
    fn palettes_have_distinct_readable_surfaces() {
        assert_ne!(LIGHT_PALETTE.canvas, LIGHT_PALETTE.surface);
        assert_ne!(HIGH_CONTRAST_PALETTE.text, HIGH_CONTRAST_PALETTE.canvas);
        assert_ne!(LIGHT_PALETTE.accent, LIGHT_PALETTE.canvas);
        assert_ne!(DARK_PALETTE.accent, DARK_PALETTE.canvas);
        assert_ne!(HIGH_CONTRAST_PALETTE.accent, HIGH_CONTRAST_PALETTE.canvas);
    }

    #[test]
    fn colorfgbg_background_detection_handles_common_values() {
        assert_eq!(colorfgbg_prefers_light_background("15;0"), Some(false));
        assert_eq!(colorfgbg_prefers_light_background("0;15"), Some(true));
        assert_eq!(colorfgbg_prefers_light_background("7;232"), Some(false));
        assert_eq!(colorfgbg_prefers_light_background("0;231"), Some(true));
        assert_eq!(colorfgbg_prefers_light_background(""), None);
    }

    #[test]
    fn auto_theme_without_terminal_background_metadata_uses_reset_canvas() {
        assert_eq!(
            detect_terminal_theme_from_values(None, None),
            ThemeMode::Light
        );
        assert_eq!(
            detect_terminal_theme_from_values(None, Some("0;15")),
            ThemeMode::Light
        );
        assert_eq!(
            detect_terminal_theme_from_values(None, Some("15;0")),
            ThemeMode::Dark
        );
        assert_eq!(
            detect_terminal_theme_from_values(Some("dark"), None),
            ThemeMode::Dark
        );
    }

    #[test]
    fn shimmer_increases_value_component() {
        let base = Color::Rgb(10, 100, 200);
        let bright = shimmer(base);
        assert_ne!(bright, base);
        let Color::Rgb(r, g, b) = bright else {
            panic!("expected Rgb output");
        };
        assert!(r > 10);
        assert!(g >= 100);
        assert!(b >= 200);
    }

    #[test]
    fn blend_colors_interpolates_components() {
        assert_eq!(
            blend_colors(Color::Rgb(20, 30, 40), Color::Rgb(120, 230, 40), 0.0),
            Color::Rgb(20, 30, 40)
        );
        assert_eq!(
            blend_colors(Color::Rgb(20, 30, 40), Color::Rgb(120, 230, 40), 1.0),
            Color::Rgb(120, 230, 40)
        );
        assert_eq!(
            blend_colors(Color::Rgb(20, 30, 40), Color::Rgb(120, 230, 40), 0.5),
            Color::Rgb(70, 130, 40)
        );
    }
}

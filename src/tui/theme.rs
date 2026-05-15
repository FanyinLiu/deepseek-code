//! Quiet terminal palette and style helpers.
use ratatui::style::{Color, Modifier, Style};
#[cfg(test)]
use std::cell::Cell;
use std::env;
#[cfg(not(test))]
use std::sync::atomic::{AtomicU8, Ordering};

// ═══════════════════════════════════════════════════════════
//  Base Layer (Backgrounds)
// ═══════════════════════════════════════════════════════════
pub const BG_DEEP: Color = Color::Rgb(14, 14, 18); //  deepest background
pub const BG_BASE: Color = Color::Rgb(20, 20, 26); //  base canvas
pub const BG_CARD: Color = Color::Rgb(28, 28, 36); //  card background
pub const BG_CARD_HOVER: Color = Color::Rgb(34, 34, 44);
pub const BG_CARD_ALT: Color = BG_CARD_HOVER; // deprecated alias //  card hover/selected
pub const BG_INPUT: Color = Color::Rgb(24, 24, 32); //  input area

// Droid-like light canvas used by the welcome surface and composer.
pub const DROID_CANVAS_BG: Color = Color::Rgb(235, 226, 196);
pub const DROID_INK: Color = Color::Rgb(17, 17, 14);
pub const DROID_MUTED: Color = Color::Rgb(78, 78, 72);
pub const DROID_ACCENT: Color = Color::Rgb(224, 82, 0);

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
}

pub const LIGHT_PALETTE: ThemePalette = ThemePalette {
    canvas: DROID_CANVAS_BG,
    surface: DROID_CANVAS_BG,
    surface_alt: Color::Rgb(228, 219, 190),
    input: DROID_CANVAS_BG,
    text: DROID_INK,
    secondary: Color::Rgb(54, 54, 48),
    dim: DROID_MUTED,
    muted: Color::Rgb(116, 104, 84),
    divider: DROID_ACCENT,
    accent: DROID_ACCENT,
    success: Color::Rgb(18, 140, 52),
    warning: Color::Rgb(190, 120, 0),
    danger: Color::Rgb(210, 55, 35),
    info: Color::Rgb(42, 102, 160),
    inverse_text: Color::Rgb(250, 246, 232),
};

pub const DARK_PALETTE: ThemePalette = ThemePalette {
    canvas: BG_DEEP,
    surface: BG_CARD,
    surface_alt: BG_CARD_HOVER,
    input: BG_INPUT,
    text: FG_PRIMARY,
    secondary: FG_SECONDARY,
    dim: FG_DIM,
    muted: FG_MUTED,
    divider: DIVIDER,
    accent: ACCENT_AMBER,
    success: ACCENT_GREEN,
    warning: ACCENT_YELLOW,
    danger: ACCENT_RED,
    info: ACCENT_BLUE,
    inverse_text: BG_DEEP,
};

pub const HIGH_CONTRAST_PALETTE: ThemePalette = ThemePalette {
    canvas: Color::Rgb(0, 0, 0),
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
};

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
    if let Ok(value) = env::var("DEEPSEEK_CODE_THEME") {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" | "bright" => return ThemeMode::Light,
            "dark" | "terminal" => return ThemeMode::Dark,
            "high-contrast" | "high_contrast" | "contrast" | "hc" => {
                return ThemeMode::HighContrast;
            }
            _ => {}
        }
    }

    if let Ok(value) = env::var("COLORFGBG") {
        if let Some(light_bg) = colorfgbg_prefers_light_background(&value) {
            return if light_bg {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            };
        }
    }

    ThemeMode::Dark
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
//  Border & Divider Layer
// ═══════════════════════════════════════════════════════════
pub const BORDER_DIM: Color = Color::Rgb(44, 44, 56);
pub const BORDER_DEFAULT: Color = Color::Rgb(58, 58, 74);
pub const BORDER_FOCUS: Color = Color::Rgb(88, 88, 108);
pub const DIVIDER: Color = Color::Rgb(48, 48, 62);

// ═══════════════════════════════════════════════════════════
//  Text Layer
// ═══════════════════════════════════════════════════════════
pub const FG_PRIMARY: Color = Color::Rgb(230, 230, 236);
pub const FG_SECONDARY: Color = Color::Rgb(176, 176, 190);
pub const FG_DIM: Color = Color::Rgb(120, 120, 138);
pub const FG_MUTED: Color = Color::Rgb(76, 76, 96);

// ═══════════════════════════════════════════════════════════
//  Accent Layer
// ═══════════════════════════════════════════════════════════
pub const ACCENT_AMBER: Color = Color::Rgb(210, 150, 100);
pub const ACCENT_GREEN: Color = Color::Rgb(110, 184, 140);
pub const ACCENT_RED: Color = Color::Rgb(220, 100, 100);
pub const ACCENT_YELLOW: Color = Color::Rgb(210, 180, 100);
pub const ACCENT_BLUE: Color = Color::Rgb(120, 160, 210);
pub const ACCENT_PURPLE: Color = Color::Rgb(170, 140, 200);

// ═══════════════════════════════════════════════════════════
//  Semantic Colors
// ═══════════════════════════════════════════════════════════
pub const SUCCESS: Color = ACCENT_GREEN;
pub const WARNING: Color = ACCENT_YELLOW;
pub const ERROR: Color = ACCENT_RED;
pub const INFO: Color = ACCENT_BLUE;
pub const BRAND: Color = ACCENT_AMBER;

// ═══════════════════════════════════════════════════════════
//  Role Colors
// ═══════════════════════════════════════════════════════════
pub const USER_BG: Color = BG_DEEP;
pub const ASSISTANT_BG: Color = BG_DEEP;
pub const SYSTEM_FG: Color = FG_DIM;
pub const TOOL_FG: Color = ACCENT_AMBER;

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
        assert_ne!(LIGHT_PALETTE.canvas, DARK_PALETTE.canvas);
        assert_ne!(LIGHT_PALETTE.text, LIGHT_PALETTE.canvas);
        assert_ne!(DARK_PALETTE.text, DARK_PALETTE.canvas);
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
}

//! ASCII art wordmarks for the TUI.

/// A compact one-line logo for dense status surfaces.
pub const LOGO_TINY: &str = "OCTOCODE";

/// Tiny product mark for compact brand lockups.
pub const OCTOCODE_TINY: &str = LOGO_TINY;
pub const OCTO_TINY: &str = LOGO_TINY;

pub const WELCOME_OCTOCODE_SOLID_HEIGHT: usize = 5;
pub const WELCOME_OCTOCODE_GAP: usize = 1;
pub const WELCOME_OCTO_SOLID_HEIGHT: usize = WELCOME_OCTOCODE_SOLID_HEIGHT;
pub const WELCOME_OCTO_GAP: usize = WELCOME_OCTOCODE_GAP;
pub const WELCOME_WHALE_MARK_HEIGHT: usize = 7;
pub const WELCOME_OCTO_COMPACT_MARK_HEIGHT: usize = 5;
pub const WELCOME_CLAWD_PIXEL_MARK_HEIGHT: usize = 10;
pub const WELCOME_CLAWD_PIXEL_MARK_WIDTH: usize = 12;

/// Pixel mascot mark adapted from the Clawd logo sketch.
///
/// Legend: `_` empty, `K` ink, `B` body, `L` highlight, `E` eye, `P` blush.
pub const WELCOME_CLAWD_PIXEL_MARK: &[&str; WELCOME_CLAWD_PIXEL_MARK_HEIGHT] = &[
    "___KKKKKK___",
    "__KBBBBBBK__",
    "_KBLBBBBLBK_",
    "_KBEBBBBEBK_",
    "_KPBBBBBBPK_",
    "_KBBBBBBBBK_",
    "__KKBBBBKK__",
    "__KBK__KBK__",
    "__KK____KK__",
    "____________",
];

/// Octo mascot mark used on spacious welcome surfaces.
pub const WELCOME_WHALE_MARK: &[&str; WELCOME_WHALE_MARK_HEIGHT] = &[
    "     .-\"\"\"-.       ",
    "   .'  o o  '.     ",
    "  /   .-^- .  \\    ",
    " |    \\___/    |   ",
    "  \\  ._____.  /    ",
    " _/\\_/| | |\\_/\\_  ",
    "/_/  \\_/ \\_/  \\_\\ ",
];

/// Compact Octo mascot for the normal 7-row welcome lockup.
pub const WELCOME_OCTO_COMPACT_MARK: &[&str; WELCOME_OCTO_COMPACT_MARK_HEIGHT] = &[
    "   .---.    ",
    " .' o o '.  ",
    "/  .-^-.  \\ ",
    "\\  \\___/  / ",
    " /\\/| |\\/\\  ",
];

#[derive(Debug, Clone, Copy)]
pub struct SolidGlyph {
    pub rows: [&'static str; WELCOME_OCTOCODE_SOLID_HEIGHT],
}

/// OCTOCODE wordmark used in welcome lockups.
pub const WELCOME_OCTOCODE_SOLID: &[SolidGlyph] = &[
    SolidGlyph {
        rows: [" █████", "██    ", "██    ", "██    ", " █████"],
    },
    SolidGlyph {
        rows: [" ████ ", "██  ██", "██  ██", "██  ██", " ████ "],
    },
    SolidGlyph {
        rows: ["██████", "  ██  ", "  ██  ", "  ██  ", "  ██  "],
    },
    SolidGlyph {
        rows: [" ████ ", "██  ██", "██  ██", "██  ██", " ████ "],
    },
    SolidGlyph {
        rows: [" █████", "██    ", "██    ", "██    ", " █████"],
    },
    SolidGlyph {
        rows: [" ████ ", "██  ██", "██  ██", "██  ██", " ████ "],
    },
    SolidGlyph {
        rows: ["█████ ", "██  ██", "██  ██", "██  ██", "█████ "],
    },
    SolidGlyph {
        rows: ["██████", "██    ", "█████ ", "██    ", "██████"],
    },
];
pub const WELCOME_OCTO_SOLID: &[SolidGlyph] = WELCOME_OCTOCODE_SOLID;

#[must_use]
pub fn welcome_octocode_solid_width() -> usize {
    let glyph_width: usize = WELCOME_OCTOCODE_SOLID
        .iter()
        .map(|glyph| glyph.rows[0].chars().count())
        .sum();
    let gap_width = WELCOME_OCTOCODE_SOLID.len().saturating_sub(1) * WELCOME_OCTOCODE_GAP;
    glyph_width + gap_width
}

#[must_use]
pub fn welcome_octo_compact_mark_width() -> usize {
    WELCOME_OCTO_COMPACT_MARK
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
}

#[must_use]
pub fn welcome_clawd_pixel_render_width() -> usize {
    WELCOME_CLAWD_PIXEL_MARK_WIDTH * 2
}

/// Compact OCTOCODE mark for narrow welcome surfaces.
pub const WELCOME_OCTOCODE_COMPACT: &[&str] = &["OCTOCODE"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_octocode_solid_stays_terminal_safe() {
        assert_eq!(WELCOME_OCTOCODE_SOLID.len(), 8);
        assert_eq!(WELCOME_OCTOCODE_SOLID_HEIGHT, 5);
        assert_eq!(welcome_octocode_solid_width(), 55);
        for glyph in WELCOME_OCTOCODE_SOLID {
            for line in glyph.rows {
                assert_eq!(line.chars().count(), 6);
                assert!(line.contains('█') || line.trim().is_empty());
            }
        }

        assert_eq!(WELCOME_OCTOCODE_COMPACT.len(), 1);
        for line in WELCOME_OCTOCODE_COMPACT {
            assert!(line.chars().count() <= 8);
            assert!(line.is_ascii());
        }

        assert_eq!(WELCOME_WHALE_MARK.len(), WELCOME_WHALE_MARK_HEIGHT);
        for line in WELCOME_WHALE_MARK {
            assert!(line.chars().count() <= 24);
            assert!(line.is_ascii());
        }

        assert_eq!(
            WELCOME_CLAWD_PIXEL_MARK.len(),
            WELCOME_CLAWD_PIXEL_MARK_HEIGHT
        );
        assert_eq!(welcome_clawd_pixel_render_width(), 24);
        for line in WELCOME_CLAWD_PIXEL_MARK {
            assert_eq!(line.chars().count(), WELCOME_CLAWD_PIXEL_MARK_WIDTH);
            assert!(line
                .chars()
                .all(|ch| matches!(ch, '_' | 'K' | 'B' | 'L' | 'E' | 'P')));
        }

        assert_eq!(
            WELCOME_OCTO_COMPACT_MARK.len(),
            WELCOME_OCTO_COMPACT_MARK_HEIGHT
        );
        assert!(welcome_octo_compact_mark_width() <= 16);
        for line in WELCOME_OCTO_COMPACT_MARK {
            assert!(line.is_ascii());
        }
    }
}

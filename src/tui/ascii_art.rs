//! ASCII art wordmarks for the TUI.

/// A compact one-line logo for dense status surfaces.
pub const LOGO_TINY: &str = "DS";

/// Tiny product mark for compact brand lockups.
pub const DSCODE_TINY: &str = LOGO_TINY;

pub const WELCOME_DSCODE_SOLID_HEIGHT: usize = 5;
pub const WELCOME_DSCODE_GAP: usize = 1;

#[derive(Debug, Clone, Copy)]
pub struct SolidGlyph {
    pub rows: [&'static str; WELCOME_DSCODE_SOLID_HEIGHT],
}

/// DSCODE wordmark used in welcome lockups.
pub const WELCOME_DSCODE_SOLID: &[SolidGlyph] = &[
    SolidGlyph {
        rows: ["█████ ", "██  ██", "██  ██", "██  ██", "█████ "],
    },
    SolidGlyph {
        rows: ["██████", "██    ", "█████ ", "    ██", "██████"],
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

#[must_use]
pub fn welcome_dscode_solid_width() -> usize {
    let glyph_width: usize = WELCOME_DSCODE_SOLID
        .iter()
        .map(|glyph| glyph.rows[0].chars().count())
        .sum();
    let gap_width = WELCOME_DSCODE_SOLID.len().saturating_sub(1) * WELCOME_DSCODE_GAP;
    glyph_width + gap_width
}

/// Compact DSCODE mark for narrow welcome surfaces.
pub const WELCOME_DSCODE_COMPACT: &[&str] = &["DSCODE"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_dscode_solid_stays_terminal_safe() {
        assert_eq!(WELCOME_DSCODE_SOLID.len(), 6);
        assert_eq!(WELCOME_DSCODE_SOLID_HEIGHT, 5);
        assert_eq!(welcome_dscode_solid_width(), 41);
        for glyph in WELCOME_DSCODE_SOLID {
            for line in glyph.rows {
                assert_eq!(line.chars().count(), 6);
                assert!(line.contains('█') || line.trim().is_empty());
            }
        }

        assert_eq!(WELCOME_DSCODE_COMPACT.len(), 1);
        for line in WELCOME_DSCODE_COMPACT {
            assert!(line.chars().count() <= 6);
            assert!(line.is_ascii());
        }
    }
}

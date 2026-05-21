//! ASCII art wordmarks for the TUI.

pub const WELCOME_OCTOCODE_SOLID_HEIGHT: usize = 5;
pub const WELCOME_OCTO_GHOST_HEIGHT: usize = 12;
pub const WELCOME_OCTO_GHOST_WIDTH: usize = 13;

/// Pac-Man-style ghost octopus mascot from the desktop reference image.
///
/// Legend: `_` empty, `B` body, `D` darker tentacle tip, `E` dark eye socket.
pub const WELCOME_OCTO_GHOST: &[&str; WELCOME_OCTO_GHOST_HEIGHT] = &[
    "_____BBB_____",
    "___BBBBBBB___",
    "__BBBBBBBBB__",
    "_BBBBBBBBBBB_",
    "_BBEEBBBEEBB_",
    "_BBEEBBBEEBB_",
    "_BBBBBBBBBBB_",
    "_BBBBBBBBBBB_",
    "_BBBBBBBBBBB_",
    "_BBBBBBBBBBB_",
    "_BB_BB_BB_BB_",
    "_DD_DD_DD_DD_",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_octocode_solid_stays_terminal_safe() {
        assert_eq!(WELCOME_OCTOCODE_SOLID.len(), 8);
        assert_eq!(WELCOME_OCTOCODE_SOLID_HEIGHT, 5);
        for glyph in WELCOME_OCTOCODE_SOLID {
            for line in glyph.rows {
                assert_eq!(line.chars().count(), 6);
                assert!(line.contains('█') || line.trim().is_empty());
            }
        }

        assert_eq!(WELCOME_OCTO_GHOST.len(), WELCOME_OCTO_GHOST_HEIGHT);
        for line in WELCOME_OCTO_GHOST {
            assert_eq!(line.chars().count(), WELCOME_OCTO_GHOST_WIDTH);
            assert!(line.chars().all(|ch| matches!(ch, '_' | 'B' | 'D' | 'E')));
        }
    }
}

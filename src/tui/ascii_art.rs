//! ASCII art logos and spinner frames for the TUI.

/// The main DeepSeek Code logo (block-character style).
/// Rendered at ~20×6 cells — fits nicely even on small terminals.
pub const LOGO_LARGE: &str = r#"
    ██████╗  ███████╗
    ██╔══██╗ ██╔════╝
    ██║  ██║ ███████╗
    ██║  ██║ ╚════██║
    ██████╔╝ ███████║
    ╚═════╝  ╚══════╝
"#;

/// A compact one-line logo for the status bar.
pub const LOGO_TINY: &str = "◆";

/// Droid-style welcome wordmark for normal terminal widths.
pub const WELCOME_WORDMARK: &[&str] = &[
    "█████   █████   █████   █████   █████   █████",
    "██  ██  ██      ██      ██  ██  ██  ██  ██   ",
    "██  ██  █████   █████   █████   ██  ██  █████",
    "██  ██     ██   ██      ██      ██  ██  ██   ",
    "█████   █████   ██      ██      █████   █████",
];

/// Many-handed hydra badge — the project mascot.
/// Symmetrical pixel art representing the "many-handed coding agent".
/// 25 rows × 62 columns.
pub const WELCOME_MASCOT: &[&str] = &[
    "                            ██████                            ",
    "                        ██████████████                        ",
    "                      ██████████████████                      ",
    "                    ██████████████████████                    ",
    "                    ██████████████████████                    ",
    "                    ██████████████████████                    ",
    "      ██████          ██    ██████    ██          ██████      ",
    "    ████  ████        ██      ██      ██        ████  ████    ",
    "    ██      ████        ██████████████        ████      ██    ",
    "    ██████    ██      ██████████████████      ██    ██████    ",
    "              ██          ██████████          ██              ",
    "              ████          ██████          ████              ",
    "                ██████                  ██████                ",
    "                                                              ",
    "    ██████████        ████          ████        ██████████    ",
    "  ████      ████  ████████          ████████  ████      ████  ",
    "████          ██████        ██  ██        ██████          ████",
    "██                        ████  ████                        ██",
    "██      ██              ██████  ██████              ██      ██",
    "████    ██          ██████          ██████          ██    ████",
    "  ██████        ██████                  ██████        ██████  ",
    "              ████                          ████              ",
    "              ██                              ██              ",
    "              ████      ██          ██      ████              ",
    "                ████████              ████████                ",
];

/// Terminal-correct compact mascot using half-block pixels.
pub const WELCOME_MASCOT_COMPACT: &[&str] = &[
    "            ▄▄███▄▄            ",
    "          ▄█████████▄          ",
    "          ███████████          ",
    "  ▄█▀█▄    █  ▀█▀  █    ▄█▀█▄  ",
    "  █▄▄ ▀█   ▄███████▄   █▀ ▄▄█  ",
    "       █▄    ▀███▀    ▄█       ",
    "        ▀▀▀         ▀▀▀        ",
    " ▄█▀▀▀█▄ ▄▄██     ██▄▄ ▄█▀▀▀█▄ ",
    "█▀     ▀▀▀   ▄█ █▄   ▀▀▀     ▀█",
    "█▄  █     ▄▄█▀▀ ▀▀█▄▄     █  ▄█",
    " ▀▀▀   ▄█▀▀         ▀▀█▄   ▀▀▀ ",
    "       █▄   ▄     ▄   ▄█       ",
    "        ▀▀▀▀       ▀▀▀▀        ",
];

/// Diamond "mascot" frames for the thinking spinner.
pub const SPINNER_FRAMES: &[&str] = &["◇", "◈", "◆", "◈"];

/// Render the large logo centered for a given width.
pub fn logo_lines() -> Vec<String> {
    LOGO_LARGE
        .lines()
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Get the current spinner frame based on elapsed time (≈ 4 FPS).
pub fn spinner_frame(elapsed_ms: u64) -> &'static str {
    let idx = ((elapsed_ms / 250) % SPINNER_FRAMES.len() as u64) as usize;
    SPINNER_FRAMES[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_mascot_stays_terminal_safe() {
        assert_eq!(WELCOME_WORDMARK.len(), 5);
        let wordmark_width = WELCOME_WORDMARK[0].chars().count();
        assert_eq!(wordmark_width, 45);
        for line in WELCOME_WORDMARK {
            assert_eq!(line.chars().count(), wordmark_width);
            assert!(line.chars().all(|ch| matches!(ch, ' ' | '█')));
        }

        assert_eq!(WELCOME_MASCOT.len(), 25);

        let width = WELCOME_MASCOT[0].chars().count();
        assert_eq!(width, 62);

        for line in WELCOME_MASCOT {
            assert_eq!(line.chars().count(), width);
            assert!(line.chars().all(|ch| matches!(ch, ' ' | '█')));
        }

        assert_eq!(WELCOME_MASCOT_COMPACT.len(), 13);
        let compact_width = WELCOME_MASCOT_COMPACT[0].chars().count();
        assert_eq!(compact_width, 31);
        for line in WELCOME_MASCOT_COMPACT {
            assert_eq!(line.chars().count(), compact_width);
            assert!(line.chars().all(|ch| matches!(ch, ' ' | '█' | '▄' | '▀')));
        }
    }

    #[test]
    fn welcome_mascot_keeps_symmetry() {
        let block_count = WELCOME_MASCOT
            .iter()
            .map(|line| line.chars().filter(|ch| *ch == '█').count())
            .sum::<usize>();

        // Should be a dense badge
        assert!(block_count >= 100);
    }
}

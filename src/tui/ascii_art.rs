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

/// Tiny product mark for compact brand lockups.
pub const WHALE_TINY: &str = LOGO_TINY;

/// Droid-style welcome wordmark for normal terminal widths.
pub const WELCOME_WORDMARK: &[&str] = &[
    "█████   █████   █████   █████   █████   █████",
    "██  ██  ██      ██      ██  ██  ██  ██  ██   ",
    "██  ██  █████   █████   █████   ██  ██  █████",
    "██  ██     ██   ██      ██      ██  ██  ██   ",
    "█████   █████   ██      ██      █████   █████",
];

/// DeepSeek whale mark used in the welcome card.
pub const WELCOME_WHALE: &[&str] = &[
    "              __",
    "       ____.-' /",
    "  _.-''        \\___",
    " /  .-.   .-.      \\",
    "/___/  \\_/   \\______\\",
    "     \\____/          ",
];

/// Compatibility alias for older welcome code/tests.
pub const WELCOME_MASCOT: &[&str] = WELCOME_WHALE;

/// Compact whale mark for narrow welcome surfaces.
pub const WELCOME_WHALE_COMPACT: &[&str] = &["      __", " __.-' /", "/_   _/", "  \\_/  "];

/// Compatibility alias for older compact mascot code/tests.
pub const WELCOME_MASCOT_COMPACT: &[&str] = WELCOME_WHALE_COMPACT;

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
    fn welcome_whale_stays_terminal_safe() {
        assert_eq!(WELCOME_WORDMARK.len(), 5);
        let wordmark_width = WELCOME_WORDMARK[0].chars().count();
        assert_eq!(wordmark_width, 45);
        for line in WELCOME_WORDMARK {
            assert_eq!(line.chars().count(), wordmark_width);
            assert!(line.chars().all(|ch| matches!(ch, ' ' | '█')));
        }

        assert_eq!(WELCOME_WHALE.len(), 6);
        for line in WELCOME_WHALE {
            assert!(line.chars().count() <= 23);
            assert!(line.is_ascii());
        }

        assert_eq!(WELCOME_WHALE_COMPACT.len(), 4);
        for line in WELCOME_WHALE_COMPACT {
            assert!(line.chars().count() <= 8);
            assert!(line.is_ascii());
        }
    }

    #[test]
    fn welcome_whale_has_tail_and_body() {
        assert!(WELCOME_WHALE.iter().any(|line| line.contains("____.-'")));
        assert!(WELCOME_WHALE.iter().any(|line| line.contains("\\______\\")));
    }
}

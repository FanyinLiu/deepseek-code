//! ASCII art wordmarks for the TUI.

/// A compact one-line logo for dense status surfaces.
pub const LOGO_TINY: &str = "◆";

/// Tiny product mark for compact brand lockups.
pub const WHALE_TINY: &str = LOGO_TINY;

/// DeepSeek whale mark used in compact welcome lockups.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_whale_stays_terminal_safe() {
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
}

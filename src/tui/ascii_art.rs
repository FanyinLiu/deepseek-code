//! ASCII art wordmarks for the TUI.

/// A compact one-line logo for dense status surfaces.
pub const LOGO_TINY: &str = "DS";

/// Tiny product mark for compact brand lockups.
pub const DSCODE_TINY: &str = LOGO_TINY;

/// DS-CODE wordmark used in welcome lockups.
pub const WELCOME_DSCODE: &[&str] = &[
    " ____  ____        ____ ___  ____  _____",
    "|  _ \\/ ___|      / ___/ _ \\|  _ \\| ____|",
    "| | | \\___ \\_____| |  | | | | | | |  _|",
    "| |_| |___) |____| |__| |_| | |_| | |___",
    "|____/|____/      \\____\\___/|____/|_____|",
];

/// Compact DS-CODE mark for narrow welcome surfaces.
pub const WELCOME_DSCODE_COMPACT: &[&str] = &["DS-CODE"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_dscode_stays_terminal_safe() {
        assert_eq!(WELCOME_DSCODE.len(), 5);
        for line in WELCOME_DSCODE {
            assert!(line.chars().count() <= 42);
            assert!(line.is_ascii());
        }

        assert_eq!(WELCOME_DSCODE_COMPACT.len(), 1);
        for line in WELCOME_DSCODE_COMPACT {
            assert!(line.chars().count() <= 8);
            assert!(line.is_ascii());
        }
    }
}

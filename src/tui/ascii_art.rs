//! ASCII art wordmarks for the TUI.

/// Droid-style welcome wordmark for normal terminal widths.
pub const WELCOME_WORDMARK: &[&str] = &[
    "█████   █████   █████   █████   █████   █████",
    "██  ██  ██      ██      ██  ██  ██  ██  ██   ",
    "██  ██  █████   █████   █████   ██  ██  █████",
    "██  ██     ██   ██      ██      ██  ██  ██   ",
    "█████   █████   ██      ██      █████   █████",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_wordmark_stays_terminal_safe() {
        assert_eq!(WELCOME_WORDMARK.len(), 5);
        let wordmark_width = WELCOME_WORDMARK[0].chars().count();
        assert_eq!(wordmark_width, 45);
        for line in WELCOME_WORDMARK {
            assert_eq!(line.chars().count(), wordmark_width);
            assert!(line.chars().all(|ch| matches!(ch, ' ' | '█')));
        }
    }
}

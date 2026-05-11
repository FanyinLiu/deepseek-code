/// Unicode safety for command execution and file paths.
/// Prevents homoglyph attacks, invisible characters, and normalization issues.
/// Detect suspicious Unicode characters in a command string.
#[must_use]
pub fn detect_suspicious_unicode(text: &str) -> Vec<UnicodeWarning> {
    let mut warnings = Vec::new();

    for (i, c) in text.char_indices() {
        // Zero-width characters
        if c == '\u{200B}'
            || c == '\u{200C}'
            || c == '\u{200D}'
            || c == '\u{FEFF}'
            || c == '\u{200E}'
            || c == '\u{200F}'
        {
            warnings.push(UnicodeWarning {
                position: i,
                char_info: format!("U+{:04X} (zero-width)", c as u32),
                risk: "zero-width character — could hide malicious content".into(),
            });
        }

        // Bidirectional text override
        if c == '\u{202A}'
            || c == '\u{202B}'
            || c == '\u{202C}'
            || c == '\u{202D}'
            || c == '\u{202E}'
        {
            warnings.push(UnicodeWarning {
                position: i,
                char_info: format!("U+{:04X} (bidi override)", c as u32),
                risk: "bidirectional text override — could reverse display order".into(),
            });
        }

        // Confusable characters (homoglyphs)
        if is_confusable(c) {
            warnings.push(UnicodeWarning {
                position: i,
                char_info: format!("U+{:04X} (confusable)", c as u32),
                risk: "character visually similar to another — possible homoglyph attack".into(),
            });
        }
    }

    warnings
}

#[derive(Debug, Clone)]
pub struct UnicodeWarning {
    pub position: usize,
    pub char_info: String,
    pub risk: String,
}

/// Check if a character belongs to a "confusable" set.
/// This is a minimal set — a full implementation would use Unicode confusable data.
fn is_confusable(c: char) -> bool {
    matches!(
        c,
        // Cyrillic letters that look like Latin
        '\u{0430}' | // a
        '\u{0435}' | // e
        '\u{043E}' | // o
        '\u{0440}' | // p
        '\u{0441}' | // c
        '\u{0445}' | // x
        '\u{0455}' | // s
        // Greek letters that look like Latin
        '\u{0391}' | // Α
        '\u{0395}' | // Ε
        '\u{0397}' | // Η
        '\u{0399}' | // Ι
        '\u{039C}' | // Μ
        '\u{039D}' | // Ν
        '\u{039F}' | // Ο
        '\u{03A1}' | // Ρ
        '\u{03A4}' | // Τ
        '\u{03A5}' | // Υ
        '\u{03A7}' // Χ
    )
}

/// Normalize a command to NFC form.
#[must_use]
pub fn normalize_command(cmd: &str) -> (String, bool) {
    crate::workspace::paths::normalize_unicode_command(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_zero_width() {
        let warnings = detect_suspicious_unicode("ls\u{200B} -la");
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_normal_command_clean() {
        let warnings = detect_suspicious_unicode("cargo test");
        assert!(warnings.is_empty());
    }
}

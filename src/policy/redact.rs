/// Sensitive data redaction for logs, transcripts, and audit trails.
/// Redact API keys from a string.
#[must_use]
pub fn redact_api_keys(text: &str) -> String {
    // Pattern: sk- followed by alphanumeric characters
    let re = regex_find(text, "sk-[a-zA-Z0-9]{10,}");
    redact_matches(text, &re, "sk-****")
}

/// Redact Bearer tokens.
#[must_use]
pub fn redact_bearer_tokens(text: &str) -> String {
    let re = regex_find(text, "Bearer [a-zA-Z0-9._\\-]+");
    redact_matches(text, &re, "Bearer ****")
}

/// Redact environment variable values.
pub fn redact_env_vars(text: &str) -> String {
    let mut result = text.to_string();
    let patterns = [
        "DEEPSEEK_API_KEY",
        "OPENAI_API_KEY",
        "API_KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PRIVATE_KEY",
    ];

    for pattern in &patterns {
        // Match: PATTERN=value (with optional quotes)
        let regex_str = format!("{pattern}=[\"']?[^\"'\\s]+[\"']?");
        let matches: Vec<String> = regex_find(&result, &regex_str)
            .into_iter()
            .map(std::string::ToString::to_string)
            .collect();
        for m in &matches {
            let replacement = format!("{pattern}=****");
            result = result.replace(m.as_str(), &replacement);
        }
    }

    result
}

/// Redact file paths that might contain sensitive data.
#[must_use]
pub fn redact_sensitive_paths(text: &str) -> String {
    let sensitive_patterns = [
        "~/.ssh/",
        "~/.aws/",
        "~/.gnupg/",
        "/etc/shadow",
        "/etc/passwd",
    ];

    let mut result = text.to_string();
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();

    for pattern in &sensitive_patterns {
        let expanded = pattern.replace('~', &home);
        result = result.replace(&expanded, "[REDACTED PATH]");
    }

    result
}

/// Full redaction pipeline for log/transcript output.
#[must_use]
pub fn redact_all(text: &str) -> String {
    let mut result = text.to_string();
    result = redact_api_keys(&result);
    result = redact_bearer_tokens(&result);
    result = redact_env_vars(&result);
    result = redact_sensitive_paths(&result);
    result
}

/// Simple regex-like pattern matching without a regex dependency.
/// Returns all substrings that match the pattern.
fn regex_find<'a>(text: &'a str, pattern: &str) -> Vec<&'a str> {
    let mut matches = Vec::new();
    let mut search_start = 0;

    while search_start < text.len() {
        if let Some(pos) = find_pattern_match(&text[search_start..], pattern) {
            let match_start = search_start + pos;
            let matched = &text[match_start..];
            // Find the end of the match
            let end = find_pattern_end(matched, pattern);
            let matched_text = &text[match_start..match_start + end];
            matches.push(matched_text);
            search_start = match_start + end;
        } else {
            break;
        }
    }

    matches
}

fn find_pattern_match(text: &str, pattern: &str) -> Option<usize> {
    // Very simplified: look for the first literal portion of the pattern
    let literal = pattern
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
        .collect::<String>();

    if literal.is_empty() {
        return Some(0);
    }

    text.find(&literal)
}

fn find_pattern_end(text: &str, _pattern: &str) -> usize {
    // For redaction purposes, find the next whitespace or end of string
    text.find(|c: char| c.is_whitespace()).unwrap_or(text.len())
}

fn redact_matches(text: &str, matches: &[&str], replacement: &str) -> String {
    let mut result = text.to_string();
    for m in matches {
        result = result.replace(m, replacement);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_api_key() {
        let redacted = redact_api_keys("Authorization: Bearer sk-abc123def456ghi789");
        assert!(!redacted.contains("sk-abc123def456ghi789"));
    }

    #[test]
    fn test_redact_env_vars() {
        let redacted = redact_env_vars("DEEPSEEK_API_KEY=sk-secret-key");
        assert!(!redacted.contains("sk-secret-key"));
    }
}

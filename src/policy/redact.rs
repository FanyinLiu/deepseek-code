/// Sensitive data redaction for logs, transcripts, and audit trails.
/// Redact API keys from a string.
#[must_use]
pub fn redact_api_keys(text: &str) -> String {
    replace_regex(text, r"\bsk-[A-Za-z0-9][A-Za-z0-9_-]{9,}\b", "sk-****")
}

/// Redact Bearer tokens.
#[must_use]
pub fn redact_bearer_tokens(text: &str) -> String {
    replace_regex(text, r"(?i)\bBearer\s+[A-Za-z0-9._\-]+", "Bearer ****")
}

/// Redact environment variable values.
pub fn redact_env_vars(text: &str) -> String {
    let mut result = text.to_string();
    let patterns = [
        "DEEPSEEK_API_KEY",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "DASHSCOPE_API_KEY",
        "BAILIAN_API_KEY",
        "QWEN_API_KEY",
        "MOONSHOT_API_KEY",
        "KIMI_API_KEY",
        "ZAI_API_KEY",
        "ZHIPUAI_API_KEY",
        "ZHIPU_API_KEY",
        "API_KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PRIVATE_KEY",
    ];

    for pattern in &patterns {
        let regex = regex::Regex::new(&format!(
            r#"(?i)\b{}=(?:"[^"]*"|'[^']*'|[^\s"'=]+)"#,
            regex::escape(pattern)
        ))
        .expect("redaction regex compiles");
        result = regex
            .replace_all(&result, format!("{pattern}=****"))
            .into_owned();
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

fn replace_regex(text: &str, pattern: &str, replacement: &str) -> String {
    regex::Regex::new(pattern)
        .expect("redaction regex compiles")
        .replace_all(text, replacement)
        .into_owned()
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

    #[test]
    fn redaction_does_not_replace_plain_words_containing_sk() {
        assert_eq!(redact_api_keys("run risky task"), "run risky task");
    }

    #[test]
    fn redaction_handles_quoted_env_values_with_spaces() {
        let redacted = redact_env_vars(r#"PASSWORD="abc def" TOKEN='secret token'"#);

        assert_eq!(redacted, "PASSWORD=**** TOKEN=****");
        assert!(!redacted.contains("abc def"));
        assert!(!redacted.contains("secret token"));
    }

    #[test]
    fn redaction_handles_bearer_tokens_without_overmatching_neighbors() {
        let redacted = redact_bearer_tokens("Authorization: Bearer abc.def-123 next");

        assert_eq!(redacted, "Authorization: Bearer **** next");
    }
}

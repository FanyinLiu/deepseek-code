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

/// Redact environment variable values. Catches four wire formats:
///   `FOO=value`            (Bourne / `.env`)
///   `set FOO=value`        (cmd.exe — `set` is consumed by the regex prefix)
///   `$env:FOO = "value"`   (PowerShell)
///   `export FOO=value`     (Bourne explicit export)
///
/// The value side is captured to end-of-token so multi-arg cmdlines don't
/// over-eat siblings.
pub fn redact_env_vars(text: &str) -> String {
    let mut result = text.to_string();
    let patterns = [
        "DEEPSEEK_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "OPENROUTER_API_KEY",
        "DASHSCOPE_API_KEY",
        "BAILIAN_API_KEY",
        "QWEN_API_KEY",
        "MOONSHOT_API_KEY",
        "KIMI_API_KEY",
        "ZAI_API_KEY",
        "ZHIPUAI_API_KEY",
        "ZHIPU_API_KEY",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "API_KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PRIVATE_KEY",
    ];

    for pattern in &patterns {
        // Posix / dotenv: `FOO=value`
        let regex = regex::Regex::new(&format!(
            r#"(?i)\b{}=(?:"[^"]*"|'[^']*'|[^\s"'=]+)"#,
            regex::escape(pattern)
        ))
        .expect("redaction regex compiles");
        result = regex
            .replace_all(&result, format!("{pattern}=****"))
            .into_owned();

        // PowerShell: `$env:FOO = "value"`
        let ps_regex = regex::Regex::new(&format!(
            r#"(?i)\$env:{}\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=]+)"#,
            regex::escape(pattern)
        ))
        .expect("redaction regex compiles");
        result = ps_regex
            .replace_all(&result, format!("$env:{pattern}=****"))
            .into_owned();
    }

    result
}

/// Redact secrets that carry a distinctive, self-identifying prefix or
/// envelope, so they are caught even when they appear bare (not as
/// `NAME=value`) — e.g. inside a git remote URL or pasted into output.
#[must_use]
pub fn redact_known_token_formats(text: &str) -> String {
    let mut result = text.to_string();
    // GitHub tokens: ghp_/gho_/ghu_/ghs_/ghr_ and fine-grained github_pat_.
    result = replace_regex(&result, r"\bgh[pousr]_[A-Za-z0-9]{20,}\b", "gh*_****");
    result = replace_regex(
        &result,
        r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
        "github_pat_****",
    );
    // AWS access key IDs (long-term AKIA, temporary ASIA).
    result = replace_regex(&result, r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", "AKIA****");
    // PEM private key blocks (RSA/EC/OPENSSH/generic).
    result = replace_regex(
        &result,
        r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
        "[REDACTED PRIVATE KEY]",
    );
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
    result = redact_known_token_formats(&result);
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

    #[test]
    fn redacts_powershell_env_assignment() {
        let redacted = redact_env_vars(r#"$env:DEEPSEEK_API_KEY = "sk-abc123""#);
        assert!(!redacted.contains("sk-abc123"));
        assert!(redacted.contains("****"));
    }

    #[test]
    fn redacts_bare_github_token_in_remote_url() {
        // A PAT embedded in a git remote URL has no NAME=value envelope.
        let redacted =
            redact_all("origin https://ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123@github.com/x/y");
        assert!(!redacted.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"));
        assert!(redacted.contains("github.com/x/y"));
    }

    #[test]
    fn redacts_bare_aws_access_key_id() {
        let redacted = redact_all("key id AKIAIOSFODNN7EXAMPLE in the logs");
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn redacts_pem_private_key_block() {
        let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\nsecretmaterial\n-----END OPENSSH PRIVATE KEY-----";
        let redacted = redact_all(pem);
        assert!(!redacted.contains("secretmaterial"));
        assert!(redacted.contains("[REDACTED PRIVATE KEY]"));
    }

    #[test]
    fn does_not_redact_benign_text_resembling_tokens() {
        // No false positives on ordinary words / short ids.
        let text = "the ghost wrote a haiku about asia";
        assert_eq!(redact_known_token_formats(text), text);
    }

    #[test]
    fn redacts_anthropic_and_github_tokens() {
        let redacted = redact_env_vars("ANTHROPIC_API_KEY=sk-ant-xxx GITHUB_TOKEN=ghp_yyy");
        assert!(!redacted.contains("sk-ant-xxx"));
        assert!(!redacted.contains("ghp_yyy"));
        assert!(redacted.matches("****").count() >= 2);
    }
}

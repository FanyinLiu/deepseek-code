use std::path::Path;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

/// Execute a `RunCommand` tool call. Requires policy approval before calling.
pub async fn run_command(
    project_root: &Path,
    command: &str,
    cwd: Option<&str>,
    timeout_seconds: u64,
) -> Result<CommandResult, anyhow::Error> {
    let working_dir = match cwd {
        Some(dir) => crate::workspace::paths::resolve_workspace_path(project_root, dir)
            .ok_or_else(|| anyhow::anyhow!("cwd not in workspace: {dir}"))?,
        None => project_root.to_path_buf(),
    };

    let start = Instant::now();

    // Safety checks
    const MAX_COMMAND_LEN: usize = 4096;
    if command.len() > MAX_COMMAND_LEN {
        return Ok(CommandResult {
            stdout: String::new(),
            stderr: format!("command exceeds maximum length of {MAX_COMMAND_LEN} characters"),
            exit_code: -1,
            duration_ms: 0,
            timed_out: false,
        });
    }

    if let Some(reason) = crate::policy::commands::contains_dangerous_pattern(command) {
        return Ok(CommandResult {
            stdout: String::new(),
            stderr: format!("dangerous command blocked: {reason}"),
            exit_code: -1,
            duration_ms: 0,
            timed_out: false,
        });
    }

    // Use shell to execute (cross-platform)
    let (shell, shell_arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let mut child = match Command::new(shell)
        .arg(shell_arg)
        .arg(command)
        .current_dir(&working_dir)
        .env_clear()
        .envs(sanitized_command_env())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Ok(CommandResult {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: -1,
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: false,
            });
        }
    };

    // Spawn background tasks to drain stdout/stderr so the pipe doesn't back-pressure.
    const MAX_OUTPUT_BYTES: u64 = 1024 * 1024; // 1 MiB

    let stdout_handle = child.stdout.take().map(|stdout| {
        tokio::spawn(async move {
            let mut limited = stdout.take(MAX_OUTPUT_BYTES);
            let mut buf = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut limited, &mut buf).await;
            buf
        })
    });

    let stderr_handle = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut limited = stderr.take(MAX_OUTPUT_BYTES);
            let mut buf = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut limited, &mut buf).await;
            buf
        })
    });

    tokio::select! {
        status = child.wait() => {
            let status = status?;
            let stdout = match stdout_handle {
                Some(h) => String::from_utf8_lossy(&h.await.unwrap_or_default()).to_string(),
                None => String::new(),
            };
            let stderr = match stderr_handle {
                Some(h) => String::from_utf8_lossy(&h.await.unwrap_or_default()).to_string(),
                None => String::new(),
            };
            Ok(CommandResult {
                stdout,
                stderr,
                exit_code: status.code().unwrap_or(-1),
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: false,
            })
        }
        () = sleep(Duration::from_secs(timeout_seconds)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            if let Some(h) = stdout_handle { h.abort(); }
            if let Some(h) = stderr_handle { h.abort(); }
            Ok(CommandResult {
                stdout: String::new(),
                stderr: format!("command timed out after {timeout_seconds}s"),
                exit_code: -1,
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: true,
            })
        }
    }
}

fn sanitized_command_env() -> Vec<(String, String)> {
    sanitized_command_env_from(std::env::vars())
}

pub(crate) fn sanitized_command_env_from<I, K, V>(vars: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    vars.into_iter()
        .filter_map(|(key, value)| {
            let key = key.into();
            let value = value.into();
            (!is_sensitive_command_env_key(&key)).then_some((key, value))
        })
        .collect()
}

fn is_sensitive_command_env_key(key: &str) -> bool {
    let normalized = key.to_ascii_uppercase();
    const SENSITIVE_PATTERNS: &[&str] = &[
        "API_KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "AUTH",
        "PRIVATE_KEY",
        "ACCESS_KEY",
        "SESSION_KEY",
        "OPENAI",
        "DEEPSEEK",
        "GITHUB",
        "ANTHROPIC",
        "MCP",
    ];

    SENSITIVE_PATTERNS
        .iter()
        .any(|pattern| normalized.contains(pattern))
}

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub timed_out: bool,
}

impl CommandResult {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.exit_code == 0 && !self.timed_out
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = String::new();
        if !self.stdout.is_empty() {
            out.push_str(&format!("stdout:\n{}\n", self.stdout));
        }
        if !self.stderr.is_empty() {
            out.push_str(&format!("stderr:\n{}\n", self.stderr));
        }
        out.push_str(&format!(
            "exit_code: {} | duration: {}ms",
            self.exit_code, self.duration_ms
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dangerous_command_blocked() {
        let result = run_command(std::path::Path::new("."), "rm -rf /", None, 120)
            .await
            .unwrap();
        assert!(!result.is_success());
        assert!(result.stderr.contains("dangerous command blocked"));
    }

    #[tokio::test]
    async fn test_command_too_long_blocked() {
        let long_cmd = "echo ".repeat(1000);
        let result = run_command(std::path::Path::new("."), &long_cmd, None, 120)
            .await
            .unwrap();
        assert!(!result.is_success());
        assert!(result.stderr.contains("exceeds maximum length"));
    }

    #[tokio::test]
    async fn test_safe_command_runs() {
        let result = run_command(std::path::Path::new("."), "echo hello", None, 120)
            .await
            .unwrap();
        assert!(result.is_success());
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn command_env_drops_sensitive_tokens() {
        let env = sanitized_command_env_from([
            ("PATH", "/usr/bin"),
            ("OPENAI_API_KEY", "sk-secret"),
            ("GITHUB_TOKEN", "ghp-secret"),
            ("DEEPSEEK_API_KEY", "ds-secret"),
            ("HOME", "/tmp/home"),
            ("SDKROOT", "/Applications/Xcode.app/SDKs/MacOSX.sdk"),
            ("PKG_CONFIG_PATH", "/opt/homebrew/lib/pkgconfig"),
            ("HTTPS_PROXY", "http://proxy.local:8080"),
        ]);

        assert!(env.iter().any(|(key, _)| key == "PATH"));
        assert!(env.iter().any(|(key, _)| key == "HOME"));
        assert!(env.iter().any(|(key, _)| key == "SDKROOT"));
        assert!(env.iter().any(|(key, _)| key == "PKG_CONFIG_PATH"));
        assert!(env.iter().any(|(key, _)| key == "HTTPS_PROXY"));
        assert!(!env.iter().any(|(key, _)| key.contains("TOKEN")));
        assert!(!env.iter().any(|(key, _)| key.contains("API_KEY")));
    }
}

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
}

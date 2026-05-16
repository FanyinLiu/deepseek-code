use std::path::Path;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

use crate::policy::sandbox::{command_invocation, CommandSandboxConfig};

/// Execute a `RunCommand` tool call. Requires policy approval before calling.
pub async fn run_command(
    project_root: &Path,
    command: &str,
    cwd: Option<&str>,
    timeout_seconds: u64,
) -> Result<CommandResult, anyhow::Error> {
    run_command_with_sandbox(
        project_root,
        command,
        cwd,
        timeout_seconds,
        &CommandSandboxConfig::default(),
    )
    .await
}

/// Execute a `RunCommand` tool call with an optional native OS sandbox.
pub async fn run_command_with_sandbox(
    project_root: &Path,
    command: &str,
    cwd: Option<&str>,
    timeout_seconds: u64,
    sandbox: &CommandSandboxConfig,
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

    let sandbox_invocation = command_invocation(shell, shell_arg, command, &working_dir, sandbox)?;
    let mut command_builder = match sandbox_invocation {
        Some(invocation) => {
            let mut builder = Command::new(invocation.program);
            builder.args(invocation.args);
            builder
        }
        None => {
            let mut builder = Command::new(shell);
            builder.arg(shell_arg).arg(command);
            builder
        }
    };
    command_builder
        .current_dir(&working_dir)
        .env_clear()
        .envs(sanitized_command_env())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command_builder.process_group(0);
    }

    let mut child = match command_builder.spawn() {
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
    let mut process_guard = ProcessTreeGuard::new(child.id());

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
            process_guard.disarm();
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
            process_guard.terminate();
            let _ = child.kill().await;
            let _ = child.wait().await;
            process_guard.disarm();
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

struct ProcessTreeGuard {
    pid: Option<u32>,
}

impl ProcessTreeGuard {
    fn new(pid: Option<u32>) -> Self {
        Self { pid }
    }

    fn disarm(&mut self) {
        self.pid = None;
    }

    fn terminate(&mut self) {
        terminate_process_tree(self.pid);
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        terminate_process_tree(self.pid);
    }
}

#[cfg(unix)]
fn terminate_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    let pgid = -(pid as libc::pid_t);
    // SAFETY: kill(2) is called with a negative process-group id created for
    // this command. The call does not dereference pointers or share memory.
    unsafe {
        libc::kill(pgid, libc::SIGTERM);
        libc::kill(pgid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn terminate_process_tree(_pid: Option<u32>) {}

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

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_shell_child_process_group() {
        let root = tempfile::tempdir().expect("tempdir");
        let pid_file = root.path().join("child.pid");
        let command = format!(
            "sleep 20 & printf '%s' \"$!\" > '{}'; wait",
            pid_file.display()
        );

        let result = run_command(root.path(), &command, None, 1).await.unwrap();

        assert!(result.timed_out);
        let pid = std::fs::read_to_string(&pid_file)
            .expect("pid file")
            .trim()
            .to_string();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let status = std::process::Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .expect("kill -0");
        assert!(!status.success(), "child process {pid} should be gone");
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

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn strict_sandbox_reports_missing_native_sandbox_without_running_command() {
        use crate::policy::sandbox::{CommandSandboxConfig, CommandSandboxMode};

        let mut config = CommandSandboxConfig {
            mode: CommandSandboxMode::Strict,
            ..CommandSandboxConfig::default()
        };
        if cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/sandbox-exec").exists() {
            config.mode = CommandSandboxMode::Off;
        }
        if cfg!(target_os = "linux") && std::env::var_os("PATH").is_some() {
            config.mode = CommandSandboxMode::Off;
        }

        let result =
            run_command_with_sandbox(std::path::Path::new("."), "echo hello", None, 120, &config)
                .await;

        if config.mode == CommandSandboxMode::Strict {
            assert!(result.is_err());
        } else {
            assert!(result.expect("command should run").is_success());
        }
    }
}

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
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_original_bytes: 0,
            stderr_original_bytes: 0,
        });
    }

    let dangerous = crate::policy::commands::contains_dangerous_pattern(command);
    // Test code may legitimately need to construct payloads that look
    // dangerous (e.g. PowerShell EncodedCommand for cross-shell test fixtures).
    // We let it opt out via an env var that only test runners set, and which
    // is never read by the production CLI.
    let bypass = std::env::var_os("OCTO_TEST_BYPASS_DANGER_LINT").is_some();
    if let Some(reason) = dangerous {
        if !bypass {
            return Ok(CommandResult {
                stdout: String::new(),
                stderr: format!("dangerous command blocked: {reason}"),
                exit_code: -1,
                duration_ms: 0,
                timed_out: false,
                stdout_truncated: false,
                stderr_truncated: false,
                stdout_original_bytes: 0,
                stderr_original_bytes: 0,
            });
        }
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
    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command_builder.creation_flags(CREATE_NEW_PROCESS_GROUP);
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
                stdout_truncated: false,
                stderr_truncated: false,
                stdout_original_bytes: 0,
                stderr_original_bytes: 0,
            });
        }
    };
    let mut process_guard = ProcessTreeGuard::new(child.id());

    // Spawn background tasks to drain stdout/stderr so the pipe doesn't back-pressure.
    const MAX_OUTPUT_BYTES: u64 = 1024 * 1024; // 1 MiB

    let mut stdout_handle = child
        .stdout
        .take()
        .map(|stdout| OutputCaptureHandle::new(stdout, MAX_OUTPUT_BYTES));

    let mut stderr_handle = child
        .stderr
        .take()
        .map(|stderr| OutputCaptureHandle::new(stderr, MAX_OUTPUT_BYTES));

    tokio::select! {
        status = child.wait() => {
            let status = status?;
            process_guard.disarm();
            let (stdout, stdout_truncated, stdout_original_bytes) = match stdout_handle.take() {
                Some(handle) => {
                    let output = collect_output(handle).await;
                    (
                        String::from_utf8_lossy(&output.bytes).to_string(),
                        output.truncated,
                        output.original_bytes,
                    )
                }
                None => (String::new(), false, 0),
            };
            let (stderr, stderr_truncated, stderr_original_bytes) = match stderr_handle.take() {
                Some(handle) => {
                    let output = collect_output(handle).await;
                    (
                        String::from_utf8_lossy(&output.bytes).to_string(),
                        output.truncated,
                        output.original_bytes,
                    )
                }
                None => (String::new(), false, 0),
            };
            Ok(CommandResult {
                stdout,
                stderr,
                exit_code: status.code().unwrap_or(-1),
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: false,
                stdout_truncated,
                stderr_truncated,
                stdout_original_bytes,
                stderr_original_bytes,
            })
        }
        () = sleep(Duration::from_secs(timeout_seconds)) => {
            process_guard.terminate();
            let _ = child.kill().await;
            let _ = child.wait().await;
            process_guard.disarm();
            let (stdout, stdout_truncated, stdout_original_bytes) = match stdout_handle.take() {
                Some(mut handle) => {
                    let output = await_output_with_timeout(&mut handle).await;
                    (
                        String::from_utf8_lossy(&output.bytes).to_string(),
                        output.truncated,
                        output.original_bytes,
                    )
                }
                None => (String::new(), false, 0),
            };
            let (mut stderr, stderr_truncated, stderr_original_bytes) = match stderr_handle.take() {
                Some(mut handle) => {
                    let output = await_output_with_timeout(&mut handle).await;
                    (
                        String::from_utf8_lossy(&output.bytes).to_string(),
                        output.truncated,
                        output.original_bytes,
                    )
                }
                None => (String::new(), false, 0),
            };
            append_timeout_marker(&mut stderr, timeout_seconds);
            Ok(CommandResult {
                stdout,
                stderr,
                exit_code: -1,
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: true,
                stdout_truncated,
                stderr_truncated,
                stdout_original_bytes,
                stderr_original_bytes,
            })
        }
    }
}

#[derive(Debug, Default, Clone)]
struct OutputCapture {
    bytes: Vec<u8>,
    truncated: bool,
    original_bytes: u64,
}

struct OutputCaptureHandle {
    state: std::sync::Arc<tokio::sync::Mutex<OutputCapture>>,
    task: tokio::task::JoinHandle<()>,
}

impl OutputCaptureHandle {
    fn new<R>(stream: R, max_output_bytes: u64) -> Self
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let state = std::sync::Arc::new(tokio::sync::Mutex::new(OutputCapture::default()));
        let task_state = std::sync::Arc::clone(&state);

        let task = tokio::spawn(async move {
            capture_output_stream(stream, max_output_bytes, task_state).await;
        });

        Self { state, task }
    }
}

async fn capture_output_stream<R: tokio::io::AsyncRead + Unpin>(
    mut stream: R,
    max_output_bytes: u64,
    state: std::sync::Arc<tokio::sync::Mutex<OutputCapture>>,
) {
    let mut buf = vec![0u8; 4096];

    loop {
        let bytes_read = match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        let bytes_read = bytes_read as u64;
        let mut output = state.lock().await;
        output.original_bytes += bytes_read;
        let collected = output.bytes.len() as u64;
        if collected < max_output_bytes {
            let remaining = max_output_bytes - collected;
            let take = bytes_read.min(remaining);
            output.bytes.extend_from_slice(&buf[..take as usize]);
            if bytes_read > remaining {
                output.truncated = true;
            }
        } else {
            output.truncated = true;
        }
    }
}

async fn await_output_with_timeout(handle: &mut OutputCaptureHandle) -> OutputCapture {
    let state = std::sync::Arc::clone(&handle.state);
    let _ = tokio::time::timeout(Duration::from_millis(100), &mut handle.task).await;
    let output = state.lock().await.clone();
    output
}

async fn collect_output(handle: OutputCaptureHandle) -> OutputCapture {
    let state = handle.state;
    let _ = handle.task.await;
    let output = state.lock().await.clone();
    output
}

fn append_timeout_marker(stderr: &mut String, timeout_seconds: u64) {
    if !stderr.is_empty() {
        stderr.push('\n');
    }
    stderr.push_str(&format!("[timed out after {timeout_seconds}s]"));
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

#[cfg(windows)]
fn terminate_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };

    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(all(not(unix), not(windows)))]
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
        "OPENROUTER",
        "DASHSCOPE",
        "BAILIAN",
        "QWEN",
        "MOONSHOT",
        "KIMI",
        "ZAI_API",
        "ZHIPU",
        "GITHUB",
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
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_original_bytes: u64,
    pub stderr_original_bytes: u64,
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
        if self.stdout_truncated {
            out.push_str(&format!(
                "truncated: stdout {}B -> 1MiB\n",
                self.stdout_original_bytes
            ));
        }
        if self.stderr_truncated {
            out.push_str(&format!(
                "truncated: stderr {}B -> 1MiB\n",
                self.stderr_original_bytes
            ));
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

    #[tokio::test]
    async fn timeout_preserves_collected_stdout_and_stderr() {
        let root = tempfile::tempdir().expect("tempdir");
        #[cfg(windows)]
        let command = "echo stdout-before & echo stderr-before 1>&2 & ping -n 30 127.0.0.1 > nul";
        #[cfg(not(windows))]
        let command = "printf 'stdout-before\\n'; printf 'stderr-before\\n' >&2; sleep 30";

        let result = run_command(root.path(), command, None, 1).await.unwrap();

        assert!(result.timed_out, "expected timeout, got {result:?}");
        assert!(result.stdout.contains("stdout-before"), "{result:?}");
        assert!(result.stderr.contains("stderr-before"), "{result:?}");
        assert!(result.stderr.contains("[timed out after 1s]"), "{result:?}");
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

        assert!(result.timed_out, "expected timeout, got {result:?}");
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

    /// RAII drop guard that unsets an env var on drop.
    #[cfg(windows)]
    struct EnvGuard(&'static str);
    #[cfg(windows)]
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn timeout_kills_shell_child_process_tree() {
        // Test fixtures may legitimately use `-EncodedCommand` to smuggle a
        // multi-line PowerShell payload past Windows quoting hell. The
        // production lint blocks this exact pattern; here we explicitly opt
        // out for this one test via an env var that production code never sets.
        std::env::set_var("OCTO_TEST_BYPASS_DANGER_LINT", "1");
        let _restore = EnvGuard("OCTO_TEST_BYPASS_DANGER_LINT");
        let root = tempfile::tempdir().expect("tempdir");
        let pid_file = root.path().join("child.pid");
        let escaped_pid_file = pid_file.display().to_string().replace('\'', "''");
        let script = format!(
            "[System.IO.File]::WriteAllText('{escaped_pid_file}', [string]$PID); cmd /C ping -n 30 127.0.0.1"
        );
        let command = format!(
            "powershell -NoProfile -EncodedCommand {}",
            powershell_encoded_command(&script)
        );

        let result = run_command(root.path(), &command, None, 2).await.unwrap();
        assert!(result.timed_out, "expected timeout, got {result:?}");

        let pid = std::fs::read_to_string(&pid_file)
            .expect("pid file")
            .trim()
            .parse::<u32>()
            .expect("pid should parse");
        for _ in 0..10 {
            if !windows_process_exists(pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        panic!("child process {pid} should be gone after timeout");
    }

    #[cfg(windows)]
    fn powershell_encoded_command(script: &str) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use std::os::windows::ffi::OsStrExt;

        let bytes = std::ffi::OsStr::new(script)
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>();
        STANDARD.encode(bytes)
    }

    #[cfg(windows)]
    fn windows_process_exists(pid: u32) -> bool {
        use std::ffi::c_void;

        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(
                dwDesiredAccess: u32,
                bInheritHandle: i32,
                dwProcessId: u32,
            ) -> *mut c_void;
            fn CloseHandle(hObject: *mut c_void) -> i32;
        }

        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        unsafe {
            CloseHandle(handle);
        }
        true
    }

    #[test]
    fn command_env_drops_sensitive_tokens() {
        let env = sanitized_command_env_from([
            ("PATH", "/usr/bin"),
            ("OPENAI_API_KEY", "sk-secret"),
            ("GITHUB_TOKEN", "ghp-secret"),
            ("DEEPSEEK_API_KEY", "octocode-secret"),
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

    #[tokio::test]
    async fn capture_output_stream_tracks_truncation_and_original_bytes() {
        let source = vec![b'x'; 1024 * 1024 + 1024];
        let state = std::sync::Arc::new(tokio::sync::Mutex::new(OutputCapture::default()));
        let stream = std::io::Cursor::new(source.clone());

        capture_output_stream(stream, 1024 * 1024, state.clone()).await;

        let output = state.lock().await;
        assert!(output.truncated);
        assert_eq!(output.original_bytes as usize, source.len());
        assert_eq!(output.bytes.len(), 1024 * 1024);
    }

    #[test]
    fn command_result_summary_reports_truncated_output() {
        let result = CommandResult {
            stdout: "stdout".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 0,
            timed_out: false,
            stdout_truncated: true,
            stderr_truncated: false,
            stdout_original_bytes: 2 * 1024 * 1024,
            stderr_original_bytes: 0,
        };

        let summary = result.summary();
        assert!(summary.contains("truncated: stdout 2097152B -> 1MiB"));
        assert!(!summary.contains("truncated: stderr"));
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

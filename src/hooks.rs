use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::storage::config::HooksConfig;

const HOOK_STDOUT_BYTES: usize = 128 * 1024;
const HOOK_STDERR_BYTES: usize = 128 * 1024;
const HOOK_KILL_WAIT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    SessionStart,
    SessionEnd,
    /// Turn was interrupted (user cancel / abort), distinct from session end.
    Stop,
    /// A subagent task completed (success or failure).
    SubagentStop,
    /// Context compaction is about to run.
    PreCompact,
    /// Noteworthy async event surfaced (plan proposed, approval required, error).
    Notification,
}

impl HookEvent {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserPromptSubmit => "user_prompt_submit",
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::Stop => "stop",
            Self::SubagentStop => "subagent_stop",
            Self::PreCompact => "pre_compact",
            Self::Notification => "notification",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    pub event: HookEvent,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub cwd: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
}

impl HookPayload {
    #[must_use]
    pub fn new(event: HookEvent, session_id: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            event,
            session_id: session_id.into(),
            turn_id: None,
            cwd: cwd.into(),
            tool_call_id: None,
            tool_name: None,
            arguments: None,
            success: None,
            summary: None,
            duration_ms: None,
            changed_files: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookCommandOutcome {
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRunSummary {
    pub event: HookEvent,
    pub outcomes: Vec<HookCommandOutcome>,
}

impl HookRunSummary {
    #[must_use]
    pub fn success(&self) -> bool {
        self.outcomes.iter().all(|outcome| outcome.success)
    }

    #[must_use]
    pub fn brief(&self) -> String {
        self.outcomes
            .iter()
            .map(|outcome| {
                let status = if outcome.success { "ok" } else { "failed" };
                format!("{}: {status}", outcome.command)
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

pub async fn run_hook_commands(
    event: HookEvent,
    commands: &[String],
    payload: &HookPayload,
    cwd: &Path,
    timeout_seconds: u64,
) -> HookRunSummary {
    let mut outcomes = Vec::new();
    for command in commands {
        outcomes.push(run_hook_command(command, event, payload, cwd, timeout_seconds).await);
    }
    HookRunSummary { event, outcomes }
}

#[must_use]
pub fn configured_commands(config: &HooksConfig, event: HookEvent) -> Vec<String> {
    match event {
        HookEvent::UserPromptSubmit => config.user_prompt_submit.clone(),
        HookEvent::PreToolUse => config.pre_tool.clone(),
        HookEvent::PostToolUse => config.post_tool.clone(),
        HookEvent::SessionStart => config.session_start.clone(),
        HookEvent::SessionEnd => {
            // `stop` is a legacy alias kept for backward-compatible configs.
            let mut commands = config.session_end.clone();
            commands.extend(config.stop.clone());
            commands
        }
        HookEvent::Stop => config.turn_stop.clone(),
        HookEvent::SubagentStop => config.subagent_stop.clone(),
        HookEvent::PreCompact => config.pre_compact.clone(),
        HookEvent::Notification => config.notification.clone(),
    }
}

pub async fn run_configured_hooks(
    event: HookEvent,
    config: &HooksConfig,
    payload: &HookPayload,
    cwd: &Path,
    timeout_seconds: u64,
) -> Option<HookRunSummary> {
    let commands = configured_commands(config, event);
    if commands.is_empty() {
        return None;
    }
    Some(run_hook_commands(event, &commands, payload, cwd, timeout_seconds).await)
}

async fn run_hook_command(
    command: &str,
    event: HookEvent,
    payload: &HookPayload,
    cwd: &Path,
    timeout_seconds: u64,
) -> HookCommandOutcome {
    let start = std::time::Instant::now();
    let mut child = shell_command(command);
    child
        .current_dir(cwd)
        .env_clear()
        .envs(crate::tools::run_command::sanitized_command_env_from(
            std::env::vars(),
        ))
        .env("DS_HOOK_EVENT", event.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        child.process_group(0);
    }

    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(error) => {
            return HookCommandOutcome {
                command: command.to_string(),
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("failed to spawn hook: {error}"),
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: false,
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let payload_json = serde_json::to_vec(payload).unwrap_or_default();
        let _ = stdin.write_all(&payload_json).await;
        let _ = stdin.write_all(b"\n").await;
    }

    let child_pid = child.id();
    let timeout = Duration::from_secs(timeout_seconds.max(1));
    let output = match wait_with_limited_hook_output(child, child_pid, timeout).await {
        Ok(HookWaitResult::Output(output)) => output,
        Ok(HookWaitResult::TimedOut) => {
            return HookCommandOutcome {
                command: command.to_string(),
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: "hook timed out".to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: true,
            };
        }
        Err(error) => {
            return HookCommandOutcome {
                command: command.to_string(),
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("failed to wait for hook: {error}"),
                duration_ms: start.elapsed().as_millis() as u64,
                timed_out: false,
            };
        }
    };

    HookCommandOutcome {
        command: command.to_string(),
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: hook_output_text(&output.stdout, output.stdout_truncated, "stdout")
            .trim_end()
            .to_string(),
        stderr: hook_output_text(&output.stderr, output.stderr_truncated, "stderr")
            .trim_end()
            .to_string(),
        duration_ms: start.elapsed().as_millis() as u64,
        timed_out: false,
    }
}

struct LimitedHookOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

enum HookWaitResult {
    Output(LimitedHookOutput),
    TimedOut,
}

async fn wait_with_limited_hook_output(
    mut child: tokio::process::Child,
    child_pid: Option<u32>,
    timeout: Duration,
) -> Result<HookWaitResult, std::io::Error> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture hook stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture hook stderr"))?;
    let stdout_handle = tokio::spawn(read_limited_async_stream(stdout, HOOK_STDOUT_BYTES));
    let stderr_handle = tokio::spawn(read_limited_async_stream(stderr, HOOK_STDERR_BYTES));

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            stdout_handle.abort();
            stderr_handle.abort();
            return Err(error);
        }
        Err(_) => {
            terminate_hook_process_tree(child_pid);
            let _ = tokio::time::timeout(HOOK_KILL_WAIT, child.wait()).await;
            stdout_handle.abort();
            stderr_handle.abort();
            return Ok(HookWaitResult::TimedOut);
        }
    };

    let (stdout, stdout_truncated) = join_reader(stdout_handle, "hook stdout").await?;
    let (stderr, stderr_truncated) = join_reader(stderr_handle, "hook stderr").await?;

    Ok(HookWaitResult::Output(LimitedHookOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    }))
}

async fn join_reader(
    handle: tokio::task::JoinHandle<Result<(Vec<u8>, bool), std::io::Error>>,
    name: &str,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    handle
        .await
        .map_err(|error| std::io::Error::other(format!("{name} reader panicked: {error}")))?
}

async fn read_limited_async_stream<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut collected = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        let remaining = max_bytes.saturating_sub(collected.len());
        let keep = bytes_read.min(remaining);
        if keep > 0 {
            collected.extend_from_slice(&buffer[..keep]);
        }
        if keep < bytes_read || collected.len() >= max_bytes {
            truncated = true;
        }
    }

    Ok((collected, truncated))
}

fn hook_output_text(bytes: &[u8], truncated: bool, stream: &str) -> String {
    let mut text = String::from_utf8_lossy(bytes).to_string();
    if truncated {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("[hook {stream} truncated]"));
    }
    text
}

#[cfg(unix)]
fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg(command);
    cmd
}

#[cfg(unix)]
fn terminate_hook_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    let pgid = -(pid as libc::pid_t);
    // SAFETY: the hook shell is started as a new process group, so a negative
    // pid targets only that hook group and does not dereference memory.
    unsafe {
        libc::kill(pgid, libc::SIGTERM);
        libc::kill(pgid, libc::SIGKILL);
    }
}

#[cfg(windows)]
fn terminate_hook_process_tree(pid: Option<u32>) {
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
fn terminate_hook_process_tree(_pid: Option<u32>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hook_receives_payload_on_stdin() {
        let temp = tempfile::tempdir().expect("tempdir");
        let payload = HookPayload::new(HookEvent::PreToolUse, "session-1", temp.path());
        let script = temp.path().join("hook_payload.py");
        std::fs::write(
            &script,
            "import json, sys\nprint(json.load(sys.stdin)['event'])\n",
        )
        .expect("write hook script");
        #[cfg(windows)]
        let command = "python hook_payload.py".to_string();
        #[cfg(not(windows))]
        let command = "python3 hook_payload.py".to_string();

        let summary =
            run_hook_commands(HookEvent::PreToolUse, &[command], &payload, temp.path(), 5).await;

        assert!(summary.success(), "{summary:?}");
        assert_eq!(summary.outcomes[0].stdout, "pre_tool_use");
    }

    #[tokio::test]
    async fn nonzero_hook_is_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let payload = HookPayload::new(HookEvent::PreToolUse, "session-1", temp.path());

        let summary = run_hook_commands(
            HookEvent::PreToolUse,
            &["exit 7".to_string()],
            &payload,
            temp.path(),
            5,
        )
        .await;

        assert!(!summary.success());
        assert_eq!(summary.outcomes[0].exit_code, Some(7));
    }

    #[tokio::test]
    async fn limited_async_reader_caps_hook_output() {
        let (mut writer, reader) = tokio::io::duplex(16 * 1024);
        let writer_task = tokio::spawn(async move {
            writer.write_all(&vec![b'x'; 8192]).await.expect("write");
        });

        let (bytes, truncated) = read_limited_async_stream(reader, 1024)
            .await
            .expect("limited stream");
        writer_task.await.expect("writer task");

        assert_eq!(bytes.len(), 1024);
        assert!(truncated);
    }

    #[test]
    fn hook_output_text_marks_truncation() {
        let output = hook_output_text(b"hello", true, "stdout");

        assert!(output.contains("hello"));
        assert!(output.contains("[hook stdout truncated]"));
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn hook_stdout_is_capped() {
        let temp = tempfile::tempdir().expect("tempdir");
        let payload = HookPayload::new(HookEvent::PreToolUse, "session-1", temp.path());
        let command =
            "python3 - <<'PY'\nimport sys\nsys.stdout.write('x' * 200000)\nPY".to_string();

        let summary =
            run_hook_commands(HookEvent::PreToolUse, &[command], &payload, temp.path(), 5).await;

        assert!(summary.success(), "{summary:?}");
        assert!(summary.outcomes[0].stdout.len() < 140_000);
        assert!(summary.outcomes[0]
            .stdout
            .contains("[hook stdout truncated]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_hook_kills_child_process_group() {
        let temp = tempfile::tempdir().expect("tempdir");
        let payload = HookPayload::new(HookEvent::PreToolUse, "session-1", temp.path());
        let pid_file = temp.path().join("child.pid");
        let command = format!(
            "sleep 20 & printf '%s' \"$!\" > '{}'; wait",
            pid_file.display()
        );

        let summary =
            run_hook_commands(HookEvent::PreToolUse, &[command], &payload, temp.path(), 1).await;

        assert!(!summary.success());
        assert!(summary.outcomes[0].timed_out);
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
        assert!(!status.success(), "hook child process {pid} should be gone");
    }

    #[test]
    fn session_end_includes_legacy_stop_hooks() {
        let config = HooksConfig {
            session_end: vec!["end".to_string()],
            stop: vec!["stop".to_string()],
            ..HooksConfig::default()
        };

        assert_eq!(
            configured_commands(&config, HookEvent::SessionEnd),
            vec!["end".to_string(), "stop".to_string()]
        );
    }
}

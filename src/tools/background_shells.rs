//! Background shell registry.
//!
//! Lets the agent spawn a long-running command (npm install, cargo test,
//! `pytest -v`, …) without blocking the turn. The agent gets a `shell_id`
//! back immediately and can poll incremental output via `bash_output` or
//! terminate it via `kill_shell`.
//!
//! ## Lifetime
//!
//! Shells live for the lifetime of the octocode process. There is no
//! cross-session persistence — restarting octo loses all background shells.
//! Each shell's output is kept in memory until the shell is killed or
//! `kill_all` is called.
//!
//! ## Safety
//!
//! Background spawn reuses the same `policy/commands` dangerous-pattern
//! filter that synchronous `run_command` does. The caller is expected to
//! have already obtained policy approval — we don't re-prompt here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

use crate::policy::sandbox::{command_invocation, CommandSandboxConfig};

const MAX_LINES_PER_SHELL: usize = 4096;
const MAX_LINE_BYTES_PER_SHELL: usize = 64 * 1024;
const TRUNCATED_LINE_MARKER: &str = " [line truncated]";

/// One running background shell.
pub struct BackgroundShell {
    pub shell_id: String,
    pub command: String,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
    pid: Option<u32>,
    inner: Arc<Mutex<ShellState>>,
    child: Arc<AsyncMutex<Option<Child>>>,
    _stdout_task: JoinHandle<()>,
    _stderr_task: JoinHandle<()>,
    _waiter_task: JoinHandle<()>,
}

struct ShellState {
    stdout_lines: Vec<String>,
    stderr_lines: Vec<String>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    exit_code: Option<i32>,
    finished_at: Option<DateTime<Utc>>,
}

impl ShellState {
    fn new() -> Self {
        Self {
            stdout_lines: Vec::new(),
            stderr_lines: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            exit_code: None,
            finished_at: None,
        }
    }

    fn push_stdout_line(&mut self, line: String, line_truncated: bool) {
        if push_bounded_line(&mut self.stdout_lines, line, line_truncated) {
            self.stdout_truncated = true;
        }
    }

    fn push_stderr_line(&mut self, line: String, line_truncated: bool) {
        if push_bounded_line(&mut self.stderr_lines, line, line_truncated) {
            self.stderr_truncated = true;
        }
    }
}

/// Snapshot returned to the agent on each poll.
#[derive(Debug, Clone)]
pub struct ShellSnapshot {
    pub shell_id: String,
    pub command: String,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    /// New stdout lines since the caller's cursor (joined with `\n`).
    pub stdout: String,
    pub stderr: String,
    /// Next cursor to pass back; opaque to the caller.
    pub next_stdout_cursor: usize,
    pub next_stderr_cursor: usize,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl ShellSnapshot {
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.exit_code.is_none()
    }
}

/// Process-wide registry of background shells.
#[derive(Default)]
pub struct BackgroundShellsRegistry {
    shells: Mutex<HashMap<String, Arc<BackgroundShell>>>,
}

static REGISTRY: OnceLock<BackgroundShellsRegistry> = OnceLock::new();

#[must_use]
pub fn registry() -> &'static BackgroundShellsRegistry {
    REGISTRY.get_or_init(BackgroundShellsRegistry::default)
}

impl BackgroundShellsRegistry {
    pub fn list(&self) -> Vec<ShellSnapshot> {
        let shells = self
            .shells
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shells.values().map(|s| s.snapshot(0, 0)).collect()
    }

    pub fn get(&self, shell_id: &str) -> Option<Arc<BackgroundShell>> {
        let shells = self
            .shells
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shells.get(shell_id).cloned()
    }

    /// Spawn a new background shell. Returns its shell_id.
    pub async fn spawn(&self, command: &str, cwd: &Path) -> Result<String> {
        self.spawn_with_sandbox(command, cwd, &CommandSandboxConfig::default())
            .await
    }

    /// Spawn a new background shell with the same sandbox/env/process isolation
    /// used by synchronous `run_command`.
    pub async fn spawn_with_sandbox(
        &self,
        command: &str,
        cwd: &Path,
        sandbox: &CommandSandboxConfig,
    ) -> Result<String> {
        let shell = BackgroundShell::spawn(command, cwd, sandbox).await?;
        let id = shell.shell_id.clone();
        let mut shells = self
            .shells
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shells.insert(id.clone(), Arc::new(shell));
        Ok(id)
    }

    /// Forcibly terminate a shell and remove it from the registry.
    pub async fn kill(&self, shell_id: &str) -> Result<()> {
        let shell = {
            let shells = self
                .shells
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            shells
                .get(shell_id)
                .cloned()
                .ok_or_else(|| anyhow!("unknown shell_id: {shell_id}"))?
        };
        shell.kill().await?;
        let mut shells = self
            .shells
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shells.remove(shell_id);
        Ok(())
    }

    pub async fn kill_all(&self) -> usize {
        let ids = {
            let shells = self
                .shells
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            shells.keys().cloned().collect::<Vec<_>>()
        };
        let mut killed = 0;
        for id in ids {
            if self.kill(&id).await.is_ok() {
                killed += 1;
            }
        }
        killed
    }
}

impl BackgroundShell {
    async fn spawn(command: &str, cwd: &Path, sandbox: &CommandSandboxConfig) -> Result<Self> {
        if let Some(reason) = crate::tools::run_command::command_policy_violation(command) {
            return Err(anyhow!(reason));
        }

        let shell_id = format!("sh-{}", uuid::Uuid::new_v4());
        let started_at = Utc::now();

        let (shell, shell_arg) = shell_program();
        let sandbox_invocation = command_invocation(shell, shell_arg, command, cwd, sandbox)?;
        let mut cmd = match sandbox_invocation {
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
        cmd.current_dir(cwd)
            .env_clear()
            .envs(crate::tools::run_command::sanitized_command_env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::tools::run_command::isolate_command_process_tree(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("failed to spawn background command: {e}"))?;
        let pid = child.id();

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("no stdout pipe on background child"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("no stderr pipe on background child"))?;

        let state = Arc::new(Mutex::new(ShellState::new()));
        let child = Arc::new(AsyncMutex::new(Some(child)));

        let stdout_task = {
            let state = state.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some((line, line_truncated))) =
                    read_limited_output_line(&mut reader, MAX_LINE_BYTES_PER_SHELL).await
                {
                    let mut s = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    s.push_stdout_line(line, line_truncated);
                }
            })
        };

        let stderr_task = {
            let state = state.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                while let Ok(Some((line, line_truncated))) =
                    read_limited_output_line(&mut reader, MAX_LINE_BYTES_PER_SHELL).await
                {
                    let mut s = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    s.push_stderr_line(line, line_truncated);
                }
            })
        };

        let waiter_task = {
            let state = state.clone();
            let child = child.clone();
            tokio::spawn(async move {
                let exit = {
                    let mut guard = child.lock().await;
                    match guard.as_mut() {
                        Some(c) => c.wait().await.ok().and_then(|s| s.code()),
                        None => None,
                    }
                };
                let mut s = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                s.exit_code = Some(exit.unwrap_or(-1));
                s.finished_at = Some(Utc::now());
            })
        };

        Ok(Self {
            shell_id,
            command: command.to_string(),
            cwd: cwd.to_path_buf(),
            started_at,
            pid,
            inner: state,
            child,
            _stdout_task: stdout_task,
            _stderr_task: stderr_task,
            _waiter_task: waiter_task,
        })
    }

    /// Take a snapshot of the shell's state starting from the given cursors.
    /// Each cursor is the index of the next line to read; the snapshot returns
    /// new cursor values to pass on the next call.
    #[must_use]
    pub fn snapshot(&self, stdout_cursor: usize, stderr_cursor: usize) -> ShellSnapshot {
        let s = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stdout = s.stdout_lines[stdout_cursor.min(s.stdout_lines.len())..].join("\n");
        let stderr = s.stderr_lines[stderr_cursor.min(s.stderr_lines.len())..].join("\n");
        ShellSnapshot {
            shell_id: self.shell_id.clone(),
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            started_at: self.started_at,
            finished_at: s.finished_at,
            exit_code: s.exit_code,
            stdout,
            stderr,
            next_stdout_cursor: s.stdout_lines.len(),
            next_stderr_cursor: s.stderr_lines.len(),
            stdout_truncated: s.stdout_truncated,
            stderr_truncated: s.stderr_truncated,
        }
    }

    async fn kill(&self) -> Result<()> {
        {
            let state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.exit_code.is_some() {
                return Ok(());
            }
        }
        crate::tools::run_command::terminate_process_tree(self.pid);
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.start_kill();
            }
        }
        for _ in 0..20 {
            {
                let state = self
                    .inner
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.exit_code.is_some() {
                    return Ok(());
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Err(anyhow!("shell did not exit after kill: {}", self.shell_id))
    }
}

fn shell_program() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    }
}

fn push_bounded_line(lines: &mut Vec<String>, line: String, line_truncated: bool) -> bool {
    let mut truncated = line_truncated;
    if lines.len() >= MAX_LINES_PER_SHELL {
        truncated = true;
        lines.remove(0);
    }
    lines.push(line);
    truncated
}

async fn read_limited_output_line<R>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<(String, bool)>>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::new();
    let mut truncated = false;

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }

        let newline_offset = available.iter().position(|byte| *byte == b'\n');
        let take = newline_offset.map_or(available.len(), |offset| offset + 1);
        let remaining = max_bytes.saturating_sub(bytes.len());
        let keep = remaining.min(take);
        if keep > 0 {
            bytes.extend_from_slice(&available[..keep]);
        }
        if take > remaining {
            truncated = true;
        }

        reader.consume(take);
        if newline_offset.is_some() {
            break;
        }
    }

    let mut line = String::from_utf8_lossy(&bytes)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if truncated {
        line.push_str(TRUNCATED_LINE_MARKER);
    }
    Ok(Some((line, truncated)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct EnvGuard(&'static str);

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    #[tokio::test]
    async fn spawn_and_poll_picks_up_stdout() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let cmd = "echo hello & echo world";
        #[cfg(not(windows))]
        let cmd = "printf 'hello\\nworld\\n'";

        let id = registry().spawn(cmd, temp.path()).await.unwrap();
        // Give the child time to finish writing.
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let snap = registry().get(&id).unwrap().snapshot(0, 0);
            if snap.exit_code.is_some() && !snap.stdout.is_empty() {
                assert!(snap.stdout.contains("hello"));
                assert!(snap.stdout.contains("world"));
                let _ = registry().kill(&id).await;
                return;
            }
        }
        panic!("background shell did not produce output in time");
    }

    #[tokio::test]
    async fn poll_with_cursor_returns_only_new_lines() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let cmd = "echo line1 & echo line2 & echo line3";
        #[cfg(not(windows))]
        let cmd = "printf 'line1\\nline2\\nline3\\n'";

        let id = registry().spawn(cmd, temp.path()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let s1 = registry().get(&id).unwrap().snapshot(0, 0);
        assert!(s1.stdout.contains("line1"));
        let s2 = registry()
            .get(&id)
            .unwrap()
            .snapshot(s1.next_stdout_cursor, s1.next_stderr_cursor);
        // After draining at cursor, second snapshot should have empty stdout
        // (the lines have been read).
        assert!(
            s2.stdout.is_empty(),
            "expected empty stdout, got: {:?}",
            s2.stdout
        );
        let _ = registry().kill(&id).await;
    }

    #[tokio::test]
    async fn kill_marks_unknown_shell_as_error() {
        let err = registry().kill("does-not-exist").await.unwrap_err();
        assert!(err.to_string().contains("unknown shell_id"));
    }

    #[tokio::test]
    async fn kill_all_removes_running_shells() {
        let temp = tempfile::tempdir().expect("tempdir");
        #[cfg(windows)]
        let cmd = "ping -n 30 127.0.0.1 > nul";
        #[cfg(not(windows))]
        let cmd = "sleep 20";

        let registry = BackgroundShellsRegistry::default();
        let first = registry.spawn(cmd, temp.path()).await.unwrap();
        let second = registry.spawn(cmd, temp.path()).await.unwrap();

        let killed = registry.kill_all().await;

        assert!(killed >= 2);
        assert!(registry.get(&first).is_none());
        assert!(registry.get(&second).is_none());
    }

    #[tokio::test]
    async fn background_spawn_blocks_dangerous_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        let err = registry().spawn("rm -rf /", temp.path()).await.unwrap_err();
        assert!(err.to_string().contains("dangerous command blocked"));
    }

    #[tokio::test]
    async fn background_spawn_strips_sensitive_env() {
        std::env::set_var("OCTO_BACKGROUND_API_KEY", "should-not-leak");
        let _guard = EnvGuard("OCTO_BACKGROUND_API_KEY");
        let temp = tempfile::tempdir().expect("tempdir");
        #[cfg(windows)]
        let cmd = "echo key=%OCTO_BACKGROUND_API_KEY%";
        #[cfg(not(windows))]
        let cmd = "printf 'key=%s\\n' \"${OCTO_BACKGROUND_API_KEY:-}\"";

        let id = registry().spawn(cmd, temp.path()).await.unwrap();
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let snap = registry().get(&id).unwrap().snapshot(0, 0);
            if snap.exit_code.is_some() && !snap.stdout.is_empty() {
                assert!(!snap.stdout.contains("should-not-leak"));
                let _ = registry().kill(&id).await;
                return;
            }
        }
        panic!("background shell did not produce env output in time");
    }

    #[tokio::test]
    async fn background_output_line_is_byte_limited() {
        let temp = tempfile::tempdir().expect("tempdir");
        #[cfg(windows)]
        let cmd = "powershell -NoProfile -Command \"$s='x'*70000; Write-Output $s\"";
        #[cfg(not(windows))]
        let cmd = "awk 'BEGIN { for (i = 0; i < 70000; i++) printf \"x\"; printf \"\\n\" }'";

        let registry = BackgroundShellsRegistry::default();
        let id = registry.spawn(cmd, temp.path()).await.unwrap();

        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let snap = registry.get(&id).unwrap().snapshot(0, 0);
            if snap.exit_code.is_some() && !snap.stdout.is_empty() {
                assert!(snap.stdout_truncated);
                assert!(snap.stdout.contains(TRUNCATED_LINE_MARKER));
                assert!(
                    snap.stdout.len() < 66_000,
                    "stdout should be capped, got {} bytes",
                    snap.stdout.len()
                );
                let _ = registry.kill(&id).await;
                return;
            }
        }

        panic!("background shell did not produce capped output in time");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn kill_terminates_child_process_group() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pid_file = temp.path().join("child.pid");
        let command = format!(
            "sleep 20 & printf '%s' \"$!\" > '{}'; wait",
            pid_file.display()
        );

        let id = registry().spawn(&command, temp.path()).await.unwrap();
        for _ in 0..20 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let pid = std::fs::read_to_string(&pid_file)
            .expect("pid file")
            .trim()
            .to_string();

        registry().kill(&id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let status = std::process::Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .expect("kill -0");
        assert!(
            !status.success(),
            "background child process {pid} should be gone"
        );
    }
}

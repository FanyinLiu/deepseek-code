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
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

const MAX_LINES_PER_SHELL: usize = 4096;

/// One running background shell.
pub struct BackgroundShell {
    pub shell_id: String,
    pub command: String,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
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
        let shell = BackgroundShell::spawn(command, cwd).await?;
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
}

impl BackgroundShell {
    async fn spawn(command: &str, cwd: &Path) -> Result<Self> {
        let shell_id = format!("sh-{}", uuid::Uuid::new_v4());
        let started_at = Utc::now();

        let mut cmd = shell_command(command);
        cmd.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("failed to spawn background command: {e}"))?;

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
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let mut s = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if s.stdout_lines.len() >= MAX_LINES_PER_SHELL {
                        s.stdout_truncated = true;
                        s.stdout_lines.remove(0);
                    }
                    s.stdout_lines.push(line);
                }
            })
        };

        let stderr_task = {
            let state = state.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let mut s = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if s.stderr_lines.len() >= MAX_LINES_PER_SHELL {
                        s.stderr_truncated = true;
                        s.stderr_lines.remove(0);
                    }
                    s.stderr_lines.push(line);
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
        let mut guard = self.child.lock().await;
        if let Some(child) = guard.as_mut() {
            let _ = child.start_kill();
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
}

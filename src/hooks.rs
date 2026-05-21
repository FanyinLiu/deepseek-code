use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::storage::config::HooksConfig;

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
        .stderr(Stdio::piped());

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

    let timeout = Duration::from_secs(timeout_seconds.max(1));
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
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
        Err(_) => {
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
    };

    HookCommandOutcome {
        command: command.to_string(),
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string(),
        stderr: String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_string(),
        duration_ms: start.elapsed().as_millis() as u64,
        timed_out: false,
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

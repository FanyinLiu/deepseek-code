//! `bash_output` — poll incremental output from a background shell.
//!
//! After spawning a long command with `run_command` + `background: true`,
//! the agent gets back a `shell_id`. This tool reads new stdout/stderr
//! since the last poll. Repeat-call-safe: each call advances the cursor.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::background_shells::registry;

#[derive(Debug, Deserialize)]
pub struct BashOutputArgs {
    pub shell_id: String,
    /// Optional cursor returned by the previous call. Defaults to 0 (read
    /// from the start). The shell registry returns updated cursors with
    /// each call.
    #[serde(default)]
    pub stdout_cursor: usize,
    #[serde(default)]
    pub stderr_cursor: usize,
}

#[derive(Debug, Serialize)]
pub struct BashOutputResponse {
    pub shell_id: String,
    pub command: String,
    pub status: &'static str,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub next_stdout_cursor: usize,
    pub next_stderr_cursor: usize,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub fn bash_output(args: BashOutputArgs) -> Result<String> {
    let shell = registry()
        .get(&args.shell_id)
        .ok_or_else(|| anyhow!("unknown shell_id: {}", args.shell_id))?;
    let snap = shell.snapshot(args.stdout_cursor, args.stderr_cursor);
    let status = if snap.is_running() {
        "running"
    } else if snap.exit_code == Some(0) {
        "completed"
    } else {
        "failed"
    };
    let response = BashOutputResponse {
        shell_id: snap.shell_id,
        command: snap.command,
        status,
        exit_code: snap.exit_code,
        stdout: snap.stdout,
        stderr: snap.stderr,
        next_stdout_cursor: snap.next_stdout_cursor,
        next_stderr_cursor: snap.next_stderr_cursor,
        stdout_truncated: snap.stdout_truncated,
        stderr_truncated: snap.stderr_truncated,
    };
    Ok(serde_json::to_string_pretty(&response)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_shell_id_returns_error() {
        let r = bash_output(BashOutputArgs {
            shell_id: "missing".to_string(),
            stdout_cursor: 0,
            stderr_cursor: 0,
        });
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("unknown shell_id"));
    }
}

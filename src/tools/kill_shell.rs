//! `kill_shell` — terminate a background shell spawned by `run_command`.
//!
//! Sends SIGKILL (Unix) or TerminateProcess (Windows) and removes the
//! shell from the registry. Safe to call on already-exited shells (it
//! will return success after the registry cleanup).

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::background_shells::registry;

#[derive(Debug, Deserialize)]
pub struct KillShellArgs {
    pub shell_id: String,
}

#[derive(Debug, Serialize)]
pub struct KillShellResponse {
    pub shell_id: String,
    pub killed: bool,
    pub message: String,
}

pub async fn kill_shell(args: KillShellArgs) -> Result<String> {
    let result = registry().kill(&args.shell_id).await;
    let response = match result {
        Ok(()) => KillShellResponse {
            shell_id: args.shell_id,
            killed: true,
            message: "shell terminated".to_string(),
        },
        Err(e) => KillShellResponse {
            shell_id: args.shell_id,
            killed: false,
            message: e.to_string(),
        },
    };
    Ok(serde_json::to_string_pretty(&response)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kill_unknown_shell_reports_message_not_panic() {
        let resp = kill_shell(KillShellArgs {
            shell_id: "missing".to_string(),
        })
        .await
        .unwrap();
        assert!(resp.contains("\"killed\": false"));
        assert!(resp.contains("unknown shell_id"));
    }
}

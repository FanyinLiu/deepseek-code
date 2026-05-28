use std::path::{Path, PathBuf};

use anyhow::Result;

use super::payload::{AgentRunDryRunPlan, AgentRunPayload};
use super::permission_mode_label;
use super::run::resolve_run_config;

#[allow(clippy::too_many_arguments)]
pub(super) fn run_agent_dry_run(
    project_root: &Path,
    name: &str,
    task_text: &str,
    focus: Option<PathBuf>,
    max_turns: Option<u32>,
    model: Option<String>,
    isolation: crate::agent::subagent::SubagentIsolation,
    approval_mode: Option<crate::agent::subagent::PermissionMode>,
) -> Result<AgentRunPayload, anyhow::Error> {
    let mut config = resolve_run_config(project_root, name, max_turns, model)?;
    config.isolation = isolation;
    if let Some(mode) = approval_mode {
        config.permission_mode = mode;
    }
    let focus_files = focus
        .map(|path| vec![path.display().to_string()])
        .unwrap_or_default();
    let isolation_label = match config.isolation {
        crate::agent::subagent::SubagentIsolation::None => "none",
        crate::agent::subagent::SubagentIsolation::Worktree => "worktree",
    };
    let plan = AgentRunDryRunPlan {
        project_root: project_root.display().to_string(),
        focus_files,
        model: config.model.as_ref().map(ToString::to_string),
        max_turns: config.max_turns,
        permission_mode: permission_mode_label(&config.permission_mode).to_string(),
        allowed_tools: config.allowed_tools.clone(),
        isolation: isolation_label.to_string(),
        would_request_api_key: false,
        network_required: false,
    };
    let summary = format!(
        "dry-run: agent '{name}' would run locally with max_turns={} and no API request",
        plan.max_turns
    );
    Ok(AgentRunPayload {
        agent: name.to_string(),
        task: task_text.to_string(),
        dry_run: true,
        success: true,
        summary: summary.clone(),
        output: summary,
        plan: Some(plan),
        tool_calls_used: Vec::new(),
        files_read: Vec::new(),
        files_written: Vec::new(),
        duration_ms: 0,
        token_usage: 0,
        failure_reason: None,
        worktree: None,
        error: None,
        approval_denials: Vec::new(),
    })
}

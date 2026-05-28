use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use tokio::sync::mpsc;

use crate::agent::orchestrator::AgentEvent;
use crate::agent::subagent::{
    SubagentConfig, SubagentExecutor, SubagentRegistry, SubagentResult, SubagentTask,
};
use crate::cli::login;
use crate::provider::{build_provider, Provider};
use crate::storage;

use super::payload::{
    AgentApprovalDenialPayload, AgentFailureReasonPayload, AgentRunPayload, AgentWorktreePayload,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_agent(
    project_root: &Path,
    name: &str,
    task_text: &str,
    focus: Option<PathBuf>,
    max_turns: Option<u32>,
    model: Option<String>,
    isolation: crate::agent::subagent::SubagentIsolation,
    approval_mode: Option<crate::agent::subagent::PermissionMode>,
    show_events: bool,
) -> Result<AgentRunPayload, anyhow::Error> {
    let mut config = resolve_run_config(project_root, name, max_turns, model)?;
    config.isolation = isolation;
    if let Some(mode) = approval_mode {
        config.permission_mode = mode;
    }
    let configured_max_turns = config.max_turns;

    let app_config = storage::Config::load(Some(project_root))?;
    let api_key = login::resolve_or_prompt_api_key(Some(project_root))?;
    let provider = build_provider(&app_config.provider, api_key);
    let client = Arc::new(provider.create_deepseek_client());
    let task = SubagentTask {
        description: task_text.chars().take(80).collect(),
        prompt: task_text.to_string(),
        context: None,
        focus_files: focus
            .map(|path| vec![path.display().to_string()])
            .unwrap_or_default(),
        expected_output: Some("Concise agent result".to_string()),
    };

    let guard = match crate::agent::subagent::maybe_start_worktree(project_root, config.isolation) {
        Ok(g) => g,
        Err(error) => {
            return Ok(run_payload_from_result(
                name,
                task_text,
                worktree_setup_result(error),
                Vec::new(),
                configured_max_turns,
            ))
        }
    };
    let effective_root = guard
        .as_ref()
        .map(|g| g.path().to_path_buf())
        .unwrap_or_else(|| project_root.to_path_buf());
    let executor = SubagentExecutor::new(client, effective_root, config);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut handle = Box::pin(tokio::spawn(
        async move { executor.run(&task, &event_tx).await },
    ));

    let mut auto_approve_session = false;
    let mut approval_denials = Vec::new();
    let mut result = loop {
        tokio::select! {
            joined = &mut handle => {
                break joined.context("agent task join failed")?;
            }
            event = event_rx.recv() => {
                if let Some(event) = event {
                    handle_run_event(
                        event,
                        show_events,
                        &mut auto_approve_session,
                        &mut approval_denials,
                    )
                    .await;
                }
            }
        }
    };
    while let Ok(event) = event_rx.try_recv() {
        handle_run_event(
            event,
            show_events,
            &mut auto_approve_session,
            &mut approval_denials,
        )
        .await;
    }
    crate::agent::subagent::finalize_worktree(guard, &mut result);

    Ok(run_payload_from_result(
        name,
        task_text,
        result,
        approval_denials,
        configured_max_turns,
    ))
}

pub(super) fn resolve_run_config(
    project_root: &Path,
    name: &str,
    max_turns: Option<u32>,
    model: Option<String>,
) -> Result<SubagentConfig, anyhow::Error> {
    let registry = SubagentRegistry::load_from_project(project_root);
    let Some(base_config) = registry.get(name) else {
        bail!("unknown agent '{name}'");
    };

    let mut config = base_config.clone();
    if let Some(max_turns) = max_turns {
        config.max_turns = max_turns;
    }
    if let Some(model) = model {
        config.model = Some(crate::provider::parse_model(&model)?);
    }
    Ok(config)
}

async fn handle_run_event(
    event: AgentEvent,
    show_events: bool,
    auto_approve_session: &mut bool,
    approval_denials: &mut Vec<AgentApprovalDenialPayload>,
) {
    match event {
        AgentEvent::SubagentStarted {
            agent_type,
            description,
            ..
        } if show_events => {
            println!("agent started: {agent_type} - {description}");
        }
        AgentEvent::SubagentDelta { content, .. } if show_events && !content.trim().is_empty() => {
            print!("{content}");
        }
        AgentEvent::SubagentToolApprovalNeeded {
            agent_id,
            tool_name,
            arguments,
            policy_decision,
            respond,
        } => {
            let is_tty = show_events && crate::cli::approval::cli_prompt_tty();
            let decision = match tokio::time::timeout(
                Duration::from_secs(55),
                crate::cli::approval::resolve_subagent_tool_approval(
                    &policy_decision,
                    crate::cli::ToolApprovalPolicy::Ask,
                    auto_approve_session,
                    is_tty,
                ),
            )
            .await
            {
                Ok(decision) => decision,
                Err(_) => crate::cli::approval::ToolApprovalDecision::denied(
                    crate::cli::approval::DENIAL_REASON_APPROVAL_TIMEOUT,
                ),
            };
            if show_events && !decision.approved {
                println!(
                    "tool approval required for {tool_name}; denied ({})",
                    decision.denial_reason.unwrap_or("unknown")
                );
            }
            let send_result = respond.send(decision.approved);
            let denial_reason = if send_result.is_err() {
                Some(crate::cli::approval::DENIAL_REASON_APPROVAL_RESPONSE_CLOSED)
            } else {
                decision.denial_reason
            };
            if let Some(reason) = denial_reason {
                approval_denials.push(AgentApprovalDenialPayload {
                    agent_id,
                    tool: tool_name.clone(),
                    reason: reason.to_string(),
                    arguments,
                    details: policy_decision.display.details,
                });
            }
        }
        AgentEvent::SubagentCompleted { result, .. } if show_events => {
            let status = if result.success { "ok" } else { "failed" };
            println!("agent completed: {status}");
        }
        AgentEvent::Error(error) if show_events => {
            eprintln!("agent error: {error}");
        }
        _ => {}
    }
}

fn worktree_setup_result(error: anyhow::Error) -> crate::agent::subagent::SubagentResult {
    let now = chrono::Utc::now();
    crate::agent::subagent::SubagentResult {
        success: false,
        summary: "Subagent skipped: failed to set up isolated worktree".to_string(),
        output: format!("worktree setup failed: {error}"),
        tool_calls_used: Vec::new(),
        files_read: Vec::new(),
        files_written: Vec::new(),
        duration_ms: 0,
        token_usage: 0,
        error: Some(error.to_string()),
        started_at: now,
        completed_at: now,
        worktree: None,
    }
}

pub(super) fn run_payload_from_result(
    name: &str,
    task: &str,
    result: SubagentResult,
    approval_denials: Vec<AgentApprovalDenialPayload>,
    max_turns: u32,
) -> AgentRunPayload {
    let failure_reason = classify_run_failure(&result, approval_denials.as_slice(), max_turns);
    let worktree = result.worktree.map(|artifact| AgentWorktreePayload {
        path: artifact.path.display().to_string(),
        branch: artifact.branch,
    });
    AgentRunPayload {
        agent: name.to_string(),
        task: task.to_string(),
        dry_run: false,
        success: result.success,
        summary: result.summary,
        output: result.output,
        plan: None,
        tool_calls_used: result.tool_calls_used,
        files_read: result.files_read,
        files_written: result.files_written,
        duration_ms: result.duration_ms,
        token_usage: result.token_usage,
        failure_reason,
        worktree,
        error: result.error,
        approval_denials,
    }
}

fn classify_run_failure(
    result: &SubagentResult,
    approval_denials: &[AgentApprovalDenialPayload],
    max_turns: u32,
) -> Option<AgentFailureReasonPayload> {
    if result.success {
        return None;
    }
    if !approval_denials.is_empty() {
        return Some(failure_reason(
            "approval_denied",
            "agent run stopped because a tool approval was denied",
            "approve the requested tool or narrow the task to avoid it",
            None,
            None,
        ));
    }
    if result.summary.contains("已用完轮次预算")
        || result.output.contains("已用完轮次预算")
        || result.error.as_deref().is_some_and(|error| {
            error.contains("已用完轮次预算")
                || error.contains("turn budget was exhausted")
                || error.contains("turn budget")
        })
    {
        return Some(failure_reason(
            "turn_budget_exhausted",
            "agent stopped after using all configured tool-call turns",
            "increase --max-turns or narrow the task/focus",
            Some(max_turns),
            Some(result.tool_calls_used.len() as u32),
        ));
    }
    if result.files_read.is_empty()
        && result
            .tool_calls_used
            .iter()
            .any(|tool| tool == "list_dir" || tool == "search_code")
    {
        return Some(failure_reason(
            "turn_budget_exhausted",
            "agent stopped after using all configured tool-call turns",
            "increase --max-turns or narrow the task/focus",
            Some(max_turns),
            Some(result.tool_calls_used.len() as u32),
        ));
    }
    if result.tool_calls_used.is_empty() {
        return Some(failure_reason(
            "no_tool_progress",
            "agent did not make observable tool progress",
            "retry with a smaller task or provide a focus path",
            Some(max_turns),
            Some(0),
        ));
    }
    if result.error.as_deref().is_some_and(|error| {
        error.contains("未形成可用结论") || error.contains("no usable conclusion")
    }) {
        return Some(failure_reason(
            "no_final_answer",
            "agent finished tool work but did not produce a usable final answer",
            "retry with a more specific expected output",
            Some(max_turns),
            Some(result.tool_calls_used.len() as u32),
        ));
    }
    Some(failure_reason(
        "agent_failed",
        "agent run failed",
        "inspect error and retry with a narrower task",
        Some(max_turns),
        Some(result.tool_calls_used.len() as u32),
    ))
}

fn failure_reason(
    code: &str,
    message: &str,
    hint: &str,
    max_turns: Option<u32>,
    turns_used: Option<u32>,
) -> AgentFailureReasonPayload {
    AgentFailureReasonPayload {
        code: code.to_string(),
        message: message.to_string(),
        hint: hint.to_string(),
        max_turns,
        turns_used,
    }
}

pub(super) fn print_run_result(payload: &AgentRunPayload) {
    println!();
    println!("Agent: {}", payload.agent);
    println!("Success: {}", payload.success);
    println!("Duration: {}ms", payload.duration_ms);
    if payload.token_usage > 0 {
        println!("Tokens: {}", payload.token_usage);
    }
    if !payload.tool_calls_used.is_empty() {
        println!("Tools: {}", payload.tool_calls_used.join(", "));
    }
    if let Some(error) = &payload.error {
        println!("Error: {error}");
    }
    if let Some(reason) = &payload.failure_reason {
        println!("Failure reason: {} - {}", reason.code, reason.hint);
    }
    if !payload.approval_denials.is_empty() {
        println!("Approval denials:");
        for denial in &payload.approval_denials {
            println!(
                "  {}:{} denied ({})",
                denial.agent_id, denial.tool, denial.reason
            );
        }
    }
    println!();
    println!("{}", payload.output);
}

use std::path::Path;

use chrono::Utc;
use tokio::sync::mpsc;

use crate::deepseek::{Session, SubTurnId, ToolCall, ToolCallRecord, ToolResultRecord, TurnId};
use crate::hooks::{HookEvent, HookPayload};
use crate::policy;
use crate::storage::{EventLogStore, SessionEvent, SessionEventKind};
use crate::tools::backend::{LocalToolBackend, ToolBackend, ToolExecutionContext};

use super::orchestrator::AgentEvent;

/// Handles the tool-call execution loop.
pub struct ToolLoop;

impl ToolLoop {
    /// Execute tools with policy checks and approval before each tool.
    /// Sends `ToolApprovalNeeded` events and awaits oneshot responses.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_tools_with_approval(
        tool_calls: &[ToolCall],
        project_root: &Path,
        turn_id: TurnId,
        _sub_turn_id: SubTurnId,
        session: &mut Session,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        yolo_mode: bool,
        policy_config: &crate::storage::config::PolicyConfig,
        hooks_config: &crate::storage::config::HooksConfig,
        event_log_store: Option<EventLogStore>,
    ) -> Vec<(ToolCall, ToolResultRecord)> {
        let mut results = Vec::new();
        let dispatch_config =
            crate::tools::dispatch::ToolDispatchConfig::from_policy(policy_config);

        for tc in tool_calls {
            // Evaluate policy
            let decision = policy::evaluate_tool(
                &tc.function.name,
                &tc.function.arguments,
                project_root,
                policy_config,
            );

            let approved = match decision.action {
                policy::PolicyAction::Allow => true,
                policy::PolicyAction::Deny => {
                    send_event(
                        event_tx,
                        AgentEvent::ToolExecuted {
                            tool_name: tc.function.name.clone(),
                            success: false,
                            summary: format!("Blocked: {}", decision.reason),
                        },
                    );
                    let record = ToolResultRecord {
                        tool_call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        result: format!("Blocked: {}", decision.reason),
                        is_error: true,
                    };
                    results.push((tc.clone(), record));
                    continue;
                }
                policy::PolicyAction::AskOnce | policy::PolicyAction::AskSession => {
                    if yolo_mode {
                        true
                    } else {
                        // Request approval via oneshot channel
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        send_event(
                            event_tx,
                            AgentEvent::ToolApprovalNeeded {
                                tool_name: tc.function.name.clone(),
                                display: decision.display.clone(),
                                respond: tx,
                            },
                        );
                        // Wait for response (timeout after 60s = auto-deny)
                        if let Ok(Ok(true)) =
                            tokio::time::timeout(std::time::Duration::from_mins(1), rx).await
                        {
                            true
                        } else {
                            send_event(
                                event_tx,
                                AgentEvent::ToolExecuted {
                                    tool_name: tc.function.name.clone(),
                                    success: false,
                                    summary: "Denied by user or timeout".into(),
                                },
                            );
                            let record = ToolResultRecord {
                                tool_call_id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                result: "Denied by user or timeout".into(),
                                is_error: true,
                            };
                            results.push((tc.clone(), record));
                            continue;
                        }
                    }
                }
            };

            let mut hook_payload = HookPayload::new(
                HookEvent::PreToolUse,
                session.id.to_string(),
                project_root.to_path_buf(),
            );
            hook_payload.turn_id = Some(turn_id.to_string());
            hook_payload.tool_call_id = Some(tc.id.clone());
            hook_payload.tool_name = Some(tc.function.name.clone());
            hook_payload.arguments = Some(tc.function.arguments.clone());
            let pre_hooks = crate::hooks::run_configured_hooks(
                HookEvent::PreToolUse,
                hooks_config,
                &hook_payload,
                project_root,
                policy_config.command_timeout_seconds,
            )
            .await;
            if let Some(pre_hooks) = pre_hooks {
                emit_hook_summary(
                    event_tx,
                    event_log_store.as_ref(),
                    project_root,
                    session.id,
                    Some(turn_id),
                    &pre_hooks,
                );
                if !pre_hooks.success() {
                    let summary = format!("Blocked by pre_tool hook: {}", pre_hooks.brief());
                    send_event(
                        event_tx,
                        AgentEvent::ToolExecuted {
                            tool_name: tc.function.name.clone(),
                            success: false,
                            summary: summary.clone(),
                        },
                    );
                    let record = ToolResultRecord {
                        tool_call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        result: summary,
                        is_error: true,
                    };
                    results.push((tc.clone(), record));
                    continue;
                }
            }

            let backend = LocalToolBackend;
            let context = ToolExecutionContext {
                project_root: project_root.to_path_buf(),
                dispatch_config: dispatch_config.clone(),
            };
            let backend_result = backend.execute(tc, &context).await;
            let result_text = backend_result.content.clone();
            let is_error = !backend_result.success;
            let duration_ms = backend_result.duration_ms;
            let changed_files = backend_result.changed_files.clone();

            if crate::hooks::configured_commands(hooks_config, HookEvent::PostToolUse).is_empty() {
                // No post-tool hooks configured.
            } else {
                let mut post_payload = HookPayload::new(
                    HookEvent::PostToolUse,
                    session.id.to_string(),
                    project_root.to_path_buf(),
                );
                post_payload.turn_id = Some(turn_id.to_string());
                post_payload.tool_call_id = Some(tc.id.clone());
                post_payload.tool_name = Some(tc.function.name.clone());
                post_payload.arguments = Some(tc.function.arguments.clone());
                post_payload.success = Some(backend_result.success);
                post_payload.summary = Some(backend_result.summary.clone());
                post_payload.duration_ms = Some(duration_ms);
                post_payload.changed_files = changed_files;
                if let Some(post_hooks) = crate::hooks::run_configured_hooks(
                    HookEvent::PostToolUse,
                    hooks_config,
                    &post_payload,
                    project_root,
                    policy_config.command_timeout_seconds,
                )
                .await
                {
                    emit_hook_summary(
                        event_tx,
                        event_log_store.as_ref(),
                        project_root,
                        session.id,
                        Some(turn_id),
                        &post_hooks,
                    );
                    if !post_hooks.success() {
                        send_event(
                            event_tx,
                            AgentEvent::ToolExecuted {
                                tool_name: "hook.post_tool".to_string(),
                                success: false,
                                summary: post_hooks.brief(),
                            },
                        );
                    }
                }
            }

            // Record in session history
            let record = ToolCallRecord {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
                result_summary: backend_result.summary,
                exit_code: if is_error { Some(1) } else { Some(0) },
                duration_ms,
                risk_level: crate::agent::utils::risk_level_for_tool(&tc.function.name).to_string(),
                approved,
                at: Utc::now(),
            };
            session.tool_call_history.push(record);

            let result_record = ToolResultRecord {
                tool_call_id: tc.id.clone(),
                name: tc.function.name.clone(),
                result: result_text,
                is_error,
            };

            results.push((tc.clone(), result_record));
        }

        results
    }

    // execute_single_tool moved to crate::tools::dispatch
}

fn emit_hook_summary(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    store: Option<&EventLogStore>,
    project_root: &Path,
    session_id: crate::deepseek::SessionId,
    turn_id: Option<TurnId>,
    summary: &crate::hooks::HookRunSummary,
) {
    if let Some(store) = store {
        let event = SessionEvent::new(
            session_id,
            turn_id,
            SessionEventKind::HookExecuted {
                event: summary.event.as_str().to_string(),
                success: summary.success(),
                summary: summary.brief(),
                command_count: summary.outcomes.len(),
            },
        );
        if let Err(err) = store.append(project_root, &event) {
            tracing::warn!("failed to append hook event: {err}");
        }
    }
    send_event(
        tx,
        AgentEvent::HookExecuted {
            event: summary.event,
            success: summary.success(),
            summary: summary.brief(),
            command_count: summary.outcomes.len(),
        },
    );
}

fn send_event(tx: &mpsc::UnboundedSender<AgentEvent>, event: AgentEvent) {
    if tx.send(event).is_err() {
        tracing::warn!("Agent event channel closed; event dropped");
    }
}

// helpers moved to crate::agent::utils

use std::path::Path;

use chrono::Utc;
use tokio::sync::mpsc;

use crate::deepseek::{Session, SubTurnId, ToolCall, ToolCallRecord, ToolResultRecord, TurnId};
use crate::policy;
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
        _turn_id: TurnId,
        _sub_turn_id: SubTurnId,
        session: &mut Session,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        yolo_mode: bool,
        policy_config: &crate::storage::config::PolicyConfig,
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

            let backend = LocalToolBackend;
            let context = ToolExecutionContext {
                project_root: project_root.to_path_buf(),
                dispatch_config,
            };
            let backend_result = backend.execute(tc, &context).await;
            let result_text = backend_result.content;
            let is_error = !backend_result.success;
            let duration_ms = backend_result.duration_ms;

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

fn send_event(tx: &mpsc::UnboundedSender<AgentEvent>, event: AgentEvent) {
    if tx.send(event).is_err() {
        tracing::warn!("Agent event channel closed; event dropped");
    }
}

// helpers moved to crate::agent::utils

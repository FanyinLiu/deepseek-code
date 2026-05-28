use std::path::Path;

use chrono::Utc;
use tokio::sync::mpsc;

use crate::deepseek::{Session, SubTurnId, ToolCall, ToolCallRecord, ToolResultRecord, TurnId};
use crate::policy::{PolicyAction, PolicyDecision, ToolCallSource};
use crate::runtime::tool_runtime::{
    ApprovalFuture, ApprovalOutcome, ApprovalResolver, LocalDispatchRuntimeBackend, ToolRuntime,
    ToolRuntimeContext,
};
use crate::storage::{EventLogStore, SessionEvent, SessionEventKind};

use super::orchestrator::AgentEvent;

/// Handles the tool-call execution loop.
pub struct ToolLoop;

#[derive(Debug, Clone)]
pub struct ToolLoopResult {
    pub call: ToolCall,
    pub result: ToolResultRecord,
    pub duration_ms: u64,
    pub changed_files: Vec<String>,
}

impl ToolLoopResult {
    #[must_use]
    pub fn new(
        call: ToolCall,
        result: ToolResultRecord,
        duration_ms: u64,
        changed_files: Vec<String>,
    ) -> Self {
        Self {
            call,
            result,
            duration_ms,
            changed_files,
        }
    }
}

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
    ) -> Vec<ToolLoopResult> {
        let mut results = Vec::new();
        let dispatch_config =
            crate::tools::dispatch::ToolDispatchConfig::from_policy(policy_config);
        let runtime = ToolRuntime::new(project_root, policy_config.clone());

        for tc in tool_calls {
            let mut context =
                ToolRuntimeContext::new(session.id.to_string(), dispatch_config.clone());
            context.session_id = Some(session.id);
            context.turn_id = Some(turn_id);
            context.hooks_config = Some(hooks_config);
            let mut resolver = AgentApprovalResolver {
                event_tx,
                yolo_mode,
                subagent: None,
            };
            let mut backend = LocalDispatchRuntimeBackend;
            let outcome = runtime
                .execute(
                    tc,
                    ToolCallSource::Main,
                    context,
                    &mut resolver,
                    &mut backend,
                )
                .await;

            for hook_summary in &outcome.hook_summaries {
                emit_hook_summary(
                    event_tx,
                    event_log_store.as_ref(),
                    project_root,
                    session.id,
                    Some(turn_id),
                    hook_summary,
                );
            }

            // Record in session history
            let approved = outcome.approval.approved();
            let is_error = outcome.result_record.is_error;
            let record = ToolCallRecord {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
                result_summary: outcome.backend_result.summary.clone(),
                exit_code: if is_error { Some(1) } else { Some(0) },
                duration_ms: outcome.backend_result.duration_ms,
                risk_level: outcome.decision.display.risk_level.to_string(),
                approved,
                at: Utc::now(),
            };
            session.tool_call_history.push(record);

            results.push(ToolLoopResult::new(
                tc.clone(),
                outcome.result_record,
                outcome.backend_result.duration_ms,
                outcome.backend_result.changed_files,
            ));
        }

        results
    }

    // execute_single_tool moved to crate::tools::dispatch
}

struct AgentApprovalResolver<'a> {
    event_tx: &'a mpsc::UnboundedSender<AgentEvent>,
    yolo_mode: bool,
    subagent: Option<(&'a str, &'a str)>,
}

impl ApprovalResolver for AgentApprovalResolver<'_> {
    fn resolve<'a>(
        &'a mut self,
        call: &'a crate::runtime::tool_runtime::ToolCall,
        decision: &'a PolicyDecision,
    ) -> ApprovalFuture<'a> {
        Box::pin(async move {
            if self.yolo_mode || matches!(decision.action, PolicyAction::Allow) {
                return ApprovalOutcome::Approved;
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            if let Some((agent_id, arguments)) = self.subagent {
                send_event(
                    self.event_tx,
                    AgentEvent::SubagentToolApprovalNeeded {
                        agent_id: agent_id.to_string(),
                        tool_name: call.tool.clone(),
                        arguments: arguments.to_string(),
                        policy_decision: decision.clone(),
                        respond: tx,
                    },
                );
            } else {
                send_event(
                    self.event_tx,
                    AgentEvent::ToolApprovalNeeded {
                        tool_name: call.tool.clone(),
                        display: decision.display.clone(),
                        respond: tx,
                    },
                );
            }
            match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                Ok(Ok(true)) => ApprovalOutcome::Approved,
                Ok(Ok(false)) => ApprovalOutcome::denied("Denied by user"),
                Ok(Err(_)) => ApprovalOutcome::denied("Approval channel closed"),
                Err(_) => ApprovalOutcome::denied("Denied by user or timeout"),
            }
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek::{
        MessageContent, MessageId, MessageVisibility, ProtocolMessage, ReasoningState, Role,
        SessionId, SessionMetadata, ToolCallFunction,
    };

    fn test_session(project_root: &Path) -> Session {
        Session {
            id: SessionId::new_v4(),
            name: None,
            project_root: project_root.to_path_buf(),
            messages: vec![ProtocolMessage {
                id: MessageId::new_v4(),
                role: Role::User,
                content: MessageContent::from("write a file"),
                reasoning_content: None,
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                turn_id: TurnId::new_v4(),
                sub_turn_id: None,
                visibility: MessageVisibility::UserVisible,
            }],
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: SessionMetadata::default(),
        }
    }

    #[tokio::test]
    async fn tool_loop_preserves_runtime_metadata_for_orchestrator_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let call = ToolCall {
            id: "call-write".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "write_file".into(),
                arguments: serde_json::json!({
                    "path": "generated.txt",
                    "content": "hello"
                })
                .to_string(),
            },
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut session = test_session(temp.path());

        let results = ToolLoop::execute_tools_with_approval(
            &[call],
            temp.path(),
            TurnId::new_v4(),
            SubTurnId::new_v4(),
            &mut session,
            &tx,
            true,
            &crate::storage::config::PolicyConfig::default(),
            &crate::storage::config::HooksConfig::default(),
            None,
        )
        .await;

        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert!(!result.result.is_error);
        assert_eq!(result.changed_files, vec!["generated.txt".to_string()]);
        assert_eq!(session.tool_call_history.len(), 1);
        assert_eq!(session.tool_call_history[0].duration_ms, result.duration_ms);
    }
}

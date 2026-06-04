use std::path::Path;

use chrono::Utc;
use tokio::sync::mpsc;

use crate::deepseek::{
    Session, SessionId, SubTurnId, ToolCall, ToolCallRecord, ToolResultRecord, TurnId,
};
use crate::policy::{PermissionMode, PolicyDecision, ToolCallSource};
use crate::runtime::tool_runtime::{
    ApprovalFuture, ApprovalOutcome, ApprovalResolver, LocalDispatchRuntimeBackend, ToolRuntime,
    ToolRuntimeContext, ToolRuntimeOutcome,
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

/// How many times a call sequence must repeat at the tail of history before the
/// next continuation of it is treated as a doom-loop and skipped.
const LOOP_REPEAT_LIMIT: usize = 2;

/// Longest call cycle the guard recognises. Period 1 is a single fixated call
/// (A,A,A…); periods 2–3 catch non-progressing oscillation (A,B,A,B… or
/// A,B,C,A,B,C…) that slips past the identical-call check.
const MAX_LOOP_PERIOD: usize = 3;

/// Outcome of deciding whether to run a call this batch.
enum CallExec {
    Ran(Box<ToolRuntimeOutcome>),
    /// Skipped as a repeated, non-progressing call (doom-loop guard).
    Looped,
}

/// True when running `tc` next would continue a non-progressing cycle: some
/// period-`p` (1..=`MAX_LOOP_PERIOD`) call sequence has already repeated
/// `LOOP_REPEAT_LIMIT` times at the tail of `history`, and `tc` is the call the
/// period predicts comes next. Period 1 is the common single-call fixation;
/// longer periods catch edit→read→edit→read style oscillation. Interleaved or
/// genuinely different calls break the period and reset the count.
///
/// Exposed so the reliability eval (`tests/reliability_eval_tests.rs`) can score
/// the guard against a labeled dataset.
pub fn would_loop(history: &[ToolCallRecord], tc: &ToolCall) -> bool {
    (1..=MAX_LOOP_PERIOD).any(|period| continues_stuck_cycle(history, tc, period))
}

/// Checks one specific cycle length for `would_loop`.
fn continues_stuck_cycle(history: &[ToolCallRecord], tc: &ToolCall, period: usize) -> bool {
    let window = period * LOOP_REPEAT_LIMIT;
    if history.len() < window {
        return false;
    }
    let tail = &history[history.len() - window..];
    let same =
        |a: &ToolCallRecord, b: &ToolCallRecord| a.name == b.name && a.arguments == b.arguments;
    // The window must be exactly period-periodic, and `tc` must be the call the
    // period predicts next (the one a full period before the end of the tail).
    (period..window).all(|i| same(&tail[i], &tail[i - period]))
        && tail[window - period].name == tc.function.name
        && tail[window - period].arguments == tc.function.arguments
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
        permission_mode: PermissionMode,
        policy_config: &crate::storage::config::PolicyConfig,
        hooks_config: &crate::storage::config::HooksConfig,
        event_log_store: Option<EventLogStore>,
    ) -> Vec<ToolLoopResult> {
        let dispatch_config =
            crate::tools::dispatch::ToolDispatchConfig::from_policy(policy_config);
        let runtime = ToolRuntime::new(project_root, policy_config.clone());
        let session_id = session.id;

        // DeepSeek V4 emits parallel tool calls. When the whole batch is
        // read-only and auto-approved (yolo), run them concurrently instead of
        // serializing the latency; `join_all` preserves input order. Anything
        // with side effects or an approval prompt stays strictly sequential.
        let parallel_safe = tool_calls.len() > 1
            && yolo_mode
            && tool_calls
                .iter()
                .all(|tc| crate::tools::metadata::is_read_only(&tc.function.name));

        // Doom-loop guard: a call that already ran LOOP_REPEAT_LIMIT times in a
        // row with the same arguments is skipped instead of executed again, so a
        // fixated model gets a course-correction nudge rather than burning turns.
        let looped: Vec<bool> = tool_calls
            .iter()
            .map(|tc| would_loop(&session.tool_call_history, tc))
            .collect();

        // Shared reference so each (possibly concurrent) future borrows the
        // runtime rather than trying to own it.
        let runtime = &runtime;
        let outcomes: Vec<CallExec> = if parallel_safe {
            futures::future::join_all(tool_calls.iter().enumerate().map(|(i, tc)| {
                let dispatch_config = dispatch_config.clone();
                let looped = looped[i];
                async move {
                    if looped {
                        CallExec::Looped
                    } else {
                        CallExec::Ran(Box::new(
                            run_tool_call(
                                tc,
                                runtime,
                                dispatch_config,
                                session_id,
                                turn_id,
                                hooks_config,
                                event_tx,
                                yolo_mode,
                                permission_mode,
                            )
                            .await,
                        ))
                    }
                }
            }))
            .await
        } else {
            let mut out = Vec::with_capacity(tool_calls.len());
            for (i, tc) in tool_calls.iter().enumerate() {
                if looped[i] {
                    out.push(CallExec::Looped);
                } else {
                    out.push(CallExec::Ran(Box::new(
                        run_tool_call(
                            tc,
                            runtime,
                            dispatch_config.clone(),
                            session_id,
                            turn_id,
                            hooks_config,
                            event_tx,
                            yolo_mode,
                            permission_mode,
                        )
                        .await,
                    )));
                }
            }
            out
        };

        let mut results = Vec::with_capacity(tool_calls.len());
        for (tc, exec) in tool_calls.iter().zip(outcomes) {
            let outcome = match exec {
                CallExec::Ran(outcome) => *outcome,
                CallExec::Looped => {
                    session.push_tool_call_record(ToolCallRecord {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                        result_summary: "skipped a repeated call".to_string(),
                        exit_code: Some(1),
                        duration_ms: 0,
                        risk_level: "none".to_string(),
                        approved: false,
                        at: Utc::now(),
                    });
                    send_event(
                        event_tx,
                        AgentEvent::ContentDelta(format!(
                            "\n[skipped a repeated `{}` call that wasn't making progress]\n",
                            tc.function.name
                        )),
                    );
                    results.push(ToolLoopResult::new(
                        tc.clone(),
                        ToolResultRecord {
                            tool_call_id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            result: "Skipped: the same steps keep repeating without moving things \
                                     forward. Try a different approach, or wrap up if you already \
                                     have what you need."
                                .to_string(),
                            is_error: true,
                        },
                        0,
                        Vec::new(),
                    ));
                    continue;
                }
            };

            for hook_summary in &outcome.hook_summaries {
                emit_hook_summary(
                    event_tx,
                    event_log_store.as_ref(),
                    project_root,
                    session_id,
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
            session.push_tool_call_record(record);

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

/// Execute one tool call through the runtime. Factored out so the batch loop
/// can run a read-only, auto-approved batch concurrently or fall back to
/// sequential execution with the same per-call setup.
#[allow(clippy::too_many_arguments)]
async fn run_tool_call<'a>(
    tc: &'a ToolCall,
    runtime: &'a ToolRuntime,
    dispatch_config: crate::tools::dispatch::ToolDispatchConfig,
    session_id: SessionId,
    turn_id: TurnId,
    hooks_config: &'a crate::storage::config::HooksConfig,
    event_tx: &'a mpsc::UnboundedSender<AgentEvent>,
    yolo_mode: bool,
    permission_mode: PermissionMode,
) -> ToolRuntimeOutcome {
    let mut context = ToolRuntimeContext::new(session_id.to_string(), dispatch_config);
    context.session_id = Some(session_id);
    context.turn_id = Some(turn_id);
    context.hooks_config = Some(hooks_config);
    let mut resolver = AgentApprovalResolver {
        event_tx,
        yolo_mode,
        permission_mode,
        subagent: None,
    };
    let mut backend = LocalDispatchRuntimeBackend;
    runtime
        .execute(
            tc,
            ToolCallSource::Main,
            context,
            &mut resolver,
            &mut backend,
        )
        .await
}

struct AgentApprovalResolver<'a> {
    event_tx: &'a mpsc::UnboundedSender<AgentEvent>,
    yolo_mode: bool,
    permission_mode: PermissionMode,
    subagent: Option<(&'a str, &'a str)>,
}

impl ApprovalResolver for AgentApprovalResolver<'_> {
    fn resolve<'a>(
        &'a mut self,
        call: &'a crate::runtime::tool_runtime::ToolCall,
        decision: &'a PolicyDecision,
    ) -> ApprovalFuture<'a> {
        Box::pin(async move {
            if let Some(outcome) = crate::agent::approval::permission_mode_approval_outcome(
                &call.tool,
                self.yolo_mode,
                self.permission_mode,
                decision,
            ) {
                return outcome;
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

    fn history_record(name: &str, arguments: &str) -> ToolCallRecord {
        ToolCallRecord {
            id: "prev".into(),
            name: name.into(),
            arguments: arguments.into(),
            result_summary: String::new(),
            exit_code: Some(0),
            duration_ms: 1,
            risk_level: "none".into(),
            approved: true,
            at: Utc::now(),
        }
    }

    #[test]
    fn would_loop_trips_only_on_consecutive_identical_tail() {
        let tc = ToolCall {
            id: "c".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "read_file".into(),
                arguments: r#"{"path":"a"}"#.into(),
            },
        };
        let same = || history_record("read_file", r#"{"path":"a"}"#);
        // Fewer than the limit, or interrupted by a different call -> no loop.
        assert!(!would_loop(&[], &tc));
        assert!(!would_loop(&[same()], &tc));
        assert!(!would_loop(
            &[same(), same(), history_record("list_files", "{}")],
            &tc
        ));
        // Different arguments don't count as the same step.
        assert!(!would_loop(
            &[
                history_record("read_file", r#"{"path":"b"}"#),
                history_record("read_file", r#"{"path":"b"}"#)
            ],
            &tc
        ));
        // Two identical calls at the tail -> the next identical call loops.
        assert!(would_loop(&[same(), same()], &tc));
    }

    #[test]
    fn would_loop_trips_on_short_oscillating_cycle() {
        // edit -> read -> edit -> read … then another edit would continue the
        // period-2 cycle with no progress.
        let edit = || history_record("edit_file", r#"{"path":"a"}"#);
        let read = || history_record("read_file", r#"{"path":"a"}"#);
        let next_edit = ToolCall {
            id: "c".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "edit_file".into(),
                arguments: r#"{"path":"a"}"#.into(),
            },
        };
        // One full cycle is not enough to trip.
        assert!(!would_loop(&[edit(), read()], &next_edit));
        // Two full cycles at the tail -> the call that continues the cycle loops.
        assert!(would_loop(&[edit(), read(), edit(), read()], &next_edit));
        // A call that breaks the period does not trip.
        let breaks_period = ToolCall {
            id: "c".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "grep".into(),
                arguments: "{}".into(),
            },
        };
        assert!(!would_loop(
            &[edit(), read(), edit(), read()],
            &breaks_period
        ));
    }

    #[tokio::test]
    async fn doom_loop_skips_a_thrice_repeated_call_without_executing_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let args = serde_json::json!({ "path": "loop.txt", "content": "x" }).to_string();
        let call = ToolCall {
            id: "c3".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "write_file".into(),
                arguments: args.clone(),
            },
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut session = test_session(temp.path());
        // Two identical prior calls already in history -> the third must skip.
        session
            .tool_call_history
            .push(history_record("write_file", &args));
        session
            .tool_call_history
            .push(history_record("write_file", &args));

        let results = ToolLoop::execute_tools_with_approval(
            &[call],
            temp.path(),
            TurnId::new_v4(),
            SubTurnId::new_v4(),
            &mut session,
            &tx,
            true,
            PermissionMode::Default,
            &crate::storage::config::PolicyConfig::default(),
            &crate::storage::config::HooksConfig::default(),
            None,
        )
        .await;

        assert_eq!(results.len(), 1);
        assert!(
            results[0].result.is_error,
            "a looping call should come back as not-run"
        );
        assert!(results[0].result.result.contains("Skipped"));
        // The side effect must not have happened.
        assert!(
            !temp.path().join("loop.txt").exists(),
            "looping write must be skipped, not executed"
        );
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
            PermissionMode::Default,
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

    #[tokio::test]
    async fn tool_loop_runs_read_only_batch_concurrently_in_order() {
        let temp = tempfile::tempdir().expect("tempdir");
        for (name, body) in [("a.txt", "alpha"), ("b.txt", "bravo"), ("c.txt", "charlie")] {
            std::fs::write(temp.path().join(name), body).expect("write fixture");
        }
        let calls: Vec<ToolCall> = ["a.txt", "b.txt", "c.txt"]
            .iter()
            .enumerate()
            .map(|(i, name)| ToolCall {
                id: format!("call-{i}"),
                call_type: "function".into(),
                function: ToolCallFunction {
                    name: "read_file".into(),
                    arguments: serde_json::json!({ "path": name }).to_string(),
                },
            })
            .collect();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut session = test_session(temp.path());

        // yolo + all read-only → concurrent fast path; results must still map
        // back to the calls in the original order.
        let results = ToolLoop::execute_tools_with_approval(
            &calls,
            temp.path(),
            TurnId::new_v4(),
            SubTurnId::new_v4(),
            &mut session,
            &tx,
            true,
            PermissionMode::Default,
            &crate::storage::config::PolicyConfig::default(),
            &crate::storage::config::HooksConfig::default(),
            None,
        )
        .await;

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].call.id, "call-0");
        assert_eq!(results[1].call.id, "call-1");
        assert_eq!(results[2].call.id, "call-2");
        assert!(results.iter().all(|r| !r.result.is_error));
        assert_eq!(session.tool_call_history.len(), 3);
    }

    #[tokio::test]
    async fn tool_loop_read_only_blocks_local_mutation_without_prompt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let call = ToolCall {
            id: "call-blocked-write".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "write_file".into(),
                arguments: serde_json::json!({
                    "path": "blocked.txt",
                    "content": "nope"
                })
                .to_string(),
            },
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut session = test_session(temp.path());

        let results = ToolLoop::execute_tools_with_approval(
            &[call],
            temp.path(),
            TurnId::new_v4(),
            SubTurnId::new_v4(),
            &mut session,
            &tx,
            false,
            PermissionMode::ReadOnly,
            &crate::storage::config::PolicyConfig::default(),
            &crate::storage::config::HooksConfig::default(),
            None,
        )
        .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].result.is_error);
        assert!(results[0].result.result.contains("read-only"));
        assert!(!temp.path().join("blocked.txt").exists());
        assert_eq!(session.tool_call_history.len(), 1);
        assert!(!session.tool_call_history[0].approved);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn tool_loop_accept_edits_auto_approves_local_file_edit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let call = ToolCall {
            id: "call-accepted-write".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "write_file".into(),
                arguments: serde_json::json!({
                    "path": "accepted.txt",
                    "content": "yes"
                })
                .to_string(),
            },
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut session = test_session(temp.path());

        let results = ToolLoop::execute_tools_with_approval(
            &[call],
            temp.path(),
            TurnId::new_v4(),
            SubTurnId::new_v4(),
            &mut session,
            &tx,
            false,
            PermissionMode::AcceptEdits,
            &crate::storage::config::PolicyConfig::default(),
            &crate::storage::config::HooksConfig::default(),
            None,
        )
        .await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].result.is_error);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("accepted.txt")).expect("read output"),
            "yes"
        );
        assert_eq!(session.tool_call_history.len(), 1);
        assert!(session.tool_call_history[0].approved);
        assert!(rx.try_recv().is_err());
    }
}

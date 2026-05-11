use std::collections::HashSet;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::mpsc;

use crate::agent::bus::MessageBus;
use crate::agent::orchestrator::AgentEvent;
use crate::agent::prompt_builder::{load_project_rules, PromptBuilder};
use crate::agent::reasoning::ReasoningManager;
use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::models::StreamChunk;
use crate::deepseek::tools as ds_tools;
use crate::deepseek::{
    ChatRequest, ExecutionLane, MessageContent, MessageId, MessageVisibility, ProtocolMessage,
    ReasoningState, Role, Session, SessionId, SessionMetadata, SubTurnId, ThinkingConfig, ToolCall,
    ToolCallRecord, ToolResultRecord, TurnId,
};

use super::types::{
    PermissionMode, SubagentConfig, SubagentResult, SubagentTask, SubagentToolArgs,
};

const MAX_SPAWN_DEPTH: u8 = 2;

/// Executes a single subagent task to completion.
///
/// This is a stripped-down version of the main orchestrator loop:
/// 1. Build an isolated session
/// 2. Stream the assistant response
/// 3. Execute any tool calls (with subagent-specific permissions)
/// 4. Follow up until no more tool calls or `max_turns` reached
/// 5. Return a summary result
#[derive(Clone)]
pub struct SubagentExecutor {
    pub client: Arc<DeepSeekClient>,
    pub project_root: PathBuf,
    pub config: SubagentConfig,
    /// Optional message bus for inter-agent coordination.
    pub bus: Option<MessageBus>,
    /// Unique ID for this agent instance (used on the bus).
    pub agent_id: String,
}

impl SubagentExecutor {
    #[must_use]
    pub fn new(client: Arc<DeepSeekClient>, project_root: PathBuf, config: SubagentConfig) -> Self {
        Self {
            client,
            project_root,
            config,
            bus: None,
            agent_id: format!("subagent-{}", uuid::Uuid::new_v4()),
        }
    }

    /// Attach a message bus for inter-agent coordination.
    #[must_use]
    pub fn with_bus(mut self, bus: MessageBus) -> Self {
        self.bus = Some(bus);
        self
    }

    fn effective_prompt_rules(&self) -> String {
        let mut parts = vec![format!(
            "## Subagent Instructions\n\n{}",
            self.config.effective_system_prompt()
        )];

        if let Some(project_rules) = load_project_rules(&self.project_root) {
            parts.push(format!("## Project Rules\n\n{project_rules}"));
        }

        parts.join("\n\n")
    }

    /// Run the subagent task and return the result.
    pub async fn run(
        &self,
        task: &SubagentTask,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> SubagentResult {
        let started_at = Utc::now();
        let start = Instant::now();

        send_event(
            event_tx,
            AgentEvent::SubagentStarted {
                agent_id: self.agent_id.clone(),
                agent_type: self.config.subagent_type.to_string(),
                description: task.description.clone(),
                is_background: self.config.is_background,
            },
        );

        // Create an isolated session
        let mut session = self.build_session(task);
        let model = self.config.effective_model();
        let lane = self
            .config
            .lane
            .clone()
            .unwrap_or(ExecutionLane::ToolLoopThinking);

        // Determine effective tools based on allowed_tools config.
        // Subagents can spawn child agents when spawn_depth < MAX_SPAWN_DEPTH.
        let can_spawn = self.config.spawn_depth < MAX_SPAWN_DEPTH;
        let all_tools = ds_tools::standard_tool_definitions();
        let effective_tools: Vec<_> = all_tools
            .into_iter()
            .filter(|t| {
                if t.function.name == "run_subagent" {
                    can_spawn
                } else {
                    true
                }
            })
            .filter(|t| {
                self.config.allowed_tools.is_empty()
                    || self.config.allowed_tools.contains(&t.function.name)
            })
            .collect();

        let mut tool_calls_used: Vec<String> = Vec::new();
        let mut files_read: Vec<String> = Vec::new();
        let mut files_written: Vec<String> = Vec::new();
        let mut token_usage: u64 = 0;
        let mut turn_count: u32 = 0;
        let mut final_output = String::new();
        let mut success = true;

        // Main loop
        loop {
            if turn_count >= self.config.max_turns {
                final_output.push_str("\n\n[Subagent reached max turns limit]");
                break;
            }
            turn_count += 1;

            let turn_id = TurnId::new_v4();
            ReasoningManager::begin_user_turn(
                &mut session.reasoning_state,
                &mut session.messages,
                turn_id,
            );

            // Build prompt
            let builder = PromptBuilder::new(model.clone(), lane.clone(), true);
            let subagent_rules = self.effective_prompt_rules();
            let (_, messages) =
                builder.build(&session, Some(&subagent_rules), None, &effective_tools);

            let thinking = Some(match lane {
                ExecutionLane::ToolLoopThinking | ExecutionLane::PlanThinking => {
                    ThinkingConfig::enabled()
                }
                _ => ThinkingConfig::disabled(),
            });

            let request = ChatRequest {
                model: model.to_string(),
                messages,
                tools: Some(effective_tools.clone()),
                thinking,
                response_format: None,
                stream: true,
                max_tokens: Some(8192),
            };

            match self
                .client
                .chat_stream_accumulated_with_deltas(&request, |chunk| {
                    emit_subagent_chunk_delta(event_tx, &self.agent_id, chunk);
                })
                .await
            {
                Ok(stream_result) => {
                    token_usage += stream_result
                        .usage
                        .as_ref()
                        .map_or(0, |u| u64::from(u.total_tokens));

                    if stream_result.tool_calls.is_empty() {
                        // No tool calls — final response
                        final_output = stream_result.content.clone();
                        let msg = ReasoningManager::new_assistant_message(
                            &stream_result.content,
                            (!stream_result.reasoning_content.is_empty())
                                .then_some(&stream_result.reasoning_content),
                            &[],
                            turn_id,
                            None,
                            false,
                        );
                        session.messages.push(msg);
                        break;
                    } else {
                        // Collect tool call info
                        for tc in &stream_result.tool_calls {
                            tool_calls_used.push(tc.function.name.clone());
                            if tc.function.name == "read_file" {
                                if let Ok(args) = serde_json::from_str::<serde_json::Value>(
                                    &tc.function.arguments,
                                ) {
                                    if let Some(path) = args["path"].as_str() {
                                        files_read.push(path.to_string());
                                    }
                                }
                            }
                            if tc.function.name == "write_file" || tc.function.name == "edit_file" {
                                if let Ok(args) = serde_json::from_str::<serde_json::Value>(
                                    &tc.function.arguments,
                                ) {
                                    if let Some(path) = args["path"].as_str() {
                                        files_written.push(path.to_string());
                                    }
                                }
                            }
                        }

                        // Execute tools
                        let results = self
                            .execute_tool_calls(
                                &stream_result.tool_calls,
                                turn_id,
                                &mut session,
                                event_tx,
                            )
                            .await;

                        // Check for errors
                        if results.iter().any(|(_, r)| r.is_error) {
                            success = false;
                        }

                        // Add assistant message
                        let assistant_msg = ReasoningManager::new_assistant_message(
                            &stream_result.content,
                            (!stream_result.reasoning_content.is_empty())
                                .then_some(&stream_result.reasoning_content),
                            &stream_result.tool_calls,
                            turn_id,
                            None,
                            true,
                        );
                        session.messages.push(assistant_msg);

                        // Add tool results
                        for (_tc, record) in &results {
                            let tool_msg = ReasoningManager::new_tool_result_message(
                                &record.tool_call_id,
                                &record.name,
                                &record.result,
                                record.is_error,
                                turn_id,
                                SubTurnId::new_v4(),
                            );
                            session.messages.push(tool_msg);
                        }

                        // Continue loop for follow-up
                        continue;
                    }
                }
                Err(e) => {
                    success = false;
                    final_output = format!("Subagent error: {e}");
                    break;
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let completed_at = Utc::now();

        let summary = if final_output.chars().count() > 300 {
            let prefix: String = final_output.chars().take(300).collect();
            format!("{prefix}...")
        } else {
            final_output.clone()
        };

        let result = SubagentResult {
            success,
            summary,
            output: final_output,
            tool_calls_used: dedupe_preserving_order(tool_calls_used),
            files_read: dedupe_preserving_order(files_read),
            files_written: dedupe_preserving_order(files_written),
            duration_ms,
            token_usage,
            error: (!success).then(|| "Subagent encountered errors during execution".to_string()),
            started_at,
            completed_at,
        };

        send_event(
            event_tx,
            AgentEvent::SubagentCompleted {
                agent_id: self.agent_id.clone(),
                result: result.clone(),
            },
        );

        result
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn build_session(&self, task: &SubagentTask) -> Session {
        let mut session = Session {
            id: SessionId::new_v4(),
            name: Some(format!("subagent-{}", self.config.subagent_type)),
            project_root: self.project_root.clone(),
            messages: Vec::new(),
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: SessionMetadata::default(),
        };

        // Add the task as the first user message
        let mut user_content = task.prompt.clone();
        if let Some(ctx) = &task.context {
            let _ = write!(user_content, "\n\n## Context\n\n{ctx}");
        }
        if !task.focus_files.is_empty() {
            let _ = write!(
                user_content,
                "\n\n## Focus Files\n\n{}",
                task.focus_files.join("\n")
            );
        }

        session.messages.push(ProtocolMessage {
            id: MessageId::new_v4(),
            role: Role::User,
            content: MessageContent::from(user_content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: TurnId::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        });

        session
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: &[ToolCall],
        _turn_id: TurnId,
        session: &mut Session,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Vec<(ToolCall, ToolResultRecord)> {
        let mut results = Vec::new();
        let policy_config = crate::storage::Config::load(Some(&self.project_root))
            .map(|c| c.policy)
            .unwrap_or_default();
        let dispatch_config =
            crate::tools::dispatch::ToolDispatchConfig::from_policy(&policy_config);

        for tc in tool_calls {
            // Delegate run_subagent to a child handler (Feature 3: recursive spawning).
            // The free function `run_nested_subagent` breaks the opaque-type-in-defining-scope
            // limitation, allowing Rust to verify the Send bound without a circular inference.
            if tc.function.name == "run_subagent" {
                let result_text: String = if self.config.spawn_depth < MAX_SPAWN_DEPTH {
                    run_nested_subagent(
                        self.client.clone(),
                        self.project_root.clone(),
                        self.config.spawn_depth + 1,
                        tc.function.arguments.clone(),
                        event_tx.clone(),
                    )
                    .await
                } else {
                    "Subagent spawning is not allowed at this nesting depth.".to_string()
                };
                results.push((
                    tc.clone(),
                    ToolResultRecord {
                        tool_call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        result: result_text,
                        is_error: false,
                    },
                ));
                continue;
            }

            // Filter by allowed_tools
            if !self.config.allowed_tools.is_empty()
                && !self.config.allowed_tools.contains(&tc.function.name)
            {
                let record = ToolResultRecord {
                    tool_call_id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    result: format!(
                        "Tool '{}' is not allowed for this subagent (type: {}).",
                        tc.function.name, self.config.subagent_type
                    ),
                    is_error: true,
                };
                results.push((tc.clone(), record));
                continue;
            }

            // Filter by permission mode
            match self.config.permission_mode {
                PermissionMode::ReadOnly => {
                    if tc.function.name == "write_file"
                        || tc.function.name == "edit_file"
                        || tc.function.name == "apply_patch"
                        || tc.function.name == "run_command"
                    {
                        let record = ToolResultRecord {
                            tool_call_id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            result: format!(
                                "Tool '{}' is blocked: subagent is in read-only mode.",
                                tc.function.name
                            ),
                            is_error: true,
                        };
                        results.push((tc.clone(), record));
                        continue;
                    }
                }
                PermissionMode::Bypass => {
                    // Allow everything
                }
                PermissionMode::AcceptEdits => {
                    if tc.function.name == "run_command" {
                        // Still require approval — fall through to normal execution
                    }
                }
                PermissionMode::Default => {
                    // Normal approval flow
                }
            }

            // Determine approval based on permission mode.
            // Bypass and AcceptEdits (for non-command tools) skip the popup entirely.
            // Only Default mode routes to the TUI approval popup and waits.
            let is_command = tc.function.name == "run_command";
            let approved = match self.config.permission_mode {
                PermissionMode::Bypass => true,
                PermissionMode::ReadOnly => {
                    // Safety net — destructive tools already filtered above.
                    !matches!(
                        tc.function.name.as_str(),
                        "write_file" | "edit_file" | "apply_patch" | "run_command"
                    )
                }
                PermissionMode::AcceptEdits if !is_command => true,
                _ => {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    send_event(
                        event_tx,
                        AgentEvent::SubagentToolApprovalNeeded {
                            agent_id: self.agent_id.clone(),
                            tool_name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                            respond: tx,
                        },
                    );
                    matches!(
                        tokio::time::timeout(Duration::from_mins(1), rx).await,
                        Ok(Ok(true))
                    )
                }
            };

            if !approved {
                let record = ToolResultRecord {
                    tool_call_id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    result: format!("Tool '{}' was denied.", tc.function.name),
                    is_error: true,
                };
                results.push((tc.clone(), record));
                continue;
            }

            // File-lock coordination for write operations in parallel mode
            // RAII guard ensures unlock on Drop even if the tool panics or errors.
            struct FileLockGuard<'a> {
                bus: &'a MessageBus,
                agent_id: &'a str,
                path: String,
            }

            impl Drop for FileLockGuard<'_> {
                fn drop(&mut self) {
                    self.bus.announce_file_unlock(self.agent_id, &self.path);
                }
            }

            let _lock_guard: Option<FileLockGuard<'_>> = if let Some(ref bus) = self.bus {
                if tc.function.name == "write_file" || tc.function.name == "edit_file" {
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                    if let Some(path) = args["path"].as_str() {
                        if bus.is_file_locked(path) {
                            let record = ToolResultRecord {
                                tool_call_id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                result: format!(
                                    "File '{path}' is currently locked by another subagent. Wait for it to complete."
                                ),
                                is_error: true,
                            };
                            results.push((tc.clone(), record));
                            continue;
                        }
                        bus.announce_file_lock(&self.agent_id, path);
                        Some(FileLockGuard {
                            bus,
                            agent_id: &self.agent_id,
                            path: path.to_string(),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Actually execute the tool
            let (result_text, is_error) = crate::tools::dispatch::execute_single_tool_with_config(
                tc,
                &self.project_root,
                dispatch_config,
            )
            .await;

            let record = ToolResultRecord {
                tool_call_id: tc.id.clone(),
                name: tc.function.name.clone(),
                result: result_text,
                is_error,
            };
            results.push((tc.clone(), record));
        }

        // Record in session history
        for (tc, record) in &results {
            let tool_record = ToolCallRecord {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
                result_summary: crate::agent::utils::truncate_for_summary(&record.result, 200),
                exit_code: if record.is_error { Some(1) } else { Some(0) },
                duration_ms: 0,
                risk_level: crate::agent::utils::risk_level_for_tool(&tc.function.name).to_string(),
                approved: true,
                at: Utc::now(),
            };
            session.tool_call_history.push(tool_record);
        }

        results
    }

    // execute_single_tool moved to crate::tools::dispatch
}

/// Run a child subagent for recursive agent spawning (Feature 3).
///
/// Returns `Pin<Box<dyn Future + Send>>` (not `async fn`) so `execute_tool_calls` holds
/// a concrete `Send` type across its await point, breaking the circular Send inference
/// that would otherwise arise from `run_parallel → spawn → run → execute_tool_calls → handle → run_parallel`.
fn run_nested_subagent(
    client: Arc<DeepSeekClient>,
    project_root: PathBuf,
    child_depth: u8,
    args_str: String,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>> {
    Box::pin(async move {
        let handler = crate::agent::task_tool::TaskToolHandler::new(
            client,
            project_root,
            crate::agent::background::BackgroundQueue::new(),
            child_depth,
        );
        match serde_json::from_str::<SubagentToolArgs>(&args_str) {
            Ok(args) => {
                let (text, _) = handler.handle(&args, &event_tx, None).await;
                text
            }
            Err(e) => format!("Failed to parse run_subagent arguments: {e}"),
        }
    })
}

fn send_event(tx: &mpsc::UnboundedSender<AgentEvent>, event: AgentEvent) {
    if tx.send(event).is_err() {
        tracing::warn!("Agent event channel closed; event dropped");
    }
}

fn emit_subagent_chunk_delta(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    agent_id: &str,
    chunk: &StreamChunk,
) {
    for choice in &chunk.choices {
        if let Some(content) = &choice.delta.content {
            if !content.trim().is_empty() {
                send_event(
                    tx,
                    AgentEvent::SubagentDelta {
                        agent_id: agent_id.to_string(),
                        content: content.clone(),
                    },
                );
            }
        }
        if let Some(reasoning) = &choice.delta.reasoning_content {
            if !reasoning.trim().is_empty() {
                send_event(
                    tx,
                    AgentEvent::SubagentDelta {
                        agent_id: agent_id.to_string(),
                        content: format!("thinking: {reasoning}"),
                    },
                );
            }
        }
    }
}

fn dedupe_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

// helpers moved to crate::agent::utils

#[cfg(test)]
mod tests {
    use super::{dedupe_preserving_order, emit_subagent_chunk_delta};
    use crate::agent::orchestrator::AgentEvent;
    use crate::deepseek::models::StreamChunk;

    #[test]
    fn dedupe_preserving_order_keeps_first_occurrence() {
        let values = vec![
            "read_file".to_string(),
            "edit_file".to_string(),
            "read_file".to_string(),
        ];

        assert_eq!(
            dedupe_preserving_order(values),
            vec!["read_file".to_string(), "edit_file".to_string()]
        );
    }

    #[test]
    fn subagent_stream_chunks_emit_card_updates() {
        let chunk = serde_json::from_str::<StreamChunk>(
            r#"{"choices":[{"index":0,"delta":{"content":"working"},"finish_reason":null}],"usage":null}"#,
        )
        .expect("valid stream chunk");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_subagent_chunk_delta(&tx, "agent-1", &chunk);

        assert!(matches!(
            rx.try_recv().expect("subagent delta"),
            AgentEvent::SubagentDelta { agent_id, content }
                if agent_id == "agent-1" && content == "working"
        ));
    }
}

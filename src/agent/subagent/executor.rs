use std::collections::{HashMap, HashSet};
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
use crate::deepseek::client::{ChatStreamClient, DeepSeekClient};
use crate::deepseek::models::StreamChunk;
use crate::deepseek::tools as ds_tools;
use crate::deepseek::{
    temperature_for_lane, ChatRequest, ExecutionLane, MessageContent, MessageId, MessageVisibility,
    ProtocolMessage, ReasoningState, Role, Session, SessionId, SessionMetadata, SubTurnId,
    ThinkingConfig, ToolCall, ToolCallRecord, ToolResultRecord, TurnId,
};
use crate::runtime::tool_runtime::{
    ApprovalFuture, ApprovalOutcome, ApprovalResolver, ToolRuntime, ToolRuntimeBackend,
    ToolRuntimeBackendFuture, ToolRuntimeContext,
};
use crate::tools::backend::{ToolExecutionContext, ToolExecutionResult};

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

        // Detected stack, so a subagent (e.g. test-runner) uses project-native
        // commands instead of guessing — mirrors the main agent's prompt.
        if let Some(profile) =
            crate::agent::project_profile::ProjectProfile::detect(&self.project_root).render()
        {
            parts.push(profile);
        }

        parts.join("\n\n")
    }

    /// Run the subagent task and return the result.
    pub async fn run(
        &self,
        task: &SubagentTask,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> SubagentResult {
        self.run_with_client(&*self.client, task, event_tx).await
    }

    /// Run the subagent's tool loop against an injected streaming client. The
    /// public [`run`] delegates here with the real client; tests pass a mock so
    /// the loop (turns, max-turn limit, tool follow-up) runs without a network
    /// call.
    pub async fn run_with_client(
        &self,
        client: &dyn ChatStreamClient,
        task: &SubagentTask,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> SubagentResult {
        let sanitized_task = sanitize_subagent_task(task);
        let task = &sanitized_task;
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
        let runtime_config =
            crate::storage::Config::load(Some(&self.project_root)).unwrap_or_default();
        let subagent_rules = self.effective_prompt_rules();
        let ctx_budget = crate::provider::context_budget_for(
            runtime_config.provider.default,
            &model,
            runtime_config.search.max_context_tokens,
        )
        .effective_budget_tokens;

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
                    || subagent_allows_tool_name(&self.config.allowed_tools, &t.function.name)
            })
            .collect();

        let mut tool_calls_used: Vec<String> = Vec::new();
        let mut files_read: Vec<String> = Vec::new();
        let mut files_written: Vec<String> = Vec::new();
        let mut recovered_tool_errors: Vec<String> = Vec::new();
        let mut token_usage: u64 = 0;
        let mut turn_count: u32 = 0;
        let mut success = true;
        let turn_id = TurnId::new_v4();
        ReasoningManager::begin_user_turn(
            &mut session.reasoning_state,
            &mut session.messages,
            turn_id,
        );

        // Main loop
        let final_output = loop {
            if turn_count >= self.config.max_turns {
                success = false;
                break structured_subagent_limit_message(task, self.config.max_turns);
            }
            turn_count += 1;

            // Build prompt
            let builder = PromptBuilder::new(model.clone(), lane.clone(), true)
                .with_context_budget(ctx_budget);
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
                // Same 32K budget rationale as the parent turn — reasoning
                // can chew through 8K alone, leaving no room for the answer.
                max_tokens: Some(32_768),
                temperature: temperature_for_lane(&lane),
            };

            send_request_token_delta(event_tx, &request);
            match client
                .stream_chat(&request, &mut |chunk| {
                    emit_subagent_chunk_delta(event_tx, &self.agent_id, chunk);
                })
                .await
            {
                Ok(stream_result) => {
                    token_usage += stream_result
                        .usage
                        .as_ref()
                        .map_or(0, |u| u64::from(u.total_tokens));
                    // Persist this subagent call's usage to the same telemetry
                    // store the main loop uses, so swarm/subagent spend counts
                    // toward usage/quota tracking instead of vanishing.
                    if let Some(ref usage) = stream_result.usage {
                        let provider_name = runtime_config.provider.default.as_str().to_string();
                        let model_name = crate::provider::request_model_name_for_config(
                            &runtime_config.provider,
                            &model,
                        );
                        if let Err(error) = crate::storage::record_usage_event(
                            &self.project_root,
                            &provider_name,
                            &model_name,
                            usage,
                        ) {
                            tracing::warn!("failed to record subagent usage event: {error}");
                        }
                    }

                    if stream_result.tool_calls.is_empty() {
                        // No tool calls — final response
                        ReasoningManager::complete_tool_loop(
                            &mut session.reasoning_state,
                            &mut session.messages,
                        );
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
                        break stream_result.content.clone();
                    } else {
                        // Collect tool call info
                        for tc in &stream_result.tool_calls {
                            tool_calls_used.push(tc.function.name.clone());
                        }

                        let sub_turn_id = SubTurnId::new_v4();
                        let assistant_msg = ReasoningManager::new_assistant_message(
                            &stream_result.content,
                            (!stream_result.reasoning_content.is_empty())
                                .then_some(&stream_result.reasoning_content),
                            &stream_result.tool_calls,
                            turn_id,
                            Some(sub_turn_id),
                            true,
                        );
                        session.messages.push(assistant_msg);
                        if let Some(last_msg) = session.messages.last() {
                            ReasoningManager::begin_tool_turn(
                                &mut session.reasoning_state,
                                last_msg,
                            );
                        }

                        // Execute tools
                        let results = self
                            .execute_tool_calls(
                                &stream_result.tool_calls,
                                turn_id,
                                &mut session,
                                &runtime_config,
                                event_tx,
                            )
                            .await;

                        for (tc, record) in &results {
                            if record.is_error {
                                recovered_tool_errors.push(format!(
                                    "{}: {}",
                                    record.name,
                                    classify_tool_result_error(&record.result)
                                ));
                            } else {
                                collect_successful_file_access(
                                    tc,
                                    &mut files_read,
                                    &mut files_written,
                                );
                            }
                        }

                        // Add tool results, trimming any single oversized
                        // result so one huge read can't dominate the context.
                        for (_tc, record) in &results {
                            let trimmed = trim_tool_result(&record.result);
                            let tool_msg = ReasoningManager::new_tool_result_message(
                                &record.tool_call_id,
                                &record.name,
                                &trimmed,
                                record.is_error,
                                turn_id,
                                sub_turn_id,
                            );
                            session.messages.push(tool_msg);
                        }

                        // Keep the running history within the model's context
                        // budget so long tool loops don't overflow before the
                        // subagent can finish.
                        compact_subagent_messages(
                            &mut session.messages,
                            subagent_context_budget(&model),
                        );

                        // Continue loop for follow-up
                        continue;
                    }
                }
                Err(e) => {
                    success = false;
                    break structured_subagent_error_message(task, &e.to_string());
                }
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;
        let completed_at = Utc::now();

        let summary = if final_output.chars().count() > 300 {
            let prefix: String = final_output.chars().take(300).collect();
            format!("{prefix}...")
        } else {
            final_output.clone()
        };

        let error = if success {
            (!recovered_tool_errors.is_empty()).then(|| {
                format!(
                    "Recovered tool errors: {}",
                    dedupe_preserving_order(recovered_tool_errors).join("; ")
                )
            })
        } else {
            Some(summarize_subagent_failure_for_parent(&final_output))
        };

        let result = SubagentResult {
            success,
            summary,
            output: final_output.clone(),
            tool_calls_used: dedupe_preserving_order(tool_calls_used),
            files_read: dedupe_preserving_order(files_read),
            files_written: dedupe_preserving_order(files_written),
            duration_ms,
            token_usage,
            error,
            started_at,
            completed_at,
            worktree: None,
        };

        send_event(
            event_tx,
            AgentEvent::SubagentCompleted {
                agent_id: self.agent_id.clone(),
                result: result.clone(),
            },
        );

        if !runtime_config.hooks.subagent_stop.is_empty() {
            let mut payload = crate::hooks::HookPayload::new(
                crate::hooks::HookEvent::SubagentStop,
                self.agent_id.clone(),
                self.project_root.clone(),
            );
            payload.success = Some(result.success);
            payload.summary = Some(result.summary.clone());
            payload.duration_ms = Some(result.duration_ms);
            let _ = crate::hooks::run_configured_hooks(
                crate::hooks::HookEvent::SubagentStop,
                &runtime_config.hooks,
                &payload,
                &self.project_root,
                runtime_config.policy.command_timeout_seconds.max(5),
            )
            .await;
        }

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
        if let Some(expected_output) = &task.expected_output {
            if !expected_output.trim().is_empty() {
                let _ = write!(user_content, "\n\n## Expected Output\n\n{expected_output}");
            }
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
        runtime_config: &crate::storage::Config,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Vec<(ToolCall, ToolResultRecord)> {
        let mut results = Vec::new();
        let mut audit_meta: HashMap<String, ToolAuditMeta> = HashMap::new();
        let policy_config = runtime_config.policy.clone();
        let hooks_config = runtime_config.hooks.clone();
        let dispatch_config =
            crate::tools::dispatch::ToolDispatchConfig::from_policy(&policy_config);
        let runtime = ToolRuntime::new(self.project_root.clone(), policy_config.clone());
        let shared_permission_mode = shared_permission_mode(self.config.permission_mode.clone());

        for tc in tool_calls {
            // Filter by allowed_tools
            if !subagent_allows_tool_call(&self.config.allowed_tools, tc) {
                push_subagent_tool_result(
                    &mut results,
                    &mut audit_meta,
                    tc,
                    format!(
                        "Tool '{}' is not allowed for this subagent (type: {}).",
                        tc.function.name, self.config.subagent_type
                    ),
                    true,
                    false,
                    crate::policy::RiskLevel::Blocked.to_string(),
                );
                continue;
            }

            // Filter by permission mode
            match self.config.permission_mode {
                PermissionMode::ReadOnly => {
                    if !read_only_allows_tool(&tc.function.name) {
                        push_subagent_tool_result(
                            &mut results,
                            &mut audit_meta,
                            tc,
                            format!(
                                "Tool '{}' is blocked: subagent is in read-only mode.",
                                tc.function.name
                            ),
                            true,
                            false,
                            crate::policy::RiskLevel::Blocked.to_string(),
                        );
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

            let mut context =
                ToolRuntimeContext::new(session.id.to_string(), dispatch_config.clone());
            context.hooks_config = Some(&hooks_config);
            context.hook_tool_name = Some(format!("{}.{}", self.agent_id, tc.function.name));
            let mut resolver = SubagentApprovalResolver {
                event_tx,
                agent_id: &self.agent_id,
                permission_mode: shared_permission_mode,
                arguments: &tc.function.arguments,
            };
            let mut backend = SubagentRuntimeBackend {
                client: self.client.clone(),
                project_root: self.project_root.clone(),
                spawn_depth: self.config.spawn_depth,
                bus: self.bus.as_ref(),
                agent_id: &self.agent_id,
                event_tx: event_tx.clone(),
            };
            let outcome = runtime
                .execute(
                    tc,
                    crate::policy::ToolCallSource::Subagent,
                    context,
                    &mut resolver,
                    &mut backend,
                )
                .await;
            for hook_summary in &outcome.hook_summaries {
                send_event(
                    event_tx,
                    AgentEvent::HookExecuted {
                        event: hook_summary.event,
                        success: hook_summary.success(),
                        summary: hook_summary.brief(),
                        command_count: hook_summary.outcomes.len(),
                    },
                );
            }
            audit_meta.insert(
                tc.id.clone(),
                ToolAuditMeta {
                    approved: outcome.approval.approved(),
                    risk_level: outcome.decision.display.risk_level.to_string(),
                },
            );
            results.push((tc.clone(), outcome.result_record));
        }

        // Record in session history
        for (tc, record) in &results {
            let meta = audit_meta
                .get(&tc.id)
                .cloned()
                .unwrap_or_else(|| ToolAuditMeta {
                    approved: false,
                    risk_level: crate::policy::RiskLevel::Blocked.to_string(),
                });
            let tool_record = ToolCallRecord {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
                result_summary: crate::agent::utils::truncate_for_summary(&record.result, 200),
                exit_code: if record.is_error { Some(1) } else { Some(0) },
                duration_ms: 0,
                risk_level: meta.risk_level,
                approved: meta.approved,
                at: Utc::now(),
            };
            session.tool_call_history.push(tool_record);
        }

        results
    }

    // execute_single_tool moved to crate::tools::dispatch
}

#[derive(Debug, Clone)]
struct ToolAuditMeta {
    approved: bool,
    risk_level: String,
}

fn shared_permission_mode(mode: PermissionMode) -> crate::policy::PermissionMode {
    match mode {
        PermissionMode::Default => crate::policy::PermissionMode::Default,
        PermissionMode::AcceptEdits => crate::policy::PermissionMode::AcceptEdits,
        PermissionMode::ReadOnly => crate::policy::PermissionMode::ReadOnly,
        PermissionMode::Bypass => crate::policy::PermissionMode::Bypass,
    }
}

struct SubagentApprovalResolver<'a> {
    event_tx: &'a mpsc::UnboundedSender<AgentEvent>,
    agent_id: &'a str,
    permission_mode: crate::policy::PermissionMode,
    arguments: &'a str,
}

impl ApprovalResolver for SubagentApprovalResolver<'_> {
    fn resolve<'a>(
        &'a mut self,
        call: &'a crate::runtime::tool_runtime::ToolCall,
        decision: &'a crate::policy::PolicyDecision,
    ) -> ApprovalFuture<'a> {
        Box::pin(async move {
            if self.permission_mode.auto_approves(&call.tool, decision) {
                return ApprovalOutcome::Approved;
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            send_event(
                self.event_tx,
                AgentEvent::SubagentToolApprovalNeeded {
                    agent_id: self.agent_id.to_string(),
                    tool_name: call.tool.clone(),
                    arguments: self.arguments.to_string(),
                    policy_decision: decision.clone(),
                    respond: tx,
                },
            );
            match tokio::time::timeout(Duration::from_secs(60), rx).await {
                Ok(Ok(true)) => ApprovalOutcome::Approved,
                Ok(Ok(false)) => ApprovalOutcome::denied("Denied by user"),
                Ok(Err(_)) => ApprovalOutcome::denied("Approval channel closed"),
                Err(_) => ApprovalOutcome::denied("Denied by user or timeout"),
            }
        })
    }
}

struct SubagentRuntimeBackend<'a> {
    client: Arc<DeepSeekClient>,
    project_root: PathBuf,
    spawn_depth: u8,
    bus: Option<&'a MessageBus>,
    agent_id: &'a str,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
}

impl ToolRuntimeBackend for SubagentRuntimeBackend<'_> {
    fn execute<'a>(
        &'a mut self,
        call: &'a ToolCall,
        context: &'a ToolExecutionContext,
    ) -> ToolRuntimeBackendFuture<'a> {
        Box::pin(async move {
            let start = Instant::now();
            let (content, success) = if call.function.name == "run_subagent" {
                if self.spawn_depth < MAX_SPAWN_DEPTH {
                    let (content, is_error) = run_nested_subagent(
                        self.client.clone(),
                        self.project_root.clone(),
                        self.spawn_depth + 1,
                        call.function.arguments.clone(),
                        self.event_tx.clone(),
                    )
                    .await;
                    (content, !is_error)
                } else {
                    (
                        "Subagent spawning is not allowed at this nesting depth.".to_string(),
                        false,
                    )
                }
            } else {
                self.execute_local_with_locks(call, context).await
            };
            ToolExecutionResult {
                success,
                summary: crate::agent::utils::truncate_for_summary(&content, 200),
                content,
                changed_files: crate::tools::backend::changed_files_for_call(call),
                duration_ms: start.elapsed().as_millis() as u64,
            }
        })
    }
}

impl SubagentRuntimeBackend<'_> {
    async fn execute_local_with_locks(
        &self,
        call: &ToolCall,
        context: &ToolExecutionContext,
    ) -> (String, bool) {
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

        let _lock_guard = if let Some(bus) = self.bus {
            if matches!(
                call.function.name.as_str(),
                "write_file" | "edit_file" | "notebook_edit"
            ) {
                let args: serde_json::Value = match serde_json::from_str(&call.function.arguments) {
                    Ok(args) => args,
                    Err(error) => {
                        return (
                            format!("Tool arguments could not be parsed for file locking: {error}"),
                            false,
                        );
                    }
                };
                if let Some(path) = args["path"].as_str() {
                    if bus.is_file_locked(path) {
                        return (
                            format!(
                                "File '{path}' is currently locked by another subagent. Wait for it to complete."
                            ),
                            false,
                        );
                    }
                    bus.announce_file_lock(self.agent_id, path);
                    Some(FileLockGuard {
                        bus,
                        agent_id: self.agent_id,
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

        let (content, is_error) = crate::tools::dispatch::execute_single_tool_with_config(
            call,
            &context.project_root,
            context.dispatch_config.clone(),
        )
        .await;
        (content, !is_error)
    }
}

fn push_subagent_tool_result(
    results: &mut Vec<(ToolCall, ToolResultRecord)>,
    audit_meta: &mut HashMap<String, ToolAuditMeta>,
    tc: &ToolCall,
    result: String,
    is_error: bool,
    approved: bool,
    risk_level: String,
) {
    audit_meta.insert(
        tc.id.clone(),
        ToolAuditMeta {
            approved,
            risk_level,
        },
    );
    results.push((
        tc.clone(),
        ToolResultRecord {
            tool_call_id: tc.id.clone(),
            name: tc.function.name.clone(),
            result,
            is_error,
        },
    ));
}

fn sanitize_subagent_task(task: &SubagentTask) -> SubagentTask {
    let defense = crate::defense::DefenseProtocol::default();
    let mut sanitized = task.clone();
    sanitized.description = defense.sanitize_input(&task.description).safe;
    sanitized.prompt = defense.sanitize_input(&task.prompt).safe;
    sanitized.context = task
        .context
        .as_ref()
        .map(|context| defense.sanitize_input(context).safe);
    sanitized.expected_output = task
        .expected_output
        .as_ref()
        .map(|expected_output| defense.sanitize_input(expected_output).safe);
    sanitized
}

fn collect_successful_file_access(
    tool_call: &ToolCall,
    files_read: &mut Vec<String>,
    files_written: &mut Vec<String>,
) {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) else {
        return;
    };
    let Some(path) = args["path"].as_str() else {
        return;
    };

    match tool_call.function.name.as_str() {
        "read_file" => files_read.push(path.to_string()),
        "write_file" | "edit_file" | "notebook_edit" => files_written.push(path.to_string()),
        _ => {}
    }
}

fn read_only_allows_tool(tool_name: &str) -> bool {
    // Single source of truth: `crate::tools::metadata::ALL_TOOLS`.
    crate::tools::metadata::is_read_only(tool_name)
}

fn subagent_allows_tool_name(allowed_tools: &[String], tool_name: &str) -> bool {
    allowed_tools.is_empty()
        || crate::policy::approvals::allowed_tools_include_tool_name(allowed_tools, tool_name)
}

fn subagent_allows_tool_call(allowed_tools: &[String], tool_call: &ToolCall) -> bool {
    if allowed_tools.is_empty() {
        return true;
    }
    let args = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
        .unwrap_or_else(|_| serde_json::json!({}));
    crate::policy::approvals::allowed_tools_match_tool_call(
        Some(allowed_tools),
        &tool_call.function.name,
        &args,
    )
}

fn structured_subagent_limit_message(task: &SubagentTask, _max_turns: u32) -> String {
    if subagent_task_uses_chinese(task) {
        format!(
            "子任务停止：已用完轮次预算。\n任务：{}\n建议：提高 --max-turns 或缩小任务范围后重试。",
            task.description
        )
    } else {
        format!(
            "Subtask stopped because the turn budget was exhausted.\nTask: {}\nNext: increase --max-turns or narrow the scope and retry.",
            task.description
        )
    }
}

fn structured_subagent_error_message(task: &SubagentTask, error: &str) -> String {
    let (classified, cause_zh, cause_en) = classify_subagent_error(error);
    let detail = safe_subagent_error_detail(error, cause_en);
    if subagent_task_uses_chinese(task) {
        format!(
            "{classified}。\n任务：{}\n原因：{cause_zh}。\n细节：{detail}。",
            task.description
        )
    } else {
        format!(
            "Subtask failed.\nTask: {}\nCause: {cause_en}.\nDetail: {detail}.",
            task.description
        )
    }
}

fn classify_subagent_error(error: &str) -> (&'static str, &'static str, &'static str) {
    let lower = error.to_lowercase();
    if lower.contains("eof while parsing")
        || lower.contains("parse error")
        || lower.contains("expected value")
        || lower.contains("expected ident")
    {
        (
            "工具调用参数解析失败",
            "工具调用参数解析失败",
            "tool call argument parse failed",
        )
    } else if lower.contains("context") && lower.contains("length") {
        (
            "子任务上下文过长",
            "上下文长度超限",
            "context length exceeded",
        )
    } else if lower.contains("rate limit") || lower.contains("429") {
        (
            "模型请求限流",
            "模型请求被限流",
            "model request was rate limited",
        )
    } else if lower.contains("timeout") || lower.contains("timed out") {
        ("模型请求超时", "模型请求超时", "model request timed out")
    } else if lower.contains("unauthorized")
        || lower.contains("401")
        || lower.contains("api key")
        || lower.contains("authentication")
    {
        (
            "模型认证失败",
            "模型认证失败",
            "model authentication failed",
        )
    } else {
        (
            "子任务执行失败",
            "子任务请求失败",
            "subagent request failed",
        )
    }
}

fn safe_subagent_error_detail(error: &str, cause_en: &str) -> String {
    if cause_en == "tool call argument parse failed" {
        return cause_en.to_string();
    }
    let first_line = error
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or("unknown error");
    let redacted = crate::policy::redact_all(first_line);
    truncate_chars(&redacted, 220)
}

fn classify_tool_result_error(result: &str) -> &'static str {
    let lower = result.to_lowercase();
    if lower.contains("not allowed") || lower.contains("denied") || lower.contains("approval") {
        "tool call was denied by policy"
    } else if lower.contains("no such file") || lower.contains("not found") {
        "requested path was not found"
    } else if lower.contains("parse") || lower.contains("invalid") {
        "tool arguments were invalid"
    } else {
        "tool call returned an error"
    }
}

fn summarize_subagent_failure_for_parent(output: &str) -> String {
    output
        .lines()
        .next()
        .unwrap_or("子任务执行失败")
        .to_string()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn subagent_task_uses_chinese(task: &SubagentTask) -> bool {
    contains_cjk_local(&task.description)
        || contains_cjk_local(&task.prompt)
        || task.context.as_deref().is_some_and(contains_cjk_local)
        || task
            .expected_output
            .as_deref()
            .is_some_and(contains_cjk_local)
}

fn contains_cjk_local(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch as u32,
            0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF
        )
    })
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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = (String, bool)> + Send>> {
    Box::pin(async move {
        let handler = crate::agent::task_tool::TaskToolHandler::new(
            client,
            project_root,
            crate::agent::background::BackgroundQueue::new(),
            child_depth,
        );
        match serde_json::from_str::<SubagentToolArgs>(&args_str) {
            Ok(args) => {
                let (text, is_error) = handler.handle(&args, &event_tx, None).await;
                (text, is_error)
            }
            Err(e) => (format!("Failed to parse run_subagent arguments: {e}"), true),
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
        let mut hidden_delta = String::new();
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
            hidden_delta.push_str(reasoning);
        }
        if let Some(tool_calls) = &choice.delta.tool_calls {
            for tool_call in tool_calls {
                if let Some(function) = &tool_call.function {
                    if let Some(name) = &function.name {
                        hidden_delta.push_str(name);
                    }
                    if let Some(arguments) = &function.arguments {
                        hidden_delta.push_str(arguments);
                    }
                }
            }
        }
        let output_tokens = estimate_stream_tokens(&hidden_delta);
        if output_tokens > 0 {
            send_event(
                tx,
                AgentEvent::TokenDelta {
                    input_tokens: 0,
                    output_tokens,
                },
            );
        }
        // Reasoning chunks are internal. Subagent cards show progress and results only.
    }
}

fn send_request_token_delta(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    request: &crate::deepseek::models::ChatRequest,
) {
    let input_tokens = crate::deepseek::models::estimate_chat_request_tokens(request);
    if input_tokens > 0 {
        send_event(
            tx,
            AgentEvent::TokenDelta {
                input_tokens,
                output_tokens: 0,
            },
        );
    }
}

fn estimate_stream_tokens(value: &str) -> u64 {
    crate::deepseek::models::estimate_tokenish_count(value)
}

fn dedupe_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

/// Largest single tool result (in chars) kept verbatim in a subagent's
/// history. A pathologically large result is clipped with a marker so one
/// read can't crowd out the rest of the conversation.
const MAX_SUBAGENT_TOOL_RESULT_CHARS: usize = 12_000;

fn trim_tool_result(result: &str) -> String {
    if result.chars().count() <= MAX_SUBAGENT_TOOL_RESULT_CHARS {
        return result.to_string();
    }
    let kept: String = result
        .chars()
        .take(MAX_SUBAGENT_TOOL_RESULT_CHARS)
        .collect();
    let dropped = result.chars().count() - MAX_SUBAGENT_TOOL_RESULT_CHARS;
    format!("{kept}\n[... {dropped} more chars trimmed to fit context ...]")
}

/// Token budget for a subagent's running message history, kept under the
/// prompt builder's hard prune limit so compaction happens gracefully first.
fn subagent_context_budget(model: &crate::deepseek::DeepSeekModel) -> usize {
    use crate::deepseek::DeepSeekModel;
    match model {
        DeepSeekModel::Pro => 40_000,
        DeepSeekModel::Flash => 18_000,
        DeepSeekModel::LegacyChat | DeepSeekModel::LegacyReasoner => 16_000,
    }
}

fn estimate_message_tokens(message: &ProtocolMessage) -> usize {
    let content = message.content.to_string_lossy().len();
    let tool_results = message
        .tool_results
        .iter()
        .map(|tr| tr.result.len())
        .sum::<usize>();
    (content + tool_results) / 4
}

/// Compact a subagent's running message history when it exceeds `budget_tokens`.
///
/// The first message (the task) is always kept. Older tool iterations are
/// dropped in whole `assistant + its tool results` groups — never splitting a
/// `tool_call`/`tool_result` pair — and replaced by a short system summary,
/// while the most recent groups within budget are retained. Returns whether
/// any compaction happened.
fn compact_subagent_messages(messages: &mut Vec<ProtocolMessage>, budget_tokens: usize) -> bool {
    let total: usize = messages.iter().map(estimate_message_tokens).sum();
    if total <= budget_tokens || messages.len() < 3 {
        return false;
    }

    let task = messages[0].clone();
    let task_turn_id = task.turn_id;

    // Split everything after the task into groups starting at each assistant.
    let mut groups: Vec<Vec<ProtocolMessage>> = Vec::new();
    for msg in &messages[1..] {
        if msg.role == Role::Assistant || groups.is_empty() {
            groups.push(vec![msg.clone()]);
        } else {
            groups.last_mut().expect("group present").push(msg.clone());
        }
    }

    // Keep the most recent groups that fit, always keeping at least one.
    let task_tokens: usize = estimate_message_tokens(&task);
    let mut used = task_tokens;
    let mut keep_from = groups.len();
    for (idx, group) in groups.iter().enumerate().rev() {
        let group_tokens: usize = group.iter().map(estimate_message_tokens).sum();
        if used + group_tokens > budget_tokens && keep_from < groups.len() {
            break;
        }
        used += group_tokens;
        keep_from = idx;
    }

    if keep_from == 0 {
        return false;
    }

    let dropped = &groups[..keep_from];
    let mut tool_names: Vec<String> = Vec::new();
    for group in dropped {
        for msg in group {
            for tc in &msg.tool_calls {
                tool_names.push(tc.function.name.clone());
            }
        }
    }
    let tool_summary = if tool_names.is_empty() {
        String::new()
    } else {
        format!(
            " Tools used: {}.",
            dedupe_preserving_order(tool_names).join(", ")
        )
    };
    let summary = ProtocolMessage {
        id: MessageId::new_v4(),
        role: Role::System,
        content: MessageContent::from(format!(
            "[Compacted {} earlier tool iteration(s) to stay within context.{}]",
            dropped.len(),
            tool_summary
        )),
        reasoning_content: None,
        tool_calls: Vec::new(),
        tool_results: Vec::new(),
        turn_id: task_turn_id,
        sub_turn_id: None,
        visibility: MessageVisibility::UserVisible,
    };

    let mut new_messages = vec![task, summary];
    for group in &groups[keep_from..] {
        new_messages.extend(group.iter().cloned());
    }
    *messages = new_messages;
    true
}

// helpers moved to crate::agent::utils

#[cfg(test)]
mod tests {
    use super::{
        collect_successful_file_access, dedupe_preserving_order, emit_subagent_chunk_delta,
        read_only_allows_tool, run_nested_subagent, structured_subagent_error_message,
        structured_subagent_limit_message, subagent_allows_tool_call, subagent_allows_tool_name,
        SubagentExecutor,
    };
    use super::{compact_subagent_messages, trim_tool_result, MAX_SUBAGENT_TOOL_RESULT_CHARS};
    use crate::agent::orchestrator::AgentEvent;
    use crate::agent::subagent::types::{SubagentConfig, SubagentTask};
    use crate::deepseek::client::DeepSeekClient;
    use crate::deepseek::models::{DeepSeekModel, StreamChunk};
    use crate::deepseek::{
        MessageContent, MessageId, MessageVisibility, ProtocolMessage, Role, TurnId,
    };
    use std::sync::Arc;

    fn proto(
        role: Role,
        content: &str,
        tool_calls: Vec<crate::deepseek::ToolCall>,
    ) -> ProtocolMessage {
        ProtocolMessage {
            id: MessageId::new_v4(),
            role,
            content: MessageContent::from(content.to_string()),
            reasoning_content: None,
            tool_calls,
            tool_results: Vec::new(),
            turn_id: TurnId::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        }
    }

    #[test]
    fn trim_tool_result_clips_oversized_output() {
        let big = "x".repeat(MAX_SUBAGENT_TOOL_RESULT_CHARS + 500);
        let trimmed = trim_tool_result(&big);
        assert!(trimmed.chars().count() < big.chars().count());
        assert!(trimmed.contains("more chars trimmed"));

        let small = "ok";
        assert_eq!(trim_tool_result(small), "ok");
    }

    #[test]
    fn compact_keeps_task_and_drops_oldest_groups() {
        let mut messages = vec![proto(Role::User, "TASK: do the thing", Vec::new())];
        // 5 tool iterations, each an assistant + a fat tool result.
        for i in 0..5 {
            messages.push(proto(
                Role::Assistant,
                &format!("calling tool {i}"),
                vec![tool_call("read_file", serde_json::json!({"path": "a"}))],
            ));
            messages.push(proto(Role::Tool, &"r".repeat(400), Vec::new()));
        }
        let before = messages.len();

        let compacted = compact_subagent_messages(&mut messages, 200);

        assert!(compacted);
        assert!(messages.len() < before);
        // Task is preserved verbatim as the first message.
        assert_eq!(messages[0].role, Role::User);
        assert!(messages[0]
            .content
            .to_string_lossy()
            .contains("TASK: do the thing"));
        // A summary marker sits right after the task.
        assert_eq!(messages[1].role, Role::System);
        assert!(messages[1].content.to_string_lossy().contains("Compacted"));
        // Groups stay intact: each kept assistant(tool_calls) is followed by its tool result.
        for (idx, msg) in messages.iter().enumerate() {
            if msg.role == Role::Assistant && !msg.tool_calls.is_empty() {
                assert_eq!(
                    messages[idx + 1].role,
                    Role::Tool,
                    "tool result must follow its call"
                );
            }
        }
    }

    #[test]
    fn compact_is_noop_under_budget() {
        let mut messages = vec![
            proto(Role::User, "small task", Vec::new()),
            proto(
                Role::Assistant,
                "hi",
                vec![tool_call("read_file", serde_json::json!({}))],
            ),
            proto(Role::Tool, "tiny", Vec::new()),
        ];
        let before = messages.clone();
        let compacted = compact_subagent_messages(&mut messages, 100_000);
        assert!(!compacted);
        assert_eq!(messages.len(), before.len());
    }

    fn tool_call(name: &str, arguments: serde_json::Value) -> crate::deepseek::ToolCall {
        crate::deepseek::ToolCall {
            id: "call-1".to_string(),
            call_type: "function".to_string(),
            function: crate::deepseek::ToolCallFunction {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
        }
    }

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
            r#"{"choices":[{"index":0,"delta":{"content":"working","reasoning_content":"hidden","tool_calls":[{"index":0,"function":{"name":"read_file","arguments":"{\"path\":\"src/lib.rs\"}"}}]},"finish_reason":null}],"usage":null}"#,
        )
        .expect("valid stream chunk");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_subagent_chunk_delta(&tx, "agent-1", &chunk);

        assert!(matches!(
            rx.try_recv().expect("subagent delta"),
            AgentEvent::SubagentDelta { agent_id, content }
                if agent_id == "agent-1" && content == "working"
        ));
        assert!(matches!(
            rx.try_recv().expect("subagent hidden token delta"),
            AgentEvent::TokenDelta { input_tokens: 0, output_tokens } if output_tokens > 0
        ));
    }

    #[test]
    fn subagent_prompt_includes_detected_project_stack() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("Cargo.toml"), "[package]\nname=\"x\"\n")
            .expect("write Cargo.toml");
        let executor = SubagentExecutor::new(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            root.path().to_path_buf(),
            SubagentConfig::default(),
        );

        let rules = executor.effective_prompt_rules();
        assert!(
            rules.contains("Detected Project") && rules.contains("cargo test"),
            "subagent prompt should carry the detected stack, got: {rules}"
        );
    }

    #[test]
    fn subagent_task_expected_output_is_model_visible() {
        let root = tempfile::tempdir().expect("tempdir");
        let executor = SubagentExecutor::new(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            root.path().to_path_buf(),
            SubagentConfig::default(),
        );
        let task = SubagentTask {
            description: "Explore package".into(),
            prompt: "Read Cargo.toml".into(),
            context: None,
            focus_files: vec!["Cargo.toml".into()],
            expected_output: Some("Return the package name and evidence.".into()),
        };

        let session = executor.build_session(&task);
        let content = session.messages[0].content.to_string_lossy();

        assert!(content.contains("## Focus Files"));
        assert!(content.contains("Cargo.toml"));
        assert!(content.contains("## Expected Output"));
        assert!(content.contains("Return the package name and evidence."));
    }

    #[test]
    fn subagent_tool_loop_keeps_active_tool_result_for_followup() {
        let root = tempfile::tempdir().expect("tempdir");
        let executor = SubagentExecutor::new(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            root.path().to_path_buf(),
            SubagentConfig::default(),
        );
        let mut session = executor.build_session(&SubagentTask {
            description: "Read package".into(),
            prompt: "Read Cargo.toml and report the package name.".into(),
            context: None,
            focus_files: Vec::new(),
            expected_output: None,
        });
        let turn_id = crate::deepseek::TurnId::new_v4();
        let sub_turn_id = crate::deepseek::SubTurnId::new_v4();
        let tool_call = crate::deepseek::ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: crate::deepseek::ToolCallFunction {
                name: "read_file".into(),
                arguments: r#"{"path":"Cargo.toml"}"#.into(),
            },
        };
        let assistant = crate::agent::reasoning::ReasoningManager::new_assistant_message(
            "",
            Some("need package name"),
            std::slice::from_ref(&tool_call),
            turn_id,
            Some(sub_turn_id),
            true,
        );
        session.messages.push(assistant);
        let assistant = session.messages.last().expect("assistant tool call");
        crate::agent::reasoning::ReasoningManager::begin_tool_turn(
            &mut session.reasoning_state,
            assistant,
        );
        let tool_result = "File: Cargo.toml\n\n     1 | [package]\n     2 | name = \"octocode\"";
        session.messages.push(
            crate::agent::reasoning::ReasoningManager::new_tool_result_message(
                "call-1",
                "read_file",
                tool_result,
                false,
                turn_id,
                sub_turn_id,
            ),
        );

        let (_, messages) = crate::agent::prompt_builder::PromptBuilder::new(
            DeepSeekModel::Flash,
            crate::deepseek::ExecutionLane::ToolLoopThinking,
            true,
        )
        .build(&session, None, None, &[]);
        let joined = messages
            .iter()
            .filter_map(|message| match message.content.as_ref()? {
                crate::deepseek::ChatMessageContent::Text(text) => Some(text.as_str()),
                crate::deepseek::ChatMessageContent::Parts(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("name = \"octocode\""));
    }

    #[test]
    fn subagent_limit_message_is_structured_and_localized() {
        let task = SubagentTask {
            description: "审查 swarm 失败摘要".into(),
            prompt: "请审查，不要写文件".into(),
            context: None,
            focus_files: Vec::new(),
            expected_output: None,
        };

        let message = structured_subagent_limit_message(&task, 12);

        assert!(message.contains("已用完轮次预算"));
        assert!(message.contains("--max-turns"));
        assert!(!message.contains("max_turns"));
        assert!(!message.contains("Subagent reached max turns limit"));
    }

    #[test]
    fn subagent_parse_error_message_hides_raw_error() {
        let task = SubagentTask {
            description: "审查工具参数".into(),
            prompt: "请审查".into(),
            context: None,
            focus_files: Vec::new(),
            expected_output: None,
        };

        let message =
            structured_subagent_error_message(&task, "parse error: EOF while parsing a string");

        assert!(message.contains("工具调用参数解析失败"));
        assert!(!message.contains("EOF while parsing"));
    }

    #[test]
    fn file_access_stats_are_collected_from_successful_tool_records_only() {
        let call = crate::deepseek::ToolCall {
            id: "call-1".to_string(),
            call_type: "function".to_string(),
            function: crate::deepseek::ToolCallFunction {
                name: "write_file".to_string(),
                arguments: serde_json::json!({ "path": "src/lib.rs" }).to_string(),
            },
        };
        let mut files_read = Vec::new();
        let mut files_written = Vec::new();

        collect_successful_file_access(&call, &mut files_read, &mut files_written);

        assert!(files_read.is_empty());
        assert_eq!(files_written, vec!["src/lib.rs"]);
    }

    #[test]
    fn read_only_only_allows_safe_read_tools() {
        for tool_name in [
            "read_file",
            "list_dir",
            "search_files",
            "search_code",
            "git_status",
            "git_diff",
            "semantic_search",
            "think",
        ] {
            assert!(
                read_only_allows_tool(tool_name),
                "{tool_name} should be allowed"
            );
        }

        for tool_name in [
            "write_file",
            "edit_file",
            "apply_patch",
            "run_command",
            "git_add",
            "git_commit",
            "run_subagent",
            "fetch_url",
            "web_search",
            "github_pr",
        ] {
            assert!(
                !read_only_allows_tool(tool_name),
                "{tool_name} should be blocked"
            );
        }
    }

    #[test]
    fn subagent_allowed_tools_patterns_expose_matching_tool_schema() {
        let allowed = vec!["Bash(git:*)".to_string()];

        assert!(subagent_allows_tool_name(&allowed, "run_command"));
        assert!(!subagent_allows_tool_name(&allowed, "read_file"));
    }

    #[test]
    fn subagent_allowed_tools_patterns_filter_by_tool_arguments() {
        let allowed = vec!["Bash(git:*)".to_string()];
        let git_call = tool_call(
            "run_command",
            serde_json::json!({ "command": "git status" }),
        );
        let cargo_call = tool_call(
            "run_command",
            serde_json::json!({ "command": "cargo test" }),
        );

        assert!(subagent_allows_tool_call(&allowed, &git_call));
        assert!(!subagent_allows_tool_call(&allowed, &cargo_call));
    }

    #[tokio::test]
    async fn nested_subagent_parse_failure_is_error() {
        let root = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let (text, is_error) = run_nested_subagent(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            root.path().to_path_buf(),
            1,
            "{".to_string(),
            tx,
        )
        .await;

        assert!(is_error);
        assert!(text.contains("Failed to parse run_subagent arguments"));
    }

    // A scripted streaming client: returns one canned StreamResult per turn so
    // the subagent loop runs end-to-end without a network round-trip.
    struct MockStreamClient {
        turns: std::sync::Mutex<std::collections::VecDeque<crate::deepseek::models::StreamResult>>,
    }

    impl MockStreamClient {
        fn new(turns: Vec<crate::deepseek::models::StreamResult>) -> Self {
            Self {
                turns: std::sync::Mutex::new(turns.into()),
            }
        }
    }

    impl crate::deepseek::client::ChatStreamClient for MockStreamClient {
        fn stream_chat<'a>(
            &'a self,
            _req: &'a crate::deepseek::ChatRequest,
            _on_chunk: &'a mut (dyn FnMut(&StreamChunk) + Send + 'a),
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::deepseek::models::StreamResult,
                            crate::deepseek::errors::DeepSeekError,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            let next = self.turns.lock().expect("mock lock").pop_front();
            Box::pin(async move {
                next.ok_or_else(|| {
                    crate::deepseek::errors::DeepSeekError::Other("mock stream exhausted".into())
                })
            })
        }
    }

    fn final_answer(content: &str) -> crate::deepseek::models::StreamResult {
        crate::deepseek::models::StreamResult {
            content: content.to_string(),
            reasoning_content: String::new(),
            tool_calls: Vec::new(),
            finish_reason: Some("stop".to_string()),
            usage: None,
        }
    }

    fn tool_call_turn(name: &str) -> crate::deepseek::models::StreamResult {
        crate::deepseek::models::StreamResult {
            content: String::new(),
            reasoning_content: String::new(),
            tool_calls: vec![tool_call(name, serde_json::json!({}))],
            finish_reason: Some("tool_calls".to_string()),
            usage: None,
        }
    }

    fn big_tool_call_turn() -> crate::deepseek::models::StreamResult {
        crate::deepseek::models::StreamResult {
            // Large assistant content so the running session crosses the
            // subagent context budget and forces in-loop compaction.
            content: "x".repeat(40_000),
            reasoning_content: String::new(),
            tool_calls: vec![tool_call("definitely_unknown_tool", serde_json::json!({}))],
            finish_reason: Some("tool_calls".to_string()),
            usage: None,
        }
    }

    fn loop_task() -> SubagentTask {
        SubagentTask {
            description: "task".into(),
            prompt: "do the task".into(),
            context: None,
            focus_files: Vec::new(),
            expected_output: None,
        }
    }

    fn loop_executor() -> (SubagentExecutor, tempfile::TempDir) {
        let root = tempfile::tempdir().expect("tempdir");
        let executor = SubagentExecutor::new(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            root.path().to_path_buf(),
            SubagentConfig::default(),
        );
        (executor, root)
    }

    #[tokio::test]
    async fn run_loop_returns_final_answer_with_no_network() {
        let (executor, _root) = loop_executor();
        let mock = MockStreamClient::new(vec![final_answer("All done — refactor complete.")]);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let result = executor.run_with_client(&mock, &loop_task(), &tx).await;

        assert!(
            result.success,
            "a no-tool-call turn ends the loop successfully"
        );
        assert!(result.output.contains("All done"));
    }

    #[tokio::test]
    async fn subagent_run_records_usage_telemetry() {
        let (executor, root) = loop_executor();
        // The model response carries usage; the executor must persist it to the
        // shared usage store so swarm/subagent spend counts toward quota
        // tracking instead of vanishing.
        let mut answer = final_answer("done");
        answer.usage = Some(crate::deepseek::models::Usage {
            prompt_tokens: 120,
            completion_tokens: 30,
            total_tokens: 150,
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            prompt_tokens_details: None,
        });
        let mock = MockStreamClient::new(vec![answer]);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let result = executor.run_with_client(&mock, &loop_task(), &tx).await;
        assert_eq!(result.token_usage, 150);

        let events = crate::storage::UsageStore::new(root.path())
            .load_events()
            .expect("load usage events");
        assert_eq!(
            events.len(),
            1,
            "the subagent model call must record exactly one usage event"
        );
        assert_eq!(events[0].total_tokens, 150);
    }

    #[tokio::test]
    async fn run_loop_fails_when_max_turns_exhausted() {
        let root = tempfile::tempdir().expect("tempdir");
        let config = SubagentConfig {
            max_turns: 1,
            ..SubagentConfig::default()
        };
        let executor = SubagentExecutor::new(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            root.path().to_path_buf(),
            config,
        );
        // Every turn asks for another tool call, so the loop never reaches a
        // final answer and must terminate at the max-turn limit.
        let mock = MockStreamClient::new(vec![tool_call_turn("definitely_unknown_tool")]);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let result = executor.run_with_client(&mock, &loop_task(), &tx).await;

        assert!(!result.success, "exhausting max_turns must report failure");
    }

    #[tokio::test]
    async fn run_loop_continues_after_tool_turn_then_finalizes() {
        let (executor, _root) = loop_executor();
        // Turn 1 asks for a tool (which errors), turn 2 produces the answer.
        let mock = MockStreamClient::new(vec![
            tool_call_turn("definitely_unknown_tool"),
            final_answer("Investigated and fixed."),
        ]);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let result = executor.run_with_client(&mock, &loop_task(), &tx).await;

        assert!(
            result.success,
            "the loop should follow up after a tool turn and reach the answer"
        );
        assert!(result.output.contains("Investigated and fixed"));
        assert!(result
            .tool_calls_used
            .contains(&"definitely_unknown_tool".to_string()));
        // The failed tool was recovered (not fatal) and surfaced as a note.
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("Recovered tool errors"));
    }

    #[tokio::test]
    async fn run_loop_survives_context_pressure_with_compaction() {
        let (executor, _root) = loop_executor();
        // Many large turns push the session past the context budget, exercising
        // the in-loop compaction path; the loop must still reach the answer.
        let mut turns: Vec<crate::deepseek::models::StreamResult> =
            (0..6).map(|_| big_tool_call_turn()).collect();
        turns.push(final_answer("Synthesized after many large turns."));
        let mock = MockStreamClient::new(turns);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let result = executor.run_with_client(&mock, &loop_task(), &tx).await;

        assert!(
            result.success,
            "compaction should keep a long loop runnable"
        );
        assert!(result.output.contains("Synthesized after many large turns"));
    }
}

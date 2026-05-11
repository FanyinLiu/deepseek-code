use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc;

use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::tools as ds_tools;
use crate::deepseek::{
    thinking_config_for_lane, CacheUsage, ChatRequest, DeepSeekModel, ExecutionLane, FinishReason,
    MessageContent, MessageId, MessageVisibility, ModelCapability, ProtocolMessage,
    ReasoningEffort, ReasoningState, Role, Session, SessionId, StreamResult, SubTurnId,
    ThinkingConfig, ToolCall, ToolDefinition, ToolResultRecord, TurnId, Usage,
};
use crate::plan;
use crate::plan::schema::{Plan, RiskLevel};
use crate::policy;
use crate::search::{self, SearchMatch};

use super::background::{BackgroundQueue, BackgroundTaskSnapshot};
use super::lanes::{classify_task, TaskClass};
use super::prompt_builder::{load_project_rules, PromptBuilder};
use super::reasoning::ReasoningManager;
use super::router::{ComplexityAssessment, ComplexityRouter, ReasonCode, Route};
use super::subagent::SubagentToolArgs;
use super::task_tool::TaskToolHandler;
use super::tool_loop::ToolLoop;

/// Events emitted by the orchestrator during execution.
#[derive(Debug)]
pub enum AgentEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ToolApprovalNeeded {
        tool_name: String,
        display: policy::ApprovalDisplay,
        respond: tokio::sync::oneshot::Sender<bool>,
    },
    ToolExecuted {
        tool_name: String,
        success: bool,
        summary: String,
    },
    StreamDone {
        finish_reason: Option<FinishReason>,
        usage: Option<Usage>,
        cache: Option<CacheUsage>,
    },
    Error(String),
    TurnComplete {
        session_id: SessionId,
        total_tokens: u64,
    },
    /// Complexity assessment completed — UI can show route chips.
    ComplexityAssessed {
        assessment: ComplexityAssessment,
    },
    /// User needs to clarify the task before proceeding.
    ClarificationNeeded {
        questions: Vec<String>,
    },
    /// A subagent has started running.
    SubagentStarted {
        agent_id: String,
        agent_type: String,
        description: String,
        is_background: bool,
    },
    /// A subagent streamed a visible update while running.
    SubagentDelta {
        agent_id: String,
        content: String,
    },
    /// A subagent has completed.
    SubagentCompleted {
        agent_id: String,
        result: crate::agent::subagent::SubagentResult,
    },
    /// A subagent tool needs approval.
    SubagentToolApprovalNeeded {
        agent_id: String,
        tool_name: String,
        arguments: String,
        respond: tokio::sync::oneshot::Sender<bool>,
    },
    /// Plan step status update for visual progress tracking.
    PlanStepUpdate {
        index: usize,
        total: usize,
        description: String,
        status: PlanStepStatus,
    },
    /// A plan has been generated and should seed the visual tracker title.
    PlanStarted {
        summary: String,
        total: usize,
    },
    /// Clear the plan tracker (plan execution finished or cancelled).
    PlanCleared,
    /// Review warnings produced after plan generation.
    PlanReviewWarnings {
        warnings: Vec<String>,
    },
    /// A file was edited or written — UI can show a diff preview.
    FileDiff {
        path: String,
        diff: String,
        stats: String,
    },
    /// Present multiple options to the user after thinking / planning.
    OptionsNeeded {
        title: String,
        options: Vec<String>,
        respond: tokio::sync::oneshot::Sender<usize>,
    },
}

/// Execution mode chosen by the user for a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanExecutionMode {
    Auto,
    Confirm,
    Preview,
    Cancel,
}

/// A plan option presented to the user with its associated execution mode.
#[derive(Debug, Clone)]
struct PlanOption {
    label: String,
    mode: PlanExecutionMode,
}

/// Generate plan execution options based on the plan's content and risk profile.
fn generate_plan_options(plan: &Plan, use_chinese: bool) -> Vec<PlanOption> {
    let has_critical = plan
        .risks
        .iter()
        .any(|r| matches!(r.level, RiskLevel::Critical));

    let mut options = Vec::new();

    // Critical-risk plans must not offer "Execute automatically" — force manual review.
    if !has_critical {
        if plan.requires_command && plan.requires_write {
            options.push(PlanOption {
                label: if use_chinese {
                    "自动执行（包含写入和命令）".into()
                } else {
                    "Execute automatically (writes + commands)".into()
                },
                mode: PlanExecutionMode::Auto,
            });
        } else if plan.requires_command {
            options.push(PlanOption {
                label: if use_chinese {
                    "自动执行（包含命令）".into()
                } else {
                    "Execute automatically (includes commands)".into()
                },
                mode: PlanExecutionMode::Auto,
            });
        } else if plan.requires_write {
            options.push(PlanOption {
                label: if use_chinese {
                    "自动执行".into()
                } else {
                    "Execute automatically".into()
                },
                mode: PlanExecutionMode::Auto,
            });
        } else {
            options.push(PlanOption {
                label: if use_chinese {
                    "执行计划".into()
                } else {
                    "Execute plan".into()
                },
                mode: PlanExecutionMode::Auto,
            });
        }
    }

    options.push(PlanOption {
        label: if use_chinese && has_critical {
            "需要确认后执行（存在高风险）".into()
        } else if use_chinese {
            "需要确认后执行".into()
        } else if has_critical {
            "Execute with confirmations (required — critical risks present)".into()
        } else {
            "Execute with confirmations".into()
        },
        mode: PlanExecutionMode::Confirm,
    });

    if plan.requires_command {
        options.push(PlanOption {
            label: if use_chinese && plan.requires_write {
                "仅预览 - 显示计划变更但不执行".into()
            } else if use_chinese {
                "仅预览命令 - 不执行".into()
            } else if plan.requires_write {
                "Preview only — show planned changes without executing".into()
            } else {
                "Preview commands — don't run them".into()
            },
            mode: PlanExecutionMode::Preview,
        });
    } else if plan.requires_write {
        options.push(PlanOption {
            label: if use_chinese {
                "预览变更 - 执行前显示 diff".into()
            } else {
                "Preview changes — show diffs before applying".into()
            },
            mode: PlanExecutionMode::Preview,
        });
    }

    // If high risk, offer breaking into sub-tasks
    let has_high_risk = plan
        .risks
        .iter()
        .any(|r| matches!(r.level, RiskLevel::High | RiskLevel::Critical));
    if has_high_risk && options.len() < 5 {
        options.push(PlanOption {
            label: if use_chinese {
                "拆成更小的子任务".into()
            } else {
                "Break into smaller sub-tasks".into()
            },
            mode: PlanExecutionMode::Cancel, // For now, cancel and let user retry
        });
    }

    options.push(PlanOption {
        label: if use_chinese {
            "取消".into()
        } else {
            "Cancel".into()
        },
        mode: PlanExecutionMode::Cancel,
    });

    options
}

fn plan_uses_chinese(plan: &Plan) -> bool {
    contains_cjk(&plan.summary)
        || plan.steps.iter().any(|step| contains_cjk(step))
        || plan
            .target_files
            .iter()
            .any(|file| contains_cjk(&file.reason) || contains_cjk(&file.path))
        || plan
            .verification
            .iter()
            .any(|verification| contains_cjk(verification))
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch as u32,
            0x4E00..=0x9FFF
                | 0x3400..=0x4DBF
                | 0x20000..=0x2A6DF
                | 0x2A700..=0x2B73F
                | 0x2B740..=0x2B81F
                | 0x2B820..=0x2CEAF
        )
    })
}

/// Visual status of a plan step in the TUI tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStepStatus {
    Pending,
    Running,
    Done,
    Failed,
}

/// Tracks which plan step is currently being executed.
#[derive(Debug, Clone)]
struct PlanExecutionState {
    steps: Vec<crate::plan::executor::PlanStep>,
    current_index: usize,
    total: usize,
    had_failure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanProgressUpdate {
    index: usize,
    total: usize,
    description: String,
    status: PlanStepStatus,
}

impl PlanExecutionState {
    fn new(steps: Vec<crate::plan::executor::PlanStep>) -> Self {
        let total = steps.len();
        Self {
            steps,
            current_index: 0,
            total,
            had_failure: false,
        }
    }

    fn current_update(&self, status: PlanStepStatus) -> Option<PlanProgressUpdate> {
        (self.current_index < self.total).then(|| self.update_for(self.current_index, status))
    }

    fn updates_after_tool_batch(&mut self, had_error: bool) -> Vec<PlanProgressUpdate> {
        let Some(current) = self.current_update(if had_error {
            PlanStepStatus::Failed
        } else {
            PlanStepStatus::Done
        }) else {
            return Vec::new();
        };

        self.current_index += 1;
        if had_error {
            self.had_failure = true;
            return vec![current];
        }

        let mut updates = vec![current];
        if let Some(next) = self.current_update(PlanStepStatus::Running) {
            updates.push(next);
        }
        updates
    }

    fn finish_remaining(&mut self, success: bool) -> Vec<PlanProgressUpdate> {
        let status = if success && !self.had_failure {
            PlanStepStatus::Done
        } else {
            PlanStepStatus::Failed
        };
        let mut updates = Vec::new();
        while self.current_index < self.total {
            updates.push(self.update_for(self.current_index, status));
            self.current_index += 1;
        }
        updates
    }

    fn update_for(&self, index: usize, status: PlanStepStatus) -> PlanProgressUpdate {
        PlanProgressUpdate {
            index,
            total: self.total,
            description: self.steps[index].display(),
            status,
        }
    }
}

/// The main agent orchestrator.
pub struct Orchestrator {
    pub client: DeepSeekClient,
    pub project_root: std::path::PathBuf,
    pub session: Session,
    pub background_queue: BackgroundQueue,
    pub yolo_mode: bool,
    plan_execution: Option<PlanExecutionState>,
    mcp_registry: Option<crate::mcp::McpRegistry>,
    mcp_initialized: bool,
}

impl Orchestrator {
    #[must_use]
    pub fn new(client: DeepSeekClient, project_root: std::path::PathBuf, session: Session) -> Self {
        Self {
            client,
            project_root,
            session,
            background_queue: BackgroundQueue::new(),
            yolo_mode: false,
            plan_execution: None,
            mcp_registry: None,
            mcp_initialized: false,
        }
    }

    /// Initialize MCP registry from config and connect to all configured servers.
    pub async fn init_mcp(&mut self, config: &crate::storage::config::McpConfig) {
        self.mcp_initialized = true;
        if !config.enabled {
            return;
        }
        let mut registry = crate::mcp::McpRegistry::new();
        for (name, server_config) in &config.servers {
            registry.register_config(name.clone(), server_config);
        }
        registry.connect_all().await;
        self.mcp_registry = Some(registry);
    }

    /// Get MCP registry status summary.
    pub fn mcp_status(&self) -> String {
        if !self.mcp_initialized {
            return "MCP: deferred until first turn".to_string();
        }
        match self.mcp_registry {
            Some(ref registry) => registry.status(),
            None => "MCP: not initialized".to_string(),
        }
    }

    #[must_use]
    pub fn mcp_initialized(&self) -> bool {
        self.mcp_initialized
    }

    /// Get all tool definitions, including standard tools and MCP tools.
    fn get_all_tools(&self) -> Vec<ToolDefinition> {
        let mut tools = ds_tools::standard_tool_definitions();
        if let Some(ref registry) = self.mcp_registry {
            if registry.has_connected() {
                for (server_name, mcp_tool) in registry.all_tools() {
                    tools.push(ToolDefinition {
                        tool_type: "function".to_string(),
                        function: crate::deepseek::models::FunctionDef {
                            name: format!("{}.{}", server_name, mcp_tool.name),
                            description: mcp_tool.description.clone().unwrap_or_default(),
                            parameters: mcp_tool.input_schema.clone(),
                        },
                    });
                }
            }
        }
        tools
    }

    /// List all background tasks.
    #[must_use]
    pub fn background_tasks(&self) -> Vec<BackgroundTaskSnapshot> {
        self.background_queue.list_tasks()
    }

    /// Generate a session name from the user's first message.
    fn generate_session_name(input: &str) -> String {
        let trimmed = input.trim();
        let truncated = if trimmed.chars().count() > 30 {
            trimmed.chars().take(30).collect::<String>()
        } else {
            trimmed.to_string()
        };
        truncated
            .replace(
                |c: char| !c.is_alphanumeric() && !c.is_whitespace() && c != '-' && c != '_',
                "",
            )
            .trim()
            .replace(' ', "-")
    }

    /// Run a turn with default lane classification.
    pub async fn run_turn(
        &mut self,
        user_input: &str,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<(), anyhow::Error> {
        self.run_turn_with_images(user_input, &[], event_tx).await
    }

    /// Run a turn with attached images.
    pub async fn run_turn_with_images(
        &mut self,
        user_input: &str,
        images: &[String],
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<(), anyhow::Error> {
        // Auto-name session on first turn if not already named
        if self.session.name.is_none() {
            self.session.name = Some(Self::generate_session_name(user_input));
        }

        let timeout = std::time::Duration::from_mins(10);
        if let Ok(result) = tokio::time::timeout(
            timeout,
            self.run_turn_inner(user_input, images, &event_tx, None),
        )
        .await
        {
            result
        } else {
            send_event(
                &event_tx,
                AgentEvent::Error("Turn timed out after 10 minutes".into()),
            );
            Err(anyhow::anyhow!("Turn timed out after 10 minutes"))
        }
    }

    /// Run a turn forcing a specific lane (for `run` command).
    pub async fn run_turn_forced(
        &mut self,
        user_input: &str,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        lane: ExecutionLane,
    ) -> Result<(), anyhow::Error> {
        let timeout = std::time::Duration::from_mins(10);
        if let Ok(result) = tokio::time::timeout(
            timeout,
            self.run_turn_inner(user_input, &[], &event_tx, Some(lane)),
        )
        .await
        {
            result
        } else {
            send_event(
                &event_tx,
                AgentEvent::Error("Turn timed out after 10 minutes".into()),
            );
            Err(anyhow::anyhow!("Turn timed out after 10 minutes"))
        }
    }

    async fn run_turn_inner(
        &mut self,
        user_input: &str,
        images: &[String],
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        force_lane: Option<ExecutionLane>,
    ) -> Result<(), anyhow::Error> {
        // Load project rules once and reuse throughout the turn.
        let project_rules = load_project_rules(&self.project_root);

        // 1. Legacy classify (for search / explicit triggers)
        let task = classify_task(user_input);
        let has_forced_lane = force_lane.is_some();

        // 2. Complexity router assessment (for all non-forced plan modes)
        let (assessment, shadow_mode) = if has_forced_lane || task == TaskClass::Plan {
            // Respect explicit user intent
            (None, false)
        } else {
            let router_config = crate::storage::Config::load(Some(&self.project_root))
                .map(|c| c.router)
                .unwrap_or_default();
            let shadow_mode = router_config.shadow_mode;
            let router = if router_config.enabled {
                ComplexityRouter::from(&router_config)
            } else {
                ComplexityRouter::new().rules_only()
            };
            let assessment = router
                .assess(user_input, project_rules.as_deref(), Some(&self.client))
                .await;
            send_event(
                event_tx,
                AgentEvent::ComplexityAssessed {
                    assessment: assessment.clone(),
                },
            );
            // Telemetry: log routing decision if enabled
            crate::telemetry::events::log_complexity_assessed(&assessment, &self.session.id);
            (Some(assessment), shadow_mode)
        };

        // 3. Route decision
        if let Some(ref a) = assessment {
            let route = if shadow_mode {
                // In shadow mode the router still assesses and emits telemetry,
                // but never overrides the actual execution path.
                Route::DirectExecute
            } else {
                a.route.clone()
            };
            match route {
                Route::Clarify => {
                    let questions = generate_clarification_questions(user_input, a);
                    send_event(event_tx, AgentEvent::ClarificationNeeded { questions });
                    return Ok(());
                }
                Route::PlanReview => {
                    // Override into plan mode even if not explicitly requested
                    let search_ctx = self.run_search_phase(user_input, event_tx).await;
                    return self.run_plan_mode(user_input, &search_ctx, event_tx).await;
                }
                Route::DirectExecute => {
                    // Continue below; lane will be chosen conservatively
                }
            }
        }

        // 4. Begin user turn
        let turn_id = TurnId::new_v4();
        crate::workspace::apply::set_current_turn_id(&turn_id.to_string());
        ReasoningManager::begin_user_turn(
            &mut self.session.reasoning_state,
            &mut self.session.messages,
            turn_id,
        );

        // 5. Resolve @-mentions and add user message
        let mention_context =
            crate::tools::mentions::resolve_mentions(&self.project_root, user_input);
        let enriched_input = if mention_context.is_empty() {
            user_input.to_string()
        } else {
            format!("{mention_context}{user_input}")
        };
        let content = if images.is_empty() {
            MessageContent::from(enriched_input.as_str())
        } else {
            let mut parts = vec![crate::deepseek::models::ContentPart {
                part_type: "text".into(),
                text: Some(enriched_input),
                image_url: None,
            }];
            for url in images {
                parts.push(crate::deepseek::models::ContentPart {
                    part_type: "image_url".into(),
                    text: None,
                    image_url: Some(url.clone()),
                });
            }
            MessageContent::MultiPart(parts)
        };
        self.session.messages.push(ProtocolMessage {
            id: MessageId::new_v4(),
            role: Role::User,
            content,
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id,
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        });

        // 6. Search if needed
        let search_ctx = if matches!(task, TaskClass::Search | TaskClass::Plan) {
            self.run_search_phase(user_input, event_tx).await
        } else {
            None
        };

        // 7. Plan mode (explicit / router-overridden already handled above)
        if task == TaskClass::Plan {
            return self.run_plan_mode(user_input, &search_ctx, event_tx).await;
        }

        // Determine execution lane
        let lane = force_lane.unwrap_or_else(|| task.default_lane());

        // 8. Build prompt (project_rules loaded once at top of run_turn_inner)
        let cap = ModelCapability::for_model(&self.session.reasoning_state.mode_to_model());
        let thinking_config = thinking_config_for_lane(
            &lane,
            &self.session.reasoning_state.mode,
            &self.session.reasoning_state.effort,
        );

        let tool_defs = self.get_all_tools();
        let send_tools = matches!(
            lane,
            ExecutionLane::ToolLoopThinking | ExecutionLane::PlanThinking
        ) || has_forced_lane;
        let effective_tools: Vec<ToolDefinition> = if send_tools { tool_defs } else { Vec::new() };

        let (_, messages) = PromptBuilder::new(cap.model.clone(), lane.clone(), true).build(
            &self.session,
            project_rules.as_deref(),
            search_ctx.as_deref(),
            &effective_tools,
        );

        let request = ChatRequest {
            model: cap.model.to_string(),
            messages,
            tools: if send_tools {
                Some(effective_tools)
            } else {
                None
            },
            thinking: thinking_config,
            response_format: None,
            stream: true,
            max_tokens: Some(8192),
        };

        // 9. Stream and process
        let mut emitted = EmittedStreamDeltas::default();
        let result = self
            .client
            .chat_stream_accumulated_with_deltas(&request, |chunk| {
                emitted.merge(emit_stream_chunk_deltas(event_tx, chunk));
            })
            .await;
        match result {
            Ok(stream_result) => {
                if stream_result.tool_calls.is_empty() {
                    let msg = ReasoningManager::new_assistant_message(
                        &stream_result.content,
                        (!stream_result.reasoning_content.is_empty())
                            .then_some(&stream_result.reasoning_content),
                        &[],
                        turn_id,
                        None,
                        false,
                    );
                    self.session.messages.push(msg);
                    if !emitted.reasoning {
                        send_reasoning_delta(event_tx, &stream_result.reasoning_content);
                    }
                    if !emitted.content {
                        send_event(
                            event_tx,
                            AgentEvent::ContentDelta(stream_result.content.clone()),
                        );
                    }
                    if let Some(ref usage) = stream_result.usage {
                        self.session.metadata.total_tokens += u64::from(usage.total_tokens);
                        self.session.metadata.total_cost_estimate +=
                            usage.estimate_cost_cny(&self.session.reasoning_state.mode_to_model());
                        send_event(
                            event_tx,
                            AgentEvent::StreamDone {
                                finish_reason: stream_result
                                    .finish_reason
                                    .clone()
                                    .map(FinishReason::from),
                                usage: Some(usage.clone()),
                                cache: Some(CacheUsage::from_usage(usage)),
                            },
                        );
                    }
                } else {
                    if !emitted.reasoning {
                        send_reasoning_delta(event_tx, &stream_result.reasoning_content);
                    }
                    if !emitted.content && !stream_result.content.is_empty() {
                        send_event(
                            event_tx,
                            AgentEvent::ContentDelta(format!("{}\n", stream_result.content)),
                        );
                    }
                    self.handle_tool_calls(&stream_result, turn_id, event_tx, 0)
                        .await?;
                }
                self.session.updated_at = Utc::now();
                send_event(
                    event_tx,
                    AgentEvent::TurnComplete {
                        session_id: self.session.id,
                        total_tokens: self.session.metadata.total_tokens,
                    },
                );
            }
            Err(e) => {
                send_event(event_tx, AgentEvent::Error(e.to_string()));
            }
        }
        Ok(())
    }

    async fn run_search_phase(
        &self,
        query: &str,
        _event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Option<String> {
        let files = search::search_files(&self.project_root, query, 20).unwrap_or_default();
        let code =
            search::search_code(&self.project_root, query, None, false, 30).unwrap_or_default();
        let all_results: Vec<SearchMatch> = files.into_iter().chain(code).collect();
        if all_results.is_empty() {
            return None;
        }
        Some(search::pack_search_results(&all_results, 8000).to_string())
    }

    async fn run_plan_mode(
        &mut self,
        user_input: &str,
        search_ctx: &Option<String>,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<(), anyhow::Error> {
        let plan = plan::generate_plan(
            &self.client,
            &DeepSeekModel::Pro,
            user_input,
            search_ctx.as_deref(),
            load_project_rules(&self.project_root).as_deref(),
        )
        .await;
        match plan {
            Ok(p) => {
                if let Err(errors) = p.validate() {
                    for e in &errors {
                        send_event(event_tx, AgentEvent::Error(e.clone()));
                    }
                    return Ok(());
                }
                let review = plan::review_plan(&p);
                if !review.warnings.is_empty() {
                    send_event(
                        event_tx,
                        AgentEvent::PlanReviewWarnings {
                            warnings: review.warnings.clone(),
                        },
                    );
                }

                // Convert plan to concrete steps and emit to TUI tracker
                let steps = plan::executor::plan_to_steps(&p);
                send_event(
                    event_tx,
                    AgentEvent::PlanStarted {
                        summary: p.summary.clone(),
                        total: steps.len(),
                    },
                );
                for (i, step) in steps.iter().enumerate() {
                    send_event(
                        event_tx,
                        AgentEvent::PlanStepUpdate {
                            index: i,
                            total: steps.len(),
                            description: step.display(),
                            status: PlanStepStatus::Pending,
                        },
                    );
                }

                // Present execution-mode options to the user (unless yolo mode)
                if !self.yolo_mode {
                    let use_chinese = plan_uses_chinese(&p);
                    let options = generate_plan_options(&p, use_chinese);
                    let (opts_tx, opts_rx) = tokio::sync::oneshot::channel();
                    send_event(
                        event_tx,
                        AgentEvent::OptionsNeeded {
                            title: if use_chinese {
                                format!("计划执行：{}", p.summary)
                            } else {
                                format!("Plan execution: {}", p.summary)
                            },
                            options: options.iter().map(|o| o.label.clone()).collect(),
                            respond: opts_tx,
                        },
                    );
                    let choice = match opts_rx.await {
                        Ok(v) => v,
                        Err(_) => {
                            send_event(
                                event_tx,
                                AgentEvent::Error("Options channel closed".into()),
                            );
                            send_event(event_tx, AgentEvent::PlanCleared);
                            self.session.updated_at = Utc::now();
                            send_event(
                                event_tx,
                                AgentEvent::TurnComplete {
                                    session_id: self.session.id,
                                    total_tokens: 0,
                                },
                            );
                            return Ok(());
                        }
                    };
                    if choice >= options.len() {
                        // Cancel (or invalid choice)
                        send_event(
                            event_tx,
                            AgentEvent::ContentDelta("Plan cancelled by user.\n".into()),
                        );
                        send_event(event_tx, AgentEvent::PlanCleared);
                        self.session.updated_at = Utc::now();
                        send_event(
                            event_tx,
                            AgentEvent::TurnComplete {
                                session_id: self.session.id,
                                total_tokens: 0,
                            },
                        );
                        return Ok(());
                    }
                    match options[choice].mode {
                        PlanExecutionMode::Auto => {
                            self.yolo_mode = true;
                        }
                        PlanExecutionMode::Confirm => { /* normal mode */ }
                        PlanExecutionMode::Preview => {
                            send_event(
                                event_tx,
                                AgentEvent::ContentDelta(
                                    "Preview mode selected — plan shown but not executed.\n".into(),
                                ),
                            );
                            send_event(event_tx, AgentEvent::PlanCleared);
                            self.session.updated_at = Utc::now();
                            send_event(
                                event_tx,
                                AgentEvent::TurnComplete {
                                    session_id: self.session.id,
                                    total_tokens: 0,
                                },
                            );
                            return Ok(());
                        }
                        PlanExecutionMode::Cancel => {
                            send_event(
                                event_tx,
                                AgentEvent::ContentDelta("Plan cancelled by user.\n".into()),
                            );
                            send_event(event_tx, AgentEvent::PlanCleared);
                            self.session.updated_at = Utc::now();
                            send_event(
                                event_tx,
                                AgentEvent::TurnComplete {
                                    session_id: self.session.id,
                                    total_tokens: 0,
                                },
                            );
                            return Ok(());
                        }
                    }
                }

                // Store plan execution state for step tracking during tool calls
                let plan_state = PlanExecutionState::new(steps);
                if let Some(update) = plan_state.current_update(PlanStepStatus::Running) {
                    send_plan_update(event_tx, update);
                }
                self.plan_execution = Some(plan_state);

                let plan_json = serde_json::to_string_pretty(&p).unwrap_or_default();
                let turn_id = TurnId::new_v4();
                crate::workspace::apply::set_current_turn_id(&turn_id.to_string());
                self.session.messages.push(ProtocolMessage {
                    id: MessageId::new_v4(),
                    role: Role::Assistant,
                    content: MessageContent::from(plan_json),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                    turn_id,
                    sub_turn_id: None,
                    visibility: MessageVisibility::AuditOnly,
                });

                // Add execution instruction as a synthetic user turn
                let execution_prompt = "Execute the above plan step by step. Use the available tools to complete each step.";
                self.session.messages.push(ProtocolMessage {
                    id: MessageId::new_v4(),
                    role: Role::User,
                    content: MessageContent::from(execution_prompt),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                    turn_id,
                    sub_turn_id: None,
                    visibility: MessageVisibility::AuditOnly,
                });

                // Stream with tools to execute the plan
                let tool_defs = self.get_all_tools();
                let cap = ModelCapability::for_model(&self.session.reasoning_state.mode_to_model());
                let thinking_config = thinking_config_for_lane(
                    &ExecutionLane::ToolLoopThinking,
                    &self.session.reasoning_state.mode,
                    &self.session.reasoning_state.effort,
                );

                let (_, messages) =
                    PromptBuilder::new(cap.model.clone(), ExecutionLane::ToolLoopThinking, true)
                        .build(
                            &self.session,
                            load_project_rules(&self.project_root).as_deref(),
                            search_ctx.as_deref(),
                            &tool_defs,
                        );

                let request = ChatRequest {
                    model: cap.model.to_string(),
                    messages,
                    tools: Some(tool_defs),
                    thinking: thinking_config,
                    response_format: None,
                    stream: true,
                    max_tokens: Some(8192),
                };

                let mut plan_execution_failed = false;
                let mut emitted = EmittedStreamDeltas::default();
                match self
                    .client
                    .chat_stream_accumulated_with_deltas(&request, |chunk| {
                        emitted.merge(emit_stream_chunk_deltas(event_tx, chunk));
                    })
                    .await
                {
                    Ok(stream_result) => {
                        if stream_result.tool_calls.is_empty() {
                            let msg = ReasoningManager::new_assistant_message(
                                &stream_result.content,
                                if stream_result.reasoning_content.is_empty() {
                                    None
                                } else {
                                    Some(&stream_result.reasoning_content)
                                },
                                &[],
                                turn_id,
                                None,
                                false,
                            );
                            self.session.messages.push(msg);
                            if !emitted.reasoning {
                                send_reasoning_delta(event_tx, &stream_result.reasoning_content);
                            }
                            if !emitted.content {
                                send_event(
                                    event_tx,
                                    AgentEvent::ContentDelta(stream_result.content),
                                );
                            }
                            if let Some(ref usage) = stream_result.usage {
                                self.session.metadata.total_tokens += u64::from(usage.total_tokens);
                                self.session.metadata.total_cost_estimate += usage
                                    .estimate_cost_cny(
                                        &self.session.reasoning_state.mode_to_model(),
                                    );
                                send_event(
                                    event_tx,
                                    AgentEvent::StreamDone {
                                        finish_reason: stream_result
                                            .finish_reason
                                            .clone()
                                            .map(FinishReason::from),
                                        usage: Some(usage.clone()),
                                        cache: Some(CacheUsage::from_usage(usage)),
                                    },
                                );
                            }
                        } else {
                            if !emitted.reasoning {
                                send_reasoning_delta(event_tx, &stream_result.reasoning_content);
                            }
                            if !emitted.content && !stream_result.content.is_empty() {
                                send_event(
                                    event_tx,
                                    AgentEvent::ContentDelta(format!(
                                        "{}\n",
                                        stream_result.content
                                    )),
                                );
                            }
                            if let Err(e) = self
                                .handle_tool_calls(&stream_result, turn_id, event_tx, 0)
                                .await
                            {
                                plan_execution_failed = true;
                                send_event(
                                    event_tx,
                                    AgentEvent::Error(format!("Plan tool execution failed: {e}")),
                                );
                            }
                        }
                        self.session.updated_at = Utc::now();
                        send_event(
                            event_tx,
                            AgentEvent::TurnComplete {
                                session_id: self.session.id,
                                total_tokens: self.session.metadata.total_tokens,
                            },
                        );
                    }
                    Err(e) => {
                        plan_execution_failed = true;
                        send_event(
                            event_tx,
                            AgentEvent::Error(format!("Plan execution failed: {e}")),
                        );
                    }
                }

                // Finish any steps that did not map cleanly to a tool batch before clearing UI state.
                if let Some(mut plan_execution) = self.plan_execution.take() {
                    let updates = plan_execution.finish_remaining(!plan_execution_failed);
                    send_plan_updates(event_tx, updates);
                }
                send_event(event_tx, AgentEvent::PlanCleared);
            }
            Err(e) => {
                send_event(
                    event_tx,
                    AgentEvent::Error(format!("Plan generation failed: {e}")),
                );
            }
        }
        Ok(())
    }

    async fn handle_tool_calls(
        &mut self,
        stream_result: &StreamResult,
        turn_id: TurnId,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        depth: u32,
    ) -> Result<(), anyhow::Error> {
        let sub_turn_id = SubTurnId::new_v4();
        let assistant_msg = ReasoningManager::new_assistant_message(
            &stream_result.content,
            if stream_result.reasoning_content.is_empty() {
                None
            } else {
                Some(&stream_result.reasoning_content)
            },
            &stream_result.tool_calls,
            turn_id,
            Some(sub_turn_id),
            true,
        );
        self.session.messages.push(assistant_msg);
        send_reasoning_delta(event_tx, &stream_result.reasoning_content);
        if let Some(last_msg) = self.session.messages.last() {
            ReasoningManager::begin_tool_turn(&mut self.session.reasoning_state, last_msg);
        }

        // Separate subagent calls from regular tool calls
        let mut subagent_calls = Vec::new();
        let mut regular_calls = Vec::new();
        for tc in &stream_result.tool_calls {
            if tc.function.name == "run_subagent" {
                subagent_calls.push(tc.clone());
            } else {
                regular_calls.push(tc.clone());
            }
        }

        // Execute subagent calls
        let mut results: Vec<(ToolCall, ToolResultRecord)> = Vec::new();
        if !subagent_calls.is_empty() {
            let handler = TaskToolHandler::new(
                Arc::new(self.client.clone()),
                self.project_root.clone(),
                self.background_queue.clone(),
                0, // top-level: depth 0
            );
            for tc in subagent_calls {
                let args: Result<SubagentToolArgs, _> =
                    serde_json::from_str(&tc.function.arguments);
                let (result_text, is_error) = match args {
                    Ok(subagent_args) => {
                        let parent_summary = summarize_parent_context(&self.session);
                        handler
                            .handle(&subagent_args, event_tx, Some(parent_summary))
                            .await
                    }
                    Err(e) => (format!("Invalid run_subagent arguments: {e}"), true),
                };
                let record = ToolResultRecord {
                    tool_call_id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    result: result_text,
                    is_error,
                };
                results.push((tc.clone(), record));
            }
        }

        // Separate MCP tools from standard tools
        let mut mcp_calls = Vec::new();
        let mut standard_calls = Vec::new();
        for tc in regular_calls {
            if tc.function.name.contains('.') {
                mcp_calls.push(tc);
            } else {
                standard_calls.push(tc);
            }
        }

        // Deduplicate standard tool calls within the same batch
        let mut deduped = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for tc in standard_calls {
            let key = format!("{}:{}", tc.function.name, tc.function.arguments);
            if seen.insert(key) {
                deduped.push(tc);
            } else {
                send_event(
                    event_tx,
                    AgentEvent::ContentDelta(format!(
                        "Skipping duplicate tool call: {}\n",
                        tc.function.name
                    )),
                );
                // Push a synthetic "already done" result so the model doesn't wait
                let record = ToolResultRecord {
                    tool_call_id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    result: "Tool call deduplicated — same call was already made in this batch."
                        .into(),
                    is_error: false,
                };
                results.push((tc.clone(), record));
            }
        }

        // Execute standard tools
        let policy_config = crate::storage::Config::load(Some(&self.project_root))
            .map(|c| c.policy)
            .unwrap_or_default();
        if !deduped.is_empty() {
            let regular_results = ToolLoop::execute_tools_with_approval(
                &deduped,
                &self.project_root,
                turn_id,
                sub_turn_id,
                &mut self.session,
                event_tx,
                self.yolo_mode,
                &policy_config,
            )
            .await;
            results.extend(regular_results);
        }

        // Execute MCP tools
        if !mcp_calls.is_empty() {
            if let Some(ref mut registry) = self.mcp_registry {
                for tc in mcp_calls {
                    // Policy check
                    let decision = policy::evaluate_tool(
                        &tc.function.name,
                        &tc.function.arguments,
                        &self.project_root,
                        &policy_config,
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
                            if self.yolo_mode {
                                true
                            } else {
                                let (tx, rx) = tokio::sync::oneshot::channel();
                                send_event(
                                    event_tx,
                                    AgentEvent::ToolApprovalNeeded {
                                        tool_name: tc.function.name.clone(),
                                        display: decision.display.clone(),
                                        respond: tx,
                                    },
                                );
                                if let Ok(Ok(true)) =
                                    tokio::time::timeout(std::time::Duration::from_mins(1), rx)
                                        .await
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

                    // Execute MCP tool
                    let args = match crate::mcp::registry::parse_mcp_tool_arguments(
                        &tc.function.arguments,
                    ) {
                        Ok(args) => args,
                        Err(e) => {
                            let result_text = e.to_string();
                            send_event(
                                event_tx,
                                AgentEvent::ToolExecuted {
                                    tool_name: tc.function.name.clone(),
                                    success: false,
                                    summary: result_text.clone(),
                                },
                            );
                            let result_record = ToolResultRecord {
                                tool_call_id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                result: result_text,
                                is_error: true,
                            };
                            results.push((tc.clone(), result_record));
                            continue;
                        }
                    };
                    let start = std::time::Instant::now();
                    let (result_text, is_error) =
                        match registry.call_tool(&tc.function.name, args).await {
                            Ok(text) => (text, false),
                            Err(e) => (e.to_string(), true),
                        };
                    let duration_ms = start.elapsed().as_millis() as u64;

                    // Record in session history
                    let record = crate::deepseek::ToolCallRecord {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                        result_summary: crate::agent::utils::truncate_for_summary(
                            &result_text,
                            200,
                        ),
                        exit_code: if is_error { Some(1) } else { Some(0) },
                        duration_ms,
                        risk_level: decision.display.risk_level.to_string(),
                        approved,
                        at: Utc::now(),
                    };
                    self.session.tool_call_history.push(record);

                    let result_record = ToolResultRecord {
                        tool_call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        result: result_text,
                        is_error,
                    };
                    results.push((tc.clone(), result_record));
                }
            } else {
                for tc in mcp_calls {
                    let record = ToolResultRecord {
                        tool_call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        result: "MCP registry not initialized".into(),
                        is_error: true,
                    };
                    results.push((tc.clone(), record));
                }
            }
        }

        for (tc, result) in &results {
            let tool_msg = ReasoningManager::new_tool_result_message(
                &result.tool_call_id,
                &result.name,
                &result.result,
                result.is_error,
                turn_id,
                sub_turn_id,
            );
            self.session.messages.push(tool_msg);
            send_event(
                event_tx,
                AgentEvent::ToolExecuted {
                    tool_name: result.name.clone(),
                    success: !result.is_error,
                    summary: result.result.clone(),
                },
            );

            // Emit diff event for file-editing tools so the TUI can show an inline preview
            if (tc.function.name == "edit_file" || tc.function.name == "write_file")
                && !result.is_error
            {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                {
                    if let Some(path) = args["path"].as_str() {
                        let stats = crate::workspace::diff::diff_stats(&result.result);
                        send_event(
                            event_tx,
                            AgentEvent::FileDiff {
                                path: path.to_string(),
                                diff: result.result.clone(),
                                stats: stats.to_string(),
                            },
                        );
                    }
                }
            }
        }

        // Self-verification after file edits
        let had_edits = results.iter().any(|(tc, r)| {
            !r.is_error
                && (tc.function.name == "edit_file"
                    || tc.function.name == "write_file"
                    || tc.function.name == "apply_patch")
        });
        if had_edits {
            if policy_config.require_approval_for_command && !self.yolo_mode {
                tracing::debug!("self-verification skipped: command approval is required");
            } else if let Ok(verify) =
                crate::workspace::test_runner::run_self_verification(&self.project_root)
            {
                let verify_msg = ProtocolMessage {
                    id: MessageId::new_v4(),
                    role: Role::System,
                    content: MessageContent::from(format!("[Self-verification] {verify}")),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                    turn_id,
                    sub_turn_id: None,
                    visibility: MessageVisibility::InternalProtocolState,
                };
                self.session.messages.push(verify_msg);
                if !is_unavailable_self_verification(&verify) {
                    send_event(
                        event_tx,
                        AgentEvent::ContentDelta(format!("\n[Self-verification] {verify}\n")),
                    );
                }
            }
        }

        // Advance plan execution step tracker after each tool batch
        if let Some(ref mut plan_exec) = self.plan_execution {
            let had_error = results.iter().any(|(_, r)| r.is_error);
            let updates = plan_exec.updates_after_tool_batch(had_error);
            send_plan_updates(event_tx, updates);
        }

        let tool_defs = self.get_all_tools();
        let project_rules = load_project_rules(&self.project_root);
        let (_, messages) = PromptBuilder::new(
            self.session.reasoning_state.mode_to_model(),
            ExecutionLane::ToolLoopThinking,
            true,
        )
        .build(&self.session, project_rules.as_deref(), None, &tool_defs);

        let followup_request = ChatRequest {
            model: self.session.reasoning_state.mode_to_model().to_string(),
            messages,
            tools: Some(tool_defs),
            thinking: Some(ThinkingConfig::enabled()),
            response_format: None,
            stream: true,
            max_tokens: Some(8192),
        };

        let mut emitted = EmittedStreamDeltas::default();
        match self
            .client
            .chat_stream_accumulated_with_deltas(&followup_request, |chunk| {
                emitted.merge(emit_stream_chunk_deltas(event_tx, chunk));
            })
            .await
        {
            Ok(followup_result) => {
                if !followup_result.tool_calls.is_empty() && depth < 10 {
                    Box::pin(self.handle_tool_calls(
                        &followup_result,
                        turn_id,
                        event_tx,
                        depth + 1,
                    ))
                    .await?;
                } else {
                    if !followup_result.tool_calls.is_empty() {
                        send_event(
                            event_tx,
                            AgentEvent::Error(
                                "Tool call recursion limit reached for this turn".into(),
                            ),
                        );
                    }
                    ReasoningManager::complete_tool_loop(
                        &mut self.session.reasoning_state,
                        &mut self.session.messages,
                    );
                    let final_msg = ReasoningManager::new_assistant_message(
                        &followup_result.content,
                        (!followup_result.reasoning_content.is_empty())
                            .then_some(&followup_result.reasoning_content),
                        &[],
                        turn_id,
                        None,
                        false,
                    );
                    self.session.messages.push(final_msg);
                    if !emitted.reasoning {
                        send_reasoning_delta(event_tx, &followup_result.reasoning_content);
                    }
                    if !emitted.content {
                        send_event(event_tx, AgentEvent::ContentDelta(followup_result.content));
                    }
                    if let Some(ref usage) = followup_result.usage {
                        self.session.metadata.total_tokens += u64::from(usage.total_tokens);
                        self.session.metadata.total_cost_estimate +=
                            usage.estimate_cost_cny(&self.session.reasoning_state.mode_to_model());
                        send_event(
                            event_tx,
                            AgentEvent::StreamDone {
                                finish_reason: None,
                                usage: Some(usage.clone()),
                                cache: Some(CacheUsage::from_usage(usage)),
                            },
                        );
                    }
                }
            }
            Err(e) => {
                send_event(
                    event_tx,
                    AgentEvent::Error(format!("Tool loop failed: {e}")),
                );
            }
        }
        Ok(())
    }
}

impl ReasoningState {
    fn mode_to_model(&self) -> DeepSeekModel {
        match self.effort {
            ReasoningEffort::Max | ReasoningEffort::High => DeepSeekModel::Pro,
            _ => DeepSeekModel::Flash,
        }
    }
}

// ---------------------------------------------------------------------------
// Clarification helper
// ---------------------------------------------------------------------------

fn generate_clarification_questions(
    _user_input: &str,
    assessment: &ComplexityAssessment,
) -> Vec<String> {
    let mut questions = Vec::new();

    // Generic ambiguity
    if assessment.reason_codes.contains(&ReasonCode::AmbiguousTask) {
        questions.push("能否更具体地描述你希望达成的结果？".into());
        questions.push("这个任务的验收标准是什么？".into());
    }

    // Scope uncertainty
    if assessment.predicted_write_files == 0
        && !assessment.reason_codes.contains(&ReasonCode::ReadOnly)
    {
        questions.push("你期望修改哪些文件或模块？".into());
    }

    // Command uncertainty
    if assessment.predicted_commands > 0 {
        questions.push("是否需要运行特定命令（如测试、构建）来验证？".into());
    }

    // Risk flags
    if !assessment.risk_flags.is_empty() {
        questions.push("这是一个高风险操作，是否已备份或确认影响范围？".into());
    }

    // Fallback if still empty
    if questions.is_empty() {
        questions.push("能否补充更多细节，以便更准确地评估任务范围？".into());
    }

    questions
}

fn send_event(tx: &mpsc::UnboundedSender<AgentEvent>, event: AgentEvent) {
    if tx.send(event).is_err() {
        tracing::warn!("Agent event channel closed; event dropped");
    }
}

fn send_reasoning_delta(tx: &mpsc::UnboundedSender<AgentEvent>, reasoning: &str) {
    if !reasoning.trim().is_empty() {
        send_event(tx, AgentEvent::ReasoningDelta(reasoning.to_string()));
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct EmittedStreamDeltas {
    content: bool,
    reasoning: bool,
}

impl EmittedStreamDeltas {
    fn merge(&mut self, other: Self) {
        self.content |= other.content;
        self.reasoning |= other.reasoning;
    }
}

fn emit_stream_chunk_deltas(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    chunk: &crate::deepseek::models::StreamChunk,
) -> EmittedStreamDeltas {
    let mut emitted = EmittedStreamDeltas::default();
    for choice in &chunk.choices {
        if let Some(content) = &choice.delta.content {
            if !content.is_empty() {
                send_event(tx, AgentEvent::ContentDelta(content.clone()));
                emitted.content = true;
            }
        }
        if let Some(reasoning) = &choice.delta.reasoning_content {
            if !reasoning.trim().is_empty() {
                send_event(tx, AgentEvent::ReasoningDelta(reasoning.clone()));
                emitted.reasoning = true;
            }
        }
    }
    emitted
}

fn send_plan_updates(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    updates: impl IntoIterator<Item = PlanProgressUpdate>,
) {
    for update in updates {
        send_plan_update(tx, update);
    }
}

fn send_plan_update(tx: &mpsc::UnboundedSender<AgentEvent>, update: PlanProgressUpdate) {
    send_event(
        tx,
        AgentEvent::PlanStepUpdate {
            index: update.index,
            total: update.total,
            description: update.description,
            status: update.status,
        },
    );
}

fn is_unavailable_self_verification(output: &str) -> bool {
    output
        .to_ascii_lowercase()
        .contains("no verification available for this project type")
}

/// Summarize the parent agent's recent session state so subagents can
/// inherit context and avoid redundant work.
fn summarize_parent_context(session: &Session) -> String {
    let mut parts = vec!["## Parent Agent Context".to_string()];

    // Recent conversation turns (last 3 user + assistant pairs)
    let recent_msgs: Vec<_> = session.messages.iter().rev().take(6).collect();
    if !recent_msgs.is_empty() {
        parts.push("\nRecent conversation:".to_string());
        for msg in recent_msgs.iter().rev() {
            let role_label = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::Tool => "Tool",
                Role::System => "System",
            };
            let text = msg.content.to_string_lossy();
            let truncated = if text.chars().count() > 300 {
                let prefix: String = text.chars().take(300).collect();
                format!("{}... [{} more chars]", prefix, text.chars().count() - 300)
            } else {
                text
            };
            parts.push(format!("{role_label}: {truncated}"));
        }
    }

    // Recent tool calls so the subagent knows what work was already done
    let recent_tools: Vec<_> = session.tool_call_history.iter().rev().take(5).collect();
    if !recent_tools.is_empty() {
        parts.push("\nRecent tools used by parent:".to_string());
        for tc in recent_tools.iter().rev() {
            parts.push(format!("- {}: {}", tc.name, tc.result_summary));
        }
    }

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        emit_stream_chunk_deltas, generate_plan_options, plan_uses_chinese, AgentEvent,
        PlanExecutionState, PlanStepStatus,
    };
    use crate::deepseek::models::StreamChunk;
    use crate::plan::executor::PlanStep;
    use crate::plan::schema::{Plan, Risk, RiskLevel};

    fn sample_plan_state() -> PlanExecutionState {
        PlanExecutionState::new(vec![
            PlanStep::ReadFile {
                path: "src/agent/orchestrator.rs".to_string(),
                reason: "inspect plan state".to_string(),
            },
            PlanStep::Verify {
                description: "run focused tests".to_string(),
            },
        ])
    }

    fn sample_plan(summary: &str) -> Plan {
        Plan {
            summary: summary.to_string(),
            target_files: vec![],
            steps: vec!["创建任务列表".to_string()],
            risks: vec![Risk {
                level: RiskLevel::Low,
                description: "low".to_string(),
            }],
            verification: vec![],
            requires_write: true,
            requires_command: false,
            recommended_model: None,
            thinking: None,
        }
    }

    #[test]
    fn chinese_plan_gets_chinese_execution_options() {
        let plan = sample_plan("创建任务并逐步展示执行计划");

        assert!(plan_uses_chinese(&plan));
        let labels = generate_plan_options(&plan, true)
            .into_iter()
            .map(|option| option.label)
            .collect::<Vec<_>>();

        assert!(labels.iter().any(|label| label.contains("自动执行")));
        assert!(labels.iter().any(|label| label.contains("需要确认后执行")));
        assert!(labels.iter().any(|label| label == "取消"));
    }

    #[test]
    fn plan_state_marks_failed_batch_without_starting_next_step() {
        let mut state = sample_plan_state();

        let updates = state.updates_after_tool_batch(true);

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].index, 0);
        assert_eq!(updates[0].status, PlanStepStatus::Failed);
    }

    #[test]
    fn plan_state_finishes_remaining_steps_as_failed_after_any_failure() {
        let mut state = sample_plan_state();
        let _ = state.updates_after_tool_batch(true);

        let updates = state.finish_remaining(true);

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].index, 1);
        assert_eq!(updates[0].status, PlanStepStatus::Failed);
    }

    #[test]
    fn plan_state_advances_and_finishes_unmapped_steps_on_success() {
        let mut state = sample_plan_state();

        let first_batch = state.updates_after_tool_batch(false);
        let remaining = state.finish_remaining(true);

        let first_statuses: Vec<_> = first_batch.iter().map(|update| update.status).collect();
        let remaining_statuses: Vec<_> = remaining.iter().map(|update| update.status).collect();
        assert_eq!(
            first_statuses,
            vec![PlanStepStatus::Done, PlanStepStatus::Running]
        );
        assert_eq!(remaining_statuses, vec![PlanStepStatus::Done]);
    }

    #[test]
    fn stream_chunk_deltas_are_emitted_before_final_accumulation() {
        let chunk = serde_json::from_str::<StreamChunk>(
            r#"{"choices":[{"index":0,"delta":{"reasoning_content":"thinking","content":"hello"},"finish_reason":null}],"usage":null}"#,
        )
        .expect("valid stream chunk");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let emitted = emit_stream_chunk_deltas(&tx, &chunk);

        assert!(emitted.content);
        assert!(emitted.reasoning);
        assert!(matches!(
            rx.try_recv().expect("content delta"),
            AgentEvent::ContentDelta(text) if text == "hello"
        ));
        assert!(matches!(
            rx.try_recv().expect("reasoning delta"),
            AgentEvent::ReasoningDelta(text) if text == "thinking"
        ));
    }
}

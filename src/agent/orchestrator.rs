use std::io::{Read, Write};
use std::path::{Component, Path};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{atomic::AtomicBool, Arc};

use chrono::Utc;
use tokio::sync::mpsc;

use crate::deepseek::client::{ChatStreamClient, DeepSeekClient};
use crate::deepseek::tools as ds_tools;
use crate::deepseek::{
    temperature_for_lane, thinking_config_for_lane, CacheUsage, ChatRequest, DeepSeekModel,
    ExecutionLane, FinishReason, MessageContent, MessageId, MessageVisibility, ModelCapability,
    ProtocolMessage, Role, Session, SessionId, StreamResult, SubTurnId, ThinkingConfig, ToolCall,
    ToolCallFunction, ToolDefinition, ToolResultRecord, TurnId, Usage,
};
use crate::hooks::{HookEvent, HookPayload, HookRunSummary};
use crate::plan;
use crate::plan::schema::{Plan, RiskLevel};
use crate::policy;
use crate::provider::request_model_name_for_config;
use crate::runtime::tool_runtime::{
    ApprovalFuture, ApprovalOutcome, ApprovalResolver, McpRegistryRuntimeBackend, ToolRuntime,
    ToolRuntimeContext,
};
use crate::search::{self, SearchMatch};
use crate::storage::{EventLogStore, SessionEvent, SessionEventKind};

/// Per-turn output-token budget. Covers reasoning_content + content
/// combined. DeepSeek V4 Pro/Flash advertise a max of 384K, but real-world
/// reasoning rarely needs more than ~20-30K. 8K (the previous value) was
/// chronically under-budget: complex prompts could burn the full budget on
/// chain-of-thought and never get to write a final answer.
const TURN_MAX_OUTPUT_TOKENS: u32 = 32_768;
const PATCH_CHECK_STDOUT_BYTES: usize = 256 * 1024;
const PATCH_CHECK_STDERR_BYTES: usize = 256 * 1024;

use super::background::{BackgroundQueue, BackgroundTaskSnapshot};
use super::compact::{
    build_compact_snapshot, retained_messages, should_auto_compact, DEFAULT_COMPACT_KEEP_MESSAGES,
};
use super::event_sink::EventSink;
use super::lanes::{classify_task, TaskClass};
use super::prompt_builder::{load_project_rules, PromptBuilder};
use super::reasoning::ReasoningManager;
use super::router::{ComplexityAssessment, ComplexityRouter, ReasonCode, Route};
use super::subagent::SubagentToolArgs;
use super::swarm::{SwarmCoordinator, SwarmRunOptions, SwarmTaskStatus};
use super::task_tool::TaskToolHandler;
use super::tool_loop::{ToolLoop, ToolLoopResult};

/// Events emitted by the orchestrator during execution.
#[derive(Debug)]
pub enum AgentEvent {
    UserMessage {
        content: String,
    },
    ContentDelta(String),
    ReasoningDelta(String),
    TokenDelta {
        input_tokens: u64,
        output_tokens: u64,
    },
    ToolApprovalNeeded {
        tool_name: String,
        display: policy::ApprovalDisplay,
        respond: tokio::sync::oneshot::Sender<bool>,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        arguments: String,
    },
    ToolExecuted {
        tool_name: String,
        success: bool,
        summary: String,
    },
    UserQuestionRequested {
        title: String,
        options: Vec<String>,
        summary: String,
        descriptions: Vec<String>,
        previews: Vec<Option<String>>,
        multi_select: bool,
    },
    ContextCompacted {
        summary: String,
        reason: String,
        before_tokens: u64,
        after_tokens: u64,
    },
    HookExecuted {
        event: HookEvent,
        success: bool,
        summary: String,
        command_count: usize,
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
        policy_decision: policy::PolicyDecision,
        respond: tokio::sync::oneshot::Sender<bool>,
    },
    /// A local swarm run has started.
    SwarmStarted {
        run_id: String,
        summary: String,
        total: usize,
    },
    /// A swarm task changed status.
    SwarmTaskUpdated {
        run_id: String,
        task_id: String,
        role: String,
        status: String,
        description: String,
    },
    /// A local swarm run has finished.
    SwarmFinished {
        run_id: String,
        success: bool,
        summary: String,
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
        kind: DecisionKind,
        title: String,
        options: Vec<String>,
        respond: tokio::sync::oneshot::Sender<usize>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    PlanAction,
    Clarification,
    Conflict,
}

#[derive(Debug, Clone)]
struct PatchHandlingReport {
    text: String,
    validation_failed: bool,
    changed_files: Vec<String>,
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

fn plan_execution_prompt(use_chinese: bool) -> &'static str {
    if use_chinese {
        "请按上面的计划逐步执行。只调用工具和更新计划状态，不要输出逐步说明、思考过程或中英混杂的执行旁白；最终结果由系统整理给用户。"
    } else {
        "Execute the plan step by step. Use tools and plan status updates only; do not stream narration, step-by-step commentary, or reasoning prose. The system will summarize the final result for the user."
    }
}

fn plan_execution_context(plan_json: &str, execution_prompt: &str) -> Vec<String> {
    vec![
        format!("Current approved execution plan JSON:\n{plan_json}"),
        execution_prompt.to_string(),
    ]
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

fn plan_or_input_uses_chinese(plan: &Plan, user_input: &str) -> bool {
    contains_cjk(user_input) || plan_uses_chinese(plan)
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
    /// Session-scoped approval mode (Claude Code-style). Layers on top of
    /// `yolo_mode`: Bypass/Auto behave like yolo, AcceptEdits auto-approves
    /// edit tools but still asks for shell/subagent, Plan/ReadOnly hard-block
    /// mutating tools. Default = ask every mutating tool (legacy behavior).
    pub permission_mode: crate::policy::PermissionMode,
    plan_execution: Option<PlanExecutionState>,
    mcp_registry: Option<crate::mcp::McpRegistry>,
    mcp_initialized: bool,
    event_log_store: Option<EventLogStore>,
    swarm_cancel_token: Option<Arc<AtomicBool>>,
    lifecycle_hooks_started: bool,
    /// Optional per-turn allowed-tools allowlist consumed on the next turn.
    /// Populated when the user runs a custom slash command whose frontmatter
    /// declares `allowed-tools:`; cleared right after the turn-scoped policy
    /// is materialized so the restriction does not leak into later turns.
    pending_allowed_tools: Option<Vec<String>>,
    /// Mirror of `pending_allowed_tools` for the lifetime of the current
    /// turn. Picked up at the start of `run_turn_inner`, applied to every
    /// policy clone the turn consults, and cleared on turn exit.
    current_turn_allowed_tools: Option<Vec<String>>,
    /// Memoized `storage::Config` for the lifetime of a single turn.
    /// `Config::load` does 3 `fs::read_to_string` calls + TOML parse + merge
    /// each time; without this cache the orchestrator was hitting disk
    /// 9× per turn (policy decisions, hook lookups, mcp setup, sandbox
    /// rebuilds, …). Reset at turn entry, populated lazily on first read.
    cached_turn_config: Option<crate::storage::Config>,
    /// Test-only override for the streaming model client. When set, all model
    /// turns stream through it instead of `client`, so the turn loop can be
    /// exercised without a network round-trip. `None` in production.
    stream_override: Option<Arc<dyn ChatStreamClient>>,
}

impl Orchestrator {
    #[must_use]
    pub fn new(client: DeepSeekClient, project_root: std::path::PathBuf, session: Session) -> Self {
        let event_log_store =
            crate::storage::user_home_dir().map(|home| EventLogStore::new(home.join(".octocode")));
        Self {
            client,
            project_root,
            session,
            background_queue: BackgroundQueue::new(),
            yolo_mode: false,
            permission_mode: crate::policy::PermissionMode::Default,
            plan_execution: None,
            mcp_registry: None,
            mcp_initialized: false,
            event_log_store,
            swarm_cancel_token: None,
            lifecycle_hooks_started: false,
            pending_allowed_tools: None,
            current_turn_allowed_tools: None,
            cached_turn_config: None,
            stream_override: None,
        }
    }

    /// Inject the streaming model client used for model turns (tests only).
    /// Production keeps the default, which streams through `self.client`.
    #[must_use]
    pub fn with_stream_client(mut self, client: Arc<dyn ChatStreamClient>) -> Self {
        self.stream_override = Some(client);
        self
    }

    /// The streaming client to use for a model turn: the injected override if
    /// present, otherwise the real `client`.
    fn stream_client(&self) -> &dyn ChatStreamClient {
        match &self.stream_override {
            Some(client) => client.as_ref(),
            None => &self.client,
        }
    }

    /// Get the project config for the current turn. Loaded once on first
    /// call within a turn and reused for the rest; `run_turn_inner` resets
    /// the cache so the next turn picks up edits to `.octocode/config.toml`.
    /// Falls back to `Config::default()` on read errors, matching the
    /// previous inline `unwrap_or_default()` callers' contract.
    fn turn_config(&mut self) -> &crate::storage::Config {
        if self.cached_turn_config.is_none() {
            let config = crate::storage::Config::load(Some(&self.project_root)).unwrap_or_default();
            self.cached_turn_config = Some(config);
        }
        self.cached_turn_config
            .as_ref()
            .expect("just populated above")
    }

    /// Stage an allowed-tools allowlist that the next turn must enforce.
    /// Calling again before the turn runs replaces the pending list (so the
    /// most recent custom command wins).
    pub fn stage_allowed_tools(&mut self, allowed_tools: Option<Vec<String>>) {
        self.pending_allowed_tools = allowed_tools;
    }

    /// Materialize the active allowed-tools allowlist into a policy clone.
    /// Called right after a `PolicyConfig::clone()` so the next
    /// `evaluate_tool` call honors per-command restrictions.
    fn apply_active_allowed_tools(&self, policy: &mut crate::storage::config::PolicyConfig) {
        if let Some(allowed) = &self.current_turn_allowed_tools {
            policy.allowed_tools = Some(allowed.clone());
        }
    }

    pub fn set_active_model(&mut self, model: DeepSeekModel) {
        let model = model.canonical();
        let previous = self.session.reasoning_state.effective_model();
        if previous == model {
            self.session.reasoning_state.selected_model = Some(model);
            return;
        }
        self.session.reasoning_state.selected_model = Some(model.clone());
        self.session
            .metadata
            .model_switches
            .push(crate::deepseek::ModelSwitchRecord {
                from: previous,
                to: model,
                at: Utc::now(),
                reason: "user selection".to_string(),
            });
    }

    fn record_usage_event(&mut self, usage: &Usage) {
        let model = self.session.reasoning_state.effective_model();
        let (provider_name, model_name) = {
            let config = self.turn_config();
            (
                config.provider.default.as_str().to_string(),
                request_model_name_for_config(&config.provider, &model),
            )
        };
        if let Err(error) = crate::storage::record_usage_event(
            &self.project_root,
            &provider_name,
            &model_name,
            usage,
        ) {
            tracing::warn!("failed to record usage event: {error}");
        }
    }

    /// Fold one model response's usage into the session totals exactly once
    /// (persisted event + tokens + cost + cache) and return its cache split.
    ///
    /// Must be called for every model call in a turn — including the
    /// tool-emitting calls. Previously only the final no-tool response was
    /// counted, so multi-tool turns under-reported tokens, cost, and cache.
    fn accrue_usage(&mut self, usage: &Usage) -> CacheUsage {
        self.record_usage_event(usage);
        let cache = CacheUsage::from_usage(usage);
        self.session.metadata.total_tokens += u64::from(usage.total_tokens);
        self.session.metadata.total_cost_estimate +=
            usage.estimate_cost_cny(&self.session.reasoning_state.effective_model());
        self.session.metadata.prompt_cache_hit_tokens = self
            .session
            .metadata
            .prompt_cache_hit_tokens
            .saturating_add(cache.prompt_cache_hit_tokens);
        self.session.metadata.prompt_cache_miss_tokens = self
            .session
            .metadata
            .prompt_cache_miss_tokens
            .saturating_add(cache.prompt_cache_miss_tokens);
        cache
    }

    pub fn set_swarm_cancel_token(&mut self, token: Arc<AtomicBool>) {
        self.swarm_cancel_token = Some(token);
    }

    fn record_event(&self, turn_id: Option<TurnId>, kind: SessionEventKind) {
        if let Some(store) = &self.event_log_store {
            let event = SessionEvent::new(self.session.id, turn_id, kind);
            if let Err(err) = store.append(&self.project_root, &event) {
                tracing::warn!("failed to append session event: {err}");
            }
        }
    }

    fn write_artifact(&self, slug: &str, content: &str) {
        if let Some(store) = &self.event_log_store {
            match store.write_artifact(&self.project_root, &self.session.id, slug, content) {
                Ok(path) => tracing::info!("wrote Octocode artifact: {}", path.display()),
                Err(err) => tracing::warn!("failed to write Octocode artifact: {err}"),
            }
        }
    }

    fn emit_event(
        &self,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        turn_id: Option<TurnId>,
        event: AgentEvent,
    ) {
        EventSink::new(
            event_tx,
            self.event_log_store.clone(),
            &self.project_root,
            self.session.id,
            turn_id,
        )
        .emit(event);
    }

    fn record_hook_summary(
        &self,
        turn_id: Option<TurnId>,
        summary: &HookRunSummary,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) {
        emit_hook_summary_event(
            event_tx,
            &self.event_log_store,
            &self.project_root,
            self.session.id,
            turn_id,
            summary,
        );
    }

    async fn run_lifecycle_hook(
        &mut self,
        hook_event: HookEvent,
        turn_id: Option<TurnId>,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        summary: Option<String>,
    ) {
        let runtime_config = self.turn_config().clone();
        let mut payload = HookPayload::new(
            hook_event,
            self.session.id.to_string(),
            self.project_root.clone(),
        );
        payload.turn_id = turn_id.map(|id| id.to_string());
        payload.summary = summary;
        if let Some(result) = crate::hooks::run_configured_hooks(
            hook_event,
            &runtime_config.hooks,
            &payload,
            &self.project_root,
            runtime_config.policy.command_timeout_seconds,
        )
        .await
        {
            self.record_hook_summary(turn_id, &result, event_tx);
        }
    }

    fn should_run_swarm(
        &mut self,
        user_input: &str,
        assessment: Option<&ComplexityAssessment>,
        forced: bool,
    ) -> bool {
        let (subagents_enabled, swarm_enabled, auto_decompose) = {
            let config = self.turn_config();
            (
                config.subagent.enabled,
                config.subagent.swarm_enabled,
                config.subagent.auto_decompose,
            )
        };
        if !subagents_enabled || !swarm_enabled {
            return false;
        }
        if forced {
            return true;
        }
        let lower = user_input.to_lowercase();
        let multi_file_hint = lower.matches("src/").count() + lower.matches(".rs").count() >= 2;
        let complex_hint = contains_any(
            &lower,
            &[
                "架构",
                "审查",
                "review",
                "多文件",
                "debug",
                "修复测试",
                "测试修复",
                "并行",
                "多 agent",
                "multi-agent",
            ],
        );
        let router_complex = assessment.is_some_and(|a| a.route == Route::PlanReview);
        auto_decompose && (multi_file_hint || (router_complex && complex_hint))
    }

    async fn run_swarm_mode(
        &mut self,
        description: &str,
        prompt: &str,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        turn_id: TurnId,
    ) -> Result<(), anyhow::Error> {
        let config = self.turn_config().clone();
        let coordinator = SwarmCoordinator::new(
            Arc::new(self.client.clone()),
            self.project_root.clone(),
            config.subagent.max_parallel,
        );
        let plan = coordinator.plan_hybrid(description, prompt, &[]).await;
        let use_chinese = contains_cjk(description) || contains_cjk(prompt);
        self.write_artifact(
            "swarm-plan",
            &format_swarm_plan_artifact(prompt, &plan, use_chinese),
        );
        self.emit_event(
            event_tx,
            Some(turn_id),
            AgentEvent::PlanStarted {
                summary: plan.summary.clone(),
                total: plan.tasks.len(),
            },
        );
        for (index, task) in plan.tasks.iter().enumerate() {
            self.emit_event(
                event_tx,
                Some(turn_id),
                AgentEvent::PlanStepUpdate {
                    index,
                    total: plan.tasks.len(),
                    description: if task.focus_files.is_empty() {
                        format!("agent {} · {}", task.role.as_str(), task.description)
                    } else {
                        format!(
                            "agent {} · {} · 文件 {}",
                            task.role.as_str(),
                            task.description,
                            task.focus_files.join(", ")
                        )
                    },
                    status: PlanStepStatus::Pending,
                },
            );
        }

        self.record_event(
            Some(turn_id),
            SessionEventKind::SwarmStarted {
                run_id: plan.run_id.clone(),
                summary: plan.summary.clone(),
                total: plan.tasks.len(),
            },
        );
        for task in &plan.tasks {
            self.record_event(
                Some(turn_id),
                SessionEventKind::SwarmTaskUpdated {
                    run_id: plan.run_id.clone(),
                    task_id: task.id.clone(),
                    role: task.role.as_str().to_string(),
                    status: SwarmTaskStatus::Pending.as_str().to_string(),
                    description: task.description.clone(),
                },
            );
        }

        let mut result = coordinator
            .run_with_options(
                plan,
                event_tx,
                SwarmRunOptions {
                    command_requires_approval: config.subagent.command_requires_approval,
                    cancel_token: self.swarm_cancel_token.clone(),
                    emit_finished: false,
                },
            )
            .await;
        // Fold the swarm's aggregate subagent token usage into the session
        // total so swarm spend shows in the session usage display (the
        // per-call usage telemetry is recorded inside the executor).
        self.session.metadata.total_tokens += result.token_usage;
        let patch_report = self
            .handle_swarm_pending_patches(
                &result,
                event_tx,
                turn_id,
                config.subagent.write_requires_approval,
                use_chinese,
            )
            .await;
        if patch_report
            .as_ref()
            .is_some_and(|report| report.validation_failed)
        {
            result.success = false;
        }
        if let Some(report) = &patch_report {
            result.files_written.extend(report.changed_files.clone());
            result.files_written.sort();
            result.files_written.dedup();
        }
        let final_output = result.format_for_user(
            patch_report.as_ref().map(|report| report.text.as_str()),
            use_chinese,
        );
        self.emit_event(
            event_tx,
            Some(turn_id),
            AgentEvent::SwarmFinished {
                run_id: result.run_id.clone(),
                success: result.success,
                summary: result.summary.clone(),
            },
        );
        let assistant_msg =
            ReasoningManager::new_assistant_message(&final_output, None, &[], turn_id, None, false);
        self.session.messages.push(assistant_msg);
        self.record_event(
            Some(turn_id),
            SessionEventKind::SwarmFinished {
                run_id: result.run_id.clone(),
                success: result.success,
                summary: result.summary.clone(),
            },
        );
        self.record_event(
            Some(turn_id),
            SessionEventKind::AssistantVisible {
                content: final_output.clone(),
            },
        );
        self.record_event(
            Some(turn_id),
            SessionEventKind::TurnFinished { total_tokens: 0 },
        );
        send_event(event_tx, AgentEvent::ContentDelta(final_output));
        send_event(
            event_tx,
            AgentEvent::TurnComplete {
                session_id: self.session.id,
                total_tokens: self.session.metadata.total_tokens,
            },
        );
        Ok(())
    }

    async fn handle_swarm_pending_patches(
        &self,
        result: &crate::agent::swarm::SwarmResult,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        turn_id: TurnId,
        write_requires_approval: bool,
        use_chinese: bool,
    ) -> Option<PatchHandlingReport> {
        if result.pending_patches.is_empty() {
            return None;
        }
        for patch in &result.pending_patches {
            self.record_event(
                Some(turn_id),
                SessionEventKind::SwarmPatchPending {
                    run_id: result.run_id.clone(),
                    task_id: patch.task_id.clone(),
                    summary: patch.summary.clone(),
                    changed_files: patch.changed_files.clone(),
                    conflict: patch.conflict,
                },
            );
        }
        if result.cancelled || result.tasks_failed > 0 {
            return Some(PatchHandlingReport {
                text: if use_chinese {
                    format!(
                        "未自动写入：蜂群任务没有全部完成（失败 {} / 总计 {}）。为避免应用不完整补丁，已保留 pending patch 到事件日志。",
                        result.tasks_failed, result.tasks_total
                    )
                } else {
                    format!(
                        "Pending patch not applied: swarm did not complete cleanly ({} failed / {} total). The patch was kept in the event log.",
                        result.tasks_failed, result.tasks_total
                    )
                },
                validation_failed: true,
                changed_files: Vec::new(),
            });
        }
        if !result.patch_conflicts.is_empty() {
            return Some(PatchHandlingReport {
                text: if use_chinese {
                    format!(
                        "未自动写入：pending patch 存在文件冲突：{}。没有修改文件。",
                        result.patch_conflicts.join(", ")
                    )
                } else {
                    format!(
                        "Pending patch blocked: conflicting files {}. No files were modified.",
                        result.patch_conflicts.join(", ")
                    )
                },
                validation_failed: true,
                changed_files: Vec::new(),
            });
        }

        let combined_patch = result
            .pending_patches
            .iter()
            .map(|patch| patch.patch.trim())
            .collect::<Vec<_>>()
            .join("\n");
        let changed_files = result
            .pending_patches
            .iter()
            .flat_map(|patch| patch.changed_files.iter().cloned())
            .collect::<Vec<_>>();

        if let Err(error) =
            validate_swarm_patch_for_auto_apply(&self.project_root, &combined_patch, &changed_files)
        {
            return Some(PatchHandlingReport {
                text: if use_chinese {
                    format!("未自动写入：安全检查未通过：{error}。没有修改文件。")
                } else {
                    format!(
                        "Pending patch blocked by safety check: {error}. No files were modified."
                    )
                },
                validation_failed: true,
                changed_files: Vec::new(),
            });
        }

        let call = ToolCall {
            id: format!("swarm-patch-{}", uuid::Uuid::new_v4()),
            call_type: "function".to_string(),
            function: ToolCallFunction {
                name: "apply_patch".to_string(),
                arguments: serde_json::json!({ "patch": combined_patch }).to_string(),
            },
        };
        // `&self` method — fresh load instead of turn cache.
        let runtime_config =
            crate::storage::Config::load(Some(&self.project_root)).unwrap_or_default();
        let mut policy_config = runtime_config.policy.clone();
        self.apply_active_allowed_tools(&mut policy_config);
        let patch_policy = policy::evaluate_tool(
            &call.function.name,
            &call.function.arguments,
            &self.project_root,
            &policy_config,
        );
        if patch_policy.action == policy::PolicyAction::Deny {
            return Some(PatchHandlingReport {
                text: if use_chinese {
                    format!(
                        "未自动写入：策略阻止 pending patch：{}。",
                        patch_policy.reason
                    )
                } else {
                    format!(
                        "Pending patch was not applied: policy blocked it: {}.",
                        patch_policy.reason
                    )
                },
                validation_failed: true,
                changed_files: Vec::new(),
            });
        }

        if write_requires_approval
            || (!self.yolo_mode
                && matches!(
                    patch_policy.action,
                    policy::PolicyAction::AskOnce | policy::PolicyAction::AskSession
                ))
        {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.emit_event(
                event_tx,
                Some(turn_id),
                AgentEvent::ToolApprovalNeeded {
                    tool_name: "apply_patch".to_string(),
                    display: policy::ApprovalDisplay {
                        title: if use_chinese {
                            "应用蜂群 pending patch".to_string()
                        } else {
                            "Apply swarm pending patch".to_string()
                        },
                        description: if use_chinese {
                            format!(
                                "{} 个 pending patch，{} 个文件",
                                result.pending_patches.len(),
                                changed_files.len()
                            )
                        } else {
                            format!(
                                "{} pending patch(es), {} file(s)",
                                result.pending_patches.len(),
                                changed_files.len()
                            )
                        },
                        risk_level: policy::RiskLevel::WriteProject,
                        details: swarm_patch_approval_details(
                            result,
                            &changed_files,
                            &combined_patch,
                            use_chinese,
                        ),
                    },
                    respond: tx,
                },
            );
            let approved = matches!(
                tokio::time::timeout(std::time::Duration::from_mins(5), rx).await,
                Ok(Ok(true))
            );
            if !approved {
                return Some(PatchHandlingReport {
                    text: if use_chinese {
                        "未写入 pending patch：用户取消或确认超时。".into()
                    } else {
                        "Pending patch was not applied: user denied or approval timed out.".into()
                    },
                    validation_failed: true,
                    changed_files: Vec::new(),
                });
            }
        }

        self.record_event(
            Some(turn_id),
            SessionEventKind::ToolCallStarted {
                tool_call_id: call.id.clone(),
                name: call.function.name.clone(),
                arguments: call.function.arguments.clone(),
            },
        );
        let backend = crate::tools::backend::LocalToolBackend;
        let execution = crate::tools::backend::ToolBackend::execute(
            &backend,
            &call,
            &crate::tools::backend::ToolExecutionContext {
                project_root: self.project_root.clone(),
                dispatch_config: crate::tools::dispatch::ToolDispatchConfig::from_policy(
                    &policy_config,
                ),
            },
        )
        .await;
        self.record_event(
            Some(turn_id),
            SessionEventKind::ToolCallFinished {
                tool_call_id: call.id,
                name: call.function.name,
                success: execution.success,
                summary: execution.summary.clone(),
                duration_ms: execution.duration_ms,
                changed_files: execution.changed_files.clone(),
            },
        );
        for path in &execution.changed_files {
            self.record_event(
                Some(turn_id),
                SessionEventKind::FileChanged {
                    path: path.clone(),
                    stats: "swarm pending patch applied".to_string(),
                },
            );
        }
        self.record_event(
            Some(turn_id),
            SessionEventKind::SwarmPatchApplied {
                run_id: result.run_id.clone(),
                success: execution.success,
                summary: execution.summary.clone(),
                changed_files: execution.changed_files.clone(),
            },
        );
        let validation_report = if execution.success {
            self.run_swarm_post_apply_validation(result, event_tx, turn_id, use_chinese)
                .await
        } else {
            None
        };
        let mut report = if use_chinese {
            format!(
                "pending patch 写入{}：{}",
                if execution.success {
                    "成功"
                } else {
                    "失败"
                },
                execution.summary
            )
        } else {
            format!(
                "Pending patch apply {}: {}",
                if execution.success {
                    "succeeded"
                } else {
                    "failed"
                },
                execution.summary
            )
        };
        let mut validation_failed = !execution.success;
        if let Some((validation, failed)) = validation_report {
            validation_failed |= failed;
            if use_chinese {
                report.push_str("\n\n写入后验证：\n");
            } else {
                report.push_str("\n\nPost-apply validation:\n");
            }
            report.push_str(&validation);
        }
        Some(PatchHandlingReport {
            text: report,
            validation_failed,
            changed_files: execution.changed_files,
        })
    }

    async fn run_swarm_post_apply_validation(
        &self,
        result: &crate::agent::swarm::SwarmResult,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        turn_id: TurnId,
        use_chinese: bool,
    ) -> Option<(String, bool)> {
        if result.validation_commands.is_empty() {
            return None;
        }
        // `&self` method — can't reach into the turn cache. Load fresh.
        // Acceptable because this path runs at most once per swarm turn.
        let runtime_config =
            crate::storage::Config::load(Some(&self.project_root)).unwrap_or_default();
        let mut policy_config = runtime_config.policy.clone();
        self.apply_active_allowed_tools(&mut policy_config);
        let backend = crate::tools::backend::LocalToolBackend;
        let mut lines = Vec::new();
        let mut failed = false;
        for command in result.validation_commands.iter().take(3) {
            let call = ToolCall {
                id: format!("swarm-verify-{}", uuid::Uuid::new_v4()),
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name: "run_command".to_string(),
                    arguments: serde_json::json!({ "command": command }).to_string(),
                },
            };
            let decision = policy::evaluate_tool(
                &call.function.name,
                &call.function.arguments,
                &self.project_root,
                &policy_config,
            );
            match decision.action {
                policy::PolicyAction::Deny => {
                    failed = true;
                    let status_label = if use_chinese { "已阻止" } else { "blocked" };
                    lines.push(format!(
                        "- `{command}`: {} - {}",
                        status_label,
                        first_line(&decision.reason)
                    ));
                    continue;
                }
                policy::PolicyAction::AskOnce | policy::PolicyAction::AskSession
                    if !self.yolo_mode =>
                {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    self.emit_event(
                        event_tx,
                        Some(turn_id),
                        AgentEvent::ToolApprovalNeeded {
                            tool_name: call.function.name.clone(),
                            display: decision.display.clone(),
                            respond: tx,
                        },
                    );
                    let approved = matches!(
                        tokio::time::timeout(std::time::Duration::from_mins(5), rx).await,
                        Ok(Ok(true))
                    );
                    if !approved {
                        failed = true;
                        let status_label = if use_chinese { "未运行" } else { "skipped" };
                        let reason = if use_chinese {
                            "用户取消或确认超时"
                        } else {
                            "user denied or approval timed out"
                        };
                        lines.push(format!("- `{command}`: {status_label} - {reason}"));
                        continue;
                    }
                }
                _ => {}
            }
            self.record_event(
                Some(turn_id),
                SessionEventKind::ToolCallStarted {
                    tool_call_id: call.id.clone(),
                    name: call.function.name.clone(),
                    arguments: call.function.arguments.clone(),
                },
            );
            let execution = crate::tools::backend::ToolBackend::execute(
                &backend,
                &call,
                &crate::tools::backend::ToolExecutionContext {
                    project_root: self.project_root.clone(),
                    dispatch_config: crate::tools::dispatch::ToolDispatchConfig::from_policy(
                        &policy_config,
                    ),
                },
            )
            .await;
            self.record_event(
                Some(turn_id),
                SessionEventKind::ToolCallFinished {
                    tool_call_id: call.id,
                    name: call.function.name,
                    success: execution.success,
                    summary: execution.summary.clone(),
                    duration_ms: execution.duration_ms,
                    changed_files: execution.changed_files,
                },
            );
            failed |= !execution.success;
            let status_label = if use_chinese {
                if execution.success {
                    "通过"
                } else {
                    "失败"
                }
            } else if execution.success {
                "passed"
            } else {
                "failed"
            };
            lines.push(format!(
                "- `{command}`: {} - {}",
                status_label,
                first_line(&execution.summary)
            ));
        }
        (!lines.is_empty()).then(|| (lines.join("\n"), failed))
    }

    fn load_session_events(&self) -> Vec<SessionEvent> {
        self.event_log_store
            .as_ref()
            .and_then(|store| store.load(&self.project_root, &self.session.id).ok())
            .unwrap_or_default()
    }

    async fn maybe_auto_compact_context(
        &mut self,
        events: &[SessionEvent],
        threshold_tokens: u64,
        turn_id: TurnId,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> bool {
        if !should_auto_compact(&self.session, events, threshold_tokens) {
            return false;
        }

        // PreCompact lets users hook in (e.g. snapshot before compaction).
        self.run_lifecycle_hook(
            HookEvent::PreCompact,
            Some(turn_id),
            event_tx,
            Some("auto threshold".into()),
        )
        .await;

        let snapshot = build_compact_snapshot(
            &self.session,
            Some(events),
            DEFAULT_COMPACT_KEEP_MESSAGES,
            "auto threshold",
        );
        self.record_event(
            Some(turn_id),
            SessionEventKind::ContextCompacted {
                before_tokens: snapshot.before_tokens,
                after_tokens: snapshot.after_tokens,
                before_messages: snapshot.before_messages,
                after_messages: snapshot.after_messages,
                retained_start: snapshot.retained_start,
                retained_count: snapshot.retained_count,
                summary: snapshot.summary.clone(),
                reason: snapshot.reason.clone(),
            },
        );
        self.session.messages = retained_messages(&self.session, DEFAULT_COMPACT_KEEP_MESSAGES);
        send_event(
            event_tx,
            AgentEvent::ContextCompacted {
                summary: snapshot.summary,
                reason: snapshot.reason,
                before_tokens: snapshot.before_tokens,
                after_tokens: snapshot.after_tokens,
            },
        );
        true
    }

    /// Initialize MCP registry from config and connect to all configured servers.
    pub async fn init_mcp(&mut self, config: &crate::storage::config::McpConfig) {
        self.mcp_initialized = true;
        if !config.enabled {
            return;
        }
        let mut registry = crate::mcp::McpRegistry::with_project_root(self.project_root.clone());
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
        // Replace the static run_subagent definition with one that knows the
        // project's custom agents (and their descriptions), so the model can
        // delegate to them, not just the built-ins.
        let agent_registry =
            crate::agent::subagent::SubagentRegistry::load_from_project(&self.project_root);
        let mut roster: Vec<(String, String)> = agent_registry
            .list()
            .into_iter()
            .filter_map(|name| {
                agent_registry
                    .get(name)
                    .map(|config| (name.to_string(), config.effective_description()))
            })
            .collect();
        roster.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(slot) = tools
            .iter_mut()
            .find(|tool| tool.function.name == "run_subagent")
        {
            *slot = ds_tools::run_subagent_def_with_agents(&roster);
        }
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
        let result = if let Ok(result) = tokio::time::timeout(
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
            // Turn was forcibly stopped — fire Stop + Notification hooks
            // before the broader SessionEnd hook below.
            self.run_lifecycle_hook(HookEvent::Stop, None, &event_tx, None)
                .await;
            self.run_lifecycle_hook(
                HookEvent::Notification,
                None,
                &event_tx,
                Some("Turn timed out after 10 minutes".into()),
            )
            .await;
            Err(anyhow::anyhow!("Turn timed out after 10 minutes"))
        };
        self.run_lifecycle_hook(HookEvent::SessionEnd, None, &event_tx, None)
            .await;
        result
    }

    /// Run a turn forcing a specific lane (for `run` command).
    pub async fn run_turn_forced(
        &mut self,
        user_input: &str,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        lane: ExecutionLane,
    ) -> Result<(), anyhow::Error> {
        let timeout = std::time::Duration::from_mins(10);
        let result = if let Ok(result) = tokio::time::timeout(
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
            // Turn was forcibly stopped — fire Stop + Notification hooks
            // before the broader SessionEnd hook below.
            self.run_lifecycle_hook(HookEvent::Stop, None, &event_tx, None)
                .await;
            self.run_lifecycle_hook(
                HookEvent::Notification,
                None,
                &event_tx,
                Some("Turn timed out after 10 minutes".into()),
            )
            .await;
            Err(anyhow::anyhow!("Turn timed out after 10 minutes"))
        };
        self.run_lifecycle_hook(HookEvent::SessionEnd, None, &event_tx, None)
            .await;
        result
    }

    async fn run_turn_inner(
        &mut self,
        user_input: &str,
        images: &[String],
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        force_lane: Option<ExecutionLane>,
    ) -> Result<(), anyhow::Error> {
        self.current_turn_allowed_tools = self.pending_allowed_tools.take();
        // Fresh config snapshot for this turn — edits to
        // `.octocode/config.toml` since the last turn take effect now.
        self.cached_turn_config = None;
        let result = self
            .run_turn_inner_body(user_input, images, event_tx, force_lane)
            .await;
        self.current_turn_allowed_tools = None;
        self.cached_turn_config = None;
        self.session.reasoning_state.auto_tier_model = None;
        result
    }

    async fn run_turn_inner_body(
        &mut self,
        user_input: &str,
        images: &[String],
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        force_lane: Option<ExecutionLane>,
    ) -> Result<(), anyhow::Error> {
        let defense = crate::defense::DefenseProtocol::default();
        let sanitized_user_input = defense.sanitize_input(user_input);
        let user_input = sanitized_user_input.safe_text();
        if !self.lifecycle_hooks_started {
            self.run_lifecycle_hook(HookEvent::SessionStart, None, event_tx, None)
                .await;
            self.lifecycle_hooks_started = true;
        }
        self.run_lifecycle_hook(
            HookEvent::UserPromptSubmit,
            None,
            event_tx,
            Some(user_input.to_string()),
        )
        .await;
        let turn_input = ContextualTurnInput::from_session(&self.session, user_input);
        let routing_input = turn_input.routing_input();

        // Load project rules once and reuse throughout the turn. We also
        // pull any skill bodies whose `keywords:` frontmatter matches the
        // user's input and append them to the rules — auto-injection of
        // saved workflows. Project skills are tried first; if there's room
        // under the cap, user-global skills under `~/.octocode/skills`
        // fill the rest, matching the discovery surface of `octo commands`.
        let mut project_rules = load_project_rules(&self.project_root);
        let mut skill_hits: Vec<(String, String)> = Vec::new();
        let project_store = crate::skill::SkillStore::for_project(&self.project_root);
        if let Ok(hits) = project_store.triggered_for_input(user_input, 3) {
            skill_hits.extend(hits);
        }
        if skill_hits.len() < 3 {
            if let Some(home) = crate::storage::user_home_dir() {
                let user_store = crate::skill::SkillStore::for_project(home);
                let remaining = 3 - skill_hits.len();
                if let Ok(hits) = user_store.triggered_for_input(user_input, remaining) {
                    for (id, body) in hits {
                        if !skill_hits.iter().any(|(existing, _)| existing == &id) {
                            skill_hits.push((id, body));
                        }
                    }
                }
            }
        }
        if !skill_hits.is_empty() {
            let mut combined = project_rules.unwrap_or_default();
            for (id, body) in skill_hits {
                if !combined.is_empty() {
                    combined.push_str("\n\n");
                }
                combined.push_str(&format!("### Triggered skill: {id}\n\n{body}"));
            }
            project_rules = Some(combined);
        }
        let force_swarm = should_force_swarm(user_input) || should_force_swarm(routing_input);

        // 1. Legacy classify (for search / explicit triggers)
        let task = classify_task(routing_input);
        let has_forced_lane = force_lane.is_some();

        // 2. Complexity router assessment (for all non-forced plan modes)
        let (assessment, shadow_mode) = if has_forced_lane || task == TaskClass::Plan {
            // Respect explicit user intent
            (None, false)
        } else {
            let router_config = self.turn_config().router.clone();
            let shadow_mode = router_config.shadow_mode;
            let router = if router_config.enabled {
                ComplexityRouter::from(&router_config)
            } else {
                ComplexityRouter::new().rules_only()
            };
            let assessment = router
                .assess(routing_input, project_rules.as_deref(), Some(&self.client))
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

        // 2b. Auto model tiering (auto mode only): keep complex/plan work on the
        // effort-based default (Pro), but downgrade clearly-lightweight direct
        // tasks to Flash to save cost. An explicit pin always wins; cleared at
        // turn end in `run_turn_inner`.
        self.session.reasoning_state.auto_tier_model = auto_tier_model_for(
            self.session.reasoning_state.selected_model.is_some(),
            assessment.as_ref(),
        );

        // 3. Route decision
        if let Some(ref a) = assessment {
            let route = if shadow_mode || force_swarm {
                // In shadow mode the router still assesses and emits telemetry,
                // but never overrides the actual execution path.
                Route::DirectExecute
            } else {
                a.route.clone()
            };
            match route {
                Route::Clarify => {
                    let questions = generate_clarification_questions(routing_input, a);
                    send_event(event_tx, AgentEvent::ClarificationNeeded { questions });
                    return Ok(());
                }
                Route::PlanReview => {
                    // Override into plan mode even if not explicitly requested
                    let search_ctx = self.run_search_phase(routing_input, event_tx).await;
                    return self
                        .run_plan_mode(routing_input, user_input, &search_ctx, event_tx, None)
                        .await;
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
        let event_user_content = enriched_input.clone();
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
        self.emit_event(
            event_tx,
            Some(turn_id),
            AgentEvent::UserMessage {
                content: event_user_content,
            },
        );

        let runtime_config = self.turn_config().clone();
        let budget = crate::provider::context_budget_for(
            runtime_config.provider.default,
            &self.session.reasoning_state.effective_model(),
            runtime_config.search.max_context_tokens,
        );
        let session_events_for_compact = self.load_session_events();
        self.maybe_auto_compact_context(
            &session_events_for_compact,
            budget.auto_compact_threshold_tokens,
            turn_id,
            event_tx,
        )
        .await;

        if self.should_run_swarm(routing_input, assessment.as_ref(), force_swarm) {
            return self
                .run_swarm_mode(
                    turn_input.task_description(),
                    routing_input,
                    event_tx,
                    turn_id,
                )
                .await;
        }

        // 6. Search if needed
        let search_ctx = if matches!(task, TaskClass::Search | TaskClass::Plan) {
            self.run_search_phase(routing_input, event_tx).await
        } else {
            None
        };

        // 7. Plan mode (explicit / router-overridden already handled above)
        if task == TaskClass::Plan {
            return self
                .run_plan_mode(
                    routing_input,
                    user_input,
                    &search_ctx,
                    event_tx,
                    Some(turn_id),
                )
                .await;
        }

        // Determine execution lane
        let lane = force_lane.unwrap_or_else(|| resolve_lane(&task, assessment.as_ref()));

        // 8. Build prompt (project_rules loaded once at top of run_turn_inner)
        let cap = ModelCapability::for_model(&self.session.reasoning_state.effective_model());
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

        let session_events = self.load_session_events();
        let (_, messages) = PromptBuilder::new(cap.model.clone(), lane.clone(), true)
            .build_with_events_and_context(
                &self.session,
                Some(&session_events),
                project_rules.as_deref(),
                search_ctx.as_deref(),
                &effective_tools,
                turn_input.transient_context(),
            );

        let request = ChatRequest {
            model: cap.model.to_string(),
            temperature: temperature_for_lane(&lane),
            messages,
            tools: if send_tools {
                Some(effective_tools)
            } else {
                None
            },
            thinking: thinking_config,
            response_format: None,
            stream: true,
            max_tokens: Some(TURN_MAX_OUTPUT_TOKENS),
        };

        // 9. Stream and process
        send_request_token_delta(event_tx, &request);
        let mut emitted = EmittedStreamDeltas::default();
        let result = self
            .stream_client()
            .stream_chat(&request, &mut |chunk| {
                emitted.merge(emit_stream_chunk_deltas(event_tx, chunk));
            })
            .await;
        match result {
            Ok(stream_result) => {
                if stream_result.tool_calls.is_empty() {
                    // GUARANTEE: every turn ends with a visible message. Per
                    // `docs/provider_eval_design.md`, **reasoning_content must
                    // not be rendered as user text** — it stays internal.
                    // So we have two cases:
                    //   1. content non-empty → show it as-is.
                    //   2. content empty (with or without reasoning) → emit a
                    //      placeholder. We diagnose whether the model burned its
                    //      output budget on reasoning so the user knows what to do.
                    let content_empty = stream_result.content.trim().is_empty();
                    let visible_content = if content_empty {
                        empty_content_placeholder(&stream_result)
                    } else {
                        stream_result.content.clone()
                    };
                    let used_placeholder = content_empty;
                    let msg = ReasoningManager::new_assistant_message(
                        &visible_content,
                        (!stream_result.reasoning_content.is_empty())
                            .then_some(&stream_result.reasoning_content),
                        &[],
                        turn_id,
                        None,
                        false,
                    );
                    self.session.messages.push(msg);
                    if !stream_result.reasoning_content.trim().is_empty() {
                        self.record_event(
                            Some(turn_id),
                            SessionEventKind::ReasoningInternal {
                                content: stream_result.reasoning_content.clone(),
                            },
                        );
                    }
                    if !visible_content.trim().is_empty() {
                        self.record_event(
                            Some(turn_id),
                            SessionEventKind::AssistantVisible {
                                content: visible_content.clone(),
                            },
                        );
                    }
                    if !emitted.reasoning {
                        send_reasoning_delta(event_tx, &stream_result.reasoning_content);
                    }
                    if !emitted.content || used_placeholder {
                        send_event(event_tx, AgentEvent::ContentDelta(visible_content.clone()));
                    }
                    if let Some(ref usage) = stream_result.usage {
                        let cache = self.accrue_usage(usage);
                        send_event(
                            event_tx,
                            AgentEvent::StreamDone {
                                finish_reason: stream_result
                                    .finish_reason
                                    .clone()
                                    .map(FinishReason::from),
                                usage: Some(usage.clone()),
                                cache: Some(cache),
                            },
                        );
                    }
                } else {
                    if !stream_result.reasoning_content.trim().is_empty() {
                        self.record_event(
                            Some(turn_id),
                            SessionEventKind::ReasoningInternal {
                                content: stream_result.reasoning_content.clone(),
                            },
                        );
                    }
                    if !stream_result.content.trim().is_empty() {
                        self.record_event(
                            Some(turn_id),
                            SessionEventKind::AssistantInternal {
                                content: stream_result.content.clone(),
                            },
                        );
                    }
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
                self.emit_event(
                    event_tx,
                    Some(turn_id),
                    AgentEvent::TurnComplete {
                        session_id: self.session.id,
                        total_tokens: self.session.metadata.total_tokens,
                    },
                );
            }
            Err(e) => {
                self.emit_event(event_tx, Some(turn_id), AgentEvent::Error(e.to_string()));
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
        planner_input: &str,
        visible_user_input: &str,
        search_ctx: &Option<String>,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        existing_turn_id: Option<TurnId>,
    ) -> Result<(), anyhow::Error> {
        let turn_id = existing_turn_id.unwrap_or_else(TurnId::new_v4);
        if existing_turn_id.is_none() {
            crate::workspace::apply::set_current_turn_id(&turn_id.to_string());
            ReasoningManager::begin_user_turn(
                &mut self.session.reasoning_state,
                &mut self.session.messages,
                turn_id,
            );
            self.session.messages.push(ProtocolMessage {
                id: MessageId::new_v4(),
                role: Role::User,
                content: MessageContent::from(visible_user_input),
                reasoning_content: None,
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                turn_id,
                sub_turn_id: None,
                visibility: MessageVisibility::UserVisible,
            });
            self.emit_event(
                event_tx,
                Some(turn_id),
                AgentEvent::UserMessage {
                    content: visible_user_input.to_string(),
                },
            );
        }

        let plan = plan::generate_plan(
            &self.client,
            &DeepSeekModel::Pro,
            planner_input,
            search_ctx.as_deref(),
            load_project_rules(&self.project_root).as_deref(),
        )
        .await;
        match plan {
            Ok(p) => {
                if let Err(errors) = p.validate() {
                    for e in &errors {
                        self.emit_event(event_tx, Some(turn_id), AgentEvent::Error(e.clone()));
                    }
                    self.emit_event(
                        event_tx,
                        Some(turn_id),
                        AgentEvent::TurnComplete {
                            session_id: self.session.id,
                            total_tokens: self.session.metadata.total_tokens,
                        },
                    );
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

                let use_chinese = plan_or_input_uses_chinese(&p, planner_input)
                    || contains_cjk(visible_user_input);
                let original_yolo_mode = self.yolo_mode;
                let mut scoped_plan_auto = false;

                // Convert plan to concrete steps and emit to TUI tracker
                let steps = plan::executor::plan_to_steps(&p);
                let step_lines = steps
                    .iter()
                    .map(|step| step.display_with_language(use_chinese))
                    .collect::<Vec<_>>();
                self.write_artifact(
                    "execution-plan",
                    &format_plan_artifact(
                        planner_input,
                        &p,
                        &step_lines,
                        &review.warnings,
                        use_chinese,
                    ),
                );
                self.emit_event(
                    event_tx,
                    Some(turn_id),
                    AgentEvent::PlanStarted {
                        summary: p.summary.clone(),
                        total: steps.len(),
                    },
                );
                for (i, description) in step_lines.iter().cloned().enumerate().take(steps.len()) {
                    self.emit_event(
                        event_tx,
                        Some(turn_id),
                        AgentEvent::PlanStepUpdate {
                            index: i,
                            total: steps.len(),
                            description,
                            status: PlanStepStatus::Pending,
                        },
                    );
                }

                // Present execution-mode options to the user (unless yolo mode)
                if !self.yolo_mode {
                    let options = generate_plan_options(&p, use_chinese);
                    let (opts_tx, opts_rx) = tokio::sync::oneshot::channel();
                    send_event(
                        event_tx,
                        AgentEvent::OptionsNeeded {
                            kind: DecisionKind::PlanAction,
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
                            self.emit_event(
                                event_tx,
                                Some(turn_id),
                                AgentEvent::Error("Options channel closed".into()),
                            );
                            self.emit_event(event_tx, Some(turn_id), AgentEvent::PlanCleared);
                            self.session.updated_at = Utc::now();
                            self.emit_event(
                                event_tx,
                                Some(turn_id),
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
                        self.emit_event(
                            event_tx,
                            Some(turn_id),
                            AgentEvent::ContentDelta(
                                if use_chinese {
                                    "计划已取消。\n"
                                } else {
                                    "Plan cancelled by user.\n"
                                }
                                .into(),
                            ),
                        );
                        self.emit_event(event_tx, Some(turn_id), AgentEvent::PlanCleared);
                        self.session.updated_at = Utc::now();
                        self.emit_event(
                            event_tx,
                            Some(turn_id),
                            AgentEvent::TurnComplete {
                                session_id: self.session.id,
                                total_tokens: 0,
                            },
                        );
                        return Ok(());
                    }
                    match options[choice].mode {
                        PlanExecutionMode::Auto => {
                            scoped_plan_auto = true;
                            self.yolo_mode = true;
                        }
                        PlanExecutionMode::Confirm => { /* normal mode */ }
                        PlanExecutionMode::Preview => {
                            self.emit_event(
                                event_tx,
                                Some(turn_id),
                                AgentEvent::ContentDelta(
                                    if use_chinese {
                                        "已选择预览模式 - 只显示计划，不执行。\n"
                                    } else {
                                        "Preview mode selected — plan shown but not executed.\n"
                                    }
                                    .into(),
                                ),
                            );
                            self.emit_event(event_tx, Some(turn_id), AgentEvent::PlanCleared);
                            self.session.updated_at = Utc::now();
                            self.emit_event(
                                event_tx,
                                Some(turn_id),
                                AgentEvent::TurnComplete {
                                    session_id: self.session.id,
                                    total_tokens: 0,
                                },
                            );
                            return Ok(());
                        }
                        PlanExecutionMode::Cancel => {
                            self.emit_event(
                                event_tx,
                                Some(turn_id),
                                AgentEvent::ContentDelta(
                                    if use_chinese {
                                        "计划已取消。\n"
                                    } else {
                                        "Plan cancelled by user.\n"
                                    }
                                    .into(),
                                ),
                            );
                            self.emit_event(event_tx, Some(turn_id), AgentEvent::PlanCleared);
                            self.session.updated_at = Utc::now();
                            self.emit_event(
                                event_tx,
                                Some(turn_id),
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
                    self.emit_event(
                        event_tx,
                        Some(turn_id),
                        AgentEvent::PlanStepUpdate {
                            index: update.index,
                            total: update.total,
                            description: update.description,
                            status: update.status,
                        },
                    );
                }
                self.plan_execution = Some(plan_state);

                let plan_json = serde_json::to_string_pretty(&p).unwrap_or_default();
                crate::workspace::apply::set_current_turn_id(&turn_id.to_string());
                self.session.messages.push(ProtocolMessage {
                    id: MessageId::new_v4(),
                    role: Role::Assistant,
                    content: MessageContent::from(plan_json.clone()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                    turn_id,
                    sub_turn_id: None,
                    visibility: MessageVisibility::AuditOnly,
                });

                // Add execution instruction as a synthetic user turn. Plan-mode prose is
                // rendered by the tracker, so model narration stays internal.
                let execution_prompt = plan_execution_prompt(use_chinese);
                let plan_context = plan_execution_context(&plan_json, execution_prompt);
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
                let cap =
                    ModelCapability::for_model(&self.session.reasoning_state.effective_model());
                let thinking_config = thinking_config_for_lane(
                    &ExecutionLane::ToolLoopThinking,
                    &self.session.reasoning_state.mode,
                    &self.session.reasoning_state.effort,
                );

                let session_events = self.load_session_events();
                let (_, messages) =
                    PromptBuilder::new(cap.model.clone(), ExecutionLane::ToolLoopThinking, true)
                        .build_with_events_and_context(
                            &self.session,
                            Some(&session_events),
                            load_project_rules(&self.project_root).as_deref(),
                            search_ctx.as_deref(),
                            &tool_defs,
                            &plan_context,
                        );

                let request = ChatRequest {
                    model: cap.model.to_string(),
                    temperature: Some(0.0),
                    messages,
                    tools: Some(tool_defs),
                    thinking: thinking_config,
                    response_format: None,
                    stream: true,
                    max_tokens: Some(TURN_MAX_OUTPUT_TOKENS),
                };

                let mut plan_execution_failed = false;
                send_request_token_delta(event_tx, &request);
                match self
                    .stream_client()
                    .stream_chat(&request, &mut |_| {})
                    .await
                {
                    Ok(stream_result) => {
                        if stream_result.tool_calls.is_empty() {
                            plan_execution_failed = true;
                            let mut msg = ReasoningManager::new_assistant_message(
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
                            msg.visibility = MessageVisibility::AuditOnly;
                            self.session.messages.push(msg);
                            self.emit_event(
                                event_tx,
                                Some(turn_id),
                                AgentEvent::Error(
                                    "Plan execution produced no tool calls; no steps were executed"
                                        .into(),
                                ),
                            );
                            if let Some(ref usage) = stream_result.usage {
                                let cache = self.accrue_usage(usage);
                                send_event(
                                    event_tx,
                                    AgentEvent::StreamDone {
                                        finish_reason: stream_result
                                            .finish_reason
                                            .clone()
                                            .map(FinishReason::from),
                                        usage: Some(usage.clone()),
                                        cache: Some(cache),
                                    },
                                );
                            }
                        } else {
                            if let Err(e) = self
                                .handle_tool_calls(&stream_result, turn_id, event_tx, 0)
                                .await
                            {
                                plan_execution_failed = true;
                                self.emit_event(
                                    event_tx,
                                    Some(turn_id),
                                    AgentEvent::Error(format!("Plan tool execution failed: {e}")),
                                );
                            }
                        }
                        self.session.updated_at = Utc::now();
                        self.emit_event(
                            event_tx,
                            Some(turn_id),
                            AgentEvent::TurnComplete {
                                session_id: self.session.id,
                                total_tokens: self.session.metadata.total_tokens,
                            },
                        );
                    }
                    Err(e) => {
                        plan_execution_failed = true;
                        self.emit_event(
                            event_tx,
                            Some(turn_id),
                            AgentEvent::Error(format!("Plan execution failed: {e}")),
                        );
                        self.emit_event(
                            event_tx,
                            Some(turn_id),
                            AgentEvent::TurnComplete {
                                session_id: self.session.id,
                                total_tokens: self.session.metadata.total_tokens,
                            },
                        );
                    }
                }
                if scoped_plan_auto {
                    self.yolo_mode = original_yolo_mode;
                }

                // Finish any steps that did not map cleanly to a tool batch before clearing UI state.
                if let Some(mut plan_execution) = self.plan_execution.take() {
                    let updates = plan_execution.finish_remaining(!plan_execution_failed);
                    for update in updates {
                        self.emit_event(
                            event_tx,
                            Some(turn_id),
                            AgentEvent::PlanStepUpdate {
                                index: update.index,
                                total: update.total,
                                description: update.description,
                                status: update.status,
                            },
                        );
                    }
                }
                self.emit_event(event_tx, Some(turn_id), AgentEvent::PlanCleared);
            }
            Err(e) => {
                self.emit_event(
                    event_tx,
                    Some(turn_id),
                    AgentEvent::Error(format!("Plan generation failed: {e}")),
                );
                self.emit_event(
                    event_tx,
                    Some(turn_id),
                    AgentEvent::TurnComplete {
                        session_id: self.session.id,
                        total_tokens: self.session.metadata.total_tokens,
                    },
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
        // Count this tool-emitting response's tokens now. The turn continues
        // with a follow-up call below, so without recording here the usage of
        // every tool-emitting call is dropped (only the final no-tool response
        // was previously counted).
        if let Some(ref usage) = stream_result.usage {
            let _ = self.accrue_usage(usage);
        }
        send_reasoning_delta(event_tx, &stream_result.reasoning_content);
        for tc in &stream_result.tool_calls {
            self.emit_event(
                event_tx,
                Some(turn_id),
                AgentEvent::ToolStarted {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                },
            );
        }
        if let Some(last_msg) = self.session.messages.last() {
            ReasoningManager::begin_tool_turn(&mut self.session.reasoning_state, last_msg);
        }

        let runtime_config = self.turn_config().clone();
        let mut policy_config = runtime_config.policy.clone();
        self.apply_active_allowed_tools(&mut policy_config);
        let hooks_config = runtime_config.hooks.clone();

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
        let mut results: Vec<ToolLoopResult> = Vec::new();
        if !subagent_calls.is_empty() {
            let handler = TaskToolHandler::new(
                Arc::new(self.client.clone()),
                self.project_root.clone(),
                self.background_queue.clone(),
                0, // top-level: depth 0
            );
            for tc in subagent_calls {
                let decision = policy::evaluate_tool(
                    &tc.function.name,
                    &tc.function.arguments,
                    &self.project_root,
                    &policy_config,
                );
                let mode_outcome = if matches!(decision.action, policy::PolicyAction::Deny) {
                    None
                } else {
                    crate::agent::approval::permission_mode_approval_outcome(
                        &tc.function.name,
                        self.yolo_mode,
                        self.permission_mode,
                        &decision,
                    )
                };
                let approved = if let Some(outcome) = mode_outcome {
                    if outcome.approved() {
                        true
                    } else {
                        let result_text = outcome
                            .reason()
                            .unwrap_or("Denied by permission mode")
                            .to_string();
                        let record = ToolResultRecord {
                            tool_call_id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            result: result_text.clone(),
                            is_error: true,
                        };
                        self.session
                            .tool_call_history
                            .push(crate::deepseek::ToolCallRecord {
                                id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                arguments: tc.function.arguments.clone(),
                                result_summary: crate::agent::utils::truncate_for_summary(
                                    &result_text,
                                    200,
                                ),
                                exit_code: Some(1),
                                duration_ms: 0,
                                risk_level: decision.display.risk_level.to_string(),
                                approved: false,
                                at: Utc::now(),
                            });
                        results.push(ToolLoopResult::new(tc.clone(), record, 0, Vec::new()));
                        continue;
                    }
                } else {
                    match decision.action {
                        policy::PolicyAction::Allow => true,
                        policy::PolicyAction::Deny => {
                            let result_text = format!("Blocked: {}", decision.reason);
                            let record = ToolResultRecord {
                                tool_call_id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                result: result_text.clone(),
                                is_error: true,
                            };
                            self.session
                                .tool_call_history
                                .push(crate::deepseek::ToolCallRecord {
                                    id: tc.id.clone(),
                                    name: tc.function.name.clone(),
                                    arguments: tc.function.arguments.clone(),
                                    result_summary: crate::agent::utils::truncate_for_summary(
                                        &result_text,
                                        200,
                                    ),
                                    exit_code: Some(1),
                                    duration_ms: 0,
                                    risk_level: decision.display.risk_level.to_string(),
                                    approved: false,
                                    at: Utc::now(),
                                });
                            results.push(ToolLoopResult::new(tc.clone(), record, 0, Vec::new()));
                            continue;
                        }
                        policy::PolicyAction::AskOnce | policy::PolicyAction::AskSession => {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            send_event(
                                event_tx,
                                AgentEvent::ToolApprovalNeeded {
                                    tool_name: tc.function.name.clone(),
                                    display: decision.display.clone(),
                                    respond: tx,
                                },
                            );
                            matches!(
                                tokio::time::timeout(std::time::Duration::from_mins(1), rx).await,
                                Ok(Ok(true))
                            )
                        }
                    }
                };

                if !approved {
                    let result_text = "Denied by user or timeout".to_string();
                    let record = ToolResultRecord {
                        tool_call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        result: result_text.clone(),
                        is_error: true,
                    };
                    self.session
                        .tool_call_history
                        .push(crate::deepseek::ToolCallRecord {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                            result_summary: result_text.clone(),
                            exit_code: Some(1),
                            duration_ms: 0,
                            risk_level: decision.display.risk_level.to_string(),
                            approved: false,
                            at: Utc::now(),
                        });
                    results.push(ToolLoopResult::new(tc.clone(), record, 0, Vec::new()));
                    continue;
                }

                let start = std::time::Instant::now();
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
                let duration_ms = start.elapsed().as_millis() as u64;
                let record = ToolResultRecord {
                    tool_call_id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    result: result_text,
                    is_error,
                };
                self.session
                    .tool_call_history
                    .push(crate::deepseek::ToolCallRecord {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                        result_summary: crate::agent::utils::truncate_for_summary(
                            &record.result,
                            200,
                        ),
                        exit_code: if is_error { Some(1) } else { Some(0) },
                        duration_ms,
                        risk_level: decision.display.risk_level.to_string(),
                        approved: true,
                        at: Utc::now(),
                    });
                results.push(ToolLoopResult::new(
                    tc.clone(),
                    record,
                    duration_ms,
                    Vec::new(),
                ));
            }
        }

        // Separate MCP tools from standard tools
        let mut mcp_calls = Vec::new();
        let mut standard_calls = Vec::new();
        for tc in regular_calls {
            if self
                .mcp_registry
                .as_ref()
                .is_some_and(|registry| registry.is_mcp_tool_name(&tc.function.name))
            {
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
                results.push(ToolLoopResult::new(tc.clone(), record, 0, Vec::new()));
            }
        }

        // Execute standard tools
        if !deduped.is_empty() {
            let regular_results = ToolLoop::execute_tools_with_approval(
                &deduped,
                &self.project_root,
                turn_id,
                sub_turn_id,
                &mut self.session,
                event_tx,
                self.yolo_mode,
                self.permission_mode,
                &policy_config,
                &hooks_config,
                self.event_log_store.clone(),
            )
            .await;
            results.extend(regular_results);
        }

        // Execute MCP tools
        if !mcp_calls.is_empty() {
            let event_log_store = self.event_log_store.clone();
            let project_root = self.project_root.clone();
            let session_id = self.session.id;
            let runtime = ToolRuntime::new(project_root.clone(), policy_config.clone());
            let dispatch_config =
                crate::tools::dispatch::ToolDispatchConfig::from_policy(&policy_config);
            if let Some(ref mut registry) = self.mcp_registry {
                for tc in mcp_calls {
                    let metadata = registry.tool_approval_metadata(&tc.function.name);
                    let mut context =
                        ToolRuntimeContext::new(session_id.to_string(), dispatch_config.clone());
                    context.session_id = Some(session_id);
                    context.turn_id = Some(turn_id);
                    context.hooks_config = Some(&hooks_config);
                    context.mcp_metadata = metadata.as_ref();
                    let mut resolver = OrchestratorApprovalResolver {
                        event_tx,
                        yolo_mode: self.yolo_mode,
                        permission_mode: self.permission_mode,
                    };
                    let mut backend = McpRegistryRuntimeBackend { registry };
                    let outcome = runtime
                        .execute(
                            &tc,
                            policy::ToolCallSource::Mcp,
                            context,
                            &mut resolver,
                            &mut backend,
                        )
                        .await;
                    for hook_summary in &outcome.hook_summaries {
                        emit_hook_summary_event(
                            event_tx,
                            &event_log_store,
                            &project_root,
                            session_id,
                            Some(turn_id),
                            hook_summary,
                        );
                    }

                    let record = crate::deepseek::ToolCallRecord {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                        result_summary: outcome.backend_result.summary.clone(),
                        exit_code: if outcome.result_record.is_error {
                            Some(1)
                        } else {
                            Some(0)
                        },
                        duration_ms: outcome.backend_result.duration_ms,
                        risk_level: outcome.decision.display.risk_level.to_string(),
                        approved: outcome.approval.approved(),
                        at: Utc::now(),
                    };
                    self.session.tool_call_history.push(record);

                    results.push(ToolLoopResult::new(
                        tc.clone(),
                        outcome.result_record,
                        outcome.backend_result.duration_ms,
                        outcome.backend_result.changed_files,
                    ));
                }
            } else {
                for tc in mcp_calls {
                    let record = ToolResultRecord {
                        tool_call_id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        result: "MCP registry not initialized".into(),
                        is_error: true,
                    };
                    results.push(ToolLoopResult::new(tc.clone(), record, 0, Vec::new()));
                }
            }
        }

        for tool_result in &results {
            let tc = &tool_result.call;
            let result = &tool_result.result;
            let changed_files = event_changed_files_for_tool_result(tool_result);
            let pending_question = (!result.is_error)
                .then(|| question_prompt_for_tool_call(tc))
                .flatten();
            self.record_event(
                Some(turn_id),
                SessionEventKind::ToolCallFinished {
                    tool_call_id: result.tool_call_id.clone(),
                    name: result.name.clone(),
                    success: !result.is_error,
                    summary: crate::agent::utils::truncate_for_summary(&result.result, 200),
                    duration_ms: tool_result.duration_ms,
                    changed_files: changed_files.clone(),
                },
            );
            if let Some(question) = &pending_question {
                self.record_event(
                    Some(turn_id),
                    SessionEventKind::UserQuestionRequested {
                        tool_call_id: result.tool_call_id.clone(),
                        name: result.name.clone(),
                        title: question.title.clone(),
                        options: question.options.clone(),
                        summary: question.summary.clone(),
                        descriptions: question.descriptions.clone(),
                        previews: question.previews.clone(),
                        multi_select: question.multi_select,
                    },
                );
            }
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
            if let Some(question) = pending_question {
                send_event(
                    event_tx,
                    AgentEvent::UserQuestionRequested {
                        title: question.title,
                        options: question.options,
                        summary: question.summary,
                        descriptions: question.descriptions,
                        previews: question.previews,
                        multi_select: question.multi_select,
                    },
                );
            }

            // Emit diff event for file-editing tools so the TUI can show an inline preview
            if (tc.function.name == "edit_file" || tc.function.name == "write_file")
                && !result.is_error
            {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                {
                    if let Some(path) = args["path"].as_str() {
                        let stats = crate::workspace::diff::diff_stats(&result.result);
                        self.emit_event(
                            event_tx,
                            Some(turn_id),
                            AgentEvent::FileDiff {
                                path: path.to_string(),
                                diff: result.result.clone(),
                                stats: stats.to_string(),
                            },
                        );
                    }
                }
            } else if !result.is_error {
                for path in &changed_files {
                    self.record_event(
                        Some(turn_id),
                        SessionEventKind::FileChanged {
                            path: path.clone(),
                            stats: format!("changed by {}", tc.function.name),
                        },
                    );
                }
            }
        }

        // Self-verification after file edits
        let had_edits = results.iter().any(|tool_result| {
            !tool_result.result.is_error
                && (tool_result.call.function.name == "edit_file"
                    || tool_result.call.function.name == "write_file"
                    || tool_result.call.function.name == "notebook_edit"
                    || tool_result.call.function.name == "apply_patch")
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
            let had_error = results
                .iter()
                .any(|tool_result| tool_result.result.is_error);
            let updates = plan_exec.updates_after_tool_batch(had_error);
            for update in updates {
                self.emit_event(
                    event_tx,
                    Some(turn_id),
                    AgentEvent::PlanStepUpdate {
                        index: update.index,
                        total: update.total,
                        description: update.description,
                        status: update.status,
                    },
                );
            }
        }

        let tool_defs = self.get_all_tools();
        let project_rules = load_project_rules(&self.project_root);
        let session_events = self.load_session_events();
        let (_, messages) = PromptBuilder::new(
            self.session.reasoning_state.effective_model(),
            ExecutionLane::ToolLoopThinking,
            true,
        )
        .build_with_events(
            &self.session,
            Some(&session_events),
            project_rules.as_deref(),
            None,
            &tool_defs,
        );

        let followup_request = ChatRequest {
            model: self.session.reasoning_state.effective_model().to_string(),
            temperature: Some(0.0),
            messages,
            tools: Some(tool_defs),
            thinking: Some(ThinkingConfig::enabled()),
            response_format: None,
            stream: true,
            max_tokens: Some(TURN_MAX_OUTPUT_TOKENS),
        };

        let suppress_visible_content = self.plan_execution.is_some();
        let mut emitted = EmittedStreamDeltas::default();
        send_request_token_delta(event_tx, &followup_request);
        match self
            .stream_client()
            .stream_chat(&followup_request, &mut |chunk| {
                emitted.merge(emit_stream_chunk_deltas_with_options(
                    event_tx,
                    chunk,
                    !suppress_visible_content,
                ));
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
                        let error = "Tool call recursion limit reached for this turn".to_string();
                        send_event(event_tx, AgentEvent::Error(error.clone()));
                        return Err(anyhow::anyhow!(error));
                    }
                    ReasoningManager::complete_tool_loop(
                        &mut self.session.reasoning_state,
                        &mut self.session.messages,
                    );
                    // Same placeholder fallback as the no-tool branch above.
                    // Reasoning stays internal per `docs/provider_eval_design.md`;
                    // when content is empty we surface a placeholder instead of
                    // leaking the chain-of-thought.
                    let content_empty = followup_result.content.trim().is_empty();
                    let visible_content = if content_empty {
                        empty_content_placeholder(&followup_result)
                    } else {
                        followup_result.content.clone()
                    };
                    let used_placeholder = content_empty;
                    let mut final_msg = ReasoningManager::new_assistant_message(
                        &visible_content,
                        (!followup_result.reasoning_content.is_empty())
                            .then_some(&followup_result.reasoning_content),
                        &[],
                        turn_id,
                        None,
                        false,
                    );
                    if suppress_visible_content {
                        final_msg.visibility = MessageVisibility::AuditOnly;
                    }
                    self.session.messages.push(final_msg);
                    if !emitted.reasoning {
                        send_reasoning_delta(event_tx, &followup_result.reasoning_content);
                    }
                    if (!emitted.content || used_placeholder) && !suppress_visible_content {
                        send_event(event_tx, AgentEvent::ContentDelta(visible_content));
                    }
                    if let Some(ref usage) = followup_result.usage {
                        let cache = self.accrue_usage(usage);
                        send_event(
                            event_tx,
                            AgentEvent::StreamDone {
                                finish_reason: None,
                                usage: Some(usage.clone()),
                                cache: Some(cache),
                            },
                        );
                    }
                }
            }
            Err(e) => {
                let error = format!("Tool loop failed: {e}");
                send_event(event_tx, AgentEvent::Error(error.clone()));
                return Err(anyhow::anyhow!(error));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Empty-content placeholder
// ---------------------------------------------------------------------------

/// Returned when a stream finishes with no visible content. Diagnoses the
/// likely cause from the usage block so the user knows what to do next.
/// Per `docs/provider_eval_design.md`, reasoning_content is NEVER included
/// in this message — it stays internal.
fn empty_content_placeholder(stream_result: &StreamResult) -> String {
    if let Some(ref usage) = stream_result.usage {
        // If completion_tokens is at/near max_tokens, the model almost
        // certainly ran out of room mid-reasoning before writing an answer.
        let used = usage.completion_tokens;
        let near_budget = used >= TURN_MAX_OUTPUT_TOKENS.saturating_sub(256);
        if near_budget {
            return format!(
                "_(模型本轮的输出预算被思考过程耗尽（completion_tokens={used}）。\
                 试着把问题拆小一些，或换 Flash 模型再问 / model exhausted its \
                 output budget on reasoning before writing an answer — try a \
                 narrower question or the Flash model.)_"
            );
        }
    }
    "_(模型本轮未返回任何内容，请换个说法重试 / model returned no content this turn — try rephrasing and asking again.)_"
        .to_string()
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

struct OrchestratorApprovalResolver<'a> {
    event_tx: &'a mpsc::UnboundedSender<AgentEvent>,
    yolo_mode: bool,
    /// PermissionMode-driven auto-approval (e.g. AcceptEdits auto-approves
    /// edit tools; Auto/Bypass auto-approve everything). Layers on top of
    /// the legacy `yolo_mode` flag so both can coexist during the
    /// transition: Bypass / Auto already imply yolo, the new modes
    /// (AcceptEdits / Plan / ReadOnly) refine policy without bypassing it.
    permission_mode: policy::PermissionMode,
}

impl ApprovalResolver for OrchestratorApprovalResolver<'_> {
    fn resolve<'a>(
        &'a mut self,
        call: &'a crate::runtime::tool_runtime::ToolCall,
        decision: &'a policy::PolicyDecision,
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
            send_event(
                self.event_tx,
                AgentEvent::ToolApprovalNeeded {
                    tool_name: call.tool.clone(),
                    display: decision.display.clone(),
                    respond: tx,
                },
            );
            match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                Ok(Ok(true)) => ApprovalOutcome::Approved,
                Ok(Ok(false)) => {
                    ApprovalOutcome::denied("Not this one — please pick another approach")
                }
                Ok(Err(_)) => {
                    ApprovalOutcome::denied("Approval flow ended before a response arrived")
                }
                Err(_) => ApprovalOutcome::denied(
                    "No approval came in within 60s — moving on without this call",
                ),
            }
        })
    }
}

fn hook_summary_event(summary: &HookRunSummary) -> AgentEvent {
    AgentEvent::HookExecuted {
        event: summary.event,
        success: summary.success(),
        summary: summary.brief(),
        command_count: summary.outcomes.len(),
    }
}

fn emit_hook_summary_event(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    store: &Option<EventLogStore>,
    project_root: &Path,
    session_id: SessionId,
    turn_id: Option<TurnId>,
    summary: &HookRunSummary,
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
    send_event(tx, hook_summary_event(summary));
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
    emit_stream_chunk_deltas_with_options(tx, chunk, true)
}

fn emit_stream_chunk_deltas_with_options(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    chunk: &crate::deepseek::models::StreamChunk,
    emit_content: bool,
) -> EmittedStreamDeltas {
    let mut emitted = EmittedStreamDeltas::default();
    for choice in &chunk.choices {
        let mut hidden_delta = String::new();
        if let Some(content) = &choice.delta.content {
            if emit_content && !content.is_empty() {
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
    }
    emitted
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

fn changed_files_for_tool_call(tc: &ToolCall) -> Vec<String> {
    crate::tools::backend::changed_files_for_call(tc)
}

fn event_changed_files_for_tool_result(tool_result: &ToolLoopResult) -> Vec<String> {
    if tool_result.changed_files.is_empty() {
        changed_files_for_tool_call(&tool_result.call)
    } else {
        tool_result.changed_files.clone()
    }
}

fn question_prompt_for_tool_call(
    tc: &ToolCall,
) -> Option<crate::tools::ask_user::PendingUserQuestion> {
    if !matches!(tc.function.name.as_str(), "ask_user" | "ask_user_question") {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(&tc.function.arguments).ok()?;
    crate::tools::ask_user::pending_question_from_tool_value(&tc.function.name, &value).ok()
}

fn should_force_swarm(user_input: &str) -> bool {
    let lower = user_input.to_lowercase();
    contains_any(
        &lower,
        &[
            "开蜂群",
            "蜂群",
            "集群",
            "并行",
            "多 agent",
            "多agent",
            "多智能体",
            "swarm",
            "multi-agent",
            "parallel agents",
        ],
    )
}

/// Reconcile the lexical task lane with the complexity router's verdict.
///
/// `classify_task` picks the lane from surface keywords, but it can land an
/// actionable edit on a tool-less chat lane (the lane decides `send_tools`).
/// When the router independently judged this a direct execution that writes
/// files or runs commands, upgrade the chat lane into the tool loop so the
/// model actually receives edit/run tools.
fn resolve_lane(task: &TaskClass, assessment: Option<&ComplexityAssessment>) -> ExecutionLane {
    let base = task.default_lane();
    let is_chat_lane = matches!(
        base,
        ExecutionLane::ChatNonThinking | ExecutionLane::ChatThinking
    );
    if is_chat_lane {
        if let Some(a) = assessment {
            if a.route == Route::DirectExecute && assessment_implies_edits(a) {
                return ExecutionLane::ToolLoopThinking;
            }
        }
    }
    base
}

/// Whether a router assessment carries positive evidence that the task changes
/// files or runs commands (as opposed to a pure read/chat task).
fn assessment_implies_edits(a: &ComplexityAssessment) -> bool {
    a.predicted_write_files > 0
        || a.predicted_commands > 0
        || a.reason_codes.iter().any(|r| {
            matches!(
                r,
                ReasonCode::SingleFileSafe
                    | ReasonCode::MultiFile
                    | ReasonCode::ShellRequired
                    | ReasonCode::TestRequired
            )
        })
}

/// Pick an ephemeral per-turn model tier for auto mode (no explicit pin).
///
/// Returns `Some(Flash)` to downgrade a clearly-lightweight direct task so it
/// does not burn the expensive Pro model; returns `None` to leave the
/// effort-based default in place (complex/plan tasks, pinned sessions, or when
/// the router did not run).
fn auto_tier_model_for(
    has_pinned_model: bool,
    assessment: Option<&ComplexityAssessment>,
) -> Option<DeepSeekModel> {
    // An explicit user pin is authoritative — never auto-tier over it.
    if has_pinned_model {
        return None;
    }
    let a = assessment?;
    if a.route == Route::DirectExecute && assessment_is_lightweight(a) {
        return Some(DeepSeekModel::Flash);
    }
    None
}

/// Whether a direct-execute assessment is light enough to run on Flash: no risk
/// flags, no commands, at most one file write, and only read-only /
/// single-file-safe reason codes.
fn assessment_is_lightweight(a: &ComplexityAssessment) -> bool {
    a.risk_flags.is_empty()
        && a.predicted_commands == 0
        && a.predicted_write_files <= 1
        && a.reason_codes.iter().all(|r| r.is_safe_for_direct())
}

fn swarm_patch_approval_details(
    result: &crate::agent::swarm::SwarmResult,
    changed_files: &[String],
    patch: &str,
    use_chinese: bool,
) -> String {
    let patch_lines = patch.lines().count();
    let files = if changed_files.is_empty() {
        "none".to_string()
    } else {
        changed_files.join(", ")
    };
    if use_chinese {
        format!(
            "来源: swarm coordinator {}\n文件: {}\n摘要: {} 个 pending patch，{} 行 diff 已隐藏\n风险: 将写入工作区文件",
            result.run_id,
            files,
            result.pending_patches.len(),
            patch_lines
        )
    } else {
        format!(
            "Source: swarm coordinator {}\nFiles: {}\nSummary: {} pending patch(es), {} diff lines hidden\nRisk: writes workspace files",
            result.run_id,
            files,
            result.pending_patches.len(),
            patch_lines
        )
    }
}

fn format_swarm_plan_artifact(
    user_input: &str,
    plan: &crate::agent::swarm::SwarmPlan,
    use_chinese: bool,
) -> String {
    if use_chinese {
        let team = plan.team_plan();
        let mut out = format!(
            "# 团队计划\n\n## 目标\n{}\n\n## 用户请求\n{}\n\n## 里程碑\n",
            team.goal, user_input
        );
        for (index, milestone) in team.milestones.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", index + 1, milestone.title));
        }
        if !team.acceptance_criteria.is_empty() {
            out.push_str("\n## 验收标准\n");
            for item in &team.acceptance_criteria {
                out.push_str(&format!("- {item}\n"));
            }
        }
        out.push_str("\n## Agent 分工\n");
        for (index, task) in plan.tasks.iter().enumerate() {
            let files = if task.focus_files.is_empty() {
                "无指定文件".to_string()
            } else {
                task.focus_files.join(", ")
            };
            out.push_str(&format!(
                "{}. {} - {}。关注文件：{}\n",
                index + 1,
                task.role.as_str(),
                task.description,
                files
            ));
        }
        if !team.risks.is_empty() {
            out.push_str("\n## 风险点\n");
            for risk in &team.risks {
                out.push_str(&format!("- {risk}\n"));
            }
        }
        if !plan.validation_commands.is_empty() {
            out.push_str("\n## 验证命令\n");
            for command in &plan.validation_commands {
                out.push_str(&format!("- `{command}`\n"));
            }
        }
        out.push_str("\n## 确认点\n- 写入、命令、冲突和恢复决策按 policy 触发审批；明确只读蜂群不弹执行确认。\n");
        out
    } else {
        let team = plan.team_plan();
        let mut out = format!(
            "# Team Plan\n\n## Goal\n{}\n\n## User Request\n{}\n\n## Milestones\n",
            team.goal, user_input
        );
        for (index, milestone) in team.milestones.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", index + 1, milestone.title));
        }
        if !team.acceptance_criteria.is_empty() {
            out.push_str("\n## Acceptance Criteria\n");
            for item in &team.acceptance_criteria {
                out.push_str(&format!("- {item}\n"));
            }
        }
        out.push_str("\n## Agents\n");
        for (index, task) in plan.tasks.iter().enumerate() {
            let files = if task.focus_files.is_empty() {
                "none".to_string()
            } else {
                task.focus_files.join(", ")
            };
            out.push_str(&format!(
                "{}. {} - {}. Focus files: {}\n",
                index + 1,
                task.role.as_str(),
                task.description,
                files
            ));
        }
        if !team.risks.is_empty() {
            out.push_str("\n## Risks\n");
            for risk in &team.risks {
                out.push_str(&format!("- {risk}\n"));
            }
        }
        if !plan.validation_commands.is_empty() {
            out.push_str("\n## Validation Commands\n");
            for command in &plan.validation_commands {
                out.push_str(&format!("- `{command}`\n"));
            }
        }
        out.push_str("\n## Confirmation Points\n- Writes, commands, conflicts, and resume decisions follow policy approvals; clear read-only swarms do not ask for execution confirmation.\n");
        out
    }
}

fn format_plan_artifact(
    user_input: &str,
    plan: &Plan,
    step_lines: &[String],
    warnings: &[String],
    use_chinese: bool,
) -> String {
    if use_chinese {
        let mut out = format!(
            "# 执行计划\n\n## 目标\n{}\n\n## 用户请求\n{}\n\n## 步骤\n",
            plan.summary, user_input
        );
        for (index, step) in step_lines.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", index + 1, step));
        }
        if !warnings.is_empty() {
            out.push_str("\n## 风险提示\n");
            for warning in warnings {
                out.push_str(&format!("- {warning}\n"));
            }
        }
        out.push_str("\n## 交互策略\n- 默认智能打断：计划动作显示在计划区；歧义、风险、命令、写入、冲突才打断用户。\n");
        out
    } else {
        let mut out = format!(
            "# Execution Plan\n\n## Goal\n{}\n\n## User Request\n{}\n\n## Steps\n",
            plan.summary, user_input
        );
        for (index, step) in step_lines.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", index + 1, step));
        }
        if !warnings.is_empty() {
            out.push_str("\n## Warnings\n");
            for warning in warnings {
                out.push_str(&format!("- {warning}\n"));
            }
        }
        out.push_str("\n## Interaction Policy\n- Smart interruptions by default: plan actions stay attached to the plan; ambiguity, risky commands, writes, conflicts, and resume decisions interrupt the user.\n");
        out
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value).trim()
}

fn validate_swarm_patch_for_auto_apply(
    project_root: &Path,
    patch: &str,
    changed_files: &[String],
) -> Result<(), String> {
    if patch.trim().is_empty() {
        return Err("empty patch".into());
    }
    let parsed_paths = crate::workspace::apply::parse_patch_paths(patch);
    if parsed_paths.is_empty() {
        return Err("patch does not declare changed paths".into());
    }
    let paths = if changed_files.is_empty() {
        parsed_paths
    } else {
        changed_files.to_vec()
    };
    for path in paths {
        let path_obj = Path::new(&path);
        if path_obj.is_absolute() {
            return Err(format!("absolute path is not allowed: {path}"));
        }
        if path_obj
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(format!("path escapes workspace: {path}"));
        }
        let resolved = project_root.join(path_obj);
        if crate::policy::paths::is_blocked_path(&resolved, project_root) {
            return Err(format!("protected path: {path}"));
        }
    }
    validate_patch_applies_cleanly(project_root, patch)?;
    Ok(())
}

fn validate_patch_applies_cleanly(project_root: &Path, patch: &str) -> Result<(), String> {
    let mut child = Command::new("git")
        .args(["apply", "--check"])
        .current_dir(project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start git apply --check: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(patch.as_bytes())
            .map_err(|e| format!("failed to write patch to git apply --check: {e}"))?;
    }
    let output = wait_with_limited_patch_check_output(child)
        .map_err(|e| format!("git apply --check failed to run: {e}"))?;
    if !output.status.success() {
        let stderr = patch_check_output_text(&output.stderr, output.stderr_truncated, "stderr");
        return Err(format!("patch does not apply cleanly: {}", stderr.trim()));
    }
    Ok(())
}

struct LimitedPatchCheckOutput {
    status: ExitStatus,
    stderr: Vec<u8>,
    stderr_truncated: bool,
}

fn wait_with_limited_patch_check_output(
    mut child: std::process::Child,
) -> Result<LimitedPatchCheckOutput, std::io::Error> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture patch check stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture patch check stderr"))?;

    let stdout_handle =
        std::thread::spawn(move || read_limited_stream(stdout, PATCH_CHECK_STDOUT_BYTES));
    let stderr_handle =
        std::thread::spawn(move || read_limited_stream(stderr, PATCH_CHECK_STDERR_BYTES));
    let status = child.wait()?;
    let (_stdout, _stdout_truncated) = stdout_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("patch check stdout reader panicked")))?;
    let (stderr, stderr_truncated) = stderr_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("patch check stderr reader panicked")))?;

    Ok(LimitedPatchCheckOutput {
        status,
        stderr,
        stderr_truncated,
    })
}

fn read_limited_stream<R: Read>(
    mut reader: R,
    max_bytes: usize,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut collected = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        let remaining = max_bytes.saturating_sub(collected.len());
        let keep = bytes_read.min(remaining);
        if keep > 0 {
            collected.extend_from_slice(&buffer[..keep]);
        }
        if keep < bytes_read || collected.len() >= max_bytes {
            truncated = true;
        }
    }

    Ok((collected, truncated))
}

fn patch_check_output_text(bytes: &[u8], truncated: bool, stream: &str) -> String {
    let mut text = String::from_utf8_lossy(bytes).to_string();
    if truncated {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("[patch check {stream} truncated]"));
    }
    text
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

#[derive(Debug, Clone)]
struct ContextualTurnInput {
    routing_input: String,
    task_description: String,
    transient_context: Vec<String>,
}

impl ContextualTurnInput {
    fn from_session(session: &Session, user_input: &str) -> Self {
        let user_input = user_input.trim();
        let standalone = || Self {
            routing_input: user_input.to_string(),
            task_description: user_input.to_string(),
            transient_context: Vec::new(),
        };

        if !is_contextual_followup_request(user_input) {
            return standalone();
        }

        let anchors = recent_user_context_anchors(session, user_input, 3);
        let Some(latest_anchor) = anchors.last().cloned() else {
            return standalone();
        };

        let use_chinese =
            contains_cjk(user_input) || anchors.iter().any(|anchor| contains_cjk(anchor));
        let anchor_lines = anchors
            .iter()
            .map(|anchor| format!("- {anchor}"))
            .collect::<Vec<_>>()
            .join("\n");
        let parent_context = summarize_parent_context(session);
        let context_note = if use_chinese {
            format!(
                "上下文续接说明：\n当前用户输入：{user_input}\n这个输入依赖最近对话。不要把它当成孤立的新任务；请延续最近用户目标。\n\n最近用户目标：\n{anchor_lines}\n\n{parent_context}"
            )
        } else {
            format!(
                "Context carry-over:\nCurrent user input: {user_input}\nThis input depends on the recent conversation. Do not treat it as a standalone task; continue the recent user goal.\n\nRecent user goals:\n{anchor_lines}\n\n{parent_context}"
            )
        };
        let routing_input = format!("{user_input}\n\n{context_note}");

        Self {
            routing_input,
            task_description: latest_anchor,
            transient_context: vec![context_note],
        }
    }

    fn routing_input(&self) -> &str {
        &self.routing_input
    }

    fn task_description(&self) -> &str {
        &self.task_description
    }

    fn transient_context(&self) -> &[String] {
        &self.transient_context
    }
}

fn recent_user_context_anchors(
    session: &Session,
    current_input: &str,
    limit: usize,
) -> Vec<String> {
    let current_compact = compact_context_text(current_input);
    let mut anchors = Vec::new();
    for msg in session.messages.iter().rev() {
        if msg.role != Role::User || msg.visibility != MessageVisibility::UserVisible {
            continue;
        }
        let text = compact_context_text(&msg.content.to_string_lossy());
        if text.is_empty()
            || text == current_compact
            || is_low_information_reply(&text)
            || is_contextual_followup_request(&text)
        {
            continue;
        }
        anchors.push(truncate_context_anchor(&text, 360));
        if anchors.len() >= limit {
            break;
        }
    }
    anchors.reverse();
    anchors
}

fn compact_context_text(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_context_anchor(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let prefix = value.chars().take(max_chars).collect::<String>();
    format!("{prefix}...")
}

fn is_contextual_followup_request(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    let compact_len = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    if compact_len > 80 {
        return false;
    }

    let lower = trimmed.to_lowercase();
    let has_followup_phrase = contains_any(
        &lower,
        &[
            "继续",
            "接着",
            "下一步",
            "按上面",
            "按照上面",
            "按这个",
            "按刚才",
            "按之前",
            "就这个",
            "就按",
            "开始干活",
            "开干",
            "去做",
            "执行吧",
            "开始执行",
            "同意",
            "可以",
            "确认",
            "批准",
            "开启多智能体",
            "打开多智能体",
            "开多智能体",
            "用多智能体",
            "让多智能体",
            "多智能体干活",
            "跑多智能体",
            "开蜂群",
            "用蜂群",
            "并行干活",
            "continue",
            "go ahead",
            "do it",
            "proceed",
            "next step",
            "use agents",
            "run agents",
            "start agents",
            "run swarm",
            "start swarm",
            "approve",
            "approved",
        ],
    );
    if !has_followup_phrase {
        return false;
    }

    let references_context = contains_any(
        &lower,
        &[
            "上面", "这个", "刚才", "之前", "前面", "继续", "it", "that", "above", "previous",
        ],
    );
    let has_explicit_path =
        lower.contains("src/") || lower.contains(".rs") || lower.contains(".md");
    !has_explicit_path || references_context
}

fn is_low_information_reply(input: &str) -> bool {
    let normalized = input
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '。' | '！' | '？' | '，' | ',' | '.' | '!' | '?' | ';' | '；'
            )
        })
        .trim()
        .to_lowercase();
    matches!(
        normalized.as_str(),
        "同意"
            | "继续"
            | "好"
            | "好的"
            | "可以"
            | "确认"
            | "批准"
            | "行"
            | "嗯"
            | "yes"
            | "ok"
            | "okay"
            | "approve"
            | "approved"
            | "continue"
            | "go ahead"
            | "do it"
            | "a"
            | "s"
            | "d"
    )
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
        auto_tier_model_for, changed_files_for_tool_call, emit_stream_chunk_deltas,
        emit_stream_chunk_deltas_with_options, event_changed_files_for_tool_result,
        generate_plan_options, is_contextual_followup_request, plan_execution_context,
        plan_execution_prompt, plan_or_input_uses_chinese, plan_uses_chinese, resolve_lane,
        swarm_patch_approval_details, validate_swarm_patch_for_auto_apply, AgentEvent,
        ComplexityAssessment, ContextualTurnInput, ExecutionLane, PlanExecutionState,
        PlanStepStatus, ReasonCode, Route, TaskClass,
    };
    use crate::agent::swarm::{SwarmAgentRole, SwarmPendingPatch, SwarmResult};
    use crate::agent::tool_loop::ToolLoopResult;
    use crate::deepseek::models::{
        DeepSeekModel, MessageContent, MessageId, MessageVisibility, ProtocolMessage,
        ReasoningState, Role, Session, SessionId, SessionMetadata, StreamChunk, ToolCall,
        ToolCallFunction, ToolResultRecord, TurnId,
    };
    use crate::plan::executor::PlanStep;
    use crate::plan::schema::{Plan, Risk, RiskLevel};

    fn direct_execute_assessment() -> ComplexityAssessment {
        ComplexityAssessment {
            route: Route::DirectExecute,
            complexity_label: crate::agent::router::ComplexityLabel::Simple,
            score: 10,
            confidence: 0.9,
            reason_codes: Vec::new(),
            hard_trigger_codes: Vec::new(),
            predicted_write_files: 0,
            predicted_commands: 0,
            predicted_duration_ms: 5_000,
            risk_flags: Vec::new(),
            classifier_version: "test".to_string(),
            explanation: String::new(),
            model_name: None,
            latency_ms: 0,
        }
    }

    #[test]
    fn router_write_signal_upgrades_chat_lane_to_tool_loop() {
        // classify_task landed on Chat, but the router judged a direct execution
        // with a file write → upgrade to the tool loop so tools get sent.
        let mut a = direct_execute_assessment();
        a.predicted_write_files = 1;
        assert_eq!(
            resolve_lane(&TaskClass::Chat, Some(&a)),
            ExecutionLane::ToolLoopThinking
        );

        // A single-file-safe reason code is also sufficient evidence.
        let mut a = direct_execute_assessment();
        a.reason_codes.push(ReasonCode::SingleFileSafe);
        assert_eq!(
            resolve_lane(&TaskClass::Chat, Some(&a)),
            ExecutionLane::ToolLoopThinking
        );
    }

    #[test]
    fn router_keeps_chat_lane_without_edit_evidence() {
        // Pure chat/explain (DirectExecute, no edit signals) stays on chat.
        let a = direct_execute_assessment();
        assert_eq!(
            resolve_lane(&TaskClass::Chat, Some(&a)),
            ExecutionLane::ChatNonThinking
        );
        // No assessment → fall back to the lexical default lane.
        assert_eq!(
            resolve_lane(&TaskClass::Chat, None),
            ExecutionLane::ChatNonThinking
        );
        // Non-chat lanes are never downgraded.
        assert_eq!(
            resolve_lane(&TaskClass::Execute, None),
            ExecutionLane::ToolLoopThinking
        );
    }

    #[test]
    fn auto_tier_downgrades_lightweight_direct_task_to_flash() {
        // Unpinned, simple direct task → run it on Flash to save cost.
        let a = direct_execute_assessment();
        assert_eq!(
            auto_tier_model_for(false, Some(&a)),
            Some(DeepSeekModel::Flash)
        );
    }

    #[test]
    fn auto_tier_keeps_base_model_for_pin_heavy_or_complex() {
        let a = direct_execute_assessment();
        // Explicit pin → never tier.
        assert_eq!(auto_tier_model_for(true, Some(&a)), None);

        // Multi-file / write-heavy direct task → keep the base (Pro) model.
        let mut heavy = direct_execute_assessment();
        heavy.predicted_write_files = 3;
        heavy.reason_codes.push(ReasonCode::MultiFile);
        assert_eq!(auto_tier_model_for(false, Some(&heavy)), None);

        // A task that runs commands stays on the base model.
        let mut cmd = direct_execute_assessment();
        cmd.predicted_commands = 1;
        assert_eq!(auto_tier_model_for(false, Some(&cmd)), None);

        // Complex (plan-review) assessment → keep the base model.
        let mut complex = direct_execute_assessment();
        complex.route = Route::PlanReview;
        assert_eq!(auto_tier_model_for(false, Some(&complex)), None);

        // No assessment (forced lane / explicit plan) → no tiering.
        assert_eq!(auto_tier_model_for(false, None), None);
    }

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

    fn session_with_user_messages(messages: &[&str]) -> Session {
        Session {
            id: SessionId::new_v4(),
            name: None,
            project_root: ".".into(),
            messages: messages
                .iter()
                .map(|message| user_message(message))
                .collect(),
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            metadata: SessionMetadata::default(),
        }
    }

    fn user_message(content: &str) -> ProtocolMessage {
        ProtocolMessage {
            id: MessageId::new_v4(),
            role: Role::User,
            content: MessageContent::from(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: TurnId::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        }
    }

    #[test]
    fn continuation_swarm_input_inherits_recent_user_context() {
        let session = session_with_user_messages(&[
            "请逐文件审查 review_report_v2.md，并对整个代码库做全量对抗审查。",
        ]);

        let turn_input = ContextualTurnInput::from_session(&session, "开启多智能体干活");

        assert!(is_contextual_followup_request("开启多智能体干活"));
        assert!(turn_input.routing_input().contains("全量对抗审查"));
        assert!(turn_input
            .routing_input()
            .contains("不要把它当成孤立的新任务"));
        assert_eq!(
            turn_input.task_description(),
            "请逐文件审查 review_report_v2.md，并对整个代码库做全量对抗审查。"
        );
        assert_eq!(turn_input.transient_context().len(), 1);
    }

    #[test]
    fn continuation_context_skips_prior_low_information_turns() {
        let session = session_with_user_messages(&[
            "审核整个 CLI，重点检查 agent orchestrator 和 swarm 规划链路。",
            "同意",
            "继续",
        ]);

        let turn_input = ContextualTurnInput::from_session(&session, "开始干活");

        assert!(turn_input.routing_input().contains("swarm 规划链路"));
        assert!(!turn_input.task_description().contains("同意"));
        assert!(!turn_input.task_description().contains("继续"));
    }

    #[test]
    fn concrete_task_inputs_remain_standalone() {
        let session = session_with_user_messages(&["审核整个 CLI。"]);

        let turn_input =
            ContextualTurnInput::from_session(&session, "修复 src/tui/app.rs 的审批弹窗位置");

        assert_eq!(
            turn_input.routing_input(),
            "修复 src/tui/app.rs 的审批弹窗位置"
        );
        assert_eq!(
            turn_input.task_description(),
            "修复 src/tui/app.rs 的审批弹窗位置"
        );
        assert!(turn_input.transient_context().is_empty());
    }

    #[test]
    fn swarm_auto_apply_rejects_protected_patch_paths() {
        let root = std::path::Path::new("/tmp/project");
        let patch = "diff --git a/.env b/.env\n--- a/.env\n+++ b/.env\n@@ -1 +1 @@\n-a\n+b\n";

        let err = validate_swarm_patch_for_auto_apply(root, patch, &[".env".to_string()])
            .expect_err("protected path should be rejected");

        assert!(err.contains("protected path"));
    }

    #[test]
    fn patch_check_stream_reader_caps_output() {
        let input = std::io::Cursor::new("x".repeat(8192));
        let (bytes, truncated) = super::read_limited_stream(input, 1024).expect("limited stream");

        assert_eq!(bytes.len(), 1024);
        assert!(truncated);
    }

    #[test]
    fn patch_check_output_marks_truncation() {
        let output = super::patch_check_output_text(b"error", true, "stderr");

        assert!(output.contains("error"));
        assert!(output.contains("[patch check stderr truncated]"));
    }

    #[test]
    fn swarm_patch_approval_summary_hides_full_diff() {
        let patch = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-secret old line\n+secret new line\n";
        let result = SwarmResult {
            run_id: "swarm-1".into(),
            success: true,
            summary: "ok".into(),
            tasks_total: 1,
            tasks_done: 1,
            tasks_failed: 0,
            token_usage: 0,
            outputs: Vec::new(),
            files_read: Vec::new(),
            files_written: Vec::new(),
            pending_patches: vec![SwarmPendingPatch {
                task_id: "task-1".into(),
                role: SwarmAgentRole::Worker,
                summary: "update a".into(),
                patch: patch.into(),
                changed_files: vec!["src/a.rs".into()],
                conflict: false,
            }],
            patch_conflicts: Vec::new(),
            validation_commands: Vec::new(),
            validation_report: None,
            cancelled: false,
            duration_ms: 10,
        };

        let details = swarm_patch_approval_details(&result, &["src/a.rs".into()], patch, true);

        assert!(details.contains("来源: swarm coordinator swarm-1"));
        assert!(details.contains("文件: src/a.rs"));
        assert!(details.contains("diff 已隐藏"));
        assert!(!details.contains("secret old line"));
        assert!(!details.contains("secret new line"));
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
    fn chinese_user_input_controls_plan_option_language() {
        let mut plan = sample_plan("Test the CLI environment");
        plan.steps = vec!["Read input history".to_string()];

        assert!(!plan_uses_chinese(&plan));
        assert!(plan_or_input_uses_chinese(&plan, "测试一下这个 CLI"));

        let labels =
            generate_plan_options(&plan, plan_or_input_uses_chinese(&plan, "测试一下这个 CLI"))
                .into_iter()
                .map(|option| option.label)
                .collect::<Vec<_>>();

        assert!(labels.iter().any(|label| label.contains("自动执行")));
        assert!(labels.iter().any(|label| label == "取消"));
        assert!(!labels.iter().any(|label| label.contains("Execute")));
    }

    #[test]
    fn plan_execution_prompt_keeps_process_narration_internal() {
        let prompt = plan_execution_prompt(true);

        assert!(prompt.contains("不要输出逐步说明"));
        assert!(prompt.contains("思考过程"));
        assert!(prompt.contains("最终结果由系统整理"));

        let prompt = plan_execution_prompt(false);
        assert!(prompt.contains("do not stream narration"));
        assert!(prompt.contains("reasoning prose"));
    }

    #[test]
    fn plan_execution_context_carries_plan_and_instruction() {
        let context = plan_execution_context(
            r#"{"summary":"修复 managed agent 链路"}"#,
            "请按上面的计划逐步执行。",
        );

        assert_eq!(context.len(), 2);
        assert!(context[0].contains("Current approved execution plan JSON"));
        assert!(context[0].contains("managed agent"));
        assert!(context[1].contains("请按上面的计划逐步执行"));
    }

    #[test]
    fn changed_files_for_tool_call_extracts_apply_patch_paths() {
        let call = ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "apply_patch".into(),
                arguments: serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: src/agent/orchestrator.rs\n*** Add File: tests/session_resume_tests.rs\n*** End Patch\n"
                })
                .to_string(),
            },
        };

        assert_eq!(
            changed_files_for_tool_call(&call),
            vec![
                "src/agent/orchestrator.rs".to_string(),
                "tests/session_resume_tests.rs".to_string()
            ]
        );
    }

    #[test]
    fn event_changed_files_prefers_runtime_metadata_over_argument_guess() {
        let call = ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "write_file".into(),
                arguments: serde_json::json!({
                    "path": "src/from-arguments.rs",
                    "content": "old"
                })
                .to_string(),
            },
        };
        let result = ToolResultRecord {
            tool_call_id: call.id.clone(),
            name: call.function.name.clone(),
            result: "ok".into(),
            is_error: false,
        };
        let tool_result =
            ToolLoopResult::new(call, result, 42, vec!["src/from-runtime.rs".to_string()]);

        assert_eq!(
            event_changed_files_for_tool_result(&tool_result),
            vec!["src/from-runtime.rs".to_string()]
        );
    }

    #[test]
    fn ask_user_question_tool_call_creates_pending_question() {
        let call = ToolCall {
            id: "call-ask".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "ask_user_question".into(),
                arguments: serde_json::json!({
                    "question": "Pick validation depth?",
                    "options": [
                        {"label": "Quick"},
                        {"label": "Full", "description": "Run all tests"}
                    ]
                })
                .to_string(),
            },
        };

        let question =
            super::question_prompt_for_tool_call(&call).expect("question payload should parse");

        assert_eq!(question.title, "Question");
        assert_eq!(question.summary, "Pick validation depth?");
        assert_eq!(question.options, vec!["Quick", "Full"]);
        assert_eq!(question.descriptions, vec!["", "Run all tests"]);
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
    fn plan_state_finishes_remaining_steps_as_failed_when_execution_did_not_run() {
        let mut state = sample_plan_state();

        let updates = state.finish_remaining(false);

        assert_eq!(updates.len(), 2);
        assert!(updates
            .iter()
            .all(|update| update.status == PlanStepStatus::Failed));
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
            r#"{"choices":[{"index":0,"delta":{"reasoning_content":"thinking","content":"hello","tool_calls":[{"index":0,"function":{"name":"read_file","arguments":"{\"path\":\"src/main.rs\"}"}}]},"finish_reason":null}],"usage":null}"#,
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
        assert!(matches!(
            rx.try_recv().expect("hidden tool token delta"),
            AgentEvent::TokenDelta { input_tokens: 0, output_tokens } if output_tokens > 0
        ));
    }

    #[test]
    fn stream_chunk_deltas_can_suppress_visible_content_for_plan_execution() {
        let chunk = serde_json::from_str::<StreamChunk>(
            r#"{"choices":[{"index":0,"delta":{"reasoning_content":"thinking","content":"internal narration"},"finish_reason":null}],"usage":null}"#,
        )
        .expect("valid stream chunk");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let emitted = emit_stream_chunk_deltas_with_options(&tx, &chunk, false);

        assert!(!emitted.content);
        assert!(emitted.reasoning);
        assert!(matches!(
            rx.try_recv().expect("reasoning delta"),
            AgentEvent::ReasoningDelta(text) if text == "thinking"
        ));
        assert!(rx.try_recv().is_err());
    }

    // A scripted streaming client so the orchestrator turn loop runs without a
    // network round-trip; returns one canned StreamResult per turn.
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

    #[tokio::test]
    async fn run_turn_streams_final_answer_through_injected_client() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut session = session_with_user_messages(&[]);
        session.project_root = root.path().to_path_buf();
        let mock = std::sync::Arc::new(MockStreamClient::new(vec![final_answer(
            "All set — nothing to change.",
        )]));
        let mut orchestrator = super::Orchestrator::new(
            crate::deepseek::client::DeepSeekClient::new("test-key".to_string()),
            root.path().to_path_buf(),
            session,
        )
        .with_stream_client(mock);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        // A greeting routes to direct-execute via rules (no model classifier),
        // so the only model call is the injected stream.
        orchestrator
            .run_turn("你好", tx)
            .await
            .expect("turn should succeed");

        assert!(
            orchestrator
                .session
                .messages
                .iter()
                .any(|m| m.content.to_string_lossy().contains("All set")),
            "the streamed final answer should land in the session"
        );
    }

    fn usage(total: u32) -> crate::deepseek::models::Usage {
        crate::deepseek::models::Usage {
            prompt_tokens: total,
            completion_tokens: 0,
            total_tokens: total,
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            prompt_tokens_details: None,
        }
    }

    fn final_answer_with_usage(
        content: &str,
        usage: crate::deepseek::models::Usage,
    ) -> crate::deepseek::models::StreamResult {
        let mut result = final_answer(content);
        result.usage = Some(usage);
        result
    }

    fn list_dir_tool_call(
        usage: crate::deepseek::models::Usage,
    ) -> crate::deepseek::models::StreamResult {
        crate::deepseek::models::StreamResult {
            content: String::new(),
            reasoning_content: String::new(),
            tool_calls: vec![crate::deepseek::models::ToolCall {
                id: "tc-1".into(),
                call_type: "function".into(),
                function: crate::deepseek::models::ToolCallFunction {
                    name: "list_dir".into(),
                    arguments: r#"{"path":"."}"#.into(),
                },
            }],
            finish_reason: Some("tool_calls".into()),
            usage: Some(usage),
        }
    }

    #[tokio::test]
    async fn tool_emitting_turn_counts_every_model_call_usage() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut session = session_with_user_messages(&[]);
        session.project_root = root.path().to_path_buf();
        // Call 1 emits a tool call (100 tokens); the follow-up is the final
        // answer (40 tokens). Both must be counted — previously only the final
        // no-tool response (40) landed in the totals.
        let mock = std::sync::Arc::new(MockStreamClient::new(vec![
            list_dir_tool_call(usage(100)),
            final_answer_with_usage("done", usage(40)),
        ]));
        let mut orchestrator = super::Orchestrator::new(
            crate::deepseek::client::DeepSeekClient::new("test-key".to_string()),
            root.path().to_path_buf(),
            session,
        )
        .with_stream_client(mock);
        orchestrator.yolo_mode = true; // auto-approve the list_dir call

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        orchestrator
            .run_turn("你好", tx)
            .await
            .expect("turn should succeed");

        assert_eq!(
            orchestrator.session.metadata.total_tokens, 140,
            "both the tool-emitting call (100) and the final answer (40) must be counted, not just the final answer"
        );
    }
}

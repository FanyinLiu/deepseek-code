use std::path::{Component, Path};
use std::sync::{atomic::AtomicBool, Arc};

use chrono::Utc;
use tokio::sync::mpsc;

use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::tools as ds_tools;
use crate::deepseek::{
    thinking_config_for_lane, CacheUsage, ChatRequest, DeepSeekModel, ExecutionLane, FinishReason,
    MessageContent, MessageId, MessageVisibility, ModelCapability, ProtocolMessage,
    ReasoningEffort, ReasoningState, Role, Session, SessionId, StreamResult, SubTurnId,
    ThinkingConfig, ToolCall, ToolCallFunction, ToolDefinition, ToolResultRecord, TurnId, Usage,
};
use crate::plan;
use crate::plan::schema::{Plan, RiskLevel};
use crate::policy;
use crate::search::{self, SearchMatch};
use crate::storage::{EventLogStore, SessionEvent, SessionEventKind};

use super::background::{BackgroundQueue, BackgroundTaskSnapshot};
use super::event_sink::EventSink;
use super::lanes::{classify_task, TaskClass};
use super::prompt_builder::{load_project_rules, PromptBuilder};
use super::reasoning::ReasoningManager;
use super::router::{ComplexityAssessment, ComplexityRouter, ReasonCode, Route};
use super::subagent::SubagentToolArgs;
use super::swarm::{SwarmCoordinator, SwarmRunOptions, SwarmTaskStatus};
use super::task_tool::TaskToolHandler;
use super::tool_loop::ToolLoop;

/// Events emitted by the orchestrator during execution.
#[derive(Debug)]
pub enum AgentEvent {
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
    plan_execution: Option<PlanExecutionState>,
    mcp_registry: Option<crate::mcp::McpRegistry>,
    mcp_initialized: bool,
    event_log_store: Option<EventLogStore>,
    swarm_cancel_token: Option<Arc<AtomicBool>>,
}

impl Orchestrator {
    #[must_use]
    pub fn new(client: DeepSeekClient, project_root: std::path::PathBuf, session: Session) -> Self {
        let event_log_store =
            dirs::home_dir().map(|home| EventLogStore::new(home.join(".deepseek-code")));
        Self {
            client,
            project_root,
            session,
            background_queue: BackgroundQueue::new(),
            yolo_mode: false,
            plan_execution: None,
            mcp_registry: None,
            mcp_initialized: false,
            event_log_store,
            swarm_cancel_token: None,
        }
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
                Ok(path) => tracing::info!("wrote DS artifact: {}", path.display()),
                Err(err) => tracing::warn!("failed to write DS artifact: {err}"),
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

    fn should_run_swarm(
        &self,
        user_input: &str,
        assessment: Option<&ComplexityAssessment>,
        forced: bool,
    ) -> bool {
        let config = crate::storage::Config::load(Some(&self.project_root)).unwrap_or_default();
        if !config.subagent.enabled || !config.subagent.swarm_enabled {
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
        config.subagent.auto_decompose && (multi_file_hint || (router_complex && complex_hint))
    }

    async fn run_swarm_mode(
        &mut self,
        user_input: &str,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        turn_id: TurnId,
    ) -> Result<(), anyhow::Error> {
        let config = crate::storage::Config::load(Some(&self.project_root)).unwrap_or_default();
        let coordinator = SwarmCoordinator::new(
            Arc::new(self.client.clone()),
            self.project_root.clone(),
            config.subagent.max_parallel,
        );
        let plan = coordinator.plan_hybrid(user_input, user_input, &[]).await;
        let use_chinese = contains_cjk(user_input);
        self.write_artifact(
            "swarm-plan",
            &format_swarm_plan_artifact(user_input, &plan, use_chinese),
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
            .flat_map(|patch| patch.changed_files.clone())
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

        if write_requires_approval {
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

        let call = ToolCall {
            id: format!("swarm-patch-{}", uuid::Uuid::new_v4()),
            call_type: "function".to_string(),
            function: ToolCallFunction {
                name: "apply_patch".to_string(),
                arguments: serde_json::json!({ "patch": combined_patch }).to_string(),
            },
        };
        self.record_event(
            Some(turn_id),
            SessionEventKind::ToolCallStarted {
                tool_call_id: call.id.clone(),
                name: call.function.name.clone(),
                arguments: call.function.arguments.clone(),
            },
        );
        let policy_config = crate::storage::Config::load(Some(&self.project_root))
            .map(|c| c.policy)
            .unwrap_or_default();
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
            self.run_swarm_post_apply_validation(result, turn_id, use_chinese)
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
        turn_id: TurnId,
        use_chinese: bool,
    ) -> Option<(String, bool)> {
        if result.validation_commands.is_empty() {
            return None;
        }
        let policy_config = crate::storage::Config::load(Some(&self.project_root))
            .map(|c| c.policy)
            .unwrap_or_default();
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
        let force_swarm = should_force_swarm(user_input);

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
            let route = if shadow_mode || force_swarm {
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
                    return self
                        .run_plan_mode(user_input, &search_ctx, event_tx, None)
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
        self.record_event(
            Some(turn_id),
            SessionEventKind::UserMessage {
                content: event_user_content,
            },
        );

        if self.should_run_swarm(user_input, assessment.as_ref(), force_swarm) {
            return self.run_swarm_mode(user_input, event_tx, turn_id).await;
        }

        // 6. Search if needed
        let search_ctx = if matches!(task, TaskClass::Search | TaskClass::Plan) {
            self.run_search_phase(user_input, event_tx).await
        } else {
            None
        };

        // 7. Plan mode (explicit / router-overridden already handled above)
        if task == TaskClass::Plan {
            return self
                .run_plan_mode(user_input, &search_ctx, event_tx, Some(turn_id))
                .await;
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

        let session_events = self.load_session_events();
        let (_, messages) = PromptBuilder::new(cap.model.clone(), lane.clone(), true)
            .build_with_events(
                &self.session,
                Some(&session_events),
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
        send_request_token_delta(event_tx, &request);
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
                            SessionEventKind::AssistantVisible {
                                content: stream_result.content.clone(),
                            },
                        );
                    }
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
        user_input: &str,
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
                content: MessageContent::from(user_input),
                reasoning_content: None,
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                turn_id,
                sub_turn_id: None,
                visibility: MessageVisibility::UserVisible,
            });
            self.record_event(
                Some(turn_id),
                SessionEventKind::UserMessage {
                    content: user_input.to_string(),
                },
            );
        }

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

                let use_chinese = plan_or_input_uses_chinese(&p, user_input);

                // Convert plan to concrete steps and emit to TUI tracker
                let steps = plan::executor::plan_to_steps(&p);
                let step_lines = steps
                    .iter()
                    .map(|step| step.display_with_language(use_chinese))
                    .collect::<Vec<_>>();
                self.write_artifact(
                    "execution-plan",
                    &format_plan_artifact(
                        user_input,
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
                for i in 0..steps.len() {
                    let description = step_lines[i].clone();
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
                let plan_context = plan_execution_context(&plan_json, &execution_prompt);
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
                    messages,
                    tools: Some(tool_defs),
                    thinking: thinking_config,
                    response_format: None,
                    stream: true,
                    max_tokens: Some(8192),
                };

                let mut plan_execution_failed = false;
                send_request_token_delta(event_tx, &request);
                match self
                    .client
                    .chat_stream_accumulated_with_deltas(&request, |_| {})
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
        send_reasoning_delta(event_tx, &stream_result.reasoning_content);
        for tc in &stream_result.tool_calls {
            self.record_event(
                Some(turn_id),
                SessionEventKind::ToolCallStarted {
                    tool_call_id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                },
            );
        }
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
            let changed_files = changed_files_for_tool_call(tc);
            self.record_event(
                Some(turn_id),
                SessionEventKind::ToolCallFinished {
                    tool_call_id: result.tool_call_id.clone(),
                    name: result.name.clone(),
                    success: !result.is_error,
                    summary: crate::agent::utils::truncate_for_summary(&result.result, 200),
                    duration_ms: 0,
                    changed_files: changed_files.clone(),
                },
            );
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
            self.session.reasoning_state.mode_to_model(),
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
            model: self.session.reasoning_state.mode_to_model().to_string(),
            messages,
            tools: Some(tool_defs),
            thinking: Some(ThinkingConfig::enabled()),
            response_format: None,
            stream: true,
            max_tokens: Some(8192),
        };

        let mut emitted = EmittedStreamDeltas::default();
        send_request_token_delta(event_tx, &followup_request);
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
        let mut hidden_delta = String::new();
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
    let mut child = std::process::Command::new("git")
        .args(["apply", "--check"])
        .current_dir(project_root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start git apply --check: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin
            .write_all(patch.as_bytes())
            .map_err(|e| format!("failed to write patch to git apply --check: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("git apply --check failed to run: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("patch does not apply cleanly: {}", stderr.trim()));
    }
    Ok(())
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
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
        changed_files_for_tool_call, emit_stream_chunk_deltas, generate_plan_options,
        plan_execution_context, plan_execution_prompt, plan_or_input_uses_chinese,
        plan_uses_chinese, swarm_patch_approval_details, validate_swarm_patch_for_auto_apply,
        AgentEvent, PlanExecutionState, PlanStepStatus,
    };
    use crate::agent::swarm::{SwarmAgentRole, SwarmPendingPatch, SwarmResult};
    use crate::deepseek::models::{StreamChunk, ToolCall, ToolCallFunction};
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
    fn swarm_auto_apply_rejects_protected_patch_paths() {
        let root = std::path::Path::new("/tmp/project");
        let patch = "diff --git a/.env b/.env\n--- a/.env\n+++ b/.env\n@@ -1 +1 @@\n-a\n+b\n";

        let err = validate_swarm_patch_for_auto_apply(root, patch, &[".env".to_string()])
            .expect_err("protected path should be rejected");

        assert!(err.contains("protected path"));
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
}

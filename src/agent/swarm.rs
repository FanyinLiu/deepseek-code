use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::json_mode::parse_json_response;
use crate::deepseek::{
    ChatMessage, ChatMessageContent, ChatRequest, DeepSeekModel, ExecutionLane, ResponseFormat,
    ThinkingConfig,
};

use super::orchestrator::{AgentEvent, PlanStepStatus};
use super::subagent::{
    MergeStrategy, ParallelBatch, PermissionMode, SubagentConfig, SubagentResult, SubagentTask,
    SubagentType,
};
use super::supervisor::Supervisor;
use super::team::{
    AgentRole, AgentRunState, TeamMilestone, TeamPlan, TeamRun, TeamTask, TeamTaskMode,
};

const READ_ONLY_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search_files",
    "search_code",
    "git_status",
    "git_diff",
];
const TESTER_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search_files",
    "search_code",
    "git_status",
    "git_diff",
    "run_command",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmAgentRole {
    Coordinator,
    Explorer,
    Reviewer,
    Planner,
    Tester,
    Worker,
    Verifier,
}

impl SwarmAgentRole {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Coordinator => "coordinator",
            Self::Explorer => "explorer",
            Self::Reviewer => "reviewer",
            Self::Planner => "planner",
            Self::Tester => "tester",
            Self::Worker => "worker",
            Self::Verifier => "verifier",
        }
    }

    fn subagent_type(&self) -> SubagentType {
        match self {
            Self::Explorer => SubagentType::CodeExplorer,
            Self::Reviewer => SubagentType::CodeReviewer,
            Self::Planner => SubagentType::Planner,
            Self::Tester | Self::Verifier => SubagentType::TestRunner,
            Self::Worker | Self::Coordinator => SubagentType::GeneralPurpose,
        }
    }

    fn team_role(&self) -> AgentRole {
        match self {
            Self::Coordinator => AgentRole::Coordinator,
            Self::Explorer => AgentRole::Explorer,
            Self::Reviewer => AgentRole::Reviewer,
            Self::Planner => AgentRole::Planner,
            Self::Tester => AgentRole::Tester,
            Self::Worker => AgentRole::Worker,
            Self::Verifier => AgentRole::Verifier,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmTaskStatus {
    Pending,
    Running,
    Done,
    Failed,
    Blocked,
    Cancelled,
}

impl SwarmTaskStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    fn agent_state(&self) -> AgentRunState {
        match self {
            Self::Pending => AgentRunState::Pending,
            Self::Running => AgentRunState::Running,
            Self::Done => AgentRunState::Succeeded,
            Self::Failed => AgentRunState::Failed,
            Self::Blocked => AgentRunState::Blocked,
            Self::Cancelled => AgentRunState::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub id: String,
    pub role: SwarmAgentRole,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub focus_files: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub status: SwarmTaskStatus,
}

impl SwarmTask {
    fn team_task(&self) -> TeamTask {
        TeamTask {
            id: self.id.clone(),
            role: self.role.team_role(),
            title: self.description.clone(),
            prompt: self.prompt.clone(),
            mode: self.team_task_mode(),
            focus_files: self.focus_files.clone(),
            depends_on: self.depends_on.clone(),
        }
    }

    fn team_task_mode(&self) -> TeamTaskMode {
        match self.role {
            SwarmAgentRole::Worker => TeamTaskMode::PendingPatch,
            SwarmAgentRole::Tester | SwarmAgentRole::Verifier => TeamTaskMode::VerifyOnly,
            SwarmAgentRole::Coordinator
            | SwarmAgentRole::Explorer
            | SwarmAgentRole::Reviewer
            | SwarmAgentRole::Planner => TeamTaskMode::ReadOnly,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPlan {
    pub run_id: String,
    pub summary: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub milestones: Vec<TeamMilestone>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    pub tasks: Vec<SwarmTask>,
    pub max_parallel: usize,
    #[serde(default)]
    pub validation_commands: Vec<String>,
}

impl SwarmPlan {
    #[must_use]
    pub fn team_plan(&self) -> TeamPlan {
        TeamPlan {
            goal: if self.goal.trim().is_empty() {
                self.summary.clone()
            } else {
                self.goal.clone()
            },
            milestones: self.milestones.clone(),
            acceptance_criteria: self.acceptance_criteria.clone(),
            agent_roles: self
                .tasks
                .iter()
                .map(|task| format!("{}: {}", task.role.as_str(), task.description))
                .collect(),
            tasks: self.tasks.iter().map(SwarmTask::team_task).collect(),
            validation_commands: self.validation_commands.clone(),
            risks: self.risks.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmRun {
    pub run_id: String,
    pub plan: SwarmPlan,
    pub status: SwarmTaskStatus,
}

impl SwarmRun {
    #[must_use]
    pub fn team_run(&self) -> TeamRun {
        TeamRun {
            id: self.run_id.clone(),
            plan: self.plan.team_plan(),
            status: self.status.agent_state(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPlanDraft {
    pub goal: String,
    #[serde(default)]
    pub tasks: Vec<SwarmPlanDraftTask>,
    #[serde(default)]
    pub validation_commands: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPlanDraftTask {
    pub role: SwarmAgentRole,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub focus_files: Vec<String>,
    #[serde(default)]
    pub write_mode: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmResult {
    pub run_id: String,
    pub success: bool,
    pub summary: String,
    pub tasks_total: usize,
    pub tasks_done: usize,
    pub tasks_failed: usize,
    /// Aggregate token usage across every subagent in the run, folded into the
    /// parent session totals so swarm spend is visible.
    #[serde(default)]
    pub token_usage: u64,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub files_read: Vec<String>,
    #[serde(default)]
    pub files_written: Vec<String>,
    #[serde(default)]
    pub pending_patches: Vec<SwarmPendingPatch>,
    #[serde(default)]
    pub patch_conflicts: Vec<String>,
    #[serde(default)]
    pub validation_commands: Vec<String>,
    #[serde(default)]
    pub validation_report: Option<String>,
    pub cancelled: bool,
    pub duration_ms: u64,
}

impl SwarmResult {
    #[must_use]
    pub fn format_for_parent(&self) -> String {
        let mut out = format!(
            "## Swarm Result\n\nstatus: {}\ntasks: {}/{} done, {} failed\nduration: {}ms\n\n",
            if self.cancelled {
                "cancelled"
            } else if self.success {
                "ok"
            } else {
                "failed"
            },
            self.tasks_done,
            self.tasks_total,
            self.tasks_failed,
            self.duration_ms
        );
        out.push_str(&self.summary);
        if !self.files_read.is_empty() {
            out.push_str("\n\nfiles read: ");
            out.push_str(&self.files_read.join(", "));
        }
        if !self.files_written.is_empty() {
            out.push_str("\nfiles written: ");
            out.push_str(&self.files_written.join(", "));
        }
        if !self.pending_patches.is_empty() {
            out.push_str("\n\npending patches: ");
            out.push_str(&self.pending_patches.len().to_string());
        }
        if !self.validation_commands.is_empty() {
            out.push_str("\nvalidation commands: ");
            out.push_str(&self.validation_commands.join(", "));
        }
        if let Some(report) = &self.validation_report {
            out.push_str("\nvalidation: ");
            out.push_str(report);
        }
        if !self.patch_conflicts.is_empty() {
            out.push_str("\npatch conflicts: ");
            out.push_str(&self.patch_conflicts.join(", "));
        }
        // Surface each subagent's detailed output, not just the one-line index
        // in `summary` above. Without this the parent has to act on a digest of
        // a digest (the summary truncates each child's already-truncated
        // summary). Bounded by a total budget so a wide fan-out can't blow the
        // parent's context window.
        if !self.outputs.is_empty() {
            const OUTPUT_BUDGET: usize = 8000;
            out.push_str("\n\n# Subagent outputs\n");
            let mut used = 0usize;
            for (idx, output) in self.outputs.iter().enumerate() {
                if used >= OUTPUT_BUDGET {
                    out.push_str(&format!(
                        "\n[{} more output(s) omitted to stay within context budget]",
                        self.outputs.len() - idx
                    ));
                    break;
                }
                let remaining = OUTPUT_BUDGET - used;
                let slice: String = output.chars().take(remaining).collect();
                used += slice.chars().count();
                out.push('\n');
                out.push_str(&slice);
                out.push('\n');
            }
        }
        out
    }

    #[must_use]
    pub fn format_for_user(&self, patch_report: Option<&str>, use_chinese: bool) -> String {
        if use_chinese {
            self.format_for_user_zh(patch_report)
        } else {
            self.format_for_user_en(patch_report)
        }
    }

    fn format_for_user_zh(&self, patch_report: Option<&str>) -> String {
        let status = if self.cancelled {
            "已取消"
        } else if self.success {
            "已完成"
        } else {
            "未完成"
        };
        let mut out = format!("蜂群任务{status}\n\n结论：");
        if self.success {
            out.push_str("蜂群已形成可用结论。");
        } else if self.cancelled {
            out.push_str("任务已取消，未继续执行后续 agent。");
        } else {
            out.push_str("这次蜂群没有形成完整可用结论，失败原因已记录到事件日志。");
        }
        out.push_str(&format!(
            "\n任务状态：完成 {} / 总计 {}，失败 {}。",
            self.tasks_done, self.tasks_total, self.tasks_failed
        ));
        let summary = sanitize_swarm_visible_text(&self.summary);
        if !summary.trim().is_empty() {
            out.push_str("\n\n发现：\n");
            out.push_str(summary.trim());
        }
        if !self.files_read.is_empty() {
            out.push_str("\n\n已读取文件：");
            out.push_str(&self.files_read.join(", "));
        }
        out.push_str("\n\n变更：");
        if !self.pending_patches.is_empty() {
            out.push_str(&format!(
                "\n待处理 patch：{} 个。通过安全检查后会自动写入。",
                self.pending_patches.len()
            ));
        }
        if !self.patch_conflicts.is_empty() {
            out.push_str("\npatch 冲突：");
            out.push_str(&self.patch_conflicts.join(", "));
        }
        if self.files_written.is_empty() {
            out.push_str("\n写入文件：无。");
        } else {
            out.push_str("\n写入文件：");
            out.push_str(&self.files_written.join(", "));
        }
        out.push_str("\n\n验证：");
        let mut has_validation = false;
        if let Some(report) = patch_report {
            has_validation = true;
            out.push_str(&sanitize_swarm_visible_text(report));
        }
        if let Some(report) = &self.validation_report {
            if has_validation {
                out.push('\n');
            }
            has_validation = true;
            out.push_str(&sanitize_swarm_visible_text(report));
        }
        if !has_validation {
            out.push_str("未运行额外验证。");
        }
        out.push_str("\n\n下一步：");
        if self.tasks_failed > 0 {
            out.push_str("建议重新运行一个更小范围的蜂群任务。");
        } else if !self.success && !self.files_written.is_empty() {
            out.push_str(
                "验证失败时不自动回滚；如需撤销，请使用 `git diff` 确认后手动回滚相关文件。",
            );
        } else {
            out.push_str("可以继续动态测试，重点观察确认页、agent 状态和最终摘要。");
        }
        out
    }

    fn format_for_user_en(&self, patch_report: Option<&str>) -> String {
        let status = if self.cancelled {
            "cancelled"
        } else if self.success {
            "completed"
        } else {
            "incomplete"
        };
        let mut out = format!(
            "Swarm task {status}\n\nTasks: {} / {} done, {} failed.",
            self.tasks_done, self.tasks_total, self.tasks_failed
        );
        let summary = sanitize_swarm_visible_text(&self.summary);
        if !summary.trim().is_empty() {
            out.push_str("\n\nSummary:\n");
            out.push_str(summary.trim());
        }
        if !self.files_read.is_empty() {
            out.push_str("\n\nFiles read: ");
            out.push_str(&self.files_read.join(", "));
        }
        if !self.pending_patches.is_empty() {
            out.push_str(&format!(
                "\nPending patches: {}. They are auto-applied after safety checks.",
                self.pending_patches.len()
            ));
        }
        if self.files_written.is_empty() {
            out.push_str("\nFiles written: none.");
        } else {
            out.push_str("\nFiles written: ");
            out.push_str(&self.files_written.join(", "));
        }
        if let Some(report) = patch_report {
            out.push_str("\n\nPatch status: ");
            out.push_str(&sanitize_swarm_visible_text(report));
        }
        if let Some(report) = &self.validation_report {
            out.push_str("\n\nValidation: ");
            out.push_str(&sanitize_swarm_visible_text(report));
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwarmPendingPatch {
    pub task_id: String,
    pub role: SwarmAgentRole,
    pub summary: String,
    pub patch: String,
    pub changed_files: Vec<String>,
    pub conflict: bool,
}

#[derive(Clone)]
pub struct SwarmRunOptions {
    pub command_requires_approval: bool,
    pub cancel_token: Option<Arc<AtomicBool>>,
    pub emit_finished: bool,
}

impl Default for SwarmRunOptions {
    fn default() -> Self {
        Self {
            command_requires_approval: true,
            cancel_token: None,
            emit_finished: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SwarmEvent {
    RunStarted {
        run_id: String,
        summary: String,
        total: usize,
    },
    TaskUpdated {
        run_id: String,
        task_id: String,
        role: SwarmAgentRole,
        status: SwarmTaskStatus,
        description: String,
    },
    RunFinished {
        run_id: String,
        success: bool,
        summary: String,
    },
}

#[derive(Clone)]
pub struct SwarmCoordinator {
    client: Arc<DeepSeekClient>,
    supervisor: Supervisor,
    project_root: PathBuf,
    max_parallel: usize,
}

#[derive(Debug, Clone)]
struct SwarmIntent {
    review: bool,
    test: bool,
    write: bool,
    proposal_only: bool,
    plan: bool,
}

impl SwarmIntent {
    fn classify(description: &str, prompt: &str) -> Self {
        let lower = format!("{description}\n{prompt}").to_lowercase();
        let proposal_only = is_no_write_request(&lower);
        Self {
            review: is_review_like(&lower),
            test: is_test_like(&lower),
            write: is_write_like(&lower) && !proposal_only,
            proposal_only,
            plan: lower.contains("计划")
                || lower.contains("方案")
                || lower.contains("plan")
                || lower.contains("proposal"),
        }
    }

    fn needs_explorer(&self, files: &[String]) -> bool {
        self.review || self.proposal_only || !files.is_empty()
    }

    fn needs_reviewer(&self, files: &[String]) -> bool {
        self.review || self.proposal_only || files.len() > 1
    }

    fn needs_planner(&self) -> bool {
        self.proposal_only || self.plan
    }
}

impl SwarmCoordinator {
    #[must_use]
    pub fn new(client: Arc<DeepSeekClient>, project_root: PathBuf, max_parallel: usize) -> Self {
        Self {
            supervisor: Supervisor::new(client.clone(), project_root.clone()),
            client,
            project_root,
            max_parallel: max_parallel.clamp(1, 8),
        }
    }

    #[must_use]
    pub fn plan(&self, description: &str, prompt: &str, focus_files: &[String]) -> SwarmPlan {
        let max_parallel = self.max_parallel;
        let mut tasks = Vec::new();
        let files = if focus_files.is_empty() {
            extract_file_paths(prompt)
        } else {
            dedupe(focus_files.to_vec())
        };
        let chinese = contains_cjk(description) || contains_cjk(prompt);
        let intent = SwarmIntent::classify(description, prompt);

        if intent.needs_explorer(&files) {
            if files.is_empty() {
                tasks.push(make_task(
                    SwarmAgentRole::Explorer,
                    if chinese {
                        "定位相关模块和入口".to_string()
                    } else {
                        "Locate relevant modules and entry points".to_string()
                    },
                    explorer_scope_prompt(prompt, &files, chinese),
                    Vec::new(),
                    Vec::new(),
                ));
            } else {
                let reserved = usize::from(intent.needs_reviewer(&files))
                    + usize::from(intent.needs_planner())
                    + usize::from(intent.test);
                let explorer_limit = max_parallel.saturating_sub(reserved).max(1);
                for file in files.iter().take(explorer_limit) {
                    tasks.push(make_task(
                        SwarmAgentRole::Explorer,
                        if chinese {
                            format!("阅读并定位 {file}")
                        } else {
                            format!("Explore {file}")
                        },
                        explorer_prompt(file, prompt, chinese),
                        vec![file.clone()],
                        Vec::new(),
                    ));
                }
            }
        }

        if intent.needs_reviewer(&files) {
            tasks.push(make_task(
                SwarmAgentRole::Reviewer,
                if chinese {
                    "审查关键风险并汇总".to_string()
                } else {
                    "Review risks and summarize".to_string()
                },
                reviewer_prompt(prompt, &files, chinese),
                files.clone(),
                Vec::new(),
            ));
        }

        if intent.needs_planner() {
            tasks.push(make_task(
                SwarmAgentRole::Planner,
                if chinese {
                    if intent.proposal_only {
                        "提出优化方案和取舍".to_string()
                    } else {
                        "规划执行路径和确认点".to_string()
                    }
                } else if intent.proposal_only {
                    "Propose optimization plan and tradeoffs".to_string()
                } else {
                    "Plan execution path and confirmation points".to_string()
                },
                planner_prompt(prompt, chinese),
                files.clone(),
                Vec::new(),
            ));
        }

        if intent.test {
            tasks.push(make_task(
                SwarmAgentRole::Tester,
                if chinese {
                    "运行安全测试并报告".to_string()
                } else {
                    "Run safe tests and report".to_string()
                },
                tester_prompt(prompt, chinese),
                Vec::new(),
                Vec::new(),
            ));
        }

        if intent.write {
            tasks.push(make_task(
                SwarmAgentRole::Worker,
                if chinese {
                    "提出修改补丁方案".to_string()
                } else {
                    "Propose implementation patch".to_string()
                },
                worker_prompt(prompt, &files, chinese),
                files.clone(),
                Vec::new(),
            ));
            tasks.push(make_task(
                SwarmAgentRole::Verifier,
                if chinese {
                    "验证方案和风险".to_string()
                } else {
                    "Verify proposal and risks".to_string()
                },
                verifier_prompt(prompt, chinese),
                files,
                Vec::new(),
            ));
        }

        dedupe_tasks_by_role_and_description(&mut tasks);

        if tasks.is_empty() {
            tasks.push(make_task(
                SwarmAgentRole::Planner,
                if chinese {
                    "规划任务拆解".to_string()
                } else {
                    "Plan task decomposition".to_string()
                },
                planner_prompt(prompt, chinese),
                Vec::new(),
                Vec::new(),
            ));
        }

        let summary = if chinese {
            format!("蜂群任务：{}", truncate(description, 80))
        } else {
            format!("Swarm task: {}", truncate(description, 80))
        };
        let validation_commands =
            fallback_validation_commands(&self.project_root, prompt, intent.write);
        SwarmPlan {
            run_id: format!("swarm-{}", Uuid::new_v4()),
            summary,
            goal: description.trim().to_string(),
            milestones: build_team_milestones(&tasks, chinese),
            acceptance_criteria: default_acceptance_criteria(intent.write, intent.test, chinese),
            risks: default_team_risks(intent.write, chinese),
            tasks,
            max_parallel,
            validation_commands,
        }
    }

    pub async fn plan_hybrid(
        &self,
        description: &str,
        prompt: &str,
        focus_files: &[String],
    ) -> SwarmPlan {
        let fallback = self.plan(description, prompt, focus_files);
        match tokio::time::timeout(
            Duration::from_secs(20),
            self.model_draft_plan(description, prompt, focus_files, &fallback),
        )
        .await
        {
            Ok(Ok(draft)) => self.plan_from_draft(draft, &fallback),
            Ok(Err(err)) => {
                tracing::warn!("swarm model draft plan failed; using fallback: {err}");
                fallback
            }
            Err(err) => {
                tracing::warn!("swarm model draft plan timed out; using fallback: {err}");
                fallback
            }
        }
    }

    async fn model_draft_plan(
        &self,
        description: &str,
        prompt: &str,
        focus_files: &[String],
        fallback: &SwarmPlan,
    ) -> Result<SwarmPlanDraft, crate::deepseek::errors::DeepSeekError> {
        let intent = SwarmIntent::classify(description, prompt);
        let allowed_roles = allowed_roles_for_intent(&intent, !focus_files.is_empty());
        let schema = "Return JSON only with shape {\"goal\": string, \"tasks\": [{\"role\": \"explorer|reviewer|planner|tester|worker|verifier\", \"description\": string, \"prompt\": string, \"focus_files\": [string], \"write_mode\": \"read_only|pending_patch\", \"depends_on\": []}], \"validation_commands\": [string], \"risks\": [string]}. Roles must be complementary and non-duplicated. Leave depends_on empty; dependent scheduling is not supported.";
        let fallback_focus_files = dedupe(
            fallback
                .tasks
                .iter()
                .flat_map(|task| task.focus_files.clone())
                .collect(),
        );
        let effective_focus_files = if focus_files.is_empty() {
            fallback_focus_files
        } else {
            focus_files.to_vec()
        };
        let user = format!(
            "User task:\n{prompt}\n\nLocal safety constraints:\n- allowed_roles: {}\n- max_parallel: {}\n- no direct file writes by subagents\n- worker may only produce pending_patch\n- proposal_only: {}\n- fallback_roles: {}\n- focus_files: {}\n\nGenerate a concise local swarm plan. Every task must have a role-specific description; do not reuse the same task title across roles. If the user writes Chinese, all user-visible descriptions must be Chinese.",
            allowed_roles.join(", "),
            self.max_parallel,
            intent.proposal_only,
            fallback
                .tasks
                .iter()
                .map(|task| task.role.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            effective_focus_files.join(", ")
        );
        let request = ChatRequest {
            model: DeepSeekModel::Flash.to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: Some(ChatMessageContent::Text(schema.to_string())),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
                ChatMessage {
                    role: "user".into(),
                    content: Some(ChatMessageContent::Text(user)),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
            ],
            tools: None,
            thinking: Some(ThinkingConfig::disabled()),
            response_format: Some(ResponseFormat::json_object()),
            stream: false,
            max_tokens: Some(4096),
            temperature: Some(0.0),
        };
        let response = self.client.chat_with_retry(&request).await?;
        parse_json_response(&response)
    }

    fn plan_from_draft(&self, draft: SwarmPlanDraft, fallback: &SwarmPlan) -> SwarmPlan {
        validate_draft_plan(&draft, fallback)
            .map(|mut plan| {
                plan.run_id = format!("swarm-{}", Uuid::new_v4());
                plan.max_parallel = self.max_parallel;
                plan
            })
            .unwrap_or_else(|err| {
                tracing::warn!("swarm draft rejected; using fallback: {err}");
                fallback.clone()
            })
    }

    pub async fn run(
        &self,
        plan: SwarmPlan,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> SwarmResult {
        self.run_with_options(plan, event_tx, SwarmRunOptions::default())
            .await
    }

    pub async fn run_with_options(
        &self,
        plan: SwarmPlan,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        options: SwarmRunOptions,
    ) -> SwarmResult {
        let start = Instant::now();
        send_event(
            event_tx,
            AgentEvent::SwarmStarted {
                run_id: plan.run_id.clone(),
                summary: plan.summary.clone(),
                total: plan.tasks.len(),
            },
        );

        let mut all_results = Vec::new();
        let mut result_tasks = Vec::new();
        let mut cancelled = false;
        for chunk in plan.tasks.chunks(plan.max_parallel) {
            if options
                .cancel_token
                .as_ref()
                .is_some_and(|token| token.load(Ordering::SeqCst))
            {
                cancelled = true;
                for task in chunk {
                    let index = plan
                        .tasks
                        .iter()
                        .position(|candidate| candidate.id == task.id)
                        .unwrap_or(0);
                    send_event(
                        event_tx,
                        AgentEvent::SwarmTaskUpdated {
                            run_id: plan.run_id.clone(),
                            task_id: task.id.clone(),
                            role: task.role.as_str().to_string(),
                            status: SwarmTaskStatus::Cancelled.as_str().to_string(),
                            description: task.description.clone(),
                        },
                    );
                    send_event(
                        event_tx,
                        AgentEvent::PlanStepUpdate {
                            index,
                            total: plan.tasks.len(),
                            description: swarm_step_description(task),
                            status: PlanStepStatus::Failed,
                        },
                    );
                }
                break;
            }
            let batch = ParallelBatch {
                tasks: chunk
                    .iter()
                    .map(|task| {
                        let index = plan
                            .tasks
                            .iter()
                            .position(|candidate| candidate.id == task.id)
                            .unwrap_or(0);
                        send_event(
                            event_tx,
                            AgentEvent::SwarmTaskUpdated {
                                run_id: plan.run_id.clone(),
                                task_id: task.id.clone(),
                                role: task.role.as_str().to_string(),
                                status: SwarmTaskStatus::Running.as_str().to_string(),
                                description: task.description.clone(),
                            },
                        );
                        send_event(
                            event_tx,
                            AgentEvent::PlanStepUpdate {
                                index,
                                total: plan.tasks.len(),
                                description: swarm_step_description(task),
                                status: PlanStepStatus::Running,
                            },
                        );
                        (
                            config_for_role(&task.role, &options),
                            subagent_task_for_swarm(task),
                        )
                    })
                    .collect(),
                independent: true,
                merge_strategy: MergeStrategy::Synthesize,
                shared_context: Some(format!(
                    "Swarm run: {}\nSafety boundary: read-only agents must not write. Workers propose patches only; do not call write_file/edit_file/apply_patch.",
                    plan.run_id
                )),
            };
            let results = self.supervisor.run_parallel(batch, event_tx).await;
            for (task, result) in chunk.iter().zip(results.iter()) {
                let index = plan
                    .tasks
                    .iter()
                    .position(|candidate| candidate.id == task.id)
                    .unwrap_or(0);
                send_event(
                    event_tx,
                    AgentEvent::SwarmTaskUpdated {
                        run_id: plan.run_id.clone(),
                        task_id: task.id.clone(),
                        role: task.role.as_str().to_string(),
                        status: if result.success {
                            SwarmTaskStatus::Done.as_str().to_string()
                        } else {
                            SwarmTaskStatus::Failed.as_str().to_string()
                        },
                        description: task.description.clone(),
                    },
                );
                send_event(
                    event_tx,
                    AgentEvent::PlanStepUpdate {
                        index,
                        total: plan.tasks.len(),
                        description: swarm_step_description(task),
                        status: if result.success {
                            PlanStepStatus::Done
                        } else {
                            PlanStepStatus::Failed
                        },
                    },
                );
            }
            result_tasks.extend(chunk.iter().cloned().zip(results.iter().cloned()));
            all_results.extend(results);
        }

        let tasks_total = plan.tasks.len();
        let tasks_done = all_results.iter().filter(|r| r.success).count();
        let tasks_failed = if cancelled {
            all_results.iter().filter(|r| !r.success).count()
        } else {
            tasks_total.saturating_sub(tasks_done)
        };
        let mut pending_patches = collect_pending_patches(&result_tasks);
        let patch_conflicts = mark_patch_conflicts(&mut pending_patches);
        let success = !cancelled && tasks_failed == 0 && patch_conflicts.is_empty();
        let files_read = dedupe(
            all_results
                .iter()
                .flat_map(|r| r.files_read.clone())
                .collect(),
        );
        let files_written = dedupe(
            all_results
                .iter()
                .flat_map(|r| r.files_written.clone())
                .collect(),
        );
        let outputs: Vec<String> = all_results
            .iter()
            .map(|r| r.format_for_parent(2200))
            .collect();
        let summary = synthesize_swarm_summary(&result_tasks);
        let token_usage = all_results.iter().map(|r| r.token_usage).sum();
        let result = SwarmResult {
            run_id: plan.run_id.clone(),
            success,
            summary,
            tasks_total,
            tasks_done,
            tasks_failed,
            token_usage,
            outputs,
            files_read,
            files_written,
            pending_patches,
            patch_conflicts,
            validation_commands: plan.validation_commands,
            validation_report: None,
            cancelled,
            duration_ms: start.elapsed().as_millis() as u64,
        };

        if options.emit_finished {
            send_event(
                event_tx,
                AgentEvent::SwarmFinished {
                    run_id: plan.run_id,
                    success: result.success,
                    summary: result.summary.clone(),
                },
            );
        }
        result
    }
}

fn make_task(
    role: SwarmAgentRole,
    description: String,
    prompt: String,
    focus_files: Vec<String>,
    depends_on: Vec<String>,
) -> SwarmTask {
    SwarmTask {
        id: format!("task-{}", Uuid::new_v4()),
        role,
        description,
        prompt,
        focus_files,
        depends_on,
        status: SwarmTaskStatus::Pending,
    }
}

fn validate_draft_plan(draft: &SwarmPlanDraft, fallback: &SwarmPlan) -> Result<SwarmPlan, String> {
    if draft.tasks.is_empty() {
        return Err("draft has no tasks".into());
    }
    let fallback_roles: HashSet<SwarmAgentRole> = fallback
        .tasks
        .iter()
        .map(|task| task.role.clone())
        .collect();
    let proposal_only = !fallback_roles.contains(&SwarmAgentRole::Worker);
    let quality_gate = SwarmPlanQualityGate::from_fallback(fallback, proposal_only);
    quality_gate.validate_draft(draft)?;
    let mut seen = HashSet::new();
    let mut seen_descriptions = HashSet::new();
    let mut tasks = Vec::new();
    for draft_task in &draft.tasks {
        if proposal_only && draft_task.role == SwarmAgentRole::Worker {
            return Err("proposal-only task cannot include worker".into());
        }
        if !fallback_roles.contains(&draft_task.role)
            && !matches!(
                draft_task.role,
                SwarmAgentRole::Explorer | SwarmAgentRole::Planner
            )
        {
            return Err(format!(
                "role {} is outside constraints",
                draft_task.role.as_str()
            ));
        }
        let description = draft_task.description.trim();
        let prompt = draft_task.prompt.trim();
        if description.is_empty() || prompt.is_empty() {
            return Err("task description and prompt are required".into());
        }
        let normalized_description = normalize_task_description(description);
        if !seen_descriptions.insert(normalized_description) {
            return Err("duplicate task description".into());
        }
        if !seen.insert((draft_task.role.clone(), description.to_string())) {
            return Err("duplicate role/description task".into());
        }
        let write_mode = draft_task.write_mode.trim();
        if draft_task.role == SwarmAgentRole::Worker {
            if write_mode != "pending_patch" {
                return Err("worker must use pending_patch write_mode".into());
            }
        } else if !write_mode.is_empty() && write_mode != "read_only" {
            return Err("non-worker tasks must be read_only".into());
        }
        if !draft_task.depends_on.is_empty() {
            return Err("swarm draft depends_on is not supported".into());
        }
        tasks.push(make_task(
            draft_task.role.clone(),
            description.to_string(),
            prompt.to_string(),
            draft_task.focus_files.clone(),
            Vec::new(),
        ));
    }
    Ok(SwarmPlan {
        run_id: String::new(),
        summary: if draft.goal.trim().is_empty() {
            fallback.summary.clone()
        } else {
            format!("蜂群任务：{}", truncate(draft.goal.trim(), 80))
        },
        goal: if draft.goal.trim().is_empty() {
            fallback.goal.clone()
        } else {
            draft.goal.trim().to_string()
        },
        milestones: build_team_milestones(&tasks, contains_cjk(&fallback.summary)),
        acceptance_criteria: fallback.acceptance_criteria.clone(),
        risks: if draft.risks.is_empty() {
            fallback.risks.clone()
        } else {
            draft.risks.clone()
        },
        tasks,
        max_parallel: fallback.max_parallel,
        validation_commands: sanitize_validation_commands(&draft.validation_commands)
            .unwrap_or_else(|| fallback.validation_commands.clone()),
    })
}

fn normalize_task_description(description: &str) -> String {
    description
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '：' | ':' | '-' | '—' | '·'))
        .collect::<String>()
        .to_lowercase()
}

struct SwarmPlanQualityGate {
    allowed_roles: HashSet<SwarmAgentRole>,
    proposal_only: bool,
    chinese_visible: bool,
}

impl SwarmPlanQualityGate {
    fn from_fallback(fallback: &SwarmPlan, proposal_only: bool) -> Self {
        let allowed_roles = fallback
            .tasks
            .iter()
            .map(|task| task.role.clone())
            .collect();
        let chinese_visible = contains_cjk(&fallback.summary)
            || fallback
                .tasks
                .iter()
                .any(|task| contains_cjk(&task.description) || contains_cjk(&task.prompt));
        Self {
            allowed_roles,
            proposal_only,
            chinese_visible,
        }
    }

    fn validate_draft(&self, draft: &SwarmPlanDraft) -> Result<(), String> {
        let mut seen_roles = HashSet::new();
        let mut seen_descriptions = HashSet::new();
        let mut seen_prompts = HashSet::new();
        for task in &draft.tasks {
            if self.proposal_only && task.role == SwarmAgentRole::Worker {
                return Err("proposal-only task cannot include worker".into());
            }
            if !self.allowed_roles.contains(&task.role)
                && !matches!(
                    task.role,
                    SwarmAgentRole::Explorer | SwarmAgentRole::Planner
                )
            {
                return Err(format!(
                    "role {} is outside constraints",
                    task.role.as_str()
                ));
            }
            let description = task.description.trim();
            let prompt = task.prompt.trim();
            if self.chinese_visible && !looks_like_chinese_visible_label(description) {
                return Err("Chinese task must use Chinese visible descriptions".into());
            }
            if !seen_descriptions.insert(normalize_task_description(description)) {
                return Err("duplicate task description".into());
            }
            if !seen_prompts.insert(normalize_task_description(prompt)) {
                return Err("duplicate task target".into());
            }
            let role_seen = !seen_roles.insert(task.role.clone());
            if role_seen && task.focus_files.is_empty() {
                return Err(format!(
                    "duplicate role without distinct focus: {}",
                    task.role.as_str()
                ));
            }
        }
        Ok(())
    }
}

fn looks_like_chinese_visible_label(value: &str) -> bool {
    if !contains_cjk(value) {
        return false;
    }
    let ascii_letters = value.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
    ascii_letters <= 12
}

fn allowed_roles_for_intent(intent: &SwarmIntent, has_focus_files: bool) -> Vec<&'static str> {
    let mut roles = Vec::new();
    if intent.needs_explorer(&[]) || has_focus_files {
        roles.push("explorer");
    }
    if intent.needs_reviewer(&[]) || has_focus_files {
        roles.push("reviewer");
    }
    if intent.needs_planner() {
        roles.push("planner");
    }
    if intent.test {
        roles.push("tester");
    }
    if intent.write {
        roles.push("worker");
        roles.push("verifier");
    }
    if roles.is_empty() {
        roles.push("planner");
    }
    roles
}

fn sanitize_validation_commands(commands: &[String]) -> Option<Vec<String>> {
    let safe: Vec<String> = commands
        .iter()
        .map(|command| command.trim())
        .filter(|command| {
            !command.is_empty()
                && command.len() <= 160
                && crate::policy::commands::contains_dangerous_pattern(command).is_none()
        })
        .take(3)
        .map(ToString::to_string)
        .collect();
    (!safe.is_empty()).then_some(safe)
}

fn fallback_validation_commands(
    project_root: &std::path::Path,
    prompt: &str,
    write: bool,
) -> Vec<String> {
    let lower = prompt.to_lowercase();
    if lower.contains("cargo test") {
        vec!["cargo test".to_string()]
    } else if lower.contains("cargo check") {
        vec!["cargo check".to_string()]
    } else if lower.contains("测试") || lower.contains("验证") || lower.contains("test") {
        vec!["cargo test".to_string()]
    } else if write && project_root.join("Cargo.toml").exists() {
        vec!["cargo check".to_string()]
    } else {
        Vec::new()
    }
}

fn build_team_milestones(tasks: &[SwarmTask], chinese: bool) -> Vec<TeamMilestone> {
    tasks
        .iter()
        .map(|task| TeamMilestone {
            title: task.description.clone(),
            acceptance: vec![if chinese {
                format!("{} 输出可审计摘要", task.role.as_str())
            } else {
                format!("{} produces an auditable summary", task.role.as_str())
            }],
        })
        .collect()
}

fn default_acceptance_criteria(write: bool, test: bool, chinese: bool) -> Vec<String> {
    let mut criteria = if chinese {
        vec![
            "每个 agent 只显示用户可见摘要".to_string(),
            "失败原因进入最终报告和事件日志".to_string(),
        ]
    } else {
        vec![
            "Each agent exposes only user-visible summaries".to_string(),
            "Failures are reported in the final output and event log".to_string(),
        ]
    };
    if write {
        criteria.push(if chinese {
            "worker 只产 pending patch，写入仍走主控权限".to_string()
        } else {
            "Workers only produce pending patches; writes stay coordinated".to_string()
        });
    }
    if test {
        criteria.push(if chinese {
            "验证命令结果进入最终报告".to_string()
        } else {
            "Validation command results appear in the final report".to_string()
        });
    }
    criteria
}

fn default_team_risks(write: bool, chinese: bool) -> Vec<String> {
    if write {
        vec![if chinese {
            "写入前必须通过路径、冲突和 protected path 检查".to_string()
        } else {
            "Writes require path, conflict, and protected-path checks".to_string()
        }]
    } else {
        Vec::new()
    }
}

fn dedupe_tasks_by_role_and_description(tasks: &mut Vec<SwarmTask>) {
    let mut seen = HashSet::new();
    tasks.retain(|task| seen.insert((task.role.clone(), task.description.clone())));
}

fn config_for_role(role: &SwarmAgentRole, options: &SwarmRunOptions) -> SubagentConfig {
    let mut config = SubagentConfig::for_type(role.subagent_type());
    config.model = Some(DeepSeekModel::Flash);
    config.max_turns = match role {
        SwarmAgentRole::Tester | SwarmAgentRole::Worker | SwarmAgentRole::Verifier => 12,
        _ => 10,
    };
    config.lane = Some(ExecutionLane::ToolLoopThinking);
    match role {
        SwarmAgentRole::Tester => {
            config.permission_mode = if options.command_requires_approval {
                PermissionMode::Default
            } else {
                PermissionMode::Bypass
            };
            config.allowed_tools = TESTER_TOOLS.iter().map(|s| (*s).to_string()).collect();
        }
        SwarmAgentRole::Worker => {
            config.permission_mode = PermissionMode::ReadOnly;
            config.allowed_tools = READ_ONLY_TOOLS.iter().map(|s| (*s).to_string()).collect();
            config.system_prompt = Some(
                "You are a worker in a local swarm. You may inspect code, but you MUST NOT modify files. If changes are needed, output exactly one git-apply-compatible unified diff between PATCH markers:\nBEGIN_PENDING_PATCH\ndiff --git a/path b/path\n--- a/path\n+++ b/path\n...\nEND_PENDING_PATCH\nAlso summarize the change and tests.".to_string(),
            );
        }
        _ => {
            config.permission_mode = PermissionMode::ReadOnly;
            config.allowed_tools = READ_ONLY_TOOLS.iter().map(|s| (*s).to_string()).collect();
        }
    }
    config
}

fn subagent_task_for_swarm(task: &SwarmTask) -> SubagentTask {
    let chinese =
        contains_cjk(&task.prompt) || task.focus_files.iter().any(|file| contains_cjk(file));
    SubagentTask {
        description: task.description.clone(),
        prompt: task.prompt.clone(),
        context: Some(format!(
            "Swarm role: {}\nStatus output must be concise and user-visible. Do not expose hidden reasoning or internal chain-of-thought.",
            task.role.as_str()
        )),
        focus_files: task.focus_files.clone(),
        expected_output: Some(expected_output_for_role(&task.role, chinese).to_string()),
    }
}

fn expected_output_for_role(role: &SwarmAgentRole, chinese: bool) -> &'static str {
    match (role, chinese) {
        (SwarmAgentRole::Explorer, true) => {
            "只输出：相关文件/入口、关键逻辑链路、可能影响范围、需要其他 agent 关注的风险。"
        }
        (SwarmAgentRole::Reviewer, true) => {
            "只输出：按严重程度排序的真实问题、证据路径、风险解释、建议验证。"
        }
        (SwarmAgentRole::Planner, true) => {
            "只输出：可执行优化方案、优先级、需要确认的边界、后续验证步骤。"
        }
        (SwarmAgentRole::Tester, true) => {
            "只输出：执行的安全命令、结果、失败原因、建议下一步。"
        }
        (SwarmAgentRole::Worker, true) => {
            "只输出：变更摘要、pending patch、测试建议；不要直接写文件。"
        }
        (SwarmAgentRole::Verifier, true) => {
            "只输出：方案可行性、回归风险、建议测试、是否建议应用补丁。"
        }
        (_, true) => "只输出用户可见结论，不要输出思考过程。",
        (SwarmAgentRole::Explorer, false) => {
            "Return relevant files, entry points, logic flow, impact surface, and risks for other agents."
        }
        (SwarmAgentRole::Reviewer, false) => {
            "Return real findings ordered by severity, evidence paths, risk explanation, and validation advice."
        }
        (SwarmAgentRole::Planner, false) => {
            "Return an executable optimization plan, priorities, confirmation boundaries, and validation steps."
        }
        (SwarmAgentRole::Tester, false) => {
            "Return safe commands run, results, failure causes, and recommended next steps."
        }
        (SwarmAgentRole::Worker, false) => {
            "Return change summary, pending patch, and test advice; do not write files directly."
        }
        (SwarmAgentRole::Verifier, false) => {
            "Return feasibility, regression risk, suggested tests, and whether to apply the patch."
        }
        _ => "Return concise user-visible conclusions only. Do not expose hidden reasoning.",
    }
}

fn swarm_step_description(task: &SwarmTask) -> String {
    if task.focus_files.is_empty() {
        format!("agent {} · {}", task.role.as_str(), task.description)
    } else {
        format!(
            "agent {} · {} · 文件 {}",
            task.role.as_str(),
            task.description,
            truncate(&task.focus_files.join(", "), 72)
        )
    }
}

fn explorer_prompt(file: &str, original: &str, chinese: bool) -> String {
    if chinese {
        format!("阅读 `{file}` 以及必要的邻近代码，说明它和任务的关系。只输出结论、关键路径和风险，不要输出思考过程。\n\n用户任务：{original}")
    } else {
        format!("Read `{file}` and nearby code needed for context. Report purpose, task relevance, and risks only.\n\nUser task: {original}")
    }
}

fn explorer_scope_prompt(original: &str, files: &[String], chinese: bool) -> String {
    let file_list = if files.is_empty() {
        if chinese {
            "未指定，先搜索和定位相关模块".to_string()
        } else {
            "not specified; first search and locate relevant modules".to_string()
        }
    } else {
        files.join(", ")
    };
    if chinese {
        format!("先定位任务相关的代码入口、模块边界和数据流。不要修改文件，不要输出思考过程。\n\n关注范围：{file_list}\n用户任务：{original}")
    } else {
        format!("Locate task-relevant entry points, module boundaries, and data flow. Do not modify files or expose reasoning.\n\nScope: {file_list}\nUser task: {original}")
    }
}

fn reviewer_prompt(original: &str, files: &[String], chinese: bool) -> String {
    let file_list = if files.is_empty() {
        "未指定".to_string()
    } else {
        files.join(", ")
    };
    if chinese {
        format!("审查这些范围：{file_list}。找真实 bug、架构风险、缺失测试和安全边界。只给用户可见结论。\n\n用户任务：{original}")
    } else {
        format!("Review scope: {file_list}. Find real bugs, architecture risks, test gaps, and safety concerns.\n\nUser task: {original}")
    }
}

fn tester_prompt(original: &str, chinese: bool) -> String {
    if chinese {
        format!("选择项目内安全测试或检查命令运行，报告命令、结果和失败原因。不要联网，不要写文件。\n\n用户任务：{original}")
    } else {
        format!("Run safe in-project test or check commands. Report commands, results, and failures. Do not use network or write files.\n\nUser task: {original}")
    }
}

fn worker_prompt(original: &str, files: &[String], chinese: bool) -> String {
    let file_list = if files.is_empty() {
        "未指定".to_string()
    } else {
        files.join(", ")
    };
    if chinese {
        format!("基于任务提出具体修改方案和补丁草案。安全要求：不要调用 write_file/edit_file/apply_patch，不要直接落盘。如果需要修改，必须输出一个 git apply 可用的 unified diff，格式如下：\nBEGIN_PENDING_PATCH\ndiff --git a/path b/path\n--- a/path\n+++ b/path\n...\nEND_PENDING_PATCH\n同时输出文件路径、变更摘要、测试建议。\n\n关注文件：{file_list}\n用户任务：{original}")
    } else {
        format!("Propose a concrete implementation patch. Safety: do not call write_file/edit_file/apply_patch and do not modify files. If changes are needed, output one git-apply-compatible unified diff exactly as:\nBEGIN_PENDING_PATCH\ndiff --git a/path b/path\n--- a/path\n+++ b/path\n...\nEND_PENDING_PATCH\nInclude paths, change summary, and tests.\n\nFocus files: {file_list}\nUser task: {original}")
    }
}

fn verifier_prompt(original: &str, chinese: bool) -> String {
    if chinese {
        format!("验证当前方案是否可执行，列出需要用户确认的写入点、测试命令和回滚风险。不要写文件。\n\n用户任务：{original}")
    } else {
        format!("Verify whether the proposal is executable. List write points needing confirmation, test commands, and rollback risks. Do not write files.\n\nUser task: {original}")
    }
}

fn planner_prompt(original: &str, chinese: bool) -> String {
    if chinese {
        format!("把任务拆成本地蜂群执行计划，说明哪些角色可并行、哪些步骤需要确认。不要执行写入。\n\n用户任务：{original}")
    } else {
        format!("Break the task into a local swarm execution plan. State parallel roles and confirmation points. Do not write files.\n\nUser task: {original}")
    }
}

fn synthesize_swarm_summary(results: &[(SwarmTask, SubagentResult)]) -> String {
    let mut out = String::from("蜂群执行摘要\n");
    for (index, (task, result)) in results.iter().enumerate() {
        let status = if result.success { "完成" } else { "失败" };
        let summary = if result.success {
            sanitize_swarm_visible_text(&result.summary)
        } else {
            summarize_swarm_failure(&result.summary)
        };
        out.push_str(&format!(
            "{}. [{}] {} · {}：{}\n",
            index + 1,
            status,
            task.role.as_str(),
            truncate(&task.description, 64),
            truncate(&summary, 220)
        ));
    }
    out
}

fn summarize_swarm_failure(summary: &str) -> String {
    let sanitized = sanitize_swarm_visible_text(summary);
    if sanitized.contains("子任务未形成可用结论") {
        "子任务未形成可用结论，建议缩小范围后重试".to_string()
    } else if sanitized.contains("工具调用参数解析失败")
        || sanitized.contains("流式响应片段不完整")
        || sanitized.contains("工具调用参数不完整")
    {
        "工具调用参数解析失败，已保留审计日志".to_string()
    } else if sanitized.trim().is_empty() {
        "子任务失败，未返回可用摘要".to_string()
    } else {
        sanitized
    }
}

fn sanitize_swarm_visible_text(value: &str) -> String {
    value
        .replace(
            "建议：提高 subagent.max_turns 后重试。",
            "建议：缩小任务范围后重试。",
        )
        .replace("或提高 subagent.max_turns 后重试", "后重试")
        .replace("[Subagent reached max turns limit]", "子任务未形成可用结论")
        .replace("Subagent reached max turns limit", "子任务未形成可用结论")
        .replace("达到轮次上限", "未形成可用结论")
        .replace(
            "reached max turn limit",
            "did not produce a usable conclusion",
        )
        .replace("subagent.max_turns", "任务范围")
        .replace(
            "Subagent error: parse error: EOF while parsing an object",
            "子任务流式响应被拆包截断",
        )
        .replace(
            "Subagent error: parse error: EOF while parsing a value",
            "子任务流式响应被拆包截断",
        )
        .replace("Subagent error: parse error", "工具调用参数解析失败")
        .replace("EOF while parsing a string", "工具调用参数不完整")
        .replace("EOF while parsing an object", "流式响应片段不完整")
        .replace("EOF while parsing a value", "流式响应片段不完整")
}

fn collect_pending_patches(results: &[(SwarmTask, SubagentResult)]) -> Vec<SwarmPendingPatch> {
    results
        .iter()
        .filter(|(task, result)| task.role == SwarmAgentRole::Worker && result.success)
        .filter_map(|(task, result)| {
            let patch = extract_pending_patch(&result.output)?;
            let changed_files = crate::workspace::apply::parse_patch_paths(&patch);
            Some(SwarmPendingPatch {
                task_id: task.id.clone(),
                role: task.role.clone(),
                summary: result.summary.clone(),
                patch,
                changed_files,
                conflict: false,
            })
        })
        .collect()
}

fn mark_patch_conflicts(patches: &mut [SwarmPendingPatch]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut conflicts = HashSet::new();
    for patch in patches.iter() {
        for file in &patch.changed_files {
            if !seen.insert(file.clone()) {
                conflicts.insert(file.clone());
            }
        }
    }
    for patch in patches {
        patch.conflict = patch
            .changed_files
            .iter()
            .any(|file| conflicts.contains(file));
    }
    let mut conflicts: Vec<String> = conflicts.into_iter().collect();
    conflicts.sort();
    conflicts
}

fn extract_pending_patch(output: &str) -> Option<String> {
    // Preferred: the explicit wrapper the worker prompt asks for.
    if let Some(marked) = extract_between(output, "BEGIN_PENDING_PATCH", "END_PENDING_PATCH") {
        if let Some(patch) = normalize_patch(marked) {
            return Some(patch);
        }
    }
    // apply_patch-style envelope.
    if let Some(start) = output.find("*** Begin Patch") {
        let rest = &output[start..];
        if let Some(end_rel) = rest.find("*** End Patch") {
            let end = end_rel + "*** End Patch".len();
            if let Some(patch) = normalize_patch(&rest[..end]) {
                return Some(patch);
            }
        }
    }
    // Fallback: the model dropped the wrapper and emitted a raw patch — very
    // common. Recognize a ```diff / ```patch fenced block first (bounded), then
    // a bare `diff --git` block to end of message. Each candidate is validated
    // by normalize_patch, so a false marker simply falls through.
    for fence in ["```diff", "```patch"] {
        if let Some(open) = output.find(fence) {
            let after = &output[open + fence.len()..];
            if let Some(close) = after.find("```") {
                if let Some(patch) = normalize_patch(&after[..close]) {
                    return Some(patch);
                }
            }
        }
    }
    if let Some(pos) = output.find("diff --git ") {
        let line_start = output[..pos].rfind('\n').map_or(0, |n| n + 1);
        // If the diff was wrapped in a bare ``` fence, stop at the closer so
        // trailing prose doesn't leak into the patch body.
        let body = output[line_start..]
            .split("\n```")
            .next()
            .unwrap_or(&output[line_start..]);
        return normalize_patch(body);
    }
    None
}

fn extract_between<'a>(value: &'a str, start_marker: &str, end_marker: &str) -> Option<&'a str> {
    let start = value.find(start_marker)? + start_marker.len();
    let rest = &value[start..];
    let end = rest.find(end_marker)?;
    Some(&rest[..end])
}

fn normalize_patch(value: &str) -> Option<String> {
    let patch = value
        .trim()
        .trim_start_matches("```patch")
        .trim_start_matches("```diff")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if patch.starts_with("diff --git ")
        || ((patch.starts_with("--- ") || patch.contains("\n--- "))
            && patch.contains("\n+++ ")
            && patch.contains("\n@@"))
    {
        Some(format!("{patch}\n"))
    } else {
        None
    }
}

fn extract_file_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | ',' | ';' | ':' | ')' | '(' | '[' | ']'
            )
        });
        if cleaned.contains('/')
            && (cleaned.ends_with(".rs")
                || cleaned.ends_with(".toml")
                || cleaned.ends_with(".md")
                || cleaned.ends_with(".json")
                || cleaned.ends_with(".ts")
                || cleaned.ends_with(".tsx")
                || cleaned.ends_with(".js")
                || cleaned.ends_with(".py"))
        {
            out.push(cleaned.to_string());
        }
    }
    dedupe(out)
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn is_review_like(lower: &str) -> bool {
    lower.contains("review")
        || lower.contains("审查")
        || lower.contains("检查")
        || lower.contains("架构")
        || lower.contains("风险")
}

fn is_test_like(lower: &str) -> bool {
    lower.contains("cargo test")
        || lower.contains("cargo check")
        || lower.contains("run test")
        || lower.contains("run tests")
        || lower.contains("run the test")
        || lower.contains("run safe test")
        || lower.contains("test suite")
        || lower.contains("tests pass")
        || lower.contains("运行测试")
        || lower.contains("跑测试")
        || lower.contains("测试通过")
        || lower.contains("运行验证")
        || lower.contains("验证命令")
}

fn is_write_like(lower: &str) -> bool {
    lower.contains("implement")
        || lower.contains("fix")
        || lower.contains("修改")
        || lower.contains("修复")
        || lower.contains("实现")
        || lower.contains("优化")
}

fn is_no_write_request(lower: &str) -> bool {
    lower.contains("不要直接写文件")
        || lower.contains("不要写文件")
        || lower.contains("不直接写文件")
        || lower.contains("不写文件")
        || lower.contains("不要直接修改")
        || lower.contains("不要修改")
        || lower.contains("只提出方案")
        || lower.contains("提出优化方案")
        || lower.contains("do not write")
        || lower.contains("don't write")
        || lower.contains("no write")
        || lower.contains("without writing")
        || lower.contains("proposal only")
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn send_event(tx: &mpsc::UnboundedSender<AgentEvent>, event: AgentEvent) {
    if tx.send(event).is_err() {
        tracing::warn!("Agent event channel closed; event dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator() -> SwarmCoordinator {
        SwarmCoordinator::new(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            PathBuf::from("/tmp/project"),
            4,
        )
    }

    #[test]
    fn test_path_mentions_do_not_create_tester_without_run_intent() {
        let plan = coordinator().plan(
            "Verify project structure",
            "Verify whether src/lib.rs, Cargo.toml, and tests/mission_cli_tests.rs exist without editing files.",
            &[],
        );

        assert!(
            !plan
                .tasks
                .iter()
                .any(|task| task.role == SwarmAgentRole::Tester),
            "file paths containing tests/ should not create a tester unless the user asks to run tests"
        );
    }

    #[test]
    fn explicit_test_command_creates_tester() {
        let plan = coordinator().plan(
            "Run checks",
            "Run cargo test after the implementation is complete.",
            &[],
        );

        assert!(
            plan.tasks
                .iter()
                .any(|task| task.role == SwarmAgentRole::Tester),
            "explicit test commands should still create a tester"
        );
    }

    #[test]
    fn complex_chinese_task_gets_worker_and_verifier() {
        let plan = coordinator().plan(
            "修复多文件测试问题",
            "请修改 src/agent/swarm.rs 和 src/tui/app.rs，并运行测试",
            &[],
        );

        assert!(plan.tasks.iter().any(|t| t.role == SwarmAgentRole::Worker));
        assert!(plan
            .tasks
            .iter()
            .any(|t| t.role == SwarmAgentRole::Verifier));
        assert!(plan.tasks.iter().any(|t| t.role == SwarmAgentRole::Tester));
        assert!(plan.summary.starts_with("蜂群任务"));
    }

    #[test]
    fn complex_task_builds_auditable_team_plan() {
        let plan = coordinator().plan(
            "修复团队计划模式并运行验证",
            "请修改 src/agent/swarm.rs 和 src/commands/mod.rs，并运行 cargo check",
            &[],
        );
        let team = plan.team_plan();
        let roles: HashSet<_> = plan.tasks.iter().map(|task| task.role.clone()).collect();

        assert!(team.goal.contains("修复团队计划模式"));
        assert!(!team.milestones.is_empty());
        assert!(team
            .acceptance_criteria
            .iter()
            .any(|item| item.contains("agent")));
        assert!(roles.contains(&SwarmAgentRole::Worker));
        assert!(roles.contains(&SwarmAgentRole::Verifier));
        assert_eq!(team.agent_roles.len(), plan.tasks.len());
        assert_eq!(team.tasks.len(), plan.tasks.len());
        assert!(team
            .tasks
            .iter()
            .any(|task| task.mode == TeamTaskMode::PendingPatch));
        assert!(team
            .tasks
            .iter()
            .any(|task| task.mode == TeamTaskMode::VerifyOnly));
    }

    #[test]
    fn swarm_run_converts_to_canonical_team_state() {
        let plan = coordinator().plan("审查", "审查 src/agent/swarm.rs", &[]);
        let run = SwarmRun {
            run_id: "swarm-1".into(),
            plan,
            status: SwarmTaskStatus::Running,
        };

        let team_run = run.team_run();

        assert_eq!(team_run.id, "swarm-1");
        assert_eq!(team_run.status, AgentRunState::Running);
        assert!(!team_run.plan.tasks.is_empty());
    }

    #[test]
    fn no_write_optimization_review_gets_distinct_read_only_tasks() {
        let plan = coordinator().plan(
            "开蜂群，审查 src/agent/swarm.rs 和 src/agent/task_tool.rs，提出优化方案，不要直接写文件",
            "开蜂群，审查 src/agent/swarm.rs 和 src/agent/task_tool.rs，提出优化方案，不要直接写文件",
            &[],
        );
        let descriptions: Vec<String> = plan
            .tasks
            .iter()
            .map(|task| task.description.clone())
            .collect();
        let unique: HashSet<String> = descriptions.iter().cloned().collect();

        assert_eq!(descriptions.len(), unique.len());
        assert!(plan
            .tasks
            .iter()
            .any(|task| task.role == SwarmAgentRole::Explorer));
        assert!(plan
            .tasks
            .iter()
            .any(|task| task.role == SwarmAgentRole::Reviewer));
        assert!(plan
            .tasks
            .iter()
            .any(|task| task.role == SwarmAgentRole::Planner));
        assert!(!plan
            .tasks
            .iter()
            .any(|task| task.role == SwarmAgentRole::Worker));
    }

    #[test]
    fn proposal_only_without_files_still_gets_complementary_agents() {
        let plan = coordinator().plan(
            "开蜂群，优化 agent 调度，不要写文件，只提出方案",
            "开蜂群，优化 agent 调度，不要写文件，只提出方案",
            &[],
        );
        let roles: HashSet<SwarmAgentRole> =
            plan.tasks.iter().map(|task| task.role.clone()).collect();

        assert!(roles.contains(&SwarmAgentRole::Explorer));
        assert!(roles.contains(&SwarmAgentRole::Reviewer));
        assert!(roles.contains(&SwarmAgentRole::Planner));
        assert!(!roles.contains(&SwarmAgentRole::Worker));
    }

    #[test]
    fn swarm_tasks_have_role_specific_expected_output() {
        let task = make_task(
            SwarmAgentRole::Reviewer,
            "审查风险".to_string(),
            "审查风险".to_string(),
            Vec::new(),
            Vec::new(),
        );
        let subagent_task = subagent_task_for_swarm(&task);

        assert!(subagent_task
            .expected_output
            .as_deref()
            .unwrap_or_default()
            .contains("真实问题"));
    }

    #[test]
    fn swarm_step_description_includes_focus_files_for_confirmation() {
        let task = make_task(
            SwarmAgentRole::Explorer,
            "阅读并定位".to_string(),
            "阅读文件".to_string(),
            vec!["src/agent/swarm.rs".into()],
            Vec::new(),
        );

        let line = swarm_step_description(&task);

        assert!(line.contains("agent explorer"));
        assert!(line.contains("src/agent/swarm.rs"));
    }

    #[test]
    fn valid_model_draft_converts_to_swarm_plan() {
        let fallback = coordinator().plan("审查 src/lib.rs", "审查 src/lib.rs", &[]);
        let draft = SwarmPlanDraft {
            goal: "审查核心入口".into(),
            tasks: vec![SwarmPlanDraftTask {
                role: SwarmAgentRole::Reviewer,
                description: "审查核心风险".into(),
                prompt: "审查 src/lib.rs 的真实风险".into(),
                focus_files: vec!["src/lib.rs".into()],
                write_mode: "read_only".into(),
                depends_on: Vec::new(),
            }],
            validation_commands: vec!["cargo test".into()],
            risks: Vec::new(),
        };

        let plan = validate_draft_plan(&draft, &fallback).expect("valid draft");

        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].role, SwarmAgentRole::Reviewer);
        assert_eq!(plan.validation_commands, vec!["cargo test".to_string()]);
    }

    #[test]
    fn invalid_model_draft_is_rejected() {
        let fallback = coordinator().plan("只提出方案，不要写文件", "只提出方案，不要写文件", &[]);
        let draft = SwarmPlanDraft {
            goal: "bad".into(),
            tasks: vec![SwarmPlanDraftTask {
                role: SwarmAgentRole::Worker,
                description: "直接修改".into(),
                prompt: "直接修改".into(),
                focus_files: Vec::new(),
                write_mode: "pending_patch".into(),
                depends_on: Vec::new(),
            }],
            validation_commands: Vec::new(),
            risks: Vec::new(),
        };

        assert!(validate_draft_plan(&draft, &fallback).is_err());
    }

    #[test]
    fn duplicate_model_draft_descriptions_are_rejected() {
        let fallback = coordinator().plan(
            "开蜂群，审查 src/agent/swarm.rs 和 src/agent/task_tool.rs，不要写文件",
            "开蜂群，审查 src/agent/swarm.rs 和 src/agent/task_tool.rs，不要写文件",
            &[],
        );
        let draft = SwarmPlanDraft {
            goal: "优化多智能体".into(),
            tasks: vec![
                SwarmPlanDraftTask {
                    role: SwarmAgentRole::Explorer,
                    description: "优化多智能体".into(),
                    prompt: "定位多智能体实现".into(),
                    focus_files: vec!["src/agent/swarm.rs".into()],
                    write_mode: "read_only".into(),
                    depends_on: Vec::new(),
                },
                SwarmPlanDraftTask {
                    role: SwarmAgentRole::Reviewer,
                    description: "优化多智能体".into(),
                    prompt: "审查多智能体风险".into(),
                    focus_files: vec!["src/agent/task_tool.rs".into()],
                    write_mode: "read_only".into(),
                    depends_on: Vec::new(),
                },
            ],
            validation_commands: Vec::new(),
            risks: Vec::new(),
        };

        let err = validate_draft_plan(&draft, &fallback).expect_err("duplicate descriptions");
        assert!(err.contains("duplicate task description"));
    }

    #[test]
    fn chinese_model_draft_rejects_english_visible_description() {
        let fallback = coordinator().plan(
            "开蜂群，审查 src/agent/swarm.rs，不要写文件",
            "开蜂群，审查 src/agent/swarm.rs，不要写文件",
            &[],
        );
        let draft = SwarmPlanDraft {
            goal: "审查 swarm".into(),
            tasks: vec![SwarmPlanDraftTask {
                role: SwarmAgentRole::Reviewer,
                description: "Review swarm runtime risks".into(),
                prompt: "审查 swarm runtime 风险".into(),
                focus_files: vec!["src/agent/swarm.rs".into()],
                write_mode: "read_only".into(),
                depends_on: Vec::new(),
            }],
            validation_commands: Vec::new(),
            risks: Vec::new(),
        };

        let err = validate_draft_plan(&draft, &fallback).expect_err("English description");
        assert!(err.contains("Chinese task"));
    }

    #[test]
    fn model_draft_duplicate_targets_are_rejected() {
        let fallback = coordinator().plan(
            "开蜂群，审查 src/agent/swarm.rs，不要写文件",
            "开蜂群，审查 src/agent/swarm.rs，不要写文件",
            &[],
        );
        let draft = SwarmPlanDraft {
            goal: "审查 swarm".into(),
            tasks: vec![
                SwarmPlanDraftTask {
                    role: SwarmAgentRole::Explorer,
                    description: "定位入口".into(),
                    prompt: "审查 src/agent/swarm.rs".into(),
                    focus_files: vec!["src/agent/swarm.rs".into()],
                    write_mode: "read_only".into(),
                    depends_on: Vec::new(),
                },
                SwarmPlanDraftTask {
                    role: SwarmAgentRole::Reviewer,
                    description: "审查风险".into(),
                    prompt: "审查 src/agent/swarm.rs".into(),
                    focus_files: vec!["src/agent/swarm.rs".into()],
                    write_mode: "read_only".into(),
                    depends_on: Vec::new(),
                },
            ],
            validation_commands: Vec::new(),
            risks: Vec::new(),
        };

        let err = validate_draft_plan(&draft, &fallback).expect_err("duplicate target");
        assert!(err.contains("duplicate task target"));
    }

    #[test]
    fn model_draft_dependencies_are_rejected_until_scheduler_support_exists() {
        let fallback = coordinator().plan(
            "开蜂群，审查 src/agent/swarm.rs，不要写文件",
            "开蜂群，审查 src/agent/swarm.rs，不要写文件",
            &[],
        );
        let draft = SwarmPlanDraft {
            goal: "审查 swarm".into(),
            tasks: vec![SwarmPlanDraftTask {
                role: SwarmAgentRole::Explorer,
                description: "定位入口".into(),
                prompt: "审查 src/agent/swarm.rs".into(),
                focus_files: vec!["src/agent/swarm.rs".into()],
                write_mode: "read_only".into(),
                depends_on: vec!["task-previous".into()],
            }],
            validation_commands: Vec::new(),
            risks: Vec::new(),
        };

        let err = validate_draft_plan(&draft, &fallback).expect_err("depends_on unsupported");
        assert!(err.contains("depends_on"));
    }

    #[test]
    fn max_parallel_is_clamped() {
        let c = SwarmCoordinator::new(
            Arc::new(DeepSeekClient::new("test-key".to_string())),
            PathBuf::from("/tmp/project"),
            0,
        );
        let plan = c.plan("review", "review src/a.rs src/b.rs", &[]);
        assert_eq!(plan.max_parallel, 1);
    }

    #[test]
    fn worker_role_is_read_only_and_cannot_write_directly() {
        let config = config_for_role(&SwarmAgentRole::Worker, &SwarmRunOptions::default());
        assert_eq!(config.permission_mode, PermissionMode::ReadOnly);
        assert!(!config.allowed_tools.contains(&"apply_patch".to_string()));
        assert!(!config.allowed_tools.contains(&"write_file".to_string()));
    }

    #[test]
    fn tester_can_request_run_command_through_policy() {
        let config = config_for_role(
            &SwarmAgentRole::Tester,
            &SwarmRunOptions {
                command_requires_approval: true,
                cancel_token: None,
                emit_finished: true,
            },
        );
        assert_eq!(config.permission_mode, PermissionMode::Default);
        assert!(config.allowed_tools.contains(&"run_command".to_string()));
    }

    #[test]
    fn tester_can_bypass_approval_when_config_allows_commands() {
        let config = config_for_role(
            &SwarmAgentRole::Tester,
            &SwarmRunOptions {
                command_requires_approval: false,
                cancel_token: None,
                emit_finished: true,
            },
        );
        assert_eq!(config.permission_mode, PermissionMode::Bypass);
    }

    #[test]
    fn role_allowed_tools_exist_in_real_tool_schema() {
        let real_tools: HashSet<String> = crate::deepseek::tools::standard_tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();
        for role in [
            SwarmAgentRole::Explorer,
            SwarmAgentRole::Reviewer,
            SwarmAgentRole::Planner,
            SwarmAgentRole::Tester,
            SwarmAgentRole::Worker,
            SwarmAgentRole::Verifier,
        ] {
            let config = config_for_role(&role, &SwarmRunOptions::default());
            for tool in config.allowed_tools {
                assert!(
                    real_tools.contains(&tool),
                    "{role:?} allowed missing tool {tool}"
                );
            }
        }
    }

    #[test]
    fn extracts_worker_pending_patch() {
        let output = "说明\nBEGIN_PENDING_PATCH\n*** Begin Patch\n*** Add File: src/new.rs\n+ok\n*** End Patch\nEND_PENDING_PATCH";
        assert!(extract_pending_patch(output).is_none());
    }

    #[test]
    fn extracts_unified_diff_pending_patch() {
        let output = "\
说明
BEGIN_PENDING_PATCH
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
END_PENDING_PATCH";
        let patch = extract_pending_patch(output).expect("pending patch");

        assert!(patch.starts_with("diff --git"));
        assert_eq!(
            crate::workspace::apply::parse_patch_paths(&patch),
            vec!["src/lib.rs".to_string()]
        );
    }

    #[test]
    fn extract_pending_patch_recovers_bare_diff_git_without_wrapper() {
        // Model dropped the BEGIN_PENDING_PATCH wrapper and emitted a raw diff —
        // the common failure mode. It must still be captured, not lost.
        let output = "Here is the change I propose:\n\ndiff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let patch = extract_pending_patch(output).expect("bare diff --git must be recovered");
        assert!(patch.starts_with("diff --git"));
        assert_eq!(
            crate::workspace::apply::parse_patch_paths(&patch),
            vec!["src/lib.rs".to_string()]
        );
    }

    #[test]
    fn extract_pending_patch_recovers_fenced_headerless_diff() {
        // No wrapper, no `diff --git` header — just a ```diff fenced unified diff.
        let output =
            "Proposed patch:\n\n```diff\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n```\n";
        let patch = extract_pending_patch(output).expect("fenced diff must be recovered");
        assert!(patch.contains("+++ b/src/lib.rs"));
        assert!(patch.contains("@@"));
    }

    #[test]
    fn normalize_patch_accepts_headerless_unified_diff() {
        let patch = normalize_patch("--- a/f.rs\n+++ b/f.rs\n@@ -1 +1 @@\n-a\n+b")
            .expect("headerless unified diff must be accepted");
        assert!(patch.contains("--- a/f.rs"));
        assert!(patch.ends_with('\n'));
    }

    #[test]
    fn pending_patch_conflicts_are_marked_by_file() {
        let mut patches = vec![
            SwarmPendingPatch {
                task_id: "a".into(),
                role: SwarmAgentRole::Worker,
                summary: "a".into(),
                patch: String::new(),
                changed_files: vec!["src/lib.rs".into()],
                conflict: false,
            },
            SwarmPendingPatch {
                task_id: "b".into(),
                role: SwarmAgentRole::Worker,
                summary: "b".into(),
                patch: String::new(),
                changed_files: vec!["src/lib.rs".into()],
                conflict: false,
            },
        ];

        let conflicts = mark_patch_conflicts(&mut patches);

        assert_eq!(conflicts, vec!["src/lib.rs".to_string()]);
        assert!(patches.iter().all(|patch| patch.conflict));
    }

    #[test]
    fn format_for_parent_includes_subagent_outputs() {
        let result = SwarmResult {
            run_id: "swarm-out".into(),
            success: true,
            summary: "蜂群执行摘要\n1. [完成] explorer · auth：ok".into(),
            tasks_total: 1,
            tasks_done: 1,
            tasks_failed: 0,
            token_usage: 256,
            outputs: vec![
                "## Subagent Result: ✅ Success\n\nsession tokens live in src/auth/session.rs"
                    .into(),
            ],
            files_read: vec![],
            files_written: vec![],
            pending_patches: vec![],
            patch_conflicts: vec![],
            validation_commands: vec![],
            validation_report: None,
            cancelled: false,
            duration_ms: 1,
        };

        let formatted = result.format_for_parent();

        // The parent must receive each subagent's detailed output, not just the
        // one-line index in `summary`.
        assert!(
            formatted.contains("session tokens live in src/auth/session.rs"),
            "parent-facing swarm output must include subagent outputs, got:\n{formatted}"
        );
    }

    #[test]
    fn user_visible_swarm_result_hides_internal_failures() {
        let result = SwarmResult {
            run_id: "swarm-1".into(),
            success: false,
            summary: "蜂群执行摘要\n1. [失败] [Subagent reached max turns limit]\n2. [失败] Subagent error: parse error: EOF while parsing a string".into(),
            tasks_total: 2,
            tasks_done: 0,
            tasks_failed: 2,
            token_usage: 0,
            outputs: vec![],
            files_read: vec!["src/agent/swarm.rs".into()],
            files_written: vec![],
            pending_patches: vec![],
            patch_conflicts: vec![],
            validation_commands: vec![],
            validation_report: None,
            cancelled: false,
            duration_ms: 100,
        };

        let visible = result.format_for_user(None, true);

        assert!(visible.contains("蜂群任务未完成"));
        assert!(visible.contains("结论："));
        assert!(visible.contains("发现："));
        assert!(visible.contains("变更："));
        assert!(visible.contains("验证："));
        assert!(visible.contains("下一步："));
        assert!(visible.contains("子任务未形成可用结论"));
        assert!(visible.contains("工具调用参数解析失败"));
        assert!(!visible.contains("轮次"));
        assert!(!visible.contains("max_turns"));
        assert!(!visible.contains("Subagent reached max turns limit"));
        assert!(!visible.contains("EOF while parsing"));
    }

    #[test]
    fn swarm_event_json_roundtrips() {
        let event = SwarmEvent::TaskUpdated {
            run_id: "swarm-1".to_string(),
            task_id: "task-1".to_string(),
            role: SwarmAgentRole::Explorer,
            status: SwarmTaskStatus::Running,
            description: "读取代码".to_string(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let decoded: SwarmEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, event);
    }
}

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AgentListPayload {
    pub project_root: String,
    pub agents: Vec<AgentListItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentListItem {
    pub name: String,
    pub source: AgentSource,
    pub description: String,
    pub subagent_type: String,
    pub permission_mode: String,
    pub model: Option<String>,
    pub max_turns: u32,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSource {
    BuiltIn,
    /// Custom agent under `.octocode/agents/`.
    Project,
    /// Custom agent under Claude Code's `.claude/agents/`.
    Claude,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentShowPayload {
    #[serde(flatten)]
    pub item: AgentListItem,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRunPayload {
    pub agent: String,
    pub task: String,
    pub dry_run: bool,
    pub success: bool,
    pub summary: String,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<AgentRunDryRunPlan>,
    pub tool_calls_used: Vec<String>,
    pub files_read: Vec<String>,
    pub files_written: Vec<String>,
    pub duration_ms: u64,
    pub token_usage: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<AgentFailureReasonPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<AgentWorktreePayload>,
    pub error: Option<String>,
    pub approval_denials: Vec<AgentApprovalDenialPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorktreePayload {
    pub path: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRunDryRunPlan {
    pub project_root: String,
    pub focus_files: Vec<String>,
    pub model: Option<String>,
    pub max_turns: u32,
    pub permission_mode: String,
    pub allowed_tools: Vec<String>,
    pub isolation: String,
    pub would_request_api_key: bool,
    pub network_required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentFailureReasonPayload {
    pub code: String,
    pub message: String,
    pub hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns_used: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentApprovalDenialPayload {
    pub agent_id: String,
    pub tool: String,
    pub reason: String,
    pub arguments: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentValidationPayload {
    pub project_root: String,
    pub reports: Vec<AgentValidationReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentValidationReport {
    pub name: String,
    pub source: AgentSource,
    pub path: Option<String>,
    pub valid: bool,
    pub errors: Vec<AgentValidationIssue>,
    pub warnings: Vec<AgentValidationIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentValidationIssue {
    pub code: String,
    pub message: String,
}

//! Stable tool runtime contracts.
//!
//! This module names the shared path that CLI, TUI, subagents, MCP, tasks, and
//! missions should converge on. It is intentionally lightweight: existing
//! dispatch code can adopt these contracts incrementally while tests pin the
//! policy, approval, result, and audit shape.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::policy::{self, PolicyDecision, ToolCallSource};
use crate::storage::config::PolicyConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: Option<String>,
    pub source: ToolCallSource,
    pub tool: String,
    pub arguments: String,
}

impl ToolCall {
    #[must_use]
    pub fn new(tool: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            id: None,
            source: ToolCallSource::Main,
            tool: tool.into(),
            arguments: arguments.into(),
        }
    }

    #[must_use]
    pub fn with_source(mut self, source: ToolCallSource) -> Self {
        self.source = source;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolBackend {
    LocalDispatch,
    Mcp,
    Task,
    Mission,
}

impl ToolBackend {
    #[must_use]
    pub const fn for_source(source: ToolCallSource) -> Self {
        match source {
            ToolCallSource::Main | ToolCallSource::Subagent => Self::LocalDispatch,
            ToolCallSource::Mcp => Self::Mcp,
            ToolCallSource::Task => Self::Task,
            ToolCallSource::Mission => Self::Mission,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub source: ToolCallSource,
    pub tool: String,
    pub risk: String,
    pub reason: String,
    pub details: String,
}

impl ApprovalRequest {
    #[must_use]
    pub fn from_decision(tool: &str, decision: &PolicyDecision) -> Self {
        Self {
            source: decision.source,
            tool: tool.to_string(),
            risk: decision.display.risk_level.to_string(),
            reason: decision.reason.clone(),
            details: decision.display.details.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalOutcome {
    NotRequired,
    Approved,
    Denied { reason: String },
}

impl ApprovalOutcome {
    #[must_use]
    pub fn denied(reason: impl Into<String>) -> Self {
        Self::Denied {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Denied { reason } => Some(reason),
            Self::NotRequired | Self::Approved => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OutputOriginalBytes {
    pub stdout: u64,
    pub stderr: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultMetadata {
    pub success: bool,
    pub summary: String,
    pub duration_ms: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub original_bytes: OutputOriginalBytes,
    pub changed_files: Vec<String>,
    pub error_code: Option<String>,
}

impl ToolResultMetadata {
    #[must_use]
    pub fn success(summary: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            success: true,
            summary: summary.into(),
            duration_ms,
            stdout_truncated: false,
            stderr_truncated: false,
            original_bytes: OutputOriginalBytes::default(),
            changed_files: Vec::new(),
            error_code: None,
        }
    }

    #[must_use]
    pub fn failure(
        summary: impl Into<String>,
        duration_ms: u64,
        error_code: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            summary: summary.into(),
            duration_ms,
            stdout_truncated: false,
            stderr_truncated: false,
            original_bytes: OutputOriginalBytes::default(),
            changed_files: Vec::new(),
            error_code: Some(error_code.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRuntimeEvent {
    pub source: ToolCallSource,
    pub backend: ToolBackend,
    pub tool: String,
    pub arguments_summary: String,
    pub policy_action: String,
    pub policy_reason: String,
    pub approval_reason: Option<String>,
    pub duration_ms: u64,
    pub success: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub original_bytes: OutputOriginalBytes,
    pub changed_files: Vec<String>,
    pub error_code: Option<String>,
}

pub struct ToolRuntime {
    project_root: PathBuf,
    policy: PolicyConfig,
}

impl ToolRuntime {
    #[must_use]
    pub fn new(project_root: impl Into<PathBuf>, policy: PolicyConfig) -> Self {
        Self {
            project_root: project_root.into(),
            policy,
        }
    }

    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    #[must_use]
    pub fn evaluate_policy(&self, call: &ToolCall) -> PolicyDecision {
        policy::evaluate_tool(
            &call.tool,
            &call.arguments,
            &self.project_root,
            &self.policy,
        )
        .with_source(call.source)
    }

    #[must_use]
    pub fn audit_event(
        &self,
        call: &ToolCall,
        decision: &PolicyDecision,
        approval: &ApprovalOutcome,
        result: &ToolResultMetadata,
    ) -> ToolRuntimeEvent {
        ToolRuntimeEvent {
            source: call.source,
            backend: ToolBackend::for_source(call.source),
            tool: call.tool.clone(),
            arguments_summary: summarize_arguments(&call.arguments),
            policy_action: format!("{:?}", decision.action),
            policy_reason: decision.reason.clone(),
            approval_reason: approval.reason().map(str::to_string),
            duration_ms: result.duration_ms,
            success: result.success,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
            original_bytes: result.original_bytes.clone(),
            changed_files: result.changed_files.clone(),
            error_code: result.error_code.clone(),
        }
    }
}

fn summarize_arguments(arguments: &str) -> String {
    let redacted = crate::policy::redact_all(arguments);
    if redacted.chars().count() <= 240 {
        return redacted;
    }
    let mut summary: String = redacted.chars().take(239).collect();
    summary.push('…');
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyAction;

    #[test]
    fn tool_runtime_evaluates_policy_with_source() {
        let root = tempfile::tempdir().expect("tempdir");
        let runtime = ToolRuntime::new(root.path(), PolicyConfig::default());
        let call = ToolCall::new(
            "run_command",
            serde_json::json!({ "command": "rm -rf /" }).to_string(),
        )
        .with_source(ToolCallSource::Subagent);

        let decision = runtime.evaluate_policy(&call);

        assert_eq!(decision.source, ToolCallSource::Subagent);
        assert_eq!(decision.action, PolicyAction::Deny);
    }

    #[test]
    fn tool_runtime_event_carries_stable_audit_metadata() {
        let root = tempfile::tempdir().expect("tempdir");
        let runtime = ToolRuntime::new(root.path(), PolicyConfig::default());
        let call = ToolCall::new(
            "run_command",
            serde_json::json!({ "command": "echo sk-test-secret" }).to_string(),
        );
        let decision = runtime.evaluate_policy(&call);
        let mut result = ToolResultMetadata::failure("blocked", 12, "policy-denied");
        result.stdout_truncated = true;
        result.original_bytes.stdout = 2_000_000;

        let event = runtime.audit_event(
            &call,
            &decision,
            &ApprovalOutcome::denied("policy=deny"),
            &result,
        );

        assert_eq!(event.source, ToolCallSource::Main);
        assert_eq!(event.backend, ToolBackend::LocalDispatch);
        assert_eq!(event.tool, "run_command");
        assert_eq!(event.approval_reason.as_deref(), Some("policy=deny"));
        assert!(event.stdout_truncated);
        assert_eq!(event.original_bytes.stdout, 2_000_000);
        assert!(!event.arguments_summary.contains("sk-test-secret"));
    }
}

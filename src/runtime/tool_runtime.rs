//! Stable tool runtime contracts.
//!
//! This module names the shared path that CLI, TUI, subagents, MCP, tasks, and
//! missions converge on for policy, approval, hooks, dispatch, and audit shape.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::deepseek::{SessionId, ToolCall as DeepSeekToolCall, ToolResultRecord, TurnId};
use crate::hooks::{HookEvent, HookPayload, HookRunSummary};
use crate::policy::{self, PolicyDecision, ToolCallSource};
use crate::storage::config::PolicyConfig;
use crate::storage::{EventLogStore, SessionEvent, SessionEventKind};
use crate::tools::backend::{
    ToolBackend as LocalToolBackendTrait, ToolExecutionContext, ToolExecutionResult,
};
use crate::tools::dispatch::ToolDispatchConfig;

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

impl From<&DeepSeekToolCall> for ToolCall {
    fn from(call: &DeepSeekToolCall) -> Self {
        Self {
            id: Some(call.id.clone()),
            source: ToolCallSource::Main,
            tool: call.function.name.clone(),
            arguments: call.function.arguments.clone(),
        }
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

    #[must_use]
    pub fn approved(&self) -> bool {
        matches!(self, Self::NotRequired | Self::Approved)
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

pub type ApprovalFuture<'a> = Pin<Box<dyn Future<Output = ApprovalOutcome> + Send + 'a>>;
pub type ToolRuntimeBackendFuture<'a> =
    Pin<Box<dyn Future<Output = ToolExecutionResult> + Send + 'a>>;

pub trait ApprovalResolver {
    fn resolve<'a>(
        &'a mut self,
        call: &'a ToolCall,
        decision: &'a PolicyDecision,
    ) -> ApprovalFuture<'a>;
}

pub trait ToolRuntimeBackend {
    fn execute<'a>(
        &'a mut self,
        call: &'a DeepSeekToolCall,
        context: &'a ToolExecutionContext,
    ) -> ToolRuntimeBackendFuture<'a>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalDispatchRuntimeBackend;

impl ToolRuntimeBackend for LocalDispatchRuntimeBackend {
    fn execute<'a>(
        &'a mut self,
        call: &'a DeepSeekToolCall,
        context: &'a ToolExecutionContext,
    ) -> ToolRuntimeBackendFuture<'a> {
        Box::pin(async move {
            crate::tools::backend::LocalToolBackend
                .execute(call, context)
                .await
        })
    }
}

pub struct McpRegistryRuntimeBackend<'a> {
    pub registry: &'a mut crate::mcp::registry::McpRegistry,
}

impl ToolRuntimeBackend for McpRegistryRuntimeBackend<'_> {
    fn execute<'a>(
        &'a mut self,
        call: &'a DeepSeekToolCall,
        _context: &'a ToolExecutionContext,
    ) -> ToolRuntimeBackendFuture<'a> {
        Box::pin(async move {
            let start = std::time::Instant::now();
            let (content, success) =
                match crate::mcp::registry::parse_mcp_tool_arguments(&call.function.arguments) {
                    Ok(args) => match self.registry.call_tool(&call.function.name, args).await {
                        Ok(text) => (text, true),
                        Err(error) => (error.to_string(), false),
                    },
                    Err(error) => (error.to_string(), false),
                };
            ToolExecutionResult {
                success,
                summary: crate::agent::utils::truncate_for_summary(&content, 200),
                content,
                changed_files: Vec::new(),
                duration_ms: start.elapsed().as_millis() as u64,
            }
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysApproveResolver;

impl ApprovalResolver for AlwaysApproveResolver {
    fn resolve<'a>(
        &'a mut self,
        _call: &'a ToolCall,
        _decision: &'a PolicyDecision,
    ) -> ApprovalFuture<'a> {
        Box::pin(async { ApprovalOutcome::Approved })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysDenyResolver;

impl ApprovalResolver for AlwaysDenyResolver {
    fn resolve<'a>(
        &'a mut self,
        _call: &'a ToolCall,
        decision: &'a PolicyDecision,
    ) -> ApprovalFuture<'a> {
        Box::pin(async move { ApprovalOutcome::denied(decision.reason.clone()) })
    }
}

pub struct ToolRuntimeContext<'a> {
    pub session_label: String,
    pub session_id: Option<SessionId>,
    pub turn_id: Option<TurnId>,
    pub hooks_config: Option<&'a crate::storage::config::HooksConfig>,
    pub event_log_store: Option<&'a EventLogStore>,
    pub dispatch_config: ToolDispatchConfig,
    pub hook_tool_name: Option<String>,
    pub mcp_metadata: Option<&'a crate::mcp::registry::McpToolApprovalMetadata>,
}

impl<'a> ToolRuntimeContext<'a> {
    #[must_use]
    pub fn new(session_label: impl Into<String>, dispatch_config: ToolDispatchConfig) -> Self {
        Self {
            session_label: session_label.into(),
            session_id: None,
            turn_id: None,
            hooks_config: None,
            event_log_store: None,
            dispatch_config,
            hook_tool_name: None,
            mcp_metadata: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolRuntimeOutcome {
    pub decision: PolicyDecision,
    pub approval: ApprovalOutcome,
    pub backend_result: ToolExecutionResult,
    pub result_record: ToolResultRecord,
    pub metadata: ToolResultMetadata,
    pub audit_event: ToolRuntimeEvent,
    pub hook_summaries: Vec<HookRunSummary>,
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
    pub fn evaluate_policy_with_context(
        &self,
        call: &ToolCall,
        context: &ToolRuntimeContext<'_>,
    ) -> PolicyDecision {
        if let Some(metadata) = context.mcp_metadata {
            policy::evaluate_mcp_tool(metadata, &call.arguments, &self.policy)
                .with_source(call.source)
        } else {
            self.evaluate_policy(call)
        }
    }

    pub async fn execute<B, R>(
        &self,
        deepseek_call: &DeepSeekToolCall,
        source: ToolCallSource,
        context: ToolRuntimeContext<'_>,
        approval_resolver: &mut R,
        backend: &mut B,
    ) -> ToolRuntimeOutcome
    where
        B: ToolRuntimeBackend + Send,
        R: ApprovalResolver + Send,
    {
        let call = ToolCall::from(deepseek_call).with_source(source);
        let decision = self.evaluate_policy_with_context(&call, &context);
        let mut hook_summaries = Vec::new();

        match decision.action {
            policy::PolicyAction::Deny => {
                let reason = decision.reason.clone();
                return self.blocked_outcome(
                    &call,
                    deepseek_call,
                    decision,
                    ApprovalOutcome::denied("policy=deny"),
                    format!("Blocked: {reason}"),
                    "policy-denied",
                    hook_summaries,
                );
            }
            policy::PolicyAction::Allow => {}
            policy::PolicyAction::AskOnce | policy::PolicyAction::AskSession => {
                let approval = approval_resolver.resolve(&call, &decision).await;
                if !approval.approved() {
                    let reason = approval
                        .reason()
                        .map(str::to_string)
                        .unwrap_or_else(|| "approval denied".to_string());
                    return self.blocked_outcome(
                        &call,
                        deepseek_call,
                        decision,
                        approval,
                        reason,
                        "approval-denied",
                        hook_summaries,
                    );
                }
                return self
                    .execute_after_approval(
                        deepseek_call,
                        &call,
                        decision,
                        ApprovalOutcome::Approved,
                        context,
                        backend,
                        &mut hook_summaries,
                    )
                    .await;
            }
        }

        self.execute_after_approval(
            deepseek_call,
            &call,
            decision,
            ApprovalOutcome::NotRequired,
            context,
            backend,
            &mut hook_summaries,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_after_approval<B>(
        &self,
        deepseek_call: &DeepSeekToolCall,
        call: &ToolCall,
        decision: PolicyDecision,
        approval: ApprovalOutcome,
        context: ToolRuntimeContext<'_>,
        backend: &mut B,
        hook_summaries: &mut Vec<HookRunSummary>,
    ) -> ToolRuntimeOutcome
    where
        B: ToolRuntimeBackend + Send,
    {
        if let Some(pre_hooks) = self
            .run_hooks(HookEvent::PreToolUse, call, &context, None)
            .await
        {
            self.record_hook_summary(&context, &pre_hooks);
            let pre_success = pre_hooks.success();
            let pre_brief = pre_hooks.brief();
            hook_summaries.push(pre_hooks);
            if !pre_success {
                return self.blocked_outcome(
                    call,
                    deepseek_call,
                    decision,
                    approval,
                    format!("Blocked by pre_tool hook: {pre_brief}"),
                    "pre-hook-denied",
                    hook_summaries.clone(),
                );
            }
        }

        let backend_context = ToolExecutionContext {
            project_root: self.project_root.clone(),
            dispatch_config: context.dispatch_config.clone(),
        };
        let backend_result = backend.execute(deepseek_call, &backend_context).await;

        if let Some(post_hooks) = self
            .run_hooks(
                HookEvent::PostToolUse,
                call,
                &context,
                Some(&backend_result),
            )
            .await
        {
            self.record_hook_summary(&context, &post_hooks);
            hook_summaries.push(post_hooks);
        }

        self.outcome_from_backend_result(
            call,
            deepseek_call,
            decision,
            approval,
            backend_result,
            hook_summaries.clone(),
        )
    }

    async fn run_hooks(
        &self,
        event: HookEvent,
        call: &ToolCall,
        context: &ToolRuntimeContext<'_>,
        result: Option<&ToolExecutionResult>,
    ) -> Option<HookRunSummary> {
        let hooks_config = context.hooks_config?;
        let mut payload = HookPayload::new(
            event,
            context.session_label.clone(),
            self.project_root.clone(),
        );
        payload.turn_id = context.turn_id.map(|turn_id| turn_id.to_string());
        payload.tool_call_id = call.id.clone();
        payload.tool_name = Some(
            context
                .hook_tool_name
                .clone()
                .unwrap_or_else(|| call.tool.clone()),
        );
        payload.arguments = Some(call.arguments.clone());
        if let Some(result) = result {
            payload.success = Some(result.success);
            payload.summary = Some(result.summary.clone());
            payload.duration_ms = Some(result.duration_ms);
            payload.changed_files = result.changed_files.clone();
        }
        crate::hooks::run_configured_hooks(
            event,
            hooks_config,
            &payload,
            &self.project_root,
            self.policy.command_timeout_seconds,
        )
        .await
    }

    fn record_hook_summary(&self, context: &ToolRuntimeContext<'_>, summary: &HookRunSummary) {
        let (Some(store), Some(session_id)) = (context.event_log_store, context.session_id) else {
            return;
        };
        let event = SessionEvent::new(
            session_id,
            context.turn_id,
            SessionEventKind::HookExecuted {
                event: summary.event.as_str().to_string(),
                success: summary.success(),
                summary: summary.brief(),
                command_count: summary.outcomes.len(),
            },
        );
        if let Err(err) = store.append(&self.project_root, &event) {
            tracing::warn!("failed to append hook event: {err}");
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn blocked_outcome(
        &self,
        call: &ToolCall,
        deepseek_call: &DeepSeekToolCall,
        decision: PolicyDecision,
        approval: ApprovalOutcome,
        message: String,
        error_code: &'static str,
        hook_summaries: Vec<HookRunSummary>,
    ) -> ToolRuntimeOutcome {
        let backend_result = ToolExecutionResult {
            success: false,
            summary: crate::agent::utils::truncate_for_summary(&message, 200),
            content: message.clone(),
            changed_files: Vec::new(),
            duration_ms: 0,
        };
        let mut metadata = ToolResultMetadata::failure(
            backend_result.summary.clone(),
            backend_result.duration_ms,
            error_code,
        );
        metadata.changed_files = backend_result.changed_files.clone();
        let audit_event = self.audit_event(call, &decision, &approval, &metadata);
        ToolRuntimeOutcome {
            result_record: ToolResultRecord {
                tool_call_id: deepseek_call.id.clone(),
                name: deepseek_call.function.name.clone(),
                result: message,
                is_error: true,
            },
            decision,
            approval,
            backend_result,
            metadata,
            audit_event,
            hook_summaries,
        }
    }

    fn outcome_from_backend_result(
        &self,
        call: &ToolCall,
        deepseek_call: &DeepSeekToolCall,
        decision: PolicyDecision,
        approval: ApprovalOutcome,
        backend_result: ToolExecutionResult,
        hook_summaries: Vec<HookRunSummary>,
    ) -> ToolRuntimeOutcome {
        let mut metadata = if backend_result.success {
            ToolResultMetadata::success(backend_result.summary.clone(), backend_result.duration_ms)
        } else {
            ToolResultMetadata::failure(
                backend_result.summary.clone(),
                backend_result.duration_ms,
                "tool-error",
            )
        };
        metadata.changed_files = backend_result.changed_files.clone();
        let audit_event = self.audit_event(call, &decision, &approval, &metadata);
        ToolRuntimeOutcome {
            result_record: ToolResultRecord {
                tool_call_id: deepseek_call.id.clone(),
                name: deepseek_call.function.name.clone(),
                result: backend_result.content.clone(),
                is_error: !backend_result.success,
            },
            decision,
            approval,
            backend_result,
            metadata,
            audit_event,
            hook_summaries,
        }
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
    use crate::deepseek::ToolCallFunction;
    use crate::policy::PolicyAction;

    fn deepseek_call(name: &str, arguments: serde_json::Value) -> DeepSeekToolCall {
        DeepSeekToolCall {
            id: format!("call-{name}"),
            call_type: "function".to_string(),
            function: ToolCallFunction {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
        }
    }

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

    #[tokio::test]
    async fn execute_denies_before_backend_runs() {
        #[derive(Default)]
        struct CountingBackend {
            calls: usize,
        }
        impl ToolRuntimeBackend for CountingBackend {
            fn execute<'a>(
                &'a mut self,
                _call: &'a DeepSeekToolCall,
                _context: &'a ToolExecutionContext,
            ) -> ToolRuntimeBackendFuture<'a> {
                Box::pin(async move {
                    self.calls += 1;
                    ToolExecutionResult {
                        success: true,
                        summary: "ran".to_string(),
                        content: "ran".to_string(),
                        changed_files: Vec::new(),
                        duration_ms: 1,
                    }
                })
            }
        }

        let root = tempfile::tempdir().expect("tempdir");
        let runtime = ToolRuntime::new(root.path(), PolicyConfig::default());
        let call = deepseek_call("does_not_exist", serde_json::json!({}));
        let context = ToolRuntimeContext::new("test-session", ToolDispatchConfig::default());
        let mut resolver = AlwaysApproveResolver;
        let mut backend = CountingBackend::default();

        let outcome = runtime
            .execute(
                &call,
                ToolCallSource::Main,
                context,
                &mut resolver,
                &mut backend,
            )
            .await;

        assert!(outcome.result_record.is_error);
        assert_eq!(backend.calls, 0);
        assert_eq!(outcome.decision.action, PolicyAction::Deny);
    }

    #[tokio::test]
    async fn execute_runs_approval_before_backend() {
        #[derive(Default)]
        struct CountingBackend {
            calls: usize,
        }
        impl ToolRuntimeBackend for CountingBackend {
            fn execute<'a>(
                &'a mut self,
                _call: &'a DeepSeekToolCall,
                _context: &'a ToolExecutionContext,
            ) -> ToolRuntimeBackendFuture<'a> {
                Box::pin(async move {
                    self.calls += 1;
                    ToolExecutionResult {
                        success: true,
                        summary: "staged".to_string(),
                        content: "staged".to_string(),
                        changed_files: Vec::new(),
                        duration_ms: 1,
                    }
                })
            }
        }

        let root = tempfile::tempdir().expect("tempdir");
        let runtime = ToolRuntime::new(root.path(), PolicyConfig::default());
        let call = deepseek_call("git_add", serde_json::json!({ "paths": ["src/lib.rs"] }));
        let context = ToolRuntimeContext::new("test-session", ToolDispatchConfig::default());
        let mut resolver = AlwaysApproveResolver;
        let mut backend = CountingBackend::default();

        let outcome = runtime
            .execute(
                &call,
                ToolCallSource::Main,
                context,
                &mut resolver,
                &mut backend,
            )
            .await;

        assert!(!outcome.result_record.is_error);
        assert_eq!(backend.calls, 1);
        assert_eq!(outcome.approval, ApprovalOutcome::Approved);
    }
}

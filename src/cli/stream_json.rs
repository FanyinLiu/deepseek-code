use serde_json::json;

use crate::agent::orchestrator::{AgentEvent, PlanStepStatus};
use crate::deepseek::{CacheUsage, FinishReason, Usage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnOutputFormat {
    Text,
    Json,
    StreamJson,
}

impl TurnOutputFormat {
    #[must_use]
    pub fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }

    #[must_use]
    pub fn is_stream_json(self) -> bool {
        matches!(self, Self::StreamJson)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FinalJsonCollector {
    session_id: Option<String>,
    final_message: String,
    tool_calls: Vec<FinalToolCall>,
    usage: Option<Usage>,
    total_tokens: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct FinalToolCall {
    id: Option<String>,
    name: String,
    arguments: Option<String>,
    success: Option<bool>,
    summary: Option<String>,
}

impl FinalJsonCollector {
    #[must_use]
    pub fn new(session_id: impl std::fmt::Display) -> Self {
        Self {
            session_id: Some(session_id.to_string()),
            ..Self::default()
        }
    }

    pub fn observe(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::ContentDelta(text) => self.final_message.push_str(text),
            AgentEvent::ToolStarted {
                tool_call_id,
                tool_name,
                arguments,
            } => self.tool_calls.push(FinalToolCall {
                id: Some(tool_call_id.clone()),
                name: tool_name.clone(),
                arguments: Some(arguments.clone()),
                success: None,
                summary: None,
            }),
            AgentEvent::ToolExecuted {
                tool_name,
                success,
                summary,
            } => self.record_tool_result(tool_name, *success, summary),
            AgentEvent::UserQuestionRequested { summary, .. } => {
                self.final_message.push_str(summary);
            }
            AgentEvent::StreamDone {
                usage: Some(usage), ..
            } => self.usage = Some(usage.clone()),
            AgentEvent::Error(message) => self.record_error(message.clone()),
            AgentEvent::TurnComplete {
                session_id,
                total_tokens,
            } => {
                self.session_id = Some(session_id.to_string());
                self.total_tokens = Some(*total_tokens);
            }
            _ => {}
        }
    }

    pub fn record_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        if !error.trim().is_empty() && self.error.is_none() {
            self.error = Some(error);
        }
    }

    #[must_use]
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn print_final(&self) {
        println!(
            "{}",
            serde_json::to_string(&self.final_value()).unwrap_or_else(|_| "{}".to_string())
        );
    }

    #[must_use]
    pub fn final_value(&self) -> serde_json::Value {
        json!({
            "status": if self.error.is_some() { "error" } else { "ok" },
            "session_id": self.session_id,
            "final_message": self.final_message,
            "tool_calls": self.tool_calls.iter().map(FinalToolCall::to_json).collect::<Vec<_>>(),
            "usage": self.usage_json(),
            "error": self.error,
        })
    }

    fn record_tool_result(&mut self, tool_name: &str, success: bool, summary: &str) {
        if let Some(tool_call) = self
            .tool_calls
            .iter_mut()
            .find(|call| call.name == tool_name && call.success.is_none())
        {
            tool_call.success = Some(success);
            tool_call.summary = Some(summary.to_string());
            return;
        }

        self.tool_calls.push(FinalToolCall {
            id: None,
            name: tool_name.to_string(),
            arguments: None,
            success: Some(success),
            summary: Some(summary.to_string()),
        });
    }

    fn usage_json(&self) -> Option<serde_json::Value> {
        self.usage.as_ref().map(usage_json).or_else(|| {
            self.total_tokens.map(|total_tokens| {
                json!({
                    "prompt_tokens": null,
                    "completion_tokens": null,
                    "total_tokens": total_tokens,
                    "prompt_cache_hit_tokens": null,
                    "prompt_cache_miss_tokens": null,
                })
            })
        })
    }
}

impl FinalToolCall {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "name": self.name,
            "arguments": self.arguments,
            "success": self.success,
            "summary": self.summary,
        })
    }
}

pub fn print_final_error(error: impl Into<String>) {
    let mut collector = FinalJsonCollector::default();
    collector.record_error(error);
    collector.print_final();
}

pub fn print_session_started(session_id: impl std::fmt::Display, project_root: &std::path::Path) {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "type": "session_started",
            "session_id": session_id.to_string(),
            "project_root": project_root.display().to_string(),
        }))
        .unwrap_or_else(|_| "{}".to_string())
    );
}

pub fn print_user_message(content: &str) {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "type": "user_message",
            "role": "user",
            "text": content,
        }))
        .unwrap_or_else(|_| "{}".to_string())
    );
}

pub fn print_event(event: &AgentEvent) {
    let value = event_to_json(event);
    println!(
        "{}",
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    );
}

pub fn print_tool_approval_denied(tool_name: impl AsRef<str>, reason: impl AsRef<str>) {
    let value = json!({
        "type": "tool_approval_denied",
        "tool": tool_name.as_ref(),
        "reason": reason.as_ref(),
    });
    println!(
        "{}",
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    );
}

pub(crate) fn event_to_json(event: &AgentEvent) -> serde_json::Value {
    match event {
        AgentEvent::UserMessage { content } => json!({
            "type": "user_message",
            "role": "user",
            "text": content,
        }),
        AgentEvent::ContentDelta(text) => json!({
            "type": "assistant_delta",
            "role": "assistant",
            "text": text,
        }),
        AgentEvent::ReasoningDelta(text) => json!({
            "type": "reasoning_delta",
            "text": text,
        }),
        AgentEvent::TokenDelta {
            input_tokens,
            output_tokens,
        } => json!({
            "type": "token_delta",
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }),
        AgentEvent::ToolApprovalNeeded {
            tool_name, display, ..
        } => json!({
            "type": "approval_request",
            "tool_name": tool_name,
            "title": display.title,
            "risk_level": display.risk_level.to_string(),
            "description": display.description,
            "details": display.details,
        }),
        AgentEvent::ToolStarted {
            tool_call_id,
            tool_name,
            arguments,
        } => json!({
            "type": "tool_started",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "arguments": arguments,
        }),
        AgentEvent::ToolExecuted {
            tool_name,
            success,
            summary,
        } => json!({
            "type": "tool_finished",
            "tool_name": tool_name,
            "success": success,
            "summary": summary,
        }),
        AgentEvent::UserQuestionRequested {
            title,
            options,
            summary,
        } => json!({
            "type": "user_question_requested",
            "title": title,
            "options": options,
            "summary": summary,
        }),
        AgentEvent::ContextCompacted {
            summary,
            reason,
            before_tokens,
            after_tokens,
        } => json!({
            "type": "context_compacted",
            "summary": summary,
            "reason": reason,
            "before_tokens": before_tokens,
            "after_tokens": after_tokens,
        }),
        AgentEvent::HookExecuted {
            event,
            success,
            summary,
            command_count,
        } => json!({
            "type": "hook_executed",
            "event": event.as_str(),
            "success": success,
            "summary": summary,
            "command_count": command_count,
        }),
        AgentEvent::StreamDone {
            finish_reason,
            usage,
            cache,
        } => json!({
            "type": "stream_done",
            "finish_reason": finish_reason.as_ref().map(finish_reason_label),
            "usage": usage.as_ref().map(usage_json),
            "cache": cache.as_ref().map(cache_json),
        }),
        AgentEvent::Error(message) => json!({
            "type": "error",
            "message": message,
        }),
        AgentEvent::TurnComplete {
            session_id,
            total_tokens,
        } => json!({
            "type": "done",
            "session_id": session_id,
            "total_tokens": total_tokens,
        }),
        AgentEvent::ComplexityAssessed { assessment } => json!({
            "type": "complexity",
            "summary": assessment.display_summary(),
        }),
        AgentEvent::ClarificationNeeded { questions } => json!({
            "type": "clarification_needed",
            "questions": questions,
        }),
        AgentEvent::SubagentStarted {
            agent_id,
            agent_type,
            description,
            is_background,
        } => json!({
            "type": "subagent_started",
            "agent_id": agent_id,
            "agent_type": agent_type,
            "description": description,
            "is_background": is_background,
        }),
        AgentEvent::SubagentDelta { agent_id, content } => json!({
            "type": "subagent_delta",
            "agent_id": agent_id,
            "content": content,
        }),
        AgentEvent::SubagentCompleted { agent_id, result } => json!({
            "type": "subagent_completed",
            "agent_id": agent_id,
            "success": result.success,
            "summary": result.summary,
        }),
        AgentEvent::SubagentToolApprovalNeeded {
            agent_id,
            tool_name,
            arguments,
            ..
        } => json!({
            "type": "subagent_approval_request",
            "agent_id": agent_id,
            "tool_name": tool_name,
            "arguments": arguments,
        }),
        AgentEvent::SwarmStarted {
            run_id,
            summary,
            total,
        } => json!({
            "type": "swarm_started",
            "run_id": run_id,
            "summary": summary,
            "total": total,
        }),
        AgentEvent::SwarmTaskUpdated {
            run_id,
            task_id,
            role,
            status,
            description,
        } => json!({
            "type": "swarm_task_updated",
            "run_id": run_id,
            "task_id": task_id,
            "role": role,
            "status": status,
            "description": description,
        }),
        AgentEvent::SwarmFinished {
            run_id,
            success,
            summary,
        } => json!({
            "type": "swarm_finished",
            "run_id": run_id,
            "success": success,
            "summary": summary,
        }),
        AgentEvent::PlanStepUpdate {
            index,
            total,
            description,
            status,
        } => json!({
            "type": "plan_step",
            "index": index,
            "total": total,
            "description": description,
            "status": plan_status(*status),
        }),
        AgentEvent::PlanStarted { summary, total } => json!({
            "type": "plan_started",
            "summary": summary,
            "total": total,
        }),
        AgentEvent::PlanCleared => json!({
            "type": "plan_cleared",
        }),
        AgentEvent::PlanReviewWarnings { warnings } => json!({
            "type": "plan_review_warnings",
            "warnings": warnings,
        }),
        AgentEvent::FileDiff { path, stats, diff } => json!({
            "type": "file_diff",
            "path": path,
            "stats": stats,
            "diff": diff,
        }),
        AgentEvent::OptionsNeeded { title, options, .. } => json!({
            "type": "options_needed",
            "title": title,
            "options": options,
        }),
    }
}

fn finish_reason_label(reason: &FinishReason) -> String {
    match reason {
        FinishReason::Stop => "stop".to_string(),
        FinishReason::Length => "length".to_string(),
        FinishReason::ToolCalls => "tool_calls".to_string(),
        FinishReason::ContentFilter => "content_filter".to_string(),
        FinishReason::Unknown(value) => value.clone(),
    }
}

fn usage_json(usage: &Usage) -> serde_json::Value {
    json!({
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "total_tokens": usage.total_tokens,
        "prompt_cache_hit_tokens": usage.prompt_cache_hit_tokens,
        "prompt_cache_miss_tokens": usage.prompt_cache_miss_tokens,
    })
}

fn cache_json(cache: &CacheUsage) -> serde_json::Value {
    json!({
        "prompt_cache_hit_tokens": cache.prompt_cache_hit_tokens,
        "prompt_cache_miss_tokens": cache.prompt_cache_miss_tokens,
        "hit_rate": cache.hit_rate(),
    })
}

fn plan_status(status: PlanStepStatus) -> &'static str {
    match status {
        PlanStepStatus::Pending => "pending",
        PlanStepStatus::Running => "running",
        PlanStepStatus::Done => "done",
        PlanStepStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEvent;

    #[test]
    fn maps_tool_and_hook_events_to_stable_names() {
        let tool = event_to_json(&AgentEvent::ToolStarted {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            arguments: "{}".to_string(),
        });
        assert_eq!(tool["type"], "tool_started");
        assert_eq!(tool["tool_name"], "read_file");

        let hook = event_to_json(&AgentEvent::HookExecuted {
            event: HookEvent::PreToolUse,
            success: true,
            summary: "pre: ok".to_string(),
            command_count: 1,
        });
        assert_eq!(hook["type"], "hook_executed");
        assert_eq!(hook["event"], "pre_tool_use");
        assert_eq!(hook["success"], true);

        let compact = event_to_json(&AgentEvent::ContextCompacted {
            summary: "summary".to_string(),
            reason: "auto threshold".to_string(),
            before_tokens: 10,
            after_tokens: 4,
        });
        assert_eq!(compact["type"], "context_compacted");
        assert_eq!(compact["before_tokens"], 10);
    }

    #[test]
    fn maps_turn_complete_to_done() {
        let value = event_to_json(&AgentEvent::TurnComplete {
            session_id: crate::deepseek::SessionId::new_v4(),
            total_tokens: 42,
        });
        assert_eq!(value["type"], "done");
        assert_eq!(value["total_tokens"], 42);
    }

    #[test]
    fn final_json_collector_aggregates_turn_result() {
        let session_id = crate::deepseek::SessionId::new_v4();
        let mut collector = FinalJsonCollector::new(session_id);

        collector.observe(&AgentEvent::ContentDelta("hello ".to_string()));
        collector.observe(&AgentEvent::ContentDelta("world".to_string()));
        collector.observe(&AgentEvent::ToolStarted {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            arguments: r#"{"path":"README.md"}"#.to_string(),
        });
        collector.observe(&AgentEvent::ToolExecuted {
            tool_name: "read_file".to_string(),
            success: true,
            summary: "read README.md".to_string(),
        });
        collector.observe(&AgentEvent::StreamDone {
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                prompt_tokens: 3,
                completion_tokens: 4,
                total_tokens: 7,
                prompt_cache_hit_tokens: Some(1),
                prompt_cache_miss_tokens: Some(2),
            }),
            cache: None,
        });
        collector.observe(&AgentEvent::TurnComplete {
            session_id,
            total_tokens: 7,
        });

        let value = collector.final_value();
        assert_eq!(value["status"], "ok");
        assert_eq!(value["session_id"], session_id.to_string());
        assert_eq!(value["final_message"], "hello world");
        assert_eq!(value["tool_calls"][0]["id"], "call-1");
        assert_eq!(value["tool_calls"][0]["name"], "read_file");
        assert_eq!(value["tool_calls"][0]["success"], true);
        assert_eq!(value["usage"]["total_tokens"], 7);
        assert!(value["error"].is_null());
    }

    #[test]
    fn final_json_collector_marks_error_even_when_turn_completes() {
        let session_id = crate::deepseek::SessionId::new_v4();
        let mut collector = FinalJsonCollector::new(session_id);
        collector.observe(&AgentEvent::Error("boom".to_string()));
        collector.observe(&AgentEvent::TurnComplete {
            session_id,
            total_tokens: 0,
        });

        let value = collector.final_value();
        assert_eq!(value["status"], "error");
        assert_eq!(value["error"], "boom");
        assert!(collector.has_error());
    }
}

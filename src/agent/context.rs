use crate::deepseek::{
    ChatMessage, ChatMessageContent, MessageContent, MessageVisibility, ProtocolMessage, Role,
    Session, SubTurnId,
};
use crate::storage::{SessionEvent, SessionEventKind};

#[derive(Debug, Clone)]
pub struct ContextAssembler {
    max_visible_messages: usize,
    max_tool_summaries: usize,
    max_event_summaries: usize,
}

impl Default for ContextAssembler {
    fn default() -> Self {
        Self {
            max_visible_messages: 12,
            max_tool_summaries: 5,
            max_event_summaries: 8,
        }
    }
}

impl ContextAssembler {
    #[must_use]
    pub fn assemble(
        &self,
        session: &Session,
        _events: Option<&[SessionEvent]>,
    ) -> Vec<ProtocolMessage> {
        let active_sub_turn_ids = session
            .messages
            .iter()
            .filter(|msg| {
                session
                    .reasoning_state
                    .preserved_assistant_messages
                    .contains(&msg.id)
            })
            .filter_map(|msg| msg.sub_turn_id)
            .collect::<Vec<_>>();
        let mut messages = session
            .messages
            .iter()
            .filter_map(|msg| sanitize_context_message(msg, session, &active_sub_turn_ids))
            .collect::<Vec<_>>();

        if messages.len() > self.max_visible_messages {
            messages = messages.split_off(messages.len().saturating_sub(self.max_visible_messages));
        }

        messages
    }

    #[must_use]
    pub fn transient_chat_messages(
        &self,
        session: &Session,
        events: Option<&[SessionEvent]>,
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::new();
        if let Some(summary) = self.tool_summary_text(session) {
            messages.push(transient_system_chat(summary));
        }
        if let Some(summary) = self.event_summary_text(events) {
            messages.push(transient_system_chat(summary));
        }
        messages
    }

    fn tool_summary_text(&self, session: &Session) -> Option<String> {
        if session.tool_call_history.is_empty() {
            return None;
        }

        let lines = session
            .tool_call_history
            .iter()
            .rev()
            .take(self.max_tool_summaries)
            .rev()
            .map(|record| {
                format!(
                    "- {}: {}",
                    record.name,
                    truncate_context_line(&record.result_summary, 180)
                )
            })
            .collect::<Vec<_>>();

        Some("[Recent tool summary]\n".to_string() + &lines.join("\n"))
    }

    fn event_summary_text(&self, events: Option<&[SessionEvent]>) -> Option<String> {
        let events = events?;
        let mut lines = Vec::new();
        for event in events.iter().rev() {
            match &event.kind {
                SessionEventKind::PlanStarted { summary, total } => {
                    lines.push(format!("- plan: {summary} ({total} steps)"));
                }
                SessionEventKind::PlanStepUpdated {
                    index,
                    total,
                    description,
                    status,
                } => {
                    lines.push(format!(
                        "- step {}/{} {status}: {}",
                        index + 1,
                        total,
                        truncate_context_line(description, 140)
                    ));
                }
                SessionEventKind::FileChanged { path, stats } => {
                    lines.push(format!("- changed {path}: {stats}"));
                }
                SessionEventKind::SwarmStarted { summary, total, .. } => {
                    lines.push(format!("- swarm: {summary} ({total} tasks)"));
                }
                SessionEventKind::SwarmTaskUpdated {
                    role,
                    status,
                    description,
                    ..
                } => {
                    lines.push(format!(
                        "- swarm {role} {status}: {}",
                        truncate_context_line(description, 140)
                    ));
                }
                SessionEventKind::SwarmFinished {
                    success, summary, ..
                } => {
                    lines.push(format!(
                        "- swarm {}: {}",
                        if *success { "ok" } else { "failed" },
                        truncate_context_line(summary, 140)
                    ));
                }
                SessionEventKind::SwarmPatchPending {
                    summary,
                    changed_files,
                    conflict,
                    ..
                } => {
                    lines.push(format!(
                        "- swarm pending patch{}: {} ({})",
                        if *conflict { " conflict" } else { "" },
                        truncate_context_line(summary, 120),
                        changed_files.join(", ")
                    ));
                }
                SessionEventKind::SwarmPatchApplied {
                    success,
                    summary,
                    changed_files,
                    ..
                } => {
                    lines.push(format!(
                        "- swarm patch {}: {} ({})",
                        if *success { "applied" } else { "failed" },
                        truncate_context_line(summary, 120),
                        changed_files.join(", ")
                    ));
                }
                SessionEventKind::ScheduledTaskUpdated {
                    task_id,
                    status,
                    summary,
                } => {
                    lines.push(format!(
                        "- scheduled task {task_id} {status}: {}",
                        truncate_context_line(summary, 140)
                    ));
                }
                SessionEventKind::ToolCallFinished {
                    name,
                    success,
                    summary,
                    ..
                } => {
                    lines.push(format!(
                        "- tool {name} {}: {}",
                        if *success { "ok" } else { "failed" },
                        truncate_context_line(summary, 140)
                    ));
                }
                SessionEventKind::AssistantVisible { .. }
                | SessionEventKind::AssistantInternal { .. }
                | SessionEventKind::ReasoningInternal { .. }
                | SessionEventKind::UserMessage { .. }
                | SessionEventKind::ToolCallStarted { .. }
                | SessionEventKind::HookExecuted { .. }
                | SessionEventKind::ApprovalRequested { .. }
                | SessionEventKind::TurnFinished { .. }
                | SessionEventKind::Error { .. } => {}
            }

            if lines.len() >= self.max_event_summaries {
                break;
            }
        }

        if lines.is_empty() {
            None
        } else {
            lines.reverse();
            Some("[Recoverable event summary]\n".to_string() + &lines.join("\n"))
        }
    }
}

fn transient_system_chat(content: String) -> ChatMessage {
    ChatMessage {
        role: "system".into(),
        content: Some(ChatMessageContent::Text(content)),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

fn truncate_context_line(text: &str, max_chars: usize) -> String {
    let text = text.lines().next().unwrap_or_default().trim();
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut out = text
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>();
        out.push_str("...");
        out
    }
}

fn sanitize_context_message(
    msg: &ProtocolMessage,
    session: &Session,
    active_sub_turn_ids: &[SubTurnId],
) -> Option<ProtocolMessage> {
    if msg.visibility == MessageVisibility::AuditOnly {
        return None;
    }
    let keep_active_tool_assistant = msg.visibility == MessageVisibility::InternalProtocolState
        && !msg.tool_calls.is_empty()
        && session
            .reasoning_state
            .preserved_assistant_messages
            .contains(&msg.id);
    let keep_active_tool_result = msg.visibility == MessageVisibility::InternalProtocolState
        && msg.role == Role::Tool
        && msg
            .sub_turn_id
            .is_some_and(|sub_turn_id| active_sub_turn_ids.contains(&sub_turn_id));
    let keep_protocol_state = keep_active_tool_assistant || keep_active_tool_result;
    if msg.visibility != MessageVisibility::UserVisible && !keep_protocol_state {
        return None;
    }

    let mut sanitized = msg.clone();
    if !keep_active_tool_assistant {
        sanitized.reasoning_content = None;
    }
    if sanitized.role == Role::Tool {
        let text = sanitized.content.to_string_lossy();
        sanitized.content = MessageContent::from(truncate_context_line(&text, 4_000));
        for result in &mut sanitized.tool_results {
            result.result = truncate_context_line(&result.result, 4_000);
        }
    }
    Some(sanitized)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::deepseek::{
        MessageContent, MessageId, ReasoningState, SessionId, SessionMetadata, SubTurnId,
        ToolCallRecord, TurnId,
    };

    fn session_with_messages(messages: Vec<ProtocolMessage>) -> Session {
        Session {
            id: SessionId::new_v4(),
            name: None,
            project_root: ".".into(),
            messages,
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: SessionMetadata::default(),
        }
    }

    fn message(content: &str, visibility: MessageVisibility) -> ProtocolMessage {
        ProtocolMessage {
            id: MessageId::new_v4(),
            role: Role::Assistant,
            content: MessageContent::from(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: TurnId::new_v4(),
            sub_turn_id: None,
            visibility,
        }
    }

    #[test]
    fn assembler_excludes_internal_reasoning_and_audit_messages() {
        let mut internal = message("hidden thought", MessageVisibility::InternalProtocolState);
        internal.reasoning_content = Some("private reasoning".into());
        let session = session_with_messages(vec![
            message("visible answer", MessageVisibility::UserVisible),
            internal,
            message("audit protocol", MessageVisibility::AuditOnly),
        ]);

        let assembled = ContextAssembler::default().assemble(&session, None);
        let text = assembled
            .iter()
            .map(|msg| msg.content.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("visible answer"));
        assert!(!text.contains("hidden thought"));
        assert!(!text.contains("private reasoning"));
        assert!(!text.contains("audit protocol"));
    }

    #[test]
    fn assembler_keeps_active_tool_protocol_for_api_continuation() {
        let turn_id = TurnId::new_v4();
        let sub_turn_id = SubTurnId::new_v4();
        let mut assistant = message("calling tool", MessageVisibility::InternalProtocolState);
        let assistant_id = assistant.id;
        assistant.turn_id = turn_id;
        assistant.sub_turn_id = Some(sub_turn_id);
        assistant.reasoning_content = Some("private chain".into());
        assistant.tool_calls.push(crate::deepseek::ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: crate::deepseek::ToolCallFunction {
                name: "read_file".into(),
                arguments: r#"{"path":"src/main.rs"}"#.into(),
            },
        });
        let mut tool = message(&"x".repeat(8_000), MessageVisibility::InternalProtocolState);
        tool.role = Role::Tool;
        tool.turn_id = turn_id;
        tool.sub_turn_id = Some(sub_turn_id);
        let mut session = session_with_messages(vec![assistant, tool]);
        session.reasoning_state.preserved_assistant_messages = vec![assistant_id];

        let assembled = ContextAssembler::default().assemble(&session, None);

        assert!(assembled.iter().any(|msg| !msg.tool_calls.is_empty()));
        assert!(assembled.iter().any(|msg| msg.role == Role::Tool));
        assert!(assembled
            .iter()
            .any(|msg| msg.reasoning_content.as_deref() == Some("private chain")));
        assert!(assembled
            .iter()
            .filter(|msg| msg.role == Role::Tool)
            .all(|msg| msg.content.to_string_lossy().chars().count() <= 4_000));
    }

    #[test]
    fn assembler_uses_summaries_instead_of_long_tool_output() {
        let mut session = session_with_messages(Vec::new());
        session.tool_call_history.push(ToolCallRecord {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments: "{}".into(),
            result_summary: "short useful summary".into(),
            exit_code: Some(0),
            duration_ms: 3,
            risk_level: "safe".into(),
            approved: true,
            at: Utc::now(),
        });

        let messages = ContextAssembler::default().transient_chat_messages(&session, None);
        let text = messages
            .first()
            .and_then(|message| message.content.as_ref())
            .map(|content| match content {
                ChatMessageContent::Text(text) => text.as_str(),
                ChatMessageContent::Parts(_) => "",
            })
            .unwrap_or_default();

        assert!(text.contains("Recent tool summary"));
        assert!(text.contains("short useful summary"));
    }
}

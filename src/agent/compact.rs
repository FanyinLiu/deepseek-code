use std::collections::BTreeSet;

use crate::deepseek::{
    models::estimate_tokenish_count, MessageVisibility, ProtocolMessage, Role, Session, SubTurnId,
};
use crate::storage::{SessionEvent, SessionEventKind};

pub const DEFAULT_COMPACT_KEEP_MESSAGES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactSnapshot {
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub before_messages: usize,
    pub after_messages: usize,
    pub retained_start: usize,
    pub retained_count: usize,
    pub summary: String,
    pub reason: String,
}

#[must_use]
pub fn build_compact_snapshot(
    session: &Session,
    events: Option<&[SessionEvent]>,
    messages_to_keep: usize,
    reason: impl Into<String>,
) -> CompactSnapshot {
    let reason = reason.into();
    let retained = retained_message_indices(session, messages_to_keep.max(1));
    let retained_messages = retained
        .iter()
        .filter_map(|index| session.messages.get(*index))
        .collect::<Vec<_>>();
    let before_tokens = estimate_session_tokens(session);
    let summary = deterministic_summary(session, events, &retained, &reason);
    let after_tokens = retained_messages
        .iter()
        .map(|msg| estimate_message_tokens(msg))
        .sum::<u64>()
        .saturating_add(estimate_tokenish_count(&summary));
    let retained_start = retained
        .iter()
        .next()
        .copied()
        .unwrap_or(session.messages.len());
    CompactSnapshot {
        before_tokens,
        after_tokens,
        before_messages: session.messages.len(),
        after_messages: retained.len(),
        retained_start,
        retained_count: retained.len(),
        summary,
        reason,
    }
}

#[must_use]
pub fn retained_messages(session: &Session, messages_to_keep: usize) -> Vec<ProtocolMessage> {
    retained_message_indices(session, messages_to_keep.max(1))
        .into_iter()
        .filter_map(|index| session.messages.get(index).cloned())
        .collect()
}

#[must_use]
pub fn estimate_session_tokens(session: &Session) -> u64 {
    let message_tokens = session
        .messages
        .iter()
        .map(estimate_message_tokens)
        .sum::<u64>();
    let tool_history_tokens = session
        .tool_call_history
        .iter()
        .map(|record| {
            estimate_tokenish_count(&record.name)
                .saturating_add(estimate_tokenish_count(&record.arguments))
                .saturating_add(estimate_tokenish_count(&record.result_summary))
        })
        .sum::<u64>();
    message_tokens.saturating_add(tool_history_tokens)
}

#[must_use]
pub fn has_active_tool_protocol(session: &Session) -> bool {
    session.reasoning_state.active_tool_turn.is_some()
        || !session
            .reasoning_state
            .preserved_assistant_messages
            .is_empty()
}

#[must_use]
pub fn should_auto_compact(
    session: &Session,
    events: &[SessionEvent],
    threshold_tokens: u64,
) -> bool {
    if threshold_tokens == 0
        || has_active_tool_protocol(session)
        || estimate_session_tokens(session) < threshold_tokens
    {
        return false;
    }

    let latest_compact = events
        .iter()
        .rposition(|event| matches!(event.kind, SessionEventKind::ContextCompacted { .. }));
    let latest_content = events.iter().rposition(|event| {
        matches!(
            event.kind,
            SessionEventKind::UserMessage { .. }
                | SessionEventKind::AssistantVisible { .. }
                | SessionEventKind::AssistantInternal { .. }
                | SessionEventKind::ToolCallFinished { .. }
                | SessionEventKind::UserQuestionRequested { .. }
                | SessionEventKind::FileChanged { .. }
                | SessionEventKind::TurnFinished { .. }
        )
    });

    match (latest_compact, latest_content) {
        (Some(compact), Some(content)) => content > compact,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

#[must_use]
pub fn latest_compact_summary(events: &[SessionEvent]) -> Option<(&str, &str)> {
    events.iter().rev().find_map(|event| match &event.kind {
        SessionEventKind::ContextCompacted {
            summary, reason, ..
        } => Some((summary.as_str(), reason.as_str())),
        _ => None,
    })
}

fn retained_message_indices(session: &Session, messages_to_keep: usize) -> BTreeSet<usize> {
    let active_sub_turn_ids = active_sub_turn_ids(session);
    let visible_indices = session
        .messages
        .iter()
        .enumerate()
        .filter_map(|(index, msg)| {
            (msg.visibility == MessageVisibility::UserVisible).then_some(index)
        })
        .collect::<Vec<_>>();
    let visible_start = visible_indices.len().saturating_sub(messages_to_keep);
    let mut retained = visible_indices
        .into_iter()
        .skip(visible_start)
        .collect::<BTreeSet<_>>();

    for (index, msg) in session.messages.iter().enumerate() {
        if is_active_protocol_message(msg, session, &active_sub_turn_ids) {
            retained.insert(index);
        }
    }

    retained
}

fn active_sub_turn_ids(session: &Session) -> Vec<SubTurnId> {
    session
        .messages
        .iter()
        .filter(|msg| {
            session
                .reasoning_state
                .preserved_assistant_messages
                .contains(&msg.id)
        })
        .filter_map(|msg| msg.sub_turn_id)
        .collect()
}

fn is_active_protocol_message(
    msg: &ProtocolMessage,
    session: &Session,
    active_sub_turn_ids: &[SubTurnId],
) -> bool {
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
    keep_active_tool_assistant || keep_active_tool_result
}

fn estimate_message_tokens(msg: &ProtocolMessage) -> u64 {
    let mut total = estimate_tokenish_count(&msg.role.to_string())
        .saturating_add(estimate_tokenish_count(&msg.content.to_string_lossy()));
    if let Some(reasoning) = &msg.reasoning_content {
        total = total.saturating_add(estimate_tokenish_count(reasoning));
    }
    for call in &msg.tool_calls {
        total = total
            .saturating_add(estimate_tokenish_count(&call.id))
            .saturating_add(estimate_tokenish_count(&call.function.name))
            .saturating_add(estimate_tokenish_count(&call.function.arguments));
    }
    for result in &msg.tool_results {
        total = total
            .saturating_add(estimate_tokenish_count(&result.tool_call_id))
            .saturating_add(estimate_tokenish_count(&result.name))
            .saturating_add(estimate_tokenish_count(&result.result));
    }
    total
}

fn deterministic_summary(
    session: &Session,
    events: Option<&[SessionEvent]>,
    retained: &BTreeSet<usize>,
    reason: &str,
) -> String {
    let mut lines = vec![
        "[Context compact summary]".to_string(),
        format!("reason: {reason}"),
        format!("messages: {} -> {}", session.messages.len(), retained.len()),
        format!(
            "retained: {}",
            retained_range_label(retained, session.messages.len())
        ),
    ];

    append_section(
        &mut lines,
        "recent user goals",
        recent_user_goals(session)
            .into_iter()
            .map(|line| truncate_line(&line, 180))
            .collect(),
    );
    append_section(
        &mut lines,
        "recent tool results",
        recent_tool_results(session, events)
            .into_iter()
            .map(|line| truncate_line(&line, 180))
            .collect(),
    );

    let todo = crate::tools::todo_state::load_todo_summary(&session.project_root);
    append_section(&mut lines, "todo", vec![todo.compact_line()]);

    let pending = pending_question(events)
        .map(|line| truncate_line(&line, 180))
        .unwrap_or_else(|| "none".to_string());
    append_section(&mut lines, "pending question", vec![pending]);

    if let Some((summary, reason)) = events.and_then(latest_compact_summary) {
        append_section(
            &mut lines,
            "previous compact",
            vec![format!("{reason}: {}", truncate_line(summary, 180))],
        );
    }

    lines.join("\n")
}

fn append_section(lines: &mut Vec<String>, title: &str, items: Vec<String>) {
    lines.push(format!("{title}:"));
    if items.is_empty() {
        lines.push("- none".to_string());
    } else {
        for item in items {
            lines.push(format!("- {item}"));
        }
    }
}

fn retained_range_label(retained: &BTreeSet<usize>, total: usize) -> String {
    match (retained.iter().next(), retained.iter().next_back()) {
        (Some(start), Some(end)) if start == end => {
            format!("message index {start} of {total}")
        }
        (Some(start), Some(end)) => {
            format!("message indexes {start}..={end} of {total}")
        }
        _ => "none".to_string(),
    }
}

fn recent_user_goals(session: &Session) -> Vec<String> {
    let mut goals = session
        .messages
        .iter()
        .rev()
        .filter(|msg| msg.role == Role::User && msg.visibility == MessageVisibility::UserVisible)
        .filter_map(|msg| {
            let text = msg.content.to_string_lossy();
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .take(3)
        .collect::<Vec<_>>();
    goals.reverse();
    goals
}

fn recent_tool_results(session: &Session, events: Option<&[SessionEvent]>) -> Vec<String> {
    let mut results = session
        .tool_call_history
        .iter()
        .rev()
        .filter_map(|record| {
            let summary = record.result_summary.trim();
            (!summary.is_empty()).then(|| format!("{}: {summary}", record.name))
        })
        .take(3)
        .collect::<Vec<_>>();
    results.reverse();

    if !results.is_empty() {
        return results;
    }

    let Some(events) = events else {
        return Vec::new();
    };
    let mut event_results = events
        .iter()
        .rev()
        .filter_map(|event| match &event.kind {
            SessionEventKind::ToolCallFinished { name, summary, .. } => {
                let summary = summary.trim();
                (!summary.is_empty()).then(|| format!("{name}: {summary}"))
            }
            _ => None,
        })
        .take(3)
        .collect::<Vec<_>>();
    event_results.reverse();
    event_results
}

fn pending_question(events: Option<&[SessionEvent]>) -> Option<String> {
    let events = events?;
    let latest_user = events
        .iter()
        .rposition(|event| matches!(event.kind, SessionEventKind::UserMessage { .. }));
    events
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, event)| match &event.kind {
            SessionEventKind::UserQuestionRequested {
                title,
                options,
                summary,
                ..
            } if latest_user.is_none_or(|user_index| index > user_index) => Some(format!(
                "{} ({} options): {}",
                title,
                options.len(),
                summary
            )),
            _ => None,
        })
}

fn truncate_line(text: &str, max_chars: usize) -> String {
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

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::deepseek::{
        MessageContent, MessageId, ReasoningState, SessionId, SessionMetadata, ToolCall,
        ToolCallFunction, TurnId,
    };

    fn message(role: Role, content: &str, visibility: MessageVisibility) -> ProtocolMessage {
        ProtocolMessage {
            id: MessageId::new_v4(),
            role,
            content: MessageContent::from(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: TurnId::new_v4(),
            sub_turn_id: None,
            visibility,
        }
    }

    fn session_with(messages: Vec<ProtocolMessage>) -> Session {
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

    #[test]
    fn compact_snapshot_generates_deterministic_summary() {
        let session = session_with(vec![
            message(Role::User, "先读配置", MessageVisibility::UserVisible),
            message(Role::Assistant, "已读取", MessageVisibility::UserVisible),
            message(Role::User, "再生成报告", MessageVisibility::UserVisible),
        ]);

        let snapshot = build_compact_snapshot(&session, None, 2, "manual /compact");

        assert_eq!(snapshot.before_messages, 3);
        assert_eq!(snapshot.after_messages, 2);
        assert!(snapshot.summary.contains("[Context compact summary]"));
        assert!(snapshot.summary.contains("manual /compact"));
        assert!(snapshot.summary.contains("再生成报告"));
    }

    #[test]
    fn retained_messages_keep_active_tool_protocol() {
        let sub_turn_id = crate::deepseek::SubTurnId::new_v4();
        let mut assistant = message(
            Role::Assistant,
            "",
            MessageVisibility::InternalProtocolState,
        );
        assistant.sub_turn_id = Some(sub_turn_id);
        assistant.tool_calls.push(ToolCall {
            id: "call-1".to_string(),
            call_type: "function".to_string(),
            function: ToolCallFunction {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        });
        let assistant_id = assistant.id;
        let mut tool = message(
            Role::Tool,
            "result",
            MessageVisibility::InternalProtocolState,
        );
        tool.sub_turn_id = Some(sub_turn_id);
        let mut session = session_with(vec![
            message(Role::User, "old", MessageVisibility::UserVisible),
            assistant,
            tool,
            message(Role::User, "new", MessageVisibility::UserVisible),
        ]);
        session.reasoning_state.preserved_assistant_messages = vec![assistant_id];

        let kept = retained_messages(&session, 1);

        assert_eq!(kept.len(), 3);
        assert!(kept.iter().any(|msg| !msg.tool_calls.is_empty()));
        assert!(kept.iter().any(|msg| msg.role == Role::Tool));
        assert!(kept
            .iter()
            .any(|msg| msg.content.to_string_lossy() == "new"));
    }

    #[test]
    fn auto_compact_requires_new_content_after_latest_compact() {
        let session = session_with(vec![message(
            Role::User,
            &"x".repeat(1_000),
            MessageVisibility::UserVisible,
        )]);
        let session_id = session.id;
        let compact = SessionEvent::new(
            session_id,
            None,
            SessionEventKind::ContextCompacted {
                before_tokens: 300,
                after_tokens: 80,
                before_messages: 3,
                after_messages: 1,
                retained_start: 2,
                retained_count: 1,
                summary: "summary".into(),
                reason: "auto".into(),
            },
        );

        assert!(!should_auto_compact(&session, &[compact], 10));

        let user = SessionEvent::new(
            session_id,
            None,
            SessionEventKind::UserMessage {
                content: "next".into(),
            },
        );
        let events = vec![
            SessionEvent::new(
                session_id,
                None,
                SessionEventKind::ContextCompacted {
                    before_tokens: 300,
                    after_tokens: 80,
                    before_messages: 3,
                    after_messages: 1,
                    retained_start: 2,
                    retained_count: 1,
                    summary: "summary".into(),
                    reason: "auto".into(),
                },
            ),
            user,
        ];

        assert!(should_auto_compact(&session, &events, 10));
    }
}

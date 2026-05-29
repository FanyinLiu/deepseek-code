use super::models::{
    ChatContentPart, ChatImageUrl, ChatMessage, ChatMessageContent, MessageContent,
    MessageVisibility, ProtocolMessage, ReasoningState, Role,
};

/// Convert `ProtocolMessages` to `ChatMessages` for API submission.
/// This is where we handle:
/// - Filtering out `InternalProtocolState` messages
/// - Injecting `reasoning_content` for active tool turns
/// - Converting tool results to the `OpenAI` format
/// - Converting multimodal `MultiPart` content to API `Parts`
#[must_use]
pub fn to_chat_messages(
    protocol_messages: &[ProtocolMessage],
    reasoning_state: &ReasoningState,
) -> Vec<ChatMessage> {
    let mut chat_msgs: Vec<ChatMessage> = Vec::new();
    let mut pending_tool_call_ids = std::collections::HashSet::<String>::new();

    // Tool-call ids that actually have a matching tool-result message. An
    // assistant tool_call with no result (e.g. a session interrupted mid tool
    // execution, then resumed) would make the request malformed, so such
    // unanswered calls are dropped below — the mirror of orphan-result removal.
    let answered_tool_call_ids: std::collections::HashSet<String> = protocol_messages
        .iter()
        .flat_map(|m| m.tool_results.iter().map(|tr| tr.tool_call_id.clone()))
        .collect();

    for msg in protocol_messages {
        let is_active_tool_protocol = msg.visibility == MessageVisibility::InternalProtocolState
            && (msg.role == Role::Tool
                || reasoning_state
                    .preserved_assistant_messages
                    .contains(&msg.id));
        if msg.visibility == MessageVisibility::InternalProtocolState && !is_active_tool_protocol {
            continue;
        }

        let role = msg.role.to_string();
        let mut chat_msg = ChatMessage {
            role,
            content: match &msg.content {
                MessageContent::None => None,
                MessageContent::Text(s) => Some(ChatMessageContent::Text(s.clone())),
                MessageContent::MultiPart(parts) => {
                    let chat_parts: Vec<ChatContentPart> = parts
                        .iter()
                        .map(|p| {
                            if p.part_type == "image_url" {
                                ChatContentPart {
                                    part_type: "image_url".into(),
                                    text: None,
                                    image_url: p
                                        .image_url
                                        .as_ref()
                                        .map(|url| ChatImageUrl { url: url.clone() }),
                                }
                            } else {
                                ChatContentPart {
                                    part_type: "text".into(),
                                    text: p.text.clone(),
                                    image_url: None,
                                }
                            }
                        })
                        .collect();
                    Some(ChatMessageContent::Parts(chat_parts))
                }
            },
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        };

        if msg.role == Role::Tool && !msg.tool_results.is_empty() {
            for tr in &msg.tool_results {
                if !pending_tool_call_ids.remove(&tr.tool_call_id) {
                    continue;
                }
                let mut tool_msg = chat_msg.clone();
                tool_msg.role = "tool".into();
                tool_msg.content = Some(ChatMessageContent::Text(tr.result.clone()));
                tool_msg.reasoning_content = None;
                tool_msg.tool_calls = None;
                tool_msg.tool_call_id = Some(tr.tool_call_id.clone());
                chat_msgs.push(tool_msg);
            }
            continue;
        }

        match msg.role {
            Role::Assistant => {
                // Include reasoning_content if this message is in the
                // preserved set for the active tool turn
                if reasoning_state
                    .preserved_assistant_messages
                    .contains(&msg.id)
                {
                    chat_msg.reasoning_content = msg.reasoning_content.clone();
                }
                let answered_calls: Vec<_> = msg
                    .tool_calls
                    .iter()
                    .filter(|tc| answered_tool_call_ids.contains(&tc.id))
                    .cloned()
                    .collect();
                if !answered_calls.is_empty() {
                    pending_tool_call_ids.extend(answered_calls.iter().map(|tc| tc.id.clone()));
                    chat_msg.tool_calls = Some(answered_calls);
                }
            }
            Role::Tool => {
                // Tool result messages: role=tool, tool_call_id, content
                if msg.tool_results.is_empty() {
                    continue;
                }
                chat_msg.role = "tool".into();
                if let Some(tr) = msg.tool_results.first() {
                    if !pending_tool_call_ids.remove(&tr.tool_call_id) {
                        continue;
                    }
                    chat_msg.tool_call_id = Some(tr.tool_call_id.clone());
                    chat_msg.content = Some(ChatMessageContent::Text(tr.result.clone()));
                }
            }
            _ => {}
        }

        // An assistant message whose only tool_calls were unanswered (and that
        // carries no text) is now empty — skip it rather than send a malformed
        // or hollow message.
        if msg.role == Role::Assistant
            && chat_msg.content.is_none()
            && chat_msg.tool_calls.is_none()
        {
            continue;
        }

        chat_msgs.push(chat_msg);
    }

    chat_msgs
}

/// Sanitize messages before sending to API.
/// - Remove null bytes
/// - Trim excessive whitespace
/// - Ensure content field is valid
#[must_use]
pub fn sanitize_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|m| {
            let mut cleaned = m.clone();
            if let Some(reasoning_content) = &mut cleaned.reasoning_content {
                let sanitized = sanitize_text(reasoning_content);
                if sanitized.is_empty() {
                    cleaned.reasoning_content = None;
                } else {
                    *reasoning_content = sanitized;
                }
            }
            if let Some(ref mut content) = cleaned.content {
                match content {
                    ChatMessageContent::Text(ref mut s) => {
                        let sanitized = sanitize_text(s);
                        if sanitized.is_empty() {
                            cleaned.content = None;
                        } else {
                            *s = sanitized;
                        }
                    }
                    ChatMessageContent::Parts(parts) => {
                        for part in parts {
                            if let Some(ref mut text) = part.text {
                                *text = sanitize_text(text);
                            }
                        }
                        // Don't drop multipart messages even if text is empty
                        // (they may contain images)
                    }
                }
            }
            cleaned
        })
        .collect()
}

fn sanitize_text(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !is_invalid_api_control(*ch))
        .collect()
}

fn is_invalid_api_control(ch: char) -> bool {
    ch == '\0' || (ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
}

/// Mark the end of a tool loop sub-turn: remove reasoning
/// from the preserved set when the tool call is resolved.
pub fn finalize_tool_turn(reasoning_state: &mut ReasoningState, messages: &mut [ProtocolMessage]) {
    // Move preserved assistant messages to audit-only visibility
    for msg_id in reasoning_state.preserved_assistant_messages.drain(..) {
        if let Some(msg) = messages.iter_mut().find(|m| m.id == msg_id) {
            msg.visibility = MessageVisibility::AuditOnly;
        }
    }
    reasoning_state.active_tool_turn = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek::models::{ToolCall, ToolCallFunction, ToolResultRecord};
    use uuid::Uuid;

    fn protocol_assistant_tool_message(tool_call_ids: &[&str]) -> ProtocolMessage {
        ProtocolMessage {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            content: MessageContent::None,
            reasoning_content: Some("private reasoning".into()),
            tool_calls: tool_call_ids
                .iter()
                .map(|id| ToolCall {
                    id: (*id).into(),
                    call_type: "function".into(),
                    function: ToolCallFunction {
                        name: "read_file".into(),
                        arguments: r#"{"path":"Cargo.toml"}"#.into(),
                    },
                })
                .collect(),
            tool_results: Vec::new(),
            turn_id: Uuid::nil(),
            sub_turn_id: None,
            visibility: MessageVisibility::InternalProtocolState,
        }
    }

    fn protocol_tool_message(tool_results: Vec<ToolResultRecord>) -> ProtocolMessage {
        ProtocolMessage {
            id: Uuid::nil(),
            role: Role::Tool,
            content: MessageContent::None,
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results,
            turn_id: Uuid::nil(),
            sub_turn_id: None,
            visibility: MessageVisibility::InternalProtocolState,
        }
    }

    #[test]
    fn converts_every_tool_result_to_api_tool_message() {
        let assistant = protocol_assistant_tool_message(&["call_a", "call_b"]);
        let reasoning_state = ReasoningState {
            preserved_assistant_messages: vec![assistant.id],
            ..ReasoningState::default()
        };
        let protocol = protocol_tool_message(vec![
            ToolResultRecord {
                tool_call_id: "call_a".into(),
                name: "read".into(),
                result: "first".into(),
                is_error: false,
            },
            ToolResultRecord {
                tool_call_id: "call_b".into(),
                name: "write".into(),
                result: "second".into(),
                is_error: false,
            },
        ]);

        let messages = to_chat_messages(&[assistant, protocol], &reasoning_state);

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "assistant");
        assert_eq!(messages[1].role, "tool");
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call_a"));
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_b"));
        assert!(matches!(
            messages[2].content.as_ref(),
            Some(ChatMessageContent::Text(text)) if text == "second"
        ));
    }

    #[test]
    fn orphan_tool_result_is_not_sent_to_api() {
        let protocol = protocol_tool_message(vec![ToolResultRecord {
            tool_call_id: "call_a".into(),
            name: "read".into(),
            result: "first".into(),
            is_error: false,
        }]);

        let messages = to_chat_messages(&[protocol], &ReasoningState::default());

        assert!(messages.iter().all(|message| message.role != "tool"));
    }

    #[test]
    fn dangling_assistant_tool_call_without_result_is_dropped() {
        // A session interrupted mid tool execution can persist an assistant
        // tool_call with no matching result; sending it would 400 the resume.
        let assistant = protocol_assistant_tool_message(&["call_x"]);
        let reasoning_state = ReasoningState {
            preserved_assistant_messages: vec![assistant.id],
            ..ReasoningState::default()
        };

        let messages = to_chat_messages(&[assistant], &reasoning_state);

        assert!(
            messages.iter().all(|m| m.tool_calls.is_none()),
            "no hanging tool_call may reach the API"
        );
        assert!(messages.is_empty(), "the now-empty assistant is dropped");
    }

    #[test]
    fn partially_answered_assistant_keeps_only_answered_tool_calls() {
        let assistant = protocol_assistant_tool_message(&["call_a", "call_b"]);
        let reasoning_state = ReasoningState {
            preserved_assistant_messages: vec![assistant.id],
            ..ReasoningState::default()
        };
        let protocol = protocol_tool_message(vec![ToolResultRecord {
            tool_call_id: "call_a".into(),
            name: "read".into(),
            result: "first".into(),
            is_error: false,
        }]);

        let messages = to_chat_messages(&[assistant, protocol], &reasoning_state);

        let assistant_msg = messages
            .iter()
            .find(|m| m.role == "assistant")
            .expect("assistant kept");
        let calls = assistant_msg.tool_calls.as_ref().expect("kept call_a");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_a");
        assert_eq!(messages.iter().filter(|m| m.role == "tool").count(), 1);
    }

    #[test]
    fn sanitize_removes_controls_without_trimming_semantic_whitespace() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: Some(ChatMessageContent::Text(
                "  first line\0\n\tsecond\u{0007}  ".into(),
            )),
            reasoning_content: Some(" keep\nreasoning\u{001b} ".into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];

        let cleaned = sanitize_messages(&messages);

        assert!(matches!(
            cleaned[0].content.as_ref(),
            Some(ChatMessageContent::Text(text)) if text == "  first line\n\tsecond  "
        ));
        assert_eq!(
            cleaned[0].reasoning_content.as_deref(),
            Some(" keep\nreasoning ")
        );
    }
}

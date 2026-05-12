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
                if !msg.tool_calls.is_empty() {
                    chat_msg.tool_calls = Some(msg.tool_calls.clone());
                }
            }
            Role::Tool => {
                // Tool result messages: role=tool, tool_call_id, content
                chat_msg.role = "tool".into();
                if let Some(tr) = msg.tool_results.first() {
                    chat_msg.tool_call_id = Some(tr.tool_call_id.clone());
                    chat_msg.content = Some(ChatMessageContent::Text(tr.result.clone()));
                }
            }
            _ => {}
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
            if let Some(ref mut content) = cleaned.content {
                match content {
                    ChatMessageContent::Text(ref mut s) => {
                        let sanitized = s.replace('\0', "").trim().to_string();
                        if sanitized.is_empty() {
                            cleaned.content = None;
                        } else {
                            *s = sanitized;
                        }
                    }
                    ChatMessageContent::Parts(parts) => {
                        for part in parts {
                            if let Some(ref mut text) = part.text {
                                *text = text.replace('\0', "").trim().to_string();
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

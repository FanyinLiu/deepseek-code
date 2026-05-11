use crate::deepseek::{
    MessageContent, MessageId, MessageVisibility, ProtocolMessage, ReasoningState, Role, SubTurnId,
    ToolCall, ToolResultRecord, ToolTurnId, TurnId,
};

/// Manages the `reasoning_content` lifecycle across tool-call sub-turns.
///
/// Critical rules:
/// 1. Each assistant message with `tool_calls` preserves its `reasoning_content`
///    for the duration of the tool-call sub-turn.
/// 2. When a new user turn begins, cleanup the previous turn's reasoning.
/// 3. Reasoning is marked `InternalProtocolState` after the tool loop completes.
pub struct ReasoningManager;

impl ReasoningManager {
    /// Start a new user turn: cleanup reasoning from the previous turn.
    pub fn begin_user_turn(
        state: &mut ReasoningState,
        messages: &mut [ProtocolMessage],
        turn_id: TurnId,
    ) {
        // Cleanup previous turn's reasoning
        for msg_id in state.preserved_assistant_messages.drain(..) {
            if let Some(msg) = messages.iter_mut().find(|m| m.id == msg_id) {
                msg.visibility = MessageVisibility::AuditOnly;
            }
        }

        state.active_tool_turn = None;
        state.last_cleanup_at_turn = Some(turn_id);
    }

    /// Start a tool-call sub-turn: the assistant wants to call tools.
    /// Preserve this message's `reasoning_content` for the loop.
    pub fn begin_tool_turn(
        state: &mut ReasoningState,
        assistant_msg: &ProtocolMessage,
    ) -> ToolTurnId {
        let turn_id = ToolTurnId::new_v4();

        state.active_tool_turn = Some(turn_id);
        state.preserved_assistant_messages.push(assistant_msg.id);

        turn_id
    }

    /// Check if a given message's reasoning should be carried forward.
    #[must_use]
    pub fn should_preserve_reasoning(msg: &ProtocolMessage, state: &ReasoningState) -> bool {
        if msg.role != Role::Assistant {
            return false;
        }
        if msg.reasoning_content.is_none() {
            return false;
        }
        if msg.tool_calls.is_empty() {
            return false;
        }
        // Only preserve if this message is in the active preserved set
        state.preserved_assistant_messages.contains(&msg.id)
    }

    /// Complete the tool loop: move preserved reasoning to audit-only.
    pub fn complete_tool_loop(state: &mut ReasoningState, messages: &mut [ProtocolMessage]) {
        for msg_id in state.preserved_assistant_messages.drain(..) {
            if let Some(msg) = messages.iter_mut().find(|m| m.id == msg_id) {
                msg.visibility = MessageVisibility::InternalProtocolState;
            }
        }
        state.active_tool_turn = None;
    }

    /// Create a new `ProtocolMessage` for an assistant response.
    pub fn new_assistant_message(
        content: &str,
        reasoning_content: Option<&str>,
        tool_calls: &[ToolCall],
        turn_id: TurnId,
        sub_turn_id: Option<SubTurnId>,
        has_tools: bool,
    ) -> ProtocolMessage {
        ProtocolMessage {
            id: MessageId::new_v4(),
            role: Role::Assistant,
            content: MessageContent::from(content),
            reasoning_content: reasoning_content.map(std::string::ToString::to_string),
            tool_calls: tool_calls.to_vec(),
            tool_results: Vec::new(),
            turn_id,
            sub_turn_id,
            visibility: if has_tools {
                MessageVisibility::InternalProtocolState
            } else {
                MessageVisibility::UserVisible
            },
        }
    }

    /// Create a `ProtocolMessage` for a tool result.
    #[must_use]
    pub fn new_tool_result_message(
        tool_call_id: &str,
        tool_name: &str,
        result: &str,
        is_error: bool,
        turn_id: TurnId,
        sub_turn_id: SubTurnId,
    ) -> ProtocolMessage {
        ProtocolMessage {
            id: MessageId::new_v4(),
            role: Role::Tool,
            content: MessageContent::from(result),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: vec![ToolResultRecord {
                tool_call_id: tool_call_id.to_string(),
                name: tool_name.to_string(),
                result: result.to_string(),
                is_error,
            }],
            turn_id,
            sub_turn_id: Some(sub_turn_id),
            visibility: MessageVisibility::InternalProtocolState,
        }
    }
}

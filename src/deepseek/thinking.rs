use super::models::{
    ExecutionLane, ProtocolMessage, ReasoningEffort, Role, ThinkingConfig, ThinkingMode, ToolTurnId,
};

/// Build `ThinkingConfig` for a given `ExecutionLane` and user preferences.
#[must_use]
pub fn thinking_config_for_lane(
    lane: &ExecutionLane,
    mode: &ThinkingMode,
    effort: &ReasoningEffort,
) -> Option<ThinkingConfig> {
    match mode {
        ThinkingMode::Off => Some(ThinkingConfig::disabled()),
        ThinkingMode::On => {
            if lane.requires_thinking() || *lane == ExecutionLane::ChatThinking {
                Some(ThinkingConfig {
                    thinking_type: "enabled".into(),
                    effort: Some(effort.to_string()),
                })
            } else {
                Some(ThinkingConfig::disabled())
            }
        }
        ThinkingMode::Auto => {
            if lane.requires_thinking() {
                Some(ThinkingConfig {
                    thinking_type: "enabled".into(),
                    effort: Some(effort.to_string()),
                })
            } else {
                None // let API decide
            }
        }
    }
}

/// Determine if a given assistant `ProtocolMessage` still needs its
/// `reasoning_content` preserved for an ongoing tool loop.
#[must_use]
pub fn should_preserve_reasoning(
    msg: &ProtocolMessage,
    active_tool_turn: Option<&ToolTurnId>,
) -> bool {
    if msg.role != Role::Assistant {
        return false;
    }
    if msg.reasoning_content.is_none() {
        return false;
    }
    if msg.tool_calls.is_empty() {
        // No tool calls — reasoning is not needed for next round
        return false;
    }
    // Has tool calls — preserve if this is the active tool turn
    match (active_tool_turn, &msg.sub_turn_id) {
        (Some(active), Some(sub)) => active == sub,
        _ => false,
    }
}

/// Extract `reasoning_content` from an assistant message that must be
/// carried forward into the next API request during a tool loop.
#[must_use]
pub fn extract_reasoning_for_tool_loop(msg: &ProtocolMessage) -> Option<&str> {
    if msg.role != Role::Assistant {
        return None;
    }
    msg.reasoning_content.as_deref()
}

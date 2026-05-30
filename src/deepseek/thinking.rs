use super::models::{
    ExecutionLane, ProtocolMessage, ReasoningEffort, Role, ThinkingConfig, ThinkingMode,
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

/// DeepSeek's recommended sampling temperature for a lane.
///
/// Coding / planning / structured-output work uses 0.0 (deterministic, per
/// DeepSeek's official parameter guidance for coding/math); chat is left at the
/// API default. Thinking turns ignore temperature, so this only takes effect on
/// non-thinking model calls (e.g. an auto-tiered Flash edit).
#[must_use]
pub fn temperature_for_lane(lane: &ExecutionLane) -> Option<f32> {
    match lane {
        ExecutionLane::ToolLoopThinking
        | ExecutionLane::PlanThinking
        | ExecutionLane::JsonOutput => Some(0.0),
        ExecutionLane::ChatNonThinking
        | ExecutionLane::ChatThinking
        | ExecutionLane::FimNonThinking => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coding_lanes_use_deterministic_temperature() {
        assert_eq!(
            temperature_for_lane(&ExecutionLane::ToolLoopThinking),
            Some(0.0)
        );
        assert_eq!(
            temperature_for_lane(&ExecutionLane::PlanThinking),
            Some(0.0)
        );
        assert_eq!(temperature_for_lane(&ExecutionLane::JsonOutput), Some(0.0));
    }

    #[test]
    fn chat_and_fim_lanes_leave_temperature_at_default() {
        assert_eq!(temperature_for_lane(&ExecutionLane::ChatNonThinking), None);
        assert_eq!(temperature_for_lane(&ExecutionLane::ChatThinking), None);
        assert_eq!(temperature_for_lane(&ExecutionLane::FimNonThinking), None);
    }
}

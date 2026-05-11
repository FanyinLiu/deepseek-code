use serde::de::DeserializeOwned;

use super::errors::DeepSeekError;
use super::models::{
    ChatMessage, ChatMessageContent, ChatRequest, ChatResponse, DeepSeekModel, ResponseFormat,
    ThinkingConfig,
};

/// Build a `ChatRequest` configured for JSON output mode.
/// Requires: `response_format={"type":"json_object"}` and explicit instruction.
#[must_use]
pub fn json_request(
    model: &DeepSeekModel,
    _system_prompt: &str,
    user_prompt: &str,
    json_schema_description: &str,
) -> ChatRequest {
    let system_msg = ChatMessage {
        role: "system".into(),
        content: Some(ChatMessageContent::Text(format!(
            "Return JSON only. {json_schema_description} Do not wrap in markdown code fences."
        ))),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    };

    let user_msg = ChatMessage {
        role: "user".into(),
        content: Some(ChatMessageContent::Text(user_prompt.to_string())),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    };

    ChatRequest {
        model: model.to_string(),
        messages: vec![system_msg, user_msg],
        tools: None,
        thinking: Some(ThinkingConfig::disabled()),
        response_format: Some(ResponseFormat::json_object()),
        stream: false,
        max_tokens: Some(4096),
    }
}

/// Parse JSON output from a `ChatResponse`, with fallback handling.
pub fn parse_json_response<T: DeserializeOwned>(
    response: &ChatResponse,
) -> Result<T, DeepSeekError> {
    let content = response
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref().and_then(|cc| cc.as_text()))
        .ok_or(DeepSeekError::EmptyContent)?;

    // Try direct parse first
    if let Ok(val) = serde_json::from_str::<T>(content) {
        Ok(val)
    } else {
        // Try stripping markdown fences
        let stripped = content
            .trim()
            .strip_prefix("```json")
            .and_then(|s| s.strip_suffix("```"))
            .or_else(|| {
                content
                    .trim()
                    .strip_prefix("```")
                    .and_then(|s| s.strip_suffix("```"))
            })
            .unwrap_or(content)
            .trim()
            .to_string();
        serde_json::from_str::<T>(&stripped).map_err(DeepSeekError::Parse)
    }
}

/// Validate that a JSON output response has non-empty content.
pub fn validate_json_response(response: &ChatResponse) -> Result<(), DeepSeekError> {
    let content = response
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref().and_then(|cc| cc.as_text()))
        .unwrap_or("");

    if content.trim().is_empty() {
        return Err(DeepSeekError::EmptyContent);
    }
    Ok(())
}

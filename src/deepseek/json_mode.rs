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
        temperature: Some(0.0),
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

    if let Ok(val) = serde_json::from_str::<T>(content) {
        return Ok(val);
    }

    if let Some(fenced) = extract_fenced_json(content) {
        if let Ok(val) = serde_json::from_str::<T>(fenced) {
            return Ok(val);
        }
    }

    if let Some(json_span) = extract_balanced_json(content) {
        if let Ok(val) = serde_json::from_str::<T>(json_span) {
            return Ok(val);
        }
    }

    serde_json::from_str::<T>(content.trim()).map_err(DeepSeekError::Parse)
}

fn extract_fenced_json(content: &str) -> Option<&str> {
    let mut search_start = 0;
    while let Some(open_rel) = content[search_start..].find("```") {
        let open = search_start + open_rel;
        let label_start = open + 3;
        let after_open = &content[label_start..];
        let newline_rel = after_open.find('\n')?;
        let label = after_open[..newline_rel].trim();
        let body_start = label_start + newline_rel + 1;
        let body = &content[body_start..];
        let close_rel = body.find("```")?;
        if label.is_empty() || label.eq_ignore_ascii_case("json") {
            return Some(body[..close_rel].trim());
        }
        search_start = body_start + close_rel + 3;
    }
    None
}

fn extract_balanced_json(content: &str) -> Option<&str> {
    for (start, ch) in content.char_indices() {
        if !matches!(ch, '{' | '[') {
            continue;
        }
        if let Some(end) = balanced_json_end(&content[start..]) {
            return Some(content[start..start + end].trim());
        }
    }
    None
}

fn balanced_json_end(candidate: &str) -> Option<usize> {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in candidate.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if stack.pop() != Some(ch) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
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

#[cfg(test)]
mod tests {
    use super::super::models::{ChatResponseMessage, Choice};
    use super::*;

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Parsed {
        ok: bool,
        label: String,
    }

    fn response_with_content(content: &str) -> ChatResponse {
        ChatResponse {
            id: "test".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatResponseMessage {
                    role: Some("assistant".into()),
                    content: Some(ChatMessageContent::Text(content.into())),
                    reasoning_content: None,
                    reasoning_details: None,
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        }
    }

    #[test]
    fn parses_json_fence_with_extra_prose() {
        let response = response_with_content(
            "Here is the result:\n```json\n{\"ok\":true,\"label\":\"done\"}\n```\nThanks.",
        );

        let parsed: Parsed = parse_json_response(&response).expect("parse fenced json");

        assert_eq!(
            parsed,
            Parsed {
                ok: true,
                label: "done".into()
            }
        );
    }

    #[test]
    fn parses_plain_fence_and_balanced_json_span() {
        let plain_fence = response_with_content("```\n{\"ok\":true,\"label\":\"plain\"}\n```");
        let parsed: Parsed = parse_json_response(&plain_fence).expect("parse plain fence");
        assert_eq!(parsed.label, "plain");

        let prose =
            response_with_content("classification follows: {\"ok\":true,\"label\":\"span\"}.");
        let parsed: Parsed = parse_json_response(&prose).expect("parse json span");
        assert_eq!(parsed.label, "span");
    }
}

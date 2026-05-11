use super::models::{DeepSeekModel, ExecutionLane};

/// Build a FIM (Fill-in-the-Middle) completion request.
/// FIM requests always run in a non-thinking lane.
/// Uses the `/beta/completions` or `/completions` endpoint.
#[must_use]
pub fn fim_request(
    model: &DeepSeekModel,
    prefix: &str,
    suffix: &str,
    max_tokens: Option<u32>,
) -> FimRequest {
    FimRequest {
        model: model.to_string(),
        prompt: prefix.to_string(),
        suffix: suffix.to_string(),
        max_tokens,
        temperature: Some(0.0),
        stream: false,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FimRequest {
    pub model: String,
    pub prompt: String,
    pub suffix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub stream: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FimResponse {
    pub choices: Vec<FimChoice>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FimChoice {
    pub text: String,
    pub index: u32,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// Validate that FIM is not being mixed with thinking mode.
pub fn validate_fim_lane(lane: &ExecutionLane) -> Result<(), String> {
    if lane.requires_thinking() {
        return Err(format!("FIM must run in a non-thinking lane, got {lane:?}"));
    }
    Ok(())
}

/// Determine if a user request is suitable for FIM.
/// FIM is for small, single-file completions, not agentic tasks.
#[must_use]
pub fn is_fim_suitable(user_input: &str) -> bool {
    let input = user_input.trim().to_lowercase();
    let agentic_markers = [
        "fix",
        "debug",
        "refactor",
        "implement",
        "add a feature",
        "create",
        "explain",
        "search",
        "find",
        "run test",
        "deploy",
    ];

    // Short completions are FIM-suitable
    if input.len() < 200 && !agentic_markers.iter().any(|m| input.contains(m)) {
        return true;
    }
    false
}

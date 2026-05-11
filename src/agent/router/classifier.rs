use std::time::Instant;

use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::json_mode::parse_json_response;
use crate::deepseek::{
    ChatMessage, ChatMessageContent, ChatRequest, DeepSeekModel, ResponseFormat, ThinkingConfig,
};

use super::{ReasonCode, RiskFlag};

/// Output of the model-based classifier.
#[derive(Debug, Clone)]
pub struct ModelAssessment {
    pub score: u32,
    pub confidence: f64,
    pub reason_codes: Vec<ReasonCode>,
    pub predicted_write_files: u32,
    pub predicted_commands: u32,
    pub predicted_duration_ms: u32,
    pub risk_flags: Vec<RiskFlag>,
    pub model_name: String,
    pub latency_ms: u64,
}

/// Call `DeepSeek` v4-flash for structured complexity classification.
///
/// Uses JSON output mode with a strict schema description.
/// Implements retry + fallback for empty-content edge cases.
pub async fn classify_with_model(
    task_input: &str,
    project_context: Option<&str>,
    client: &DeepSeekClient,
) -> Result<ModelAssessment, crate::deepseek::errors::DeepSeekError> {
    let start = Instant::now();
    let model = DeepSeekModel::Flash;

    let system_prompt = build_system_prompt();
    let user_prompt = build_user_prompt(task_input, project_context);

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: Some(ChatMessageContent::Text(system_prompt)),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "user".into(),
                content: Some(ChatMessageContent::Text(user_prompt)),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ],
        tools: None,
        thinking: Some(ThinkingConfig::disabled()),
        response_format: Some(ResponseFormat::json_object()),
        stream: false,
        max_tokens: Some(4096),
    };

    // Attempt with retry
    let response = client.chat_with_retry(&request).await?;
    let latency = start.elapsed().as_millis() as u64;

    let raw: RawClassification = parse_json_response(&response)?;
    let assessment = convert_raw(raw, &model, latency);
    Ok(assessment)
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn build_system_prompt() -> String {
    r#"You are a task-complexity classifier for a coding agent.
Your job is to analyse a user task and emit a structured JSON assessment.

Scoring guidelines (0 = trivial, 100 = extremely complex):
- 0–20: Read-only, single-file cosmetic changes, no commands, no tests.
- 21–40: Single-file logic change, no external deps, no commands.
- 41–60: Multi-file or needs tests/build, still local and safe.
- 61–80: Cross-module, needs commands, networking, or significant refactoring.
- 81–100: High-risk (delete, migrate, deploy, permission changes), or very large scope.

Confidence guidelines:
- 0.9+ if the task is unambiguous and you can predict scope clearly.
- 0.6–0.8 if some details are missing but intent is clear.
- <0.6 if the task is vague, lacks acceptance criteria, or refers to missing context.

Always include at least one reason code. If multiple apply, include all relevant ones.
If the task is ambiguous and you cannot infer acceptance criteria, include AMBIGUOUS_TASK.

Respond with JSON only. Do not wrap in markdown code fences.
Schema:
{
  "score": integer (0-100),
  "confidence": number (0.0-1.0),
  "reason_codes": ["READ_ONLY", "SINGLE_FILE_SAFE", "MULTI_FILE", "SHELL_REQUIRED", "TEST_REQUIRED", "NETWORK_REQUIRED", "HIGH_RISK", "AMBIGUOUS_TASK"],
  "predicted_write_files": integer (0+),
  "predicted_commands": integer (0+),
  "predicted_duration_ms": integer,
  "risk_flags": ["DELETE", "MIGRATION", "DEPLOY", "PERMISSION_CHANGE", "MAIN_BRANCH_PUSH"]
}
"#.trim().to_string()
}

fn build_user_prompt(task_input: &str, project_context: Option<&str>) -> String {
    let mut prompt = format!("Task: \"{task_input}\"\n");
    if let Some(ctx) = project_context {
        prompt.push_str(&format!("\nProject context (truncated):\n{ctx}\n"));
    }
    prompt.push_str("\nEmit the classification JSON now.");
    prompt
}

// ---------------------------------------------------------------------------
// Raw JSON schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct RawClassification {
    score: u32,
    confidence: f64,
    reason_codes: Vec<String>,
    #[serde(default)]
    predicted_write_files: u32,
    #[serde(default)]
    predicted_commands: u32,
    #[serde(default)]
    predicted_duration_ms: u32,
    #[serde(default)]
    risk_flags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

fn convert_raw(raw: RawClassification, model: &DeepSeekModel, latency: u64) -> ModelAssessment {
    ModelAssessment {
        score: raw.score.min(100),
        confidence: raw.confidence.clamp(0.0, 1.0),
        reason_codes: raw
            .reason_codes
            .into_iter()
            .filter_map(parse_reason_code)
            .collect(),
        predicted_write_files: raw.predicted_write_files,
        predicted_commands: raw.predicted_commands,
        predicted_duration_ms: raw.predicted_duration_ms,
        risk_flags: raw
            .risk_flags
            .into_iter()
            .filter_map(parse_risk_flag)
            .collect(),
        model_name: model.to_string(),
        latency_ms: latency,
    }
}

fn parse_reason_code(s: String) -> Option<ReasonCode> {
    match s.as_str() {
        "READ_ONLY" => Some(ReasonCode::ReadOnly),
        "SINGLE_FILE_SAFE" => Some(ReasonCode::SingleFileSafe),
        "MULTI_FILE" => Some(ReasonCode::MultiFile),
        "SHELL_REQUIRED" => Some(ReasonCode::ShellRequired),
        "TEST_REQUIRED" => Some(ReasonCode::TestRequired),
        "NETWORK_REQUIRED" => Some(ReasonCode::NetworkRequired),
        "HIGH_RISK" => Some(ReasonCode::HighRisk),
        "AMBIGUOUS_TASK" => Some(ReasonCode::AmbiguousTask),
        _ => None,
    }
}

fn parse_risk_flag(s: String) -> Option<RiskFlag> {
    match s.as_str() {
        "DELETE" => Some(RiskFlag::Delete),
        "MIGRATION" => Some(RiskFlag::Migration),
        "DEPLOY" => Some(RiskFlag::Deploy),
        "PERMISSION_CHANGE" => Some(RiskFlag::PermissionChange),
        "MAIN_BRANCH_PUSH" => Some(RiskFlag::MainBranchPush),
        _ => None,
    }
}

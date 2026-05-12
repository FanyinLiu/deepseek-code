use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::json_mode;
use crate::deepseek::DeepSeekModel;

use super::schema::Plan;

/// Generate a plan using `DeepSeek` in Plan Mode.
/// This is a read-only operation — no files are modified.
pub async fn generate_plan(
    client: &DeepSeekClient,
    model: &DeepSeekModel,
    task: &str,
    search_context: Option<&str>,
    project_rules: Option<&str>,
) -> Result<Plan, anyhow::Error> {
    let language_instruction = if contains_cjk(task) {
        "Use Simplified Chinese for all human-facing string fields: summary, target_files.reason, steps, risks.description, and verification."
    } else {
        "Use the same language as the user task for all human-facing string fields: summary, target_files.reason, steps, risks.description, and verification."
    };
    let rule_context = project_rules
        .filter(|rules| !rules.trim().is_empty())
        .map(|rules| format!("Project rules that must be respected:\n{rules}\n\n"))
        .unwrap_or_default();

    let user_prompt = if let Some(ctx) = search_context {
        format!(
            "{rule_context}Task: {task}\n\n\
             Relevant project context from search:\n{ctx}\n\n\
             Generate a plan for completing this task. \
             {language_instruction} \
             Include: target files, steps, risks, verification steps."
        )
    } else {
        format!(
            "{rule_context}Task: {task}\n\n\
             Generate a plan for completing this task. \
             {language_instruction} \
             First identify what files you need to read. \
             Include: target files, steps, risks, verification steps."
        )
    };

    let json_schema = r#"{
        "summary": "One-line description of the plan",
        "target_files": [{"path": "relative/path", "reason": "why this file is relevant"}],
        "steps": ["step 1", "step 2"],
        "risks": [{"level": "low|medium|high|critical", "description": "risk description"}],
        "verification": ["verification step 1"],
        "requires_write": true,
        "requires_command": false,
        "recommended_model": "deepseek-v4-flash",
        "thinking": true
    }"#;

    let request = json_mode::json_request(
        model,
        "You are a software engineering planning assistant. \
         You work in a coding agent that is READ-ONLY during plan mode. \
         Analyze the task and generate a detailed execution plan. \
         You CANNOT modify files during planning.",
        &user_prompt,
        &format!("Output MUST be valid JSON matching this schema:\n{json_schema}"),
    );

    let response = client.chat_with_retry(&request).await?;

    // Validate response before parsing
    json_mode::validate_json_response(&response)?;

    let plan: Plan = json_mode::parse_json_response(&response)?;

    // Fallback: if JSON parsing fails, return a minimal plan
    // (parse_json_response already handles markdown fences)

    Ok(plan)
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch as u32,
            0x4E00..=0x9FFF
                | 0x3400..=0x4DBF
                | 0x20000..=0x2A6DF
                | 0x2A700..=0x2B73F
                | 0x2B740..=0x2B81F
                | 0x2B820..=0x2CEAF
        )
    })
}

/// Quick plan — shorter, fewer steps, for simpler tasks.
pub async fn generate_quick_plan(
    client: &DeepSeekClient,
    model: &DeepSeekModel,
    task: &str,
) -> Result<Plan, anyhow::Error> {
    let request = json_mode::json_request(
        model,
        "You are a concise software planning assistant. Generate a minimal plan.",
        &format!("Task: {task}\n\nGenerate a minimal execution plan."),
        "Output only the JSON plan object.",
    );

    let response = client.chat_with_retry(&request).await?;
    json_mode::validate_json_response(&response)?;
    json_mode::parse_json_response(&response).map_err(|e| anyhow::anyhow!("{e}"))
}

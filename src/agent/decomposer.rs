use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::{
    ChatMessage, ChatMessageContent, ChatRequest, DeepSeekModel, ResponseFormat, ThinkingConfig,
};

use super::subagent::{MergeStrategy, ParallelBatch, SubagentConfig, SubagentTask, SubagentType};

/// LLM-driven task decomposition.
///
/// Uses `DeepSeek` v4-flash to analyze a task and break it into
/// parallel or sequential sub-tasks with appropriate agent types.
pub struct LlmDecomposer;

impl LlmDecomposer {
    /// Ask the LLM to decompose a task into subagent assignments.
    ///
    /// Returns `None` if the LLM decides the task should not be decomposed
    /// (e.g., it is too simple or inherently sequential).
    pub async fn decompose(
        client: &DeepSeekClient,
        task_description: &str,
        task_prompt: &str,
    ) -> Option<ParallelBatch> {
        // Fast pre-filter: skip LLM call when task is obviously not decomposable.
        if Self::is_clearly_single_task(task_description, task_prompt) {
            return None;
        }

        let system_prompt = Self::build_system_prompt();
        let user_prompt = format!(
            "Task description: {task_description}\n\nTask prompt:\n{task_prompt}\n\nAnalyze this task and decide whether it should be split into parallel subagent tasks. If yes, return the decomposition JSON."
        );

        let request = ChatRequest {
            model: DeepSeekModel::Pro.to_string(),
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
            response_format: Some(ResponseFormat::json_object()),
            thinking: Some(ThinkingConfig::disabled()),
            stream: false,
            max_tokens: Some(4096),
            tools: None,
        };

        let response = match client.chat_with_retry(&request).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("LLM decomposition failed: {}", e);
                return None;
            }
        };

        let content = response
            .choices
            .first()?
            .message
            .content
            .as_ref()?
            .as_text()?;
        Self::parse_decomposition(content)
    }

    // ------------------------------------------------------------------
    // Pre-filter
    // ------------------------------------------------------------------

    /// Return true when the task is obviously a single-agent job, avoiding an
    /// unnecessary LLM round-trip for decomposition analysis.
    fn is_clearly_single_task(description: &str, prompt: &str) -> bool {
        let combined = format!("{description} {prompt}");
        let words: usize = combined.split_whitespace().count();

        // Very short tasks are never worth splitting.
        if words < 8 {
            return true;
        }

        // Must contain at least one conjunction/quantifier that implies multiple
        // independent concerns before we bother asking the LLM.
        let lower = combined.to_lowercase();
        let parallel_signals = [
            " and ",
            " also ",
            " plus ",
            " & ",
            " additionally ",
            " both ",
            " as well as ",
            " meanwhile ",
            " simultaneously ",
        ];
        !parallel_signals.iter().any(|s| lower.contains(s))
    }

    // ------------------------------------------------------------------
    // Prompt construction
    // ------------------------------------------------------------------

    fn build_system_prompt() -> String {
        r#"You are a task decomposition engine for a multi-agent coding system.

Your job is to analyze a user task and decide whether it can be broken into independent sub-tasks that can run in parallel via specialized subagents.

Available subagent types:
- general-purpose: any coding task
- code-explorer: read-only codebase exploration
- code-reviewer: code review and critique (read-only)
- planner: architecture and planning (read-only)
- test-runner: running tests and reporting results
- architect: API/schema design

Rules:
1. Only decompose if tasks are TRULY independent (no file overlap, no dependencies).
2. Prefer parallel execution for independent research, review, or exploration tasks.
3. Do NOT decompose simple tasks (single file edit, one-line fix, greeting).
4. Do NOT decompose sequential tasks (schema -> API -> frontend).

Response format (JSON):
{
  "should_decompose": true,
  "reasoning": "Why this decomposition makes sense",
  "tasks": [
    {
      "description": "Short task name",
      "subagent_type": "code-explorer",
      "prompt": "Detailed instructions for this subagent",
      "focus_files": ["optional/file.rs"],
      "context": "Optional extra context"
    }
  ],
  "independent": true,
  "merge_strategy": "synthesize"
}

If the task should NOT be decomposed, return:
{
  "should_decompose": false,
  "reasoning": "Why a single agent is better"
}

merge_strategy options: concatenate, synthesize, best_of_n
"#
        .to_string()
    }

    // ------------------------------------------------------------------
    // Parsing
    // ------------------------------------------------------------------

    fn parse_decomposition(content: &str) -> Option<ParallelBatch> {
        #[derive(Debug, serde::Deserialize)]
        struct DecompositionResponse {
            #[serde(default)]
            should_decompose: bool,
            #[allow(dead_code)]
            #[serde(default)]
            reasoning: String,
            #[serde(default)]
            tasks: Vec<DecomposedTask>,
            #[serde(default)]
            independent: bool,
            #[serde(default = "default_merge_strategy")]
            merge_strategy: String,
        }

        #[derive(Debug, serde::Deserialize)]
        struct DecomposedTask {
            description: String,
            #[serde(default = "default_subagent_type")]
            subagent_type: String,
            prompt: String,
            #[serde(default)]
            focus_files: Vec<String>,
            #[serde(default)]
            context: Option<String>,
        }

        let parsed: DecompositionResponse = match serde_json::from_str(content) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse decomposition JSON: {}\nContent: {}",
                    e,
                    content
                );
                return None;
            }
        };

        if !parsed.should_decompose || parsed.tasks.is_empty() {
            return None;
        }

        let tasks: Vec<(SubagentConfig, SubagentTask)> = parsed
            .tasks
            .into_iter()
            .map(|t| {
                let subagent_type = t
                    .subagent_type
                    .parse()
                    .unwrap_or(SubagentType::GeneralPurpose);
                let config = SubagentConfig::for_type(subagent_type);
                let task = SubagentTask {
                    description: t.description,
                    prompt: t.prompt,
                    context: t.context,
                    focus_files: t.focus_files,
                    expected_output: None,
                };
                (config, task)
            })
            .collect();

        let merge_strategy = match parsed.merge_strategy.as_str() {
            "concatenate" => MergeStrategy::Concatenate,
            "best_of_n" => MergeStrategy::BestOfN,
            _ => MergeStrategy::Synthesize,
        };

        Some(ParallelBatch {
            tasks,
            independent: parsed.independent,
            merge_strategy,
            shared_context: None,
        })
    }
}

fn default_merge_strategy() -> String {
    "synthesize".to_string()
}

fn default_subagent_type() -> String {
    "general-purpose".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_decomposition_valid() {
        let json = r#"{
            "should_decompose": true,
            "reasoning": "Two independent files to review",
            "tasks": [
                {
                    "description": "Review auth module",
                    "subagent_type": "code-reviewer",
                    "prompt": "Review src/auth.rs for security issues",
                    "focus_files": ["src/auth.rs"]
                },
                {
                    "description": "Review db module",
                    "subagent_type": "code-reviewer",
                    "prompt": "Review src/db.rs for SQL injection risks",
                    "focus_files": ["src/db.rs"]
                }
            ],
            "independent": true,
            "merge_strategy": "synthesize"
        }"#;

        let result = LlmDecomposer::parse_decomposition(json);
        assert!(result.is_some());
        let batch = result.unwrap();
        assert_eq!(batch.tasks.len(), 2);
        assert!(batch.independent);
        assert!(matches!(batch.merge_strategy, MergeStrategy::Synthesize));
    }

    #[test]
    fn test_parse_decomposition_no_decompose() {
        let json = r#"{
            "should_decompose": false,
            "reasoning": "Too simple"
        }"#;

        let result = LlmDecomposer::parse_decomposition(json);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_decomposition_invalid_json() {
        let json = "not json at all";
        let result = LlmDecomposer::parse_decomposition(json);
        assert!(result.is_none());
    }
}

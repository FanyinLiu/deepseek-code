use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{mpsc, Semaphore};

use crate::deepseek::client::DeepSeekClient;

use super::bus::MessageBus;
use super::decomposer::LlmDecomposer;
use super::orchestrator::AgentEvent;
use super::subagent::{
    MergeStrategy, ParallelBatch, SubagentConfig, SubagentExecutor, SubagentRegistry,
    SubagentResult, SubagentTask, SubagentType,
};

const DEFAULT_MAX_PARALLEL_SUBAGENTS: usize = 8;

/// The supervisor orchestrates multi-agent execution.
///
/// It is responsible for:
/// 1. Deciding whether a task should be delegated to subagents
/// 2. Decomposing tasks into parallel or sequential sub-tasks
/// 3. Dispatching subagents and collecting results
/// 4. Synthesizing results for the parent orchestrator
#[derive(Clone)]
pub struct Supervisor {
    client: Arc<DeepSeekClient>,
    project_root: PathBuf,
    max_parallel_subagents: usize,
    pub registry: SubagentRegistry,
}

impl Supervisor {
    #[must_use]
    pub fn new(client: Arc<DeepSeekClient>, project_root: PathBuf) -> Self {
        let registry = SubagentRegistry::load_from_project(&project_root);
        let max_parallel_subagents = configured_max_parallel_subagents(&project_root);
        Self {
            client,
            project_root,
            max_parallel_subagents,
            registry,
        }
    }

    /// Run a single subagent task.
    pub async fn run_single(
        &self,
        config: SubagentConfig,
        task: SubagentTask,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> SubagentResult {
        let executor =
            SubagentExecutor::new(self.client.clone(), self.project_root.clone(), config);
        executor.run(&task, event_tx).await
    }

    /// Run a batch of subagent tasks in parallel.
    ///
    /// Tasks are spawned concurrently up to the configured supervisor limit.
    /// Results are returned in the same order.
    pub async fn run_parallel(
        &self,
        batch: ParallelBatch,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Vec<SubagentResult> {
        let mut handles = Vec::with_capacity(batch.tasks.len());
        let semaphore = Arc::new(Semaphore::new(self.max_parallel_subagents));

        // Create a shared message bus for inter-agent coordination
        let bus = MessageBus::new(64);

        for (config, mut task) in batch.tasks {
            // Inject shared context into each task's context field.
            if let Some(ref ctx) = batch.shared_context {
                let merged = match task.context {
                    Some(ref existing) => format!("{existing}\n\n## Shared Context\n\n{ctx}"),
                    None => format!("## Shared Context\n\n{ctx}"),
                };
                task.context = Some(merged);
            }
            let executor =
                SubagentExecutor::new(self.client.clone(), self.project_root.clone(), config)
                    .with_bus(bus.clone());
            let event_tx = event_tx.clone();
            let semaphore = semaphore.clone();
            let handle = tokio::spawn(async move {
                let Ok(_permit) = semaphore.acquire_owned().await else {
                    return subagent_failure(
                        "Subagent skipped because the supervisor limiter closed".to_string(),
                        "Supervisor limiter closed before the subagent could start".to_string(),
                        "supervisor limiter closed".to_string(),
                    );
                };
                executor.run(&task, &event_tx).await
            });
            handles.push(handle);
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    results.push(subagent_failure(
                        format!("Subagent panicked: {e}"),
                        format!("Panicked: {e}"),
                        e.to_string(),
                    ));
                }
            }
        }

        results
    }

    /// Decompose a complex task into sub-tasks (rule-based).
    ///
    /// For LLM-driven decomposition, use `decompose_async`.
    #[must_use]
    pub fn decompose(&self, task_description: &str, task_prompt: &str) -> Option<ParallelBatch> {
        let lower = task_description.to_lowercase();

        // Pattern: "review X and Y" or "review these files"
        if lower.contains("review")
            && (lower.contains("and ") || lower.contains("files") || lower.contains("changes"))
        {
            return self.decompose_review(task_description, task_prompt);
        }

        // Pattern: "explore X and Y" or "understand these modules"
        if lower.contains("explore") || lower.contains("understand") || lower.contains("research") {
            return self.decompose_explore(task_description, task_prompt);
        }

        // Pattern: "run tests" or "check tests"
        if lower.contains("test") && lower.contains("run") {
            return self.decompose_test(task_description, task_prompt);
        }

        // Pattern: "implement X and Y" or "fix A and B"
        if (lower.contains("implement") || lower.contains("fix") || lower.contains("refactor"))
            && (lower.contains("and ") || lower.contains("multiple"))
        {
            return self.decompose_implement(task_description, task_prompt);
        }

        None
    }

    /// Decompose a task using LLM-driven analysis.
    ///
    /// First tries rule-based decomposition for speed. If that fails,
    /// falls back to an LLM call to determine the optimal decomposition.
    pub async fn decompose_async(
        &self,
        task_description: &str,
        task_prompt: &str,
    ) -> Option<ParallelBatch> {
        // Phase 1: fast rule-based decomposition
        if let Some(batch) = self.decompose(task_description, task_prompt) {
            return Some(batch);
        }

        // Phase 2: LLM-driven decomposition
        LlmDecomposer::decompose(&self.client, task_description, task_prompt).await
    }

    // ------------------------------------------------------------------
    // Decomposition strategies
    // ------------------------------------------------------------------

    fn decompose_review(&self, _description: &str, prompt: &str) -> Option<ParallelBatch> {
        // Extract file paths from the prompt heuristically
        let files = extract_file_paths(prompt);
        if files.len() < 2 {
            return None;
        }

        let tasks: Vec<(SubagentConfig, SubagentTask)> = files
            .into_iter()
            .map(|file| {
                let config = SubagentConfig::for_type(SubagentType::CodeReviewer);
                let task = SubagentTask {
                    description: format!("Review {file}"),
                    prompt: format!(
                        "Review the file {file} for correctness, style, security, and performance issues. \
Provide clear, actionable feedback."
                    ),
                    context: Some(prompt.to_string()),
                    focus_files: vec![file],
                    expected_output: Some("Code review comments".to_string()),
                };
                (config, task)
            })
            .collect();

        Some(ParallelBatch {
            tasks,
            independent: true,
            merge_strategy: MergeStrategy::Concatenate,
            shared_context: None,
        })
    }

    fn decompose_explore(&self, _description: &str, prompt: &str) -> Option<ParallelBatch> {
        let files = extract_file_paths(prompt);
        if files.len() < 2 {
            return None;
        }

        let tasks: Vec<(SubagentConfig, SubagentTask)> = files
            .into_iter()
            .map(|file| {
                let config = SubagentConfig::for_type(SubagentType::CodeExplorer);
                let task = SubagentTask {
                    description: format!("Explore {file}"),
                    prompt: format!(
                        "Read and understand the file {file}. Summarize its purpose, key structures, and how it relates to the rest of the codebase."
                    ),
                    context: Some(prompt.to_string()),
                    focus_files: vec![file],
                    expected_output: Some("Summary of the file's purpose and structure".to_string()),
                };
                (config, task)
            })
            .collect();

        Some(ParallelBatch {
            tasks,
            independent: true,
            merge_strategy: MergeStrategy::Synthesize,
            shared_context: None,
        })
    }

    fn decompose_test(&self, _description: &str, prompt: &str) -> Option<ParallelBatch> {
        // Split by test suite or test file patterns
        let config = SubagentConfig::for_type(SubagentType::TestRunner);

        // Try to find multiple test commands or directories
        let tasks = vec![(
            config.clone(),
            SubagentTask {
                description: "Run unit tests".to_string(),
                prompt: prompt.to_string(),
                context: None,
                focus_files: Vec::new(),
                expected_output: Some("Test results summary".to_string()),
            },
        )];

        Some(ParallelBatch {
            tasks,
            independent: true,
            merge_strategy: MergeStrategy::Synthesize,
            shared_context: None,
        })
    }

    fn decompose_implement(&self, _description: &str, prompt: &str) -> Option<ParallelBatch> {
        let files = extract_file_paths(prompt);
        if files.len() < 2 {
            return None;
        }

        let tasks: Vec<(SubagentConfig, SubagentTask)> = files
            .into_iter()
            .map(|file| {
                let config = SubagentConfig::for_type(SubagentType::GeneralPurpose);
                let task = SubagentTask {
                    description: format!("Implement changes in {file}"),
                    prompt: format!(
                        "Make the necessary changes to {file} according to the following instructions:\n\n{prompt}"
                    ),
                    context: None,
                    focus_files: vec![file],
                    expected_output: Some("Implementation summary".to_string()),
                };
                (config, task)
            })
            .collect();

        Some(ParallelBatch {
            tasks,
            independent: false, // File changes may have dependencies
            merge_strategy: MergeStrategy::Synthesize,
            shared_context: None,
        })
    }

    /// Synthesize results from multiple subagents into a single summary.
    #[must_use]
    pub fn synthesize_results(
        &self,
        results: &[SubagentResult],
        strategy: &MergeStrategy,
    ) -> String {
        match strategy {
            MergeStrategy::Concatenate => {
                let mut out = String::from("# Parallel Subagent Results\n\n");
                for (i, result) in results.iter().enumerate() {
                    out.push_str(&format!(
                        "## Result {}\n\n{}",
                        i + 1,
                        result.format_for_parent(2000)
                    ));
                    out.push_str("\n\n---\n\n");
                }
                out
            }
            MergeStrategy::Synthesize => {
                let mut out = String::from("# Synthesized Subagent Results\n\n");
                let (successful, failed): (Vec<_>, Vec<_>) =
                    results.iter().partition(|r| r.success);

                out.push_str(&format!(
                    "**Successful:** {} / {}\n\n",
                    successful.len(),
                    results.len()
                ));

                if !failed.is_empty() {
                    out.push_str("## Failures\n\n");
                    for result in &failed {
                        out.push_str(&format!("- {}\n", result.summary));
                    }
                    out.push('\n');
                }

                out.push_str("## Summaries\n\n");
                for (i, result) in successful.iter().enumerate() {
                    out.push_str(&format!("### Subagent {}\n{}", i + 1, result.summary));
                    if !result.files_written.is_empty() {
                        out.push_str(&format!(
                            "\n\n**Modified:** {}",
                            result.files_written.join(", ")
                        ));
                    }
                    out.push_str("\n\n");
                }

                out
            }
            MergeStrategy::BestOfN => {
                // Prefer successful results with the most tangible output
                // (files written + tool calls used) rather than raw length.
                results
                    .iter()
                    .filter(|r| r.success && r.error.is_none())
                    .max_by_key(|r| r.files_written.len() + r.tool_calls_used.len())
                    .or_else(|| results.first())
                    .map_or_else(|| "No results".to_string(), |r| r.format_for_parent(4000))
            }
        }
    }
}

// ------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------

fn configured_max_parallel_subagents(project_root: &Path) -> usize {
    let mut configured = None;

    if let Some(home_dir) = dirs::home_dir() {
        let global_path = home_dir.join(".octocode").join("config.toml");
        configured = read_configured_subagent_max_parallel(&global_path).or(configured);
    }

    let root = crate::storage::config::normalize_project_root(project_root);
    let project_config = root.join(".octocode").join("config.toml");
    configured = read_configured_subagent_max_parallel(&project_config).or(configured);
    let local_config = root.join(".octocode").join("local.toml");
    configured = read_configured_subagent_max_parallel(&local_config).or(configured);

    configured
        .map(normalize_max_parallel_subagents)
        .unwrap_or(DEFAULT_MAX_PARALLEL_SUBAGENTS)
}

fn read_configured_subagent_max_parallel(path: &Path) -> Option<usize> {
    let content = std::fs::read_to_string(path).ok()?;
    let value = content.parse::<toml::Value>().ok()?;
    let max_parallel = value.get("subagent")?.get("max_parallel")?.as_integer()?;
    usize::try_from(max_parallel).ok()
}

fn normalize_max_parallel_subagents(limit: usize) -> usize {
    limit.max(1)
}

fn subagent_failure(summary: String, output: String, error: String) -> SubagentResult {
    SubagentResult {
        success: false,
        summary,
        output,
        tool_calls_used: Vec::new(),
        files_read: Vec::new(),
        files_written: Vec::new(),
        duration_ms: 0,
        token_usage: 0,
        error: Some(error),
        started_at: chrono::Utc::now(),
        completed_at: chrono::Utc::now(),
    }
}

/// Heuristically extract file paths from a prompt string.
fn extract_file_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Look for patterns like "src/foo.rs", "path/to/file.txt", etc.
    for word in text.split_whitespace() {
        let cleaned = word
            .trim_matches(|c: char| c.is_ascii_punctuation() && c != '/' && c != '\\' && c != '-');

        if cleaned.contains('/')
            && (cleaned.ends_with(".rs")
                || cleaned.ends_with(".ts")
                || cleaned.ends_with(".js")
                || cleaned.ends_with(".py")
                || cleaned.ends_with(".toml")
                || cleaned.ends_with(".md")
                || cleaned.ends_with(".json")
                || cleaned.ends_with(".yaml")
                || cleaned.ends_with(".yml")
                || cleaned.ends_with(".html")
                || cleaned.ends_with(".css")
                || cleaned.ends_with(".go")
                || cleaned.ends_with(".java")
                || cleaned.ends_with(".c")
                || cleaned.ends_with(".cpp")
                || cleaned.ends_with(".h"))
        {
            let path = cleaned.to_string();
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_file_paths() {
        let text = "Please review src/main.rs and src/lib.rs, also check tests/test_main.rs.";
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"src/lib.rs".to_string()));
        assert!(paths.contains(&"tests/test_main.rs".to_string()));
    }

    #[test]
    fn default_parallel_limit_is_eight_and_never_zero() {
        assert_eq!(DEFAULT_MAX_PARALLEL_SUBAGENTS, 8);
        assert_eq!(normalize_max_parallel_subagents(0), 1);
        assert_eq!(normalize_max_parallel_subagents(3), 3);
    }

    #[test]
    fn supervisor_reads_project_parallel_limit() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("local.toml"),
            "[subagent]\nmax_parallel = 2\n",
        )
        .expect("write config");

        let supervisor = Supervisor::new(
            Arc::new(DeepSeekClient::new("test".to_string())),
            root.path().to_path_buf(),
        );

        assert_eq!(supervisor.max_parallel_subagents, 2);
    }

    #[test]
    fn test_synthesize_concatenate() {
        let results = vec![
            SubagentResult {
                success: true,
                summary: "Found 3 issues".to_string(),
                output: "Detailed output A".to_string(),
                tool_calls_used: vec!["read_file".to_string()],
                files_read: vec!["a.rs".to_string()],
                files_written: vec![],
                duration_ms: 1000,
                token_usage: 500,
                error: None,
                started_at: chrono::Utc::now(),
                completed_at: chrono::Utc::now(),
            },
            SubagentResult {
                success: true,
                summary: "Looks good".to_string(),
                output: "Detailed output B".to_string(),
                tool_calls_used: vec!["read_file".to_string()],
                files_read: vec!["b.rs".to_string()],
                files_written: vec![],
                duration_ms: 800,
                token_usage: 300,
                error: None,
                started_at: chrono::Utc::now(),
                completed_at: chrono::Utc::now(),
            },
        ];

        let supervisor = Supervisor::new(
            Arc::new(DeepSeekClient::new("test".to_string())),
            PathBuf::from("."),
        );
        let out = supervisor.synthesize_results(&results, &MergeStrategy::Concatenate);
        assert!(out.contains("Parallel Subagent Results"));
        assert!(out.contains("Found 3 issues"));
        assert!(out.contains("Looks good"));
    }
}

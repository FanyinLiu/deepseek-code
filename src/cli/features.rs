use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;

use crate::agent::subagent::SubagentRegistry;
use crate::cli::resolve_project_root;
use crate::storage;

const MATRIX_RELATIVE_PATH: &str = "docs/cli_parity_matrix.md";
const EMBEDDED_MATRIX: &str = include_str!("../../docs/cli_parity_matrix.md");
const BUILT_IN_AGENTS: &[&str] = &[
    "general-purpose",
    "code-explorer",
    "code-reviewer",
    "planner",
    "test-runner",
    "architect",
    "security-auditor",
];

pub enum FeaturesCommand {
    Matrix { json: bool },
    Status { json: bool },
    Recommend { task: String, json: bool },
}

pub async fn features(
    command: FeaturesCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "features")?;
    match command {
        FeaturesCommand::Matrix { json } => {
            let matrix = matrix_payload(&root)?;
            if json {
                print_json(&matrix)?;
            } else {
                print_matrix(&matrix);
            }
        }
        FeaturesCommand::Status { json } => {
            let status = status_payload(&root);
            if json {
                print_json(&status)?;
            } else {
                print_status(&status);
            }
        }
        FeaturesCommand::Recommend { task, json } => {
            let recommendation = recommend_payload(&task);
            if json {
                print_json(&recommendation)?;
            } else {
                print_recommendation(&recommendation);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureMatrix {
    pub source_path: String,
    pub total_capabilities: usize,
    pub action_counts: BTreeMap<String, usize>,
    pub rows: Vec<FeatureMatrixRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureMatrixRow {
    pub capability: String,
    pub peer_tool: String,
    pub peer_b: String,
    pub kimi: String,
    pub qwen: String,
    pub gemini: String,
    pub droid: String,
    pub opencode: String,
    pub deepseek_code_current: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureStatus {
    pub tui_available: bool,
    pub deepseek_config_status: String,
    pub api_key_configured: bool,
    pub agent_registry_status: String,
    pub built_in_agents_count: usize,
    pub custom_agents_count: usize,
    pub mcp_module_present: bool,
    pub policy_system_present: bool,
    pub session_storage_present: bool,
    pub plan_mode_present: bool,
    pub swarm_subagent_support_present: bool,
    /// Number of user-defined slash commands discovered in
    /// `.octocode/commands` (project) + `~/.octocode/commands` (user).
    pub custom_slash_commands_count: usize,
    /// Number of skills discovered under `.octocode/skills` (project) +
    /// `~/.octocode/skills` (user) — auto-injected at turn start when their
    /// frontmatter keywords match the user's input.
    pub skills_count: usize,
    /// Whether the cwd is a git repo *and* `git worktree` works here, so
    /// `octo agent run --isolation worktree` will actually create an
    /// isolated worktree.
    pub worktree_isolation_available: bool,
    /// Number of HookEvent variants that have at least one command
    /// configured in settings.json `hooks.*`.
    pub configured_hook_events: usize,
    pub config_paths: ConfigPaths,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigPaths {
    pub project_root: String,
    pub project_config: String,
    pub project_local_config: String,
    pub project_agents_dir: String,
    pub project_missions_dir: String,
    pub user_config: Option<String>,
    pub user_sessions_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureRecommendation {
    pub task: String,
    pub recommended_mode: RecommendedMode,
    pub suggested_agent: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecommendedMode {
    Direct,
    Plan,
    AgentRun,
    Swarm,
    MissionDryRun,
}

impl std::fmt::Display for RecommendedMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Direct => "direct",
            Self::Plan => "plan",
            Self::AgentRun => "agent-run",
            Self::Swarm => "swarm",
            Self::MissionDryRun => "mission-dry-run",
        };
        write!(f, "{value}")
    }
}

pub fn matrix_payload(project_root: &Path) -> Result<FeatureMatrix, anyhow::Error> {
    let source_path = project_root.join(MATRIX_RELATIVE_PATH);
    let (source_path, content) = match std::fs::read_to_string(&source_path) {
        Ok(content) => (source_path.display().to_string(), content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            format!("embedded:{MATRIX_RELATIVE_PATH}"),
            EMBEDDED_MATRIX.to_string(),
        ),
        Err(error) => {
            return Err(error).with_context(|| format!("read {}", source_path.display()));
        }
    };
    let rows = parse_matrix_rows(&content);
    let mut action_counts = BTreeMap::new();
    for row in &rows {
        *action_counts.entry(row.action.clone()).or_insert(0) += 1;
    }

    Ok(FeatureMatrix {
        source_path,
        total_capabilities: rows.len(),
        action_counts,
        rows,
    })
}

pub fn status_payload(project_root: &Path) -> FeatureStatus {
    let config_result = storage::Config::load(Some(project_root));
    let deepseek_config_status = match &config_result {
        Ok(_) => "loaded".to_string(),
        Err(error) => format!("using defaults ({error})"),
    };
    let config = config_result.unwrap_or_default();
    let registry = SubagentRegistry::load_from_project(project_root);
    let agent_names = registry.list();
    let built_in_agents_count = agent_names
        .iter()
        .filter(|name| BUILT_IN_AGENTS.contains(name))
        .count();
    let custom_agents_count = agent_names.len().saturating_sub(built_in_agents_count);

    let mut command_registry = crate::commands::CommandRegistry::new();
    let custom_slash_commands_count = command_registry.load_prompt_commands(project_root);
    let skills_count = count_skills(project_root);
    let worktree_isolation_available = git_worktree_works_here(project_root);
    let configured_hook_events = count_configured_hook_events(&config.hooks);

    FeatureStatus {
        tui_available: true,
        deepseek_config_status,
        api_key_configured: api_key_configured_from_local_sources(&config),
        agent_registry_status: "loaded".to_string(),
        built_in_agents_count,
        custom_agents_count,
        mcp_module_present: true,
        policy_system_present: true,
        session_storage_present: true,
        plan_mode_present: true,
        swarm_subagent_support_present: true,
        custom_slash_commands_count,
        skills_count,
        worktree_isolation_available,
        configured_hook_events,
        config_paths: config_paths(project_root),
    }
}

fn count_skills(project_root: &Path) -> usize {
    let mut total = 0;
    for dir in skill_dirs(project_root) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry
                    .file_type()
                    .map(|t| t.is_dir())
                    .unwrap_or(false)
                    && entry.path().join("SKILL.md").is_file()
                {
                    total += 1;
                }
            }
        }
    }
    total
}

fn skill_dirs(project_root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![project_root.join(".octocode").join("skills")];
    if let Some(home) = crate::storage::user_home_dir() {
        dirs.push(home.join(".octocode").join("skills"));
    }
    dirs
}

fn git_worktree_works_here(project_root: &Path) -> bool {
    // `git rev-parse --is-inside-work-tree` returns 0 only inside a real
    // git repo; if that holds and `git worktree list` succeeds, we can
    // create isolated worktrees.
    let inside = std::process::Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();
    let inside_ok = matches!(inside, Ok(out) if out.status.success() && out.stdout.starts_with(b"true"));
    if !inside_ok {
        return false;
    }
    matches!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(project_root)
            .args(["worktree", "list"])
            .output(),
        Ok(out) if out.status.success()
    )
}

fn count_configured_hook_events(hooks: &crate::storage::config::HooksConfig) -> usize {
    [
        crate::hooks::HookEvent::UserPromptSubmit,
        crate::hooks::HookEvent::PreToolUse,
        crate::hooks::HookEvent::PostToolUse,
        crate::hooks::HookEvent::SessionStart,
        crate::hooks::HookEvent::SessionEnd,
        crate::hooks::HookEvent::Stop,
        crate::hooks::HookEvent::SubagentStop,
        crate::hooks::HookEvent::PreCompact,
        crate::hooks::HookEvent::Notification,
    ]
    .into_iter()
    .filter(|event| !crate::hooks::configured_commands(hooks, *event).is_empty())
    .count()
}

pub fn recommend_payload(task: &str) -> FeatureRecommendation {
    let normalized = task.to_lowercase();
    let words = normalized.split_whitespace().count();
    let mentions_many_files = normalized.matches("src/").count() >= 2
        || normalized.contains("these three")
        || normalized.contains("multiple files")
        || normalized.contains("across the codebase")
        || contains_any(
            &normalized,
            &["多文件", "多个文件", "跨代码库", "整个代码库"],
        );
    let asks_review = contains_any(
        &normalized,
        &[
            "review",
            "audit",
            "security",
            "safety",
            "审核",
            "审查",
            "评审",
            "代码审查",
        ],
    );
    let asks_explain = contains_any(&normalized, &["explain", "understand", "summarize", "map"]);
    let asks_refactor = contains_any(
        &normalized,
        &["refactor", "architecture", "runtime", "long-running"],
    );
    let asks_fix = contains_any(&normalized, &["fix", "repair", "修复", "修一下"]);
    let asks_tests = contains_any(&normalized, &["test", "tests", "cargo test", "fix build"]);
    let small_edit = contains_any(
        &normalized,
        &["wording", "typo", "rename", "small", "readme wording"],
    ) && words <= 8;

    if (asks_refactor || asks_fix) && (asks_tests || mentions_many_files || words > 8) {
        return FeatureRecommendation {
            task: task.to_string(),
            recommended_mode: RecommendedMode::MissionDryRun,
            suggested_agent: Some("planner".to_string()),
            reason: "large or risky edit work should start with the executable dry-run path: octo mission new \"<task>\" --dry-run".to_string(),
        };
    }

    if asks_review && mentions_many_files {
        return FeatureRecommendation {
            task: task.to_string(),
            recommended_mode: RecommendedMode::AgentRun,
            suggested_agent: Some("code-reviewer".to_string()),
            reason: "multi-file review should use the executable agent path: octo agent run code-reviewer \"<task>\"".to_string(),
        };
    }

    if asks_review {
        let suggested_agent = if normalized.contains("security") || normalized.contains("safety") {
            "security-auditor".to_string()
        } else {
            "code-reviewer".to_string()
        };
        return FeatureRecommendation {
            task: task.to_string(),
            recommended_mode: RecommendedMode::AgentRun,
            suggested_agent: Some(suggested_agent.clone()),
            reason: format!(
                "review and audit tasks are executable read-only agent work: octo agent run {suggested_agent} \"<task>\""
            ),
        };
    }

    if asks_explain {
        return FeatureRecommendation {
            task: task.to_string(),
            recommended_mode: RecommendedMode::AgentRun,
            suggested_agent: Some("code-explorer".to_string()),
            reason: "explanation tasks use the executable explorer path: octo agent run code-explorer \"<task>\"".to_string(),
        };
    }

    if small_edit {
        return FeatureRecommendation {
            task: task.to_string(),
            recommended_mode: RecommendedMode::Direct,
            suggested_agent: None,
            reason: "small known-file edits are cheapest to handle directly".to_string(),
        };
    }

    FeatureRecommendation {
        task: task.to_string(),
        recommended_mode: RecommendedMode::Plan,
        suggested_agent: Some("planner".to_string()),
        reason: "the task is not obviously tiny; start with a plan before edits".to_string(),
    }
}

fn parse_matrix_rows(content: &str) -> Vec<FeatureMatrixRow> {
    let mut rows = Vec::new();
    let mut in_matrix = false;

    for line in content.lines() {
        if line.starts_with("| Capability |")
            && line.contains('|')
            && line.matches('|').count() >= 8
        {
            in_matrix = true;
            continue;
        }
        if !in_matrix {
            continue;
        }
        if line.starts_with("|---") {
            continue;
        }
        if !line.starts_with('|') || line.starts_with("## ") {
            break;
        }

        let columns: Vec<String> = line
            .trim_matches('|')
            .split('|')
            .map(|value| value.trim().to_string())
            .collect();
        if columns.len() != 10 {
            continue;
        }
        rows.push(FeatureMatrixRow {
            capability: columns[0].clone(),
            peer_tool: columns[1].clone(),
            peer_b: columns[2].clone(),
            kimi: columns[3].clone(),
            qwen: columns[4].clone(),
            gemini: columns[5].clone(),
            droid: columns[6].clone(),
            opencode: columns[7].clone(),
            deepseek_code_current: columns[8].clone(),
            action: columns[9].clone(),
        });
    }

    rows
}

fn print_matrix(matrix: &FeatureMatrix) {
    println!("Feature matrix: {}", matrix.source_path);
    println!("Capabilities: {}", matrix.total_capabilities);
    println!("Actions:");
    for (action, count) in &matrix.action_counts {
        println!("  {action}: {count}");
    }
    println!();
    println!("{:<34} {:<16} Octocode current", "Capability", "Action");
    println!("{}", "-".repeat(88));
    for row in &matrix.rows {
        println!(
            "{:<34} {:<16} {}",
            truncate(&row.capability, 34),
            row.action,
            truncate(&row.deepseek_code_current, 36)
        );
    }
}

fn print_status(status: &FeatureStatus) {
    println!("Feature status");
    println!("  TUI available: {}", status.tui_available);
    println!("  DeepSeek config: {}", status.deepseek_config_status);
    println!("  API key configured: {}", status.api_key_configured);
    println!("  Agent registry: {}", status.agent_registry_status);
    println!("  Built-in agents: {}", status.built_in_agents_count);
    println!("  Custom agents: {}", status.custom_agents_count);
    println!("  MCP module present: {}", status.mcp_module_present);
    println!("  Policy system present: {}", status.policy_system_present);
    println!(
        "  Session storage present: {}",
        status.session_storage_present
    );
    println!("  Plan mode present: {}", status.plan_mode_present);
    println!(
        "  Swarm/subagent support present: {}",
        status.swarm_subagent_support_present
    );
    println!(
        "  Custom slash commands: {}",
        status.custom_slash_commands_count
    );
    println!("  Skills loaded: {}", status.skills_count);
    println!(
        "  Worktree isolation available: {}",
        status.worktree_isolation_available
    );
    println!(
        "  Configured hook events: {}",
        status.configured_hook_events
    );
    println!("Config paths");
    println!("  Project root: {}", status.config_paths.project_root);
    println!("  Project config: {}", status.config_paths.project_config);
    println!(
        "  Project local config: {}",
        status.config_paths.project_local_config
    );
    println!(
        "  Project agents: {}",
        status.config_paths.project_agents_dir
    );
    println!(
        "  Project missions: {}",
        status.config_paths.project_missions_dir
    );
    if let Some(path) = &status.config_paths.user_config {
        println!("  User config: {path}");
    }
    if let Some(path) = &status.config_paths.user_sessions_dir {
        println!("  User sessions: {path}");
    }
}

fn print_recommendation(recommendation: &FeatureRecommendation) {
    println!("Task: {}", recommendation.task);
    println!("Recommended mode: {}", recommendation.recommended_mode);
    if let Some(agent) = &recommendation.suggested_agent {
        println!("Suggested agent: {agent}");
    }
    println!("Reason: {}", recommendation.reason);
}

fn print_json<T: Serialize>(value: &T) -> Result<(), anyhow::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn config_paths(project_root: &Path) -> ConfigPaths {
    let project_config_dir = project_root.join(".octocode");
    let user_config_dir = dirs::home_dir().map(|home| home.join(".octocode"));
    ConfigPaths {
        project_root: project_root.display().to_string(),
        project_config: project_config_dir.join("config.toml").display().to_string(),
        project_local_config: project_config_dir.join("local.toml").display().to_string(),
        project_agents_dir: project_config_dir.join("agents").display().to_string(),
        project_missions_dir: project_config_dir.join("missions").display().to_string(),
        user_config: user_config_dir
            .as_ref()
            .map(|dir| dir.join("config.toml").display().to_string()),
        user_sessions_dir: user_config_dir.map(|dir| dir.join("sessions").display().to_string()),
    }
}

fn api_key_configured_from_local_sources(config: &storage::Config) -> bool {
    storage::get_env_api_key_for_provider(config.provider.default).is_some()
        || config
            .api_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty())
        || config.profiles.values().any(|profile| {
            profile
                .api_key
                .as_ref()
                .is_some_and(|key| !key.trim().is_empty())
        })
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    let suffix = "...";
    let mut truncated = value
        .chars()
        .take(width.saturating_sub(suffix.len()))
        .collect::<String>();
    truncated.push_str(suffix);
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_payload_parses_project_document() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let matrix = matrix_payload(&root).expect("parse feature matrix");

        assert!(matrix.total_capabilities >= 30);
        assert!(matrix.action_counts.contains_key("implement-now"));
        assert!(matrix
            .rows
            .iter()
            .any(|row| row.capability == "custom agents"));
        serde_json::to_string(&matrix).expect("serialize matrix");
    }

    #[test]
    fn matrix_payload_falls_back_to_embedded_document_outside_source_tree() {
        let root = tempfile::tempdir().expect("tempdir");
        let matrix = matrix_payload(root.path()).expect("parse embedded feature matrix");

        assert_eq!(matrix.source_path, "embedded:docs/cli_parity_matrix.md");
        assert!(matrix.total_capabilities >= 30);
        assert!(matrix
            .rows
            .iter()
            .any(|row| row.capability == "custom agents"));
    }

    #[test]
    fn status_payload_does_not_require_api_key() {
        let root = tempfile::tempdir().expect("tempdir");
        let status = status_payload(root.path());

        assert!(status.tui_available);
        assert_eq!(status.agent_registry_status, "loaded");
        assert!(status.built_in_agents_count >= 6);
        serde_json::to_string(&status).expect("serialize status");
    }

    #[test]
    fn recommend_obvious_examples() {
        let explain = recommend_payload("explain src/agent");
        assert_eq!(explain.recommended_mode, RecommendedMode::AgentRun);
        assert_eq!(explain.suggested_agent.as_deref(), Some("code-explorer"));

        let review = recommend_payload("review src/agent/orchestrator.rs src/agent/swarm.rs");
        assert_eq!(review.recommended_mode, RecommendedMode::AgentRun);
        assert_eq!(review.suggested_agent.as_deref(), Some("code-reviewer"));
        assert!(review.reason.contains("octo agent run code-reviewer"));

        let wording = recommend_payload("modify README wording");
        assert_eq!(wording.recommended_mode, RecommendedMode::Direct);

        let refactor = recommend_payload("refactor agent runtime and add tests");
        assert_eq!(refactor.recommended_mode, RecommendedMode::MissionDryRun);
        assert!(refactor.reason.contains("octo mission new"));
    }

    #[test]
    fn recommend_multifile_review_and_fix_use_executable_entries() {
        let english_review = recommend_payload("review multiple files in src/agent and src/cli");
        assert_eq!(english_review.recommended_mode, RecommendedMode::AgentRun);
        assert_eq!(
            english_review.suggested_agent.as_deref(),
            Some("code-reviewer")
        );
        assert!(english_review
            .reason
            .contains("octo agent run code-reviewer"));

        let chinese_fix = recommend_payload("多文件修复 src/agent 和 src/cli 的状态问题");
        assert_eq!(chinese_fix.recommended_mode, RecommendedMode::MissionDryRun);
        assert_eq!(chinese_fix.suggested_agent.as_deref(), Some("planner"));
        assert!(chinese_fix.reason.contains("octo mission new"));

        let chinese_review = recommend_payload("审核多个文件的权限逻辑");
        assert_eq!(chinese_review.recommended_mode, RecommendedMode::AgentRun);
        assert_eq!(
            chinese_review.suggested_agent.as_deref(),
            Some("code-reviewer")
        );
        assert!(chinese_review
            .reason
            .contains("octo agent run code-reviewer"));
    }
}

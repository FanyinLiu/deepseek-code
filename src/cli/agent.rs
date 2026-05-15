use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::agent::orchestrator::AgentEvent;
use crate::agent::subagent::{
    PermissionMode, SubagentConfig, SubagentExecutor, SubagentRegistry, SubagentResult,
    SubagentTask,
};
use crate::provider::{build_provider, Provider};
use crate::storage;

const BUILT_IN_AGENTS: &[&str] = &[
    "general-purpose",
    "code-explorer",
    "code-reviewer",
    "planner",
    "test-runner",
    "architect",
    "security-auditor",
];

pub enum AgentCommand {
    List {
        json: bool,
    },
    Show {
        name: String,
        json: bool,
    },
    Run {
        name: String,
        task: String,
        focus: Option<PathBuf>,
        max_turns: Option<u32>,
        model: Option<String>,
        json: bool,
    },
    Create {
        name: String,
        template: AgentTemplate,
    },
    Validate {
        target: AgentValidateTarget,
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTemplate {
    Explorer,
    Reviewer,
    Auditor,
    Tester,
    Planner,
    Writer,
}

pub enum AgentValidateTarget {
    One(String),
    All,
}

pub async fn agent(
    command: AgentCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_root(project_root);
    match command {
        AgentCommand::List { json } => {
            let payload = list_payload(&root);
            if json {
                print_json(&payload)?;
            } else {
                print_agent_list(&payload);
            }
        }
        AgentCommand::Show { name, json } => {
            let payload = show_payload(&root, &name)?;
            if json {
                print_json(&payload)?;
            } else {
                print_agent_show(&payload);
            }
        }
        AgentCommand::Run {
            name,
            task,
            focus,
            max_turns,
            model,
            json,
        } => {
            let result = run_agent(&root, &name, &task, focus, max_turns, model, !json).await?;
            if json {
                print_json(&result)?;
            } else {
                print_run_result(&result);
            }
        }
        AgentCommand::Create { name, template } => {
            let path = create_agent(&root, &name, template)?;
            println!("Created agent template: {}", path.display());
        }
        AgentCommand::Validate { target, json } => {
            let payload = validate_payload(&root, target)?;
            if json {
                print_json(&payload)?;
            } else {
                print_validation_reports(&payload);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentListPayload {
    pub project_root: String,
    pub agents: Vec<AgentListItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentListItem {
    pub name: String,
    pub source: AgentSource,
    pub subagent_type: String,
    pub permission_mode: String,
    pub model: Option<String>,
    pub max_turns: u32,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSource {
    BuiltIn,
    Custom,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentShowPayload {
    #[serde(flatten)]
    pub item: AgentListItem,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRunPayload {
    pub agent: String,
    pub task: String,
    pub success: bool,
    pub summary: String,
    pub output: String,
    pub tool_calls_used: Vec<String>,
    pub files_read: Vec<String>,
    pub files_written: Vec<String>,
    pub duration_ms: u64,
    pub token_usage: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentValidationPayload {
    pub project_root: String,
    pub reports: Vec<AgentValidationReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentValidationReport {
    pub name: String,
    pub source: AgentSource,
    pub path: Option<String>,
    pub valid: bool,
    pub errors: Vec<AgentValidationIssue>,
    pub warnings: Vec<AgentValidationIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentValidationIssue {
    pub code: String,
    pub message: String,
}

pub fn list_payload(project_root: &Path) -> AgentListPayload {
    let registry = SubagentRegistry::load_from_project(project_root);
    let agents = registry
        .list()
        .into_iter()
        .filter_map(|name| {
            registry
                .get(name)
                .map(|config| item_from_config(name, config))
        })
        .collect();

    AgentListPayload {
        project_root: project_root.display().to_string(),
        agents,
    }
}

pub fn show_payload(project_root: &Path, name: &str) -> Result<AgentShowPayload, anyhow::Error> {
    let registry = SubagentRegistry::load_from_project(project_root);
    let Some(config) = registry.get(name) else {
        bail!("unknown agent '{name}'");
    };

    Ok(AgentShowPayload {
        item: item_from_config(name, config),
        system_prompt: config.effective_system_prompt(),
    })
}

pub fn validate_payload(
    project_root: &Path,
    target: AgentValidateTarget,
) -> Result<AgentValidationPayload, anyhow::Error> {
    let reports = match target {
        AgentValidateTarget::One(name) => vec![validate_one(project_root, &name)?],
        AgentValidateTarget::All => validate_all(project_root)?,
    };

    Ok(AgentValidationPayload {
        project_root: project_root.display().to_string(),
        reports,
    })
}

fn validate_all(project_root: &Path) -> Result<Vec<AgentValidationReport>, anyhow::Error> {
    let mut reports: BTreeMap<String, AgentValidationReport> = BTreeMap::new();
    for name in BUILT_IN_AGENTS {
        reports.insert((*name).to_string(), validate_built_in(name));
    }

    let agents_dir = SubagentRegistry::agents_dir(project_root);
    if agents_dir.is_dir() {
        for entry in std::fs::read_dir(&agents_dir)
            .with_context(|| format!("read {}", agents_dir.display()))?
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            reports.insert(name.to_string(), validate_agent_file(name, &path));
        }
    }

    Ok(reports.into_values().collect())
}

fn validate_one(project_root: &Path, name: &str) -> Result<AgentValidationReport, anyhow::Error> {
    let path = agent_file_for_name(project_root, name);
    if path.exists() {
        return Ok(validate_agent_file(name, &path));
    }

    if BUILT_IN_AGENTS.contains(&name) {
        return Ok(validate_built_in(name));
    }

    bail!("unknown agent '{name}'");
}

fn validate_built_in(name: &str) -> AgentValidationReport {
    AgentValidationReport {
        name: name.to_string(),
        source: AgentSource::BuiltIn,
        path: None,
        valid: true,
        errors: Vec::new(),
        warnings: Vec::new(),
    }
}

fn validate_agent_file(name: &str, path: &Path) -> AgentValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            errors.push(issue(
                "read_failed",
                format!("failed to read {}: {error}", path.display()),
            ));
            return report(name, path, errors, warnings);
        }
    };

    let prompt_body = markdown_body(&content);
    if prompt_body.trim().is_empty() {
        errors.push(issue("empty_prompt", "agent prompt body is empty"));
    }
    warnings.extend(dangerous_instruction_warnings(prompt_body));

    match SubagentRegistry::parse_markdown_agent(&content) {
        Ok(config) => {
            let known_tools = known_tool_names();
            for tool in &config.allowed_tools {
                if !known_tools.contains(tool) {
                    errors.push(issue(
                        "unknown_tool",
                        format!("allowed tool '{tool}' is not a known local tool"),
                    ));
                }
            }
        }
        Err(error) => {
            errors.push(issue("frontmatter_parse", error.to_string()));
        }
    }

    report(name, path, errors, warnings)
}

fn report(
    name: &str,
    path: &Path,
    errors: Vec<AgentValidationIssue>,
    warnings: Vec<AgentValidationIssue>,
) -> AgentValidationReport {
    AgentValidationReport {
        name: name.to_string(),
        source: AgentSource::Custom,
        path: Some(path.display().to_string()),
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

fn issue(code: impl Into<String>, message: impl Into<String>) -> AgentValidationIssue {
    AgentValidationIssue {
        code: code.into(),
        message: message.into(),
    }
}

fn create_agent(
    project_root: &Path,
    name: &str,
    template: AgentTemplate,
) -> Result<PathBuf, anyhow::Error> {
    validate_agent_name(name)?;
    let agents_dir = SubagentRegistry::agents_dir(project_root);
    std::fs::create_dir_all(&agents_dir)?;
    let path = agents_dir.join(format!("{name}.md"));
    if path.exists() {
        bail!("agent '{name}' already exists at {}", path.display());
    }

    let content = render_template(name, template);
    SubagentRegistry::parse_markdown_agent(&content)
        .with_context(|| format!("template for '{name}' did not parse"))?;
    std::fs::write(&path, content)?;
    Ok(path)
}

async fn run_agent(
    project_root: &Path,
    name: &str,
    task_text: &str,
    focus: Option<PathBuf>,
    max_turns: Option<u32>,
    model: Option<String>,
    show_events: bool,
) -> Result<AgentRunPayload, anyhow::Error> {
    let registry = SubagentRegistry::load_from_project(project_root);
    let Some(base_config) = registry.get(name) else {
        bail!("unknown agent '{name}'");
    };

    let mut config = base_config.clone();
    if let Some(max_turns) = max_turns {
        config.max_turns = max_turns;
    }
    if let Some(model) = model {
        config.model = Some(crate::provider::parse_model(&model)?);
    }

    let app_config = storage::Config::load(Some(project_root))?;
    let api_key = super::login::resolve_or_prompt_api_key(Some(project_root))?;
    let provider = build_provider(&app_config.provider, api_key);
    let client = Arc::new(provider.create_deepseek_client());
    let task = SubagentTask {
        description: task_text.chars().take(80).collect(),
        prompt: task_text.to_string(),
        context: None,
        focus_files: focus
            .map(|path| vec![path.display().to_string()])
            .unwrap_or_default(),
        expected_output: Some("Concise agent result".to_string()),
    };

    let executor = SubagentExecutor::new(client, project_root.to_path_buf(), config);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut handle = Box::pin(tokio::spawn(
        async move { executor.run(&task, &event_tx).await },
    ));

    let result = loop {
        tokio::select! {
            joined = &mut handle => {
                break joined.context("agent task join failed")?;
            }
            event = event_rx.recv() => {
                if let Some(event) = event {
                    handle_run_event(event, show_events);
                }
            }
        }
    };

    Ok(run_payload_from_result(name, task_text, result))
}

fn handle_run_event(event: AgentEvent, show_events: bool) {
    match event {
        AgentEvent::SubagentStarted {
            agent_type,
            description,
            ..
        } if show_events => {
            println!("agent started: {agent_type} - {description}");
        }
        AgentEvent::SubagentDelta { content, .. } if show_events && !content.trim().is_empty() => {
            print!("{content}");
        }
        AgentEvent::SubagentToolApprovalNeeded {
            tool_name, respond, ..
        } => {
            if show_events {
                println!(
                    "tool approval required for {tool_name}; denied in non-interactive agent run"
                );
            }
            let _ = respond.send(false);
        }
        AgentEvent::SubagentCompleted { result, .. } if show_events => {
            let status = if result.success { "ok" } else { "failed" };
            println!("agent completed: {status}");
        }
        AgentEvent::Error(error) if show_events => {
            eprintln!("agent error: {error}");
        }
        _ => {}
    }
}

fn run_payload_from_result(name: &str, task: &str, result: SubagentResult) -> AgentRunPayload {
    AgentRunPayload {
        agent: name.to_string(),
        task: task.to_string(),
        success: result.success,
        summary: result.summary,
        output: result.output,
        tool_calls_used: result.tool_calls_used,
        files_read: result.files_read,
        files_written: result.files_written,
        duration_ms: result.duration_ms,
        token_usage: result.token_usage,
        error: result.error,
    }
}

fn item_from_config(name: &str, config: &SubagentConfig) -> AgentListItem {
    AgentListItem {
        name: name.to_string(),
        source: if BUILT_IN_AGENTS.contains(&name) {
            AgentSource::BuiltIn
        } else {
            AgentSource::Custom
        },
        subagent_type: config.subagent_type.to_string(),
        permission_mode: permission_mode_label(&config.permission_mode).to_string(),
        model: config.model.as_ref().map(ToString::to_string),
        max_turns: config.max_turns,
        allowed_tools: config.allowed_tools.clone(),
    }
}

fn print_agent_list(payload: &AgentListPayload) {
    println!("Agents for {}", payload.project_root);
    println!(
        "{:<24} {:<10} {:<18} {:<12} Model",
        "Name", "Source", "Type", "Mode"
    );
    println!("{}", "-".repeat(86));
    for agent in &payload.agents {
        println!(
            "{:<24} {:<10} {:<18} {:<12} {}",
            agent.name,
            source_label(agent.source),
            agent.subagent_type,
            agent.permission_mode,
            agent.model.as_deref().unwrap_or("-")
        );
    }
}

fn print_agent_show(payload: &AgentShowPayload) {
    println!("Agent: {}", payload.item.name);
    println!("  Source: {}", source_label(payload.item.source));
    println!("  Type: {}", payload.item.subagent_type);
    println!("  Permission mode: {}", payload.item.permission_mode);
    println!(
        "  Model: {}",
        payload.item.model.as_deref().unwrap_or("default")
    );
    println!("  Max turns: {}", payload.item.max_turns);
    if payload.item.allowed_tools.is_empty() {
        println!("  Allowed tools: all standard tools");
    } else {
        println!("  Allowed tools: {}", payload.item.allowed_tools.join(", "));
    }
    println!();
    println!("{}", payload.system_prompt);
}

fn print_validation_reports(payload: &AgentValidationPayload) {
    println!("Agent validation for {}", payload.project_root);
    for report in &payload.reports {
        let status = if report.valid { "valid" } else { "invalid" };
        println!("- {} ({status})", report.name);
        for error in &report.errors {
            println!("  error[{}]: {}", error.code, error.message);
        }
        for warning in &report.warnings {
            println!("  warning[{}]: {}", warning.code, warning.message);
        }
    }
}

fn print_run_result(payload: &AgentRunPayload) {
    println!();
    println!("Agent: {}", payload.agent);
    println!("Success: {}", payload.success);
    println!("Duration: {}ms", payload.duration_ms);
    if payload.token_usage > 0 {
        println!("Tokens: {}", payload.token_usage);
    }
    if !payload.tool_calls_used.is_empty() {
        println!("Tools: {}", payload.tool_calls_used.join(", "));
    }
    if let Some(error) = &payload.error {
        println!("Error: {error}");
    }
    println!();
    println!("{}", payload.output);
}

fn print_json<T: Serialize>(value: &T) -> Result<(), anyhow::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn render_template(name: &str, template: AgentTemplate) -> String {
    let spec = template_spec(template);
    format!(
        r#"---
subagent_type = "{subagent_type}"
allowed_tools = [{allowed_tools}]
permission_mode = "{permission_mode}"
model = "{model}"
max_turns = {max_turns}
---

# {title}

{prompt}

Agent name: `{name}`.
"#,
        subagent_type = spec.subagent_type,
        allowed_tools = spec
            .allowed_tools
            .iter()
            .map(|tool| format!("\"{tool}\""))
            .collect::<Vec<_>>()
            .join(", "),
        permission_mode = spec.permission_mode,
        model = spec.model,
        max_turns = spec.max_turns,
        title = spec.title,
        prompt = spec.prompt,
        name = name,
    )
}

struct TemplateSpec {
    title: &'static str,
    subagent_type: &'static str,
    allowed_tools: &'static [&'static str],
    permission_mode: &'static str,
    model: &'static str,
    max_turns: u32,
    prompt: &'static str,
}

fn template_spec(template: AgentTemplate) -> TemplateSpec {
    match template {
        AgentTemplate::Explorer => TemplateSpec {
            title: "Code Explorer",
            subagent_type: "code-explorer",
            allowed_tools: &["read_file", "list_dir", "search_files", "search_code", "git_status"],
            permission_mode: "read_only",
            model: "deepseek-v4-flash",
            max_turns: 8,
            prompt: "You are a read-only codebase explorer. Inspect files and explain structure, dependencies, risks, and open questions. Do not edit files or run mutating commands.",
        },
        AgentTemplate::Reviewer => TemplateSpec {
            title: "Code Reviewer",
            subagent_type: "code-reviewer",
            allowed_tools: &["read_file", "list_dir", "search_files", "search_code", "git_status", "git_diff"],
            permission_mode: "read_only",
            model: "deepseek-v4-pro",
            max_turns: 10,
            prompt: "You are a read-only code reviewer. Prioritize bugs, regressions, missing tests, and maintainability risks. Report findings with file paths and concrete evidence.",
        },
        AgentTemplate::Auditor => TemplateSpec {
            title: "Security Auditor",
            subagent_type: "security-auditor",
            allowed_tools: &["read_file", "list_dir", "search_files", "search_code", "git_status", "git_diff"],
            permission_mode: "read_only",
            model: "deepseek-v4-pro",
            max_turns: 10,
            prompt: "You are a read-only security auditor. Look for hardcoded secrets, credential leakage, dangerous commands, protected path writes, prompt-injection persistence, and policy bypass attempts. If a critical issue is found, mark VETO and explain why execution should stop.",
        },
        AgentTemplate::Tester => TemplateSpec {
            title: "Test Runner",
            subagent_type: "test-runner",
            allowed_tools: &["read_file", "list_dir", "search_files", "search_code", "git_status", "git_diff", "run_command"],
            permission_mode: "accept_edits",
            model: "deepseek-v4-flash",
            max_turns: 8,
            prompt: "You are a test runner. Run focused validation commands, analyze failures, and report the smallest credible fix. Do not write files unless the active policy permits it.",
        },
        AgentTemplate::Planner => TemplateSpec {
            title: "Planner",
            subagent_type: "planner",
            allowed_tools: &["read_file", "list_dir", "search_files", "search_code", "git_status", "git_diff"],
            permission_mode: "read_only",
            model: "deepseek-v4-pro",
            max_turns: 8,
            prompt: "You are a read-only planning agent. Build a concrete implementation plan with phases, risks, validation commands, and rollback notes. Do not edit files.",
        },
        AgentTemplate::Writer => TemplateSpec {
            title: "Documentation Writer",
            subagent_type: "general-purpose",
            allowed_tools: &["read_file", "list_dir", "search_files", "search_code", "git_status", "git_diff", "edit_file", "write_file"],
            permission_mode: "accept_edits",
            model: "deepseek-v4-flash",
            max_turns: 8,
            prompt: "You are a documentation writer. Improve docs with concise, accurate language. You may propose patches, but must follow the active write policy and avoid unrelated refactors.",
        },
    }
}

fn markdown_body(content: &str) -> &str {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return "";
    }
    let Some(end) = trimmed[3..].find("---") else {
        return "";
    };
    trimmed[end + 6..].trim()
}

fn dangerous_instruction_warnings(body: &str) -> Vec<AgentValidationIssue> {
    let lower = body.to_lowercase();
    let mut warnings = Vec::new();
    for (code, phrase) in [
        ("dangerous_delete", "delete files"),
        ("dangerous_remove", "rm -rf"),
        ("policy_bypass", "bypass policy"),
        ("ignore_policy", "ignore policy"),
        ("unsafe_secret", "print secrets"),
        ("unsafe_write", "write outside"),
        ("unsafe_command", "run any command"),
    ] {
        if lower.contains(phrase) {
            warnings.push(issue(
                code,
                format!("prompt contains risky phrase '{phrase}'"),
            ));
        }
    }
    warnings
}

fn known_tool_names() -> BTreeSet<String> {
    crate::deepseek::tools::standard_tool_definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect()
}

fn permission_mode_label(mode: &PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Default => "default",
        PermissionMode::AcceptEdits => "accept_edits",
        PermissionMode::ReadOnly => "read_only",
        PermissionMode::Bypass => "bypass",
    }
}

fn source_label(source: AgentSource) -> &'static str {
    match source {
        AgentSource::BuiltIn => "built-in",
        AgentSource::Custom => "custom",
    }
}

fn validate_agent_name(name: &str) -> Result<(), anyhow::Error> {
    if name.is_empty() {
        bail!("agent name cannot be empty");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("agent name may only contain ASCII letters, numbers, '-' and '_'");
    }
    if BUILT_IN_AGENTS.contains(&name) {
        bail!("agent name '{name}' is reserved for a built-in agent");
    }
    Ok(())
}

fn agent_file_for_name(project_root: &Path, name: &str) -> PathBuf {
    SubagentRegistry::agents_dir(project_root).join(format!("{name}.md"))
}

fn resolve_root(project_root: Option<PathBuf>) -> PathBuf {
    project_root
        .or_else(storage::find_project_root)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::subagent::SubagentType;
    use crate::deepseek::DeepSeekModel;

    #[test]
    fn list_payload_contains_built_ins() {
        let root = tempfile::tempdir().expect("tempdir");
        let payload = list_payload(root.path());
        let names: BTreeSet<_> = payload
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();

        assert!(names.contains("code-reviewer"));
        assert!(names.contains("security-auditor"));
        serde_json::to_string(&payload).expect("serialize list");
    }

    #[test]
    fn create_template_writes_parseable_agent() {
        let root = tempfile::tempdir().expect("tempdir");
        let path =
            create_agent(root.path(), "my-auditor", AgentTemplate::Auditor).expect("create agent");

        assert!(path.exists());
        let content = std::fs::read_to_string(path).expect("read created agent");
        let config = SubagentRegistry::parse_markdown_agent(&content).expect("parse created agent");
        assert_eq!(config.subagent_type, SubagentType::SecurityAuditor);
    }

    #[test]
    fn validate_catches_malformed_agent() {
        let root = tempfile::tempdir().expect("tempdir");
        let agents_dir = SubagentRegistry::agents_dir(root.path());
        std::fs::create_dir_all(&agents_dir).expect("create agents dir");
        std::fs::write(agents_dir.join("bad.md"), "---\nnot valid =\n---\n\n")
            .expect("write bad agent");

        let payload =
            validate_payload(root.path(), AgentValidateTarget::All).expect("validate all");
        let bad = payload
            .reports
            .iter()
            .find(|report| report.name == "bad")
            .expect("bad report");
        assert!(!bad.valid);
        assert!(bad
            .errors
            .iter()
            .any(|error| error.code == "frontmatter_parse"));
    }

    #[test]
    fn validate_unknown_tool_is_error() {
        let root = tempfile::tempdir().expect("tempdir");
        let agents_dir = SubagentRegistry::agents_dir(root.path());
        std::fs::create_dir_all(&agents_dir).expect("create agents dir");
        std::fs::write(
            agents_dir.join("weird.md"),
            r#"---
subagent_type = "planner"
allowed_tools = ["read_file", "unknown_tool"]
permission_mode = "read_only"
model = "deepseek-v4-flash"
max_turns = 3
---

Plan safely.
"#,
        )
        .expect("write weird agent");

        let payload = validate_payload(root.path(), AgentValidateTarget::One("weird".to_string()))
            .expect("validate weird");
        let report = payload.reports.first().expect("report");
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|error| error.code == "unknown_tool"));
    }

    #[test]
    fn agent_model_aliases_use_shared_provider_parser() {
        assert_eq!(
            crate::provider::parse_model("v4-pro").expect("v4-pro alias"),
            DeepSeekModel::Pro
        );
        assert_eq!(
            crate::provider::parse_model("v4-flash").expect("v4-flash alias"),
            DeepSeekModel::Flash
        );
    }
}

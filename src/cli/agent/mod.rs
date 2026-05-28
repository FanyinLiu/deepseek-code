mod dry_run;
mod list;
mod payload;
mod run;
mod show;
mod validate;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Serialize;

use crate::agent::subagent::{PermissionMode, SubagentConfig, SubagentRegistry};
use crate::cli::resolve_project_root;

pub use list::list_payload;
pub use payload::*;
pub use show::show_payload;
pub use validate::validate_payload;

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
        dry_run: bool,
        isolation: crate::agent::subagent::SubagentIsolation,
        approval_mode: Option<crate::agent::subagent::PermissionMode>,
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
    let root = resolve_project_root(project_root, "agent")?;
    match command {
        AgentCommand::List { json } => {
            let payload = list::list_payload(&root);
            if json {
                print_json(&payload)?;
            } else {
                list::print_agent_list(&payload);
            }
        }
        AgentCommand::Show { name, json } => {
            let payload = show::show_payload(&root, &name)?;
            if json {
                print_json(&payload)?;
            } else {
                show::print_agent_show(&payload);
            }
        }
        AgentCommand::Run {
            name,
            task,
            focus,
            max_turns,
            model,
            dry_run,
            isolation,
            approval_mode,
            json,
        } => {
            let result = if dry_run {
                dry_run::run_agent_dry_run(
                    &root,
                    &name,
                    &task,
                    focus,
                    max_turns,
                    model,
                    isolation,
                    approval_mode,
                )?
            } else {
                run::run_agent(
                    &root,
                    &name,
                    &task,
                    focus,
                    max_turns,
                    model,
                    isolation,
                    approval_mode,
                    !json,
                )
                .await?
            };
            let failed = !result.success;
            if json {
                print_json(&result)?;
            } else {
                run::print_run_result(&result);
            }
            if failed {
                bail!("agent run failed");
            }
        }
        AgentCommand::Create { name, template } => {
            let path = create_agent(&root, &name, template)?;
            println!("Created agent template: {}", path.display());
        }
        AgentCommand::Validate { target, json } => {
            let payload = validate::validate_payload(&root, target)?;
            let has_invalid_reports = payload.reports.iter().any(|report| !report.valid);
            if json {
                print_json(&payload)?;
            } else {
                validate::print_validation_reports(&payload);
            }
            if has_invalid_reports {
                bail!("agent validation failed");
            }
        }
    }
    Ok(())
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
    crate::storage::atomic::write_text_atomic(&path, &content)?;
    Ok(path)
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

fn agent_files_for_name(project_root: &Path, name: &str) -> [PathBuf; 2] {
    [
        SubagentRegistry::agents_dir(project_root).join(format!("{name}.md")),
        SubagentRegistry::agents_dir(project_root).join(format!("{name}.toml")),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use crate::agent::subagent::SubagentResult;
    use crate::agent::subagent::SubagentType;
    use crate::deepseek::DeepSeekModel;

    #[test]
    fn list_payload_contains_built_ins() {
        let root = tempfile::tempdir().expect("tempdir");
        let payload = list::list_payload(root.path());
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
    fn run_payload_serializes_approval_denials() {
        let payload = AgentRunPayload {
            agent: "code-reviewer".to_string(),
            task: "review".to_string(),
            dry_run: false,
            success: false,
            summary: String::new(),
            output: String::new(),
            plan: None,
            tool_calls_used: Vec::new(),
            files_read: Vec::new(),
            files_written: Vec::new(),
            duration_ms: 0,
            token_usage: 0,
            failure_reason: None,
            worktree: None,
            error: Some("approval denied".to_string()),
            approval_denials: vec![AgentApprovalDenialPayload {
                agent_id: "agent-1".to_string(),
                tool: "run_command".to_string(),
                reason: "policy=ask-no-tty".to_string(),
                arguments: "{\"command\":\"echo hi\"}".to_string(),
                details: "Command: echo hi".to_string(),
            }],
        };

        let value = serde_json::to_value(&payload).expect("serialize payload");
        assert_eq!(value["approval_denials"][0]["reason"], "policy=ask-no-tty");
        assert_eq!(
            value["approval_denials"][0]["arguments"],
            "{\"command\":\"echo hi\"}"
        );
        assert_eq!(value["approval_denials"][0]["details"], "Command: echo hi");
    }

    #[test]
    fn run_payload_classifies_turn_budget_failure() {
        let now = chrono::Utc::now();
        let result = SubagentResult {
            success: false,
            summary: "子任务停止：已用完轮次预算。".to_string(),
            output: "子任务停止：已用完轮次预算。".to_string(),
            tool_calls_used: vec!["list_dir".to_string()],
            files_read: Vec::new(),
            files_written: Vec::new(),
            duration_ms: 10,
            token_usage: 20,
            error: Some("子任务停止：已用完轮次预算。".to_string()),
            started_at: now,
            completed_at: now,
            worktree: None,
        };

        let payload =
            run::run_payload_from_result("code-explorer", "inspect", result, Vec::new(), 1);

        let reason = payload.failure_reason.expect("failure reason");
        assert_eq!(reason.code, "turn_budget_exhausted");
        assert_eq!(reason.max_turns, Some(1));
        assert_eq!(reason.turns_used, Some(1));
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

        let payload = validate::validate_payload(root.path(), AgentValidateTarget::All)
            .expect("validate all");
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

        let payload =
            validate::validate_payload(root.path(), AgentValidateTarget::One("weird".to_string()))
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

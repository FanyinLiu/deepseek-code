use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context};

use crate::agent::subagent::{SubagentConfig, SubagentRegistry};

use super::payload::{
    AgentSource, AgentValidationIssue, AgentValidationPayload, AgentValidationReport,
};
use super::{agent_files_for_name, AgentValidateTarget, BUILT_IN_AGENTS};

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

    // Scan Claude Code's directory first, then octocode's, so an
    // octocode-native agent file overrides a same-named Claude Code one.
    for agents_dir in [
        SubagentRegistry::claude_agents_dir(project_root),
        SubagentRegistry::agents_dir(project_root),
    ] {
        if !agents_dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&agents_dir)
            .with_context(|| format!("read {}", agents_dir.display()))?
            .flatten()
        {
            let path = entry.path();
            let extension = path.extension().and_then(|value| value.to_str());
            if !matches!(extension, Some("md" | "toml")) {
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
    for path in agent_files_for_name(project_root, name) {
        if path.exists() {
            return Ok(validate_agent_file(name, &path));
        }
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
    let content = match crate::storage::read_text_file_capped(path) {
        Ok(content) => content,
        Err(error) => {
            errors.push(issue(
                "read_failed",
                format!("failed to read {}: {error}", path.display()),
            ));
            return report(name, path, errors, warnings);
        }
    };

    let is_toml = path.extension().and_then(|value| value.to_str()) == Some("toml");
    let prompt_body = if is_toml {
        toml_prompt_body(&content)
    } else {
        markdown_body(&content).to_string()
    };
    if prompt_body.trim().is_empty() {
        errors.push(issue("empty_prompt", "agent prompt body is empty"));
    }
    warnings.extend(dangerous_instruction_warnings(&prompt_body));

    let parsed = if is_toml {
        toml::from_str::<SubagentConfig>(&content).map_err(|error| {
            crate::agent::subagent::registry::AgentParseError {
                message: format!("invalid toml agent: {error}"),
            }
        })
    } else {
        SubagentRegistry::parse_markdown_agent(&content)
    };

    match parsed {
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
    let source = if path.components().any(|c| c.as_os_str() == ".claude") {
        AgentSource::Claude
    } else {
        AgentSource::Project
    };
    AgentValidationReport {
        name: name.to_string(),
        source,
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

fn toml_prompt_body(content: &str) -> String {
    toml::from_str::<SubagentConfig>(content)
        .ok()
        .and_then(|config| config.system_prompt)
        .unwrap_or_default()
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

pub(super) fn print_validation_reports(payload: &AgentValidationPayload) {
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

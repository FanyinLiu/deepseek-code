use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use super::types::{SubagentConfig, SubagentType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentParseError {
    pub message: String,
}

impl AgentParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AgentParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AgentParseError {}

/// Registry of subagent configurations.
///
/// Built-in agents are always available. Project-specific agents can be
/// loaded from `.octocode/agents/*.toml` or `.octocode/agents/*.md`.
#[derive(Debug, Clone)]
pub struct SubagentRegistry {
    agents: HashMap<String, SubagentConfig>,
}

impl Default for SubagentRegistry {
    fn default() -> Self {
        let mut reg = Self {
            agents: HashMap::new(),
        };
        reg.register_built_in();
        reg
    }
}

impl SubagentRegistry {
    /// Create a new registry with only built-in agents.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load agents from the project directory.
    #[must_use]
    pub fn load_from_project(project_root: &Path) -> Self {
        let mut reg = Self::new();
        reg.discover_project_agents(project_root);
        reg
    }

    /// Get a configuration by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SubagentConfig> {
        self.agents.get(name)
    }

    /// Register a custom agent.
    pub fn register(&mut self, name: &str, config: SubagentConfig) {
        self.agents.insert(name.to_string(), config);
    }

    /// List all available agent names.
    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .agents
            .keys()
            .map(std::string::String::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    /// Resolve a subagent type to its config.
    #[must_use]
    pub fn resolve(&self, subagent_type: &SubagentType) -> SubagentConfig {
        let name = subagent_type.as_str();
        self.get(name)
            .cloned()
            .unwrap_or_else(|| SubagentConfig::for_type(subagent_type.clone()))
    }

    // ------------------------------------------------------------------
    // Built-in agents
    // ------------------------------------------------------------------

    fn register_built_in(&mut self) {
        self.agents.insert(
            "general-purpose".to_string(),
            SubagentConfig::for_type(SubagentType::GeneralPurpose),
        );
        self.agents.insert(
            "code-explorer".to_string(),
            SubagentConfig::for_type(SubagentType::CodeExplorer),
        );
        self.agents.insert(
            "code-reviewer".to_string(),
            SubagentConfig::for_type(SubagentType::CodeReviewer),
        );
        self.agents.insert(
            "planner".to_string(),
            SubagentConfig::for_type(SubagentType::Planner),
        );
        self.agents.insert(
            "test-runner".to_string(),
            SubagentConfig::for_type(SubagentType::TestRunner),
        );
        self.agents.insert(
            "architect".to_string(),
            SubagentConfig::for_type(SubagentType::Architect),
        );
        self.agents.insert(
            "security-auditor".to_string(),
            SubagentConfig::for_type(SubagentType::SecurityAuditor),
        );
    }

    // ------------------------------------------------------------------
    // Project discovery
    // ------------------------------------------------------------------

    fn discover_project_agents(&mut self, project_root: &Path) {
        // Scan Claude Code's directory first, then octocode's, so an
        // octocode-native agent overrides a same-named Claude Code one.
        self.discover_agents_in_dir(&Self::claude_agents_dir(project_root));
        self.discover_agents_in_dir(&Self::agents_dir(project_root));
    }

    fn discover_agents_in_dir(&mut self, agents_dir: &Path) {
        if !agents_dir.is_dir() {
            return;
        }

        let entries = match std::fs::read_dir(agents_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                if let Ok(content) = crate::storage::read_text_file_capped(&path) {
                    if let Ok(config) = toml::from_str::<SubagentConfig>(&content) {
                        self.agents.insert(name.to_string(), config);
                    }
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                // Markdown files with TOML or Claude Code YAML frontmatter.
                if let Ok(content) = crate::storage::read_text_file_capped(&path) {
                    if let Ok(config) = Self::parse_markdown_agent(&content) {
                        self.agents.insert(name.to_string(), config);
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn agents_dir(project_root: &Path) -> std::path::PathBuf {
        project_root.join(".octocode").join("agents")
    }

    /// Claude Code's agent directory, scanned so `.claude/agents/*.md` files
    /// work without being copied into `.octocode/agents/`.
    #[must_use]
    pub fn claude_agents_dir(project_root: &Path) -> std::path::PathBuf {
        project_root.join(".claude").join("agents")
    }

    pub fn load_markdown_agent_file(path: &Path) -> Result<SubagentConfig, AgentParseError> {
        let content = crate::storage::read_text_file_capped(path).map_err(|error| {
            AgentParseError::new(format!("failed to read {}: {error}", path.display()))
        })?;
        Self::parse_markdown_agent(&content)
    }

    /// Parse a markdown agent file. Two frontmatter dialects are accepted:
    ///
    /// octocode TOML:
    /// ```markdown
    /// ---
    /// subagent_type = "general-purpose"
    /// allowed_tools = ["read_file", "search_code"]
    /// model = "deepseek-v4-flash"
    /// ---
    /// You are a specialized agent for ...
    /// ```
    ///
    /// Claude Code YAML (so `.claude/agents/*.md` files work as-is):
    /// ```markdown
    /// ---
    /// name: code-reviewer
    /// description: Use this agent to review diffs for bugs.
    /// tools: Read, Grep, Bash
    /// model: opus
    /// ---
    /// You are a code reviewer ...
    /// ```
    pub fn parse_markdown_agent(content: &str) -> Result<SubagentConfig, AgentParseError> {
        let content = content.trim_start();
        if !content.starts_with("---") {
            return Err(AgentParseError::new("missing frontmatter block"));
        }

        let end = content[3..]
            .find("---")
            .ok_or_else(|| AgentParseError::new("unterminated frontmatter block"))?
            + 3;
        let frontmatter = &content[3..end];
        let body = content[end + 3..].trim();

        // Prefer the native TOML dialect; fall back to Claude Code's YAML so
        // existing agent files from either ecosystem load unchanged.
        let mut config = match toml::from_str::<SubagentConfig>(frontmatter) {
            Ok(config) => config,
            Err(toml_error) => parse_claude_frontmatter(frontmatter).map_err(|yaml_error| {
                AgentParseError::new(format!(
                    "invalid frontmatter (not TOML: {toml_error}; not Claude YAML: {yaml_error})"
                ))
            })?,
        };
        if !body.is_empty() {
            config.system_prompt = Some(body.to_string());
        }
        Ok(config)
    }
}

/// Parse a Claude Code agent frontmatter block (flat YAML scalars) into a
/// [`SubagentConfig`]. Only the fields Claude Code defines are recognized;
/// `tools` and `model` are mapped to octocode equivalents on a best-effort
/// basis, and an omitted `tools` means "inherit everything" (empty list).
fn parse_claude_frontmatter(frontmatter: &str) -> Result<SubagentConfig, AgentParseError> {
    let description = yaml_scalar(frontmatter, "description");
    let name = yaml_scalar(frontmatter, "name");
    let tools = yaml_scalar(frontmatter, "tools");
    let model = yaml_scalar(frontmatter, "model");

    // A Claude agent file must at least name itself or describe itself; without
    // either it is indistinguishable from arbitrary text and we reject it.
    if name.is_none() && description.is_none() {
        return Err(AgentParseError::new(
            "missing `name` and `description` fields",
        ));
    }

    let subagent_type = name
        .as_deref()
        .and_then(|name| SubagentType::from_str(name).ok())
        .unwrap_or(SubagentType::GeneralPurpose);

    let allowed_tools = tools
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|tool| !tool.is_empty())
                .map(map_claude_tool)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(SubagentConfig {
        subagent_type,
        description,
        allowed_tools,
        model: model.as_deref().and_then(map_claude_model),
        ..SubagentConfig::default()
    })
}

/// Read a single-line scalar value for `key` from flat YAML frontmatter,
/// stripping surrounding quotes. Returns `None` if absent or empty.
fn yaml_scalar(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    frontmatter.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed
            .strip_prefix(&prefix)?
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Map a Claude Code tool name to its octocode equivalent. Unknown names pass
/// through unchanged so nothing is silently dropped.
fn map_claude_tool(name: &str) -> String {
    match name {
        "Read" => "read_file",
        "Write" => "write_file",
        "Edit" | "MultiEdit" => "edit_file",
        "Grep" => "grep",
        "Glob" => "glob",
        "Bash" => "run_command",
        "WebFetch" => "fetch_url",
        "WebSearch" => "web_search",
        "Task" => "run_subagent",
        "LS" => "list_dir",
        other => other,
    }
    .to_string()
}

/// Map a Claude Code model alias to an octocode model. `inherit` (and anything
/// unrecognized) returns `None`, leaving the agent on the default.
fn map_claude_model(name: &str) -> Option<crate::deepseek::DeepSeekModel> {
    use crate::deepseek::DeepSeekModel;
    match name.to_ascii_lowercase().as_str() {
        "opus" | "sonnet" => Some(DeepSeekModel::Pro),
        "haiku" => Some(DeepSeekModel::Flash),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_built_in_agents() {
        let reg = SubagentRegistry::new();
        assert!(reg.get("general-purpose").is_some());
        assert!(reg.get("code-explorer").is_some());
        assert!(reg.get("code-reviewer").is_some());
        assert!(reg.get("planner").is_some());
        assert!(reg.get("test-runner").is_some());
        assert!(reg.get("architect").is_some());
        assert!(reg.get("security-auditor").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_resolve_returns_config() {
        let reg = SubagentRegistry::new();
        let config = reg.resolve(&SubagentType::CodeExplorer);
        assert_eq!(config.subagent_type, SubagentType::CodeExplorer);
    }

    #[test]
    fn security_auditor_resolves_to_read_only_pro_agent() {
        let reg = SubagentRegistry::new();
        let config = reg.resolve(&SubagentType::SecurityAuditor);
        assert_eq!(config.subagent_type, SubagentType::SecurityAuditor);
        assert_eq!(
            config.permission_mode,
            super::super::types::PermissionMode::ReadOnly
        );
        assert_eq!(config.model, Some(crate::deepseek::DeepSeekModel::Pro));
        assert_eq!(
            config.allowed_tools,
            vec![
                "read_file".to_string(),
                "list_dir".to_string(),
                "search_files".to_string(),
                "search_code".to_string(),
                "git_status".to_string(),
                "git_diff".to_string()
            ]
        );
        let prompt = config.effective_system_prompt();
        assert!(prompt.contains("VETO"));
        assert!(prompt.contains(".gitignore"));
        assert!(prompt.contains("credential leakage"));
        assert!(prompt.contains("policy bypass"));
    }

    #[test]
    fn test_resolve_unknown_returns_default() {
        let reg = SubagentRegistry::new();
        let config = reg.resolve(&SubagentType::Custom {
            name: "unknown-agent".to_string(),
        });
        assert_eq!(
            config.subagent_type,
            SubagentType::Custom {
                name: "unknown-agent".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_markdown_agent() {
        let md = r#"---
subagent_type = "general-purpose"
allowed_tools = ["read_file", "search_code"]
model = "deepseek-v4-flash"
max_turns = 5
---

You are a security auditor. Focus on finding vulnerabilities.
"#;
        let config = SubagentRegistry::parse_markdown_agent(md).unwrap();
        assert_eq!(
            config.system_prompt,
            Some("You are a security auditor. Focus on finding vulnerabilities.".to_string())
        );
        assert_eq!(config.max_turns, 5);
        assert!(config.allowed_tools.contains(&"read_file".to_string()));
        assert_eq!(config.subagent_type, SubagentType::GeneralPurpose);
    }

    #[test]
    fn markdown_agent_carries_selection_description() {
        let md = r#"---
subagent_type = "general-purpose"
description = "Runs and verifies database migrations."
allowed_tools = ["read_file", "run_command"]
---

You are a migration runner.
"#;
        let config = SubagentRegistry::parse_markdown_agent(md).unwrap();
        assert_eq!(
            config.effective_description(),
            "Runs and verifies database migrations."
        );
    }

    #[test]
    fn parses_claude_code_yaml_agent() {
        let md = r#"---
name: code-reviewer
description: Use this agent to review diffs for bugs and regressions.
tools: Read, Grep, Bash
model: opus
---

You are a meticulous code reviewer. Report issues with file paths.
"#;
        let config = SubagentRegistry::parse_markdown_agent(md).expect("parse claude agent");
        assert_eq!(config.subagent_type, SubagentType::CodeReviewer);
        assert_eq!(
            config.effective_description(),
            "Use this agent to review diffs for bugs and regressions."
        );
        // Claude tool names are mapped to octocode equivalents.
        assert_eq!(
            config.allowed_tools,
            vec![
                "read_file".to_string(),
                "grep".to_string(),
                "run_command".to_string()
            ]
        );
        assert_eq!(config.model, Some(crate::deepseek::DeepSeekModel::Pro));
        assert!(config
            .effective_system_prompt()
            .contains("meticulous code reviewer"));
    }

    #[test]
    fn claude_agent_without_tools_inherits_all() {
        let md = r#"---
name: helper
description: A general helper.
---
Body.
"#;
        let config = SubagentRegistry::parse_markdown_agent(md).expect("parse");
        assert!(config.allowed_tools.is_empty());
        assert_eq!(config.model, None);
    }

    #[test]
    fn test_parse_markdown_agent_no_frontmatter() {
        let md = "Just plain markdown without frontmatter.";
        assert!(SubagentRegistry::parse_markdown_agent(md).is_err());
    }
}

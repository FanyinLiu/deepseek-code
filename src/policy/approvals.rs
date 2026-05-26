use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage::config::PolicyConfig;

/// Policy decision for a tool execution.
#[derive(Debug, Clone)]
pub struct PolicyDecision {
    pub source: ToolCallSource,
    pub action: PolicyAction,
    pub reason: String,
    pub display: ApprovalDisplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolCallSource {
    Main,
    Subagent,
    Mcp,
    Task,
    Mission,
}

impl ToolCallSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Subagent => "subagent",
            Self::Mcp => "mcp",
            Self::Task => "task",
            Self::Mission => "mission",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAction {
    Allow,
    Deny,
    AskOnce,
    AskSession,
}

#[derive(Debug, Clone)]
pub struct ApprovalDisplay {
    pub title: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub details: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskLevel {
    SafeRead,
    SensitiveRead,
    WriteProject,
    GitMutation,
    CommandExecution,
    NetworkAccess,
    Blocked,
}

impl PolicyDecision {
    pub(crate) fn allow(
        reason: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        risk: RiskLevel,
    ) -> Self {
        Self {
            source: ToolCallSource::Main,
            action: PolicyAction::Allow,
            reason: reason.into(),
            display: ApprovalDisplay {
                title: title.into(),
                description: description.into(),
                risk_level: risk,
                details: String::new(),
            },
        }
    }

    pub(crate) fn deny(
        reason: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        risk: RiskLevel,
        details: impl Into<String>,
    ) -> Self {
        Self {
            source: ToolCallSource::Main,
            action: PolicyAction::Deny,
            reason: reason.into(),
            display: ApprovalDisplay {
                title: title.into(),
                description: description.into(),
                risk_level: risk,
                details: details.into(),
            },
        }
    }

    fn ask_once(
        reason: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        risk: RiskLevel,
        details: impl Into<String>,
    ) -> Self {
        Self {
            source: ToolCallSource::Main,
            action: PolicyAction::AskOnce,
            reason: reason.into(),
            display: ApprovalDisplay {
                title: title.into(),
                description: description.into(),
                risk_level: risk,
                details: details.into(),
            },
        }
    }

    #[must_use]
    pub(crate) fn with_source(mut self, source: ToolCallSource) -> Self {
        self.source = source;
        self
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SafeRead => write!(f, "SafeRead"),
            Self::SensitiveRead => write!(f, "SensitiveRead"),
            Self::WriteProject => write!(f, "WriteProject"),
            Self::GitMutation => write!(f, "GitMutation"),
            Self::CommandExecution => write!(f, "CommandExecution"),
            Self::NetworkAccess => write!(f, "NetworkAccess"),
            Self::Blocked => write!(f, "Blocked"),
        }
    }
}

/// Check if a tool name is declared read-only in the central metadata table.
///
/// Source of truth lives in `crate::tools::metadata::ALL_TOOLS`. To change
/// which tools are read-only, edit that table, not here.
#[must_use]
pub fn is_safe_read_tool(name: &str) -> bool {
    crate::tools::metadata::is_read_only(name)
}

/// Evaluate an MCP tool call using server metadata instead of treating it as an unknown tool.
#[must_use]
pub fn evaluate_mcp_tool(
    metadata: &crate::mcp::registry::McpToolApprovalMetadata,
    arguments: &str,
    policy: &PolicyConfig,
) -> PolicyDecision {
    evaluate_mcp_tool_inner(metadata, arguments, policy).with_source(ToolCallSource::Mcp)
}

fn evaluate_mcp_tool_inner(
    metadata: &crate::mcp::registry::McpToolApprovalMetadata,
    arguments: &str,
    policy: &PolicyConfig,
) -> PolicyDecision {
    if let Err(error) = serde_json::from_str::<serde_json::Value>(arguments) {
        return PolicyDecision::deny(
            format!("invalid MCP tool arguments JSON: {error}"),
            format!("Blocked MCP: {}", metadata.identity.source),
            "MCP tool arguments must be valid JSON",
            RiskLevel::Blocked,
            format_mcp_details(metadata, "read=unknown write=unknown network=unknown"),
        );
    }

    let risk_flags = infer_mcp_risk_flags(metadata);
    let risk = mcp_risk_level(metadata, &risk_flags);
    let details = format_mcp_details(metadata, &risk_flags);

    if !metadata.allowed_by_filters {
        return PolicyDecision::deny(
            format!(
                "MCP tool blocked by include/exclude filters: {}",
                metadata.identity.source
            ),
            format!("Blocked MCP: {}", metadata.identity.source),
            "MCP server include/exclude policy blocks this tool",
            RiskLevel::Blocked,
            details,
        );
    }

    if !metadata.advertised {
        return PolicyDecision::deny(
            format!(
                "MCP tool was not advertised by server: {}",
                metadata.identity.source
            ),
            format!("Blocked MCP: {}", metadata.identity.source),
            "MCP server did not advertise this tool",
            RiskLevel::Blocked,
            details,
        );
    }

    if risk == RiskLevel::NetworkAccess && !policy.network_access {
        return PolicyDecision::deny(
            "network access disabled by policy",
            format!("Blocked MCP: {}", metadata.identity.source),
            "MCP tool appears to require network access",
            RiskLevel::Blocked,
            details,
        );
    }

    let title = format!("MCP: {}", metadata.identity.source);
    let description = metadata
        .description
        .clone()
        .unwrap_or_else(|| "MCP tool call".to_string());

    if metadata.trust
        && risk == RiskLevel::SafeRead
        && (policy.auto_approve_safe_read || policy.auto_mode)
    {
        return PolicyDecision::allow(
            format!("trusted MCP safe read: {}", metadata.identity.source),
            title,
            description,
            risk,
        );
    }

    if metadata.trust && risk == RiskLevel::WriteProject && !policy.require_approval_for_write {
        return PolicyDecision::allow(
            format!(
                "trusted MCP write allowed by policy: {}",
                metadata.identity.source
            ),
            title,
            description,
            risk,
        );
    }

    PolicyDecision::ask_once(
        format!("MCP tool requires approval: {}", metadata.identity.source),
        title,
        description,
        risk,
        details,
    )
}

/// Evaluate a tool call and return a policy decision.
#[must_use]
pub fn evaluate_tool(
    tool_name: &str,
    arguments: &str,
    project_root: &Path,
    policy: &PolicyConfig,
) -> PolicyDecision {
    evaluate_tool_inner(tool_name, arguments, project_root, policy)
        .with_source(infer_tool_call_source(tool_name))
}

fn evaluate_tool_inner(
    tool_name: &str,
    arguments: &str,
    project_root: &Path,
    policy: &PolicyConfig,
) -> PolicyDecision {
    if let Some(allowed) = &policy.allowed_tools {
        if !allowed.iter().any(|name| name == tool_name) {
            let allowlist = allowed.join(", ");
            return PolicyDecision::deny(
                format!("tool {tool_name} blocked by command allowed-tools"),
                format!("Blocked: {tool_name}"),
                "Tool not in the active command's allowed-tools list",
                RiskLevel::Blocked,
                format!("active allowed-tools: {allowlist}"),
            );
        }
    }
    let auto_approve_safe_read = policy.auto_approve_safe_read || policy.auto_mode;
    let args: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(error) => {
            return PolicyDecision::deny(
                format!("invalid tool arguments JSON: {error}"),
                format!("Blocked: {tool_name}"),
                "Tool arguments must be valid JSON",
                RiskLevel::Blocked,
                "Unable to parse tool arguments for policy evaluation.".to_string(),
            );
        }
    };
    if let Some(violation) =
        crate::defense::BehavioralPerimeter.check_tool_call(tool_name, arguments, project_root)
    {
        return PolicyDecision::deny(
            violation.reason.clone(),
            format!("Blocked: {tool_name}"),
            violation.reason,
            RiskLevel::Blocked,
            violation.detail,
        );
    }

    match tool_name {
        "read_file" | "list_dir" => {
            let path = match args.get("path").and_then(serde_json::Value::as_str) {
                Some(path) => path,
                None => {
                    return PolicyDecision::deny(
                        "missing required path",
                        format!("Blocked: {tool_name}"),
                        "The path argument is required",
                        RiskLevel::Blocked,
                        "No path argument was supplied.".to_string(),
                    );
                }
            };
            let risk = evaluate_path_risk(path, project_root, policy.block_protected_paths);

            match risk {
                RiskLevel::SafeRead if auto_approve_safe_read => PolicyDecision::allow(
                    "safe read within project",
                    format!("Read: {path}"),
                    "Reading file within project workspace",
                    RiskLevel::SafeRead,
                ),
                RiskLevel::SafeRead => PolicyDecision::ask_once(
                    "safe read within project",
                    format!("Read: {path}"),
                    "Reading file within project workspace",
                    RiskLevel::SafeRead,
                    String::new(),
                ),
                RiskLevel::Blocked => PolicyDecision::deny(
                    "protected path",
                    format!("Blocked: {path}"),
                    "Path is protected",
                    RiskLevel::Blocked,
                    String::new(),
                ),
                _ => PolicyDecision::ask_once(
                    format!("path outside project: {path}"),
                    format!("Read outside project: {path}"),
                    "Reading a file outside the workspace",
                    RiskLevel::SensitiveRead,
                    format!("Path: {path}\nScope: outside workspace"),
                ),
            }
        }

        "glob" | "grep" | "search_files" | "search_code" | "git_status" | "git_diff"
        | "git_log" | "task_get" | "task_list" | "ask_user" | "ask_user_question"
        | "bash_output" => PolicyDecision::allow(
            "safe read-only operation",
            tool_name.to_string(),
            "Read-only search/git operation",
            RiskLevel::SafeRead,
        ),

        "kill_shell" => {
            let shell_id = match args.get("shell_id").and_then(serde_json::Value::as_str) {
                Some(shell_id) => shell_id,
                None => {
                    return PolicyDecision::deny(
                        "missing required shell_id",
                        "Blocked: Kill Shell",
                        "The shell_id argument is required",
                        RiskLevel::Blocked,
                        "No shell_id argument was supplied.".to_string(),
                    );
                }
            };
            if policy.autonomy_level.auto_local_commands() || !policy.require_approval_for_command {
                return PolicyDecision::allow(
                    format!("kill_shell allowed by policy: {shell_id}"),
                    "Kill Shell",
                    format!("Terminate background shell {shell_id}"),
                    RiskLevel::CommandExecution,
                );
            }
            PolicyDecision::ask_once(
                format!("terminate background shell: {shell_id}"),
                "Kill Shell",
                format!("Terminate background shell {shell_id}"),
                RiskLevel::CommandExecution,
                format!("shell_id: {shell_id}"),
            )
        }

        "git_add" => {
            let paths: Vec<String> = args
                .get("paths")
                .and_then(serde_json::Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if paths.is_empty() {
                return PolicyDecision::deny(
                    "missing paths for git_add",
                    "Blocked: git_add",
                    "Staging requires at least one explicit path",
                    RiskLevel::Blocked,
                    "No paths were supplied.".to_string(),
                );
            }
            let details = format_path_details(&paths);
            if policy.auto_mode
                || policy.autonomy_level.auto_workspace_writes()
                || !policy.require_approval_for_write
            {
                return PolicyDecision::allow(
                    format!("git_add allowed by policy: {}", summarize_paths(&paths)),
                    "Git add",
                    "Staging files for commit",
                    RiskLevel::GitMutation,
                );
            }
            PolicyDecision::ask_once(
                format!("git_add requires approval: {}", summarize_paths(&paths)),
                "Git add",
                "Staging files for commit",
                RiskLevel::GitMutation,
                details,
            )
        }

        "fetch_url" | "web_search" => {
            if !policy.network_access {
                return PolicyDecision::deny(
                    "network access disabled by policy",
                    format!("Blocked: {tool_name}"),
                    "Network operations require network_access to be enabled in config",
                    RiskLevel::Blocked,
                    String::new(),
                );
            }
            PolicyDecision::allow(
                "network read operation",
                tool_name.to_string(),
                "Fetching content from the internet",
                RiskLevel::NetworkAccess,
            )
        }

        "github_pr" => {
            if !policy.network_access {
                return PolicyDecision::deny(
                    "network access disabled by policy",
                    "Blocked: GitHub PR",
                    "GitHub API requires network_access to be enabled in config",
                    RiskLevel::Blocked,
                    String::new(),
                );
            }
            PolicyDecision::allow(
                "network read/write to GitHub",
                tool_name.to_string(),
                "Interacting with GitHub API",
                RiskLevel::NetworkAccess,
            )
        }

        "semantic_search" => PolicyDecision::allow(
            "local search operation",
            tool_name.to_string(),
            "Searching codebase with TF-IDF",
            RiskLevel::SafeRead,
        ),

        "todo_write" => write_paths_decision(
            tool_name,
            &[".octocode/todos.json".to_string()],
            project_root,
            policy,
        ),

        "task_create" | "task_update" | "task_stop" => write_paths_decision(
            tool_name,
            &[".octocode/todos.json".to_string()],
            project_root,
            policy,
        ),

        "git_commit" => {
            let message = args
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(no message)");
            if !policy.require_approval_for_write {
                return PolicyDecision::allow(
                    "git commit allowed by policy",
                    "Git Commit",
                    message.to_string(),
                    RiskLevel::GitMutation,
                );
            }
            PolicyDecision::ask_once(
                "git commit",
                "Git Commit",
                message.to_string(),
                RiskLevel::GitMutation,
                format!("Commit message: {message}"),
            )
        }

        "edit_file" | "write_file" => {
            let path = match args.get("path").and_then(serde_json::Value::as_str) {
                Some(path) => path,
                None => {
                    return PolicyDecision::deny(
                        "missing required path",
                        format!("Blocked: {tool_name}"),
                        "The path argument is required",
                        RiskLevel::Blocked,
                        "No path argument was supplied.".to_string(),
                    );
                }
            };
            write_paths_decision(tool_name, &[path.to_string()], project_root, policy)
        }

        "apply_patch" => {
            let patch = args
                .get("patch")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let paths = crate::workspace::apply::parse_patch_paths(patch);
            write_paths_decision(tool_name, &paths, project_root, policy)
        }

        "run_command" => {
            let command = match args.get("command").and_then(serde_json::Value::as_str) {
                Some(command) => command,
                None => {
                    return PolicyDecision::deny(
                        "missing required command",
                        "Blocked: Run Command",
                        "The command argument is required",
                        RiskLevel::Blocked,
                        "No command argument was supplied.".to_string(),
                    );
                }
            };
            if let Some(reason) = crate::policy::commands::contains_dangerous_pattern(command) {
                return PolicyDecision::deny(
                    format!("dangerous command: {reason}"),
                    "Blocked: Run Command",
                    "Command matches a dangerous shell pattern",
                    RiskLevel::Blocked,
                    command.to_string(),
                );
            }
            if crate::policy::commands::requires_network(command) && !policy.network_access {
                return PolicyDecision::deny(
                    "network access disabled by policy",
                    "Blocked: Run Command",
                    "This command appears to require network access, but network_access is disabled",
                    RiskLevel::Blocked,
                    command.to_string(),
                );
            }
            if policy.autonomy_level.auto_local_commands() || !policy.require_approval_for_command {
                return PolicyDecision::allow(
                    format!("local command allowed by policy: {command}"),
                    "Run Command",
                    command.to_string(),
                    RiskLevel::CommandExecution,
                );
            }
            PolicyDecision::ask_once(
                format!("execute: {command}"),
                "Run Command",
                command.to_string(),
                RiskLevel::CommandExecution,
                format!(
                    "Command: {}\nCWD: {}",
                    command,
                    args["cwd"].as_str().unwrap_or("project root")
                ),
            )
        }

        "run_subagent" => PolicyDecision::ask_once(
            "run_subagent tool invocation requires operator approval",
            "Run Subagent",
            "Launching another agent can create additional tool calls",
            RiskLevel::WriteProject,
            "Use only trusted task inputs and allowlist agent policies.".to_string(),
        ),

        _ => PolicyDecision::deny(
            format!("unknown tool: {tool_name}"),
            format!("Unknown tool: {tool_name}"),
            "This tool is not recognized",
            RiskLevel::Blocked,
            String::new(),
        ),
    }
}

fn infer_tool_call_source(tool_name: &str) -> ToolCallSource {
    if tool_name == "todo_write" || tool_name.starts_with("task_") {
        ToolCallSource::Task
    } else if tool_name.starts_with("mission_") {
        ToolCallSource::Mission
    } else {
        ToolCallSource::Main
    }
}

fn write_paths_decision(
    tool_name: &str,
    paths: &[String],
    project_root: &Path,
    policy: &PolicyConfig,
) -> PolicyDecision {
    if paths.is_empty() {
        return PolicyDecision::deny(
            "unable to determine write targets",
            format!("Blocked: {tool_name}"),
            "Cannot approve a write without known affected paths",
            RiskLevel::Blocked,
            "No affected paths were found in the tool arguments.".to_string(),
        );
    }

    let blocked_paths: Vec<_> = paths
        .iter()
        .filter(|path| {
            evaluate_path_risk(path, project_root, policy.block_protected_paths)
                == RiskLevel::Blocked
        })
        .cloned()
        .collect();
    if !blocked_paths.is_empty() {
        return PolicyDecision::deny(
            "protected path",
            format!("Blocked write: {}", summarize_paths(&blocked_paths)),
            "Cannot write to protected paths",
            RiskLevel::Blocked,
            format_path_details(&blocked_paths),
        );
    }

    let title = if tool_name == "apply_patch" {
        format!("Apply patch: {}", summarize_paths(paths))
    } else {
        format!("Write: {}", summarize_paths(paths))
    };
    let description = if tool_name == "apply_patch" {
        "Patch will modify files"
    } else {
        "File will be modified"
    };
    let details = format_path_details(paths);

    let workspace_safe = paths.iter().all(|path| {
        evaluate_path_risk(path, project_root, policy.block_protected_paths) == RiskLevel::SafeRead
    });
    if (policy.autonomy_level.auto_workspace_writes() && workspace_safe)
        || !policy.require_approval_for_write
    {
        return PolicyDecision::allow(
            format!("file write allowed by policy: {}", summarize_paths(paths)),
            title,
            description,
            RiskLevel::WriteProject,
        );
    }

    PolicyDecision::ask_once(
        format!("file write: {}", summarize_paths(paths)),
        title,
        description,
        RiskLevel::WriteProject,
        details,
    )
}

fn summarize_paths(paths: &[String]) -> String {
    match paths {
        [] => "(none)".to_string(),
        [single] => single.clone(),
        _ => format!("{} files", paths.len()),
    }
}

fn format_path_details(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| format!("Path: {path}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn infer_mcp_risk_flags(metadata: &crate::mcp::registry::McpToolApprovalMetadata) -> String {
    let haystack = format!(
        "{} {} {}",
        metadata.identity.tool,
        metadata.description.as_deref().unwrap_or(""),
        metadata.input_schema
    )
    .to_ascii_lowercase();
    let write = contains_any(
        &haystack,
        &[
            "write", "edit", "create", "update", "delete", "remove", "apply", "patch", "save",
            "commit", "upload", "move", "rename",
        ],
    );
    let network = matches!(
        metadata.transport,
        crate::mcp::client::McpTransport::Http | crate::mcp::client::McpTransport::Sse
    ) || contains_any(
        &haystack,
        &[
            "web", "url", "http", "fetch", "request", "download", "remote",
        ],
    );
    let read = !write
        || contains_any(
            &haystack,
            &["read", "list", "get", "search", "find", "query", "fetch"],
        );

    format!(
        "read={} write={} network={}",
        yes_no(read),
        yes_no(write),
        yes_no(network)
    )
}

fn mcp_risk_level(
    metadata: &crate::mcp::registry::McpToolApprovalMetadata,
    risk_flags: &str,
) -> RiskLevel {
    if risk_flags.contains("network=yes") {
        RiskLevel::NetworkAccess
    } else if risk_flags.contains("write=yes") {
        RiskLevel::WriteProject
    } else if metadata.trust {
        RiskLevel::SafeRead
    } else {
        RiskLevel::SensitiveRead
    }
}

fn format_mcp_details(
    metadata: &crate::mcp::registry::McpToolApprovalMetadata,
    risk_flags: &str,
) -> String {
    format!(
        "Source: {}\nServer: {}\nTool: {}\nTrust: {}\nInclude: {}\nExclude: {}\nTransport: {}\nRisk: {}",
        metadata.identity.source,
        metadata.identity.server,
        metadata.identity.tool,
        yes_no(metadata.trust),
        format_tool_filter(&metadata.include_tools, "(all)"),
        format_tool_filter(&metadata.exclude_tools, "(none)"),
        mcp_transport_label(metadata.transport),
        risk_flags,
    )
}

fn format_tool_filter(values: &[String], empty_label: &str) -> String {
    if values.is_empty() {
        empty_label.to_string()
    } else {
        values.join(",")
    }
}

fn mcp_transport_label(transport: crate::mcp::client::McpTransport) -> &'static str {
    match transport {
        crate::mcp::client::McpTransport::Stdio => "stdio",
        crate::mcp::client::McpTransport::Http => "http",
        crate::mcp::client::McpTransport::Sse => "sse",
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// Strip the Windows UNC prefix (`\\?\`) from a path so that
/// `canonicalize` output can be compared against non-canonical roots.
fn strip_unc_prefix(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        std::path::PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

fn evaluate_path_risk(
    path_str: &str,
    project_root: &Path,
    block_protected_paths: bool,
) -> RiskLevel {
    if path_str.trim().is_empty() {
        return RiskLevel::Blocked;
    }

    // Check protected paths (load user config instead of hard-coded defaults)
    let protected_patterns = crate::storage::Config::load(Some(project_root))
        .map(|c| c.paths.protected)
        .unwrap_or_default();
    let path = std::path::Path::new(path_str);
    let absolute_for_block = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };

    if block_protected_paths
        && (crate::workspace::paths::is_protected_path(path, &protected_patterns)
            || crate::workspace::paths::is_protected_path(&absolute_for_block, &protected_patterns)
            || crate::policy::paths::is_blocked_path(&absolute_for_block, project_root))
    {
        return RiskLevel::Blocked;
    }

    // Check if within project
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };

    let project_root_norm = std::fs::canonicalize(project_root)
        .map(|path| strip_unc_prefix(&path))
        .unwrap_or_else(|_| strip_unc_prefix(project_root));
    let project_root_raw_norm = strip_unc_prefix(project_root);

    if let Ok(canonical) = std::fs::canonicalize(&absolute) {
        let canonical_norm = strip_unc_prefix(&canonical);
        if canonical_norm.starts_with(&project_root_norm)
            || canonical_norm.starts_with(&project_root_raw_norm)
        {
            RiskLevel::SafeRead
        } else {
            RiskLevel::SensitiveRead
        }
    } else {
        // Path doesn't exist yet — for writes, this is normal
        let absolute_norm = strip_unc_prefix(&absolute);
        if absolute_norm.starts_with(&project_root_norm)
            || absolute_norm.starts_with(&project_root_raw_norm)
        {
            RiskLevel::SafeRead
        } else {
            RiskLevel::SensitiveRead
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: serde_json::Value) -> String {
        value.to_string()
    }

    #[test]
    fn allowed_tools_blocks_tool_outside_allowlist() {
        let policy = PolicyConfig {
            allowed_tools: Some(vec!["read_file".to_string(), "glob".to_string()]),
            ..PolicyConfig::default()
        };
        let decision = evaluate_tool(
            "write_file",
            &args(serde_json::json!({ "path": "a.txt", "content": "x" })),
            Path::new("."),
            &policy,
        );
        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
    }

    #[test]
    fn allowed_tools_permits_tool_in_allowlist() {
        let policy = PolicyConfig {
            auto_approve_safe_read: true,
            allowed_tools: Some(vec!["glob".to_string()]),
            ..PolicyConfig::default()
        };
        let decision = evaluate_tool(
            "glob",
            &args(serde_json::json!({ "pattern": "src/**" })),
            Path::new("."),
            &policy,
        );
        assert_eq!(decision.action, PolicyAction::Allow);
    }

    #[test]
    fn allowed_tools_none_means_unrestricted() {
        let policy = PolicyConfig {
            allowed_tools: None,
            ..PolicyConfig::default()
        };
        let decision = evaluate_tool(
            "glob",
            &args(serde_json::json!({ "pattern": "src/**" })),
            Path::new("."),
            &policy,
        );
        // Without an allowlist, glob still passes through (it is a SafeRead).
        assert_ne!(decision.action, PolicyAction::Deny);
    }

    fn mcp_metadata(
        server: &str,
        tool: &str,
        trust: bool,
        transport: crate::mcp::client::McpTransport,
    ) -> crate::mcp::registry::McpToolApprovalMetadata {
        crate::mcp::registry::McpToolApprovalMetadata {
            identity: crate::mcp::registry::McpToolIdentity {
                server: server.to_string(),
                tool: tool.to_string(),
                source: format!("mcp:{server}.{tool}"),
            },
            trust,
            include_tools: vec![tool.to_string()],
            exclude_tools: Vec::new(),
            transport,
            advertised: true,
            allowed_by_filters: true,
            description: Some(tool.replace('_', " ")),
            input_schema: serde_json::json!({}),
        }
    }

    #[test]
    fn outside_workspace_read_requires_approval() {
        let root = tempfile::tempdir().expect("workspace");
        let outside = tempfile::NamedTempFile::new().expect("outside file");
        let policy = PolicyConfig::default();

        let decision = evaluate_tool(
            "read_file",
            &args(serde_json::json!({ "path": outside.path().to_string_lossy() })),
            root.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::AskOnce);
        assert_eq!(decision.display.risk_level, RiskLevel::SensitiveRead);
        assert!(decision.display.title.contains("Read outside project"));
        assert!(decision.display.details.contains("Path:"));
        assert!(decision
            .display
            .details
            .contains("Scope: outside workspace"));
    }

    #[test]
    fn workspace_read_is_auto_allowed_by_default() {
        let root = tempfile::tempdir().expect("workspace");
        let file = root.path().join("note.txt");
        std::fs::write(&file, "hello").expect("write file");
        let policy = PolicyConfig::default();

        let decision = evaluate_tool(
            "read_file",
            &args(serde_json::json!({ "path": "note.txt" })),
            root.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Allow);
        assert_eq!(decision.display.risk_level, RiskLevel::SafeRead);
    }

    #[test]
    fn malformed_tool_args_are_blocked() {
        let root = tempfile::tempdir().expect("workspace");
        let policy = PolicyConfig::default();
        let decision = evaluate_tool("read_file", "{", root.path(), &policy);
        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
        assert!(decision.display.title.contains("Blocked: read_file"));
    }

    #[test]
    fn missing_required_read_path_is_blocked() {
        let root = tempfile::tempdir().expect("workspace");
        let policy = PolicyConfig::default();
        let decision = evaluate_tool("read_file", "{}", root.path(), &policy);
        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
        assert_eq!(decision.display.title, "Blocked: read_file");
    }

    #[test]
    fn mcp_approval_details_include_source_filters_and_risk_flags() {
        let policy = PolicyConfig::default();
        let metadata = mcp_metadata(
            "fs",
            "read_file",
            false,
            crate::mcp::client::McpTransport::Stdio,
        );

        let decision = evaluate_mcp_tool(&metadata, "{}", &policy);

        assert_eq!(decision.action, PolicyAction::AskOnce);
        assert_eq!(decision.display.risk_level, RiskLevel::SensitiveRead);
        assert!(decision.display.title.contains("mcp:fs.read_file"));
        assert!(decision
            .display
            .details
            .contains("Source: mcp:fs.read_file"));
        assert!(decision.display.details.contains("Trust: no"));
        assert!(decision
            .display
            .details
            .contains("Risk: read=yes write=no network=no"));
    }

    #[test]
    fn mcp_remote_tool_is_blocked_when_network_is_disabled() {
        let policy = PolicyConfig {
            network_access: false,
            ..PolicyConfig::default()
        };
        let metadata = mcp_metadata(
            "web",
            "fetch_url",
            true,
            crate::mcp::client::McpTransport::Http,
        );

        let decision = evaluate_mcp_tool(&metadata, r#"{"url":"https://example.com"}"#, &policy);

        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
        assert!(decision.display.details.contains("Transport: http"));
        assert!(decision.display.details.contains("network=yes"));
    }

    #[test]
    fn mcp_filtered_tool_is_blocked_before_approval() {
        let policy = PolicyConfig::default();
        let mut metadata = mcp_metadata(
            "fs",
            "write_file",
            true,
            crate::mcp::client::McpTransport::Stdio,
        );
        metadata.allowed_by_filters = false;
        metadata.advertised = false;

        let decision = evaluate_mcp_tool(&metadata, "{}", &policy);

        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
        assert!(decision.display.description.contains("include/exclude"));
    }

    #[test]
    fn local_commands_are_not_blocked_when_network_is_disabled() {
        let policy = PolicyConfig {
            network_access: false,
            require_approval_for_command: true,
            ..PolicyConfig::default()
        };

        let decision = evaluate_tool(
            "run_command",
            &args(serde_json::json!({ "command": "cargo test" })),
            Path::new("."),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::AskOnce);
        assert_eq!(decision.display.risk_level, RiskLevel::CommandExecution);
    }

    #[test]
    fn background_shell_poll_is_safe_read_tool() {
        let policy = PolicyConfig::default();

        let decision = evaluate_tool(
            "bash_output",
            &args(serde_json::json!({ "shell_id": "sh-test" })),
            Path::new("."),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Allow);
        assert_eq!(decision.display.risk_level, RiskLevel::SafeRead);
    }

    #[test]
    fn kill_shell_uses_command_approval_policy() {
        let policy = PolicyConfig {
            require_approval_for_command: true,
            ..PolicyConfig::default()
        };

        let decision = evaluate_tool(
            "kill_shell",
            &args(serde_json::json!({ "shell_id": "sh-test" })),
            Path::new("."),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::AskOnce);
        assert_eq!(decision.display.risk_level, RiskLevel::CommandExecution);
    }

    #[test]
    fn network_commands_are_blocked_when_network_is_disabled() {
        let policy = PolicyConfig {
            network_access: false,
            ..PolicyConfig::default()
        };

        let decision = evaluate_tool(
            "run_command",
            &args(serde_json::json!({ "command": "git pull" })),
            Path::new("."),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
    }

    #[test]
    fn write_approval_can_be_disabled_by_policy() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig {
            require_approval_for_write: false,
            ..PolicyConfig::default()
        };

        let decision = evaluate_tool(
            "write_file",
            &args(serde_json::json!({ "path": "src/main.rs" })),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Allow);
        assert_eq!(decision.display.risk_level, RiskLevel::WriteProject);
    }

    #[test]
    fn task_write_tools_target_project_todos_file() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig::default();

        let decision = evaluate_tool(
            "task_create",
            &args(serde_json::json!({ "content": "Ship task tools" })),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::AskOnce);
        assert_eq!(decision.display.risk_level, RiskLevel::WriteProject);
        assert!(decision.display.details.contains(".octocode/todos.json"));
        assert!(!decision.display.details.contains(".octocode/tasks"));
    }

    #[test]
    fn command_approval_can_be_disabled_by_policy() {
        let policy = PolicyConfig {
            require_approval_for_command: false,
            ..PolicyConfig::default()
        };

        let decision = evaluate_tool(
            "run_command",
            &args(serde_json::json!({ "command": "cargo check" })),
            Path::new("."),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Allow);
    }

    #[test]
    fn unknown_tools_are_denied_without_approval_prompt() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig::default();

        let decision = evaluate_tool(
            "does_not_exist",
            &args(serde_json::json!({})),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
    }

    #[test]
    fn git_add_requires_approval_by_default() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig::default();

        let decision = evaluate_tool(
            "git_add",
            &args(serde_json::json!({ "paths": ["src/lib.rs"] })),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::AskOnce);
        assert_eq!(decision.display.risk_level, RiskLevel::GitMutation);
        assert!(decision.display.details.contains("src/lib.rs"));
    }

    #[test]
    fn autonomy_low_allows_workspace_writes_but_not_commands() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig {
            autonomy_level: crate::storage::config::AutonomyLevel::Low,
            require_approval_for_write: true,
            require_approval_for_command: true,
            ..PolicyConfig::default()
        };

        let write = evaluate_tool(
            "write_file",
            &args(serde_json::json!({ "path": "src/main.rs" })),
            temp.path(),
            &policy,
        );
        let command = evaluate_tool(
            "run_command",
            &args(serde_json::json!({ "command": "cargo check" })),
            temp.path(),
            &policy,
        );

        assert_eq!(write.action, PolicyAction::Allow);
        assert_eq!(command.action, PolicyAction::AskOnce);
    }

    #[test]
    fn autonomy_medium_allows_local_commands() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig {
            autonomy_level: crate::storage::config::AutonomyLevel::Medium,
            require_approval_for_command: true,
            ..PolicyConfig::default()
        };

        let decision = evaluate_tool(
            "run_command",
            &args(serde_json::json!({ "command": "cargo check" })),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Allow);
    }

    #[test]
    fn apply_patch_approval_lists_affected_paths() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig::default();
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
";

        let decision = evaluate_tool(
            "apply_patch",
            &args(serde_json::json!({ "patch": patch })),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::AskOnce);
        assert_eq!(decision.display.risk_level, RiskLevel::WriteProject);
        assert!(decision.display.title.contains("Apply patch"));
        assert!(decision.display.details.contains("Path: src/lib.rs"));
    }

    #[test]
    fn apply_patch_to_protected_path_is_blocked_before_approval() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig::default();
        let patch = "\
*** Begin Patch
*** Update File: .env
@@
+SECRET=1
*** End Patch
";

        let decision = evaluate_tool(
            "apply_patch",
            &args(serde_json::json!({ "patch": patch })),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
        assert!(decision.display.details.contains("Path: .env"));
    }

    #[test]
    fn apply_patch_without_parseable_paths_is_blocked() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig::default();

        let decision = evaluate_tool(
            "apply_patch",
            &args(serde_json::json!({ "patch": "not a patch" })),
            temp.path(),
            &policy,
        );

        assert_eq!(decision.action, PolicyAction::Deny);
        assert_eq!(decision.display.risk_level, RiskLevel::Blocked);
        assert!(decision
            .display
            .details
            .contains("No affected paths were found"));
    }

    #[test]
    fn policy_decision_sources_are_inferred_for_builtin_tool_classes() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let policy = PolicyConfig::default();

        let main = evaluate_tool(
            "read_file",
            &args(serde_json::json!({ "path": "Cargo.toml" })),
            temp.path(),
            &policy,
        );
        let task = evaluate_tool(
            "task_update",
            &args(serde_json::json!({ "id": "1", "status": "done" })),
            temp.path(),
            &policy,
        );
        let mission = evaluate_tool(
            "mission_replay",
            &args(serde_json::json!({ "id": "latest" })),
            temp.path(),
            &policy,
        );

        assert_eq!(main.source, ToolCallSource::Main);
        assert_eq!(task.source, ToolCallSource::Task);
        assert_eq!(mission.source, ToolCallSource::Mission);
        assert_eq!(ToolCallSource::Subagent.as_str(), "subagent");
    }
}

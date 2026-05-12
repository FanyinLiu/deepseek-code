use std::path::Path;

use crate::storage::config::PolicyConfig;

/// Policy decision for a tool execution.
#[derive(Debug, Clone)]
pub struct PolicyDecision {
    pub action: PolicyAction,
    pub reason: String,
    pub display: ApprovalDisplay,
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

/// Safe-read tools that do not mutate the workspace.
pub const SAFE_READ_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search_files",
    "search_code",
    "git_status",
    "git_diff",
    "git_log",
];

/// Check if a tool name is in the safe-read list.
#[must_use]
pub fn is_safe_read_tool(name: &str) -> bool {
    SAFE_READ_TOOLS.contains(&name)
}

/// Evaluate a tool call and return a policy decision.
#[must_use]
pub fn evaluate_tool(
    tool_name: &str,
    arguments: &str,
    project_root: &Path,
    policy: &PolicyConfig,
) -> PolicyDecision {
    let auto_approve_safe_read = policy.auto_approve_safe_read || policy.auto_mode;
    // Parse arguments for path extraction
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
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
            let path = args["path"].as_str().unwrap_or("");
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

        "search_files" | "search_code" | "git_status" | "git_diff" | "git_log" => {
            PolicyDecision::allow(
                "safe read-only operation",
                tool_name.to_string(),
                "Read-only search/git operation",
                RiskLevel::SafeRead,
            )
        }

        "git_add" => PolicyDecision::allow(
            "staging operation",
            tool_name.to_string(),
            "Staging files for commit",
            RiskLevel::GitMutation,
        ),

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

        "git_commit" => {
            let message = args["message"].as_str().unwrap_or("(no message)");
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
            let path = args["path"].as_str().unwrap_or("");
            write_paths_decision(tool_name, &[path.to_string()], project_root, policy)
        }

        "apply_patch" => {
            let patch = args["patch"].as_str().unwrap_or("");
            let paths = crate::workspace::apply::parse_patch_paths(patch);
            write_paths_decision(tool_name, &paths, project_root, policy)
        }

        "run_command" => {
            let command = args["command"].as_str().unwrap_or("");
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

        _ => PolicyDecision::ask_once(
            format!("unknown tool: {tool_name}"),
            format!("Unknown tool: {tool_name}"),
            "This tool is not recognized",
            RiskLevel::Blocked,
            String::new(),
        ),
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
}

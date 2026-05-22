//! Centralized tool metadata.
//!
//! Single source of truth for per-tool flags (`read_only`, `destructive`,
//! `concurrency_safe`). Replaces three scattered hardcoded lists that
//! previously lived in `policy/approvals.rs`, `agent/subagent/executor.rs`,
//! and inline matches in `tools/dispatch.rs`.
//!
//! Adding a new tool? Add a row here, plus its dispatch arm. The policy
//! layer and subagent read-only guard pick the change up automatically.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolMetadata {
    pub name: &'static str,
    /// True if the tool cannot mutate the workspace, network, or processes.
    pub read_only: bool,
    /// True if the tool can perform irreversible destructive operations
    /// (delete files, force-push, drop tables, etc.). Subset of NOT read_only.
    pub destructive: bool,
    /// True if calling this tool concurrently with itself (different inputs)
    /// is safe and produces no race conditions.
    pub concurrency_safe: bool,
}

/// Master tool registry. Adding a new tool? Append a row.
pub const ALL_TOOLS: &[ToolMetadata] = &[
    // ── read-only ──
    ToolMetadata {
        name: "read_file",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "list_dir",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "glob",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "grep",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "search_files",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "search_code",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "semantic_search",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "git_status",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "git_diff",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "git_log",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "task_get",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "task_list",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "ask_user",
        read_only: true,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "ask_user_question",
        read_only: true,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "think",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    // ── mutating but non-destructive ──
    ToolMetadata {
        name: "write_file",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "edit_file",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "notebook_edit",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "apply_patch",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "todo_write",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "task_create",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "task_update",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "task_stop",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "git_add",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "git_commit",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    // ── potentially destructive (depends on args) ──
    ToolMetadata {
        name: "run_command",
        read_only: false,
        destructive: true,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "fetch_url",
        read_only: false,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "web_search",
        read_only: false,
        destructive: false,
        concurrency_safe: true,
    },
    ToolMetadata {
        name: "github_pr",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    ToolMetadata {
        name: "run_subagent",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
    // ── background shells ──
    // bash_output is a pure read on the in-process registry.
    ToolMetadata {
        name: "bash_output",
        read_only: true,
        destructive: false,
        concurrency_safe: true,
    },
    // kill_shell terminates a running child. It's a mutation on the
    // process tree but cannot affect the workspace, so it's not flagged
    // destructive (the cost of a wrong kill is bounded — the user can
    // just rerun the command).
    ToolMetadata {
        name: "kill_shell",
        read_only: false,
        destructive: false,
        concurrency_safe: false,
    },
];

#[must_use]
pub fn metadata(name: &str) -> Option<&'static ToolMetadata> {
    ALL_TOOLS.iter().find(|m| m.name == name)
}

/// True if the tool is registered and declared read-only.
#[must_use]
pub fn is_read_only(name: &str) -> bool {
    metadata(name).is_some_and(|m| m.read_only)
}

/// True if the tool is registered and declared destructive.
#[must_use]
pub fn is_destructive(name: &str) -> bool {
    metadata(name).is_some_and(|m| m.destructive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duplicate_tool_names() {
        let mut seen = std::collections::HashSet::new();
        for meta in ALL_TOOLS {
            assert!(
                seen.insert(meta.name),
                "duplicate tool entry: {}",
                meta.name
            );
        }
    }

    #[test]
    fn destructive_implies_not_read_only() {
        for meta in ALL_TOOLS {
            if meta.destructive {
                assert!(
                    !meta.read_only,
                    "tool {} is destructive but marked read_only",
                    meta.name
                );
            }
        }
    }

    #[test]
    fn known_read_only_tools_resolve() {
        assert!(is_read_only("read_file"));
        assert!(is_read_only("grep"));
        assert!(is_read_only("git_status"));
        assert!(!is_read_only("write_file"));
        assert!(!is_read_only("run_command"));
        assert!(!is_read_only("unknown_tool"));
    }
}

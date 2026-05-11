use super::models::ToolDefinition;

/// Build the standard tool definitions that are sent to `DeepSeek` API.
/// These mirror the local tool implementations in `src/tools/`.
#[must_use]
pub fn standard_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        read_file_def(),
        list_dir_def(),
        search_files_def(),
        search_code_def(),
        git_status_def(),
        git_diff_def(),
        git_add_def(),
        git_commit_def(),
        edit_file_def(),
        write_file_def(),
        apply_patch_def(),
        run_command_def(),
        fetch_url_def(),
        web_search_def(),
        github_pr_def(),
        semantic_search_def(),
        run_subagent_def(),
        think_def(),
    ]
}

fn read_file_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "read_file".into(),
            description: "Read a local file. Relative paths are resolved inside the project workspace; absolute paths may read files outside the workspace after user approval. Protected secrets and system-sensitive paths can be blocked by policy. On Windows, prefer forward slashes in absolute paths, such as C:/Users/name/file.txt, to avoid JSON backslash escaping mistakes.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path, or an absolute local path such as C:/Users/name/file.txt" },
                    "offset": { "type": "integer", "description": "Line offset to start reading from" },
                    "limit": { "type": "integer", "description": "Number of lines to read" }
                },
                "required": ["path"]
            }),
        },
    }
}

fn list_dir_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "list_dir".into(),
            description: "List local files and directories. Relative paths are resolved inside the project workspace; absolute directories outside the workspace require user approval. On Windows, prefer forward slashes in absolute paths, such as C:/Users/name/Desktop.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path, or an absolute local directory path; use forward slashes on Windows, for example C:/Users/name/Desktop" },
                    "recursive": { "type": "boolean", "description": "Whether to list recursively" }
                },
                "required": ["path"]
            }),
        },
    }
}

fn search_files_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "search_files".into(),
            description: "Search for files by name glob pattern.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "File name or glob pattern" },
                    "limit": { "type": "integer", "description": "Maximum results (default 20)" }
                },
                "required": ["query"]
            }),
        },
    }
}

fn search_code_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "search_code".into(),
            description: "Search code content with a regex pattern.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "glob": { "type": "string", "description": "File glob filter" },
                    "case_sensitive": { "type": "boolean", "description": "Case sensitive search" },
                    "limit": { "type": "integer", "description": "Maximum results (default 30)" }
                },
                "required": ["pattern"]
            }),
        },
    }
}

fn git_status_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "git_status".into(),
            description: "Show git working tree status.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    }
}

fn git_diff_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "git_diff".into(),
            description: "Show git diff (unstaged by default).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "staged": { "type": "boolean", "description": "Show staged changes" },
                    "path": { "type": "string", "description": "Limit diff to a specific file" }
                },
                "required": []
            }),
        },
    }
}

fn edit_file_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "edit_file".into(),
            description: "Replace a string in a file (exact match required).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path in the workspace" },
                    "old_string": { "type": "string", "description": "Exact text to replace" },
                    "new_string": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
    }
}

fn write_file_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "write_file".into(),
            description: "Write or overwrite a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path in the workspace" },
                    "content": { "type": "string", "description": "Full file content" }
                },
                "required": ["path", "content"]
            }),
        },
    }
}

fn apply_patch_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "apply_patch".into(),
            description: "Apply a unified diff patch to files in the workspace.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "patch": { "type": "string", "description": "Unified diff content" }
                },
                "required": ["patch"]
            }),
        },
    }
}

fn run_command_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "run_command".into(),
            description: "Run a shell command in the project workspace. Requires approval.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute" },
                    "cwd": { "type": "string", "description": "Working directory (default: project root)" },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Command timeout in seconds. Defaults to policy.command_timeout_seconds.",
                        "minimum": 1,
                        "maximum": 600
                    }
                },
                "required": ["command"]
            }),
        },
    }
}

fn git_add_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "git_add".into(),
            description: "Stage files for commit. Use this before git_commit.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of file paths to stage (relative to project root)"
                    }
                },
                "required": ["paths"]
            }),
        },
    }
}

fn git_commit_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "git_commit".into(),
            description: "Commit staged changes with a message. Requires git_add first.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Commit message (should be concise and descriptive)"
                    }
                },
                "required": ["message"]
            }),
        },
    }
}

fn web_search_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "web_search".into(),
            description: "Search the web for information. Uses DuckDuckGo (no API key needed). \
	Use this to find documentation, library versions, error solutions, or any up-to-date info."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 10, max 20)"
                    }
                },
                "required": ["query"]
            }),
        },
    }
}

fn github_pr_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "github_pr".into(),
            description: "Interact with GitHub pull requests. \
	Actions: list (list PRs), get (get PR details), diff (get PR diff), comment (add a comment). \
	Requires GITHUB_TOKEN env var for comment action."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "get", "diff", "comment"],
                        "description": "Action to perform"
                    },
                    "owner": { "type": "string", "description": "Repository owner" },
                    "repo": { "type": "string", "description": "Repository name" },
                    "number": { "type": "integer", "description": "PR number (for get/diff/comment)" },
                    "state": { "type": "string", "description": "Filter for list: open/closed/all (default open)" },
                    "body": { "type": "string", "description": "Comment body (for comment action)" }
                },
                "required": ["action", "owner", "repo"]
            }),
        },
    }
}

fn semantic_search_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "semantic_search".into(),
            description: "Search the codebase using TF-IDF semantic similarity. \
	Returns files ranked by relevance to the query. Good for finding conceptually related code."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 10)"
                    }
                },
                "required": ["query"]
            }),
        },
    }
}

fn fetch_url_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "fetch_url".into(),
            description: "Fetch the content of a URL and return it as plain text. \
	Use this to read documentation, API references, or any web page. \
	HTML is stripped automatically. Content is truncated to ~8000 chars."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Full URL to fetch (e.g. https://docs.rs/serde/latest/serde/)"
                    }
                },
                "required": ["url"]
            }),
        },
    }
}

fn run_subagent_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "run_subagent".into(),
            description: "Spawn a specialized subagent to handle a specific task independently. \
	Use this when a task can be decomposed into parallel or isolated work. \
	The subagent runs with its own context and returns a summary result. \
	Available types: general-purpose, code-explorer, code-reviewer, planner, test-runner, architect."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "Short description of what the subagent should do (shown in UI)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Detailed instructions for the subagent"
                    },
                    "subagent_type": {
                        "type": "string",
                        "description": "Type of subagent to spawn",
                        "enum": ["general-purpose", "code-explorer", "code-reviewer", "planner", "test-runner", "architect"]
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional additional context (file contents, search results, etc.)"
                    },
                    "focus_files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Files the subagent should focus on"
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional model override (deepseek-v4-pro or deepseek-v4-flash)"
                    },
                    "max_turns": {
                        "type": "integer",
                        "description": "Maximum tool-call turns before forcing completion (default: 10)"
                    }
                },
                "required": ["description", "prompt"]
            }),
        },
    }
}

fn think_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "think".into(),
            description: "Use this tool to think through a problem step by step. \
	Call this before making complex decisions or when you need to reason about multiple options. \
	Your thought will be recorded but no action will be taken — follow up with the actual tool calls after thinking.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "thought": {
                        "type": "string",
                        "description": "Your step-by-step reasoning about the problem or decision"
                    }
                },
                "required": ["thought"]
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_and_list_tools_advertise_absolute_local_paths() {
        let tools = standard_tool_definitions();
        let read = tools
            .iter()
            .find(|tool| tool.function.name == "read_file")
            .expect("read_file tool");
        let list = tools
            .iter()
            .find(|tool| tool.function.name == "list_dir")
            .expect("list_dir tool");

        assert!(read.function.description.contains("absolute paths"));
        assert!(read.function.description.contains("user approval"));
        assert_eq!(
            read.function.parameters["properties"]["path"]["description"],
            "Workspace-relative path, or an absolute local path such as C:/Users/name/file.txt"
        );
        assert!(read.function.description.contains("forward slashes"));
        assert!(list.function.description.contains("absolute directories"));
        assert!(list.function.description.contains("user approval"));
        assert!(list.function.description.contains("forward slashes"));
    }
}

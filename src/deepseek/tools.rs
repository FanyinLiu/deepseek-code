use super::models::ToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionProfile {
    SafeRead,
    WorkspaceWrite,
    Command,
    Network,
    GitMutation,
    Agent,
    InternalReasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOutputSummary {
    Inline,
    Compact,
    UserQuestion,
    PersistentState,
}

#[derive(Debug, Clone)]
pub struct ToolRegistryEntry {
    pub definition: ToolDefinition,
    pub display_name: &'static str,
    pub permission: ToolPermissionProfile,
    pub risk: ToolRiskLevel,
    pub output_summary: ToolOutputSummary,
}

/// Build the standard tool definitions that are sent to `DeepSeek` API.
/// These mirror the local tool implementations in `src/tools/`.
#[must_use]
pub fn standard_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        read_file_def(),
        list_dir_def(),
        glob_def(),
        grep_def(),
        search_files_def(),
        search_code_def(),
        todo_write_def(),
        task_create_def(),
        task_get_def(),
        task_list_def(),
        task_update_def(),
        task_stop_def(),
        ask_user_def(),
        ask_user_question_def(),
        git_status_def(),
        git_diff_def(),
        git_add_def(),
        git_commit_def(),
        edit_file_def(),
        write_file_def(),
        apply_patch_def(),
        run_command_def(),
        bash_output_def(),
        kill_shell_def(),
        notebook_edit_def(),
        fetch_url_def(),
        web_search_def(),
        github_pr_def(),
        semantic_search_def(),
        run_subagent_def(),
        think_def(),
    ]
}

#[must_use]
pub fn standard_tool_registry() -> Vec<ToolRegistryEntry> {
    standard_tool_definitions()
        .into_iter()
        .map(|definition| {
            let name = definition.function.name.clone();
            let permission = tool_permission_profile(&name);
            let display_name = tool_display_name(&name);
            ToolRegistryEntry {
                definition,
                display_name,
                permission,
                risk: tool_risk_level(permission),
                output_summary: tool_output_summary(&name),
            }
        })
        .collect()
}

fn tool_display_name(name: &str) -> &'static str {
    match name {
        "read_file" => "Read file",
        "list_dir" => "List directory",
        "glob" => "Glob",
        "grep" => "Grep",
        "search_files" => "Search files",
        "search_code" => "Search code",
        "todo_write" => "Todo write",
        "task_create" => "TaskCreate",
        "task_get" => "TaskGet",
        "task_list" => "TaskList",
        "task_update" => "TaskUpdate",
        "task_stop" => "TaskStop",
        "ask_user" => "Ask user",
        "ask_user_question" => "Ask user question",
        "git_status" => "Git status",
        "git_diff" => "Git diff",
        "git_add" => "Git add",
        "git_commit" => "Git commit",
        "edit_file" => "Edit file",
        "write_file" => "Write file",
        "apply_patch" => "Apply patch",
        "notebook_edit" => "Notebook edit",
        "run_command" => "Run command",
        "bash_output" => "Bash output",
        "kill_shell" => "Kill shell",
        "fetch_url" => "Fetch URL",
        "web_search" => "Web search",
        "github_pr" => "GitHub PR",
        "semantic_search" => "Semantic search",
        "run_subagent" => "Run subagent",
        "think" => "Think",
        _ => "Tool",
    }
}

fn tool_permission_profile(name: &str) -> ToolPermissionProfile {
    match name {
        "read_file" | "list_dir" | "glob" | "grep" | "search_files" | "search_code"
        | "git_status" | "git_diff" | "semantic_search" | "task_get" | "task_list" | "ask_user"
        | "ask_user_question" => ToolPermissionProfile::SafeRead,
        "todo_write" | "task_create" | "task_update" | "task_stop" => {
            ToolPermissionProfile::WorkspaceWrite
        }
        "edit_file" | "write_file" | "apply_patch" | "notebook_edit" => {
            ToolPermissionProfile::WorkspaceWrite
        }
        "git_add" | "git_commit" => ToolPermissionProfile::GitMutation,
        "run_command" | "kill_shell" => ToolPermissionProfile::Command,
        "bash_output" => ToolPermissionProfile::SafeRead,
        "fetch_url" | "web_search" | "github_pr" => ToolPermissionProfile::Network,
        "run_subagent" => ToolPermissionProfile::Agent,
        "think" => ToolPermissionProfile::InternalReasoning,
        _ => ToolPermissionProfile::Command,
    }
}

fn tool_output_summary(name: &str) -> ToolOutputSummary {
    match name {
        "todo_write" | "task_create" | "task_update" | "task_stop" => {
            ToolOutputSummary::PersistentState
        }
        "ask_user" | "ask_user_question" => ToolOutputSummary::UserQuestion,
        "read_file" | "list_dir" | "grep" | "search_code" | "run_command" | "bash_output" => {
            ToolOutputSummary::Compact
        }
        _ => ToolOutputSummary::Inline,
    }
}

fn tool_risk_level(permission: ToolPermissionProfile) -> ToolRiskLevel {
    match permission {
        ToolPermissionProfile::SafeRead | ToolPermissionProfile::InternalReasoning => {
            ToolRiskLevel::Low
        }
        ToolPermissionProfile::WorkspaceWrite | ToolPermissionProfile::Agent => {
            ToolRiskLevel::Medium
        }
        ToolPermissionProfile::Command
        | ToolPermissionProfile::Network
        | ToolPermissionProfile::GitMutation => ToolRiskLevel::High,
    }
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

fn glob_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "glob".into(),
            description: "Find files by path pattern. Supports **, *, ?, and character classes. Respects .gitignore and sorts results by modification time. Prefer this over shell find, dir, ls, or rg --files.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern, for example **/*.rs or src/**/test_*.py" },
                    "path": { "type": "string", "description": "Optional search root; defaults to project root" },
                    "limit": { "type": "integer", "description": "Maximum paths to return (default 200, max 2000)" }
                },
                "required": ["pattern"]
            }),
        },
    }
}

fn grep_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "grep".into(),
            description: "Regex content search across files. Respects .gitignore. Supports files_with_matches, content with before/after context, and count modes. Prefer this over shell grep or rg.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Rust regex syntax" },
                    "path": { "type": "string", "description": "Optional search root; defaults to project root" },
                    "glob": { "type": "string", "description": "Optional file-name glob filter, for example *.rs" },
                    "output_mode": { "type": "string", "enum": ["files_with_matches", "content", "count"], "description": "Default is files_with_matches" },
                    "case_insensitive": { "type": "boolean" },
                    "multiline": { "type": "boolean", "description": "Allow pattern to span lines; dot matches newline" },
                    "before": { "type": "integer", "description": "Context lines before each match in content mode" },
                    "after": { "type": "integer", "description": "Context lines after each match in content mode" },
                    "head_limit": { "type": "integer", "description": "Stop after this many files (default 250)" },
                    "show_line_numbers": { "type": "boolean" }
                },
                "required": ["pattern"]
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

fn todo_write_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "todo_write".into(),
            description: "Replace the current todo list with structured items. Pass the full list each call; omitted items are deleted. Use this proactively whenever a task has three or more steps. Keep at most one item in_progress.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Stable todo id" },
                                "content": { "type": "string", "description": "Todo text" },
                                "active_form": { "type": "string", "description": "Present-continuous wording for active state, for example Reading config.toml" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"], "description": "Todo state" },
                                "priority": { "type": "string", "enum": ["low", "medium", "high"], "description": "Optional priority" }
                            },
                            "required": ["content", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }),
        },
    }
}

fn task_create_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "task_create".into(),
            description: "Create a project-local task in .octocode/todos.json. Use this to track multi-step work in the same state shown by /todo and /task.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Optional stable task id. If omitted, Octocode assigns task-N." },
                    "content": { "type": "string", "description": "Task text or objective" },
                    "active_form": { "type": "string", "description": "Present-continuous wording for in_progress display, for example Implementing task tools" },
                    "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"], "description": "Initial task status. Defaults to pending." }
                },
                "required": ["content"]
            }),
        },
    }
}

fn task_get_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "task_get".into(),
            description: "Load one project-local task from .octocode/todos.json by id, id prefix, or 1-based list number.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id or unique id prefix" }
                },
                "required": ["id"]
            }),
        },
    }
}

fn task_list_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "task_list".into(),
            description: "List project-local tasks from .octocode/todos.json, the same state used by todo_write and /todo.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    }
}

fn task_update_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "task_update".into(),
            description:
                "Update a project-local task in .octocode/todos.json. Can change status, content, or active_form."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id, unique id prefix, or 1-based list number" },
                    "content": { "type": "string", "description": "Updated task text" },
                    "active_form": { "type": "string", "description": "Updated present-continuous active wording" },
                    "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"], "description": "New task status" }
                },
                "required": ["id"]
            }),
        },
    }
}

fn task_stop_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "task_stop".into(),
            description: "Cancel a project-local task in .octocode/todos.json by id, id prefix, or 1-based list number.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id or unique id prefix" }
                },
                "required": ["id"]
            }),
        },
    }
}

fn ask_user_question_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "ask_user_question".into(),
            description: "Ask the user one structured question when progress depends on a real choice. Prefer ask_user for multi-question prompts. Do not use this for questions answerable from local files.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "Question shown to the user" },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string", "description": "Short option label" },
                                "description": { "type": "string", "description": "What choosing this option means" },
                                "preview": { "type": "string", "description": "Optional text preview or mockup" }
                            },
                            "required": ["label"]
                        }
                    },
                    "multi_select": { "type": "boolean", "description": "Whether multiple options may be selected" }
                },
                "required": ["question", "options"]
            }),
        },
    }
}

fn ask_user_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "ask_user".into(),
            description: "Ask the user one or more structured questions when a real decision is needed, such as choosing a library or resolving ambiguity. Prefer this over prose questions. Each question needs 2-4 options. Preview is only for single-select options.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 4,
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": { "type": "string" },
                                "header": { "type": "string", "description": "Short chip label, 12 chars or fewer" },
                                "multi_select": { "type": "boolean", "default": false },
                                "options": {
                                    "type": "array",
                                    "minItems": 2,
                                    "maxItems": 4,
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": { "type": "string" },
                                            "description": { "type": "string" },
                                            "preview": { "type": "string" }
                                        },
                                        "required": ["label", "description"]
                                    }
                                }
                            },
                            "required": ["question", "header", "options"]
                        }
                    }
                },
                "required": ["questions"]
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
            description: "Run a shell command in the project workspace. Requires approval. Do not use this for file reading, directory listing, or code search commands such as cat, ls, find, grep, rg, sed, head, or tail; use read_file, list_dir, search_files, or search_code instead. For long-running commands (npm install, cargo build, tests), pass run_in_background: true to get a shell_id back immediately and then poll progress via bash_output.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute" },
                    "cwd": { "type": "string", "description": "Working directory (default: project root)" },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Command timeout in seconds. Ignored when run_in_background is true.",
                        "minimum": 1,
                        "maximum": 600
                    },
                    "run_in_background": {
                        "type": "boolean",
                        "description": "If true, spawn the command in the background and return a shell_id immediately instead of waiting for completion. Use bash_output to poll progress."
                    }
                },
                "required": ["command"]
            }),
        },
    }
}

fn bash_output_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "bash_output".into(),
            description: "Poll incremental stdout/stderr from a background shell previously started via run_command with run_in_background: true. Returns only new output since the last cursor; pass back next_stdout_cursor / next_stderr_cursor on the next call.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "shell_id": { "type": "string", "description": "The shell id returned by run_command in background mode." },
                    "stdout_cursor": { "type": "integer", "minimum": 0, "description": "Cursor returned by the previous call; defaults to 0." },
                    "stderr_cursor": { "type": "integer", "minimum": 0, "description": "Cursor returned by the previous call; defaults to 0." }
                },
                "required": ["shell_id"]
            }),
        },
    }
}

fn kill_shell_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "kill_shell".into(),
            description: "Terminate a background shell. Safe on already-exited shells.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "shell_id": { "type": "string", "description": "The shell id to terminate." }
                },
                "required": ["shell_id"]
            }),
        },
    }
}

fn notebook_edit_def() -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".into(),
        function: super::models::FunctionDef {
            name: "notebook_edit".into(),
            description: "Modify a single cell in a .ipynb notebook. Target the cell by 0-based index or by its `id` field. Modes: replace (rewrite source and clear outputs; default), insert (add a new code cell after target), delete (remove target cell).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path to the .ipynb file." },
                    "cell": { "type": "string", "description": "Either a numeric index (\"0\", \"3\") or the cell's `id` field." },
                    "new_source": { "type": "string", "description": "New cell source. Required for replace/insert." },
                    "edit_mode": { "type": "string", "enum": ["replace", "insert", "delete"], "description": "Default: replace." }
                },
                "required": ["path", "cell"]
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

        let run = tools
            .iter()
            .find(|tool| tool.function.name == "run_command")
            .expect("run_command tool");
        assert!(run.function.description.contains("Requires approval"));
        assert!(run
            .function
            .description
            .contains("Do not use this for file reading"));
        assert!(run.function.description.contains("cat, ls, find, grep, rg"));
        assert!(run.function.description.contains("use read_file, list_dir"));
    }

    #[test]
    fn standard_tool_registry_covers_all_builtin_tools() {
        let definitions = standard_tool_definitions();
        let registry = standard_tool_registry();

        assert_eq!(registry.len(), definitions.len());
        for definition in definitions {
            let entry = registry
                .iter()
                .find(|entry| entry.definition.function.name == definition.function.name)
                .unwrap_or_else(|| {
                    panic!("missing registry entry for {}", definition.function.name)
                });
            assert_ne!(entry.display_name, "Tool");
        }

        let run_command = registry
            .iter()
            .find(|entry| entry.definition.function.name == "run_command")
            .expect("run_command registry entry");
        assert_eq!(run_command.permission, ToolPermissionProfile::Command);
        assert_eq!(run_command.risk, ToolRiskLevel::High);

        let read_file = registry
            .iter()
            .find(|entry| entry.definition.function.name == "read_file")
            .expect("read_file registry entry");
        assert_eq!(read_file.permission, ToolPermissionProfile::SafeRead);
        assert_eq!(read_file.risk, ToolRiskLevel::Low);

        let task_create = registry
            .iter()
            .find(|entry| entry.definition.function.name == "task_create")
            .expect("task_create registry entry");
        assert_eq!(task_create.display_name, "TaskCreate");
        assert_eq!(
            task_create.permission,
            ToolPermissionProfile::WorkspaceWrite
        );
        assert_eq!(
            task_create.output_summary,
            ToolOutputSummary::PersistentState
        );

        let task_list = registry
            .iter()
            .find(|entry| entry.definition.function.name == "task_list")
            .expect("task_list registry entry");
        assert_eq!(task_list.display_name, "TaskList");
        assert_eq!(task_list.permission, ToolPermissionProfile::SafeRead);
    }
}

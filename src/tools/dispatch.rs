use std::path::Path;

use crate::policy::sandbox::CommandSandboxConfig;
use crate::storage::config::PolicyConfig;
use crate::tools;

#[derive(Debug, Clone)]
pub struct ToolDispatchConfig {
    pub command_timeout_seconds: u64,
    pub command_sandbox: CommandSandboxConfig,
}

impl ToolDispatchConfig {
    #[must_use]
    pub fn from_policy(policy: &PolicyConfig) -> Self {
        Self {
            command_timeout_seconds: policy.command_timeout_seconds.clamp(1, 600),
            command_sandbox: policy.command_sandbox.clone(),
        }
    }
}

impl Default for ToolDispatchConfig {
    fn default() -> Self {
        Self::from_policy(&PolicyConfig::default())
    }
}

/// Suggest similar files when a read_file path is not found.
fn suggest_similar_files(project_root: &Path, requested_path: &str) -> Option<String> {
    let requested_name = Path::new(requested_path)
        .file_name()?
        .to_str()?
        .to_lowercase();

    let mut matches = Vec::new();
    let mut dirs = vec![project_root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_str()?.to_lowercase();
                if name.contains(&requested_name) || requested_name.contains(&name) {
                    let rel = path.strip_prefix(project_root).ok()?;
                    matches.push(format!("  - {}", rel.display()));
                    if matches.len() >= 5 {
                        break;
                    }
                }
                if path.is_dir() && dirs.len() < 3 {
                    dirs.push(path);
                }
            }
        }
        if matches.len() >= 5 {
            break;
        }
    }

    if matches.is_empty() {
        None
    } else {
        Some(matches.join("\n"))
    }
}

/// Provide current file context when edit_file fails because old_string is not found.
fn get_edit_context(project_root: &Path, path: &str, old_string: &str) -> Option<String> {
    let resolved = crate::workspace::paths::resolve_workspace_path(project_root, path)?;
    let content = std::fs::read_to_string(&resolved).ok()?;

    // Try to find lines containing a substring of old_string
    let target = old_string.lines().next()?.trim();
    let fragment = &target[..target.len().min(20)];
    let mut lines_found = Vec::new();

    for (i, line) in content.lines().enumerate() {
        if line.trim().contains(fragment) {
            lines_found.push(format!("{:>6} | {}", i + 1, line));
            if lines_found.len() >= 5 {
                break;
            }
        }
    }

    if lines_found.is_empty() {
        let preview: Vec<_> = content
            .lines()
            .take(20)
            .enumerate()
            .map(|(i, l)| format!("{:>6} | {}", i + 1, l))
            .collect();
        Some(format!(
            "First 20 lines of the file:\n{}",
            preview.join("\n")
        ))
    } else {
        Some(format!(
            "Lines containing similar text:\n{}",
            lines_found.join("\n")
        ))
    }
}

/// Analyze a failed command result and suggest fixes.
fn suggest_command_fix(result: &tools::run_command::CommandResult) -> Option<String> {
    if result.timed_out {
        return Some("Command timed out. Try a simpler command or increase timeout.".into());
    }

    let stderr_lower = result.stderr.to_lowercase();

    if stderr_lower.contains("not found") || stderr_lower.contains("not recognized") {
        return Some("Command not found. Check if the tool is installed and in PATH.".into());
    }

    if stderr_lower.contains("permission denied") {
        return Some(
            "Permission denied. Try checking file permissions or using a different path.".into(),
        );
    }

    if stderr_lower.contains("no such file or directory") {
        return Some("File or directory not found. Check the path and ensure it exists.".into());
    }

    if stderr_lower.contains("syntax error") {
        return Some("Syntax error in command. Check quoting and special characters.".into());
    }

    if result.exit_code == -1 {
        return Some("Command failed to execute. Check syntax and available tools.".into());
    }

    None
}

fn update_todos(project_root: &Path, args: &serde_json::Value) -> Result<String, anyhow::Error> {
    let todos = args
        .get("todos")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("todos must be an array"))?;
    let summary = crate::tools::todo_state::write_todo_items(project_root, todos.clone())?;
    Ok(format!(
        "Todo list updated: {} item(s), pending {}, in_progress {}, completed {}.",
        summary.total, summary.pending, summary.in_progress, summary.completed
    ))
}

fn ask_user_question_payload(args: &serde_json::Value) -> Result<String, anyhow::Error> {
    if args.get("questions").is_some() {
        let parsed: crate::tools::ask_user::AskUserArgs = serde_json::from_value(args.clone())?;
        return crate::tools::ask_user::ask_user(parsed);
    }
    let question = args
        .get("question")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("question is required"))?;
    let options = args
        .get("options")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("options must be an array"))?;
    if options.len() < 2 {
        anyhow::bail!("options needs at least 2 options");
    }
    let mut lines = vec![
        "User decision required".to_string(),
        format!("question  {question}"),
        format!(
            "select    {}",
            if args
                .get("multi_select")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "multiple"
            } else {
                "one"
            }
        ),
        String::new(),
    ];
    for (idx, option) in options.iter().enumerate() {
        let label = option
            .get("label")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("option label is required"))?;
        let description = option
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        lines.push(format!("{}. {}  {}", idx + 1, label, description));
        if let Some(preview) = option.get("preview").and_then(serde_json::Value::as_str) {
            if !preview.trim().is_empty() {
                lines.push(format!("   preview: {}", preview.trim()));
            }
        }
    }
    lines.push(String::new());
    lines.push(
        "(Waiting for the user's reply in the next turn. Do not assume an answer.)".to_string(),
    );
    Ok(lines.join("\n"))
}

/// Execute a single tool call and return `(result_text, is_error)`.
pub async fn execute_single_tool(
    tc: &crate::deepseek::ToolCall,
    project_root: &Path,
) -> (String, bool) {
    execute_single_tool_with_config(tc, project_root, ToolDispatchConfig::default()).await
}

/// Execute a single tool call with explicit dispatch defaults.
pub async fn execute_single_tool_with_config(
    tc: &crate::deepseek::ToolCall,
    project_root: &Path,
    dispatch_config: ToolDispatchConfig,
) -> (String, bool) {
    let args: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
        Ok(v) => v,
        Err(e) => {
            return (format!("Invalid JSON in tool arguments: {e}"), true);
        }
    };

    match tc.function.name.as_str() {
        "read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let offset = args["offset"].as_u64().map(|v| v as usize);
            let limit = args["limit"].as_u64().map(|v| v as usize);
            match tools::read_file(project_root, path, offset, limit) {
                Ok(content) => (content, false),
                Err(e) => {
                    let mut msg = e.to_string();
                    let err_str = e.to_string().to_lowercase();
                    if err_str.contains("not found")
                        || err_str.contains("no such file")
                        || err_str.contains("protected")
                    {
                        if let Some(suggestions) = suggest_similar_files(project_root, path) {
                            msg.push_str(&format!("\n\nDid you mean one of these?\n{suggestions}"));
                        }
                    }
                    (msg, true)
                }
            }
        }
        "list_dir" => {
            let path = args["path"].as_str().unwrap_or(".");
            let recursive = args["recursive"].as_bool().unwrap_or(false);
            match tools::list_dir(project_root, path, recursive) {
                Ok(content) => (content, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "glob" => {
            let parsed: Result<crate::tools::glob::GlobArgs, _> = serde_json::from_value(args);
            match parsed {
                Ok(parsed) => match crate::tools::glob::glob(project_root, parsed) {
                    Ok(content) => (content, false),
                    Err(e) => (e.to_string(), true),
                },
                Err(e) => (e.to_string(), true),
            }
        }
        "grep" => {
            let parsed: Result<crate::tools::grep::GrepArgs, _> = serde_json::from_value(args);
            match parsed {
                Ok(parsed) => match crate::tools::grep::grep(project_root, parsed) {
                    Ok(content) => (content, false),
                    Err(e) => (e.to_string(), true),
                },
                Err(e) => (e.to_string(), true),
            }
        }
        "search_files" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;
            match crate::search::search_files(project_root, query, limit) {
                Ok(matches) => (
                    matches
                        .iter()
                        .map(|m| m.path.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    false,
                ),
                Err(e) => (e.to_string(), true),
            }
        }
        "search_code" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            let glob = args["glob"].as_str();
            let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);
            let limit = args["limit"].as_u64().unwrap_or(30) as usize;
            match crate::search::search_code(project_root, pattern, glob, case_sensitive, limit) {
                Ok(matches) => (
                    matches
                        .iter()
                        .map(|m| {
                            format!(
                                "{}:{}: {}",
                                m.path.display(),
                                m.line_number.unwrap_or(0),
                                m.matched_text
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    false,
                ),
                Err(e) => (e.to_string(), true),
            }
        }
        "todo_write" => match update_todos(project_root, &args) {
            Ok(content) => (content, false),
            Err(e) => (e.to_string(), true),
        },
        "task_create" => {
            let parsed: Result<crate::tools::task_todo::TaskCreateArgs, _> =
                serde_json::from_value(args);
            match parsed {
                Ok(parsed) => match crate::tools::task_todo::task_create(project_root, parsed) {
                    Ok(content) => (content, false),
                    Err(e) => (e.to_string(), true),
                },
                Err(e) => (e.to_string(), true),
            }
        }
        "task_get" => {
            let id = args["id"].as_str().unwrap_or("");
            match crate::tools::task_todo::task_get(project_root, id) {
                Ok(content) => (content, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "task_list" => match crate::tools::task_todo::task_list(project_root) {
            Ok(content) => (content, false),
            Err(e) => (e.to_string(), true),
        },
        "task_update" => {
            let parsed: Result<crate::tools::task_todo::TaskUpdateArgs, _> =
                serde_json::from_value(args);
            match parsed {
                Ok(parsed) => match crate::tools::task_todo::task_update(project_root, parsed) {
                    Ok(content) => (content, false),
                    Err(e) => (e.to_string(), true),
                },
                Err(e) => (e.to_string(), true),
            }
        }
        "task_stop" => {
            let id = args["id"].as_str().unwrap_or("");
            match crate::tools::task_todo::task_stop(project_root, id) {
                Ok(content) => (content, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "ask_user" => {
            let parsed: Result<crate::tools::ask_user::AskUserArgs, _> =
                serde_json::from_value(args);
            match parsed {
                Ok(parsed) => match crate::tools::ask_user::ask_user(parsed) {
                    Ok(content) => (content, false),
                    Err(e) => (e.to_string(), true),
                },
                Err(e) => (e.to_string(), true),
            }
        }
        "ask_user_question" => match ask_user_question_payload(&args) {
            Ok(content) => (content, false),
            Err(e) => (e.to_string(), true),
        },
        "git_status" => match tools::git_status(project_root) {
            Ok(content) => (content, false),
            Err(e) => (e.to_string(), true),
        },
        "git_diff" => {
            let staged = args["staged"].as_bool().unwrap_or(false);
            let path = args["path"].as_str();
            match tools::git_diff(project_root, staged, path) {
                Ok(content) => (content, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "edit_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let old_string = args["old_string"].as_str().unwrap_or("");
            let new_string = args["new_string"].as_str().unwrap_or("");
            match tools::edit_file(project_root, path, old_string, new_string) {
                Ok(result) => (result.diff, false),
                Err(e) => {
                    let mut msg = e.to_string();
                    let err_str = e.to_string().to_lowercase();
                    if err_str.contains("not found") || err_str.contains("must be unique") {
                        if let Some(context) = get_edit_context(project_root, path, old_string) {
                            msg.push_str(&format!("\n\n{context}"));
                        }
                    }
                    (msg, true)
                }
            }
        }
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            match tools::write_file(project_root, path, content) {
                Ok(result) => (result.diff, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "apply_patch" => {
            let patch = args["patch"].as_str().unwrap_or("");
            match crate::workspace::apply::apply_patch(project_root, patch) {
                Ok(()) => ("Patch applied successfully".to_string(), false),
                Err(e) => (e.to_string(), true),
            }
        }
        "run_command" => {
            let command = args["command"].as_str().unwrap_or("");
            let cwd = args["cwd"].as_str();
            let timeout_seconds = args["timeout_seconds"]
                .as_u64()
                .or_else(|| args["timeout"].as_u64())
                .unwrap_or(dispatch_config.command_timeout_seconds)
                .clamp(1, 600);
            match tools::run_command_with_sandbox(
                project_root,
                command,
                cwd,
                timeout_seconds,
                &dispatch_config.command_sandbox,
            )
            .await
            {
                Ok(result) => {
                    if !result.is_success() {
                        let mut summary = result.summary();
                        if let Some(suggestion) = suggest_command_fix(&result) {
                            summary.push_str(&format!("\n\nSuggestion: {suggestion}"));
                        }
                        (crate::policy::redact_all(&summary), true)
                    } else {
                        (crate::policy::redact_all(&result.summary()), false)
                    }
                }
                Err(e) => (crate::policy::redact_all(&e.to_string()), true),
            }
        }
        "git_add" => {
            let paths: Vec<&str> = args["paths"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            match crate::workspace::git::git_add(project_root, &paths) {
                Ok(()) => (format!("Staged {} file(s)", paths.len()), false),
                Err(e) => (e.to_string(), true),
            }
        }
        "git_commit" => {
            let message = args["message"].as_str().unwrap_or("");
            match crate::workspace::git::git_commit(project_root, message) {
                Ok(()) => (format!("Committed: {message}"), false),
                Err(e) => (e.to_string(), true),
            }
        }
        "fetch_url" => {
            let url = args["url"].as_str().unwrap_or("");
            match crate::tools::fetch_url::fetch_url(url, 8000).await {
                Ok(content) => (content, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "web_search" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            match crate::tools::web_search::web_search(query, limit).await {
                Ok(results) => (results, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "github_pr" => {
            let action = args["action"].as_str().unwrap_or("list");
            let owner = args["owner"].as_str().unwrap_or("");
            let repo = args["repo"].as_str().unwrap_or("");
            let number = args["number"].as_u64();
            let state = args["state"].as_str().unwrap_or("open");
            let body = args["body"].as_str().unwrap_or("");

            let result = match action {
                "list" => crate::tools::github::list_prs(owner, repo, state).await,
                "get" => {
                    if let Some(n) = number {
                        crate::tools::github::get_pr(owner, repo, n).await
                    } else {
                        Err(anyhow::anyhow!("PR number required for 'get' action"))
                    }
                }
                "diff" => {
                    if let Some(n) = number {
                        crate::tools::github::get_pr_diff(owner, repo, n).await
                    } else {
                        Err(anyhow::anyhow!("PR number required for 'diff' action"))
                    }
                }
                "comment" => {
                    if let Some(n) = number {
                        crate::tools::github::comment_pr(owner, repo, n, body).await
                    } else {
                        Err(anyhow::anyhow!("PR number required for 'comment' action"))
                    }
                }
                _ => Err(anyhow::anyhow!("Unknown github_pr action: {action}")),
            };
            match result {
                Ok(content) => (content, false),
                Err(e) => (e.to_string(), true),
            }
        }
        "semantic_search" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            match crate::search::semantic::build_project_index(project_root) {
                Ok(index) => {
                    let results = index.search(query, limit);
                    if results.is_empty() {
                        ("No semantically relevant files found.".to_string(), false)
                    } else {
                        let mut output = format!("Semantic search results for '{}':\n\n", query);
                        for (idx, (path, score)) in results.iter().enumerate() {
                            output.push_str(&format!(
                                "{}. {} (score: {:.3})\n",
                                idx + 1,
                                path,
                                score
                            ));
                        }
                        (output, false)
                    }
                }
                Err(e) => (e.to_string(), true),
            }
        }
        "think" => {
            let thought = args["thought"].as_str().unwrap_or("(no thought provided)");
            (format!("💭 Thinking:\n{thought}"), false)
        }
        _ => (format!("unknown tool: {}", tc.function.name), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek::{ToolCall, ToolCallFunction};

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "call-test".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: name.into(),
                arguments: arguments.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn todo_write_persists_project_state_and_accepts_active_form() {
        let temp = tempfile::tempdir().expect("tempdir");
        let tool = call(
            "todo_write",
            serde_json::json!({
                "todos": [
                    {"id":"a","content":"Plan","active_form":"Planning","status":"in_progress"},
                    {"id":"b","content":"Verify","status":"pending"}
                ]
            }),
        );

        let (output, is_error) = execute_single_tool(&tool, temp.path()).await;

        assert!(!is_error, "{output}");
        assert!(output.contains("in_progress 1"));
        assert!(crate::tools::todo_state::todo_state_path(temp.path()).exists());
    }

    #[tokio::test]
    async fn todo_write_rejects_two_in_progress_items() {
        let temp = tempfile::tempdir().expect("tempdir");
        let tool = call(
            "todo_write",
            serde_json::json!({
                "todos": [
                    {"content":"A","status":"in_progress"},
                    {"content":"B","status":"in_progress"}
                ]
            }),
        );

        let (output, is_error) = execute_single_tool(&tool, temp.path()).await;

        assert!(is_error);
        assert!(output.contains("at most one"));
    }

    #[tokio::test]
    async fn task_tools_share_project_todo_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let create = call(
            "task_create",
            serde_json::json!({
                "id": "p0-4",
                "content": "Ship task tools",
                "active_form": "Shipping task tools"
            }),
        );

        let (created, create_error) = execute_single_tool(&create, temp.path()).await;

        assert!(!create_error, "{created}");
        assert!(created.contains("p0-4"));
        assert!(crate::tools::todo_state::todo_state_path(temp.path()).exists());

        let update = call(
            "task_update",
            serde_json::json!({
                "id": "p0",
                "status": "in_progress"
            }),
        );
        let (updated, update_error) = execute_single_tool(&update, temp.path()).await;

        assert!(!update_error, "{updated}");
        assert!(updated.contains("Shipping task tools"));

        let list = call("task_list", serde_json::json!({}));
        let (listed, list_error) = execute_single_tool(&list, temp.path()).await;

        assert!(!list_error, "{listed}");
        assert!(listed.contains("Tasks: 1"));

        let stop = call("task_stop", serde_json::json!({"id": "1"}));
        let (stopped, stop_error) = execute_single_tool(&stop, temp.path()).await;

        assert!(!stop_error, "{stopped}");
        assert!(stopped.contains("cancelled"));
    }

    #[tokio::test]
    async fn task_get_unknown_id_is_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let get = call("task_get", serde_json::json!({"id": "missing"}));

        let (output, is_error) = execute_single_tool(&get, temp.path()).await;

        assert!(is_error);
        assert!(output.contains("task not found"));
    }
}

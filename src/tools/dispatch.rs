use std::path::Path;

use crate::search;
use crate::storage::config::PolicyConfig;
use crate::tools;

#[derive(Debug, Clone, Copy)]
pub struct ToolDispatchConfig {
    pub command_timeout_seconds: u64,
}

impl ToolDispatchConfig {
    #[must_use]
    pub fn from_policy(policy: &PolicyConfig) -> Self {
        Self {
            command_timeout_seconds: policy.command_timeout_seconds.clamp(1, 600),
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
        "search_files" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;
            match search::search_files(project_root, query, limit) {
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
            match search::search_code(project_root, pattern, glob, case_sensitive, limit) {
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
            match tools::run_command(project_root, command, cwd, timeout_seconds).await {
                Ok(result) => {
                    if !result.is_success() {
                        let mut summary = result.summary();
                        if let Some(suggestion) = suggest_command_fix(&result) {
                            summary.push_str(&format!("\n\nSuggestion: {suggestion}"));
                        }
                        (summary, true)
                    } else {
                        (result.summary(), false)
                    }
                }
                Err(e) => (e.to_string(), true),
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

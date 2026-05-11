/// Returns a human-readable risk level string for a tool name.
#[must_use]
pub fn risk_level_for_tool(name: &str) -> &'static str {
    match name {
        "read_file" | "list_dir" | "search_files" | "search_code" | "git_status" | "git_diff"
        | "git_log" => "SafeRead",
        "edit_file" | "write_file" | "apply_patch" => "WriteProject",
        "run_command" => "CommandExecution",
        _ => "Unknown",
    }
}

/// Truncate a string to its first line, capped at `max_len` characters.
#[must_use]
pub fn truncate_for_summary(s: &str, max_len: usize) -> String {
    let first_line = s.lines().next().unwrap_or("");
    if first_line.chars().count() <= max_len {
        first_line.to_string()
    } else {
        let prefix: String = first_line.chars().take(max_len).collect();
        format!("{prefix}...")
    }
}

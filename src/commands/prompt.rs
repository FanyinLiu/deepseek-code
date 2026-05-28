use std::path::Path;

/// A user-defined prompt-type slash command. Loaded from
/// `.octocode/commands/*.md` (project) and `~/.octocode/commands/*.md` (user).
/// When invoked, the body is rendered (with `$ARGUMENTS` substitution) and
/// queued as the next user input, exactly as if the user had typed the
/// rendered text into the prompt.
///
/// Files in subdirectories are namespaced with `:`. For example,
/// `commands/git/status.md` is loaded as `/git:status`.
#[derive(Debug, Clone)]
pub struct PromptCommand {
    pub name: String,
    pub description: String,
    pub body: String,
    pub argument_hint: Option<String>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub source_path: std::path::PathBuf,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ParsedPrompt {
    pub description: String,
    pub body: String,
    pub argument_hint: Option<String>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
}

impl PromptCommand {
    /// Render the body with `$1`-`$9` replaced by whitespace-split args and
    /// `$ARGUMENTS` replaced by the full arg string. Missing positional args
    /// substitute the empty string.
    #[must_use]
    pub fn render(&self, args: &str) -> String {
        let trimmed = args.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let positional = apply_positional_args(&self.body, &parts);
        positional.replace("$ARGUMENTS", trimmed)
    }
}

/// Replace `$1`..`$9` placeholders with the corresponding positional argument.
/// Missing positions become the empty string. `$ARGUMENTS` is left untouched
/// for the caller to substitute; it captures the full, untokenized arg string.
#[must_use]
pub fn apply_positional_args(template: &str, args: &[&str]) -> String {
    let re = positional_arg_regex();
    re.replace_all(template, |caps: &regex::Captures<'_>| {
        let idx: usize = caps[1].parse().unwrap_or(0);
        if idx == 0 || idx > 9 {
            return caps[0].to_string();
        }
        args.get(idx - 1).copied().unwrap_or("").to_string()
    })
    .to_string()
}

fn positional_arg_regex() -> &'static regex::Regex {
    static REGEX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    REGEX.get_or_init(|| regex::Regex::new(r"\$([1-9])").expect("static regex"))
}

/// Parse a prompt-command markdown file into description / body /
/// argument hint. All frontmatter fields are optional; missing pieces fall
/// back to "user-defined prompt" for the description, and `None` for the
/// argument hint.
pub(super) fn parse_prompt_command(content: &str) -> ParsedPrompt {
    let text = content.trim_start();
    if let Some(stripped) = text.strip_prefix("---") {
        if let Some(end) = stripped.find("\n---") {
            let frontmatter = &stripped[..end];
            let body = stripped[end + 4..].trim_start_matches('\n').to_string();
            let mut description = String::new();
            let mut argument_hint = None;
            let mut model = None;
            let mut allowed_tools = None;
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("description:") {
                    description = rest.trim().trim_matches('"').to_string();
                } else if let Some(rest) = line
                    .strip_prefix("argument-hint:")
                    .or_else(|| line.strip_prefix("argument_hint:"))
                {
                    let value = rest.trim().trim_matches('"').to_string();
                    if !value.is_empty() {
                        argument_hint = Some(value);
                    }
                } else if let Some(rest) = line.strip_prefix("model:") {
                    let value = rest.trim().trim_matches('"').to_string();
                    if !value.is_empty() {
                        model = Some(value);
                    }
                } else if let Some(rest) = line
                    .strip_prefix("allowed-tools:")
                    .or_else(|| line.strip_prefix("allowed_tools:"))
                {
                    allowed_tools = parse_allowed_tools_value(rest);
                }
            }
            if description.is_empty() {
                description = "user-defined prompt".to_string();
            }
            return ParsedPrompt {
                description,
                body,
                argument_hint,
                model,
                allowed_tools,
            };
        }
    }

    let mut lines = text.lines();
    let description = lines
        .by_ref()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("user-defined prompt")
        .trim()
        .trim_start_matches('#')
        .trim()
        .to_string();
    let body: String = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    ParsedPrompt {
        description,
        body,
        argument_hint: None,
        model: None,
        allowed_tools: None,
    }
}

/// Expand leading `!cmd` lines in `text` by running each command via `sh -c`
/// from `project_root` and inlining its stdout. Lines that do not start with
/// `!` pass through. When `enabled` is false the text is returned unchanged.
#[must_use]
pub fn expand_bang_lines(text: &str, project_root: &Path, enabled: bool) -> String {
    if !text.contains('!') {
        return text.to_string();
    }
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        let (body, eol) = match line.strip_suffix('\n') {
            Some(rest) => (rest, "\n"),
            None => (line, ""),
        };
        let trimmed = body.trim_start();
        let indent_len = body.len() - trimmed.len();
        if !enabled || !trimmed.starts_with('!') {
            out.push_str(line);
            continue;
        }
        let command = trimmed[1..].trim();
        if command.is_empty() {
            out.push_str(line);
            continue;
        }
        out.push_str(&body[..indent_len]);
        out.push_str(&run_bang_line(command, project_root));
        out.push_str(eol);
    }
    out
}

fn run_bang_line(command: &str, project_root: &Path) -> String {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(project_root)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                String::from_utf8_lossy(&output.stdout)
                    .trim_end_matches('\n')
                    .to_string()
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                format!(
                    "[!{command} failed (exit {}): {}]",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
                )
            }
        }
        Err(error) => format!("[!{command} error: {error}]"),
    }
}

/// True iff `allowed_tools` would permit a custom command to shell out via
/// inline `!cmd` lines. Matches `Bash` case-insensitively to align with
/// Claude Code's `allowed-tools: Bash(...)` convention.
#[must_use]
pub fn allowed_tools_permit_bang(allowed: Option<&[String]>) -> bool {
    let Some(tools) = allowed else {
        return false;
    };
    tools.iter().any(|tool| {
        let head = tool.split(['(', ':']).next().unwrap_or("");
        head.trim().eq_ignore_ascii_case("Bash")
    })
}

/// Parse the value of an `allowed-tools:` frontmatter line.
///
/// Accepts a bracketed YAML list (`[a, b, c]`) or a comma-separated string.
/// Returns `Some` only when at least one non-empty tool name is present.
#[must_use]
pub fn parse_allowed_tools_value(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(trimmed);
    let tools: Vec<String> = inner
        .split(',')
        .map(|piece| {
            piece
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .filter(|piece| !piece.is_empty())
        .collect();
    if tools.is_empty() {
        None
    } else {
        Some(tools)
    }
}

/// Compute the slash-command name for a prompt-command file living under a
/// `commands/` root directory. Subdirectories are joined with `:`.
pub fn prompt_command_name(root: &Path, file: &Path) -> Option<String> {
    let stem = file.file_stem().and_then(|s| s.to_str())?;
    let parent = file.parent()?;
    let relative = parent.strip_prefix(root).ok()?;
    let prefix: Vec<&str> = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect();
    if prefix.is_empty() {
        Some(format!("/{stem}"))
    } else {
        Some(format!("/{}:{stem}", prefix.join(":")))
    }
}

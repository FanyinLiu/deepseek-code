//! Slash command system for the TUI.
use std::collections::HashMap;
use std::io::Write;

/// Context available when executing a slash command.
pub struct CommandContext<'a> {
    pub app: &'a mut crate::tui::app::TuiApp,
    pub project_root: &'a std::path::Path,
    pub yolo_mode: &'a mut bool,
    pub mcp_status: &'a str,
    pub background_tasks: &'a [crate::agent::BackgroundTaskSnapshot],
}

/// A slash command definition.
pub struct SlashCommand {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub usage: &'static str,
    pub handler: fn(&str, &mut CommandContext) -> CommandResult,
}

pub type CommandResult = Result<Option<String>, String>;

/// Registry of all slash commands.
pub struct CommandRegistry {
    commands: HashMap<&'static str, &'static SlashCommand>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };
        registry.register_all();
        registry
    }

    fn register(&mut self, cmd: &'static SlashCommand) {
        self.commands.insert(cmd.name, cmd);
        for &alias in cmd.aliases {
            self.commands.insert(alias, cmd);
        }
    }

    /// Parse and execute a slash command from user input.
    /// Returns `Some(result_message)` if a command was executed, `None` if not a command.
    pub fn execute(&self, input: &str, ctx: &mut CommandContext) -> Option<CommandResult> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        let name = parts[0];
        let args = parts.get(1).copied().unwrap_or("");

        if let Some(cmd) = self.commands.get(name) {
            Some((cmd.handler)(args, ctx))
        } else {
            Some(Err(format!(
                "Unknown command: {name}. Type /help for available commands."
            )))
        }
    }

    /// Get a list of all primary commands for help display.
    #[must_use]
    pub fn list_commands(&self) -> Vec<&&'static SlashCommand> {
        let mut seen = std::collections::HashSet::new();
        let mut list: Vec<&&'static SlashCommand> = self
            .commands
            .values()
            .filter(|cmd| seen.insert(cmd.name))
            .collect();
        list.sort_by_key(|cmd| cmd.name);
        list
    }

    /// Find commands matching a prefix (for autocomplete).
    #[must_use]
    pub fn match_prefix(&self, prefix: &str) -> Vec<&&'static SlashCommand> {
        let mut seen = std::collections::HashSet::new();
        let mut list: Vec<&&'static SlashCommand> = self
            .commands
            .values()
            .filter(|cmd| {
                (cmd.name.starts_with(prefix) || cmd.aliases.iter().any(|a| a.starts_with(prefix)))
                    && seen.insert(cmd.name)
            })
            .collect();
        list.sort_by_key(|cmd| cmd.name);
        list
    }
}

// ---------------------------------------------------------------------------
// Built-in commands
// ---------------------------------------------------------------------------

fn cmd_yolo(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    *ctx.yolo_mode = !*ctx.yolo_mode;
    let msg = if *ctx.yolo_mode {
        "YOLO mode ON — all approvals auto-granted"
    } else {
        "YOLO mode OFF — manual approvals required"
    };
    Ok(Some(msg.to_string()))
}

fn cmd_clear(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.messages.clear();
    ctx.app.stream_buffer.clear();
    ctx.app.reasoning_buffer.clear();
    ctx.app.plan_steps.clear();
    ctx.app.plan_current_step = 0;
    ctx.app.plan_total_steps = 0;
    ctx.app.plan_summary = None;
    ctx.app.subagents.clear();
    ctx.app.file_diffs.clear();
    ctx.app.pending_options = None;
    ctx.app.pending_images.clear();
    ctx.app.activity_log.clear();
    crate::workspace::apply::clear_history();
    Ok(Some("Screen cleared".to_string()))
}

fn cmd_copy(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let Some(message) = ctx
        .app
        .messages
        .iter()
        .rev()
        .find(|message| message.role == crate::deepseek::Role::Assistant)
    else {
        return Ok(Some("No assistant message to copy.".to_string()));
    };
    let text = message.content.to_string_lossy();
    copy_to_clipboard(&text)?;
    Ok(Some(
        "Copied last assistant message to clipboard.".to_string(),
    ))
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        let mut child = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Set-Clipboard -Value $input"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("clipboard command failed: {e}"))?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "clipboard stdin unavailable".to_string())?
            .write_all(text.as_bytes())
            .map_err(|e| format!("clipboard write failed: {e}"))?;
        let output = child
            .wait_with_output()
            .map_err(|e| format!("clipboard command failed: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    } else {
        let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
            &[("pbcopy", &[])]
        } else {
            &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])]
        };
        for (command, args) in candidates {
            let Ok(mut child) = std::process::Command::new(command)
                .args(*args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            else {
                continue;
            };
            if let Some(stdin) = child.stdin.as_mut() {
                if stdin.write_all(text.as_bytes()).is_ok()
                    && child.wait().is_ok_and(|status| status.success())
                {
                    return Ok(());
                }
            }
        }
        Err("No clipboard helper found on PATH.".to_string())
    }
}

fn cmd_undo(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    match crate::workspace::apply::undo_last_change(ctx.project_root) {
        Ok(path) => {
            ctx.app.push_activity(format!("undo: restored {path}"));
            Ok(Some(format!("Undid last change: {path}")))
        }
        Err(e) => Err(format!("Undo failed: {e}")),
    }
}

fn cmd_image(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let path = args.trim();
    if path.is_empty() {
        return Err("Usage: /image <path>".to_string());
    }
    let full_path = ctx.project_root.join(path);
    match crate::tools::image_input::encode_image_to_base64(&full_path) {
        Ok(data_url) => {
            ctx.app.pending_images.push(data_url);
            Ok(Some(format!("Image attached: {path}")))
        }
        Err(e) => Err(format!("Failed to attach image: {e}")),
    }
}

fn cmd_commit(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let message = if args.trim().is_empty() {
        "Auto-commit by deepseek-code"
    } else {
        args.trim()
    };
    if let Err(e) = crate::workspace::git::git_add(ctx.project_root, &["."]) {
        return Err(format!("git add failed: {e}"));
    }
    match crate::workspace::git::git_commit(ctx.project_root, message) {
        Ok(()) => Ok(Some(format!("Committed: {message}"))),
        Err(e) => Err(format!("git commit failed: {e}")),
    }
}

fn cmd_test(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let framework = args.trim();
    let result: Result<String, String> = if framework.is_empty() {
        crate::workspace::test_runner::detect_and_run_tests(ctx.project_root)
            .map_err(|e| e.to_string())
    } else {
        run_local_shell_command(ctx.project_root, framework)
    };
    match result {
        Ok(output) => Ok(Some(output)),
        Err(e) => Err(format!("Test run failed: {e}")),
    }
}

fn run_local_shell_command(
    project_root: &std::path::Path,
    command: &str,
) -> Result<String, String> {
    const MAX_COMMAND_LEN: usize = 4096;
    if command.len() > MAX_COMMAND_LEN {
        return Err(format!(
            "command exceeds maximum length of {MAX_COMMAND_LEN} characters"
        ));
    }
    if let Some(reason) = crate::policy::commands::contains_dangerous_pattern(command) {
        return Err(format!("dangerous command blocked: {reason}"));
    }

    let (shell, shell_arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let output = std::process::Command::new(shell)
        .arg(shell_arg)
        .arg(command)
        .current_dir(project_root)
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("stderr:\n");
        result.push_str(&stderr);
    }
    let code = output.status.code().unwrap_or(-1);
    if !output.status.success() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!("exit_code: {code}"));
    }
    Ok(result)
}

fn cmd_fix(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let target = if args.trim().is_empty() {
        "the current issue"
    } else {
        args.trim()
    };
    ctx.app.status_message = format!("Fixing: {target}");
    Ok(None)
}

fn cmd_explain(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let target = if args.trim().is_empty() {
        "the current context"
    } else {
        args.trim()
    };
    ctx.app.status_message = format!("Explaining: {target}");
    Ok(None)
}

fn cmd_review(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.status_message = "Starting code review...".to_string();
    Ok(None)
}

fn cmd_wiki(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.status_message = "Generating codebase wiki...".to_string();
    Ok(None)
}

fn cmd_readiness_report(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.status_message = "Assessing agent readiness...".to_string();
    Ok(None)
}

fn cmd_security_review(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.status_message = "Starting security review...".to_string();
    Ok(None)
}

fn cmd_simplify(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.status_message = "Reviewing code for simplification...".to_string();
    Ok(None)
}

fn cmd_run(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let target = if args.trim().is_empty() {
        "the requested task"
    } else {
        args.trim()
    };
    ctx.app.status_message = format!("Running: {target}");
    Ok(None)
}

fn cmd_ask(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let target = if args.trim().is_empty() {
        "the current context"
    } else {
        args.trim()
    };
    ctx.app.status_message = format!("Asking: {target}");
    Ok(None)
}

fn cmd_plan(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let target = if args.trim().is_empty() {
        "the requested task"
    } else {
        args.trim()
    };
    ctx.app.status_message = format!("Planning: {target}");
    Ok(None)
}

fn cmd_search(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let query = args.trim();
    if query.is_empty() {
        return Err("Usage: /search <query>".to_string());
    }
    match crate::search::semantic::build_project_index(ctx.project_root) {
        Ok(index) => {
            let results = index.search(query, 10);
            if results.is_empty() {
                Ok(Some("No results found.".to_string()))
            } else {
                let mut out = format!("Search results for '{}':\n", query);
                for (i, (path, score)) in results.iter().enumerate() {
                    out.push_str(&format!("{}. {} (score: {:.3})\n", i + 1, path, score));
                }
                Ok(Some(out))
            }
        }
        Err(e) => Err(format!("Search failed: {e}")),
    }
}

fn cmd_status(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let git_status = crate::workspace::git::git_status(ctx.project_root).unwrap_or_default();
    let mut parts = vec![format!("Model: {:?}", ctx.app.model)];
    parts.push(format!(
        "YOLO: {}",
        if *ctx.yolo_mode { "ON" } else { "OFF" }
    ));
    parts.push(format!("Messages: {}", ctx.app.messages.len()));
    parts.push(format!(
        "Session: {}",
        ctx.app.session_name.as_deref().unwrap_or("default")
    ));
    if !ctx.mcp_status.is_empty() && ctx.mcp_status != "MCP: not initialized" {
        parts.push("MCP: configured".to_string());
    }
    if !git_status.is_empty() {
        parts.push(format!("\nGit status:\n{git_status}"));
    } else {
        parts.push("\nGit: clean".to_string());
    }
    Ok(Some(parts.join(" | ")))
}

fn cmd_context(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    const CONTEXT_LIMIT_TOKENS: u64 = 200_000;
    let total = ctx.app.total_tokens;
    let percent = if CONTEXT_LIMIT_TOKENS == 0 {
        0.0
    } else {
        (total as f64 / CONTEXT_LIMIT_TOKENS as f64) * 100.0
    };
    let cache_hit = ctx
        .app
        .cache
        .as_ref()
        .map_or(0.0, |cache| cache.hit_rate() * 100.0);
    Ok(Some(
        [
            manager_header("context", "ready"),
            format!("window      {total}/{CONTEXT_LIMIT_TOKENS} tokens ({percent:.1}%)"),
            format!("current     {} tokens", ctx.app.current_turn_tokens),
            format!("messages    {}", ctx.app.messages.len()),
            format!("cache       {cache_hit:.0}%"),
            format!("cost        ¥{:.3}", ctx.app.total_cost),
            "next        use /compact to keep only recent messages".to_string(),
        ]
        .join("\n"),
    ))
}

fn cmd_cwd(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let requested = args.trim();
    if requested.is_empty() {
        return Ok(Some(format!(
            "{}\npath      {}",
            manager_header("cwd", "ready"),
            ctx.project_root.display()
        )));
    }
    let candidate = ctx.project_root.join(requested);
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|e| format!("Cannot change cwd to {}: {e}", candidate.display()))?;
    if !canonical.is_dir() {
        return Err(format!("Not a directory: {}", canonical.display()));
    }
    ctx.app
        .push_activity(format!("cwd validated: {}", canonical.display()));
    Ok(Some(format!(
        "{}\nvalidated {}\n\nPersistent cwd switching will be wired into session state in a future turn.",
        manager_header("cwd", "validated"),
        canonical.display()
    )))
}

fn cmd_mcp(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    if ctx.mcp_status.is_empty() || ctx.mcp_status == "MCP: not initialized" {
        Ok(Some([
            manager_header("mcp", "empty"),
            "status    no servers configured".to_string(),
            "next      add [mcp.servers] to ~/.deepseek-code/config.toml or ./.deepseek-code/config.toml".to_string(),
        ].join("\n")))
    } else {
        Ok(Some(format!(
            "{}\n{}",
            manager_header("mcp", "ready"),
            ctx.mcp_status
        )))
    }
}

fn cmd_usage(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let mut lines = vec!["Session Usage".to_string(), "".to_string()];
    lines.push(format!("Model: {:?}", ctx.app.model));
    lines.push(format!("Total tokens: {}", ctx.app.total_tokens));
    lines.push(format!("Estimated cost: ¥{:.3}", ctx.app.total_cost));
    lines.push("".to_string());
    lines.push("Pricing (DeepSeek API, CNY / MTok):".to_string());
    lines.push("  Flash: input 0.5 (cache hit 0.1), output 2.0".to_string());
    lines.push("  Pro:   input 2.0 (cache hit 0.5), output 8.0".to_string());
    Ok(Some(lines.join("\n")))
}

fn cmd_doctor(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root));
    let api_key = crate::storage::get_effective_api_key(Some(ctx.project_root));
    let git_available = command_available("git");
    let cargo_available = command_available("cargo");
    let session_store = dirs::home_dir()
        .map(|home| home.join(".deepseek-code").join("sessions"))
        .filter(|path| path.exists());

    let mut lines = vec![
        "DeepSeek-Code Doctor".to_string(),
        String::new(),
        doctor_line(
            api_key.is_some(),
            "API key",
            if api_key.is_some() {
                "configured"
            } else {
                "missing; run `deepseek-code login --api-key sk-...` or set DEEPSEEK_API_KEY"
            },
        ),
        doctor_line(
            config.is_ok(),
            "Config",
            if config.is_ok() {
                "loaded"
            } else {
                "using defaults; config file could not be loaded"
            },
        ),
        doctor_line(
            ctx.project_root.exists(),
            "Workspace",
            ctx.project_root.display().to_string(),
        ),
        doctor_line(git_available, "git", tool_status(git_available)),
        doctor_line(cargo_available, "cargo", tool_status(cargo_available)),
    ];

    if let Ok(config) = config {
        lines.push(format!(
            "Policy: auto_mode={} safe_reads={} network={} command_timeout={}s",
            on_off(config.policy.auto_mode),
            on_off(config.policy.auto_approve_safe_read),
            on_off(config.policy.network_access),
            config.policy.command_timeout_seconds
        ));
        lines.push(format!(
            "MCP: enabled={} configured_servers={}",
            on_off(config.mcp.enabled),
            config.mcp.servers.len()
        ));
    }

    if !ctx.mcp_status.is_empty() {
        lines.push(format!("TUI MCP: {}", ctx.mcp_status));
    }

    lines.push(format!(
        "Session store: {}",
        session_store
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not created yet".to_string())
    ));
    lines.push(String::new());
    lines.push(
        "Doctor is local-only in the TUI; run `deepseek-code doctor` for API connectivity checks."
            .to_string(),
    );

    Ok(Some(lines.join("\n")))
}

fn command_available(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn doctor_line(ok: bool, label: &str, detail: impl AsRef<str>) -> String {
    let status = if ok { "ok" } else { "missing" };
    format!("{label:<10} {status:<8} {}", detail.as_ref())
}

fn tool_status(available: bool) -> &'static str {
    if available {
        "available"
    } else {
        "not found on PATH"
    }
}

fn cmd_checkpoint(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let label = if args.trim().is_empty() {
        "manual"
    } else {
        args.trim()
    };
    let history = crate::workspace::apply::get_history_by_turn();
    if history.is_empty() {
        return Ok(Some("No changes to checkpoint.".to_string()));
    }
    let mut lines = vec![format!("Checkpoint: {label}"), "".to_string()];
    for (turn_id, files) in history.iter().rev().take(5) {
        lines.push(format!("turn {turn_id} — {} file(s)", files.len()));
    }
    ctx.app.push_activity(format!("checkpoint: {label}"));
    Ok(Some(lines.join("\n")))
}

fn cmd_restore(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let n: usize = args.trim().parse().unwrap_or(1);
    match crate::workspace::apply::undo_n_changes(ctx.project_root, n) {
        Ok(restored) => {
            if restored.is_empty() {
                Ok(Some("No changes to restore.".to_string()))
            } else {
                ctx.app
                    .push_activity(format!("restored {} files", restored.len()));
                Ok(Some(format!(
                    "Restored {} files:\n{}",
                    restored.len(),
                    restored.join("\n")
                )))
            }
        }
        Err(e) => Err(format!("Restore failed: {e}")),
    }
}

fn cmd_auto(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    let enabled = config.policy.auto_mode;
    let msg = if enabled {
        "Auto mode ON — safe operations auto-approved"
    } else {
        "Auto mode OFF — all operations require manual approval"
    };
    Ok(Some(msg.to_string()))
}

fn cmd_theme(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let requested = args.trim().to_ascii_lowercase();
    let mode = match requested.as_str() {
        "" => {
            return Ok(Some(format!(
                "Theme: {}\n\nUsage: /theme light | dark | toggle",
                ctx.app.theme_mode.label()
            )));
        }
        "light" | "droid" => crate::tui::theme::ThemeMode::Light,
        "dark" | "terminal" => crate::tui::theme::ThemeMode::Dark,
        "toggle" => ctx.app.theme_mode.toggled(),
        other => {
            return Err(format!(
                "Unknown theme: {other}. Use /theme light, /theme dark, or /theme toggle."
            ));
        }
    };
    ctx.app.set_theme_mode(mode);
    Ok(Some(format!("Theme set to {}", mode.label())))
}

fn cmd_settings(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.open_settings_panel();
    Ok(None)
}

fn cmd_mode(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let requested = args.trim().to_ascii_lowercase();
    let mode = match requested.as_str() {
        "" => {
            ctx.app.pending_options = Some((
                "Choose interaction mode".to_string(),
                vec![
                    "/mode ask".to_string(),
                    "/mode plan".to_string(),
                    "/mode review".to_string(),
                    "/mode full".to_string(),
                ],
            ));
            ctx.app.selected_option_index = 0;
            return Ok(Some(format!(
                "Mode: {}\n\nSelect a mode, or type /mode ask | plan | review | full",
                ctx.app.interaction_mode.label()
            )));
        }
        "ask" | "default" | "permissions" => crate::tui::app::InteractionMode::Ask,
        "plan" => crate::tui::app::InteractionMode::Plan,
        "review" | "auto-review" | "auto_review" => crate::tui::app::InteractionMode::AutoReview,
        "full" | "full-access" | "full_access" | "yolo" => {
            crate::tui::app::InteractionMode::FullAccess
        }
        other => {
            return Err(format!(
                "Unknown mode: {other}. Use /mode ask, /mode plan, /mode review, or /mode full."
            ));
        }
    };
    ctx.app.set_interaction_mode(mode);
    Ok(Some(format!("Mode set to {}", mode.label())))
}

fn cmd_permissions(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    let policy = config.policy;
    let lines = [
        manager_header("permissions", "ready"),
        format!("auto mode: {}", on_off(policy.auto_mode)),
        format!(
            "safe reads auto-approved: {}",
            on_off(policy.auto_approve_safe_read)
        ),
        format!("network access: {}", on_off(policy.network_access)),
        format!(
            "write approval required: {}",
            on_off(policy.require_approval_for_write)
        ),
        format!(
            "command approval required: {}",
            on_off(policy.require_approval_for_command)
        ),
        format!(
            "protected paths blocked: {}",
            on_off(policy.block_protected_paths)
        ),
        format!("command timeout: {}s", policy.command_timeout_seconds),
    ];
    Ok(Some(lines.join("\n")))
}

fn cmd_model(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let requested = args.trim();
    if requested.is_empty() {
        ctx.app.pending_options = Some((
            "Choose model".to_string(),
            vec!["/model flash".to_string(), "/model pro".to_string()],
        ));
        ctx.app.selected_option_index = 0;
        return Ok(Some(format!(
            "Current model: {:?}\n\nSelect a model, or type /model flash | /model pro",
            ctx.app.model
        )));
    }

    let model = match requested {
        "pro" | "v4-pro" => crate::deepseek::DeepSeekModel::Pro,
        "flash" | "v4-flash" => crate::deepseek::DeepSeekModel::Flash,
        other => crate::deepseek::migration::migrate_model_name(other)
            .ok_or_else(|| format!("Unknown model: {other}"))?,
    };
    ctx.app.model = model.clone();
    ctx.app.welcome = crate::tui::welcome::WelcomeDashboardData::load(
        ctx.project_root,
        ctx.app.model.clone(),
        ctx.app.thinking_mode.clone(),
    );
    Ok(Some(format!("Model set to {:?}", model)))
}

fn cmd_config(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    let project_config = ctx.project_root.join(".deepseek-code").join("config.toml");
    let user_config = dirs::home_dir()
        .map(|home| home.join(".deepseek-code").join("config.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.deepseek-code/config.toml"));
    let lines = [
        "Config".to_string(),
        "".to_string(),
        format!("project: {}", project_config.display()),
        format!("user: {}", user_config.display()),
        format!("default model: {:?}", config.model.default),
        format!("heavy model: {:?}", config.model.heavy),
        format!("MCP enabled: {}", on_off(config.mcp.enabled)),
        format!("subagents enabled: {}", on_off(config.subagent.enabled)),
    ];
    Ok(Some(lines.join("\n")))
}

fn cmd_memory(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let candidates = [
        ctx.project_root.join("AGENTS.md"),
        ctx.project_root.join(".deepseek-code").join("AGENTS.md"),
        ctx.project_root.join(".deepseek-code").join("rules.md"),
    ];
    let mut lines = vec!["Memory files".to_string(), "".to_string()];
    for path in candidates {
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            lines.push(format!("{} — {} bytes", path.display(), content.len()));
        } else {
            lines.push(format!("{} — missing", path.display()));
        }
    }
    lines.push(String::new());
    lines.push(
        "Non-empty files are injected into the system prompt in the order shown above.".to_string(),
    );
    Ok(Some(lines.join("\n")))
}

fn cmd_sessions(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let Some(home) = dirs::home_dir() else {
        return Err("Cannot find home directory.".to_string());
    };
    let store = crate::storage::SessionStore::new(home.join(".deepseek-code"));
    let sessions = store
        .list(ctx.project_root)
        .map_err(|e| format!("Failed to list sessions: {e}"))?;
    if sessions.is_empty() {
        return Ok(Some(format!(
            "{}\nstatus    no saved sessions for this project",
            manager_header("sessions", "empty")
        )));
    }
    let mut lines = vec![
        manager_header("sessions", "ready"),
        format!("count     {}", sessions.len()),
    ];
    for session in sessions.iter().take(8) {
        lines.push(format!(
            "{}  {}  {} msgs  {} tools  {}",
            session.id.to_string().chars().take(8).collect::<String>(),
            session.name.as_deref().unwrap_or("unnamed"),
            session.message_count,
            session.tool_call_count,
            session.updated_at.format("%m-%d %H:%M")
        ));
    }
    lines.push(String::new());
    lines.push("next      use `ds resume <id-prefix>` outside TUI to resume".to_string());
    Ok(Some(lines.join("\n")))
}

fn cmd_init(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let path = ctx.project_root.join("AGENTS.md");
    if path.exists() {
        return Ok(Some(format!(
            "{}\npath      {}\nstatus    already exists",
            manager_header("init", "ready"),
            path.display()
        )));
    }
    let content = r#"# Repository Guidelines

## Project
- Keep changes small and focused.
- Prefer existing modules and local conventions.
- Run formatting, tests, and lint checks before publishing changes.

## Agent Workflow
- Inspect the code before editing.
- Use tools for local file questions instead of guessing.
- Ask before risky writes outside the workspace.
"#;
    std::fs::write(&path, content).map_err(|e| format!("Failed to write AGENTS.md: {e}"))?;
    Ok(Some(format!(
        "{}\ncreated   {}\nnext      edit AGENTS.md with project-specific rules",
        manager_header("init", "done"),
        path.display()
    )))
}

fn cmd_compact(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let keep = args.trim().parse::<usize>().unwrap_or(20).max(4);
    let before = ctx.app.messages.len();
    if before <= keep {
        return Ok(Some(format!(
            "No compaction needed; {before} messages in context."
        )));
    }
    let split_at = before - keep;
    ctx.app.messages.drain(0..split_at);
    ctx.app
        .push_activity(format!("compact: kept last {keep} messages"));
    Ok(Some(format!(
        "Compacted local transcript: removed {split_at} older messages, kept {keep}."
    )))
}

fn cmd_add_dir(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let path = args.trim();
    if path.is_empty() {
        return Err("Usage: /add-dir <path>".to_string());
    }
    let candidate = ctx.project_root.join(path);
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|e| format!("Cannot add directory {}: {e}", candidate.display()))?;
    if !canonical.is_dir() {
        return Err(format!("Not a directory: {}", canonical.display()));
    }
    ctx.app
        .push_activity(format!("add-dir requested: {}", canonical.display()));
    Ok(Some(format!(
        "Additional directory validated: {}\nPersistent multi-root access will use this path in a future turn.",
        canonical.display()
    )))
}

fn cmd_commands(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let project_dir = ctx.project_root.join(".deepseek-code").join("commands");
    let user_dir = dirs::home_dir()
        .map(|home| home.join(".deepseek-code").join("commands"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.deepseek-code/commands"));
    let project_count = count_entries(&project_dir);
    let user_count = count_entries(&user_dir);
    Ok(Some(
        [
            manager_header("commands", "ready"),
            format!(
                "built-in   {}",
                CommandRegistry::new().list_commands().len()
            ),
            format!("project    {} ({})", project_count, project_dir.display()),
            format!("user       {} ({})", user_count, user_dir.display()),
            "next       add prompt command files in a future custom-command pass".to_string(),
        ]
        .join("\n"),
    ))
}

fn cmd_skills(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let project_dir = ctx.project_root.join(".deepseek-code").join("skills");
    let user_dir = dirs::home_dir()
        .map(|home| home.join(".deepseek-code").join("skills"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.deepseek-code/skills"));
    let project = list_entry_names(&project_dir);
    let user = list_entry_names(&user_dir);
    let mut lines = vec![
        manager_header("skills", "ready"),
        format!("project    {} ({})", project.len(), project_dir.display()),
        format!("user       {} ({})", user.len(), user_dir.display()),
    ];
    if !project.is_empty() {
        lines.push(format!("project skills: {}", project.join(", ")));
    }
    if !user.is_empty() {
        lines.push(format!("user skills: {}", user.join(", ")));
    }
    lines.push("next       .deepseek-code/skills/<name>/SKILL.md".to_string());
    Ok(Some(lines.join("\n")))
}

fn cmd_hooks(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    Ok(Some(
        [
            manager_header("hooks", "ready"),
            format!("pre_tool   {}", config.hooks.pre_tool.len()),
            format!("post_tool  {}", config.hooks.post_tool.len()),
            format!("stop       {}", config.hooks.stop.len()),
            "events     PreToolUse, PostToolUse, Stop".to_string(),
        ]
        .join("\n"),
    ))
}

fn cmd_plugins(_args: &str, _ctx: &mut CommandContext) -> CommandResult {
    Ok(Some(
        [
            manager_header("plugins", "planned"),
            "status    local plugin manager is not enabled yet".to_string(),
            "next      use MCP, skills, and hooks for extension points today".to_string(),
        ]
        .join("\n"),
    ))
}

fn cmd_statusline(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    Ok(Some(
        [
            manager_header("statusline", "ready"),
            format!("mode      {}", ctx.app.interaction_mode.label()),
            format!("theme     {}", ctx.app.theme_mode.label()),
            "style     compact chips".to_string(),
            "segments  app, mode, web, context, tokens, cost, cache, tools, permissions"
                .to_string(),
        ]
        .join("\n"),
    ))
}

fn count_entries(path: &std::path::Path) -> usize {
    list_entry_names(path).len()
}

fn list_entry_names(path: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

fn cmd_agents(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let registry = crate::agent::subagent::SubagentRegistry::load_from_project(ctx.project_root);
    let mut lines = vec![manager_header("agents", "ready")];
    for name in registry.list() {
        if let Some(agent) = registry.get(name) {
            lines.push(format!(
                "{} — tools: {} — max turns: {}",
                name,
                if agent.allowed_tools.is_empty() {
                    "default".to_string()
                } else {
                    agent.allowed_tools.join(", ")
                },
                agent.max_turns
            ));
        }
    }
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    lines.push(String::new());
    lines.push(format!(
        "custom agents: {}",
        on_off(config.subagent.allow_custom_agents)
    ));
    if let Some(dir) = config.subagent.custom_agents_dir {
        lines.push(format!("custom dir: {}", dir.display()));
    }
    Ok(Some(lines.join("\n")))
}

fn cmd_tasks(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    if ctx.background_tasks.is_empty() {
        return Ok(Some(format!(
            "{}\nstatus    no background tasks",
            manager_header("tasks", "empty")
        )));
    }

    let mut lines = vec![
        manager_header("tasks", "running"),
        format!("count     {}", ctx.background_tasks.len()),
    ];
    for task in ctx.background_tasks {
        lines.push(task.format_for_display());
        lines.push(String::new());
    }
    Ok(Some(lines.join("\n").trim_end().to_string()))
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn manager_header(name: &str, status: &str) -> String {
    format!("◆ manager {name}  status:{status}")
}

fn cmd_help(_args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let registry = CommandRegistry::new();
    let mut lines = vec!["Available commands:".to_string(), "".to_string()];
    for cmd in registry.list_commands() {
        lines.push(format!("  {} — {}", cmd.name, cmd.description));
        if !cmd.aliases.is_empty() {
            lines.push(format!("    aliases: {}", cmd.aliases.join(", ")));
        }
        lines.push(format!("    usage: {}", cmd.usage));
        lines.push(String::new());
    }
    Ok(Some(lines.join("\n")))
}

impl CommandRegistry {
    fn register_all(&mut self) {
        self.register(&SlashCommand {
            name: "/yolo",
            aliases: &[],
            description: "Toggle auto-approve (YOLO) mode",
            usage: "/yolo",
            handler: cmd_yolo,
        });
        self.register(&SlashCommand {
            name: "/clear",
            aliases: &["/new"],
            description: "Reset conversation and clear screen",
            usage: "/clear or /new",
            handler: cmd_clear,
        });
        self.register(&SlashCommand {
            name: "/copy",
            aliases: &[],
            description: "Copy the last assistant message to clipboard",
            usage: "/copy",
            handler: cmd_copy,
        });
        self.register(&SlashCommand {
            name: "/undo",
            aliases: &[],
            description: "Revert the last file edit",
            usage: "/undo",
            handler: cmd_undo,
        });
        self.register(&SlashCommand {
            name: "/image",
            aliases: &[],
            description: "Attach an image to the next message",
            usage: "/image <path>",
            handler: cmd_image,
        });
        self.register(&SlashCommand {
            name: "/commit",
            aliases: &[],
            description: "Stage and commit all changes",
            usage: "/commit [message]",
            handler: cmd_commit,
        });
        self.register(&SlashCommand {
            name: "/test",
            aliases: &[],
            description: "Run project tests",
            usage: "/test [command or framework]",
            handler: cmd_test,
        });
        self.register(&SlashCommand {
            name: "/fix",
            aliases: &[],
            description: "Ask the agent to fix an issue",
            usage: "/fix [description]",
            handler: cmd_fix,
        });
        self.register(&SlashCommand {
            name: "/explain",
            aliases: &[],
            description: "Ask the agent to explain code",
            usage: "/explain [target]",
            handler: cmd_explain,
        });
        self.register(&SlashCommand {
            name: "/review",
            aliases: &[],
            description: "Start a code review",
            usage: "/review",
            handler: cmd_review,
        });
        self.register(&SlashCommand {
            name: "/security-review",
            aliases: &[],
            description: "Start a security-focused code review",
            usage: "/security-review [scope]",
            handler: cmd_security_review,
        });
        self.register(&SlashCommand {
            name: "/simplify",
            aliases: &[],
            description: "Review changed code for reuse, quality, and efficiency",
            usage: "/simplify [scope]",
            handler: cmd_simplify,
        });
        self.register(&SlashCommand {
            name: "/wiki",
            aliases: &[],
            description: "Generate codebase documentation",
            usage: "/wiki [scope]",
            handler: cmd_wiki,
        });
        self.register(&SlashCommand {
            name: "/readiness-report",
            aliases: &[],
            description: "Assess repository readiness for agent workflows",
            usage: "/readiness-report",
            handler: cmd_readiness_report,
        });
        self.register(&SlashCommand {
            name: "/run",
            aliases: &[],
            description: "Run a task through the agent",
            usage: "/run <task>",
            handler: cmd_run,
        });
        self.register(&SlashCommand {
            name: "/ask",
            aliases: &[],
            description: "Ask a read-only question",
            usage: "/ask <question>",
            handler: cmd_ask,
        });
        self.register(&SlashCommand {
            name: "/plan",
            aliases: &[],
            description: "Plan before executing",
            usage: "/plan <task>",
            handler: cmd_plan,
        });
        self.register(&SlashCommand {
            name: "/search",
            aliases: &[],
            description: "Semantic search the codebase",
            usage: "/search <query>",
            handler: cmd_search,
        });
        self.register(&SlashCommand {
            name: "/status",
            aliases: &[],
            description: "Show current session status",
            usage: "/status",
            handler: cmd_status,
        });
        self.register(&SlashCommand {
            name: "/context",
            aliases: &["/limits"],
            description: "Show context window, tokens, cache, and cost",
            usage: "/context",
            handler: cmd_context,
        });
        self.register(&SlashCommand {
            name: "/cwd",
            aliases: &[],
            description: "Show or validate the working directory",
            usage: "/cwd [path]",
            handler: cmd_cwd,
        });
        self.register(&SlashCommand {
            name: "/mcp",
            aliases: &[],
            description: "Show MCP server status",
            usage: "/mcp",
            handler: cmd_mcp,
        });
        self.register(&SlashCommand {
            name: "/usage",
            aliases: &["/cost", "/stats"],
            description: "Show session token usage and cost",
            usage: "/usage or /cost or /stats",
            handler: cmd_usage,
        });
        self.register(&SlashCommand {
            name: "/doctor",
            aliases: &["/diagnostics"],
            description: "Run local environment diagnostics",
            usage: "/doctor or /diagnostics",
            handler: cmd_doctor,
        });
        self.register(&SlashCommand {
            name: "/checkpoint",
            aliases: &["/cp"],
            description: "Show recent checkpoints (edit history)",
            usage: "/checkpoint [label]",
            handler: cmd_checkpoint,
        });
        self.register(&SlashCommand {
            name: "/restore",
            aliases: &["/rs"],
            description: "Restore the last N file changes",
            usage: "/restore [n]",
            handler: cmd_restore,
        });
        self.register(&SlashCommand {
            name: "/auto",
            aliases: &[],
            description: "Show auto-approval mode status",
            usage: "/auto",
            handler: cmd_auto,
        });
        self.register(&SlashCommand {
            name: "/permissions",
            aliases: &["/perm", "/permission"],
            description: "Show current approval and sandbox policy",
            usage: "/permissions",
            handler: cmd_permissions,
        });
        self.register(&SlashCommand {
            name: "/theme",
            aliases: &["/themes"],
            description: "Show or switch the UI theme",
            usage: "/theme [light|dark|toggle]",
            handler: cmd_theme,
        });
        self.register(&SlashCommand {
            name: "/settings",
            aliases: &["/set"],
            description: "Open the read-only settings panel",
            usage: "/settings",
            handler: cmd_settings,
        });
        self.register(&SlashCommand {
            name: "/mode",
            aliases: &[],
            description: "Switch interaction mode",
            usage: "/mode [ask|plan|review|full]",
            handler: cmd_mode,
        });
        self.register(&SlashCommand {
            name: "/model",
            aliases: &[],
            description: "Show or switch the active model",
            usage: "/model [flash|pro]",
            handler: cmd_model,
        });
        self.register(&SlashCommand {
            name: "/config",
            aliases: &[],
            description: "Show resolved configuration summary",
            usage: "/config",
            handler: cmd_config,
        });
        self.register(&SlashCommand {
            name: "/memory",
            aliases: &[],
            description: "Show project memory files",
            usage: "/memory",
            handler: cmd_memory,
        });
        self.register(&SlashCommand {
            name: "/sessions",
            aliases: &["/session-navigation"],
            description: "List saved sessions for this project",
            usage: "/sessions",
            handler: cmd_sessions,
        });
        self.register(&SlashCommand {
            name: "/init",
            aliases: &[],
            description: "Create AGENTS.md contributor guidelines",
            usage: "/init",
            handler: cmd_init,
        });
        self.register(&SlashCommand {
            name: "/compact",
            aliases: &["/compress", "/handoff"],
            description: "Compact the local transcript to recent messages",
            usage: "/compact [messages-to-keep]",
            handler: cmd_compact,
        });
        self.register(&SlashCommand {
            name: "/add-dir",
            aliases: &[],
            description: "Validate an additional working directory",
            usage: "/add-dir <path>",
            handler: cmd_add_dir,
        });
        self.register(&SlashCommand {
            name: "/agents",
            aliases: &["/droids"],
            description: "List built-in and configured subagents",
            usage: "/agents",
            handler: cmd_agents,
        });
        self.register(&SlashCommand {
            name: "/tasks",
            aliases: &["/missions", "/ms"],
            description: "List background subagent tasks",
            usage: "/tasks",
            handler: cmd_tasks,
        });
        self.register(&SlashCommand {
            name: "/commands",
            aliases: &[],
            description: "Show custom command locations and built-in count",
            usage: "/commands",
            handler: cmd_commands,
        });
        self.register(&SlashCommand {
            name: "/skills",
            aliases: &[],
            description: "List project and user skills",
            usage: "/skills",
            handler: cmd_skills,
        });
        self.register(&SlashCommand {
            name: "/hooks",
            aliases: &[],
            description: "Show configured tool hooks",
            usage: "/hooks",
            handler: cmd_hooks,
        });
        self.register(&SlashCommand {
            name: "/plugins",
            aliases: &[],
            description: "Show plugin extension status",
            usage: "/plugins",
            handler: cmd_plugins,
        });
        self.register(&SlashCommand {
            name: "/statusline",
            aliases: &[],
            description: "Show statusline configuration summary",
            usage: "/statusline",
            handler: cmd_statusline,
        });
        self.register(&SlashCommand {
            name: "/help",
            aliases: &["/h"],
            description: "Show available commands",
            usage: "/help",
            handler: cmd_help,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_commands() {
        let reg = CommandRegistry::new();
        assert!(!reg.list_commands().is_empty());
    }

    #[test]
    fn test_help_command() {
        let result = execute_with_test_app("/help");
        assert!(result.is_ok());
    }

    #[test]
    fn test_unknown_command() {
        let result = execute_with_test_app("/unknown");
        assert!(result.is_err());
    }

    #[test]
    fn agent_slash_commands_are_forwarded() {
        assert!(matches!(
            execute_with_test_app("/run fix the bug"),
            Ok(None)
        ));
        assert!(matches!(
            execute_with_test_app("/ask where is main"),
            Ok(None)
        ));
        assert!(matches!(
            execute_with_test_app("/plan improve startup"),
            Ok(None)
        ));
    }

    #[test]
    fn mode_command_reports_current_mode() {
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };
        let output = reg
            .execute("/mode", &mut ctx)
            .expect("command should be handled")
            .expect("mode should run")
            .expect("mode should show output");

        assert!(output.contains("Mode: Ask"));
        assert!(output.contains("/mode ask | plan | review | full"));
        assert!(ctx.app.pending_options.is_some());
    }

    #[test]
    fn model_command_opens_model_picker() {
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/model", &mut ctx)
            .expect("command should be handled")
            .expect("model should run")
            .expect("model should show output");

        assert!(output.contains("Current model"));
        let (_, options) = ctx.app.pending_options.as_ref().expect("model picker");
        assert_eq!(
            options,
            &vec!["/model flash".to_string(), "/model pro".to_string()]
        );
    }

    #[test]
    fn mode_command_switches_interaction_mode() {
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/mode full", &mut ctx)
            .expect("command should be handled")
            .expect("mode should run")
            .expect("mode should show output");

        assert!(output.contains("Mode set to Full access"));
        assert_eq!(
            ctx.app.interaction_mode,
            crate::tui::app::InteractionMode::FullAccess
        );
        assert!(ctx.app.session_auto_approve);
    }

    #[test]
    fn selected_droid_style_commands_are_registered() {
        let reg = CommandRegistry::new();
        let names: std::collections::HashSet<_> =
            reg.list_commands().iter().map(|cmd| cmd.name).collect();

        for expected in [
            "/commands",
            "/context",
            "/copy",
            "/cwd",
            "/hooks",
            "/init",
            "/plugins",
            "/readiness-report",
            "/security-review",
            "/sessions",
            "/simplify",
            "/skills",
            "/statusline",
            "/wiki",
        ] {
            assert!(names.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn droid_style_aliases_dispatch_to_local_commands() {
        let compact = execute_with_test_app("/compress")
            .expect("compress alias should run")
            .expect("compress alias should show output");
        assert!(compact.contains("No compaction needed"));

        let agents = execute_with_test_app("/droids")
            .expect("droids alias should run")
            .expect("droids alias should show output");
        assert!(agents.contains("manager agents"));

        let tasks = execute_with_test_app("/missions")
            .expect("missions alias should run")
            .expect("missions alias should show output");
        assert!(tasks.contains("manager tasks"));

        let diagnostics = execute_with_test_app("/diagnostics")
            .expect("diagnostics alias should run")
            .expect("diagnostics alias should show output");
        assert!(diagnostics.contains("DeepSeek-Code Doctor"));
    }

    #[test]
    fn context_command_reports_token_window() {
        let output = execute_with_test_app("/context")
            .expect("context should run")
            .expect("context should show output");

        assert!(output.contains("manager context"));
        assert!(output.contains("window"));
        assert!(output.contains("current"));
    }

    #[test]
    fn sessions_command_handles_empty_project_history() {
        let output = execute_with_test_app("/sessions")
            .expect("sessions should run")
            .expect("sessions should show output");

        assert!(output.contains("manager sessions"));
    }

    #[test]
    fn tasks_command_reports_empty_queue() {
        let output = execute_with_test_app("/tasks")
            .expect("tasks should run")
            .expect("tasks should show output");

        assert!(output.contains("manager tasks"));
        assert!(output.contains("status:empty"));
        assert!(output.contains("no background tasks"));
    }

    #[test]
    fn tasks_command_formats_background_snapshots() {
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        let tasks = vec![crate::agent::BackgroundTaskSnapshot {
            task_id: "bg-test".to_string(),
            description: "Review plan UI".to_string(),
            status: crate::agent::TaskStatus::Running,
            success: None,
            summary: Some("checking panels".to_string()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            duration_ms: None,
        }];
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &tasks,
        };

        let output = reg
            .execute("/tasks", &mut ctx)
            .expect("command should be handled")
            .expect("tasks should run")
            .expect("tasks should show output");

        assert!(output.contains("manager tasks"));
        assert!(output.contains("status:running"));
        assert!(output.contains("count     1"));
        assert!(output.contains("bg-test"));
        assert!(output.contains("Review plan UI"));
        assert!(output.contains("checking panels"));
    }

    #[test]
    fn cost_alias_shows_usage() {
        let output = execute_with_test_app("/cost")
            .expect("cost should run")
            .expect("cost should show output");

        assert!(output.contains("Session Usage"));
        assert!(output.contains("Estimated cost"));
    }

    #[test]
    fn doctor_command_is_local_only() {
        let output = execute_with_test_app("/doctor")
            .expect("doctor should run")
            .expect("doctor should show output");

        assert!(output.contains("DeepSeek-Code Doctor"));
        assert!(output.contains("Doctor is local-only in the TUI"));
    }

    #[test]
    fn theme_command_switches_current_app_theme() {
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/theme dark", &mut ctx)
            .expect("command should be handled")
            .expect("theme should run")
            .expect("theme should show output");

        assert!(output.contains("dark"));
        assert_eq!(ctx.app.theme_mode, crate::tui::theme::ThemeMode::Dark);
    }

    #[test]
    fn settings_command_opens_read_only_panel() {
        let result = execute_with_test_app("/settings").expect("settings should run");

        assert!(result.is_none());

        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/set", &mut ctx)
            .expect("command should be handled")
            .expect("settings should run");

        assert!(output.is_none());
        assert!(ctx.app.settings_open);
        assert!(ctx.app.status_message.contains("read-only"));
    }

    #[test]
    fn test_command_uses_platform_shell() {
        let output = execute_with_test_app("/test echo deepseek-code-test")
            .expect("test command should run")
            .expect("test command should show output");

        assert!(output.contains("deepseek-code-test"));
    }

    #[test]
    fn test_command_blocks_dangerous_patterns() {
        let result = execute_with_test_app("/test rm -rf /");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("dangerous command blocked"));
    }

    fn execute_with_test_app(input: &str) -> CommandResult {
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };
        reg.execute(input, &mut ctx)
            .expect("command should be handled")
    }
}

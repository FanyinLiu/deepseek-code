//! Slash command system for the TUI.
use std::collections::HashMap;
use std::io::Write;
use unicode_width::UnicodeWidthStr;

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

#[must_use]
pub fn localized_command_description(
    name: &str,
    fallback: &'static str,
    language: &str,
) -> &'static str {
    if !crate::tui::welcome::is_chinese_display_language(language) {
        return fallback;
    }
    match name {
        "/yolo" => "切换自动批准模式",
        "/clear" => "清空对话并重置屏幕",
        "/exit" => "退出交互式 TUI",
        "/copy" => "复制上一条助手回复",
        "/undo" => "撤销上一次文件修改",
        "/image" => "给下一条消息附加图片",
        "/commit" => "暂存并提交全部更改",
        "/test" => "运行项目测试",
        "/fix" => "让智能体修复问题",
        "/explain" => "解释代码",
        "/review" => "开始代码审查",
        "/security-review" => "开始安全审查",
        "/simplify" => "审查变更的复用、质量和效率",
        "/wiki" => "生成代码库文档",
        "/readiness-report" => "评估仓库的智能体工作流就绪度",
        "/run" => "让智能体执行任务",
        "/ask" => "提只读问题",
        "/plan" => "先规划再执行",
        "/todo" => "查看和维护本次工作待办",
        "/task" => "管理本地可恢复任务",
        "/glob" => "按模式查找文件",
        "/grep" => "搜索文件内容",
        "/search" => "语义搜索代码库",
        "/status" => "查看当前会话状态",
        "/context" => "查看上下文、token、缓存和费用",
        "/cwd" => "查看或验证工作目录",
        "/mcp" => "查看 MCP 服务状态",
        "/usage" => "查看会话 token 与费用",
        "/doctor" => "运行本地环境诊断",
        "/checkpoint" => "查看最近检查点",
        "/restore" => "恢复最近 N 次文件更改",
        "/auto" => "查看自动批准模式",
        "/permissions" => "查看审批和沙箱策略",
        "/theme" => "查看或切换界面主题",
        "/tui" => "查看或切换终端渲染器",
        "/settings" => "打开可编辑设置",
        "/output-style" => "查看或切换输出风格",
        "/keybindings" => "查看或写入快捷键配置",
        "/language" => "查看或切换界面语言",
        "/mode" => "切换交互模式",
        "/model" => "查看或切换当前模型",
        "/config" => "查看解析后的配置摘要",
        "/memory" => "查看项目记忆文件",
        "/sessions" => "列出项目会话",
        "/init" => "创建 AGENTS.md 协作说明",
        "/compact" => "压缩本地对话记录",
        "/add-dir" => "验证额外工作目录",
        "/agents" => "列出内置和已配置智能体",
        "/swarm" => "控制自动本地智能体群组",
        "/schedule" => "管理本地计划任务",
        "/tasks" => "列出后台智能体任务",
        "/commands" => "查看自定义命令目录和内置数量",
        "/skills" => "列出项目和用户技能",
        "/hooks" => "查看工具 hook 配置",
        "/plugins" => "查看插件扩展状态",
        "/statusline" => "查看状态栏配置摘要",
        "/help" => "查看所有命令",
        _ => fallback,
    }
}

#[must_use]
pub fn localized_command_usage(usage: &str, language: &str) -> String {
    if crate::tui::welcome::is_chinese_display_language(language) {
        usage.replace(" or ", " 或 ")
    } else {
        usage.to_string()
    }
}

/// A user-defined prompt-type slash command. Loaded from
/// `.octocode/commands/*.md` (project) and `~/.octocode/commands/*.md` (user).
/// When invoked, the body is rendered (with `$ARGUMENTS` substitution) and
/// queued as the next user input — exactly as if the user had typed the
/// rendered text into the prompt.
#[derive(Debug, Clone)]
pub struct PromptCommand {
    pub name: String,
    pub description: String,
    pub body: String,
    pub source_path: std::path::PathBuf,
}

impl PromptCommand {
    /// Render the body with `$ARGUMENTS` replaced by the user's args (possibly empty).
    #[must_use]
    pub fn render(&self, args: &str) -> String {
        self.body.replace("$ARGUMENTS", args.trim())
    }
}

/// Registry of all slash commands.
pub struct CommandRegistry {
    commands: HashMap<&'static str, &'static SlashCommand>,
    prompt_commands: HashMap<String, PromptCommand>,
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
            prompt_commands: HashMap::new(),
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

    /// Load user-defined prompt commands from project + user directories.
    /// Project commands take precedence over user commands on name collision.
    /// Built-in commands always win (we don't allow shadowing).
    /// Returns the number of prompt commands loaded.
    pub fn load_prompt_commands(&mut self, project_root: &std::path::Path) -> usize {
        self.prompt_commands.clear();
        let mut count = 0;
        // User-global first (lower precedence), then project (overrides on conflict).
        if let Some(home) = dirs::home_dir() {
            count += self.load_prompt_dir(&home.join(".octocode").join("commands"));
        }
        count += self.load_prompt_dir(&project_root.join(".octocode").join("commands"));
        count
    }

    fn load_prompt_dir(&mut self, dir: &std::path::Path) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // Skip names that collide with built-in commands.
            let slash_name = format!("/{name}");
            if self.commands.contains_key(slash_name.as_str()) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (description, body) = parse_prompt_command(&content);
            self.prompt_commands.insert(
                slash_name.clone(),
                PromptCommand {
                    name: slash_name,
                    description,
                    body,
                    source_path: path,
                },
            );
            count += 1;
        }
        count
    }

    /// List all loaded prompt commands (for /commands listing).
    #[must_use]
    pub fn prompt_commands(&self) -> Vec<&PromptCommand> {
        let mut v: Vec<&PromptCommand> = self.prompt_commands.values().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
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
            return Some((cmd.handler)(args, ctx));
        }

        if let Some(prompt) = self.prompt_commands.get(name) {
            // Render and queue as the next user input. We use the same queue
            // the user's own keystrokes flow through, so the prompt rides the
            // normal turn pipeline (history, hooks, policy, …).
            let rendered = prompt.render(args);
            ctx.app.queued_inputs.push_back(rendered);
            let label = prompt
                .source_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("command");
            return Some(Ok(Some(format!("→ {} ({})", prompt.name, label))));
        }

        Some(Err(format!(
            "Unknown command: {name}. Type /help for available commands."
        )))
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
// Prompt-command parser
// ---------------------------------------------------------------------------

/// Parse a prompt-command markdown file into (description, body).
///
/// Format (all parts optional except body):
/// ```text
/// ---
/// description: short summary
/// ---
/// Body text with $ARGUMENTS placeholder.
/// ```
///
/// If no frontmatter is present we treat the first non-empty line as the
/// description and the rest as body. This makes it easy to write a one-file
/// command without ceremony.
fn parse_prompt_command(content: &str) -> (String, String) {
    let text = content.trim_start();
    if let Some(stripped) = text.strip_prefix("---") {
        // Frontmatter form. Find the closing `---`.
        if let Some(end) = stripped.find("\n---") {
            let frontmatter = &stripped[..end];
            let body = stripped[end + 4..].trim_start_matches('\n').to_string();
            let mut desc = String::new();
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("description:") {
                    desc = rest.trim().trim_matches('"').to_string();
                    break;
                }
            }
            if desc.is_empty() {
                desc = "user-defined prompt".to_string();
            }
            return (desc, body);
        }
    }
    // No frontmatter: first non-empty line is the description, rest is body.
    let mut lines = text.lines();
    let description = lines
        .by_ref()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("user-defined prompt")
        .trim()
        .trim_start_matches('#')
        .trim()
        .to_string();
    let body: String = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (description, body)
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

fn cmd_exit(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.running = false;
    Ok(Some("Goodbye.".to_string()))
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
        "Auto-commit by octo"
    } else {
        args.trim()
    };
    ctx.app.status_message = format!("Commit request queued: {message}");
    Ok(None)
}

fn cmd_test(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let framework = args.trim();
    ctx.app.status_message = if framework.is_empty() {
        "Test request queued".to_string()
    } else {
        format!("Test command queued: {framework}")
    };
    Ok(None)
}

#[must_use]
pub fn forwarded_agent_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let mut parts = trimmed.splitn(2, ' ');
    let name = parts.next()?;
    let args = parts.next().unwrap_or("").trim();
    match name {
        "/commit" => {
            let message = if args.is_empty() {
                "Auto-commit by octo"
            } else {
                args
            };
            Some(format!(
                "Stage the appropriate workspace changes and create a git commit with this message: {message}"
            ))
        }
        "/test" => {
            if args.is_empty() {
                Some(
                    "Run the project test suite through the normal tool approval flow.".to_string(),
                )
            } else {
                Some(format!(
                    "Run this test command through the normal tool approval flow: {args}"
                ))
            }
        }
        _ => None,
    }
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

fn cmd_glob(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let mut parts = args.split_whitespace();
    let Some(pattern) = parts.next() else {
        return Err("Usage: /glob <pattern> [limit]".to_string());
    };
    let limit = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50);
    let matches = crate::search::search_files(ctx.project_root, pattern, limit)
        .map_err(|e| format!("Glob failed: {e}"))?;
    if matches.is_empty() {
        return Ok(Some("No files matched.".to_string()));
    }
    let mut lines = vec![
        manager_header("glob", "ready"),
        format!("count     {}", matches.len()),
    ];
    for item in matches {
        lines.push(item.path.display().to_string());
    }
    Ok(Some(lines.join("\n")))
}

fn cmd_grep(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let mut parts = args.split_whitespace();
    let Some(pattern) = parts.next() else {
        return Err("Usage: /grep <pattern> [glob]".to_string());
    };
    let glob = parts.next();
    let matches = crate::search::search_code(ctx.project_root, pattern, glob, false, 50)
        .map_err(|e| format!("Grep failed: {e}"))?;
    if matches.is_empty() {
        return Ok(Some("No matches found.".to_string()));
    }
    let mut lines = vec![
        manager_header("grep", "ready"),
        format!("count     {}", matches.len()),
    ];
    for item in matches {
        lines.push(format!(
            "{}:{}: {}",
            item.path.display(),
            item.line_number.unwrap_or(0),
            item.matched_text
        ));
    }
    Ok(Some(lines.join("\n")))
}

fn read_todo_items(project_root: &std::path::Path) -> Result<Vec<serde_json::Value>, String> {
    crate::tools::todo_state::read_todo_items(project_root)
        .map_err(|e| format!("Failed to read todos: {e}"))
}

fn write_todo_items(
    project_root: &std::path::Path,
    todos: Vec<serde_json::Value>,
) -> Result<(), String> {
    crate::tools::todo_state::write_todo_items(project_root, todos)
        .map(|_| ())
        .map_err(|e| format!("Failed to write todos: {e}"))
}

fn render_todos(project_root: &std::path::Path, language: &str) -> Result<String, String> {
    let todos = read_todo_items(project_root)?;
    if todos.is_empty() {
        return Ok(format!(
            "{}\nstatus    no todos",
            localized_manager_header("todo", "empty", language)
        ));
    }
    let mut lines = vec![
        localized_manager_header("todo", "ready", language),
        format!("count     {}", todos.len()),
    ];
    for (idx, todo) in todos.iter().enumerate() {
        let status = todo
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("pending");
        let content = todo
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(empty)");
        lines.push(format!("{:>2}. {:<11} {}", idx + 1, status, content));
    }
    Ok(lines.join("\n"))
}

fn cmd_todo(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let mut parts = args.trim().splitn(2, ' ');
    let action = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match action {
        "" | "list" => render_todos(ctx.project_root, &ctx.app.config.ui.language).map(Some),
        "add" => {
            if rest.is_empty() {
                return Err("Usage: /todo add <item>".to_string());
            }
            let mut todos = read_todo_items(ctx.project_root)?;
            todos.push(serde_json::json!({
                "content": rest,
                "status": "pending",
                "priority": "medium"
            }));
            write_todo_items(ctx.project_root, todos)?;
            ctx.app.refresh_todo_summary();
            render_todos(ctx.project_root, &ctx.app.config.ui.language).map(Some)
        }
        "clear" => {
            write_todo_items(ctx.project_root, Vec::new())?;
            ctx.app.refresh_todo_summary();
            Ok(Some("Todo list cleared.".to_string()))
        }
        "start" | "done" | "cancel" => {
            let index = rest
                .parse::<usize>()
                .map_err(|_| format!("Usage: /todo {action} <number>"))?;
            let mut todos = read_todo_items(ctx.project_root)?;
            let Some(todo) = todos.get_mut(index.saturating_sub(1)) else {
                return Err(format!("Todo not found: {index}"));
            };
            let status = match action {
                "start" => "in_progress",
                "done" => "completed",
                _ => "cancelled",
            };
            if let Some(object) = todo.as_object_mut() {
                object.insert(
                    "status".to_string(),
                    serde_json::Value::String(status.to_string()),
                );
            }
            write_todo_items(ctx.project_root, todos)?;
            ctx.app.refresh_todo_summary();
            render_todos(ctx.project_root, &ctx.app.config.ui.language).map(Some)
        }
        _ => Err("Usage: /todo list|add|start|done|cancel|clear".to_string()),
    }
}

fn render_tasks(project_root: &std::path::Path, language: &str) -> Result<String, String> {
    let todos = read_todo_items(project_root)?;
    if todos.is_empty() {
        return Ok(format!(
            "{}\nstatus    no local tasks\nusage     /task list|create|get|update|stop",
            localized_manager_header("task", "empty", language)
        ));
    }
    let mut lines = vec![
        localized_manager_header("task", "ready", language),
        format!("count     {}", todos.len()),
    ];
    for (idx, todo) in todos.iter().enumerate() {
        let id = todo
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("#{}", idx + 1));
        let status = todo
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("pending");
        let content = todo
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(empty)");
        lines.push(format!("{id:<12} {status:<11} {content}"));
    }
    lines.push("usage     /task list|create|get|update|stop".to_string());
    Ok(lines.join("\n"))
}

fn parse_task_update_args(rest: &str) -> Result<crate::tools::task_todo::TaskUpdateArgs, String> {
    let mut parts = rest.trim().splitn(3, ' ');
    let id = parts.next().unwrap_or("").trim();
    let field = parts.next().unwrap_or("").trim();
    let value = parts.next().unwrap_or("").trim();
    if id.is_empty() || field.is_empty() {
        return Err("Usage: /task update <id> status|content|active_form <value>".to_string());
    }
    let mut args = crate::tools::task_todo::TaskUpdateArgs {
        id: id.to_string(),
        content: None,
        active_form: None,
        status: None,
    };
    match field {
        "status" => args.status = Some(value.to_string()),
        "content" => args.content = Some(value.to_string()),
        "active_form" | "active-form" => args.active_form = Some(value.to_string()),
        "pending" | "in_progress" | "in-progress" | "active" | "running" | "completed" | "done"
        | "cancelled" | "canceled" | "stop" | "stopped" => {
            args.status = Some(field.to_string());
        }
        _ => return Err("Usage: /task update <id> status|content|active_form <value>".to_string()),
    }
    Ok(args)
}

fn cmd_task(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let mut parts = args.trim().splitn(2, ' ');
    let action = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match action {
        "help" => Ok(Some(
            "Usage: /task list|create|get|update|start|done|stop\nState: .octocode/todos.json"
                .to_string(),
        )),
        "" | "list" => render_tasks(ctx.project_root, &ctx.app.config.ui.language).map(Some),
        "add" | "create" => {
            if rest.is_empty() {
                return Err("Usage: /task create <content>".to_string());
            }
            let output = crate::tools::task_todo::task_create(
                ctx.project_root,
                crate::tools::task_todo::TaskCreateArgs {
                    id: None,
                    content: rest.to_string(),
                    active_form: None,
                    status: None,
                },
            )
            .map_err(|e| format!("Failed to create task: {e}"))?;
            ctx.app.refresh_todo_summary();
            Ok(Some(format!(
                "{}\n{output}",
                localized_manager_header("task", "created", &ctx.app.config.ui.language)
            )))
        }
        "get" | "show" => {
            if rest.is_empty() {
                return Err("Usage: /task get <id>".to_string());
            }
            crate::tools::task_todo::task_get(ctx.project_root, rest)
                .map(|output| Some(format!("{}\n{output}", manager_header("task", "ready"))))
                .map_err(|e| format!("Failed to get task: {e}"))
        }
        "update" => {
            let output = crate::tools::task_todo::task_update(
                ctx.project_root,
                parse_task_update_args(rest)?,
            )
            .map_err(|e| format!("Failed to update task: {e}"))?;
            ctx.app.refresh_todo_summary();
            Ok(Some(format!(
                "{}\n{output}",
                manager_header("task", "ready")
            )))
        }
        "start" | "done" | "complete" => {
            if rest.is_empty() {
                return Err(format!("Usage: /task {action} <id>"));
            }
            let status = if action == "start" {
                "in_progress"
            } else {
                "completed"
            };
            let output = crate::tools::task_todo::task_update(
                ctx.project_root,
                crate::tools::task_todo::TaskUpdateArgs {
                    id: rest.to_string(),
                    content: None,
                    active_form: None,
                    status: Some(status.to_string()),
                },
            )
            .map_err(|e| format!("Failed to update task: {e}"))?;
            ctx.app.refresh_todo_summary();
            Ok(Some(format!(
                "{}\n{output}",
                manager_header("task", "ready")
            )))
        }
        "stop" | "cancel" => {
            if rest.is_empty() {
                return Err(format!("Usage: /task {action} <id>"));
            }
            let output = crate::tools::task_todo::task_stop(ctx.project_root, rest)
                .map_err(|e| format!("Failed to stop task: {e}"))?;
            ctx.app.refresh_todo_summary();
            Ok(Some(format!(
                "{}\n{output}",
                manager_header("task", "ready")
            )))
        }
        _ => Err("Usage: /task list|create|get|update|start|done|stop".to_string()),
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
    let total = ctx.app.total_tokens;
    let budget = crate::provider::context_budget_for(
        ctx.app.config.provider.default,
        &ctx.app.model,
        ctx.app.config.search.max_context_tokens,
    );
    let percent = budget.usage_percent(total);
    let cache_hit = ctx
        .app
        .cache
        .as_ref()
        .map_or(0.0, |cache| cache.hit_rate() * 100.0);
    let model_window = budget
        .model_window_tokens
        .map_or_else(|| "unknown".to_string(), compact_token_count);
    let language = &ctx.app.config.ui.language;
    let chinese = crate::tui::welcome::is_chinese_display_language(language);
    let tool_summary = if ctx.app.recent_tool_summaries.is_empty() {
        if chinese {
            "无".to_string()
        } else {
            "none".to_string()
        }
    } else {
        ctx.app
            .recent_tool_summaries
            .iter()
            .map(|line| truncate_display_width(line, 72))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let pending_question = ctx
        .app
        .last_user_question_summary
        .as_deref()
        .map(|summary| truncate_display_width(summary, 72))
        .unwrap_or_else(|| {
            if chinese {
                "无".to_string()
            } else {
                "none".to_string()
            }
        });
    let compact_summary = ctx
        .app
        .latest_compact_summary
        .as_deref()
        .map(|summary| truncate_display_width(summary, 72))
        .unwrap_or_else(|| {
            if budget.next_action(total) == "auto compact should run before the next large turn" {
                if chinese {
                    "达到阈值后会在 turn 边界写入确定性摘要"
                } else {
                    "deterministic summary will be written at the next turn boundary"
                }
                .to_string()
            } else if chinese {
                "无".to_string()
            } else {
                "none".to_string()
            }
        });
    let compact_source = ctx
        .app
        .latest_compact_reason
        .as_deref()
        .unwrap_or(if chinese { "无" } else { "none" });
    let resume_items = [
        (!ctx.app.todo_summary.is_empty()).then_some("todo"),
        (!ctx.app.recent_tool_summaries.is_empty()).then_some(if chinese {
            "工具摘要"
        } else {
            "tool summaries"
        }),
        ctx.app
            .last_user_question_summary
            .is_some()
            .then_some(if chinese {
                "待选问题"
            } else {
                "pending question"
            }),
        ctx.app
            .latest_compact_summary
            .is_some()
            .then_some(if chinese {
                "压缩摘要"
            } else {
                "compact summary"
            }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let resume_line = if resume_items.is_empty() {
        if chinese { "无" } else { "none" }.to_string()
    } else {
        resume_items.join(", ")
    };
    Ok(Some(
        [
            localized_manager_header("context", "ready", language),
            format!(
                "{}       {} / {}",
                if chinese { "模型" } else { "model" },
                ctx.app.config.provider.default.display_name(),
                ctx.app.model
            ),
            format!(
                "{}      {model_window} tokens ({})",
                if chinese { "窗口" } else { "window" },
                localized_context_window_source(budget.model_window_source, chinese)
            ),
            format!(
                "{}       {} tokens (search.max_context_tokens)",
                if chinese { "本地" } else { "local" },
                compact_token_count(budget.local_budget_tokens)
            ),
            format!(
                "{}      {}/{} tokens ({percent:.1}%)",
                if chinese { "预算" } else { "budget" },
                compact_token_count(total),
                compact_token_count(budget.effective_budget_tokens)
            ),
            format!(
                "{}     auto threshold {} tokens",
                if chinese { "压缩" } else { "compact" },
                compact_token_count(budget.auto_compact_threshold_tokens)
            ),
            format!(
                "{}     {} tokens",
                if chinese { "本轮" } else { "current" },
                ctx.app.current_turn_tokens
            ),
            format!(
                "{}    {}",
                if chinese { "消息" } else { "messages" },
                ctx.app.messages.len()
            ),
            ctx.app.todo_summary.context_line(chinese),
            format!(
                "{} {}",
                if chinese {
                    "工具摘要  "
                } else {
                    "tool summaries"
                },
                tool_summary
            ),
            format!(
                "{} {}",
                if chinese {
                    "待选问题  "
                } else {
                    "pending question"
                },
                pending_question
            ),
            format!(
                "{}     {}",
                if chinese {
                    "最近摘要"
                } else {
                    "last summary"
                },
                compact_summary
            ),
            format!(
                "{}     {}",
                if chinese {
                    "摘要来源"
                } else {
                    "summary source"
                },
                compact_source
            ),
            format!(
                "{}     {}",
                if chinese {
                    "可恢复项"
                } else {
                    "recoverable"
                },
                resume_line
            ),
            format!("cache       {cache_hit:.0}%"),
            format!("cost        ¥{:.3}", ctx.app.total_cost),
            format!(
                "{}        {}",
                if chinese { "下一步" } else { "next" },
                localized_context_next_action(budget.next_action(total), chinese)
            ),
        ]
        .join("\n"),
    ))
}

fn localized_context_window_source(source: &'static str, chinese: bool) -> &'static str {
    if !chinese {
        return source;
    }
    match source {
        "DeepSeek API model details" => "DeepSeek API 模型能力",
        "not declared by provider preset; using local assembly budget" => {
            "provider 未声明，使用本地上下文预算"
        }
        _ => source,
    }
}

fn localized_context_next_action(action: &'static str, chinese: bool) -> &'static str {
    if !chinese {
        return action;
    }
    match action {
        "auto compact should run before the next large turn" => "下一次大任务前应自动压缩",
        "keep collecting context" => "继续收集上下文",
        _ => action,
    }
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
        Ok(Some(
            [
                manager_header("mcp", "empty"),
                "status    no servers configured".to_string(),
                "next      add [mcp.servers] to ~/.octocode/config.toml or ./.octocode/config.toml"
                    .to_string(),
            ]
            .join("\n"),
        ))
    } else {
        Ok(Some(format!(
            "{}\n{}",
            manager_header("mcp", "ready"),
            ctx.mcp_status
        )))
    }
}

fn cmd_usage(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let all_time = args.trim() == "--all-time" || args.trim() == "all";
    if all_time {
        return cmd_usage_all_time(ctx);
    }
    let mut lines = vec!["Session Usage".to_string(), "".to_string()];
    lines.push(format!("Model: {:?}", ctx.app.model));
    lines.push(format!("Total tokens: {}", ctx.app.total_tokens));
    lines.push(format!("Estimated cost: ¥{:.3}", ctx.app.total_cost));
    if let Some(cache) = ctx.app.cache.as_ref() {
        let total = cache.prompt_cache_hit_tokens + cache.prompt_cache_miss_tokens;
        if total > 0 {
            let rate = cache.hit_rate() * 100.0;
            lines.push("".to_string());
            lines.push("Prompt cache:".to_string());
            lines.push(format!("  hit  : {} tokens", cache.prompt_cache_hit_tokens));
            lines.push(format!(
                "  miss : {} tokens",
                cache.prompt_cache_miss_tokens
            ));
            lines.push(format!("  rate : {:.1}%", rate));
            let saved_rate = match ctx.app.model {
                crate::deepseek::DeepSeekModel::Pro => 2.0 - 0.5,
                _ => 0.5 - 0.1,
            };
            let saved = cache.prompt_cache_hit_tokens as f64 / 1_000_000.0 * saved_rate;
            lines.push(format!("  saved: ~¥{:.3}", saved));
        }
    }
    lines.push("".to_string());
    lines.push("Pricing (DeepSeek API, CNY / MTok):".to_string());
    lines.push("  Flash: input 0.5 (cache hit 0.1), output 2.0".to_string());
    lines.push("  Pro:   input 2.0 (cache hit 0.5), output 8.0".to_string());
    lines.push("".to_string());
    lines.push(
        "Tip: `/usage --all-time` sums across all saved sessions for this project.".to_string(),
    );
    Ok(Some(lines.join("\n")))
}

/// Walk the session store and aggregate usage across every saved session
/// for the current project. Sessions written before P1-B don't have the
/// cache fields and are read with `#[serde(default)]` → zero contribution.
fn cmd_usage_all_time(ctx: &mut CommandContext) -> CommandResult {
    let Some(home) = dirs::home_dir() else {
        return Ok(Some("Cannot resolve home directory.".to_string()));
    };
    let session_store = crate::storage::SessionStore::new(home.join(".octocode").join("sessions"));
    let summaries = session_store.list(ctx.project_root).unwrap_or_default();

    let mut total_tokens: u64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut cache_hit: u64 = 0;
    let mut cache_miss: u64 = 0;
    let mut counted = 0_usize;
    for summary in &summaries {
        if let Ok(session) = session_store.load(ctx.project_root, &summary.id) {
            total_tokens = total_tokens.saturating_add(session.metadata.total_tokens);
            total_cost += session.metadata.total_cost_estimate;
            cache_hit = cache_hit.saturating_add(session.metadata.prompt_cache_hit_tokens);
            cache_miss = cache_miss.saturating_add(session.metadata.prompt_cache_miss_tokens);
            counted += 1;
        }
    }

    let mut lines = vec![
        format!("All-Time Usage (this project, {counted} sessions)"),
        String::new(),
    ];
    lines.push(format!("Total tokens : {total_tokens}"));
    lines.push(format!("Estimated cost: ¥{:.3}", total_cost));
    let cache_total = cache_hit + cache_miss;
    if cache_total > 0 {
        let rate = (cache_hit as f64 / cache_total as f64) * 100.0;
        lines.push(String::new());
        lines.push("Prompt cache (cross-session):".to_string());
        lines.push(format!("  hit  : {cache_hit} tokens"));
        lines.push(format!("  miss : {cache_miss} tokens"));
        lines.push(format!("  rate : {:.1}%", rate));
    } else {
        lines.push(String::new());
        lines.push(
            "Prompt cache: no data (older sessions did not record cache fields).".to_string(),
        );
    }
    Ok(Some(lines.join("\n")))
}

fn cmd_doctor(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root));
    let provider = config
        .as_ref()
        .map(|config| config.provider.default)
        .unwrap_or_default();
    let api_key = crate::storage::get_effective_api_key(Some(ctx.project_root));
    let git_available = command_available("git");
    let cargo_available = command_available("cargo");
    let session_store = dirs::home_dir()
        .map(|home| home.join(".octocode").join("sessions"))
        .filter(|path| path.exists());
    let api_key_detail = if api_key.is_some() {
        "configured".to_string()
    } else {
        format!(
            "missing; run `octo login --api-key <key>` or set {}",
            crate::storage::api_key_env_hint(provider)
        )
    };

    let mut lines = vec![
        "Octocode Doctor".to_string(),
        String::new(),
        doctor_line(api_key.is_some(), "API key", &api_key_detail),
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
        "Doctor is local-only in the TUI; run `octo doctor` for API connectivity checks."
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

fn compact_token_count(value: u64) -> String {
    if value >= 1_000_000 {
        if value.is_multiple_of(1_000_000) {
            format!("{}M", value / 1_000_000)
        } else {
            format!("{:.1}M", value as f64 / 1_000_000.0)
        }
    } else if value >= 1_000 {
        if value.is_multiple_of(1_000) {
            format!("{}K", value / 1_000)
        } else {
            format!("{:.1}K", value as f64 / 1_000.0)
        }
    } else {
        value.to_string()
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
                "Theme: {}\n\nUsage: /theme auto | light | dark | high-contrast | toggle",
                ctx.app.theme_mode.label()
            )));
        }
        "auto" | "system" => crate::tui::theme::ThemeMode::Auto,
        "light" | "droid" => crate::tui::theme::ThemeMode::Light,
        "dark" | "terminal" => crate::tui::theme::ThemeMode::Dark,
        "high-contrast" | "high_contrast" | "contrast" | "hc" => {
            crate::tui::theme::ThemeMode::HighContrast
        }
        "toggle" => ctx.app.theme_mode.toggled(),
        other => {
            return Err(format!(
                "Unknown theme: {other}. Use /theme auto, /theme light, /theme dark, /theme high-contrast, or /theme toggle."
            ));
        }
    };
    write_project_ui_string_override(ctx.project_root, "theme", mode.label())?;
    ctx.app.set_theme_mode(mode);
    Ok(Some(format!(
        "Theme set to {}. Saved to local config.",
        mode.label()
    )))
}

fn cmd_tui(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let requested = args.trim().to_ascii_lowercase();
    let mode = match requested.as_str() {
        "" => {
            return Ok(Some(format!(
                "TUI renderer: {}\n\nUsage: /tui auto | classic | fullscreen",
                ctx.app.renderer_mode.label()
            )));
        }
        "auto" | "default" => "auto",
        "classic" | "terminal" | "scrollback" => "classic",
        "fullscreen" | "full-screen" | "alternate" | "alt" => "fullscreen",
        other => {
            return Err(format!(
                "Unknown TUI renderer: {other}. Use /tui auto, /tui classic, or /tui fullscreen."
            ));
        }
    };
    write_project_ui_string_override(ctx.project_root, "renderer", mode)?;
    ctx.app.set_renderer_config(mode);
    Ok(Some(format!(
        "TUI renderer set to {} (resolved to {}). Restart octo to apply terminal mode.",
        mode,
        ctx.app.renderer_mode.label()
    )))
}

fn cmd_settings(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    ctx.app.open_settings_panel();
    Ok(None)
}

fn output_style_path(project_root: &std::path::Path) -> std::path::PathBuf {
    project_root.join(".octocode").join("output_style.json")
}

fn cmd_output_style(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let requested = args.trim().to_ascii_lowercase();
    let path = output_style_path(ctx.project_root);
    if requested.is_empty() {
        let current = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|value| {
                value
                    .get("style")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "normal".to_string());
        return Ok(Some(format!(
            "Output style: {current}\n\nUsage: /output-style normal | teaching | concise"
        )));
    }
    let style = match requested.as_str() {
        "normal" | "default" | "普通" => "normal",
        "teaching" | "teach" | "教学" => "teaching",
        "concise" | "short" | "minimal" | "简洁" => "concise",
        other => {
            return Err(format!(
                "Unknown output style: {other}. Use /output-style normal, teaching, or concise."
            ));
        }
    };
    let dir = ctx.project_root.join(".octocode");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .octocode: {e}"))?;
    let prompt_policy = match style {
        "teaching" => "explain important decisions and teach as you work",
        "concise" => "answer directly and keep explanations short",
        _ => "balanced engineering explanation",
    };
    let payload = serde_json::json!({
        "style": style,
        "updated_at": chrono::Utc::now(),
        "prompt_policy": prompt_policy
    });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("Failed to render output style: {e}"))?,
    )
    .map_err(|e| format!("Failed to write output style: {e}"))?;
    ctx.app.push_activity(format!("output-style: {style}"));
    Ok(Some(format!("Output style set to {style}.")))
}

fn keybindings_path(project_root: &std::path::Path) -> std::path::PathBuf {
    project_root.join(".octocode").join("keybindings.toml")
}

fn cmd_keybindings(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let action = args.trim();
    let path = keybindings_path(ctx.project_root);
    if matches!(action, "defaults" | "write-defaults" | "init") {
        let dir = ctx.project_root.join(".octocode");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .octocode: {e}"))?;
        let content = r#"[keybindings]
submit = "enter"
exit = "ctrl+d ctrl+d"
interrupt = "esc"
history_up = "up"
history_down = "down"
complete = "tab"
open_settings = "ctrl+,"

[labels.zh-CN]
submit = "发送"
exit = "连按两次退出"
interrupt = "中断"
history_up = "上一条"
history_down = "下一条"
complete = "补全"
open_settings = "设置"
"#;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write keybindings: {e}"))?;
    }

    let mut lines = vec![manager_header("keybindings", "ready")];
    if path.exists() {
        lines.push(format!("config    {}", path.display()));
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(16)
            {
                lines.push(line.to_string());
            }
        }
    } else {
        lines.push("config    not created".to_string());
        lines.push("next      /keybindings defaults".to_string());
    }
    Ok(Some(lines.join("\n")))
}

fn cmd_language(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let requested = args.trim().to_ascii_lowercase();
    let current = ctx.app.config.ui.language.clone();
    let current_label = language_display_name(&current);
    let value = match requested.as_str() {
        "" => {
            return Ok(Some(format!(
                "Language: {current_label}\n\nUsage: /language auto | Chinese | English | Japanese"
            )));
        }
        "auto" | "system" => "auto",
        "zh" | "zh-cn" | "cn" | "chinese" | "中文" => "zh-CN",
        "en" | "en-us" | "english" => "en-US",
        "ja" | "ja-jp" | "jp" | "japanese" | "日本語" => "ja-JP",
        other => {
            return Err(format!(
                "Unknown language: {other}. Use /language auto, /language Chinese, /language English, or /language Japanese."
            ));
        }
    };
    write_project_ui_string_override(ctx.project_root, "language", value)?;
    ctx.app.config.ui.language = value.to_string();
    ctx.app.welcome.display_language = value.to_string();
    ctx.app
        .push_activity(format!("language: {}", ctx.app.config.ui.language));
    let label = language_display_name(&ctx.app.config.ui.language);
    Ok(Some(format!(
        "Language set to {label}. Saved to local config."
    )))
}

fn language_display_name(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "auto" | "" => "Automatic",
        "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" | "chinese" => "Chinese",
        "en" | "en-us" | "en-gb" | "english" => "English",
        "ja" | "ja-jp" | "jp" | "japanese" => "Japanese",
        value if value.starts_with("zh") => "Chinese",
        value if value.starts_with("en") => "English",
        value if value.starts_with("ja") => "Japanese",
        _ => "English",
    }
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
        let current_model = crate::tui::model_hint::model_display_name(&ctx.app.model);
        ctx.app.pending_options = Some((
            "Choose model".to_string(),
            vec!["/model flash".to_string(), "/model pro".to_string()],
        ));
        ctx.app.selected_option_index = 0;
        return Ok(Some(format!(
            "Current model: {current_model}\n\nSelect a model:\n- DeepSeek V4 Flash: /model flash\n- DeepSeek V4 Pro: /model pro"
        )));
    }

    let model = crate::provider::parse_model(requested).map_err(|error| error.to_string())?;
    let model_name = crate::tui::model_hint::model_display_name(&model);
    ctx.app.model = model.clone();
    ctx.app.welcome = crate::tui::welcome::WelcomeDashboardData::load(
        ctx.project_root,
        ctx.app.model.clone(),
        ctx.app.thinking_mode.clone(),
    );
    Ok(Some(format!("Session model set to {model_name}")))
}

fn cmd_config(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    let project_config = ctx.project_root.join(".octocode").join("config.toml");
    let user_config = dirs::home_dir()
        .map(|home| home.join(".octocode").join("config.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.octocode/config.toml"));
    let lines = [
        "Config".to_string(),
        "".to_string(),
        format!("project: {}", project_config.display()),
        format!("user: {}", user_config.display()),
        format!("provider: {}", config.provider.default.as_str()),
        format!("default model: {:?}", config.model.default),
        format!("heavy model: {:?}", config.model.heavy),
        format!("theme: {}", config.ui.theme),
        format!("motion: {}", config.ui.motion),
        format!("renderer: {}", config.ui.renderer),
        format!("autonomy: {}", config.policy.autonomy_level.as_str()),
        format!("MCP enabled: {}", on_off(config.mcp.enabled)),
        format!("subagents enabled: {}", on_off(config.subagent.enabled)),
    ];
    Ok(Some(lines.join("\n")))
}

fn cmd_memory(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let mut lines = vec!["Memory files".to_string(), "".to_string()];
    for path in crate::agent::prompt_builder::project_rule_candidates(ctx.project_root) {
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
    let store = crate::storage::SessionStore::new(home.join(".octocode"));
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
    lines.push("next      use `octo resume <id-prefix>` outside TUI to resume".to_string());
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
    let snapshot = ctx
        .app
        .compact_local_context(ctx.project_root, keep, "manual /compact");
    let removed = before.saturating_sub(snapshot.after_messages);
    let chinese = crate::tui::welcome::is_chinese_display_language(&ctx.app.config.ui.language);
    if chinese {
        return Ok(Some(format!(
            "已压缩本地上下文：移除 {removed} 条旧消息，保留 {} 条；token {} -> {}。\n摘要会写入会话事件并在后续 prompt/resume 中恢复。",
            snapshot.after_messages,
            compact_token_count(snapshot.before_tokens),
            compact_token_count(snapshot.after_tokens)
        )));
    }
    Ok(Some(format!(
        "Compacted local context: removed {removed} older messages, kept {}. Tokens {} -> {}.\nSummary was written to the session event log for prompt/resume recovery.",
        snapshot.after_messages,
        compact_token_count(snapshot.before_tokens),
        compact_token_count(snapshot.after_tokens)
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
    let project_dir = ctx.project_root.join(".octocode").join("commands");
    let user_dir = dirs::home_dir()
        .map(|home| home.join(".octocode").join("commands"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.octocode/commands"));
    let project_count = count_entries(&project_dir);
    let user_count = count_entries(&user_dir);
    let language = &ctx.app.config.ui.language;
    let chinese = crate::tui::welcome::is_chinese_display_language(language);
    Ok(Some(
        [
            localized_manager_header("commands", "ready", language),
            format!(
                "{}   {}",
                if chinese { "内置" } else { "built-in" },
                CommandRegistry::new().list_commands().len()
            ),
            format!(
                "{}    {} ({})",
                if chinese { "项目" } else { "project" },
                project_count,
                project_dir.display()
            ),
            format!(
                "{}       {} ({})",
                if chinese { "用户" } else { "user" },
                user_count,
                user_dir.display()
            ),
            if chinese {
                "下一步    在 .octocode/commands 中添加提示词命令文件".to_string()
            } else {
                "next       add prompt command files in .octocode/commands".to_string()
            },
        ]
        .join("\n"),
    ))
}

fn cmd_skills(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let project_dir = ctx.project_root.join(".octocode").join("skills");
    let user_dir = dirs::home_dir()
        .map(|home| home.join(".octocode").join("skills"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.octocode/skills"));
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
    lines.push("next       .octocode/skills/<name>/SKILL.md".to_string());
    Ok(Some(lines.join("\n")))
}

fn cmd_hooks(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    Ok(Some(
        [
            manager_header("hooks", "ready"),
            format!(
                "user_prompt_submit {}",
                config.hooks.user_prompt_submit.len()
            ),
            format!("pre_tool   {}", config.hooks.pre_tool.len()),
            format!("post_tool  {}", config.hooks.post_tool.len()),
            format!("session_start {}", config.hooks.session_start.len()),
            format!("session_end {}", config.hooks.session_end.len()),
            format!("stop       {}", config.hooks.stop.len()),
            "events     UserPromptSubmit, PreToolUse, PostToolUse, SessionStart, SessionEnd"
                .to_string(),
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
    let language = &ctx.app.config.ui.language;
    let chinese = crate::tui::welcome::is_chinese_display_language(language);
    Ok(Some(
        [
            localized_manager_header("statusline", "ready", language),
            format!(
                "{}      {}",
                if chinese { "模式" } else { "mode" },
                ctx.app.interaction_mode.label()
            ),
            format!(
                "{}     {}",
                if chinese { "主题" } else { "theme" },
                ctx.app.theme_mode.label()
            ),
            if chinese {
                "样式     紧凑标签".to_string()
            } else {
                "style     compact chips".to_string()
            },
            if chinese {
                "段落     app, mode, model, status, permissions, context".to_string()
            } else {
                "segments  app, mode, model, status, permissions, context".to_string()
            },
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
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    let language = &ctx.app.config.ui.language;
    let chinese = crate::tui::welcome::is_chinese_display_language(language);
    let mut lines = vec![localized_manager_header("agents", "ready", language)];
    if chinese {
        let agent_names = registry.list();
        let name_width = agent_names
            .iter()
            .map(|name| display_width(name))
            .max()
            .unwrap_or(4)
            .max(display_width("名称"));
        let tools_width = agent_names
            .iter()
            .filter_map(|name| registry.get(name))
            .map(|agent| display_width(&agent_tools_label(agent.allowed_tools.as_slice(), chinese)))
            .max()
            .unwrap_or(4)
            .max(display_width("工具"))
            .min(52);
        lines.push(format!(
            "{}  {}  轮次",
            pad_display("名称", name_width),
            pad_display("工具", tools_width)
        ));
        for name in agent_names {
            if let Some(agent) = registry.get(name) {
                let tools = truncate_display_width(
                    &agent_tools_label(agent.allowed_tools.as_slice(), chinese),
                    tools_width,
                );
                lines.push(format!(
                    "{}  {}  {}",
                    pad_display(name, name_width),
                    pad_display(&tools, tools_width),
                    agent.max_turns
                ));
            }
        }
        lines.push(String::new());
        lines.push(format!(
            "{}  {}",
            pad_display(
                "自定义智能体",
                name_width.max(display_width("自定义智能体"))
            ),
            localized_on_off(config.subagent.allow_custom_agents, chinese)
        ));
        if let Some(dir) = config.subagent.custom_agents_dir {
            lines.push(format!("自定义目录  {}", dir.display()));
        }
    } else {
        for name in registry.list() {
            if let Some(agent) = registry.get(name) {
                lines.push(format!(
                    "{} — tools: {} — max turns: {}",
                    name,
                    agent_tools_label(agent.allowed_tools.as_slice(), chinese),
                    agent.max_turns
                ));
            }
        }
        lines.push(String::new());
        lines.push(format!(
            "custom agents: {}",
            on_off(config.subagent.allow_custom_agents)
        ));
        if let Some(dir) = config.subagent.custom_agents_dir {
            lines.push(format!("custom dir: {}", dir.display()));
        }
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

fn cmd_schedule(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let store = crate::storage::ScheduledTaskStore::default_user();
    let mut parts = args.trim().splitn(3, ' ');
    let action = parts.next().unwrap_or("");
    match action {
        "" | "list" => {
            let tasks = store
                .list()
                .map_err(|e| format!("Failed to list scheduled tasks: {e}"))?;
            let mut lines = vec![manager_header("schedule", "local")];
            if tasks.is_empty() {
                lines.push("status   no planned tasks".to_string());
                lines.push(format!("store    {}", store.root().display()));
            } else {
                lines.push(format!("count    {}", tasks.len()));
                for task in tasks {
                    lines.push(task.format_row());
                }
            }
            lines.push(
                "usage    /schedule add heartbeat|standalone <task> | pause|resume|logs|rm <id>"
                    .to_string(),
            );
            Ok(Some(lines.join("\n")))
        }
        "add" => {
            let Some(kind) = parts.next() else {
                return Err("Usage: /schedule add heartbeat|standalone <task>".to_string());
            };
            let Some(prompt) = parts.next() else {
                return Err("Usage: /schedule add heartbeat|standalone <task>".to_string());
            };
            let kind = kind.parse::<crate::storage::ScheduledTaskKind>()?;
            let task = store
                .create(kind, prompt.trim().to_string(), ctx.project_root.to_path_buf())
                .map_err(|e| format!("Failed to create scheduled task: {e}"))?;
            Ok(Some(format!(
                "{}\nid      {}\nkind    {}\nstatus  {}",
                manager_header("schedule", "created"),
                task.id,
                task.kind.as_str(),
                task.status.as_str()
            )))
        }
        "pause" | "resume" | "logs" | "rm" | "remove" | "run" => {
            let Some(id) = parts.next() else {
                return Err(format!("Usage: /schedule {action} <id>"));
            };
            match action {
                "pause" => {
                    let task = store
                        .set_status(id, crate::storage::ScheduledTaskStatus::Paused)
                        .map_err(|e| format!("Failed to pause task: {e}"))?;
                    Ok(Some(format!(
                        "{}\nid      {}",
                        manager_header("schedule", "paused"),
                        task.id
                    )))
                }
                "resume" => {
                    let task = store
                        .set_status(id, crate::storage::ScheduledTaskStatus::Active)
                        .map_err(|e| format!("Failed to resume task: {e}"))?;
                    Ok(Some(format!(
                        "{}\nid      {}",
                        manager_header("schedule", "resumed"),
                        task.id
                    )))
                }
                "logs" => {
                    let task = store
                        .load(id)
                        .map_err(|e| format!("Failed to load task: {e}"))?;
                    let mut lines = vec![
                        manager_header("schedule", "logs"),
                        format!("id      {}", task.id),
                        format!("kind    {}", task.kind.as_str()),
                        format!("status  {}", task.status.as_str()),
                        format!("root    {}", task.project_root.display()),
                        format!("prompt  {}", task.prompt),
                        format!("last    {}", task.last_status.as_deref().unwrap_or("never run")),
                    ];
                    if let Some(path) = task.last_log_path {
                        lines.push(format!("log     {}", path.display()));
                    } else {
                        lines.push(
                            "log     use `octo resume` to inspect session event logs".to_string(),
                        );
                    }
                    Ok(Some(lines.join("\n")))
                }
                "run" => Ok(Some(
                    [
                        manager_header("schedule", "manual-run"),
                        format!("id      {id}"),
                        "next    run `octo task run <id>` from the shell".to_string(),
                        "note    TUI slash commands stay synchronous to avoid hidden side effects"
                            .to_string(),
                    ]
                    .join("\n"),
                )),
                _ => {
                    let task = store
                        .remove(id)
                        .map_err(|e| format!("Failed to remove task: {e}"))?;
                    Ok(Some(format!(
                        "{}\nid      {}",
                        manager_header("schedule", "removed"),
                        task.id
                    )))
                }
            }
        }
        _ => Err(
            "Usage: /schedule list | add heartbeat|standalone <task> | pause|resume|run|logs|rm <id>"
                .to_string(),
        ),
    }
}

fn cmd_swarm(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let action = args.trim();
    let config = crate::storage::Config::load(Some(ctx.project_root)).unwrap_or_default();
    match action {
        "" | "status" => {
            let running = ctx
                .app
                .subagents
                .iter()
                .filter(|card| {
                    card.status == crate::tui::subagent_cards::SubagentCardStatus::Running
                })
                .count();
            let mut lines = vec![
                manager_header(
                    "swarm",
                    if config.subagent.swarm_enabled {
                        "on"
                    } else {
                        "off"
                    },
                ),
                format!("max_parallel {}", config.subagent.max_parallel),
                format!(
                    "write_requires_approval {}",
                    on_off(config.subagent.write_requires_approval)
                ),
                format!(
                    "command_requires_approval {}",
                    on_off(config.subagent.command_requires_approval)
                ),
                format!("running_agents {}", running),
            ];
            if let Some(swarm) = ctx.app.active_swarm.as_ref() {
                lines.extend([
                    format!("run_id   {}", swarm.run_id),
                    format!("summary  {}", swarm.summary),
                    format!("status   {}", swarm.status),
                    format!(
                        "tasks    running {} · done {} · failed {} · cancelled {} · total {}",
                        swarm.running, swarm.done, swarm.failed, swarm.cancelled, swarm.total
                    ),
                    format!("cancel_requested {}", on_off(swarm.cancel_requested)),
                ]);
            }
            lines.push(
                "usage     /swarm on | /swarm off | /swarm status | /swarm cancel".to_string(),
            );
            Ok(Some(lines.join("\n")))
        }
        "on" | "off" => {
            let enabled = action == "on";
            write_project_swarm_override(ctx.project_root, enabled)?;
            Ok(Some(format!(
                "{}\nswarm_enabled {}",
                manager_header("swarm", action),
                on_off(enabled)
            )))
        }
        "cancel" => {
            ctx.app.request_swarm_cancel();
            Ok(Some(
                [
                    manager_header("swarm", "cancel-requested"),
                    "running swarm will stop before the next task batch".to_string(),
                    "already-started commands are not force killed; use Esc for a hard interrupt"
                        .to_string(),
                ]
                .join("\n"),
            ))
        }
        _ => Err("Usage: /swarm on | /swarm off | /swarm status | /swarm cancel".into()),
    }
}

fn write_project_swarm_override(
    project_root: &std::path::Path,
    enabled: bool,
) -> Result<(), String> {
    let dir = project_root.join(".octocode");
    let path = dir.join("local.toml");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    let mut table = if path.exists() {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read local.toml: {e}"))?;
        toml::from_str::<toml::Value>(&content)
            .ok()
            .and_then(|value| value.as_table().cloned())
            .unwrap_or_default()
    } else {
        toml::map::Map::new()
    };
    let subagent = table
        .entry("subagent".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let subagent_table = subagent
        .as_table_mut()
        .ok_or_else(|| "local.toml [subagent] is not a table".to_string())?;
    subagent_table.insert("swarm_enabled".to_string(), toml::Value::Boolean(enabled));
    let rendered = toml::to_string_pretty(&toml::Value::Table(table))
        .map_err(|e| format!("Failed to render local.toml: {e}"))?;
    std::fs::write(&path, rendered).map_err(|e| format!("Failed to write local.toml: {e}"))?;
    Ok(())
}

fn write_project_ui_string_override(
    project_root: &std::path::Path,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let dir = project_root.join(".octocode");
    let path = dir.join("local.toml");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    let mut table = if path.exists() {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read local.toml: {e}"))?;
        toml::from_str::<toml::Value>(&content)
            .ok()
            .and_then(|value| value.as_table().cloned())
            .unwrap_or_default()
    } else {
        toml::map::Map::new()
    };
    let ui = table
        .entry("ui".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let ui_table = ui
        .as_table_mut()
        .ok_or_else(|| "local.toml [ui] is not a table".to_string())?;
    ui_table.insert(key.to_string(), toml::Value::String(value.to_string()));
    let rendered = toml::to_string_pretty(&toml::Value::Table(table))
        .map_err(|e| format!("Failed to render local.toml: {e}"))?;
    std::fs::write(&path, rendered).map_err(|e| format!("Failed to write local.toml: {e}"))?;
    Ok(())
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn localized_on_off(value: bool, chinese: bool) -> &'static str {
    match (value, chinese) {
        (true, true) => "开启",
        (false, true) => "关闭",
        (true, false) => "on",
        (false, false) => "off",
    }
}

fn agent_tools_label(tools: &[String], chinese: bool) -> String {
    if tools.is_empty() {
        if chinese {
            "默认".to_string()
        } else {
            "default".to_string()
        }
    } else {
        tools.join(", ")
    }
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn pad_display(value: &str, width: usize) -> String {
    let used = display_width(value);
    if used >= width {
        value.to_string()
    } else {
        format!("{value}{}", " ".repeat(width - used))
    }
}

fn truncate_display_width(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_string();
    }
    let mut out = String::new();
    let target = width.saturating_sub(1);
    for ch in value.chars() {
        let next = format!("{out}{ch}");
        if display_width(&next) > target {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

fn manager_header(name: &str, status: &str) -> String {
    format!("◆ manager {name}  status:{status}")
}

fn localized_manager_header(name: &str, status: &str, language: &str) -> String {
    if crate::tui::welcome::is_chinese_display_language(language) {
        let status = match status {
            "ready" => "就绪",
            "running" => "运行中",
            "planned" => "计划中",
            "empty" => "空",
            "local" => "本地",
            "created" => "已创建",
            other => other,
        };
        format!("◆ 管理器 {name}  状态:{status}")
    } else {
        manager_header(name, status)
    }
}

fn cmd_help(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let registry = CommandRegistry::new();
    let language = &ctx.app.config.ui.language;
    let chinese = crate::tui::welcome::is_chinese_display_language(language);
    let mut lines = vec![
        if chinese {
            "可用命令:".to_string()
        } else {
            "Available commands:".to_string()
        },
        String::new(),
    ];
    for cmd in registry.list_commands() {
        lines.push(format!(
            "  {} — {}",
            cmd.name,
            localized_command_description(cmd.name, cmd.description, language)
        ));
        if !cmd.aliases.is_empty() {
            let label = if chinese { "别名" } else { "aliases" };
            lines.push(format!("    {label}: {}", cmd.aliases.join(", ")));
        }
        let label = if chinese { "用法" } else { "usage" };
        lines.push(format!(
            "    {label}: {}",
            localized_command_usage(cmd.usage, language)
        ));
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
            name: "/exit",
            aliases: &["/quit", "/q"],
            description: "Exit the interactive TUI",
            usage: "/exit or /quit",
            handler: cmd_exit,
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
            name: "/todo",
            aliases: &["/todos"],
            description: "Show and update the current work todo list",
            usage: "/todo list|add|start|done|cancel|clear",
            handler: cmd_todo,
        });
        self.register(&SlashCommand {
            name: "/task",
            aliases: &[],
            description: "Manage project todo-backed tasks",
            usage: "/task list|create|get|update|start|done|stop|help",
            handler: cmd_task,
        });
        self.register(&SlashCommand {
            name: "/glob",
            aliases: &[],
            description: "Find files by glob pattern",
            usage: "/glob <pattern> [limit]",
            handler: cmd_glob,
        });
        self.register(&SlashCommand {
            name: "/grep",
            aliases: &[],
            description: "Search file contents",
            usage: "/grep <pattern> [glob]",
            handler: cmd_grep,
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
            usage: "/theme [auto|light|dark|high-contrast|toggle]",
            handler: cmd_theme,
        });
        self.register(&SlashCommand {
            name: "/tui",
            aliases: &["/renderer"],
            description: "Show or switch the terminal renderer",
            usage: "/tui [classic|fullscreen]",
            handler: cmd_tui,
        });
        self.register(&SlashCommand {
            name: "/settings",
            aliases: &["/set"],
            description: "Open the editable settings panel",
            usage: "/settings",
            handler: cmd_settings,
        });
        self.register(&SlashCommand {
            name: "/output-style",
            aliases: &["/style"],
            description: "Show or switch assistant output style",
            usage: "/output-style [normal|teaching|concise]",
            handler: cmd_output_style,
        });
        self.register(&SlashCommand {
            name: "/keybindings",
            aliases: &["/keys"],
            description: "Show or write keybinding config",
            usage: "/keybindings [defaults]",
            handler: cmd_keybindings,
        });
        self.register(&SlashCommand {
            name: "/language",
            aliases: &["/lang"],
            description: "Show or switch the display language",
            usage: "/language [auto|Chinese|English|Japanese]",
            handler: cmd_language,
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
            name: "/swarm",
            aliases: &["/cluster"],
            description: "Control automatic local swarm agents",
            usage: "/swarm on|off|status|cancel",
            handler: cmd_swarm,
        });
        self.register(&SlashCommand {
            name: "/schedule",
            aliases: &[],
            description: "Manage local planned tasks",
            usage: "/schedule list|add|pause|resume|run|logs|rm",
            handler: cmd_schedule,
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
    fn parse_prompt_command_with_frontmatter() {
        let content = "---\ndescription: \"review the PR\"\n---\nReview these changes: $ARGUMENTS";
        let (desc, body) = parse_prompt_command(content);
        assert_eq!(desc, "review the PR");
        assert_eq!(body, "Review these changes: $ARGUMENTS");
    }

    #[test]
    fn parse_prompt_command_no_frontmatter_uses_first_line() {
        let content = "# Review code\nPlease review $ARGUMENTS.";
        let (desc, body) = parse_prompt_command(content);
        assert_eq!(desc, "Review code");
        assert_eq!(body, "Please review $ARGUMENTS.");
    }

    #[test]
    fn prompt_command_render_substitutes_arguments() {
        let cmd = PromptCommand {
            name: "/review".to_string(),
            description: "review".to_string(),
            body: "Look at $ARGUMENTS carefully.".to_string(),
            source_path: std::path::PathBuf::from("test.md"),
        };
        assert_eq!(cmd.render("src/main.rs"), "Look at src/main.rs carefully.");
    }

    #[test]
    fn load_prompt_commands_picks_up_md_files() {
        // Use a name that does NOT collide with any built-in.
        let temp = tempfile::tempdir().expect("tempdir");
        let cmd_dir = temp.path().join(".octocode").join("commands");
        std::fs::create_dir_all(&cmd_dir).expect("mkdir");
        std::fs::write(
            cmd_dir.join("daily-review.md"),
            "---\ndescription: daily review\n---\nReview $ARGUMENTS",
        )
        .expect("write file");

        let mut reg = CommandRegistry::new();
        let n = reg.load_prompt_commands(temp.path());
        assert!(n >= 1, "expected at least 1 prompt command, got {n}");
        let cmd = reg
            .prompt_commands
            .get("/daily-review")
            .expect("/daily-review should load");
        assert_eq!(cmd.description, "daily review");
        assert!(cmd.body.contains("$ARGUMENTS"));
    }

    #[test]
    fn prompt_command_does_not_shadow_builtin() {
        // `/help` is a built-in. A user file named help.md must be ignored.
        let temp = tempfile::tempdir().expect("tempdir");
        let cmd_dir = temp.path().join(".octocode").join("commands");
        std::fs::create_dir_all(&cmd_dir).expect("mkdir");
        std::fs::write(cmd_dir.join("help.md"), "rogue").expect("write");

        let mut reg = CommandRegistry::new();
        reg.load_prompt_commands(temp.path());
        // /help still points at the built-in
        assert!(reg.commands.contains_key("/help"));
        // and the prompt registry did not pick up the shadowing file
        assert!(!reg.prompt_commands.contains_key("/help"));
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
        assert!(output.contains("DeepSeek"));
        assert!(output.contains("DeepSeek V4 Pro"));
        let (_, options) = ctx.app.pending_options.as_ref().expect("model picker");
        assert_eq!(
            options,
            &vec!["/model flash".to_string(), "/model pro".to_string()]
        );
    }

    #[test]
    fn model_command_switches_session_model() {
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
            .execute("/model pro", &mut ctx)
            .expect("command should be handled")
            .expect("model should run")
            .expect("model should show output");

        assert!(output.contains("Session model set to DeepSeek V4 Pro"));
        assert_eq!(ctx.app.model, crate::deepseek::DeepSeekModel::Pro);
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
            "/exit",
            "/hooks",
            "/init",
            "/language",
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
        assert!(agents.contains("manager agents") || agents.contains("管理器 agents"));

        let tasks = execute_with_test_app("/missions")
            .expect("missions alias should run")
            .expect("missions alias should show output");
        assert!(tasks.contains("manager tasks"));

        let diagnostics = execute_with_test_app("/diagnostics")
            .expect("diagnostics alias should run")
            .expect("diagnostics alias should show output");
        assert!(diagnostics.contains("Octocode Doctor"));
    }

    #[test]
    fn context_command_reports_token_window() {
        let output = execute_with_test_app("/context")
            .expect("context should run")
            .expect("context should show output");

        assert!(output.contains("context"));
        assert!(output.contains("状态:就绪"));
        assert!(output.contains("DeepSeek"));
        assert!(output.contains("deepseek-v4-flash"));
        assert!(output.contains("窗口"));
        assert!(output.contains("压缩"));
    }

    #[test]
    fn compact_command_writes_summary_state() {
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            std::path::PathBuf::from("."),
        );
        for index in 0..8 {
            app.messages
                .push(test_user_message(&format!("message {index}")));
        }
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: std::path::Path::new("."),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/compact 4", &mut ctx)
            .expect("compact command should be handled")
            .expect("compact command should run")
            .expect("compact should show output");

        assert!(output.contains("已压缩本地上下文"));
        assert_eq!(ctx.app.messages.len(), 4);
        assert!(ctx.app.latest_compact_summary.is_some());
        assert_eq!(
            ctx.app.latest_compact_reason.as_deref(),
            Some("manual /compact")
        );
    }

    #[test]
    fn language_command_switches_display_language() {
        let temp = tempfile::tempdir().expect("tempdir");
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            temp.path().to_path_buf(),
        );
        app.config.ui.language = "zh-CN".to_string();
        app.welcome.display_language = app.config.ui.language.clone();
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: temp.path(),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/language en-US", &mut ctx)
            .expect("command should be handled")
            .expect("language should run")
            .expect("language should show output");

        assert!(output.contains("Language set to English"));
        assert_eq!(ctx.app.config.ui.language, "en-US");
        assert_eq!(ctx.app.welcome.display_language, "en-US");
        let local = std::fs::read_to_string(temp.path().join(".octocode/local.toml"))
            .expect("local config exists");
        assert!(local.contains("language = \"en-US\""));
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
    fn task_command_uses_project_todo_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            temp.path().to_path_buf(),
        );
        app.config.ui.language = "en-US".to_string();
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: temp.path(),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let created = reg
            .execute("/task create Ship P0.4", &mut ctx)
            .expect("command handled")
            .expect("task create runs")
            .expect("task create output");

        assert!(created.contains("Task created"));
        assert!(crate::tools::todo_state::todo_state_path(temp.path()).exists());
        assert_eq!(ctx.app.todo_summary.total, 1);

        let started = reg
            .execute("/task start task-1", &mut ctx)
            .expect("command handled")
            .expect("task start runs")
            .expect("task start output");

        assert!(started.contains("in_progress"));
        assert_eq!(ctx.app.todo_summary.in_progress, 1);

        let listed = reg
            .execute("/task list", &mut ctx)
            .expect("command handled")
            .expect("task list runs")
            .expect("task list output");

        assert!(listed.contains("manager task"));
        assert!(listed.contains("Ship P0.4"));
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

        assert!(output.contains("Octocode Doctor"));
        assert!(output.contains("Doctor is local-only in the TUI"));
    }

    #[test]
    fn theme_command_switches_current_app_theme() {
        let temp = tempfile::tempdir().expect("tempdir");
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            temp.path().to_path_buf(),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: temp.path(),
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
        let local = std::fs::read_to_string(temp.path().join(".octocode/local.toml"))
            .expect("local config");
        assert!(local.contains("theme = \"dark\""));
    }

    #[test]
    fn theme_command_accepts_auto() {
        let temp = tempfile::tempdir().expect("tempdir");
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            temp.path().to_path_buf(),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: temp.path(),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/theme auto", &mut ctx)
            .expect("command should be handled")
            .expect("theme should run")
            .expect("theme should show output");

        assert!(output.contains("auto"));
        assert_eq!(ctx.app.theme_mode, crate::tui::theme::ThemeMode::Auto);
        let local = std::fs::read_to_string(temp.path().join(".octocode/local.toml"))
            .expect("local config");
        assert!(local.contains("theme = \"auto\""));
    }

    #[test]
    fn exit_command_stops_tui_loop() {
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
            .execute("/quit", &mut ctx)
            .expect("command should be handled")
            .expect("exit should run")
            .expect("exit should show output");

        assert_eq!(output, "Goodbye.");
        assert!(!ctx.app.running);
    }

    #[test]
    fn tui_command_writes_renderer_override() {
        let temp = tempfile::tempdir().expect("tempdir");
        let reg = CommandRegistry::new();
        let mut app = crate::tui::app::TuiApp::new(
            crate::deepseek::DeepSeekModel::Flash,
            crate::deepseek::ThinkingMode::Auto,
            None,
            temp.path().to_path_buf(),
        );
        let mut yolo = false;
        let mut ctx = CommandContext {
            app: &mut app,
            project_root: temp.path(),
            yolo_mode: &mut yolo,
            mcp_status: "MCP: not initialized",
            background_tasks: &[],
        };

        let output = reg
            .execute("/tui fullscreen", &mut ctx)
            .expect("command should be handled")
            .expect("tui should run")
            .expect("tui should show output");

        assert!(output.contains("fullscreen"));
        assert_eq!(
            ctx.app.renderer_mode,
            crate::tui::app::RendererMode::Fullscreen
        );
        let local = std::fs::read_to_string(temp.path().join(".octocode/local.toml"))
            .expect("local config");
        assert!(local.contains("renderer = \"fullscreen\""));

        let output = reg
            .execute("/tui auto", &mut ctx)
            .expect("command should be handled")
            .expect("tui should run")
            .expect("tui should show output");

        assert!(output.contains("auto"));
        let local = std::fs::read_to_string(temp.path().join(".octocode/local.toml"))
            .expect("local config");
        assert!(local.contains("renderer = \"auto\""));
    }

    #[test]
    fn settings_command_opens_editable_panel() {
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
        assert!(
            ctx.app.status_message.contains("edits selected value")
                || ctx.app.status_message.contains("修改当前项")
        );
    }

    #[test]
    fn test_command_forwards_to_agent_tool_flow() {
        let output =
            execute_with_test_app("/test echo octocode-test").expect("test command should run");

        assert!(output.is_none());
        assert_eq!(
            forwarded_agent_input("/test echo octocode-test").as_deref(),
            Some("Run this test command through the normal tool approval flow: echo octocode-test")
        );
    }

    #[test]
    fn commit_command_forwards_to_agent_tool_flow() {
        let output = execute_with_test_app("/commit ship it").expect("commit command should run");

        assert!(output.is_none());
        assert_eq!(
            forwarded_agent_input("/commit ship it").as_deref(),
            Some("Stage the appropriate workspace changes and create a git commit with this message: ship it")
        );
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

    fn test_user_message(content: &str) -> crate::deepseek::ProtocolMessage {
        crate::deepseek::ProtocolMessage {
            id: crate::deepseek::MessageId::new_v4(),
            role: crate::deepseek::Role::User,
            content: crate::deepseek::MessageContent::from(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: crate::deepseek::TurnId::new_v4(),
            sub_turn_id: None,
            visibility: crate::deepseek::MessageVisibility::UserVisible,
        }
    }
}

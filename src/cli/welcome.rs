use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::cli::resolve_project_root;
use crate::deepseek::DeepSeekModel;
use crate::storage;

fn use_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Print a quiet terminal welcome page and enter interactive chat.
pub async fn welcome(
    project_root: Option<PathBuf>,
    thinking: bool,
    model_override: Option<String>,
    session: Option<String>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "welcome")?;

    let config = match storage::Config::load(Some(&root)) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("warning: failed to load config (using defaults): {e}");
            storage::Config::default()
        }
    };
    let model = match model_override.as_deref() {
        Some(value) => crate::provider::parse_model(value)
            .map_err(|error| anyhow::anyhow!("invalid model override: {error}"))?,
        None => config.model.default.canonical(),
    };

    // Collect status
    let api_key_ok = storage::get_effective_api_key(Some(&root)).is_some();
    let config_ok = storage::Config::load(Some(&root)).is_ok();
    let workspace_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(|name| {
            if name == concat!("deepseek", "-", "code") {
                "octocode"
            } else {
                name
            }
        })
        .unwrap_or("workspace");

    // Load recent sessions
    let recent_sessions = dirs::home_dir()
        .map(|home| storage::SessionStore::new(home.join(".octocode")).list(&root))
        .and_then(Result::ok)
        .unwrap_or_default()
        .into_iter()
        .take(3)
        .map(|s| {
            let label = s.name.unwrap_or_else(|| {
                let id = s.id.to_string();
                id.chars().take(8).collect()
            });
            let updated = s.updated_at.format("%m-%d %H:%M").to_string();
            (label, updated, s.message_count, s.tool_call_count)
        })
        .collect::<Vec<_>>();

    // Print welcome banner
    print_welcome(
        &root,
        workspace_name,
        &model,
        thinking,
        api_key_ok,
        config_ok,
        &recent_sessions,
    );

    if !api_key_ok {
        println!();
        super::login::prompt_and_store_api_key(Some(&root))?;
    }

    // Enter interactive chat loop (reuses existing chat logic)
    super::chat::chat(
        None,
        thinking,
        model_override,
        Some(root),
        session,
        crate::cli::TurnOutputFormat::Text,
        crate::cli::ToolApprovalPolicy::Ask,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
fn print_welcome(
    root: &Path,
    workspace_name: &str,
    model: &DeepSeekModel,
    thinking: bool,
    api_key_ok: bool,
    config_ok: bool,
    recent_sessions: &[(String, String, usize, usize)],
) {
    let version = env!("CARGO_PKG_VERSION");

    // Top border
    println!("{}", dim(&"━".repeat(56)));
    println!();

    // Title line
    println!("  {}   {}", cyan("OCTOCODE"), bold(&format!("v{version}")));
    println!();

    // Status tree
    println!("  {}", green("●  Ready"));
    println!("  {}  ", dim("│"));
    println!(
        "  {}  {}  {}",
        dim("│"),
        dim("Workspace:"),
        cyan(workspace_name)
    );
    println!(
        "  {}  {}  {}",
        dim("│"),
        dim("Path:     "),
        dim(&root.display().to_string())
    );
    println!(
        "  {}  {}  {}",
        dim("│"),
        dim("Model:    "),
        yellow(&model.to_string())
    );
    println!(
        "  {}  {}  {}",
        dim("│"),
        dim("Thinking: "),
        magenta(if thinking { "on" } else { "auto" })
    );
    println!("  {}  ", dim("│"));
    println!(
        "  {}  {}  {}",
        dim("│"),
        dim("API Key:  "),
        if api_key_ok {
            green("ready")
        } else {
            red("missing")
        }
    );
    println!(
        "  {}  {}  {}",
        dim("│"),
        dim("Config:   "),
        if config_ok {
            green("loaded")
        } else {
            dim("fallback")
        }
    );
    println!();

    // Recent sessions
    if !recent_sessions.is_empty() {
        println!("{}", dim(&"─".repeat(56)));
        println!();
        println!("  {}", bold("Recent sessions"));
        println!();
        for (i, (label, updated, msgs, tools)) in recent_sessions.iter().enumerate() {
            println!(
                "  {}  {}  {}  {}  {}",
                cyan(&format!("{}", i + 1)),
                white(label),
                dim(updated),
                dim(&format!("{msgs} msgs")),
                dim(&format!("/ {tools} tools")),
            );
        }
        println!();
    }

    // Suggested prompts
    println!("{}", dim(&"─".repeat(56)));
    println!();
    println!("  {}", bold("Try asking"));
    println!();
    for (i, prompt) in suggested_prompts().iter().enumerate() {
        println!("  {}  {}", cyan(&format!("{}", i + 1)), white(prompt));
    }
    println!();

    // Quick commands
    println!("{}", dim(&"─".repeat(56)));
    println!();
    println!("  {}", bold("Slash commands"));
    println!();
    println!(
        "    {}  {}",
        cyan("/plan"),
        dim("generate a plan before executing")
    );
    println!("    {}  {}", cyan("/run"), dim("execute with tool access"));
    println!("    {}  {}", cyan("/search"), dim("search the codebase"));
    println!("    {}  {}", cyan("/ask"), dim("ask a read-only question"));
    println!("    {}  {}", cyan("/exit"), dim("quit"));
    println!();

    // Hint about auto-routing
    println!(
        "  {} {}",
        dim("Just type your task and press Enter."),
        cyan("The router will judge complexity automatically.")
    );
    println!(
        "  {}",
        dim("Simple tasks run directly; complex tasks show a plan first.")
    );
    println!();

    // Bottom border
    println!("{}", dim(&"━".repeat(56)));
    println!();

    if !api_key_ok {
        println!(
            "  {}  API key not configured. Run: {}",
            yellow("!"),
            cyan("octo login --api-key <your-key>")
        );
        println!();
    }
}

fn suggested_prompts() -> Vec<&'static str> {
    vec![
        "解释这个项目的架构和入口",
        "查找路径保护和审批逻辑在哪里",
        "制定一个 Windows 发行版修复计划",
    ]
}

// ---------------------------------------------------------------------------
// ANSI helpers
// ---------------------------------------------------------------------------

fn bold(s: &str) -> String {
    if use_color() {
        format!("\x1b[1m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn white(s: &str) -> String {
    if use_color() {
        format!("\x1b[37m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn cyan(s: &str) -> String {
    if use_color() {
        format!("\x1b[36m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn yellow(s: &str) -> String {
    if use_color() {
        format!("\x1b[33m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn green(s: &str) -> String {
    if use_color() {
        format!("\x1b[32m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn red(s: &str) -> String {
    if use_color() {
        format!("\x1b[31m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn magenta(s: &str) -> String {
    if use_color() {
        format!("\x1b[35m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn dim(s: &str) -> String {
    if use_color() {
        format!("\x1b[2m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

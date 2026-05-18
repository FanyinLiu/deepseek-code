use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::resolve_project_root;
use crate::storage;

#[derive(Debug, Clone)]
pub enum ConfigCommand {
    Explain { json: bool },
}

#[derive(Debug, Serialize)]
struct ConfigExplainPayload {
    project_root: String,
    sources: Vec<ConfigSourcePayload>,
    effective: EffectiveConfigPayload,
}

#[derive(Debug, Serialize)]
struct ConfigSourcePayload {
    name: &'static str,
    path: String,
    exists: bool,
}

#[derive(Debug, Serialize)]
struct EffectiveConfigPayload {
    provider_default: String,
    model_default: String,
    model_heavy: String,
    thinking_mode: String,
    reasoning_effort: String,
    ui_theme: String,
    mcp_enabled: bool,
    mcp_servers: usize,
    hooks_user_prompt_submit: usize,
    hooks_pre_tool: usize,
    hooks_post_tool: usize,
    hooks_session_start: usize,
    hooks_session_end: usize,
    hooks_stop: usize,
    approval_write: bool,
    approval_command: bool,
    sandbox_mode: String,
    max_parallel_subagents: usize,
    api_key_configured: bool,
}

pub async fn config_cmd(
    command: ConfigCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "config")?;
    match command {
        ConfigCommand::Explain { json } => explain(&root, json),
    }
}

fn explain(root: &Path, json: bool) -> Result<(), anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    let payload = ConfigExplainPayload {
        project_root: root.display().to_string(),
        sources: config_sources(root),
        effective: EffectiveConfigPayload {
            provider_default: config.provider.default.as_str().to_string(),
            model_default: config.model.default.to_string(),
            model_heavy: config.model.heavy.to_string(),
            thinking_mode: config.model.thinking_mode.to_string(),
            reasoning_effort: config.model.reasoning_effort.to_string(),
            ui_theme: config.ui.theme,
            mcp_enabled: config.mcp.enabled,
            mcp_servers: config.mcp.servers.len(),
            hooks_user_prompt_submit: config.hooks.user_prompt_submit.len(),
            hooks_pre_tool: config.hooks.pre_tool.len(),
            hooks_post_tool: config.hooks.post_tool.len(),
            hooks_session_start: config.hooks.session_start.len(),
            hooks_session_end: config.hooks.session_end.len(),
            hooks_stop: config.hooks.stop.len(),
            approval_write: config.policy.require_approval_for_write,
            approval_command: config.policy.require_approval_for_command,
            sandbox_mode: config.policy.command_sandbox.mode.as_str().to_string(),
            max_parallel_subagents: config.subagent.max_parallel,
            api_key_configured: config.api_key.is_some(),
        },
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_human(&payload);
    }
    Ok(())
}

fn config_sources(root: &Path) -> Vec<ConfigSourcePayload> {
    let global = dirs::home_dir()
        .map(|home| home.join(".octocode").join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("~/.octocode/config.toml"));
    let project = root.join(".octocode").join("config.toml");
    let local = root.join(".octocode").join("local.toml");
    vec![
        source("user", global),
        source("project", project),
        source("local", local),
    ]
}

fn source(name: &'static str, path: PathBuf) -> ConfigSourcePayload {
    ConfigSourcePayload {
        name,
        exists: path.exists(),
        path: path.display().to_string(),
    }
}

fn print_human(payload: &ConfigExplainPayload) {
    println!("Config explain for {}", payload.project_root);
    println!("Sources (later entries override earlier ones):");
    for source in &payload.sources {
        println!(
            "  {:7} {} {}",
            source.name,
            if source.exists { "✓" } else { "○" },
            source.path
        );
    }
    println!();
    println!("Effective values:");
    println!(
        "  provider.default       {}",
        payload.effective.provider_default
    );
    println!(
        "  model.default          {}",
        payload.effective.model_default
    );
    println!("  model.heavy            {}", payload.effective.model_heavy);
    println!(
        "  model.thinking_mode    {}",
        payload.effective.thinking_mode
    );
    println!(
        "  model.reasoning_effort {}",
        payload.effective.reasoning_effort
    );
    println!("  ui.theme               {}", payload.effective.ui_theme);
    println!("  mcp.enabled            {}", payload.effective.mcp_enabled);
    println!("  mcp.servers            {}", payload.effective.mcp_servers);
    println!(
        "  hooks                  prompt={} pre={} post={} start={} end={} stop={}",
        payload.effective.hooks_user_prompt_submit,
        payload.effective.hooks_pre_tool,
        payload.effective.hooks_post_tool,
        payload.effective.hooks_session_start,
        payload.effective.hooks_session_end,
        payload.effective.hooks_stop
    );
    println!(
        "  approval               write={} command={}",
        payload.effective.approval_write, payload.effective.approval_command
    );
    println!(
        "  sandbox.mode           {}",
        payload.effective.sandbox_mode
    );
    println!(
        "  subagent.max_parallel  {}",
        payload.effective.max_parallel_subagents
    );
    println!(
        "  api_key                {}",
        if payload.effective.api_key_configured {
            "configured"
        } else {
            "not configured"
        }
    );
}

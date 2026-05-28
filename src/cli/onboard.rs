use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agent::subagent::SubagentRegistry;
use crate::provider::request_model_name_for_config;
use crate::storage;

#[derive(Debug, Clone, Copy, Default)]
pub struct OnboardOptions {
    pub json: bool,
    pub daemon_preview: bool,
}

#[derive(Debug, Serialize)]
struct OnboardReport {
    project_root: String,
    terminal_first: bool,
    status: OnboardStatus,
    checks: Vec<OnboardCheck>,
    next_steps: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum OnboardStatus {
    Ready,
    NeedsAttention,
}

#[derive(Debug, Serialize)]
struct OnboardCheck {
    name: &'static str,
    status: CheckStatus,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Pass,
    Warn,
    Info,
}

pub async fn onboard(
    project_root: Option<PathBuf>,
    options: OnboardOptions,
) -> Result<(), anyhow::Error> {
    let root = crate::cli::resolve_project_root_or_cwd(project_root);
    let report = build_report(&root, options)?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }

    Ok(())
}

fn build_report(root: &Path, options: OnboardOptions) -> Result<OnboardReport, anyhow::Error> {
    let mut checks = Vec::new();
    checks.push(project_root_check(root));
    checks.push(primary_command_check());

    let config = match storage::Config::load(Some(root)) {
        Ok(config) => {
            let kind = config.provider.default;
            checks.push(OnboardCheck {
                name: "config",
                status: CheckStatus::Pass,
                summary: format!(
                    "provider={} model={}",
                    kind.as_str(),
                    request_model_name_for_config(&config.provider, &config.model.default)
                ),
                detail: Some("resolved from ~/.octocode plus project .octocode config".to_string()),
            });
            Some((config, kind))
        }
        Err(error) => {
            checks.push(OnboardCheck {
                name: "config",
                status: CheckStatus::Warn,
                summary: "config could not be loaded".to_string(),
                detail: Some(error.to_string()),
            });
            None
        }
    };

    if let Some((config, kind)) = config.as_ref() {
        checks.push(api_key_check(root, config, *kind));
        checks.push(mcp_check(config));
    }

    checks.push(agent_check(root));
    checks.push(mission_check(root));
    checks.push(tui_check());

    if options.daemon_preview {
        checks.push(daemon_preview_check());
    }

    let status = if checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Warn))
    {
        OnboardStatus::NeedsAttention
    } else {
        OnboardStatus::Ready
    };

    let next_steps = next_steps(status, options.daemon_preview);

    Ok(OnboardReport {
        project_root: root.display().to_string(),
        terminal_first: true,
        status,
        checks,
        next_steps,
    })
}

fn project_root_check(root: &Path) -> OnboardCheck {
    let status = if root.is_dir() {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    OnboardCheck {
        name: "project_root",
        status,
        summary: root.display().to_string(),
        detail: Some(".octocode is the project-local config and runtime directory".to_string()),
    }
}

fn primary_command_check() -> OnboardCheck {
    OnboardCheck {
        name: "primary_command",
        status: CheckStatus::Pass,
        summary: "octo is the primary terminal command".to_string(),
        detail: Some("octocode remains a compatibility binary".to_string()),
    }
}

fn api_key_check(
    root: &Path,
    config: &storage::Config,
    kind: crate::provider::ProviderKind,
) -> OnboardCheck {
    let config_value = storage::config_api_key(config);
    let source = if storage::get_env_api_key_for_provider(kind).is_some() {
        Some("environment")
    } else if storage::get_keyring_api_key().is_some() {
        Some("system keyring")
    } else if config_value.is_some() {
        Some("config file")
    } else {
        None
    };

    if let Some(source) = source {
        OnboardCheck {
            name: "api_key",
            status: CheckStatus::Pass,
            summary: format!("API key available from {source}"),
            detail: Some("run `octo doctor --smoke` for live provider checks".to_string()),
        }
    } else if storage::get_effective_api_key(Some(root)).is_some() {
        OnboardCheck {
            name: "api_key",
            status: CheckStatus::Pass,
            summary: "API key available".to_string(),
            detail: Some("run `octo doctor --smoke` for live provider checks".to_string()),
        }
    } else {
        OnboardCheck {
            name: "api_key",
            status: CheckStatus::Warn,
            summary: "API key not configured".to_string(),
            detail: Some(format!(
                "set {} or run `octo login --api-key <key>`",
                storage::api_key_env_hint(kind)
            )),
        }
    }
}

fn mcp_check(config: &storage::Config) -> OnboardCheck {
    let server_count = config.mcp.servers.len();
    if config.mcp.enabled {
        OnboardCheck {
            name: "mcp",
            status: CheckStatus::Pass,
            summary: format!("enabled with {server_count} configured server(s)"),
            detail: Some("inspect with `octo mcp status`".to_string()),
        }
    } else {
        OnboardCheck {
            name: "mcp",
            status: CheckStatus::Info,
            summary: format!("disabled; {server_count} configured server(s)"),
            detail: Some("enable project tools in .octocode/config.toml when needed".to_string()),
        }
    }
}

fn agent_check(root: &Path) -> OnboardCheck {
    let registry = SubagentRegistry::load_from_project(root);
    let agent_count = registry.list().len();
    OnboardCheck {
        name: "agents",
        status: CheckStatus::Pass,
        summary: format!("{agent_count} built-in/custom agent(s) available"),
        detail: Some("validate custom agents with `octo agent validate --all`".to_string()),
    }
}

fn mission_check(root: &Path) -> OnboardCheck {
    OnboardCheck {
        name: "missions",
        status: CheckStatus::Pass,
        summary: "mission dry-runs are replayable from events.jsonl".to_string(),
        detail: Some(
            root.join(".octocode")
                .join("missions")
                .display()
                .to_string(),
        ),
    }
}

fn tui_check() -> OnboardCheck {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        OnboardCheck {
            name: "tui",
            status: CheckStatus::Pass,
            summary: "current process has an interactive terminal".to_string(),
            detail: Some("start with `octo` or `octo tui`".to_string()),
        }
    } else {
        OnboardCheck {
            name: "tui",
            status: CheckStatus::Warn,
            summary: "current process is not attached to an interactive terminal".to_string(),
            detail: Some(
                "run `octo` from Terminal.app/iTerm, or use `octo preview-tui` for snapshots"
                    .to_string(),
            ),
        }
    }
}

fn daemon_preview_check() -> OnboardCheck {
    if !cfg!(target_os = "macos") {
        return OnboardCheck {
            name: "daemon_preview",
            status: CheckStatus::Info,
            summary: "macOS launchd preview skipped on this platform".to_string(),
            detail: None,
        };
    }

    if command_exists("launchctl") {
        OnboardCheck {
            name: "daemon_preview",
            status: CheckStatus::Pass,
            summary: "launchctl is available for a future local gateway daemon".to_string(),
            detail: Some(
                "this command only previews readiness; it does not install launchd files"
                    .to_string(),
            ),
        }
    } else {
        OnboardCheck {
            name: "daemon_preview",
            status: CheckStatus::Warn,
            summary: "launchctl was not found in PATH".to_string(),
            detail: Some(
                "daemon installation should stay manual until launchd is available".to_string(),
            ),
        }
    }
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|path| {
                let candidate = path.join(name);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

fn next_steps(status: OnboardStatus, daemon_preview: bool) -> Vec<String> {
    let mut steps = Vec::new();
    if matches!(status, OnboardStatus::NeedsAttention) {
        steps.push("fix warned checks above".to_string());
    }
    steps.push("octo doctor --smoke".to_string());
    steps.push("octo agent validate --all".to_string());
    steps.push("octo tui".to_string());
    if daemon_preview {
        steps.push("keep daemon work terminal-driven; no web dashboard is required".to_string());
    }
    steps
}

fn print_report(report: &OnboardReport) {
    println!("Octo Onboard");
    println!();
    println!("scope      terminal-first local coding agent");
    println!("project    {}", report.project_root);
    println!("status     {}", status_label(report.status));
    println!();
    println!("Checks:");
    for check in &report.checks {
        println!(
            "  {:<6} {:<16} {}",
            check_status_label(check.status),
            check.name,
            check.summary
        );
        if let Some(detail) = &check.detail {
            println!("         {detail}");
        }
    }
    println!();
    println!("Next:");
    for (index, step) in report.next_steps.iter().enumerate() {
        println!("  {}. {step}", index + 1);
    }
}

fn status_label(status: OnboardStatus) -> &'static str {
    match status {
        OnboardStatus::Ready => "ready",
        OnboardStatus::NeedsAttention => "needs_attention",
    }
}

fn check_status_label(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "[ok]",
        CheckStatus::Warn => "[warn]",
        CheckStatus::Info => "[info]",
    }
}

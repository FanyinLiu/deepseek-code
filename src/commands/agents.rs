use super::{
    display_width, localized_manager_header, manager_header, on_off, pad_display,
    truncate_display_width, CommandContext, CommandResult,
};

pub(super) fn cmd_agents(_args: &str, ctx: &mut CommandContext) -> CommandResult {
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

pub(super) fn cmd_swarm(args: &str, ctx: &mut CommandContext) -> CommandResult {
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
        let content = crate::storage::read_text_file_capped(&path)
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
    crate::storage::atomic::write_text_atomic(&path, &rendered)
        .map_err(|e| format!("Failed to write local.toml: {e}"))?;
    Ok(())
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

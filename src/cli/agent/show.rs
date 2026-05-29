use std::path::Path;

use anyhow::bail;

use crate::agent::subagent::SubagentRegistry;

use super::payload::AgentShowPayload;
use super::{item_from_config, source_label};

pub fn show_payload(project_root: &Path, name: &str) -> Result<AgentShowPayload, anyhow::Error> {
    let registry = SubagentRegistry::load_from_project(project_root);
    let Some(config) = registry.get(name) else {
        bail!("unknown agent '{name}'");
    };

    Ok(AgentShowPayload {
        item: item_from_config(project_root, name, config),
        system_prompt: config.effective_system_prompt(),
    })
}

pub(super) fn print_agent_show(payload: &AgentShowPayload) {
    println!("Agent: {}", payload.item.name);
    println!("  Source: {}", source_label(payload.item.source));
    println!("  Description: {}", payload.item.description);
    println!("  Type: {}", payload.item.subagent_type);
    println!("  Permission mode: {}", payload.item.permission_mode);
    println!(
        "  Model: {}",
        payload.item.model.as_deref().unwrap_or("default")
    );
    println!("  Max turns: {}", payload.item.max_turns);
    if payload.item.allowed_tools.is_empty() {
        println!("  Allowed tools: all standard tools");
    } else {
        println!("  Allowed tools: {}", payload.item.allowed_tools.join(", "));
    }
    println!();
    println!("{}", payload.system_prompt);
}

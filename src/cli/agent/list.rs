use std::path::Path;

use crate::agent::subagent::SubagentRegistry;

use super::payload::AgentListPayload;
use super::{item_from_config, source_label};

pub fn list_payload(project_root: &Path) -> AgentListPayload {
    let registry = SubagentRegistry::load_from_project(project_root);
    let agents = registry
        .list()
        .into_iter()
        .filter_map(|name| {
            registry
                .get(name)
                .map(|config| item_from_config(name, config))
        })
        .collect();

    AgentListPayload {
        project_root: project_root.display().to_string(),
        agents,
    }
}

pub(super) fn print_agent_list(payload: &AgentListPayload) {
    println!("Agents for {}", payload.project_root);
    println!(
        "{:<24} {:<10} {:<18} {:<12} Model",
        "Name", "Source", "Type", "Mode"
    );
    println!("{}", "-".repeat(86));
    for agent in &payload.agents {
        println!(
            "{:<24} {:<10} {:<18} {:<12} {}",
            agent.name,
            source_label(agent.source),
            agent.subagent_type,
            agent.permission_mode,
            agent.model.as_deref().unwrap_or("-")
        );
    }
}

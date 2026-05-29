use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::deepseek::client::DeepSeekClient;

use super::background::BackgroundQueue;
use super::orchestrator::AgentEvent;
use super::subagent::{
    PermissionMode, SubagentConfig, SubagentTask, SubagentToolArgs, SubagentType,
};
use super::supervisor::Supervisor;
use super::swarm::SwarmCoordinator;

/// Handles `run_subagent` tool invocations from the main agent.
///
/// Supports both synchronous (wait for result) and background execution modes.
#[derive(Clone)]
pub struct TaskToolHandler {
    client: Arc<DeepSeekClient>,
    project_root: PathBuf,
    supervisor: Supervisor,
    background_queue: BackgroundQueue,
    /// The spawn_depth assigned to agents created by this handler.
    spawn_depth: u8,
}

impl TaskToolHandler {
    #[must_use]
    pub fn new(
        client: Arc<DeepSeekClient>,
        project_root: PathBuf,
        background_queue: BackgroundQueue,
        spawn_depth: u8,
    ) -> Self {
        let supervisor = Supervisor::new(client.clone(), project_root.clone());
        Self {
            client,
            project_root,
            supervisor,
            background_queue,
            spawn_depth,
        }
    }

    /// Handle a `run_subagent` tool call.
    ///
    /// If `args.background` is true, spawns the task on the background queue
    /// and returns a task ID immediately. Otherwise waits for completion.
    pub async fn handle(
        &self,
        args: &SubagentToolArgs,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        parent_context: Option<String>,
    ) -> (String, bool) {
        // Resolve config
        let mut config = self.supervisor.registry.resolve(&args.subagent_type);

        // Apply overrides from tool args
        if let Some(model) = &args.model {
            config.model = Some(model.clone());
        }
        if let Some(max_turns) = args.max_turns {
            config.max_turns = max_turns;
        }
        config.isolation = args.isolation;
        config.spawn_depth = self.spawn_depth;

        // Build task — merge optional parent context into the task context
        let merged_context = match (args.context.clone(), parent_context) {
            (Some(c), Some(p)) => Some(format!(
                "{p}
\n{c}"
            )),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        };
        let task = SubagentTask {
            description: args.description.clone(),
            prompt: args.prompt.clone(),
            context: merged_context,
            focus_files: args.focus_files.clone(),
            expected_output: None,
        };

        // Background execution: spawn and return task ID immediately
        if args.background {
            let mut bg_config = config.clone();
            bg_config.is_background = true;
            let task_id = self.background_queue.spawn(
                self.supervisor.clone(),
                bg_config,
                task,
                event_tx.clone(),
            );
            return (
                format!(
                    "Background subagent spawned. Task ID: {task_id}\nUse `/tasks` to check status."
                ),
                false,
            );
        }

        // Synchronous execution: route through the local swarm coordinator when enabled.
        let runtime_config =
            crate::storage::Config::load(Some(&self.project_root)).unwrap_or_default();
        if should_route_subagent_through_swarm(args, &config, runtime_config.subagent.swarm_enabled)
        {
            let coordinator = SwarmCoordinator::new(
                self.client.clone(),
                self.project_root.clone(),
                runtime_config.subagent.max_parallel,
            );
            let plan = coordinator.plan(&args.description, &args.prompt, &args.focus_files);
            let result = coordinator.run(plan, event_tx).await;
            return (result.format_for_parent(), !result.success);
        }

        // Fallback: legacy optional parallel decomposition.
        if let Some(batch) = self
            .supervisor
            .decompose_async(&args.description, &args.prompt)
            .await
        {
            if batch.tasks.len() > 1 && batch.independent {
                // Honor the decomposer's chosen merge strategy instead of always
                // synthesizing; capture it before `batch` is moved into the run.
                let merge_strategy = batch.merge_strategy.clone();
                let results = self.supervisor.run_parallel(batch, event_tx).await;
                let has_failed_subagent = results.iter().any(|result| !result.success);
                let merged = self
                    .supervisor
                    .synthesize_results(&results, &merge_strategy);
                return (merged, has_failed_subagent);
            }
        }

        // Single subagent execution
        let result = self.supervisor.run_single(config, task, event_tx).await;

        if result.success {
            let formatted = result.format_for_parent(4000);
            (formatted, false)
        } else {
            let err = result
                .error
                .clone()
                .unwrap_or_else(|| "Subagent failed without error details".to_string());
            (
                format!("Subagent failed: {}\n\nOutput:\n{}", err, result.output),
                true,
            )
        }
    }

    /// Get the background queue for status queries.
    #[must_use]
    pub fn background_queue(&self) -> &BackgroundQueue {
        &self.background_queue
    }
}

fn should_route_subagent_through_swarm(
    args: &SubagentToolArgs,
    config: &SubagentConfig,
    swarm_enabled: bool,
) -> bool {
    swarm_enabled
        && matches!(args.subagent_type, SubagentType::GeneralPurpose)
        && matches!(config.subagent_type, SubagentType::GeneralPurpose)
        && matches!(config.permission_mode, PermissionMode::Default)
        && config.allowed_tools.is_empty()
        && args.model.is_none()
        && args.max_turns.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subagent_args(subagent_type: SubagentType) -> SubagentToolArgs {
        SubagentToolArgs {
            description: "review project".to_string(),
            prompt: "review project".to_string(),
            subagent_type,
            context: None,
            focus_files: Vec::new(),
            model: None,
            max_turns: None,
            background: false,
            isolation: crate::agent::subagent::SubagentIsolation::None,
        }
    }

    #[test]
    fn swarm_route_keeps_explicit_agent_boundaries() {
        let auditor_args = subagent_args(SubagentType::SecurityAuditor);
        let auditor_config = SubagentConfig::for_type(SubagentType::SecurityAuditor);

        assert!(!should_route_subagent_through_swarm(
            &auditor_args,
            &auditor_config,
            true
        ));

        let general_args = subagent_args(SubagentType::GeneralPurpose);
        let general_config = SubagentConfig::for_type(SubagentType::GeneralPurpose);

        assert!(should_route_subagent_through_swarm(
            &general_args,
            &general_config,
            true
        ));
    }

    #[test]
    fn subagent_tool_args_deserializes_isolation_field() {
        let json = serde_json::json!({
            "description": "scan",
            "prompt": "scan",
            "isolation": "worktree"
        });
        let args: SubagentToolArgs = serde_json::from_value(json).expect("deserialize");
        assert!(matches!(
            args.isolation,
            crate::agent::subagent::SubagentIsolation::Worktree
        ));
    }

    #[test]
    fn subagent_tool_args_defaults_isolation_to_none() {
        let json = serde_json::json!({
            "description": "scan",
            "prompt": "scan"
        });
        let args: SubagentToolArgs = serde_json::from_value(json).expect("deserialize");
        assert!(matches!(
            args.isolation,
            crate::agent::subagent::SubagentIsolation::None
        ));
    }

    #[test]
    fn swarm_route_preserves_explicit_overrides() {
        let mut args = subagent_args(SubagentType::GeneralPurpose);
        let config = SubagentConfig::for_type(SubagentType::GeneralPurpose);

        args.max_turns = Some(1);

        assert!(!should_route_subagent_through_swarm(&args, &config, true));
    }
}

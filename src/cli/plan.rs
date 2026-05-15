use std::path::PathBuf;

use crate::agent::orchestrator::{AgentEvent, Orchestrator};
use crate::cli::output_blocks;
use crate::deepseek::{
    ReasoningEffort, ReasoningState, Session, SessionId, SessionMetadata, ThinkingMode,
};
use crate::provider::{build_provider, Provider};
use crate::storage;

/// Run the plan command: read-only analysis, no file modifications.
pub async fn plan(task: String, project_root: Option<PathBuf>) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));
    let api_key = super::login::resolve_or_prompt_api_key(Some(&root))?;
    let config = crate::storage::Config::load(Some(&root))?;
    let provider = build_provider(&config.provider, api_key);
    let client = provider.create_deepseek_client();

    output_blocks::print_header("plan mode", output_blocks::BlockStatus::Running);
    output_blocks::print_kv("project", root.display().to_string());
    output_blocks::print_kv("task", &task);

    let session = Session {
        id: SessionId::new_v4(),
        name: None,
        project_root: root.clone(),
        messages: Vec::new(),
        reasoning_state: ReasoningState {
            mode: ThinkingMode::On,
            effort: ReasoningEffort::Max,
            selected_model: Some(config.model.heavy.canonical()),
            ..Default::default()
        },
        tool_call_history: Vec::new(),
        checkpoints: Vec::new(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        metadata: SessionMetadata::default(),
    };

    let mut orchestrator = Orchestrator::new(client, root, session);
    orchestrator.init_mcp(&config.mcp).await;

    // Prepend /plan to trigger plan mode classification
    let plan_input = format!("/plan {task}");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let turn_handle = tokio::spawn(async move {
        let result = orchestrator.run_turn(&plan_input, tx).await;
        result.map(|_| orchestrator)
    });

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::ContentDelta(text) => print!("{text}"),
            AgentEvent::Error(e) => eprintln!("\nError: {e}"),
            AgentEvent::PlanStarted { summary, total } => {
                output_blocks::print_plan_started(&summary, total);
            }
            AgentEvent::PlanStepUpdate {
                index,
                total,
                description,
                status,
            } => {
                output_blocks::print_plan_step(index, total, &description, status);
            }
            AgentEvent::OptionsNeeded {
                kind: _,
                title,
                options,
                respond,
            } => {
                output_blocks::print_option_block(&title, &options);
                let preview_choice = plan_preview_choice(&options);
                println!(
                    "CLI plan is read-only; selecting {}. {}",
                    preview_choice + 1,
                    options
                        .get(preview_choice)
                        .map(String::as_str)
                        .unwrap_or("Cancel")
                );
                let _ = respond.send(preview_choice);
            }
            AgentEvent::TurnComplete { .. } => {}
            AgentEvent::PlanCleared => {}
            _ => {}
        }
    }

    let _returned_orchestrator = turn_handle.await??;

    output_blocks::print_header("plan mode complete", output_blocks::BlockStatus::Done);
    output_blocks::print_kv("next", "review the plan above before executing");
    output_blocks::print_kv("run", "ds run \"<your approved steps>\"");

    Ok(())
}

fn plan_preview_choice(options: &[String]) -> usize {
    options
        .iter()
        .position(|option| {
            let english = option.to_ascii_lowercase();
            english.contains("preview")
                || english.contains("show")
                || option.contains("预览")
                || option.contains("显示")
                || option.contains("查看")
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::plan_preview_choice;

    #[test]
    fn plan_preview_choice_handles_chinese_options() {
        let options = vec![
            "执行计划".to_string(),
            "预览计划".to_string(),
            "取消".to_string(),
        ];

        assert_eq!(plan_preview_choice(&options), 1);
    }

    #[test]
    fn plan_preview_choice_does_not_default_to_cancel() {
        let options = vec!["继续".to_string(), "取消".to_string()];

        assert_eq!(plan_preview_choice(&options), 0);
    }
}

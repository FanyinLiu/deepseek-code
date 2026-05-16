use std::path::PathBuf;

use crate::agent::orchestrator::{AgentEvent, Orchestrator};
use crate::cli::output_blocks;
use crate::cli::{resolve_project_root, TurnOutputFormat};
use crate::deepseek::{
    ExecutionLane, ReasoningState, Session, SessionId, SessionMetadata, ThinkingMode,
};
use crate::provider::{build_provider, ModelSelection, Provider};

/// Run the run command: execute a task with tool access and approval.
pub async fn run(
    task: String,
    thinking: bool,
    project_root: Option<PathBuf>,
    output_format: TurnOutputFormat,
) -> Result<(), anyhow::Error> {
    let root = match resolve_project_root(project_root, "run") {
        Ok(root) => root,
        Err(error) => return json_error_result(output_format, error),
    };
    let api_key = match if output_format.is_json() {
        super::login::resolve_api_key_non_interactive(Some(&root))
    } else {
        super::login::resolve_or_prompt_api_key(Some(&root))
    } {
        Ok(api_key) => api_key,
        Err(error) => return json_error_result(output_format, error),
    };
    let config = match crate::storage::Config::load(Some(&root)) {
        Ok(config) => config,
        Err(error) => return json_error_result(output_format, error),
    };
    let provider = build_provider(&config.provider, api_key);
    let client = provider.create_deepseek_client();
    let model = match ModelSelection::resolve(&config.provider, &config.model, None) {
        Ok(selection) => selection.model,
        Err(error) => return json_error_result(output_format, error.into()),
    };

    let session = Session {
        id: SessionId::new_v4(),
        name: None,
        project_root: root.clone(),
        messages: Vec::new(),
        reasoning_state: ReasoningState {
            mode: if thinking {
                ThinkingMode::On
            } else {
                ThinkingMode::Auto
            },
            selected_model: Some(model),
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
    let mut final_json = output_format
        .is_json()
        .then(|| crate::cli::stream_json::FinalJsonCollector::new(orchestrator.session.id));
    if output_format.is_stream_json() {
        crate::cli::stream_json::print_session_started(
            orchestrator.session.id,
            &orchestrator.session.project_root,
        );
    }

    // Spawn the turn in the background so approval events can be handled
    // concurrently instead of deadlocking.
    let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
    let task_for_turn = task.clone();
    let mut orch_for_turn = orchestrator;
    let turn_handle = tokio::spawn(async move {
        let result = orch_for_turn
            .run_turn_forced(&task_for_turn, ev_tx, ExecutionLane::ToolLoopThinking)
            .await;
        (orch_for_turn, result)
    });

    let mut auto_approve_session = false;

    while let Some(event) = ev_rx.recv().await {
        if let Some(collector) = final_json.as_mut() {
            collector.observe(&event);
            match event {
                AgentEvent::ToolApprovalNeeded { respond, .. }
                | AgentEvent::SubagentToolApprovalNeeded { respond, .. } => {
                    let _ = respond.send(false);
                }
                AgentEvent::OptionsNeeded { respond, .. } => {
                    let _ = respond.send(0);
                }
                _ => {}
            }
            continue;
        }
        if output_format.is_stream_json() {
            crate::cli::stream_json::print_event(&event);
            match event {
                AgentEvent::ToolApprovalNeeded { respond, .. }
                | AgentEvent::SubagentToolApprovalNeeded { respond, .. } => {
                    let _ = respond.send(false);
                }
                AgentEvent::OptionsNeeded { respond, .. } => {
                    let _ = respond.send(0);
                }
                _ => {}
            }
            continue;
        }
        match event {
            AgentEvent::UserMessage { .. } => {}
            AgentEvent::ContentDelta(text) => print!("{text}"),
            AgentEvent::ReasoningDelta(_) => {}
            AgentEvent::TokenDelta { .. } => {}
            AgentEvent::ToolApprovalNeeded {
                tool_name,
                display,
                respond,
            } => {
                let approved = if auto_approve_session {
                    output_blocks::print_approval(&tool_name, &display, true);
                    true
                } else {
                    output_blocks::print_approval(&tool_name, &display, false);
                    print!("Approve? [a]once [s]session [d]eny: ");
                    let mut answer = String::new();
                    std::io::stdin().read_line(&mut answer)?;
                    match answer.trim().to_lowercase().as_str() {
                        "a" | "yes" | "y" => {
                            println!("Approved once");
                            true
                        }
                        "s" => {
                            println!("Approved for session");
                            auto_approve_session = true;
                            true
                        }
                        _ => {
                            println!("Denied — skipping");
                            false
                        }
                    }
                };
                let _ = respond.send(approved);
            }
            AgentEvent::ToolStarted { .. } => {}
            AgentEvent::ToolExecuted {
                tool_name,
                success,
                summary,
            } => {
                output_blocks::print_tool_result(&tool_name, success, &summary);
            }
            AgentEvent::HookExecuted { .. } => {}
            AgentEvent::StreamDone { usage, cache, .. } => {
                if let Some(u) = usage {
                    println!(
                        "\n— Tokens: {} (prompt: {} + completion: {})",
                        u.total_tokens, u.prompt_tokens, u.completion_tokens
                    );
                    if let Some(c) = cache {
                        println!(
                            "  Cache: hit={} miss={} rate={:.0}%",
                            c.prompt_cache_hit_tokens,
                            c.prompt_cache_miss_tokens,
                            c.hit_rate() * 100.0
                        );
                    }
                }
            }
            AgentEvent::ComplexityAssessed { assessment } => {
                println!("\n[Complexity] {}", assessment.display_summary());
            }
            AgentEvent::ClarificationNeeded { questions } => {
                println!("\n[Clarification needed] More information is required:");
                for (i, q) in questions.iter().enumerate() {
                    println!("  {}. {}", i + 1, q);
                }
            }
            AgentEvent::Error(e) => eprintln!("\nError: {e}"),
            AgentEvent::TurnComplete {
                session_id,
                total_tokens,
            } => {
                output_blocks::print_task_complete(&session_id.to_string(), total_tokens);
            }
            AgentEvent::SubagentStarted {
                agent_id,
                agent_type,
                description,
                ..
            } => {
                output_blocks::print_worker_started(&agent_id, &agent_type, &description);
            }
            AgentEvent::SubagentDelta { agent_id, content } => {
                print!("\n[Subagent {agent_id}] {content}");
            }
            AgentEvent::SubagentCompleted { agent_id, result } => {
                output_blocks::print_worker_completed(&agent_id, &result);
            }
            AgentEvent::SubagentToolApprovalNeeded {
                agent_id,
                tool_name,
                arguments,
                respond,
            } => {
                output_blocks::print_header(
                    &format!("agent tool {}", output_blocks::truncate(&tool_name, 48)),
                    output_blocks::BlockStatus::Running,
                );
                output_blocks::print_kv("agent", &agent_id);
                output_blocks::print_kv("args", output_blocks::truncate(&arguments, 120));
                let approved = if auto_approve_session {
                    output_blocks::print_kv("action", "approved by session trust");
                    true
                } else {
                    print!("Approve subagent tool? [y/n]: ");
                    let mut answer = String::new();
                    let _ = std::io::stdin().read_line(&mut answer);
                    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes" | "a")
                };
                let _ = respond.send(approved);
            }
            AgentEvent::PlanStepUpdate {
                index,
                total,
                description,
                status,
            } => {
                output_blocks::print_plan_step(index, total, &description, status);
            }
            AgentEvent::PlanStarted { summary, total } => {
                output_blocks::print_plan_started(&summary, total);
            }
            AgentEvent::PlanCleared => {
                output_blocks::print_plan_complete();
            }
            AgentEvent::PlanReviewWarnings { warnings } => {
                for w in &warnings {
                    eprintln!("⚠ {w}");
                }
            }
            AgentEvent::SwarmStarted { summary, total, .. } => {
                output_blocks::print_header("swarm", output_blocks::BlockStatus::Running);
                output_blocks::print_kv("summary", &summary);
                output_blocks::print_kv("agents", total.to_string());
            }
            AgentEvent::SwarmTaskUpdated {
                role,
                status,
                description,
                ..
            } => {
                output_blocks::print_kv(
                    &format!("swarm {role}"),
                    format!("{status}: {description}"),
                );
            }
            AgentEvent::SwarmFinished {
                success, summary, ..
            } => {
                output_blocks::print_header(
                    "swarm",
                    if success {
                        output_blocks::BlockStatus::Done
                    } else {
                        output_blocks::BlockStatus::Failed
                    },
                );
                output_blocks::print_kv("summary", &summary);
            }
            AgentEvent::FileDiff { path, stats, .. } => {
                output_blocks::print_header("diff", output_blocks::BlockStatus::Done);
                output_blocks::print_kv("path", path);
                output_blocks::print_kv("stats", stats);
            }
            AgentEvent::OptionsNeeded {
                kind: _,
                title,
                options,
                respond,
            } => {
                output_blocks::print_option_block(&title, &options);
                println!("Select option (1–{}): ", options.len());
                let mut answer = String::new();
                let _ = std::io::stdin().read_line(&mut answer);
                let choice = answer
                    .trim()
                    .parse::<usize>()
                    .unwrap_or(options.len())
                    .saturating_sub(1);
                let _ = respond.send(choice);
            }
        }
    }

    let mut turn_error: Option<anyhow::Error> = None;
    match turn_handle.await {
        Ok((returned_orch, result)) => {
            let _ = returned_orch;
            if let Err(error) = result {
                if let Some(collector) = final_json.as_mut() {
                    collector.record_error(error.to_string());
                }
                turn_error = Some(error);
            }
        }
        Err(e) => {
            let error = anyhow::anyhow!("Turn task failed: {e}");
            if let Some(collector) = final_json.as_mut() {
                collector.record_error(error.to_string());
            }
            turn_error = Some(error);
        }
    }

    if let Some(collector) = final_json {
        collector.print_final();
        if collector.has_error() {
            anyhow::bail!("{}", collector.error_message().unwrap_or("turn failed"));
        }
    }

    if let Some(error) = turn_error {
        return Err(error);
    }

    Ok(())
}

fn json_error_result<T>(
    output_format: TurnOutputFormat,
    error: anyhow::Error,
) -> Result<T, anyhow::Error> {
    if output_format.is_json() {
        crate::cli::stream_json::print_final_error(error.to_string());
    }
    Err(error)
}

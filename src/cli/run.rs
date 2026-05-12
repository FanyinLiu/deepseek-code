use std::path::PathBuf;

use crate::agent::orchestrator::{AgentEvent, Orchestrator};
use crate::cli::output_blocks;
use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::{
    ExecutionLane, ReasoningState, Session, SessionId, SessionMetadata, ThinkingMode,
};
use crate::storage;

/// Run the run command: execute a task with tool access and approval.
pub async fn run(
    task: String,
    thinking: bool,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));
    let api_key = super::login::resolve_or_prompt_api_key(Some(&root))?;
    let client = DeepSeekClient::new(api_key);

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
            ..Default::default()
        },
        tool_call_history: Vec::new(),
        checkpoints: Vec::new(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        metadata: SessionMetadata::default(),
    };

    let mut orchestrator = Orchestrator::new(client, root, session);
    let config = crate::storage::Config::load(Some(&orchestrator.project_root)).unwrap_or_default();
    orchestrator.init_mcp(&config.mcp).await;

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
        match event {
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
            AgentEvent::ToolExecuted {
                tool_name,
                success,
                summary,
            } => {
                output_blocks::print_tool_result(&tool_name, success, &summary);
            }
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

    match turn_handle.await {
        Ok((returned_orch, result)) => {
            let _ = returned_orch;
            result?;
        }
        Err(e) => anyhow::bail!("Turn task failed: {e}"),
    }

    Ok(())
}

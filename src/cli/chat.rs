use std::path::PathBuf;

use crate::agent::orchestrator::{AgentEvent, Orchestrator};
use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::{
    DeepSeekModel, ReasoningState, Session, SessionId, SessionMetadata, ThinkingMode,
};
use crate::storage;

/// Run the chat command: one-shot or interactive conversation.
pub async fn chat(
    prompt: Option<String>,
    thinking: bool,
    model_override: Option<String>,
    project_root: Option<PathBuf>,
    session_id: Option<String>,
) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));
    let api_key = super::login::resolve_or_prompt_api_key(Some(&root))?;
    let client = DeepSeekClient::new(api_key);

    // Resolve model
    let model = match model_override.as_deref() {
        Some("pro" | "v4-pro") => DeepSeekModel::Pro,
        Some("flash" | "v4-flash") | None => DeepSeekModel::Flash,
        Some(other) => {
            if let Some(m) = crate::deepseek::migration::migrate_model_name(other) {
                m
            } else {
                eprintln!("Unknown model: {other}. Using flash.");
                DeepSeekModel::Flash
            }
        }
    };

    // Create or load session
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;
    let store = storage::SessionStore::new(home.join(".deepseek-code"));

    let session = if let Some(ref sid) = session_id {
        let sid = uuid::Uuid::parse_str(sid)?;
        match store.load(&root, &sid) {
            Ok(s) => {
                eprintln!(
                    "Resumed session: {} ({} messages, {} tool calls)",
                    sid,
                    s.messages.len(),
                    s.tool_call_history.len()
                );
                s
            }
            Err(e) => {
                anyhow::bail!("Failed to load session {sid}: {e}");
            }
        }
    } else {
        Session {
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
        }
    };

    let mut orchestrator = Some(Orchestrator::new(client, root, session));
    if let Some(ref mut orch) = orchestrator {
        let config = crate::storage::Config::load(Some(&orch.project_root)).unwrap_or_default();
        orch.init_mcp(&config.mcp).await;
    }

    if let Some(p) = prompt {
        // One-shot mode: spawn the turn so approval events are handled
        // concurrently rather than deadlocking.
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut orch = orchestrator.take().expect("orchestrator already consumed");
        let p_for_turn = p.clone();
        let turn_handle = tokio::spawn(async move {
            let result = orch.run_turn(&p_for_turn, ev_tx).await;
            (orch, result)
        });

        while let Some(event) = ev_rx.recv().await {
            match event {
                AgentEvent::ContentDelta(text) => print!("{text}"),
                AgentEvent::ReasoningDelta(_) => {}
                AgentEvent::ToolApprovalNeeded {
                    tool_name,
                    display,
                    respond,
                } => {
                    println!(
                        "\n[Tool: {} — Risk: {}] {}",
                        display.title, display.risk_level, display.description
                    );
                    if !display.details.is_empty() {
                        println!("{}", display.details);
                    }
                    // In one-shot mode, only auto-approve safe read operations.
                    let is_safe_read = crate::policy::is_safe_read_tool(&tool_name);
                    let _ = respond.send(is_safe_read);
                    if !is_safe_read {
                        println!(
                            "[Denied — use `deepseek-code run` for interactive tool approval]"
                        );
                    }
                }
                AgentEvent::ToolExecuted {
                    tool_name,
                    success,
                    summary,
                } => {
                    println!(
                        "[Tool done: {} — {}] {}",
                        tool_name,
                        if success { "ok" } else { "error" },
                        summary
                    );
                }
                AgentEvent::StreamDone { usage, cache, .. } => {
                    if let Some(u) = usage {
                        println!(
                            "\n[Tokens: {} | Cache hit: {}]",
                            u.total_tokens,
                            cache.map_or_else(
                                || "N/A".into(),
                                |c| format!("{:.0}%", c.hit_rate() * 100.0)
                            )
                        );
                    }
                }
                AgentEvent::ComplexityAssessed { assessment } => {
                    println!("\n[Complexity] {}", assessment.display_summary());
                }
                AgentEvent::ClarificationNeeded { questions } => {
                    println!("\n[Clarification needed]");
                    for q in &questions {
                        println!("  • {q}");
                    }
                }
                AgentEvent::Error(e) => eprintln!("\nError: {e}"),
                AgentEvent::TurnComplete {
                    session_id,
                    total_tokens,
                } => {
                    println!("\n[Session: {session_id} | Tokens: {total_tokens}]");
                }
                AgentEvent::SubagentStarted {
                    agent_id,
                    agent_type,
                    description,
                    ..
                } => {
                    println!("\n[Subagent {agent_id} — {agent_type}] {description}");
                }
                AgentEvent::SubagentDelta { agent_id, content } => {
                    print!("\n[Subagent {agent_id}] {content}");
                }
                AgentEvent::SubagentCompleted { agent_id, result } => {
                    println!(
                        "\n[Subagent {} completed] success={}",
                        agent_id, result.success
                    );
                }
                AgentEvent::SubagentToolApprovalNeeded {
                    agent_id,
                    tool_name,
                    arguments,
                    respond,
                } => {
                    println!("\n[Subagent {agent_id} tool: {tool_name}] {arguments}");
                    let is_safe_read = crate::policy::is_safe_read_tool(&tool_name);
                    let _ = respond.send(is_safe_read);
                    if !is_safe_read {
                        println!(
                            "[Denied — use `deepseek-code run` for interactive tool approval]"
                        );
                    }
                }
                AgentEvent::PlanStepUpdate {
                    index,
                    total,
                    description,
                    status,
                } => {
                    let icon = match status {
                        crate::agent::orchestrator::PlanStepStatus::Pending => "○",
                        crate::agent::orchestrator::PlanStepStatus::Running => "◈",
                        crate::agent::orchestrator::PlanStepStatus::Done => "◆",
                        crate::agent::orchestrator::PlanStepStatus::Failed => "✗",
                    };
                    println!("\n[{icon} Step {}/{}] {}", index + 1, total, description);
                }
                AgentEvent::PlanStarted { summary, total } => {
                    println!("\n[Plan] {summary} ({total} steps)");
                }
                AgentEvent::PlanCleared => {
                    println!("\n[Plan complete]");
                }
                AgentEvent::PlanReviewWarnings { warnings } => {
                    for w in &warnings {
                        eprintln!("⚠ {w}");
                    }
                }
                AgentEvent::SwarmStarted { summary, total, .. } => {
                    println!("\n[Swarm] {summary} ({total} agents)");
                }
                AgentEvent::SwarmTaskUpdated {
                    role,
                    status,
                    description,
                    ..
                } => {
                    println!("\n[Swarm {role}] {status}: {description}");
                }
                AgentEvent::SwarmFinished {
                    success, summary, ..
                } => {
                    println!(
                        "\n[Swarm complete: {}] {summary}",
                        if success { "ok" } else { "error" }
                    );
                }
                AgentEvent::FileDiff { path, stats, .. } => {
                    println!("\n[Diff: {path}] {stats}");
                }
                AgentEvent::OptionsNeeded {
                    kind: _,
                    title,
                    options,
                    respond,
                } => {
                    println!("\n{title}");
                    for (i, opt) in options.iter().enumerate() {
                        println!("  {}. {opt}", i + 1);
                    }
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
                result?;
                orchestrator = Some(returned_orch);
            }
            Err(e) => anyhow::bail!("Turn task failed: {e}"),
        }

        // Save session after one-shot
        if let Some(ref o) = orchestrator {
            let home =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;
            let store = storage::SessionStore::new(home.join(".deepseek-code"));
            store.save(&o.session)?;
        }
    } else {
        // Interactive mode
        println!("DeepSeek-Code interactive chat (Ctrl+C to exit, /help for commands)");
        println!("Model: {model} | Thinking: {thinking}");

        loop {
            print!("> ");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim().to_string();

            if input.is_empty() {
                continue;
            }
            if input == "/exit" || input == "/quit" {
                break;
            }
            if input == "/help" {
                println!("Commands:");
                println!("  /exit, /quit  — exit");
                println!("  /plan <task> — enter plan mode");
                println!("  /resume      — list saved sessions");
                println!("  /doctor      — run diagnostics");
                println!("  /tasks       — list background subagent tasks");
                continue;
            }
            if input == "/tasks" {
                if let Some(ref o) = orchestrator {
                    let tasks = o.background_tasks();
                    if tasks.is_empty() {
                        println!("No background tasks.");
                    } else {
                        println!("Background tasks ({}):", tasks.len());
                        for task in &tasks {
                            println!("{}", task.format_for_display());
                        }
                    }
                }
                continue;
            }

            // Spawn the turn in the background so we can handle approval events
            // concurrently instead of deadlocking when the orchestrator waits
            // for a response while we are still awaiting run_turn.
            let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
            let input_for_turn = input.clone();
            let mut orch = orchestrator.take().expect("orchestrator already consumed");
            let turn_handle = tokio::spawn(async move {
                let result = orch.run_turn(&input_for_turn, ev_tx).await;
                (orch, result)
            });

            let mut auto_approve_session = false;
            while let Some(event) = ev_rx.recv().await {
                match event {
                    AgentEvent::ContentDelta(text) => print!("{text}"),
                    AgentEvent::ComplexityAssessed { assessment } => {
                        println!("\n[Complexity] {}", assessment.display_summary());
                    }
                    AgentEvent::ClarificationNeeded { questions } => {
                        println!("\n[Clarification needed]");
                        for q in &questions {
                            println!("  • {q}");
                        }
                    }
                    AgentEvent::ToolApprovalNeeded {
                        tool_name,
                        display,
                        respond,
                    } => {
                        let approved = if auto_approve_session {
                            println!("\n[Auto-approved: {tool_name} — {}]", display.risk_level);
                            true
                        } else {
                            println!();
                            println!("{}", "─".repeat(50));
                            println!("Tool: {}", display.title);
                            println!("Risk: {}", display.risk_level);
                            println!("Action: {}", display.description);
                            if !display.details.is_empty() {
                                println!("Details:\n{}", display.details);
                            }
                            print!("Approve? [a]once [s]session [d]eny: ");
                            let mut answer = String::new();
                            let _ = std::io::stdin().read_line(&mut answer);
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
                    AgentEvent::SubagentToolApprovalNeeded {
                        agent_id,
                        tool_name,
                        arguments,
                        respond,
                    } => {
                        let approved = if auto_approve_session {
                            println!("\n[Auto-approved subagent {agent_id}: {tool_name}]");
                            true
                        } else {
                            println!();
                            println!("{}", "─".repeat(50));
                            println!("Subagent {agent_id} tool: {tool_name}");
                            println!("Args: {arguments}");
                            print!("Approve? [a]once [s]session [d]eny: ");
                            let mut answer = String::new();
                            let _ = std::io::stdin().read_line(&mut answer);
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
                        tool_name, success, ..
                    } => {
                        if success {
                            println!("\n  [{tool_name} ✓]");
                        } else {
                            println!("\n  [{tool_name} ✗]");
                        }
                    }
                    AgentEvent::ReasoningDelta(_) => {}
                    AgentEvent::Error(e) => eprintln!("\nError: {e}"),
                    AgentEvent::TurnComplete { .. } => {}
                    AgentEvent::StreamDone { .. } => {}
                    AgentEvent::SubagentStarted {
                        agent_id,
                        agent_type,
                        description,
                        ..
                    } => {
                        println!("\n[Subagent {agent_id} — {agent_type}] {description}");
                    }
                    AgentEvent::SubagentDelta { agent_id, content } => {
                        print!("\n[Subagent {agent_id}] {content}");
                    }
                    AgentEvent::SubagentCompleted { agent_id, result } => {
                        println!(
                            "\n[Subagent {} completed] success={}",
                            agent_id, result.success
                        );
                    }
                    AgentEvent::PlanStepUpdate {
                        index,
                        total,
                        description,
                        status,
                    } => {
                        let icon = match status {
                            crate::agent::orchestrator::PlanStepStatus::Pending => "○",
                            crate::agent::orchestrator::PlanStepStatus::Running => "◈",
                            crate::agent::orchestrator::PlanStepStatus::Done => "◆",
                            crate::agent::orchestrator::PlanStepStatus::Failed => "✗",
                        };
                        println!("\n[{icon} Step {}/{}] {}", index + 1, total, description);
                    }
                    AgentEvent::PlanStarted { summary, total } => {
                        println!("\n[Plan] {summary} ({total} steps)");
                    }
                    AgentEvent::PlanCleared => {
                        println!("\n[Plan complete]");
                    }
                    AgentEvent::PlanReviewWarnings { warnings } => {
                        for w in &warnings {
                            eprintln!("⚠ {w}");
                        }
                    }
                    AgentEvent::SwarmStarted { summary, total, .. } => {
                        println!("\n[Swarm] {summary} ({total} agents)");
                    }
                    AgentEvent::SwarmTaskUpdated {
                        role,
                        status,
                        description,
                        ..
                    } => {
                        println!("\n[Swarm {role}] {status}: {description}");
                    }
                    AgentEvent::SwarmFinished {
                        success, summary, ..
                    } => {
                        println!(
                            "\n[Swarm complete: {}] {summary}",
                            if success { "ok" } else { "error" }
                        );
                    }
                    AgentEvent::FileDiff { path, stats, .. } => {
                        println!("\n[Diff: {path}] {stats}");
                    }
                    AgentEvent::OptionsNeeded {
                        kind: _,
                        title,
                        options,
                        respond,
                    } => {
                        println!("\n{title}");
                        for (i, opt) in options.iter().enumerate() {
                            println!("  {}. {opt}", i + 1);
                        }
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
                Ok((returned_orch, _)) => {
                    orchestrator = Some(returned_orch);
                }
                Err(e) => {
                    eprintln!("Turn task failed: {e}");
                    break;
                }
            }

            // Save session after each interactive turn
            if let Some(ref o) = orchestrator {
                let home = dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;
                let store = storage::SessionStore::new(home.join(".deepseek-code"));
                let _ = store.save(&o.session);
            }
            println!();
        }
    }

    Ok(())
}

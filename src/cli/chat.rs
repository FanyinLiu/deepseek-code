use std::path::PathBuf;

use crate::agent::orchestrator::{AgentEvent, Orchestrator};
use crate::cli::approval;
use crate::cli::inline_popup;
use crate::cli::{resolve_project_root, ToolApprovalPolicy, TurnOutputFormat};
use crate::deepseek::{ReasoningState, Session, SessionId, SessionMetadata, ThinkingMode};
use crate::provider::{build_provider, ModelSelection, Provider};
use crate::storage;

/// Run the chat command: one-shot or interactive conversation.
#[allow(clippy::too_many_arguments)]
pub async fn chat(
    prompt: Option<String>,
    thinking: bool,
    model_override: Option<String>,
    project_root: Option<PathBuf>,
    session_id: Option<String>,
    output_format: TurnOutputFormat,
    tool_approval: ToolApprovalPolicy,
    approval_mode: Option<crate::policy::PermissionMode>,
) -> Result<(), anyhow::Error> {
    let root = match resolve_project_root(project_root, "chat") {
        Ok(root) => root,
        Err(error) => {
            if output_format.is_machine_readable() {
                crate::cli::stream_json::print_output_error(output_format, error.to_string());
            }
            return Err(error);
        }
    };
    let api_key = match if output_format.is_machine_readable() {
        super::login::resolve_api_key_non_interactive(Some(&root))
    } else {
        super::login::resolve_or_prompt_api_key(Some(&root))
    } {
        Ok(api_key) => api_key,
        Err(error) => {
            if output_format.is_machine_readable() {
                crate::cli::stream_json::print_output_error(output_format, error.to_string());
            }
            return Err(error);
        }
    };
    let config = match crate::storage::Config::load(Some(&root)) {
        Ok(config) => config,
        Err(error) => {
            if output_format.is_machine_readable() {
                crate::cli::stream_json::print_output_error(output_format, error.to_string());
            }
            return Err(error);
        }
    };
    let provider = build_provider(&config.provider, api_key);
    let client = provider.create_deepseek_client();

    let requested_model =
        match ModelSelection::resolve(&config.provider, &config.model, model_override.as_deref()) {
            Ok(selection) => selection.model,
            Err(error) => return json_error_result(output_format, error.into()),
        };

    // Create or load session
    let home = match storage::user_home_dir() {
        Some(home) => home,
        None => {
            return json_error_result(output_format, anyhow::anyhow!("cannot find home directory"));
        }
    };
    let store = storage::SessionStore::new(home.join(".octocode"));

    let mut session = if let Some(ref sid) = session_id {
        let sid = match uuid::Uuid::parse_str(sid) {
            Ok(sid) => sid,
            Err(error) => return json_error_result(output_format, error.into()),
        };
        match store.load(&root, &sid) {
            Ok(s) => {
                if !output_format.is_machine_readable() {
                    eprintln!(
                        "Resumed session: {} ({} messages, {} tool calls)",
                        sid,
                        s.messages.len(),
                        s.tool_call_history.len()
                    );
                }
                s
            }
            Err(e) => {
                return json_error_result(
                    output_format,
                    anyhow::anyhow!("Failed to load session {sid}: {e}"),
                );
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
                selected_model: Some(requested_model.clone()),
                ..Default::default()
            },
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            metadata: SessionMetadata::default(),
        }
    };
    if model_override.is_some() {
        session.reasoning_state.selected_model = Some(requested_model);
    }

    let policy_root = root.clone();
    let mut orchestrator = Some(Orchestrator::new(client, root, session));
    if let Some(ref mut orch) = orchestrator {
        if let Some(mode) = approval_mode {
            orch.permission_mode = mode;
            // Bypass / Auto both imply "approve everything"; mirror that
            // into the legacy yolo_mode flag so any code path that still
            // reads yolo_mode (subagent dispatch, hooks-only branches, …)
            // behaves consistently with the new mode.
            if matches!(
                mode,
                crate::policy::PermissionMode::Bypass | crate::policy::PermissionMode::Auto
            ) {
                orch.yolo_mode = true;
            }
        }
        orch.init_mcp(&config.mcp).await;
    }

    if (output_format.is_stream_json() || output_format.is_json()) && prompt.is_none() {
        let name = if output_format.is_json() {
            "json"
        } else {
            "stream-json"
        };
        return json_error_result(
            output_format,
            anyhow::anyhow!("chat --output-format {name} requires a prompt"),
        );
    }

    if let Some(p) = prompt {
        // One-shot mode: spawn the turn so approval events are handled
        // concurrently rather than deadlocking.
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut final_json = output_format.is_json().then(|| {
            let session_id = orchestrator
                .as_ref()
                .expect("orchestrator should exist")
                .session
                .id;
            crate::cli::stream_json::FinalJsonCollector::new(session_id)
        });
        if output_format.is_stream_json() {
            if let Some(orch) = orchestrator.as_ref() {
                crate::cli::stream_json::print_session_started(orch.session.id, &policy_root);
            }
        }
        let mut orch = orchestrator.take().expect("orchestrator already consumed");
        let turn_handle = tokio::spawn(async move {
            let result = orch.run_turn(&p, ev_tx).await;
            (orch, result)
        });

        let is_tty = approval::cli_prompt_tty();
        let mut tool_auto_approve_session = false;
        let mut subagent_auto_approve_session = false;
        while let Some(event) = ev_rx.recv().await {
            if let Some(collector) = final_json.as_mut() {
                collector.observe(&event);
                match event {
                    AgentEvent::ToolApprovalNeeded {
                        tool_name,
                        display,
                        respond,
                    } => {
                        let decision = approval::resolve_tool_approval(
                            tool_approval,
                            &mut tool_auto_approve_session,
                            is_tty,
                            &display,
                        )
                        .await;
                        let _ = respond.send(decision.approved);
                        if let Some(reason) = decision.denial_reason {
                            collector.record_tool_approval_denied(&tool_name, reason);
                        }
                    }
                    AgentEvent::SubagentToolApprovalNeeded {
                        tool_name,
                        policy_decision,
                        respond,
                        ..
                    } => {
                        let decision = approval::resolve_subagent_tool_approval(
                            &policy_decision,
                            tool_approval,
                            &mut subagent_auto_approve_session,
                            is_tty,
                        )
                        .await;
                        let _ = respond.send(decision.approved);
                        if let Some(reason) = decision.denial_reason {
                            collector.record_tool_approval_denied(&tool_name, reason);
                        }
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
                    AgentEvent::ToolApprovalNeeded {
                        tool_name,
                        display,
                        respond,
                    } => {
                        let decision = approval::resolve_tool_approval(
                            tool_approval,
                            &mut tool_auto_approve_session,
                            is_tty,
                            &display,
                        )
                        .await;
                        let _ = respond.send(decision.approved);
                        if let Some(reason) = decision.denial_reason {
                            crate::cli::stream_json::print_tool_approval_denied(&tool_name, reason);
                        }
                    }
                    AgentEvent::SubagentToolApprovalNeeded {
                        tool_name,
                        policy_decision,
                        respond,
                        ..
                    } => {
                        let decision = approval::resolve_subagent_tool_approval(
                            &policy_decision,
                            tool_approval,
                            &mut subagent_auto_approve_session,
                            is_tty,
                        )
                        .await;
                        let _ = respond.send(decision.approved);
                        if let Some(reason) = decision.denial_reason {
                            crate::cli::stream_json::print_tool_approval_denied(&tool_name, reason);
                        }
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
                    let decision = approval::resolve_tool_approval(
                        tool_approval,
                        &mut tool_auto_approve_session,
                        is_tty,
                        &display,
                    )
                    .await;
                    let _ = respond.send(decision.approved);
                    if let Some(reason) = decision.denial_reason {
                        println!("[Denied: {tool_name} — {reason}]");
                    }
                }
                AgentEvent::ToolStarted { .. } => {}
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
                AgentEvent::UserQuestionRequested { title, options, .. } => {
                    println!("\n[Question] {title}");
                    for (i, option) in options.iter().enumerate() {
                        println!("  {}. {option}", i + 1);
                    }
                }
                AgentEvent::ContextCompacted {
                    reason,
                    before_tokens,
                    after_tokens,
                    ..
                } => {
                    println!(
                        "\n[Context compacted: {reason}] {before_tokens} -> {after_tokens} tokens"
                    );
                }
                AgentEvent::HookExecuted { .. } => {}
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
                    tool_name,
                    policy_decision,
                    respond,
                    ..
                } => {
                    let decision = approval::resolve_subagent_tool_approval(
                        &policy_decision,
                        tool_approval,
                        &mut subagent_auto_approve_session,
                        is_tty,
                    )
                    .await;
                    if let Some(reason) = decision.denial_reason {
                        println!("\n[Subagent approval denied for '{tool_name}': {reason}]");
                    }
                    let _ = respond.send(decision.approved);
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
                    let choice = prompt_cli_select(&title, &options, is_tty).await;
                    let _ = respond.send(choice);
                }
            }
        }

        let mut turn_error: Option<anyhow::Error> = None;
        match turn_handle.await {
            Ok((returned_orch, result)) => {
                if let Err(error) = result {
                    if let Some(collector) = final_json.as_mut() {
                        collector.record_error(error.to_string());
                    } else if output_format.is_stream_json() {
                        crate::cli::stream_json::print_error_event(error.to_string());
                    }
                    turn_error = Some(error);
                }
                orchestrator = Some(returned_orch);
            }
            Err(e) => {
                let error = anyhow::anyhow!("Turn task failed: {e}");
                if let Some(collector) = final_json.as_mut() {
                    collector.record_error(error.to_string());
                } else if output_format.is_stream_json() {
                    crate::cli::stream_json::print_error_event(error.to_string());
                }
                turn_error = Some(error);
            }
        }

        // Save session after one-shot
        if let Some(ref o) = orchestrator {
            if let Err(error) = store.save(&o.session) {
                if let Some(collector) = final_json.as_mut() {
                    collector.record_error(error.to_string());
                    turn_error = Some(error);
                } else if output_format.is_stream_json() {
                    crate::cli::stream_json::print_error_event(error.to_string());
                    turn_error = Some(error);
                } else {
                    return Err(error);
                }
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
    } else {
        // Interactive mode
        println!("Octocode interactive chat (Ctrl+C to exit, /help for commands)");
        if let Some(ref o) = orchestrator {
            println!(
                "Model: {} | Thinking: {thinking}",
                o.session.reasoning_state.effective_model()
            );
        }

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
            let mut orch = orchestrator.take().expect("orchestrator already consumed");
            let turn_handle = tokio::spawn(async move {
                let result = orch.run_turn(&input, ev_tx).await;
                (orch, result)
            });

            let mut tool_auto_approve_session = false;
            let mut subagent_auto_approve_session = false;
            let is_tty = approval::cli_prompt_tty();
            while let Some(event) = ev_rx.recv().await {
                match event {
                    AgentEvent::UserMessage { .. } => {}
                    AgentEvent::ContentDelta(text) => print!("{text}"),
                    AgentEvent::TokenDelta { .. } => {}
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
                        let decision = approval::resolve_tool_approval(
                            tool_approval,
                            &mut tool_auto_approve_session,
                            is_tty,
                            &display,
                        )
                        .await;
                        if let Some(reason) = decision.denial_reason {
                            println!("\n[Denied: {tool_name} — {reason}]");
                        }
                        let _ = respond.send(decision.approved);
                    }
                    AgentEvent::ToolStarted { .. } => {}
                    AgentEvent::SubagentToolApprovalNeeded {
                        tool_name,
                        policy_decision,
                        respond,
                        ..
                    } => {
                        let decision = approval::resolve_subagent_tool_approval(
                            &policy_decision,
                            tool_approval,
                            &mut subagent_auto_approve_session,
                            is_tty,
                        )
                        .await;
                        if let Some(reason) = decision.denial_reason {
                            println!("\n[Subagent approval denied for '{tool_name}': {reason}]");
                        }
                        let _ = respond.send(decision.approved);
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
                    AgentEvent::UserQuestionRequested { title, options, .. } => {
                        println!("\n[Question] {title}");
                        for (i, option) in options.iter().enumerate() {
                            println!("  {}. {option}", i + 1);
                        }
                    }
                    AgentEvent::ContextCompacted {
                        reason,
                        before_tokens,
                        after_tokens,
                        ..
                    } => {
                        println!(
                            "\n[Context compacted: {reason}] {before_tokens} -> {after_tokens} tokens"
                        );
                    }
                    AgentEvent::HookExecuted { .. } => {}
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
                        let choice = prompt_cli_select(&title, &options, is_tty).await;
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
                let home = storage::user_home_dir()
                    .ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;
                let store = storage::SessionStore::new(home.join(".octocode"));
                if let Err(e) = store.save(&o.session) {
                    eprintln!("warning: failed to persist session: {e}");
                }
            }
            println!();
        }
    }

    Ok(())
}

fn json_error_result<T>(
    output_format: TurnOutputFormat,
    error: anyhow::Error,
) -> Result<T, anyhow::Error> {
    if output_format.is_machine_readable() {
        crate::cli::stream_json::print_output_error(output_format, error.to_string());
    }
    Err(error)
}

async fn prompt_cli_select(title: &str, options: &[String], is_tty: bool) -> usize {
    if is_tty {
        return inline_popup::prompt_select_inline(title, options)
            .await
            .ok()
            .flatten()
            .unwrap_or(0);
    }

    println!("\n{title}");
    for (i, opt) in options.iter().enumerate() {
        println!("  {}. {opt}", i + 1);
    }
    println!("Select option (1–{}): ", options.len());
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    answer
        .trim()
        .parse::<usize>()
        .unwrap_or(options.len())
        .saturating_sub(1)
}

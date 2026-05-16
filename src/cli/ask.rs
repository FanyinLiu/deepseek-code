use std::path::PathBuf;

use crate::agent::orchestrator::{AgentEvent, Orchestrator};
use crate::cli::{resolve_project_root, TurnOutputFormat};
use crate::deepseek::{
    MessageContent, MessageId, MessageVisibility, ProtocolMessage, ReasoningState, Role, Session,
    SessionId, SessionMetadata, TurnId,
};
use crate::provider::{build_provider, ModelSelection, Provider};

/// Run the ask command: search-first, non-editing query.
pub async fn ask(
    question: String,
    project_root: Option<PathBuf>,
    output_format: TurnOutputFormat,
) -> Result<(), anyhow::Error> {
    let root = match resolve_project_root(project_root, "ask") {
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

    let mut session = Session {
        id: SessionId::new_v4(),
        name: None,
        project_root: root.clone(),
        messages: Vec::new(),
        reasoning_state: ReasoningState {
            selected_model: Some(model),
            ..ReasoningState::default()
        },
        tool_call_history: Vec::new(),
        checkpoints: Vec::new(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        metadata: SessionMetadata::default(),
    };

    // Pre-search: find relevant files and code for context
    let mut search_results: Vec<crate::search::SearchMatch> = Vec::new();
    if let Ok(files) = crate::search::search_files(&root, &question, 10) {
        search_results.extend(files);
    }
    if let Ok(code) = crate::search::search_code(&root, &question, None, false, 15) {
        search_results.extend(code);
    }

    // Build search context and inject as first user message
    if !search_results.is_empty() {
        let ctx = crate::search::pack_search_results(&search_results, 6000);
        session.messages.push(ProtocolMessage {
            id: MessageId::new_v4(),
            role: Role::User,
            content: MessageContent::from(format!(
                "I searched the project for \"{question}\". Here are the results:\n\n{ctx}"
            )),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: TurnId::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        });
    }

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

    // Spawn the turn in the background so we don't deadlock on approval events.
    let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
    let question_for_turn = question.clone();
    let mut orch_for_turn = orchestrator;
    let turn_handle = tokio::spawn(async move {
        let result = orch_for_turn.run_turn(&question_for_turn, ev_tx).await;
        (orch_for_turn, result)
    });

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
            AgentEvent::ToolStarted { .. } => {}
            AgentEvent::ToolApprovalNeeded {
                tool_name, respond, ..
            } => {
                // ask is read-only: deny any tool that requires approval
                println!("\n[Tool approval required for '{tool_name}'; denying in ask mode]");
                let _ = respond.send(false);
            }
            AgentEvent::ToolExecuted {
                tool_name, success, ..
            } if success => {
                println!("[{tool_name} ✓]");
            }
            AgentEvent::HookExecuted { .. } => {}
            AgentEvent::Error(e) => eprintln!("\nError: {e}"),
            _ => {}
        }
    }
    if output_format == TurnOutputFormat::Text {
        println!();
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

use std::path::PathBuf;

use crate::agent::orchestrator::{AgentEvent, Orchestrator};
use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::{
    MessageContent, MessageId, MessageVisibility, ProtocolMessage, ReasoningState, Role, Session,
    SessionId, SessionMetadata, TurnId,
};
use crate::storage;

/// Run the ask command: search-first, non-editing query.
pub async fn ask(question: String, project_root: Option<PathBuf>) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));
    let api_key = super::login::resolve_or_prompt_api_key(Some(&root))?;
    let client = DeepSeekClient::new(api_key);

    let mut session = Session {
        id: SessionId::new_v4(),
        name: None,
        project_root: root.clone(),
        messages: Vec::new(),
        reasoning_state: ReasoningState::default(),
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
    let config = crate::storage::Config::load(Some(&orchestrator.project_root)).unwrap_or_default();
    orchestrator.init_mcp(&config.mcp).await;

    // Spawn the turn in the background so we don't deadlock on approval events.
    let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
    let question_for_turn = question.clone();
    let mut orch_for_turn = orchestrator;
    let turn_handle = tokio::spawn(async move {
        let result = orch_for_turn.run_turn(&question_for_turn, ev_tx).await;
        (orch_for_turn, result)
    });

    while let Some(event) = ev_rx.recv().await {
        match event {
            AgentEvent::ContentDelta(text) => print!("{text}"),
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
            AgentEvent::Error(e) => eprintln!("\nError: {e}"),
            _ => {}
        }
    }
    println!();

    match turn_handle.await {
        Ok((returned_orch, result)) => {
            let _ = returned_orch;
            result?;
        }
        Err(e) => anyhow::bail!("Turn task failed: {e}"),
    }

    Ok(())
}

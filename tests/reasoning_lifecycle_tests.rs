/// Tests for `reasoning_content` lifecycle — the highest-risk protocol state.
/// Validates: tool loop preservation, cleanup between turns, resume after restore.
use std::path::PathBuf;

use chrono::Utc;

use deepseek_code::agent::reasoning::ReasoningManager;
use deepseek_code::deepseek::*;

fn make_session() -> Session {
    Session {
        id: SessionId::new_v4(),
        name: None,
        project_root: PathBuf::from("/tmp/test-project"),
        messages: Vec::new(),
        reasoning_state: ReasoningState::default(),
        tool_call_history: Vec::new(),
        checkpoints: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        metadata: SessionMetadata::default(),
    }
}

fn make_assistant_message(
    content: &str,
    reasoning: Option<&str>,
    tool_calls: Vec<ToolCall>,
    turn_id: TurnId,
    sub_turn_id: Option<SubTurnId>,
) -> ProtocolMessage {
    let has_tools = !tool_calls.is_empty();
    ProtocolMessage {
        id: MessageId::new_v4(),
        role: Role::Assistant,
        content: MessageContent::from(content),
        reasoning_content: reasoning.map(std::string::ToString::to_string),
        tool_calls,
        tool_results: Vec::new(),
        turn_id,
        sub_turn_id,
        visibility: if has_tools {
            MessageVisibility::InternalProtocolState
        } else {
            MessageVisibility::UserVisible
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: begin_user_turn cleans up previous reasoning
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_begin_user_turn_cleans_previous_reasoning() {
    let mut session = make_session();
    let turn_1 = TurnId::new_v4();
    let turn_2 = TurnId::new_v4();

    // Simulate a tool-call turn in turn 1
    let sub_turn = SubTurnId::new_v4();
    let tool_call = ToolCall {
        id: "tc-1".into(),
        call_type: "function".into(),
        function: ToolCallFunction {
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
        },
    };

    let assistant_msg = make_assistant_message(
        "Let me read that file",
        Some("thinking about reading the file"),
        vec![tool_call],
        turn_1,
        Some(sub_turn),
    );

    session.messages.push(assistant_msg.clone());
    ReasoningManager::begin_tool_turn(&mut session.reasoning_state, &assistant_msg);

    // Verify reasoning is preserved for tool turn
    assert!(session.reasoning_state.active_tool_turn.is_some());
    assert!(!session
        .reasoning_state
        .preserved_assistant_messages
        .is_empty());

    // Now begin a new user turn
    ReasoningManager::begin_user_turn(&mut session.reasoning_state, &mut session.messages, turn_2);

    // After cleanup: no active tool turn, no preserved messages
    assert!(session.reasoning_state.active_tool_turn.is_none());
    assert!(session
        .reasoning_state
        .preserved_assistant_messages
        .is_empty());
    assert_eq!(session.reasoning_state.last_cleanup_at_turn, Some(turn_2));

    // The old assistant message should be marked as AuditOnly
    if let Some(msg) = session.messages.first() {
        assert_eq!(msg.visibility, MessageVisibility::AuditOnly);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: reasoning is preserved for active tool turn
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_preserve_reasoning_for_active_tool_turn() {
    let mut session = make_session();
    let turn_id = TurnId::new_v4();
    let sub_turn = SubTurnId::new_v4();

    let tool_call = ToolCall {
        id: "tc-read".into(),
        call_type: "function".into(),
        function: ToolCallFunction {
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
        },
    };

    let assistant_msg = make_assistant_message(
        "Reading the file now",
        Some("I need to check src/main.rs first"),
        vec![tool_call.clone()],
        turn_id,
        Some(sub_turn),
    );

    let msg_id = assistant_msg.id;
    session.messages.push(assistant_msg);

    ReasoningManager::begin_tool_turn(&mut session.reasoning_state, &session.messages[0]);

    // Preservation is now tracked by membership: an assistant message that
    // opened a tool turn is in the preserved set for the loop's duration.
    assert!(session
        .reasoning_state
        .preserved_assistant_messages
        .contains(&msg_id));
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: non-tool-call messages should NOT preserve reasoning
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_reasoning_not_preserved_without_tool_calls() {
    let mut session = make_session();
    let turn_id = TurnId::new_v4();

    let assistant_msg = make_assistant_message(
        "Here's the answer",
        Some("I thought about this carefully"),
        vec![], // no tool calls
        turn_id,
        None,
    );

    session.messages.push(assistant_msg);

    // A message with no tool calls never opens a tool turn, so it is never
    // added to the preserved set.
    let last_msg = session.messages.last().expect("messages not empty");
    assert!(last_msg.tool_calls.is_empty());
    assert!(!session
        .reasoning_state
        .preserved_assistant_messages
        .contains(&last_msg.id));
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: complete_tool_loop moves reasoning to internal state
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_complete_tool_loop_moves_reasoning_to_internal() {
    let mut session = make_session();
    let turn_id = TurnId::new_v4();
    let sub_turn = SubTurnId::new_v4();

    let tool_call = ToolCall {
        id: "tc-edit".into(),
        call_type: "function".into(),
        function: ToolCallFunction {
            name: "edit_file".into(),
            arguments: r#"{"path":"src/main.rs","old_string":"a","new_string":"b"}"#.into(),
        },
    };

    let assistant_msg = make_assistant_message(
        "I'll fix that",
        Some("thinking about the edit"),
        vec![tool_call],
        turn_id,
        Some(sub_turn),
    );

    let msg_id = assistant_msg.id;
    session.messages.push(assistant_msg);

    ReasoningManager::begin_tool_turn(&mut session.reasoning_state, &session.messages[0]);

    // Complete the tool loop
    ReasoningManager::complete_tool_loop(&mut session.reasoning_state, &mut session.messages);

    // After completion
    assert!(session.reasoning_state.active_tool_turn.is_none());
    assert!(session
        .reasoning_state
        .preserved_assistant_messages
        .is_empty());

    // The message should now be InternalProtocolState
    if let Some(msg) = session.messages.iter().find(|m| m.id == msg_id) {
        assert_eq!(msg.visibility, MessageVisibility::InternalProtocolState);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: new_tool_result_message creates correct structure
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_new_tool_result_message() {
    let turn_id = TurnId::new_v4();
    let sub_turn_id = SubTurnId::new_v4();

    let msg = ReasoningManager::new_tool_result_message(
        "tc-1",
        "read_file",
        "file contents here",
        false,
        turn_id,
        sub_turn_id,
    );

    assert_eq!(msg.role, Role::Tool);
    assert_eq!(msg.visibility, MessageVisibility::InternalProtocolState);
    assert_eq!(msg.tool_results.len(), 1);
    assert_eq!(msg.tool_results[0].tool_call_id, "tc-1");
    assert_eq!(msg.tool_results[0].name, "read_file");
    assert!(!msg.tool_results[0].is_error);
}

#[test]
fn test_new_tool_result_message_error() {
    let turn_id = TurnId::new_v4();
    let sub_turn_id = SubTurnId::new_v4();

    let msg = ReasoningManager::new_tool_result_message(
        "tc-err",
        "run_command",
        "command not found",
        true,
        turn_id,
        sub_turn_id,
    );

    assert_eq!(msg.role, Role::Tool);
    assert!(msg.tool_results[0].is_error);
}

/// Tests for session persistence: save, load, resume with reasoning state.
use std::path::PathBuf;

use chrono::Utc;

use deepseek_code::deepseek::*;
use deepseek_code::storage::{EventLogStore, SessionEvent, SessionEventKind, SessionStore};

fn make_test_session(name: &str, project_root: &std::path::Path) -> Session {
    let turn_id = TurnId::new_v4();
    let sub_turn = SubTurnId::new_v4();

    let mut session = Session {
        id: SessionId::new_v4(),
        name: Some(name.to_string()),
        project_root: project_root.to_path_buf(),
        messages: Vec::new(),
        reasoning_state: ReasoningState {
            mode: ThinkingMode::On,
            effort: ReasoningEffort::Max,
            selected_model: Some(DeepSeekModel::Pro),
            active_tool_turn: Some(sub_turn),
            preserved_assistant_messages: vec![MessageId::new_v4()],
            last_cleanup_at_turn: Some(turn_id),
        },
        tool_call_history: vec![ToolCallRecord {
            id: "tc-1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
            result_summary: "file content".into(),
            exit_code: Some(0),
            duration_ms: 100,
            risk_level: "SafeRead".into(),
            approved: true,
            at: Utc::now(),
        }],
        checkpoints: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        metadata: SessionMetadata::default(),
    };

    // Add a few messages to simulate conversation
    session.messages.push(ProtocolMessage {
        id: MessageId::new_v4(),
        role: Role::User,
        content: MessageContent::from("read main.rs"),
        reasoning_content: None,
        tool_calls: Vec::new(),
        tool_results: Vec::new(),
        turn_id,
        sub_turn_id: None,
        visibility: MessageVisibility::UserVisible,
    });

    session
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Save and load round-trip
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_session_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");

    let store = SessionStore::new(tmp.path().to_path_buf());
    let original = make_test_session("test-session", &project_root);

    // Save
    store.save(&original).expect("save session");

    // Load
    let loaded = store
        .load(&project_root, &original.id)
        .expect("load session");

    assert_eq!(loaded.id, original.id);
    assert_eq!(loaded.name, original.name);
    assert_eq!(loaded.messages.len(), original.messages.len());
    assert_eq!(
        loaded.tool_call_history.len(),
        original.tool_call_history.len()
    );

    // Verify reasoning state was preserved
    assert_eq!(loaded.reasoning_state.mode, original.reasoning_state.mode);
    assert_eq!(
        loaded.reasoning_state.effort,
        original.reasoning_state.effort
    );
    assert_eq!(
        loaded.reasoning_state.preserved_assistant_messages.len(),
        original.reasoning_state.preserved_assistant_messages.len()
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// List sessions for a project
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_session_list() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");

    let store = SessionStore::new(tmp.path().to_path_buf());

    let s1 = make_test_session("session-1", &project_root);
    let s2 = make_test_session("session-2", &project_root);

    store.save(&s1).expect("save s1");
    store.save(&s2).expect("save s2");

    let summaries = store.list(&project_root).expect("list sessions");
    assert_eq!(summaries.len(), 2);

    // Newest first (sorted by updated_at)
    assert_eq!(summaries[0].name, Some("session-2".into()));
    assert_eq!(summaries[1].name, Some("session-1".into()));
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Delete a session
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_session_delete() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");

    let store = SessionStore::new(tmp.path().to_path_buf());
    let session = make_test_session("to-delete", &project_root);

    store.save(&session).expect("save session");
    assert_eq!(store.list(&project_root).expect("list sessions").len(), 1);

    store
        .delete(&project_root, &session.id)
        .expect("delete session");
    assert_eq!(store.list(&project_root).expect("list sessions").len(), 0);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Transcript export contains expected content
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_transcript_export_includes_session_info() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");

    let store = SessionStore::new(tmp.path().to_path_buf());
    let session = make_test_session("export-test", &project_root);

    store.save(&session).expect("save session");

    // Check that transcript.md was generated
    let session_dir = store.session_dir(&project_root, &session.id);
    let transcript_path = session_dir.join("transcript.md");
    assert!(transcript_path.exists());

    let content = std::fs::read_to_string(&transcript_path).expect("read transcript");
    assert!(content.contains("export-test"));
    assert!(content.contains("read main.rs"));
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Empty session list returns empty vec
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_empty_session_list() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = SessionStore::new(tmp.path().to_path_buf());
    let summaries = store
        .list(PathBuf::from("/nonexistent").as_path())
        .expect("list nonexistent");
    assert!(summaries.is_empty());
}

#[test]
fn test_event_log_marks_unfinished_session_without_replaying_commands() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");
    let session = make_test_session("unfinished", &project_root);
    let event_store = EventLogStore::new(tmp.path().to_path_buf());
    let turn_id = TurnId::new_v4();

    event_store
        .append(
            &project_root,
            &SessionEvent::new(
                session.id,
                Some(turn_id),
                SessionEventKind::UserMessage {
                    content: "run tests".into(),
                },
            ),
        )
        .expect("append user event");
    event_store
        .append(
            &project_root,
            &SessionEvent::new(
                session.id,
                Some(turn_id),
                SessionEventKind::ToolCallStarted {
                    tool_call_id: "call-1".into(),
                    name: "run_command".into(),
                    arguments: r#"{"command":"cargo test"}"#.into(),
                },
            ),
        )
        .expect("append tool event");

    assert_eq!(
        event_store
            .unfinished_turn(&project_root, &session.id)
            .expect("unfinished turn"),
        Some(turn_id)
    );
}

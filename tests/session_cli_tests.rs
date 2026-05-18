use std::process::Command;

use deepseek_code::deepseek::{ReasoningState, Session, SessionId, SessionMetadata};
use deepseek_code::storage::{EventLogStore, SessionEvent, SessionEventKind, SessionStore};

fn octocode_command(project_root: &std::path::Path, home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octocode"));
    command.arg("-C").arg(project_root);
    command.env("HOME", home);
    command.env("DEEPSEEK_CODE_HOME", home);
    command.current_dir(project_root);
    command
}

#[test]
fn session_list_replay_and_export_use_saved_data_without_rerun() {
    let root = tempfile::tempdir().expect("project tempdir");
    let home = tempfile::tempdir().expect("home tempdir");
    let base = home.path().join(".octocode");
    let session = Session {
        id: SessionId::new_v4(),
        name: Some("test-session".to_string()),
        project_root: root.path().to_path_buf(),
        messages: Vec::new(),
        reasoning_state: ReasoningState::default(),
        tool_call_history: Vec::new(),
        checkpoints: Vec::new(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        metadata: SessionMetadata::default(),
    };
    SessionStore::new(base.clone())
        .save(&session)
        .expect("save session");
    EventLogStore::new(base)
        .append(
            root.path(),
            &SessionEvent::new(
                session.id,
                None,
                SessionEventKind::UserMessage {
                    content: "hello".to_string(),
                },
            ),
        )
        .expect("append event");
    EventLogStore::new(home.path().join(".octocode"))
        .append(
            root.path(),
            &SessionEvent::new(
                session.id,
                None,
                SessionEventKind::HookExecuted {
                    event: "pre_tool_use".to_string(),
                    success: true,
                    summary: "pre: ok".to_string(),
                    command_count: 1,
                },
            ),
        )
        .expect("append hook event");

    let output = octocode_command(root.path(), home.path())
        .args(["session", "list", "--json"])
        .output()
        .expect("run octocode session list");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("session list json parses");
    assert_eq!(json.as_array().expect("list").len(), 1);

    let output = octocode_command(root.path(), home.path())
        .args(["session", "replay", "test-session"])
        .output()
        .expect("run octocode session replay");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let replay_stdout = String::from_utf8_lossy(&output.stdout);
    assert!(replay_stdout.contains("hook pre_tool_use success=true commands=1"));
    assert!(replay_stdout.contains("No tools or commands were re-run"));

    let output = octocode_command(root.path(), home.path())
        .args(["session", "export", "test-session", "--format", "json"])
        .output()
        .expect("run octocode session export");
    assert!(output.status.success(), "stderr={}", stderr(&output));
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

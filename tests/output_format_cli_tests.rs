use std::process::Command;

fn octocode_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_octocode"))
}

fn octo_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_octo"))
}

#[test]
fn turn_commands_emit_final_json_error_schema_for_preflight_failures() {
    for (command_name, args) in [
        ("chat", vec!["chat", "hello", "--output-format", "json"]),
        ("ask", vec!["ask", "hello", "--output-format", "json"]),
        ("run", vec!["run", "hello", "--output-format", "json"]),
    ] {
        let root = tempfile::tempdir().expect("tempdir");
        let output = octocode_command()
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("run octocode {command_name}: {error}"));

        assert!(!output.status.success(), "{command_name} should fail");
        let stdout = stdout(&output);
        let lines = stdout.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "{command_name} stdout: {stdout:?}");
        let value: serde_json::Value =
            serde_json::from_str(lines[0]).expect("stdout should be one JSON object");
        assert_eq!(value["status"], "error");
        assert!(value["session_id"].is_null());
        assert_eq!(value["final_message"], "");
        assert_eq!(value["tool_calls"].as_array().unwrap().len(), 0);
        assert!(value["usage"].is_null());
        assert!(value["error"]
            .as_str()
            .unwrap()
            .contains("requires a project root"));
        assert!(!stderr(&output).contains("not supported yet"));
    }
}

#[test]
fn octo_run_accepts_tool_approval_flags_for_json_preflight() {
    for (case, args) in [
        (
            "json-deny",
            vec![
                "run",
                "hello",
                "--output-format",
                "json",
                "--tool-approval",
                "deny",
            ],
        ),
        (
            "stream-json-deny",
            vec![
                "run",
                "hello",
                "--output-format",
                "stream-json",
                "--tool-approval",
                "deny",
            ],
        ),
        (
            "auto-approve-short",
            vec!["run", "hello", "--output-format", "json", "-y"],
        ),
    ] {
        let root = tempfile::tempdir().expect("tempdir");
        let output = octo_command()
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("run octo {case}: {error}"));

        assert!(
            !output.status.success(),
            "{case} should fail before running"
        );
        assert!(
            !stderr(&output).contains("unexpected argument"),
            "{case} stderr: {}",
            stderr(&output)
        );
        let stdout = stdout(&output);
        if case == "stream-json-deny" {
            let lines = stdout.lines().collect::<Vec<_>>();
            assert_eq!(lines.len(), 1, "{case} stdout: {stdout:?}");
            let value: serde_json::Value =
                serde_json::from_str(lines[0]).expect("stdout should be one JSON object");
            assert_eq!(value["type"], "error");
            assert!(
                value["error"]
                    .as_str()
                    .unwrap()
                    .contains("requires a project root"),
                "{case}: {stdout:?}"
            );
        } else {
            assert!(
                stdout.contains("\"status\":\"error\""),
                "{case}: {stdout:?}"
            );
            assert!(
                stdout.contains("requires a project root"),
                "{case}: {stdout:?}"
            );
        }
    }
}

#[test]
fn turn_commands_emit_stream_json_error_event_for_preflight_failures() {
    for (command_name, args) in [
        (
            "chat",
            vec!["chat", "hello", "--output-format", "stream-json"],
        ),
        (
            "ask",
            vec!["ask", "hello", "--output-format", "stream-json"],
        ),
        (
            "run",
            vec!["run", "hello", "--output-format", "stream-json"],
        ),
    ] {
        let root = tempfile::tempdir().expect("tempdir");
        let output = octo_command()
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("run octo {command_name}: {error}"));

        assert!(!output.status.success(), "{command_name} should fail");
        let stdout = stdout(&output);
        let lines = stdout.lines().collect::<Vec<_>>();
        assert!(
            !lines.is_empty(),
            "{command_name} should emit at least one stream-json line"
        );
        let value: serde_json::Value =
            serde_json::from_str(lines[0]).expect("stdout line should be JSON");
        assert_eq!(value["type"], "error", "{command_name}: {stdout:?}");
        assert!(value["error"]
            .as_str()
            .unwrap()
            .contains("requires a project root"));
    }
}

#[test]
fn octo_chat_and_ask_accept_tool_approval_flags() {
    for (case, args) in [
        (
            "chat-deny",
            vec![
                "chat",
                "hello",
                "--output-format",
                "json",
                "--tool-approval",
                "deny",
            ],
        ),
        (
            "chat-auto-approve",
            vec!["chat", "hello", "--output-format", "json", "-y"],
        ),
        (
            "ask-allow",
            vec![
                "ask",
                "hello",
                "--output-format",
                "json",
                "--tool-approval",
                "allow",
            ],
        ),
        (
            "ask-auto-approve",
            vec!["ask", "hello", "--output-format", "json", "-y"],
        ),
    ] {
        let root = tempfile::tempdir().expect("tempdir");
        let output = octo_command()
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("run octo {case}: {error}"));

        assert!(
            !output.status.success(),
            "{case} should fail before running"
        );
        assert!(
            !stderr(&output).contains("unexpected argument"),
            "{case} stderr: {}",
            stderr(&output)
        );
        let stdout = stdout(&output);
        let lines = stdout.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "{case} stdout: {stdout:?}");
        let value: serde_json::Value =
            serde_json::from_str(lines[0]).expect("stdout should be one JSON object");
        assert_eq!(value["status"], "error");
        assert!(value["events"].is_array(), "{case}: {value}");
        assert!(value["error"]
            .as_str()
            .unwrap()
            .contains("requires a project root"));
    }
}

#[test]
fn octo_task_run_exposes_json_format_flag() {
    let output = octo_command()
        .args(["task", "run", "--help"])
        .output()
        .unwrap_or_else(|error| panic!("run octo task run --help: {error}"));

    assert!(output.status.success(), "help should succeed");
    let stdout = stdout(&output);
    assert!(stdout.contains("--format"), "{stdout}");
    assert!(stdout.contains("stream-json"), "{stdout}");
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

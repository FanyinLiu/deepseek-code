use std::process::Command;

fn octocode_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_octocode"))
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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

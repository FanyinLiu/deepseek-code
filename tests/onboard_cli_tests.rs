use std::process::Command;

use serde_json::Value;

fn octo(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octo"));
    command
        .arg("-C")
        .arg(root)
        .env("OCTOCODE_HOME", root.join("home"))
        .env("DEEPSEEK_API_KEY", "test-key");
    command
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn onboard_json_reports_terminal_first_readiness() {
    let root = tempfile::tempdir().expect("temp project root");
    let output = octo(root.path())
        .args(["onboard", "--json", "--daemon-preview"])
        .output()
        .expect("run octo onboard json");

    assert_success(&output);
    let json: Value = serde_json::from_slice(&output.stdout).expect("onboard json parses");
    assert_eq!(json["terminal_first"], true);
    assert_eq!(json["project_root"], root.path().display().to_string());

    let checks = json["checks"].as_array().expect("checks array");
    for name in [
        "project_root",
        "primary_command",
        "config",
        "api_key",
        "agents",
        "mcp",
        "missions",
        "tui",
        "daemon_preview",
    ] {
        assert!(
            checks.iter().any(|check| check["name"] == name),
            "missing check {name}: {checks:#?}"
        );
    }

    let next_steps = json["next_steps"].as_array().expect("next steps array");
    assert!(next_steps
        .iter()
        .any(|step| step.as_str() == Some("octo doctor --smoke")));
    assert!(next_steps
        .iter()
        .any(|step| step.as_str() == Some("octo tui")));
}

#[test]
fn onboard_text_keeps_setup_terminal_oriented() {
    let root = tempfile::tempdir().expect("temp project root");
    let output = octo(root.path())
        .args(["onboard"])
        .output()
        .expect("run octo onboard text");

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Octo Onboard"));
    assert!(stdout.contains("terminal-first local coding agent"));
    assert!(stdout.contains("octo doctor --smoke"));
    assert!(stdout.contains("octo tui"));
    assert!(!stdout.contains("dashboard"));
}

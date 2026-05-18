use std::process::Command;

fn octocode_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octocode"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn config_explain_json_includes_sources_and_effective_values() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["config", "explain", "--json"])
        .output()
        .expect("run octocode config explain");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("config explain json parses");
    assert_eq!(json["project_root"], root.path().display().to_string());
    assert!(json["sources"].as_array().expect("sources").len() >= 3);
    assert_eq!(json["effective"]["provider_default"], "deepseek");
}

#[test]
fn settings_get_and_set_use_project_local_config() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["settings", "set", "ui.theme", "light"])
        .output()
        .expect("run octocode settings set");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let local = root.path().join(".octocode").join("local.toml");
    let local_content = std::fs::read_to_string(&local).expect("local config");
    assert!(local_content.contains("theme = \"light\""));

    let output = octocode_command(root.path())
        .args(["settings", "get", "ui.theme"])
        .output()
        .expect("run octocode settings get");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "light");
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

use std::process::Command;

fn octocode_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octocode"));
    command.arg("-C").arg(project_root);
    command
}

fn octo_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octo"));
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

#[test]
fn settings_can_set_ui_language() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["settings", "set", "ui.language", "en-us"])
        .output()
        .expect("run octocode settings set");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let local = root.path().join(".octocode").join("local.toml");
    let local_content = std::fs::read_to_string(&local).expect("local config");
    assert!(local_content.contains("language = \"en-US\""));

    let output = octocode_command(root.path())
        .args(["settings", "get", "ui.language"])
        .output()
        .expect("run octocode settings get");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "en-US");

    let output = octocode_command(root.path())
        .args(["settings", "set", "ui.language", "Japanese"])
        .output()
        .expect("run octocode settings set Japanese");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let local_content = std::fs::read_to_string(&local).expect("local config");
    assert!(local_content.contains("language = \"ja-JP\""));
}

#[test]
fn octo_models_json_lists_chinese_provider_profiles() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octo_command(root.path())
        .args(["models", "--json"])
        .output()
        .expect("run octo models");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("models json parses");
    let providers = json["providers"].as_array().expect("providers array");

    for name in ["qwen", "minimax", "tencent", "qianfan", "stepfun", "doubao"] {
        assert!(
            providers
                .iter()
                .any(|provider| provider["provider"].as_str() == Some(name)),
            "missing provider {name}"
        );
    }
    let kimi = providers
        .iter()
        .find(|provider| provider["provider"].as_str() == Some("kimi"))
        .expect("kimi provider");
    assert_eq!(kimi["base_url"], "https://api.moonshot.ai/v1");
    assert_eq!(kimi["capabilities"]["last_verified"], "2026-05-21");
    assert!(kimi["capabilities"]["routes"]
        .as_array()
        .expect("routes")
        .iter()
        .any(|route| route.as_str() == Some("research_vision")));
}

#[test]
fn octo_settings_can_set_new_provider_defaults() {
    for provider in ["qwen", "minimax", "tencent", "qianfan", "stepfun"] {
        let root = tempfile::tempdir().expect("tempdir");
        let output = octo_command(root.path())
            .args(["settings", "set", "provider.default", provider])
            .output()
            .expect("run octo settings set");

        assert!(
            output.status.success(),
            "provider={provider} stderr={}",
            stderr(&output)
        );
        let local = root.path().join(".octocode").join("local.toml");
        let local_content = std::fs::read_to_string(&local).expect("local config");
        assert!(local_content.contains(&format!("default = \"{provider}\"")));
    }
}

#[test]
fn octo_settings_rejects_unknown_provider_with_full_hint() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octo_command(root.path())
        .args(["settings", "set", "provider.default", "bad"])
        .output()
        .expect("run octo settings set");

    assert!(!output.status.success(), "stdout={}", stdout(&output));
    let stderr = stderr(&output);
    assert!(stderr.contains("provider.default must be one of:"));
    assert!(stderr.contains("minimax"));
    assert!(stderr.contains("qianfan"));
    assert!(stderr.contains("openai-compatible"));
}

#[test]
fn octo_doctor_no_network_reports_provider_without_credentials() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octo_command(root.path())
        .args(["doctor", "--no-network"])
        .output()
        .expect("run octo doctor");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("Provider: deepseek"));
    assert!(stdout.contains("API key env: DEEPSEEK_API_KEY"));
    assert!(stdout.contains("Network checks skipped"));
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

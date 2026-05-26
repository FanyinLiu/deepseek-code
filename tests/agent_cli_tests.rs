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
fn agent_list_json_contains_built_ins() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["agent", "list", "--json"])
        .output()
        .expect("run octocode agent list");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("agent list json parses");
    let names: Vec<_> = json["agents"]
        .as_array()
        .expect("agents array")
        .iter()
        .filter_map(|agent| agent["name"].as_str())
        .collect();

    assert!(names.contains(&"code-reviewer"));
    assert!(names.contains(&"security-auditor"));
}

#[test]
fn agent_show_code_reviewer_json_parses() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["agent", "show", "code-reviewer", "--json"])
        .output()
        .expect("run octocode agent show");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("agent show json parses");
    assert_eq!(json["name"], "code-reviewer");
    assert_eq!(json["permission_mode"], "read_only");
    assert!(json["system_prompt"]
        .as_str()
        .expect("system prompt")
        .contains("code reviewer"));
}

#[test]
fn agent_show_unknown_fails_cleanly() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["agent", "show", "missing-agent"])
        .output()
        .expect("run octocode agent show missing");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown agent 'missing-agent'"));
}

#[test]
fn agent_create_template_writes_file_and_validate_json_parses() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["agent", "create", "my-auditor", "--template", "auditor"])
        .output()
        .expect("run octocode agent create");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let path = root
        .path()
        .join(".octocode")
        .join("agents")
        .join("my-auditor.md");
    assert!(path.exists());

    let output = octocode_command(root.path())
        .args(["agent", "validate", "--all", "--json"])
        .output()
        .expect("run octocode agent validate all");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("validate json parses");
    let report = json["reports"]
        .as_array()
        .expect("reports array")
        .iter()
        .find(|report| report["name"] == "my-auditor")
        .expect("my-auditor report");
    assert_eq!(report["valid"], true);
}

#[test]
fn agent_validate_catches_malformed_agent() {
    let root = tempfile::tempdir().expect("tempdir");
    let agents_dir = root.path().join(".octocode").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("create agents dir");
    std::fs::write(agents_dir.join("broken.md"), "---\nnot-valid =\n---\n\n")
        .expect("write broken agent");

    let output = octocode_command(root.path())
        .args(["agent", "validate", "--all", "--json"])
        .output()
        .expect("run octocode agent validate");

    assert!(
        !output.status.success(),
        "malformed agents should fail validation"
    );
    assert!(stderr(&output).contains("agent validation failed"));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("validate json parses");
    let report = json["reports"]
        .as_array()
        .expect("reports array")
        .iter()
        .find(|report| report["name"] == "broken")
        .expect("broken report");
    assert_eq!(report["valid"], false);
    assert!(report["errors"]
        .as_array()
        .expect("errors")
        .iter()
        .any(|error| error["code"] == "frontmatter_parse"));
}

#[test]
fn agent_validate_all_includes_toml_custom_agents() {
    let root = tempfile::tempdir().expect("tempdir");
    let agents_dir = root.path().join(".octocode").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("create agents dir");
    std::fs::write(
        agents_dir.join("toml-explorer.toml"),
        r#"
subagent_type = "code-explorer"
allowed_tools = ["read_file", "search_code"]
permission_mode = "read_only"
model = "deepseek-v4-flash"
max_turns = 3
system_prompt = "Read the codebase and report concise findings."
"#,
    )
    .expect("write toml agent");

    let output = octocode_command(root.path())
        .args(["agent", "validate", "--all", "--json"])
        .output()
        .expect("run octocode agent validate all");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("validate json parses");
    let report = json["reports"]
        .as_array()
        .expect("reports array")
        .iter()
        .find(|report| report["name"] == "toml-explorer")
        .expect("toml-explorer report");
    assert_eq!(report["valid"], true);
}

#[test]
fn octo_agent_run_dry_run_json_does_not_require_api_key() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octo_command(root.path())
        .args([
            "agent",
            "run",
            "code-explorer",
            "inspect cli agent",
            "--dry-run",
            "--json",
        ])
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .expect("run octo agent dry-run");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    assert!(
        !stderr(&output).contains("API key"),
        "dry-run should not resolve API keys"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("agent dry-run json parses");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["success"], true);
    assert_eq!(json["plan"]["would_request_api_key"], false);
    assert_eq!(json["plan"]["network_required"], false);
    assert_eq!(json["agent"], "code-explorer");
}

#[test]
fn octo_agent_run_dry_run_json_applies_overrides() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octo_command(root.path())
        .args([
            "agent",
            "run",
            "code-explorer",
            "inspect cli agent",
            "--focus",
            "src/cli/agent.rs",
            "--max-turns",
            "1",
            "--model",
            "flash",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("run octo agent dry-run with overrides");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("agent dry-run json parses");
    assert_eq!(json["plan"]["max_turns"], 1);
    assert_eq!(json["plan"]["model"], "deepseek-v4-flash");
    assert_eq!(json["plan"]["focus_files"][0], "src/cli/agent.rs");
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

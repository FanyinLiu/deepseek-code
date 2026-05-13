use std::process::Command;

fn ds_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn agent_list_json_contains_built_ins() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = ds_command(root.path())
        .args(["agent", "list", "--json"])
        .output()
        .expect("run ds agent list");

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
    let output = ds_command(root.path())
        .args(["agent", "show", "code-reviewer", "--json"])
        .output()
        .expect("run ds agent show");

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
    let output = ds_command(root.path())
        .args(["agent", "show", "missing-agent"])
        .output()
        .expect("run ds agent show missing");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown agent 'missing-agent'"));
}

#[test]
fn agent_create_template_writes_file_and_validate_json_parses() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = ds_command(root.path())
        .args(["agent", "create", "my-auditor", "--template", "auditor"])
        .output()
        .expect("run ds agent create");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let path = root
        .path()
        .join(".deepseek-code")
        .join("agents")
        .join("my-auditor.md");
    assert!(path.exists());

    let output = ds_command(root.path())
        .args(["agent", "validate", "--all", "--json"])
        .output()
        .expect("run ds agent validate all");

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
    let agents_dir = root.path().join(".deepseek-code").join("agents");
    std::fs::create_dir_all(&agents_dir).expect("create agents dir");
    std::fs::write(agents_dir.join("broken.md"), "---\nnot-valid =\n---\n\n")
        .expect("write broken agent");

    let output = ds_command(root.path())
        .args(["agent", "validate", "--all", "--json"])
        .output()
        .expect("run ds agent validate");

    assert!(output.status.success(), "stderr={}", stderr(&output));
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

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

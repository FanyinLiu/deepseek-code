use std::process::Command;

fn octocode_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octocode"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn commands_list_json_exposes_builtin_slash_commands() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["commands", "list", "--json"])
        .output()
        .expect("run octocode commands list");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("commands list json parses");
    let builtins = json["builtins"].as_array().expect("builtins");
    let help = builtins
        .iter()
        .find(|cmd| cmd["name"] == "/help")
        .expect("/help command");
    assert_eq!(help["group"], "local");
    assert!(help["usage"].as_str().unwrap().contains("/help"));
}

#[test]
fn commands_list_discovers_project_custom_markdown_commands() {
    let root = tempfile::tempdir().expect("tempdir");
    let command_dir = root.path().join(".octocode").join("commands");
    std::fs::create_dir_all(&command_dir).expect("create commands dir");
    std::fs::write(
        command_dir.join("fix-docs.md"),
        "---\ndescription: Fix documentation drift\n---\n\n# Fix docs\n",
    )
    .expect("write custom command");

    let output = octocode_command(root.path())
        .args(["commands", "list", "--json", "--filter", "docs"])
        .output()
        .expect("run octocode commands list");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("commands list json parses");
    let custom = json["custom"].as_array().expect("custom commands");
    let command = custom
        .iter()
        .find(|cmd| cmd["name"] == "/fix-docs")
        .expect("custom command");
    assert_eq!(command["source"], "project");
    assert_eq!(command["description"], "Fix documentation drift");
    assert_eq!(command["conflicts_builtin"], false);
}

#[test]
fn commands_list_marks_builtin_alias_conflicts() {
    let root = tempfile::tempdir().expect("tempdir");
    let command_dir = root.path().join(".octocode").join("commands");
    std::fs::create_dir_all(&command_dir).expect("create commands dir");
    std::fs::write(command_dir.join("h.md"), "# Custom help alias\n").expect("write alias command");

    let output = octocode_command(root.path())
        .args(["commands", "list", "--json", "--filter", "/h"])
        .output()
        .expect("run octocode commands list");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("commands list json parses");
    let custom = json["custom"].as_array().expect("custom commands");
    let command = custom
        .iter()
        .find(|cmd| cmd["name"] == "/h")
        .expect("custom alias command");
    assert_eq!(command["conflicts_builtin"], true);
}

#[test]
fn commands_list_includes_skills_mcp_prompt_namespaces_and_conflict_groups() {
    let root = tempfile::tempdir().expect("tempdir");
    let deepseek_dir = root.path().join(".octocode");
    let skill_dir = deepseek_dir.join("skills").join("reviewer");
    let command_dir = deepseek_dir.join("commands");
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    std::fs::create_dir_all(&command_dir).expect("create command dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: Review code with local rules\n---\n\n# Reviewer\n",
    )
    .expect("write skill");
    std::fs::write(command_dir.join("help.md"), "# Custom help\n").expect("write conflict");
    std::fs::write(
        deepseek_dir.join("config.toml"),
        r#"
[mcp]
enabled = true

[mcp.servers.docs]
transport = "stdio"
command = "docs-mcp"
"#,
    )
    .expect("write config");

    let output = octocode_command(root.path())
        .args(["commands", "list", "--json"])
        .output()
        .expect("run octocode commands list");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("commands list json parses");
    let skills = json["skills"].as_array().expect("skills");
    assert!(skills.iter().any(|skill| {
        skill["name"] == "/skill:reviewer" && skill["description"] == "Review code with local rules"
    }));
    let prompts = json["mcp_prompts"].as_array().expect("mcp prompts");
    assert!(prompts
        .iter()
        .any(|prompt| prompt["name"] == "mcp:docs:prompts"));
    let conflicts = json["conflicts"].as_array().expect("conflicts");
    assert!(conflicts
        .iter()
        .any(|conflict| conflict["name"] == "/help" && conflict["builtin"] == true));
}

#[test]
fn commands_locations_json_includes_project_location() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["commands", "locations", "--json"])
        .output()
        .expect("run octocode commands locations");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("locations json parses");
    let locations = json.as_array().expect("locations");
    assert!(locations.iter().any(|location| {
        location["source"] == "project"
            && location["path"]
                .as_str()
                .is_some_and(|path| path.replace('\\', "/").ends_with(".octocode/commands"))
    }));
}

#[test]
fn commands_show_json_renders_custom_markdown_prompt() {
    let root = tempfile::tempdir().expect("tempdir");
    let command_dir = root.path().join(".octocode").join("commands");
    std::fs::create_dir_all(&command_dir).expect("create commands dir");
    std::fs::write(
        command_dir.join("review-docs.md"),
        "---\ndescription: Review docs\nargument-hint: <path>\n---\n\nReview $ARGUMENTS for clarity.\n",
    )
    .expect("write custom command");

    let output = octocode_command(root.path())
        .args(["commands", "show", "/review-docs", "--json"])
        .output()
        .expect("run octocode commands show");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("commands show json parses");
    assert_eq!(json["name"], "/review-docs");
    assert_eq!(json["description"], "Review docs");
    assert_eq!(json["argument_hint"], "<path>");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["prompt"], "Review $ARGUMENTS for clarity.");
}

#[test]
fn commands_run_dry_run_expands_arguments_without_model_call() {
    let root = tempfile::tempdir().expect("tempdir");
    let command_dir = root.path().join(".octocode").join("commands");
    std::fs::create_dir_all(&command_dir).expect("create commands dir");
    std::fs::write(
        command_dir.join("fix-docs.md"),
        "---\ndescription: Fix docs\n---\n\nFix documentation drift for {{arguments}}.\n",
    )
    .expect("write custom command");

    let output = octocode_command(root.path())
        .args([
            "commands",
            "run",
            "/fix-docs",
            "--dry-run",
            "--json",
            "README.md",
            "docs/",
        ])
        .output()
        .expect("run octocode commands run dry-run");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("commands run json parses");
    assert_eq!(json["name"], "/fix-docs");
    assert_eq!(json["args"], serde_json::json!(["README.md", "docs/"]));
    assert_eq!(
        json["prompt"],
        "Fix documentation drift for README.md docs/."
    );
    assert_eq!(json["dry_run"], true);
}

#[test]
fn commands_run_dry_run_supports_toml_prompt_files() {
    let root = tempfile::tempdir().expect("tempdir");
    let command_dir = root.path().join(".octocode").join("commands");
    std::fs::create_dir_all(&command_dir).expect("create commands dir");
    std::fs::write(
        command_dir.join("audit.toml"),
        r#"
description = "Audit a target"
argument_hint = "<target>"
prompt = "Audit $ARGUMENTS and return risks."
"#,
    )
    .expect("write custom command");

    let output = octocode_command(root.path())
        .args(["commands", "run", "audit", "--dry-run", "--json", "src/cli"])
        .output()
        .expect("run octocode commands run dry-run");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("commands run json parses");
    assert_eq!(json["name"], "/audit");
    assert_eq!(json["description"], "Audit a target");
    assert_eq!(json["argument_hint"], "<target>");
    assert_eq!(json["prompt"], "Audit src/cli and return risks.");
}

#[test]
fn commands_run_rejects_builtin_conflicts() {
    let root = tempfile::tempdir().expect("tempdir");
    let command_dir = root.path().join(".octocode").join("commands");
    std::fs::create_dir_all(&command_dir).expect("create commands dir");
    std::fs::write(command_dir.join("help.md"), "# Custom help\n").expect("write custom command");

    let output = octocode_command(root.path())
        .args(["commands", "run", "/help", "--dry-run"])
        .output()
        .expect("run octocode commands run dry-run");

    assert!(!output.status.success(), "stdout={}", stdout(&output));
    assert!(stderr(&output).contains("conflicts with a built-in slash command"));
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

use std::process::Command;

fn octo_command(project_root: &std::path::Path, home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octo"));
    command.arg("-C").arg(project_root);
    command.env("HOME", home);
    command.env("DEEPSEEK_CODE_HOME", home);
    command.current_dir(project_root);
    command
}

#[test]
fn usage_summary_json_groups_local_events_by_model() {
    let root = tempfile::tempdir().expect("project tempdir");
    let home = tempfile::tempdir().expect("home tempdir");
    let usage_dir = root.path().join(".octocode").join("usage");
    std::fs::create_dir_all(&usage_dir).expect("create usage dir");
    let now = chrono::Utc::now().to_rfc3339();
    std::fs::write(
        usage_dir.join("events.jsonl"),
        format!(
            r#"{{"timestamp":"{now}","provider":"deepseek","model":"deepseek-v4-flash","prompt_tokens":100,"completion_tokens":25,"total_tokens":125,"cache_read_tokens":80,"cache_write_tokens":0,"cache_miss_tokens":20,"source":"model_response"}}
{{"timestamp":"{now}","provider":"deepseek","model":"deepseek-v4-pro","prompt_tokens":50,"completion_tokens":10,"total_tokens":60,"cache_read_tokens":0,"cache_write_tokens":0,"cache_miss_tokens":0,"source":"model_response"}}
"#
        ),
    )
    .expect("write usage events");

    let output = octo_command(root.path(), home.path())
        .args(["usage", "summary", "--json"])
        .output()
        .expect("run octo usage summary");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("usage summary json parses");
    assert_eq!(json["totals"]["requests"], 2);
    assert_eq!(json["totals"]["total_tokens"], 185);
    assert_eq!(json["by_model"].as_array().expect("by_model").len(), 2);
    assert_eq!(json["by_model"][0]["model"], "deepseek-v4-flash");
    assert_eq!(json["by_model"][0]["cache_hit_rate"], 0.8);
}

#[test]
fn usage_summary_human_handles_empty_store() {
    let root = tempfile::tempdir().expect("project tempdir");
    let home = tempfile::tempdir().expect("home tempdir");
    std::fs::create_dir_all(root.path().join(".octocode")).expect("create marker");

    let output = octo_command(root.path(), home.path())
        .args(["usage", "summary"])
        .output()
        .expect("run octo usage summary");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No usage events recorded yet."));
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

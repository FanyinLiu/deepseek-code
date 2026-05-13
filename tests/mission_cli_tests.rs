use std::process::Command;

fn ds_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn mission_new_dry_run_json_creates_record() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = ds_command(root.path())
        .args([
            "mission",
            "new",
            "analyze src/agent architecture",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("run mission new");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mission new json parses");
    let mission_id = json["mission"]["id"].as_str().expect("mission id");
    assert!(mission_id.starts_with("mission-"));
    assert_eq!(json["state"]["status"], "completed");
    assert!(root
        .path()
        .join(".deepseek-code")
        .join("missions")
        .join(mission_id)
        .join("mission.json")
        .exists());
}

#[test]
fn mission_status_inspect_and_replay_latest_work() {
    let root = tempfile::tempdir().expect("tempdir");
    let create = ds_command(root.path())
        .args(["mission", "new", "refactor src/agent safely", "--dry-run"])
        .output()
        .expect("run mission new");
    assert!(create.status.success(), "stderr={}", stderr(&create));

    let status = ds_command(root.path())
        .args(["mission", "status", "latest"])
        .output()
        .expect("run mission status");
    assert!(status.status.success(), "stderr={}", stderr(&status));
    assert!(stdout(&status).contains("status  completed"));

    let inspect = ds_command(root.path())
        .args(["mission", "inspect", "latest", "--json", "--events"])
        .output()
        .expect("run mission inspect");
    assert!(inspect.status.success(), "stderr={}", stderr(&inspect));
    let json: serde_json::Value =
        serde_json::from_slice(&inspect.stdout).expect("inspect json parses");
    assert_eq!(json["events"].as_array().expect("events").len(), 3);

    let replay = ds_command(root.path())
        .args(["mission", "replay", "latest"])
        .output()
        .expect("run mission replay");
    assert!(replay.status.success(), "stderr={}", stderr(&replay));
    assert!(stdout(&replay).contains("replayed status  completed"));
}

#[test]
fn mission_list_json_parses() {
    let root = tempfile::tempdir().expect("tempdir");
    let create = ds_command(root.path())
        .args(["mission", "new", "review src/agent for safety", "--dry-run"])
        .output()
        .expect("run mission new");
    assert!(create.status.success(), "stderr={}", stderr(&create));

    let list = ds_command(root.path())
        .args(["mission", "list", "--json"])
        .output()
        .expect("run mission list");
    assert!(list.status.success(), "stderr={}", stderr(&list));
    let json: serde_json::Value = serde_json::from_slice(&list.stdout).expect("list json parses");
    assert_eq!(json["missions"].as_array().expect("missions").len(), 1);
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

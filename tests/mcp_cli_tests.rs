use std::process::Command;

fn ds_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn mcp_add_list_remove_updates_project_local_config() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = ds_command(root.path())
        .args([
            "mcp",
            "add",
            "localfs",
            "--transport",
            "stdio",
            "--command",
            "mcp-filesystem",
            "--arg",
            ".",
            "--timeout-ms",
            "1000",
        ])
        .output()
        .expect("run ds mcp add");

    assert!(output.status.success(), "stderr={}", stderr(&output));

    let output = ds_command(root.path())
        .args(["mcp", "list", "--json"])
        .output()
        .expect("run ds mcp list");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp list json parses");
    let server = json["servers"]
        .as_array()
        .expect("servers")
        .iter()
        .find(|server| server["name"] == "localfs")
        .expect("localfs server");
    assert_eq!(server["transport"], "stdio");
    assert_eq!(server["timeout_ms"], 1000);

    let output = ds_command(root.path())
        .args(["mcp", "get", "localfs", "--json"])
        .output()
        .expect("run ds mcp get");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp get json parses");
    assert_eq!(json["name"], "localfs");
    assert_eq!(json["transport"], "stdio");

    let output = ds_command(root.path())
        .args(["mcp", "remove", "localfs"])
        .output()
        .expect("run ds mcp remove");
    assert!(output.status.success(), "stderr={}", stderr(&output));
}

#[test]
fn mcp_status_json_does_not_connect_by_default() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = ds_command(root.path())
        .args(["mcp", "status", "--json"])
        .output()
        .expect("run ds mcp status");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp status json parses");
    assert_eq!(json["enabled"], false);
    assert_eq!(json["connected"], false);
    assert!(json["servers"].as_array().expect("servers").is_empty());
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

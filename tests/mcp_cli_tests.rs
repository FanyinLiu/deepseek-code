use std::process::Command;

fn octocode_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octocode"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn mcp_add_list_remove_updates_project_local_config() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
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
        .expect("run octocode mcp add");

    assert!(output.status.success(), "stderr={}", stderr(&output));

    let output = octocode_command(root.path())
        .args(["mcp", "list", "--json"])
        .output()
        .expect("run octocode mcp list");
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

    let output = octocode_command(root.path())
        .args(["mcp", "get", "localfs", "--json"])
        .output()
        .expect("run octocode mcp get");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp get json parses");
    assert_eq!(json["name"], "localfs");
    assert_eq!(json["transport"], "stdio");

    let output = octocode_command(root.path())
        .args(["mcp", "remove", "localfs"])
        .output()
        .expect("run octocode mcp remove");
    assert!(output.status.success(), "stderr={}", stderr(&output));
}

#[test]
fn mcp_status_json_does_not_connect_by_default() {
    let root = tempfile::tempdir().expect("tempdir");
    let output = octocode_command(root.path())
        .args(["mcp", "status", "--json"])
        .output()
        .expect("run octocode mcp status");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp status json parses");
    assert_eq!(json["enabled"], false);
    assert_eq!(json["connected"], false);
    assert!(json["servers"].as_array().expect("servers").is_empty());
}

#[test]
fn mcp_tools_list_and_call_fixture_server() {
    let Some(python) = python_command() else {
        eprintln!("skipping MCP fixture test because python is unavailable");
        return;
    };
    let root = tempfile::tempdir().expect("tempdir");
    let script = write_fixture_server(root.path());
    write_fixture_config(root.path(), &python, &script);

    let output = octocode_command(root.path())
        .args(["mcp", "tools", "list", "fixture", "--json"])
        .output()
        .expect("run octocode mcp tools list");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp tools list json parses");
    assert_eq!(json["server"], "fixture");
    assert_eq!(json["tools"][0]["name"], "echo");

    let output = octocode_command(root.path())
        .args([
            "mcp",
            "tools",
            "call",
            "fixture",
            "echo",
            "--arguments",
            r#"{"text":"hello"}"#,
            "--json",
        ])
        .output()
        .expect("run octocode mcp tools call");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp tools call json parses");
    assert_eq!(json["server"], "fixture");
    assert_eq!(json["tool"], "echo");
    assert_eq!(json["result"]["content"][0]["text"], "echo: hello");
}

#[test]
fn mcp_resources_list_and_read_fixture_server() {
    let Some(python) = python_command() else {
        eprintln!("skipping MCP fixture test because python is unavailable");
        return;
    };
    let root = tempfile::tempdir().expect("tempdir");
    let script = write_fixture_server(root.path());
    write_fixture_config(root.path(), &python, &script);

    let output = octocode_command(root.path())
        .args(["mcp", "resources", "list", "fixture", "--json"])
        .output()
        .expect("run octocode mcp resources list");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp resources list json parses");
    assert_eq!(json["server"], "fixture");
    assert_eq!(json["resources"][0]["uri"], "fixture://guide");

    let output = octocode_command(root.path())
        .args([
            "mcp",
            "resources",
            "read",
            "fixture",
            "fixture://guide",
            "--json",
        ])
        .output()
        .expect("run octocode mcp resources read");
    assert!(output.status.success(), "stderr={}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("mcp resources read json parses");
    assert_eq!(json["server"], "fixture");
    assert_eq!(json["uri"], "fixture://guide");
    assert_eq!(json["result"]["contents"][0]["text"], "fixture guide");
}

fn python_command() -> Option<String> {
    for candidate in ["python3", "python"] {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return Some(candidate.to_string());
        }
    }
    None
}

fn write_fixture_config(root: &std::path::Path, python: &str, script: &std::path::Path) {
    let config_dir = root.join(".octocode");
    std::fs::create_dir_all(&config_dir).expect("create .octocode");
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
[mcp]
enabled = true

[mcp.servers.fixture]
transport = "stdio"
command = "{}"
args = ["{}"]
timeout_ms = 5000
"#,
            python.replace('\\', "\\\\").replace('"', "\\\""),
            script
                .display()
                .to_string()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        ),
    )
    .expect("write MCP config");
}

fn write_fixture_server(root: &std::path::Path) -> std::path::PathBuf {
    let script = root.join("mcp_fixture.py");
    std::fs::write(
        &script,
        r#"
import json
import sys


def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode("ascii").split(":", 1)
        headers[key.lower()] = value.strip()
    length = int(headers["content-length"])
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))


def send(payload):
    body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()


while True:
    request = read_message()
    if request is None:
        break
    method = request.get("method")
    if "id" not in request:
        if method == "exit":
            break
        continue
    request_id = request["id"]
    if method == "initialize":
        result = {
            "protocol_version": "2024-11-05",
            "capabilities": {
                "tools": {"list_changed": False},
                "resources": {"subscribe": False, "list_changed": False},
            },
            "server_info": {"name": "fixture", "version": "1.0"},
        }
    elif method == "tools/list":
        result = {
            "tools": [
                {
                    "name": "echo",
                    "description": "Echo text",
                    "input_schema": {"type": "object"},
                }
            ]
        }
    elif method == "tools/call":
        args = request.get("params", {}).get("arguments", {})
        result = {
            "content": [{"type": "text", "text": "echo: " + args.get("text", "")}],
            "is_error": False,
        }
    elif method == "resources/list":
        result = {
            "resources": [
                {
                    "uri": "fixture://guide",
                    "name": "guide",
                    "description": "Fixture guide",
                    "mime_type": "text/plain",
                }
            ]
        }
    elif method == "resources/read":
        result = {
            "contents": [
                {
                    "type": "text",
                    "uri": "fixture://guide",
                    "mime_type": "text/plain",
                    "text": "fixture guide",
                }
            ]
        }
    elif method == "shutdown":
        result = {}
    else:
        send({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": method}})
        continue
    send({"jsonrpc": "2.0", "id": request_id, "result": result})
"#,
    )
    .expect("write MCP fixture server");
    script
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

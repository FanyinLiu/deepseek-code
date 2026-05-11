use deepseek_code::deepseek::errors::DeepSeekError;
use deepseek_code::deepseek::{ToolCall, ToolCallFunction};

fn tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: format!("call_{name}"),
        call_type: "function".to_string(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

#[test]
fn write_file_creates_new_file() {
    let root = tempfile::tempdir().expect("tempdir");

    let result = deepseek_code::tools::write_file(
        root.path(),
        "nested/new_file.txt",
        "hello from a new file\n",
    )
    .expect("write_file should create new files");

    assert_eq!(
        std::fs::read_to_string(root.path().join("nested/new_file.txt"))
            .expect("new file should exist"),
        "hello from a new file\n"
    );
    assert!(result.diff.contains("hello from a new file"));
}

#[test]
fn write_file_rejects_path_escape_and_protected_paths() {
    let root = tempfile::tempdir().expect("tempdir");

    let escape_err = deepseek_code::tools::write_file(root.path(), "../outside.txt", "nope")
        .expect_err("path escape should be rejected");
    assert!(
        escape_err.to_string().contains("path escapes project root"),
        "unexpected escape error: {escape_err}"
    );

    let protected_err = deepseek_code::tools::write_file(root.path(), ".env", "SECRET=1\n")
        .expect_err("protected path should be rejected");
    assert!(
        protected_err.to_string().contains("path is protected"),
        "unexpected protected path error: {protected_err}"
    );
}

#[tokio::test]
async fn dispatch_recognizes_apply_patch() {
    let root = tempfile::tempdir().expect("tempdir");
    let call = tool_call(
        "apply_patch",
        serde_json::json!({
            "patch": "this is not a unified diff\n"
        }),
    );

    let (output, is_error) =
        deepseek_code::tools::dispatch::execute_single_tool(&call, root.path()).await;

    assert!(is_error, "invalid patch should fail");
    assert!(
        !output.contains("unknown tool"),
        "apply_patch should be dispatched, got: {output}"
    );
    assert!(
        output.contains("patch apply failed") || output.contains("git executable not found"),
        "unexpected apply_patch output: {output}"
    );
}

#[tokio::test]
async fn dispatch_read_file_accepts_absolute_local_path() {
    let root = tempfile::tempdir().expect("workspace");
    let outside = tempfile::NamedTempFile::new().expect("outside file");
    std::fs::write(outside.path(), "hello from outside workspace\n").expect("write outside file");
    let call = tool_call(
        "read_file",
        serde_json::json!({
            "path": outside.path().to_string_lossy()
        }),
    );

    let (output, is_error) =
        deepseek_code::tools::dispatch::execute_single_tool(&call, root.path()).await;

    assert!(
        !is_error,
        "absolute read should work after approval: {output}"
    );
    assert!(output.contains("hello from outside workspace"));
}

#[tokio::test]
async fn dispatch_uses_configurable_run_command_timeout() {
    let root = tempfile::tempdir().expect("tempdir");
    let command = if cfg!(windows) {
        "ping 127.0.0.1 -n 3"
    } else {
        "sleep 2"
    };
    let call = tool_call(
        "run_command",
        serde_json::json!({
            "command": command,
            "timeout_seconds": 1
        }),
    );

    let (output, is_error) =
        deepseek_code::tools::dispatch::execute_single_tool(&call, root.path()).await;

    assert!(is_error, "timed out command should be an error");
    assert!(
        output.contains("timed out after 1s"),
        "dispatch should pass timeout_seconds through, got: {output}"
    );
}

#[tokio::test]
async fn dispatch_uses_policy_default_run_command_timeout() {
    let root = tempfile::tempdir().expect("tempdir");
    let command = if cfg!(windows) {
        "ping 127.0.0.1 -n 3"
    } else {
        "sleep 2"
    };
    let call = tool_call(
        "run_command",
        serde_json::json!({
            "command": command
        }),
    );
    let dispatch_config = deepseek_code::tools::dispatch::ToolDispatchConfig {
        command_timeout_seconds: 1,
    };

    let (output, is_error) = deepseek_code::tools::dispatch::execute_single_tool_with_config(
        &call,
        root.path(),
        dispatch_config,
    )
    .await;

    assert!(is_error, "timed out command should be an error");
    assert!(
        output.contains("timed out after 1s"),
        "dispatch should use the configured default timeout, got: {output}"
    );
}

#[tokio::test]
async fn dispatch_write_file_does_not_auto_run_tests() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"dispatch-auto-test-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir(root.path().join("src")).expect("create src");
    std::fs::write(
        root.path().join("src").join("lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .expect("write lib.rs");
    let call = tool_call(
        "write_file",
        serde_json::json!({
            "path": "src/generated.rs",
            "content": "pub const GENERATED: bool = true;\n"
        }),
    );

    let (output, is_error) =
        deepseek_code::tools::dispatch::execute_single_tool(&call, root.path()).await;

    assert!(!is_error, "write_file should succeed: {output}");
    assert!(
        !output.contains("[Auto-test]"),
        "write_file dispatch should not run implicit tests: {output}"
    );
}

#[test]
fn run_command_schema_exposes_timeout_seconds() {
    let tools = deepseek_code::deepseek::tools::standard_tool_definitions();
    let run_command = tools
        .iter()
        .find(|tool| tool.function.name == "run_command")
        .expect("run_command tool should exist");

    assert!(
        run_command.function.parameters["properties"]
            .get("timeout_seconds")
            .is_some(),
        "run_command schema should expose timeout_seconds"
    );
}

#[test]
fn deepseek_retries_408_and_504() {
    let request_timeout = DeepSeekError::from_status(408, "request timeout");
    let gateway_timeout = DeepSeekError::from_status(504, "gateway timeout");

    assert!(request_timeout.is_retryable());
    assert!(gateway_timeout.is_retryable());
    assert!(matches!(request_timeout, DeepSeekError::ServerError(408)));
    assert!(matches!(gateway_timeout, DeepSeekError::ServerError(504)));
}

#[tokio::test]
async fn fetch_url_blocks_local_addresses_before_connecting() {
    let err = deepseek_code::tools::fetch_url::fetch_url("http://127.0.0.1:1/", 100)
        .await
        .expect_err("loopback fetch should be blocked");

    assert!(
        err.to_string().contains("blocked local/private/link-local"),
        "unexpected fetch_url error: {err}"
    );
}

//! MCP server: expose octocode's tools to external agents over JSON-RPC.
//!
//! Run via `octo mcp serve --stdio`. Reads JSON-RPC messages from stdin,
//! writes responses to stdout. Each message is a single line of JSON.
//!
//! ## Security
//!
//! Only **read-only** tools are exposed by default. Destructive operations
//! (run_command, write_file, edit_file, …) require an explicit
//! `mcp.server.allow_destructive = true` flag in the project config, and even
//! then go through `policy::approvals::evaluate_tool` first. There is no
//! human-in-the-loop in serve mode — denied calls return a CallToolResult
//! with `is_error: true` so the client sees the refusal.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::deepseek::{
    models::ToolDefinition, tools::standard_tool_definitions, ToolCall, ToolCallFunction,
};
use crate::mcp::protocol::{
    CallToolResult, Implementation, InitializeResult, ListToolsResult, McpTool, ServerCapabilities,
    ToolContent, ToolsCapability, PROTOCOL_VERSION,
};
use crate::tools::metadata;

/// Run the MCP server on stdio until EOF.
pub async fn serve_stdio(project_root: PathBuf, allow_destructive: bool) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut writer = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    tracing::info!(
        "MCP server starting on stdio (project={}, allow_destructive={})",
        project_root.display(),
        allow_destructive
    );

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&line, &project_root, allow_destructive).await {
            writer.write_all(response.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
    }

    tracing::info!("MCP server stdio closed");
    Ok(())
}

async fn handle_message(
    line: &str,
    project_root: &Path,
    allow_destructive: bool,
) -> Option<String> {
    let parsed: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!("MCP: invalid JSON: {err}");
            return None;
        }
    };

    let method = parsed.get("method").and_then(Value::as_str)?;
    let id = parsed.get("id").cloned();
    let params = parsed.get("params").cloned().unwrap_or(json!({}));

    // Notifications (no id) get no response.
    let is_notification = id.is_none();

    let result_value = match method {
        "initialize" => Ok(serde_json::to_value(initialize_result()).ok()?),
        "initialized" | "notifications/initialized" => return None,
        "tools/list" => Ok(serde_json::to_value(list_tools(allow_destructive)).ok()?),
        "tools/call" => call_tool(params, project_root, allow_destructive).await,
        "ping" => Ok(json!({})),
        other => Err(format!("method not found: {other}")),
    };

    if is_notification {
        return None;
    }

    let id = id.unwrap_or(Value::Null);
    let response = match result_value {
        Ok(value) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": value,
        }),
        Err(msg) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32603,
                "message": msg,
            },
        }),
    };
    Some(response.to_string())
}

fn initialize_result() -> InitializeResult {
    InitializeResult {
        protocol_version: PROTOCOL_VERSION.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: false,
            }),
            resources: None,
        },
        server_info: Implementation {
            name: "octocode".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    }
}

fn list_tools(allow_destructive: bool) -> ListToolsResult {
    let defs = standard_tool_definitions();
    let tools = defs
        .into_iter()
        .filter(|def| tool_is_exposed(&def.function.name, allow_destructive))
        .map(definition_to_mcp_tool)
        .collect();
    ListToolsResult {
        tools,
        next_cursor: None,
    }
}

fn tool_is_exposed(name: &str, allow_destructive: bool) -> bool {
    // Always expose read-only tools. Mutating/destructive tools only when
    // explicitly opted-in via config.
    metadata::is_read_only(name) || allow_destructive
}

fn definition_to_mcp_tool(def: ToolDefinition) -> McpTool {
    McpTool {
        name: def.function.name,
        description: Some(def.function.description),
        input_schema: def.function.parameters,
    }
}

async fn call_tool(
    params: Value,
    project_root: &Path,
    allow_destructive: bool,
) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing tool name")?
        .to_string();
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    if !tool_is_exposed(&name, allow_destructive) {
        // Refuse without invoking dispatch — return as MCP-level error result
        // so the calling agent learns the policy rather than a JSON-RPC fault.
        let result = CallToolResult {
            content: vec![ToolContent::Text {
                text: format!(
                    "octocode refused: tool `{name}` is not exposed by MCP server \
                     (read-only mode). Set mcp.server.allow_destructive = true to allow."
                ),
            }],
            is_error: true,
        };
        return serde_json::to_value(result).map_err(|e| e.to_string());
    }

    let call = ToolCall {
        id: format!("mcp-{}", uuid::Uuid::new_v4()),
        call_type: "function".to_string(),
        function: ToolCallFunction {
            name: name.clone(),
            arguments: arguments.to_string(),
        },
    };

    let (content, is_error) = crate::tools::dispatch::execute_single_tool_with_config(
        &call,
        project_root,
        crate::tools::dispatch::ToolDispatchConfig::default(),
    )
    .await;

    let result = CallToolResult {
        content: vec![ToolContent::Text { text: content }],
        is_error,
    };
    serde_json::to_value(result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_tools_are_always_exposed() {
        assert!(tool_is_exposed("read_file", false));
        assert!(tool_is_exposed("grep", false));
        assert!(tool_is_exposed("git_status", false));
    }

    #[test]
    fn destructive_tools_gated_by_flag() {
        assert!(!tool_is_exposed("write_file", false));
        assert!(!tool_is_exposed("run_command", false));
        assert!(tool_is_exposed("write_file", true));
        assert!(tool_is_exposed("run_command", true));
    }

    #[test]
    fn unknown_tool_is_never_exposed_in_safe_mode() {
        // Unknown name not in metadata -> is_read_only=false -> blocked when
        // allow_destructive=false.
        assert!(!tool_is_exposed("does_not_exist", false));
    }

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let response = handle_message(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &PathBuf::from("."),
            false,
        )
        .await
        .expect("initialize should respond");
        assert!(response.contains("octocode"));
        assert!(response.contains("\"id\":1"));
    }

    #[tokio::test]
    async fn list_tools_returns_only_read_only_by_default() {
        let response = handle_message(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            &PathBuf::from("."),
            false,
        )
        .await
        .expect("tools/list should respond");
        assert!(response.contains("read_file"));
        assert!(!response.contains("\"name\":\"write_file\""));
        assert!(!response.contains("\"name\":\"run_command\""));
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let response = handle_message(
            r#"{"jsonrpc":"2.0","id":3,"method":"unknown/foo","params":{}}"#,
            &PathBuf::from("."),
            false,
        )
        .await
        .expect("should respond with error");
        assert!(response.contains("method not found"));
    }

    #[tokio::test]
    async fn notification_returns_none() {
        // No id => notification, no response expected.
        let response = handle_message(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
            &PathBuf::from("."),
            false,
        )
        .await;
        assert!(response.is_none());
    }
}

//! MCP server: expose octocode's tools to external agents over JSON-RPC.
//!
//! Run via `octo mcp serve --stdio`. Reads standard MCP `Content-Length`
//! JSON-RPC frames from stdin and writes framed responses to stdout.
//!
//! ## Security
//!
//! Only **read-only** tools are exposed by default. Destructive operations
//! (run_command, write_file, edit_file, …) require an explicit
//! `mcp.server.allow_destructive = true` flag in the project config, and even
//! then go through the shared `ToolRuntime` policy, hook, approval, and
//! dispatch path. Approval prompts use the file-backed approval bridge under
//! `.octocode/approvals` so stdio remains clean JSON-RPC.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::deepseek::{
    models::ToolDefinition, tools::standard_tool_definitions, ToolCall, ToolCallFunction,
};
use crate::mcp::approval_bridge::FileApprovalBridge;
use crate::mcp::protocol::{
    CallToolResult, Implementation, InitializeResult, ListToolsResult, McpTool, ServerCapabilities,
    ToolContent, ToolsCapability, PROTOCOL_VERSION,
};
use crate::policy::ToolCallSource;
use crate::runtime::tool_runtime::{LocalDispatchRuntimeBackend, ToolRuntime, ToolRuntimeContext};
use crate::storage::config::PolicyConfig;
use crate::tools::{metadata, registry};

const MAX_MCP_CONTENT_LENGTH: usize = 8 * 1024 * 1024;
const MAX_MCP_HEADER_LINE_LENGTH: usize = 8 * 1024;

/// Run the MCP server on stdio until EOF.
pub async fn serve_stdio(project_root: PathBuf, allow_destructive: bool) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut writer = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let config = crate::storage::Config::load(Some(&project_root)).unwrap_or_default();
    let policy = config.policy;

    tracing::info!(
        "MCP server starting on stdio (project={}, allow_destructive={})",
        project_root.display(),
        allow_destructive
    );

    while let Some(message) = read_stdio_message(&mut reader).await? {
        if let Some(response) =
            handle_message(&message, &project_root, allow_destructive, &policy).await
        {
            writer
                .write_all(encode_stdio_frame(&response).as_bytes())
                .await?;
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
    policy: &PolicyConfig,
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
        "tools/call" => call_tool(params, project_root, allow_destructive, policy).await,
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
    metadata::metadata(name).is_some_and(|meta| meta.read_only || allow_destructive)
        && registry::get(name).is_some()
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
    policy: &PolicyConfig,
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

    let runtime = ToolRuntime::new(project_root, policy.clone());
    let dispatch_config = crate::tools::dispatch::ToolDispatchConfig::from_policy(policy);
    let context = ToolRuntimeContext::new("mcp-stdio", dispatch_config);
    let mut resolver = FileApprovalBridge::new(project_root);
    let mut backend = LocalDispatchRuntimeBackend;
    let outcome = runtime
        .execute(
            &call,
            ToolCallSource::Mcp,
            context,
            &mut resolver,
            &mut backend,
        )
        .await;

    let result = CallToolResult {
        content: vec![ToolContent::Text {
            text: outcome.result_record.result,
        }],
        is_error: outcome.result_record.is_error,
    };
    serde_json::to_value(result).map_err(|e| e.to_string())
}

fn encode_stdio_frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

async fn read_stdio_message<R>(reader: &mut R) -> Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    read_stdio_message_with_limits(reader, MAX_MCP_CONTENT_LENGTH, MAX_MCP_HEADER_LINE_LENGTH).await
}

async fn read_stdio_message_with_limits<R>(
    reader: &mut R,
    max_content_length: usize,
    max_header_line_length: usize,
) -> Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let first_line = loop {
        let line = match read_limited_line(reader, max_content_length).await? {
            Some(line) => line,
            None => return Ok(None),
        };
        if !line.trim().is_empty() {
            break line;
        }
    };

    let trimmed = first_line.trim_start();
    if trimmed.starts_with('{') {
        return Ok(Some(first_line.trim().to_string()));
    }
    if first_line.len() > max_header_line_length {
        anyhow::bail!("MCP header line too long");
    }

    let mut content_length = parse_content_length_header(&first_line)?;
    loop {
        let line = read_limited_line(reader, max_header_line_length)
            .await?
            .ok_or_else(|| anyhow::anyhow!("EOF while reading MCP headers"))?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(len) = parse_content_length_header(line)? {
            if content_length.is_some() {
                anyhow::bail!("duplicate Content-Length header");
            }
            content_length = Some(len);
        }
    }

    let len = content_length.ok_or_else(|| anyhow::anyhow!("missing Content-Length header"))?;
    if len > max_content_length {
        anyhow::bail!("MCP frame too large: Content-Length {len} exceeds {max_content_length}");
    }
    let mut buf = vec![0; len];
    reader.read_exact(&mut buf).await?;
    Ok(Some(String::from_utf8(buf)?))
}

async fn read_limited_line<R>(reader: &mut R, max_len: usize) -> Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if bytes.len().saturating_add(take) > max_len {
            anyhow::bail!("MCP line too long");
        }

        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            break;
        }
    }

    Ok(Some(String::from_utf8(bytes)?))
}

fn parse_content_length_header(line: &str) -> Result<Option<usize>> {
    let (name, value) = line
        .trim_end_matches(['\r', '\n'])
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid MCP header line: {line}"))?;
    if !name.eq_ignore_ascii_case("Content-Length") {
        return Ok(None);
    }
    let len: usize = value
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid Content-Length header: {}", value.trim()))?;
    if len == 0 {
        anyhow::bail!("invalid Content-Length header: zero-length frame");
    }
    Ok(Some(len))
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
            &PolicyConfig::default(),
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
            &PolicyConfig::default(),
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
            &PolicyConfig::default(),
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
            &PolicyConfig::default(),
        )
        .await;
        assert!(response.is_none());
    }

    #[tokio::test]
    async fn stdio_reader_accepts_content_length_frame() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let frame = encode_stdio_frame(body);
        let mut reader = BufReader::new(frame.as_bytes());

        let message = read_stdio_message(&mut reader)
            .await
            .expect("read frame")
            .expect("message");

        assert_eq!(message, body);
    }

    #[tokio::test]
    async fn stdio_reader_accepts_legacy_line_json() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let bytes = format!("{body}\n").into_bytes();
        let mut reader = BufReader::new(bytes.as_slice());

        let message = read_stdio_message(&mut reader)
            .await
            .expect("read line")
            .expect("message");

        assert_eq!(message, body);
    }

    #[tokio::test]
    async fn stdio_reader_rejects_oversized_first_header_line() {
        let body = "{}";
        let frame = encode_stdio_frame(body);
        let mut reader = BufReader::new(frame.as_bytes());

        let err = read_stdio_message_with_limits(&mut reader, MAX_MCP_CONTENT_LENGTH, 8)
            .await
            .expect_err("first header line should be capped");

        assert!(err.to_string().contains("MCP header line too long"));
    }

    #[tokio::test]
    async fn stdio_reader_rejects_oversized_legacy_line_json() {
        let bytes = b"{\"jsonrpc\":\"2.0\"}\n";
        let mut reader = BufReader::new(bytes.as_slice());

        let err = read_stdio_message_with_limits(&mut reader, 8, MAX_MCP_HEADER_LINE_LENGTH)
            .await
            .expect_err("legacy JSON line should be capped");

        assert!(err.to_string().contains("MCP line too long"));
    }
}

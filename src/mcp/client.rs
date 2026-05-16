//! MCP client — connects to an MCP server over stdio, HTTP, or SSE.
use std::{collections::HashMap, process::Stdio, time::Duration};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time;

use super::protocol::*;

pub const DEFAULT_MCP_TIMEOUT_MS: u64 = 30_000;
pub const MAX_MCP_CONTENT_LENGTH: usize = 8 * 1024 * 1024;
const MAX_MCP_HEADER_LINE_LENGTH: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    #[default]
    Stdio,
    Http,
    Sse,
}

/// An active MCP connection.
pub enum McpClient {
    Stdio(StdioMcpClient),
    Http(RemoteMcpClient),
    Sse(RemoteMcpClient),
}

/// An active MCP connection over stdio.
pub struct StdioMcpClient {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
    server_info: Option<Implementation>,
    timeout: Duration,
    max_content_length: usize,
}

/// Configuration for launching an MCP server.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

impl McpClient {
    /// Connect to an MCP server and perform any transport-specific setup.
    pub async fn connect(config: &McpServerConfig) -> Result<Self, anyhow::Error> {
        Self::connect_with_timeout(config, DEFAULT_MCP_TIMEOUT_MS).await
    }

    /// Connect to an MCP server with a per-server request/read timeout.
    pub async fn connect_with_timeout(
        config: &McpServerConfig,
        timeout_ms: u64,
    ) -> Result<Self, anyhow::Error> {
        match config.transport {
            McpTransport::Stdio => Ok(Self::Stdio(
                StdioMcpClient::connect_with_timeout(config, timeout_ms).await?,
            )),
            McpTransport::Http => Ok(Self::Http(
                RemoteMcpClient::connect(config, timeout_ms, McpTransport::Http).await?,
            )),
            McpTransport::Sse => Ok(Self::Sse(
                RemoteMcpClient::connect(config, timeout_ms, McpTransport::Sse).await?,
            )),
        }
    }

    /// List tools available from the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, anyhow::Error> {
        match self {
            Self::Stdio(client) => client.list_tools().await,
            Self::Http(client) | Self::Sse(client) => client.list_tools().await,
        }
    }

    /// Call a tool on the server.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, anyhow::Error> {
        match self {
            Self::Stdio(client) => client.call_tool(name, arguments).await,
            Self::Http(client) | Self::Sse(client) => client.call_tool(name, arguments).await,
        }
    }

    /// List resources available from the server.
    pub async fn list_resources(&mut self) -> Result<Vec<McpResource>, anyhow::Error> {
        match self {
            Self::Stdio(client) => client.list_resources().await,
            Self::Http(client) | Self::Sse(client) => client.list_resources().await,
        }
    }

    /// Read a resource from the server.
    pub async fn read_resource(&mut self, uri: &str) -> Result<ReadResourceResult, anyhow::Error> {
        match self {
            Self::Stdio(client) => client.read_resource(uri).await,
            Self::Http(client) | Self::Sse(client) => client.read_resource(uri).await,
        }
    }

    /// Gracefully shut down the connection.
    pub async fn shutdown(self) -> Result<(), anyhow::Error> {
        match self {
            Self::Stdio(client) => client.shutdown().await,
            Self::Http(client) | Self::Sse(client) => client.shutdown().await,
        }
    }
}

impl StdioMcpClient {
    /// Launch an MCP server with a per-server request/read timeout.
    async fn connect_with_timeout(
        config: &McpServerConfig,
        timeout_ms: u64,
    ) -> Result<Self, anyhow::Error> {
        let command = config.command.as_deref().ok_or_else(|| {
            anyhow::anyhow!("stdio MCP server '{}' requires command", config.name)
        })?;
        let mut cmd = Command::new(command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        if let Some(env_map) = &config.env {
            for (k, v) in env_map {
                cmd.env(k, v);
            }
        }

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("stdin not available"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("stdout not available"))?;
        let reader = BufReader::new(stdout);

        let mut client = Self {
            child,
            stdin,
            reader,
            next_id: 1,
            server_info: None,
            timeout: Duration::from_millis(timeout_ms.max(1)),
            max_content_length: MAX_MCP_CONTENT_LENGTH,
        };

        // Initialize handshake
        let init_req = InitializeRequest {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "deepseek-code".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let result: InitializeResult = client.request("initialize", init_req).await?;
        if result.protocol_version != PROTOCOL_VERSION {
            anyhow::bail!(
                "protocol version mismatch: expected {}, got {}",
                PROTOCOL_VERSION,
                result.protocol_version
            );
        }
        client.server_info = Some(result.server_info);

        // Send initialized notification
        client
            .notify("notifications/initialized", serde_json::json!({}))
            .await?;

        Ok(client)
    }

    /// List tools available from the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, anyhow::Error> {
        let result: ListToolsResult = self
            .request("tools/list", ListToolsRequest { cursor: None })
            .await?;
        Ok(result.tools)
    }

    /// Call a tool on the server.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, anyhow::Error> {
        self.request(
            "tools/call",
            CallToolRequest {
                name: name.to_string(),
                arguments,
            },
        )
        .await
    }

    /// List resources available from the server.
    pub async fn list_resources(&mut self) -> Result<Vec<McpResource>, anyhow::Error> {
        let result: ListResourcesResult = self
            .request("resources/list", ListResourcesRequest { cursor: None })
            .await?;
        Ok(result.resources)
    }

    /// Read a resource from the server.
    pub async fn read_resource(&mut self, uri: &str) -> Result<ReadResourceResult, anyhow::Error> {
        self.request(
            "resources/read",
            ReadResourceRequest {
                uri: uri.to_string(),
            },
        )
        .await
    }

    /// Gracefully shut down the connection.
    pub async fn shutdown(mut self) -> Result<(), anyhow::Error> {
        let _ = self
            .request::<_, serde_json::Value>("shutdown", serde_json::json!({}))
            .await;
        self.notify("exit", serde_json::json!({})).await.ok();
        let _ = self.child.wait().await;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Low-level JSON-RPC
    // -----------------------------------------------------------------------

    async fn request<T: serde::Serialize, R: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: T,
    ) -> Result<R, anyhow::Error> {
        let id = self.next_id;
        self.next_id += 1;

        let body = make_request(id, method, params);
        self.send_raw_with_timeout(method, &body).await?;

        let response_text = self.read_message().await?;
        let response: JsonRpcResponse<R> = serde_json::from_str(&response_text)
            .map_err(|e| anyhow::anyhow!("failed to parse response: {e}\nraw: {response_text}"))?;

        match response.result {
            JsonRpcResult::Result(r) => Ok(r),
            JsonRpcResult::Error(err) => Err(anyhow::anyhow!("{}: {}", err.code, err.message)),
        }
    }

    async fn notify<T: serde::Serialize>(
        &mut self,
        method: &str,
        params: T,
    ) -> Result<(), anyhow::Error> {
        let body = make_notification(method, params);
        self.send_raw_with_timeout(method, &body).await
    }

    async fn send_raw_with_timeout(
        &mut self,
        method: &str,
        body: &str,
    ) -> Result<(), anyhow::Error> {
        time::timeout(self.timeout, self.send_raw(body))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP request '{method}' timed out while sending after {} ms",
                    self.timeout.as_millis()
                )
            })?
    }

    async fn send_raw(&mut self, body: &str) -> Result<(), anyhow::Error> {
        let header = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.stdin.write_all(header.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<String, anyhow::Error> {
        read_mcp_message_with_timeout(&mut self.reader, self.timeout, self.max_content_length).await
    }
}

/// An active MCP connection over HTTP-style transports.
pub struct RemoteMcpClient {
    http: reqwest::Client,
    url: String,
    headers: HeaderMap,
    next_id: i64,
    server_info: Option<Implementation>,
    timeout: Duration,
    transport: McpTransport,
}

impl RemoteMcpClient {
    async fn connect(
        config: &McpServerConfig,
        timeout_ms: u64,
        transport: McpTransport,
    ) -> Result<Self, anyhow::Error> {
        let url = config.url.as_deref().ok_or_else(|| {
            anyhow::anyhow!("{transport:?} MCP server '{}' requires url", config.name)
        })?;
        let timeout = Duration::from_millis(timeout_ms.max(1));
        let mut client = Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .map_err(|e| anyhow::anyhow!("failed to build MCP HTTP client: {e}"))?,
            url: url.trim_end_matches('/').to_string(),
            headers: build_header_map(config.headers.as_ref())?,
            next_id: 1,
            server_info: None,
            timeout,
            transport,
        };

        let init_req = InitializeRequest {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "deepseek-code".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };
        let result: InitializeResult = client.request("initialize", init_req).await?;
        if result.protocol_version != PROTOCOL_VERSION {
            anyhow::bail!(
                "protocol version mismatch: expected {}, got {}",
                PROTOCOL_VERSION,
                result.protocol_version
            );
        }
        client.server_info = Some(result.server_info);
        client
            .notify("notifications/initialized", serde_json::json!({}))
            .await?;

        Ok(client)
    }

    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, anyhow::Error> {
        let result: ListToolsResult = self
            .request("tools/list", ListToolsRequest { cursor: None })
            .await?;
        Ok(result.tools)
    }

    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, anyhow::Error> {
        self.request(
            "tools/call",
            CallToolRequest {
                name: name.to_string(),
                arguments,
            },
        )
        .await
    }

    pub async fn list_resources(&mut self) -> Result<Vec<McpResource>, anyhow::Error> {
        let result: ListResourcesResult = self
            .request("resources/list", ListResourcesRequest { cursor: None })
            .await?;
        Ok(result.resources)
    }

    pub async fn read_resource(&mut self, uri: &str) -> Result<ReadResourceResult, anyhow::Error> {
        self.request(
            "resources/read",
            ReadResourceRequest {
                uri: uri.to_string(),
            },
        )
        .await
    }

    pub async fn shutdown(mut self) -> Result<(), anyhow::Error> {
        let _ = self
            .request::<_, serde_json::Value>("shutdown", serde_json::json!({}))
            .await;
        self.notify("exit", serde_json::json!({})).await.ok();
        Ok(())
    }

    async fn request<T: serde::Serialize, R: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: T,
    ) -> Result<R, anyhow::Error> {
        let id = self.next_id;
        self.next_id += 1;

        let body = make_request(id, method, params);
        let response_text = self.send_json_rpc(method, body, true).await?;
        let response: JsonRpcResponse<R> = serde_json::from_str(&response_text)
            .map_err(|e| anyhow::anyhow!("failed to parse response: {e}\nraw: {response_text}"))?;

        match response.result {
            JsonRpcResult::Result(r) => Ok(r),
            JsonRpcResult::Error(err) => Err(anyhow::anyhow!("{}: {}", err.code, err.message)),
        }
    }

    async fn notify<T: serde::Serialize>(
        &mut self,
        method: &str,
        params: T,
    ) -> Result<(), anyhow::Error> {
        let body = make_notification(method, params);
        self.send_json_rpc(method, body, false).await.map(|_| ())
    }

    async fn send_json_rpc(
        &self,
        method: &str,
        body: String,
        expect_response: bool,
    ) -> Result<String, anyhow::Error> {
        let mut request = self
            .http
            .post(&self.url)
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json");
        request = match self.transport {
            McpTransport::Sse => request.header(ACCEPT, "text/event-stream"),
            McpTransport::Http | McpTransport::Stdio => {
                request.header(ACCEPT, "application/json, text/event-stream")
            }
        };

        let response = time::timeout(self.timeout, request.body(body).send())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP request '{method}' timed out after {} ms",
                    self.timeout.as_millis()
                )
            })?
            .map_err(|e| anyhow::anyhow!("MCP request '{method}' failed: {e}"))?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let text = time::timeout(self.timeout, response.text())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP response for '{method}' timed out after {} ms",
                    self.timeout.as_millis()
                )
            })?
            .map_err(|e| anyhow::anyhow!("failed to read MCP response for '{method}': {e}"))?;

        if !status.is_success() {
            anyhow::bail!("MCP request '{method}' failed with HTTP {status}: {text}");
        }
        if text.len() > MAX_MCP_CONTENT_LENGTH {
            anyhow::bail!(
                "MCP response for '{method}' too large: {} bytes exceeds {}",
                text.len(),
                MAX_MCP_CONTENT_LENGTH
            );
        }
        if !expect_response {
            return Ok(String::new());
        }
        if content_type.starts_with("text/event-stream") || looks_like_sse(&text) {
            extract_sse_json(&text)
        } else {
            Ok(text)
        }
    }
}

fn build_header_map(headers: Option<&HashMap<String, String>>) -> Result<HeaderMap, anyhow::Error> {
    let mut map = HeaderMap::new();
    let Some(headers) = headers else {
        return Ok(map);
    };
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| anyhow::anyhow!("invalid MCP header name '{name}': {e}"))?;
        let value = HeaderValue::from_str(value)
            .map_err(|e| anyhow::anyhow!("invalid MCP header value for '{name}': {e}"))?;
        map.insert(name, value);
    }
    Ok(map)
}

fn looks_like_sse(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim_start().starts_with("data:"))
}

fn extract_sse_json(text: &str) -> Result<String, anyhow::Error> {
    let mut event_data = String::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            if let Some(json) = finish_sse_event(&event_data) {
                return Ok(json);
            }
            event_data.clear();
            continue;
        }
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim_start();
        if data == "[DONE]" {
            event_data.clear();
            continue;
        }
        if !event_data.is_empty() {
            event_data.push('\n');
        }
        event_data.push_str(data);
    }

    if let Some(json) = finish_sse_event(&event_data) {
        return Ok(json);
    }
    anyhow::bail!("SSE MCP response did not contain a JSON data frame")
}

fn finish_sse_event(event_data: &str) -> Option<String> {
    if event_data.trim().is_empty() {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(event_data)
        .ok()
        .map(|_| event_data.to_string())
}

async fn read_mcp_message_with_timeout<R>(
    reader: &mut R,
    timeout: Duration,
    max_content_length: usize,
) -> Result<String, anyhow::Error>
where
    R: AsyncBufRead + Unpin,
{
    time::timeout(timeout, read_mcp_message(reader, max_content_length))
        .await
        .map_err(|_| anyhow::anyhow!("MCP read timed out after {} ms", timeout.as_millis()))?
}

async fn read_mcp_message<R>(
    reader: &mut R,
    max_content_length: usize,
) -> Result<String, anyhow::Error>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            anyhow::bail!("EOF while reading header");
        }

        if line.len() > MAX_MCP_HEADER_LINE_LENGTH {
            anyhow::bail!("MCP header line too long");
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }

        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid MCP header line: {line}"))?;
        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                anyhow::bail!("duplicate Content-Length header");
            }
            let value = value.trim();
            let len: usize = value
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid Content-Length header: {value}"))?;
            if len == 0 {
                anyhow::bail!("invalid Content-Length header: zero-length frame");
            }
            if len > max_content_length {
                anyhow::bail!(
                    "MCP frame too large: Content-Length {len} exceeds {max_content_length}"
                );
            }
            content_length = Some(len);
        }
    }

    let len = content_length.ok_or_else(|| anyhow::anyhow!("missing Content-Length header"))?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    String::from_utf8(buf).map_err(|e| anyhow::anyhow!("invalid UTF-8 in MCP frame body: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader as TokioBufReader;
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_mcp_server_config_clone() {
        let cfg = McpServerConfig {
            name: "test".into(),
            transport: McpTransport::Stdio,
            command: Some("echo".into()),
            args: vec!["hello".into()],
            env: None,
            url: None,
            headers: None,
        };
        let _ = cfg.clone();
    }

    #[tokio::test]
    async fn read_message_accepts_valid_frame() {
        let mut reader = TokioBufReader::new(&b"Content-Length: 15\r\n\r\n{\"jsonrpc\":\"2\"}"[..]);
        let message = read_mcp_message(&mut reader, MAX_MCP_CONTENT_LENGTH)
            .await
            .expect("valid MCP frame should parse");
        assert_eq!(message, "{\"jsonrpc\":\"2\"}");
    }

    #[tokio::test]
    async fn read_message_rejects_invalid_content_length() {
        let mut reader = TokioBufReader::new(&b"Content-Length: nope\r\n\r\n{}"[..]);
        let err = read_mcp_message(&mut reader, MAX_MCP_CONTENT_LENGTH)
            .await
            .expect_err("invalid Content-Length should fail");
        assert!(err.to_string().contains("invalid Content-Length"));
    }

    #[tokio::test]
    async fn read_message_rejects_oversized_frame() {
        let mut reader = TokioBufReader::new(&b"Content-Length: 9\r\n\r\n{}"[..]);
        let err = read_mcp_message(&mut reader, 8)
            .await
            .expect_err("oversized frame should fail");
        assert!(err.to_string().contains("MCP frame too large"));
    }

    #[tokio::test]
    async fn read_message_times_out() {
        let (read_half, _write_half) = tokio::io::duplex(64);
        let mut reader = TokioBufReader::new(read_half);
        let err = read_mcp_message_with_timeout(
            &mut reader,
            Duration::from_millis(5),
            MAX_MCP_CONTENT_LENGTH,
        )
        .await
        .expect_err("empty stream should time out");
        assert!(err.to_string().contains("MCP read timed out"));
    }

    #[tokio::test]
    async fn http_client_initializes_and_lists_tools() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"method": "initialize"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocol_version": PROTOCOL_VERSION,
                    "capabilities": {},
                    "server_info": {"name": "remote", "version": "1.0"}
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "method": "notifications/initialized"
            })))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"method": "tools/list"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "tools": [
                        {"name": "read", "description": "Read", "input_schema": {}}
                    ]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = McpServerConfig {
            name: "remote".into(),
            transport: McpTransport::Http,
            command: None,
            args: Vec::new(),
            env: None,
            url: Some(server.uri()),
            headers: None,
        };
        let mut client = McpClient::connect_with_timeout(&config, 1_000)
            .await
            .expect("HTTP MCP client connects");

        let tools = client.list_tools().await.expect("tools list");

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read");
    }

    #[test]
    fn sse_json_extraction_reads_data_frame() {
        let frame =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";

        let json = extract_sse_json(frame).expect("SSE data frame parses");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).expect("json"),
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})
        );
    }
}

//! MCP stdio client — connects to an MCP server over stdin/stdout.
use std::{process::Stdio, time::Duration};

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time;

use super::protocol::*;

pub const DEFAULT_MCP_TIMEOUT_MS: u64 = 30_000;
pub const MAX_MCP_CONTENT_LENGTH: usize = 8 * 1024 * 1024;
const MAX_MCP_HEADER_LINE_LENGTH: usize = 8 * 1024;

/// An active MCP connection over stdio.
pub struct McpClient {
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
    pub command: String,
    pub args: Vec<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

impl McpClient {
    /// Launch an MCP server and perform the initialize handshake.
    pub async fn connect(config: &McpServerConfig) -> Result<Self, anyhow::Error> {
        Self::connect_with_timeout(config, DEFAULT_MCP_TIMEOUT_MS).await
    }

    /// Launch an MCP server with a per-server request/read timeout.
    pub async fn connect_with_timeout(
        config: &McpServerConfig,
        timeout_ms: u64,
    ) -> Result<Self, anyhow::Error> {
        let mut cmd = Command::new(&config.command);
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

    #[test]
    fn test_mcp_server_config_clone() {
        let cfg = McpServerConfig {
            name: "test".into(),
            command: "echo".into(),
            args: vec!["hello".into()],
            env: None,
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
}

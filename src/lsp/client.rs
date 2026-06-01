use anyhow::Context;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

const MAX_LSP_CONTENT_LENGTH: usize = 8 * 1024 * 1024;
const MAX_LSP_HEADER_LINE_LENGTH: usize = 8 * 1024;

pub struct LspClient {
    child: Child,
    stdout: BufReader<ChildStdout>,
    stdin: ChildStdin,
    next_id: i64,
}

/// Severity of a diagnostic, mirroring the LSP `DiagnosticSeverity` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

/// One diagnostic for a file (1-based line/character for display).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: u32,
    pub character: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

impl Diagnostic {
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == DiagnosticSeverity::Error
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Information => "info",
            DiagnosticSeverity::Hint => "hint",
        };
        write!(
            f,
            "{}:{} {label}: {}",
            self.line, self.character, self.message
        )
    }
}

async fn read_message<R>(reader: &mut BufReader<R>) -> Result<serde_json::Value, anyhow::Error>
where
    R: AsyncRead + Unpin,
{
    let mut content_length: Option<usize> = None;

    let mut header_buf = String::new();
    loop {
        header_buf.clear();
        let n = reader
            .read_line(&mut header_buf)
            .await
            .context("reading header line")?;
        if n == 0 {
            return Err(anyhow::anyhow!("EOF while reading LSP message headers"));
        }
        if header_buf.len() > MAX_LSP_HEADER_LINE_LENGTH {
            anyhow::bail!("LSP header line too long");
        }
        if header_buf == "\r\n" || header_buf == "\n" {
            break;
        }
        let header = header_buf.trim_end();
        if let Some((key, value)) = header.split_once(':') {
            if key.eq_ignore_ascii_case("Content-Length") {
                if content_length.is_some() {
                    anyhow::bail!("duplicate Content-Length header");
                }
                let len = value.trim().parse().context("parsing Content-Length")?;
                if len == 0 {
                    anyhow::bail!("invalid Content-Length header: zero-length frame");
                }
                if len > MAX_LSP_CONTENT_LENGTH {
                    anyhow::bail!(
                        "LSP frame too large: Content-Length {len} exceeds {MAX_LSP_CONTENT_LENGTH}"
                    );
                }
                content_length = Some(len);
            }
        }
    }

    let len = content_length.ok_or_else(|| anyhow::anyhow!("Missing Content-Length header"))?;
    let mut body_buf = vec![0u8; len];
    reader
        .read_exact(&mut body_buf)
        .await
        .context("reading message body")?;
    let msg = serde_json::from_slice(&body_buf).context("parsing JSON body")?;
    Ok(msg)
}

fn path_to_uri(path: &str) -> String {
    if path.starts_with("file://") {
        path.to_string()
    } else {
        let normalized = path.replace('\\', "/");
        format!("file:///{}", normalized)
    }
}

/// LSP language id for a file path — used both as the config key for picking a
/// server and as the `languageId` sent in `didOpen`. `None` for extensions we
/// don't map.
#[must_use]
pub fn language_id_for_path(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_str()?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        _ => return None,
    })
}

impl LspClient {
    pub async fn start(
        command: &str,
        args: &[&str],
        root_uri: &str,
    ) -> Result<Self, anyhow::Error> {
        let mut child = tokio::process::Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawning LSP server: {}", command))?;

        let stdin = child.stdin.take().context("stdin not available")?;
        let stdout = child.stdout.take().context("stdout not available")?;
        let stdout = BufReader::new(stdout);

        let mut client = Self {
            child,
            stdout,
            stdin,
            next_id: 1,
        };

        client.initialize(root_uri).await?;
        Ok(client)
    }

    pub async fn initialize(&mut self, root_uri: &str) -> Result<serde_json::Value, anyhow::Error> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": path_to_uri(root_uri),
            "capabilities": {},
        });
        self.send_request("initialize", params).await
    }

    pub async fn hover(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<String>, anyhow::Error> {
        let params = json!({
            "textDocument": { "uri": path_to_uri(file_path) },
            "position": { "line": line, "character": character },
        });
        let resp = self.send_request("textDocument/hover", params).await?;

        if let Some(result) = resp.get("result") {
            if result.is_null() {
                return Ok(None);
            }
            Ok(extract_hover_contents(result))
        } else {
            Ok(None)
        }
    }

    pub async fn definition(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<String>, anyhow::Error> {
        let params = json!({
            "textDocument": { "uri": path_to_uri(file_path) },
            "position": { "line": line, "character": character },
        });
        let resp = self.send_request("textDocument/definition", params).await?;

        if let Some(result) = resp.get("result") {
            if result.is_null() {
                return Ok(Vec::new());
            }
            Ok(extract_locations(result))
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.send_request("shutdown", serde_json::Value::Null)
            .await?;

        let exit = json!({ "jsonrpc": "2.0", "method": "exit" });
        let body = serde_json::to_string(&exit)?;
        let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.stdin.write_all(msg.as_bytes()).await?;
        self.stdin.flush().await?;

        let _ = self.child.wait().await;
        Ok(())
    }

    /// Tell the server a document is open so it computes diagnostics for it.
    pub async fn did_open(
        &mut self,
        file_path: &str,
        language_id: &str,
        text: &str,
    ) -> Result<(), anyhow::Error> {
        let params = json!({
            "textDocument": {
                "uri": path_to_uri(file_path),
                "languageId": language_id,
                "version": 1,
                "text": text,
            }
        });
        self.send_notification("textDocument/didOpen", params).await
    }

    /// Collect `publishDiagnostics` for `file_path` until `timeout` elapses.
    /// Each notification carries the file's full diagnostic set, so the latest
    /// one for the URI wins. Best-effort: any read error or the timeout returns
    /// whatever was gathered and never fails the caller (fail-safe).
    pub async fn collect_diagnostics(
        &mut self,
        file_path: &str,
        timeout: std::time::Duration,
    ) -> Vec<Diagnostic> {
        let uri = path_to_uri(file_path);
        let start = std::time::Instant::now();
        let mut diagnostics = Vec::new();
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                break;
            }
            match tokio::time::timeout(timeout - elapsed, read_message(&mut self.stdout)).await {
                Ok(Ok(message)) => {
                    if is_publish_diagnostics_for(&message, &uri) {
                        diagnostics = parse_publish_diagnostics(&message, &uri);
                    }
                }
                // Read error or deadline reached: stop and return what we have.
                Ok(Err(_)) | Err(_) => break,
            }
        }
        diagnostics
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        let note = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let body = serde_json::to_string(&note)?;
        let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.stdin
            .write_all(msg.as_bytes())
            .await
            .context("writing LSP notification")?;
        self.stdin
            .flush()
            .await
            .context("flushing LSP notification")?;
        Ok(())
    }

    async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let id = self.next_id;
        self.next_id += 1;

        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let body = serde_json::to_string(&req)?;
        let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.stdin
            .write_all(msg.as_bytes())
            .await
            .context("writing LSP request")?;
        self.stdin.flush().await.context("flushing LSP request")?;

        // LSP servers interleave notifications (diagnostics, $/progress,
        // window/logMessage) with responses. Skip anything that isn't the
        // reply to our request id, or we'd return a notification as the result.
        const MAX_SKIPPED_MESSAGES: usize = 256;
        for _ in 0..=MAX_SKIPPED_MESSAGES {
            let message = read_message(&mut self.stdout).await?;
            if is_response_to(&message, id) {
                return Ok(message);
            }
        }
        anyhow::bail!("LSP server sent too many messages without a reply to request {id}")
    }
}

/// True if a JSON-RPC message is the response to the request with `id`.
/// Notifications (no `id`) and replies to other requests return false.
fn is_response_to(message: &serde_json::Value, id: i64) -> bool {
    message.get("id").and_then(serde_json::Value::as_i64) == Some(id)
}

fn extract_hover_contents(value: &serde_json::Value) -> Option<String> {
    let contents = value.get("contents")?;
    match contents {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(o) => o
            .get("value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        serde_json::Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                match item {
                    serde_json::Value::String(s) => parts.push(s.clone()),
                    serde_json::Value::Object(o) => {
                        if let Some(s) = o.get("value").and_then(|v| v.as_str()) {
                            parts.push(s.to_string());
                        }
                    }
                    _ => {}
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

fn extract_locations(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(arr) => arr.iter().filter_map(location_to_string).collect(),
        serde_json::Value::Object(_) => location_to_string(value).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn location_to_string(value: &serde_json::Value) -> Option<String> {
    let uri = value.get("uri")?.as_str()?;
    let range = value.get("range")?;
    let start = range.get("start")?;
    let line = start.get("line")?.as_u64()? as u32;
    let character = start.get("character")?.as_u64()? as u32;
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    Some(format!("{}:{}:{}", path, line + 1, character + 1))
}

/// True if `message` is a `textDocument/publishDiagnostics` notification for `uri`.
fn is_publish_diagnostics_for(message: &serde_json::Value, uri: &str) -> bool {
    message.get("method").and_then(serde_json::Value::as_str)
        == Some("textDocument/publishDiagnostics")
        && message
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(serde_json::Value::as_str)
            == Some(uri)
}

/// Parse the diagnostics from a `publishDiagnostics` notification, but only
/// when it targets `uri`. Returns 1-based line/character for display.
fn parse_publish_diagnostics(message: &serde_json::Value, uri: &str) -> Vec<Diagnostic> {
    if !is_publish_diagnostics_for(message, uri) {
        return Vec::new();
    }
    let Some(items) = message
        .get("params")
        .and_then(|p| p.get("diagnostics"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    items.iter().filter_map(parse_one_diagnostic).collect()
}

fn parse_one_diagnostic(value: &serde_json::Value) -> Option<Diagnostic> {
    let start = value.get("range")?.get("start")?;
    let line = start.get("line")?.as_u64()? as u32;
    let character = start.get("character")?.as_u64()? as u32;
    let message = value.get("message")?.as_str()?.to_string();
    let severity = match value.get("severity").and_then(serde_json::Value::as_u64) {
        Some(2) => DiagnosticSeverity::Warning,
        Some(3) => DiagnosticSeverity::Information,
        Some(4) => DiagnosticSeverity::Hint,
        // 1 (Error) or unspecified: surface it as an error so real problems
        // (e.g. a hallucinated API) aren't silently downgraded.
        _ => DiagnosticSeverity::Error,
    };
    Some(Diagnostic {
        line: line + 1,
        character: character + 1,
        severity,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_matching_skips_notifications_and_other_ids() {
        assert!(is_response_to(
            &json!({"jsonrpc":"2.0","id":4,"result":{}}),
            4
        ));
        // Diagnostics/progress notifications have no id and must be skipped.
        assert!(!is_response_to(
            &json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{}}),
            4
        ));
        // A reply to a different request must be skipped.
        assert!(!is_response_to(
            &json!({"jsonrpc":"2.0","id":3,"result":{}}),
            4
        ));
    }

    #[test]
    fn parses_publish_diagnostics_for_matching_uri_only() {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///src/main.rs",
                "diagnostics": [
                    {
                        "range": {"start": {"line": 11, "character": 4}, "end": {"line": 11, "character": 9}},
                        "severity": 1,
                        "message": "cannot find function `frobnicate`"
                    },
                    {
                        "range": {"start": {"line": 3, "character": 0}},
                        "message": "no severity given"
                    }
                ]
            }
        });

        let diags = parse_publish_diagnostics(&msg, "file:///src/main.rs");
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].line, 12); // 1-based for display
        assert_eq!(diags[0].character, 5);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert!(diags[0].is_error());
        assert!(diags[0].message.contains("cannot find function"));
        // Missing severity is surfaced as an error, not silently dropped.
        assert_eq!(diags[1].severity, DiagnosticSeverity::Error);

        // A notification for another file, or a non-diagnostics message, is empty.
        assert!(parse_publish_diagnostics(&msg, "file:///other.rs").is_empty());
        let reply = json!({"jsonrpc":"2.0","id":4,"result":{}});
        assert!(parse_publish_diagnostics(&reply, "file:///src/main.rs").is_empty());
    }

    #[test]
    fn language_id_maps_known_extensions() {
        assert_eq!(language_id_for_path("src/main.rs"), Some("rust"));
        assert_eq!(language_id_for_path("/abs/app.py"), Some("python"));
        assert_eq!(language_id_for_path("a/b/c.tsx"), Some("typescriptreact"));
        assert_eq!(language_id_for_path("x.go"), Some("go"));
        assert_eq!(language_id_for_path("README.md"), None);
        assert_eq!(language_id_for_path("Makefile"), None);
    }

    #[tokio::test]
    async fn test_lsp_message_parsing() {
        let data = b"Content-Length: 36\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}";
        let mut reader = BufReader::new(&data[..]);
        let msg = read_message(&mut reader).await.unwrap();
        assert_eq!(msg["jsonrpc"], "2.0");
        assert_eq!(msg["id"], 1);
        assert_eq!(msg["result"], json!({}));
    }

    #[tokio::test]
    async fn test_lsp_message_parsing_extra_headers() {
        let data = b"Content-Length: 24\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":2}";
        let mut reader = BufReader::new(&data[..]);
        let msg = read_message(&mut reader).await.unwrap();
        assert_eq!(msg["jsonrpc"], "2.0");
        assert_eq!(msg["id"], 2);
    }

    #[tokio::test]
    async fn read_message_rejects_oversized_frame() {
        let data = format!("Content-Length: {}\r\n\r\n{{}}", MAX_LSP_CONTENT_LENGTH + 1);
        let mut reader = BufReader::new(data.as_bytes());
        let error = read_message(&mut reader)
            .await
            .expect_err("oversized LSP frame should fail before allocation");
        assert!(error.to_string().contains("LSP frame too large"));
    }

    #[tokio::test]
    async fn read_message_accepts_content_length_without_space() {
        let data = b"Content-Length:24\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":3}";
        let mut reader = BufReader::new(&data[..]);
        let msg = read_message(&mut reader)
            .await
            .expect("compact LSP Content-Length header should parse");
        assert_eq!(msg["id"], 3);
    }

    #[tokio::test]
    async fn test_extract_hover_string() {
        let value = json!({ "contents": "hello world" });
        assert_eq!(
            extract_hover_contents(&value),
            Some("hello world".to_string())
        );
    }

    #[tokio::test]
    async fn test_extract_hover_markdown() {
        let value = json!({ "contents": { "kind": "markdown", "value": "# Title" } });
        assert_eq!(extract_hover_contents(&value), Some("# Title".to_string()));
    }

    #[tokio::test]
    async fn test_extract_locations_single() {
        let value = json!({
            "uri": "file:///src/main.rs",
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }
        });
        assert_eq!(extract_locations(&value), vec!["/src/main.rs:1:1"]);
    }

    #[tokio::test]
    async fn test_extract_locations_array() {
        let value = json!([
            { "uri": "file:///a.rs", "range": { "start": { "line": 1, "character": 2 } } },
            { "uri": "file:///b.rs", "range": { "start": { "line": 3, "character": 4 } } }
        ]);
        assert_eq!(extract_locations(&value), vec!["/a.rs:2:3", "/b.rs:4:5"]);
    }
}

use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use tokio_stream::Stream;

use super::errors::DeepSeekError;
use super::models::{
    ChatRequest, ChatResponse, StreamChunk, StreamResult, ThinkingConfig, ThinkingWireFormat,
};
use super::stream::{parse_stream, StreamAccumulator, StreamEvent};

const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const MAX_RETRIES: u32 = 3;
const MAX_DEEPSEEK_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_DEEPSEEK_ERROR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    http: HttpClient,
    api_key: String,
    base_url: String,
    pro_model_name: Option<String>,
    flash_model_name: Option<String>,
    thinking_wire_format: ThinkingWireFormat,
}

impl DeepSeekClient {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            http: HttpClient::builder()
                .timeout(Duration::from_mins(2))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
            api_key,
            base_url: DEEPSEEK_BASE_URL.to_string(),
            pro_model_name: None,
            flash_model_name: None,
            thinking_wire_format: ThinkingWireFormat::DeepSeekNative,
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    #[must_use]
    pub fn with_model_names(
        mut self,
        pro_model: Option<String>,
        flash_model: Option<String>,
    ) -> Self {
        self.pro_model_name = pro_model;
        self.flash_model_name = flash_model;
        self
    }

    #[must_use]
    pub fn with_thinking_wire_format(mut self, format: ThinkingWireFormat) -> Self {
        self.thinking_wire_format = format;
        self
    }

    fn map_chat_request_model(&self, req: &ChatRequest) -> ChatRequest {
        let mut mapped = req.clone();
        mapped.model = self.provider_model_name(&req.model);
        mapped
    }

    fn map_fim_request_model(&self, req: &super::fim::FimRequest) -> super::fim::FimRequest {
        let mut mapped = req.clone();
        mapped.model = self.provider_model_name(&req.model);
        mapped
    }

    fn provider_model_name(&self, model: &str) -> String {
        match model {
            "deepseek-v4-pro" | "deepseek-reasoner" => self
                .pro_model_name
                .clone()
                .unwrap_or_else(|| model.to_string()),
            "deepseek-v4-flash" | "deepseek-chat" => self
                .flash_model_name
                .clone()
                .unwrap_or_else(|| model.to_string()),
            other => other.to_string(),
        }
    }

    fn chat_payload(&self, req: &ChatRequest) -> Result<serde_json::Value, DeepSeekError> {
        let req = self.map_chat_request_model(req);
        let mut payload = serde_json::to_value(&req)?;
        self.apply_thinking_wire_format(&mut payload);
        Ok(payload)
    }

    fn apply_thinking_wire_format(&self, payload: &mut serde_json::Value) {
        let Some(object) = payload.as_object_mut() else {
            return;
        };
        let thinking = object.get("thinking").cloned();
        match self.thinking_wire_format {
            ThinkingWireFormat::DeepSeekNative => {}
            ThinkingWireFormat::NativeTypeOnly => {
                if let Some(thinking) = object.get_mut("thinking").and_then(|v| v.as_object_mut()) {
                    thinking.remove("effort");
                }
            }
            ThinkingWireFormat::DashScopeEnableThinking => {
                object.remove("thinking");
                let parsed = thinking.as_ref().and_then(parse_thinking_config);
                let enabled = parsed.as_ref().is_some_and(|config| config.is_enabled());
                object.insert(
                    "enable_thinking".to_string(),
                    serde_json::Value::Bool(enabled),
                );
                if enabled {
                    if let Some(budget) = parsed
                        .and_then(|config| thinking_budget_for_effort(config.effort.as_deref()))
                    {
                        object.insert(
                            "thinking_budget".to_string(),
                            serde_json::Value::Number(budget.into()),
                        );
                    }
                }
            }
            ThinkingWireFormat::QianfanThinking => {
                object.remove("thinking");
                let parsed = thinking.as_ref().and_then(parse_thinking_config);
                let enabled = parsed.as_ref().is_some_and(|config| config.is_enabled());
                object.insert(
                    "thinking".to_string(),
                    serde_json::json!({
                        "type": if enabled { "enabled" } else { "disabled" },
                    }),
                );
                object.insert(
                    "enable_thinking".to_string(),
                    serde_json::Value::Bool(enabled),
                );
                if enabled {
                    if let Some(budget) = parsed
                        .and_then(|config| thinking_budget_for_effort(config.effort.as_deref()))
                    {
                        object.insert(
                            "thinking_budget".to_string(),
                            serde_json::Value::Number(budget.into()),
                        );
                    }
                }
            }
            ThinkingWireFormat::MiniMaxReasoningSplit => {
                object.remove("thinking");
                let parsed = thinking.as_ref().and_then(parse_thinking_config);
                let enabled = parsed.as_ref().is_some_and(|config| config.is_enabled());
                object.insert(
                    "reasoning_split".to_string(),
                    serde_json::Value::Bool(enabled),
                );
            }
            ThinkingWireFormat::Unsupported => {
                object.remove("thinking");
            }
        }
    }

    async fn require_success(
        response: reqwest::Response,
    ) -> Result<reqwest::Response, DeepSeekError> {
        let status = response.status();
        if !status.is_success() {
            let body = response_text_capped(response, MAX_DEEPSEEK_ERROR_BYTES)
                .await
                .unwrap_or_else(|error| format!("[response body unavailable: {error}]"));
            return Err(DeepSeekError::from_status(status.as_u16(), &body));
        }
        Ok(response)
    }

    /// Non-streaming chat completion.
    pub async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, DeepSeekError> {
        let url = format!("{}/chat/completions", self.base_url);
        let payload = self.chat_payload(req)?;
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?;

        let response = Self::require_success(response).await?;
        let resp: ChatResponse = response_json_capped(response, MAX_DEEPSEEK_JSON_BYTES).await?;
        Ok(resp)
    }

    /// Non-streaming chat with retry for retryable errors.
    pub async fn chat_with_retry(&self, req: &ChatRequest) -> Result<ChatResponse, DeepSeekError> {
        let mut last_err = None;
        for attempt in 0..MAX_RETRIES {
            match self.chat(req).await {
                Ok(resp) => return Ok(resp),
                Err(e) if e.is_retryable() && attempt + 1 < MAX_RETRIES => {
                    let delay = e.retry_delay();
                    tracing::warn!(
                        "retry {}/{} after {:?}: {}",
                        attempt + 1,
                        MAX_RETRIES,
                        delay,
                        e
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        // This line is theoretically unreachable because the loop always returns
        // inside — either with Ok(resp) or the last Err(e). We keep it as a
        // defensive fallback with an explicit message in case the loop logic is
        // ever refactored incorrectly.
        Err(last_err.expect("MAX_RETRIES exhausted but no error was recorded"))
    }

    /// Streaming chat completion — returns an SSE event stream.
    pub async fn chat_stream(
        &self,
        req: &ChatRequest,
    ) -> Result<impl tokio_stream::Stream<Item = Result<StreamEvent, DeepSeekError>>, DeepSeekError>
    {
        let url = format!("{}/chat/completions", self.base_url);
        let payload = self.chat_payload(req)?;
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?;

        let response = Self::require_success(response).await?;
        Ok(parse_stream(response.bytes_stream()))
    }

    /// Stream and accumulate into a `StreamResult`.
    pub async fn chat_stream_accumulated(
        &self,
        req: &ChatRequest,
    ) -> Result<StreamResult, DeepSeekError> {
        self.chat_stream_accumulated_with_deltas(req, |_| {}).await
    }

    /// Stream and accumulate into a `StreamResult`, calling `on_chunk` for
    /// every received SSE chunk before it is merged into the final result.
    pub async fn chat_stream_accumulated_with_deltas<F>(
        &self,
        req: &ChatRequest,
        mut on_chunk: F,
    ) -> Result<StreamResult, DeepSeekError>
    where
        F: FnMut(&super::models::StreamChunk),
    {
        let stream = self.chat_stream(req).await?;
        accumulate_stream_events(stream, &mut on_chunk).await
    }

    /// Health check — returns raw JSON of available models.
    pub async fn list_models(&self) -> Result<serde_json::Value, DeepSeekError> {
        let url = format!("{}/models", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        let response = Self::require_success(response).await?;
        response_json_capped(response, MAX_DEEPSEEK_JSON_BYTES).await
    }

    /// FIM completion (non-thinking lane).
    pub async fn fim_completion(
        &self,
        req: &super::fim::FimRequest,
    ) -> Result<super::fim::FimResponse, DeepSeekError> {
        let url = format!("{}/beta/completions", self.base_url);
        let req = self.map_fim_request_model(req);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;

        let response = Self::require_success(response).await?;
        response_json_capped(response, MAX_DEEPSEEK_JSON_BYTES).await
    }
}

async fn response_json_capped<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<T, DeepSeekError> {
    let bytes = response_bytes_capped(response, max_bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn response_text_capped(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<String, DeepSeekError> {
    let bytes = response_bytes_capped(response, max_bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

async fn response_bytes_capped(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, DeepSeekError> {
    if let Some(content_length) = response.content_length() {
        if content_length > max_bytes as u64 {
            return Err(DeepSeekError::Other(format!(
                "response too large: {content_length} bytes exceeds {max_bytes} byte limit"
            )));
        }
    }
    collect_limited_body(response.bytes_stream(), max_bytes).await
}

async fn collect_limited_body<S>(stream: S, max_bytes: usize) -> Result<Vec<u8>, DeepSeekError>
where
    S: Stream<Item = reqwest::Result<Bytes>>,
{
    futures::pin_mut!(stream);

    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(DeepSeekError::Other(format!(
                "response too large: exceeds {max_bytes} byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_thinking_config(value: &serde_json::Value) -> Option<ThinkingConfig> {
    serde_json::from_value(value.clone()).ok()
}

fn thinking_budget_for_effort(effort: Option<&str>) -> Option<u32> {
    match effort {
        Some("min") => Some(1_024),
        Some("low") => Some(2_048),
        Some("medium") => Some(4_096),
        Some("high") => Some(8_192),
        Some("max") => Some(16_384),
        _ => None,
    }
}

async fn accumulate_stream_events<S, F>(
    stream: S,
    mut on_chunk: F,
) -> Result<StreamResult, DeepSeekError>
where
    S: Stream<Item = Result<StreamEvent, DeepSeekError>>,
    F: FnMut(&StreamChunk),
{
    tokio::pin!(stream);
    let mut accum = StreamAccumulator::new();

    const CHUNK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

    loop {
        let event = tokio::time::timeout(CHUNK_IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| {
                DeepSeekError::Other("stream idle timeout — no data received for 30s".into())
            })?;
        match event {
            Some(Ok(StreamEvent::Chunk(chunk))) => {
                on_chunk(&chunk);
                accum.apply_chunk(&chunk)?;
            }
            Some(Ok(StreamEvent::Done)) => return Ok(accum.finalize()),
            Some(Err(e)) => return Err(e),
            None => return Err(DeepSeekError::StreamEnd),
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{accumulate_stream_events, DeepSeekError, StreamEvent};
    use crate::deepseek::models::{ChatRequest, StreamChunk, ThinkingConfig, ThinkingWireFormat};

    fn chunk(content: &str) -> StreamChunk {
        serde_json::from_str::<StreamChunk>(&format!(
            r#"{{"choices":[{{"index":0,"delta":{{"content":"{content}"}},"finish_reason":null}}],"usage":null}}"#
        ))
        .expect("valid stream chunk")
    }

    #[tokio::test]
    async fn eof_without_done_returns_stream_end() {
        let stream = futures::stream::iter([Ok(StreamEvent::Chunk(chunk("partial")))]);

        let err = accumulate_stream_events(stream, |_| {})
            .await
            .expect_err("missing DONE should fail");

        assert!(matches!(err, DeepSeekError::StreamEnd));
    }

    #[tokio::test]
    async fn done_finishes_accumulated_stream() {
        let stream =
            futures::stream::iter([Ok(StreamEvent::Chunk(chunk("ok"))), Ok(StreamEvent::Done)]);

        let result = accumulate_stream_events(stream, |_| {})
            .await
            .expect("DONE should finish stream");

        assert_eq!(result.content, "ok");
    }

    #[tokio::test]
    async fn collect_limited_body_rejects_large_deepseek_response() {
        let chunks = futures::stream::iter([
            Ok(Bytes::from_static(b"12345")),
            Ok(Bytes::from_static(b"67890")),
        ]);

        let error = super::collect_limited_body(chunks, 8)
            .await
            .expect_err("body over limit should fail");

        assert!(
            error.to_string().contains("response too large"),
            "unexpected error: {error}"
        );
    }

    fn request_with_thinking(thinking: ThinkingConfig) -> ChatRequest {
        ChatRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: Vec::new(),
            tools: None,
            thinking: Some(thinking),
            response_format: None,
            stream: true,
            max_tokens: None,
        }
    }

    #[test]
    fn deepseek_payload_keeps_native_thinking() {
        let client = super::DeepSeekClient::new(String::new())
            .with_thinking_wire_format(ThinkingWireFormat::DeepSeekNative);
        let payload = client
            .chat_payload(&request_with_thinking(ThinkingConfig::with_effort("high")))
            .expect("payload");

        assert_eq!(payload["thinking"]["type"], "enabled");
        assert_eq!(payload["thinking"]["effort"], "high");
        assert!(payload.get("enable_thinking").is_none());
    }

    #[test]
    fn dashscope_payload_maps_thinking_to_enable_thinking() {
        let client = super::DeepSeekClient::new(String::new())
            .with_thinking_wire_format(ThinkingWireFormat::DashScopeEnableThinking);
        let payload = client
            .chat_payload(&request_with_thinking(ThinkingConfig::with_effort("high")))
            .expect("payload");

        assert!(payload.get("thinking").is_none());
        assert_eq!(payload["enable_thinking"], true);
        assert_eq!(payload["thinking_budget"], 8192);
    }

    #[test]
    fn qianfan_payload_maps_thinking_to_nested_and_top_level_flags() {
        let client = super::DeepSeekClient::new(String::new())
            .with_thinking_wire_format(ThinkingWireFormat::QianfanThinking);
        let payload = client
            .chat_payload(&request_with_thinking(ThinkingConfig::with_effort(
                "medium",
            )))
            .expect("payload");

        assert_eq!(payload["thinking"]["type"], "enabled");
        assert_eq!(payload["enable_thinking"], true);
        assert_eq!(payload["thinking_budget"], 4096);
    }

    #[test]
    fn minimax_payload_maps_thinking_to_reasoning_split() {
        let client = super::DeepSeekClient::new(String::new())
            .with_thinking_wire_format(ThinkingWireFormat::MiniMaxReasoningSplit);
        let payload = client
            .chat_payload(&request_with_thinking(ThinkingConfig::enabled()))
            .expect("payload");

        assert!(payload.get("thinking").is_none());
        assert_eq!(payload["reasoning_split"], true);
    }

    #[test]
    fn native_type_only_payload_strips_effort() {
        let client = super::DeepSeekClient::new(String::new())
            .with_thinking_wire_format(ThinkingWireFormat::NativeTypeOnly);
        let payload = client
            .chat_payload(&request_with_thinking(ThinkingConfig::with_effort("max")))
            .expect("payload");

        assert_eq!(payload["thinking"]["type"], "enabled");
        assert!(payload["thinking"].get("effort").is_none());
    }

    #[test]
    fn unsupported_payload_removes_thinking() {
        let client = super::DeepSeekClient::new(String::new())
            .with_thinking_wire_format(ThinkingWireFormat::Unsupported);
        let payload = client
            .chat_payload(&request_with_thinking(ThinkingConfig::enabled()))
            .expect("payload");

        assert!(payload.get("thinking").is_none());
    }
}

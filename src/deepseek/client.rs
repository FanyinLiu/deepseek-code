use std::time::Duration;

use futures::StreamExt;
use reqwest::Client as HttpClient;

use super::errors::DeepSeekError;
use super::models::{ChatRequest, ChatResponse, StreamResult};
use super::stream::{parse_stream, StreamAccumulator, StreamEvent};

const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    http: HttpClient,
    api_key: String,
    base_url: String,
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
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    async fn require_success(
        response: reqwest::Response,
    ) -> Result<reqwest::Response, DeepSeekError> {
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(DeepSeekError::from_status(status.as_u16(), &body));
        }
        Ok(response)
    }

    /// Non-streaming chat completion.
    pub async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, DeepSeekError> {
        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await?;

        let response = Self::require_success(response).await?;
        let resp: ChatResponse = response.json().await?;
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
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(req)
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
        let mut stream = self.chat_stream(req).await?;
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
                Some(Ok(StreamEvent::Done)) => break,
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }

        Ok(accum.finalize())
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
        Ok(response.json().await?)
    }

    /// FIM completion (non-thinking lane).
    pub async fn fim_completion(
        &self,
        req: &super::fim::FimRequest,
    ) -> Result<super::fim::FimResponse, DeepSeekError> {
        let url = format!("{}/beta/completions", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await?;

        let response = Self::require_success(response).await?;
        Ok(response.json().await?)
    }
}

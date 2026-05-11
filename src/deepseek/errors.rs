use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum DeepSeekError {
    #[error("authentication failed — run `deepseek-code login`")]
    Unauthorized,

    #[error("insufficient balance or quota exceeded")]
    PaymentRequired,

    #[error("invalid request: {0}")]
    BadRequest(String),

    #[error("request could not be processed: {0}")]
    Unprocessable(String),

    #[error("rate limited — retry after {0:?}")]
    RateLimited(Option<Duration>),

    #[error("server error ({0}) — try again later")]
    ServerError(u16),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("stream ended unexpectedly")]
    StreamEnd,

    #[error("model returned empty content — try again or simplify the request")]
    EmptyContent,

    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl DeepSeekError {
    #[must_use]
    pub fn from_status(status: u16, body: &str) -> Self {
        match status {
            400 => Self::BadRequest(body.to_string()),
            401 => Self::Unauthorized,
            402 => Self::PaymentRequired,
            422 => Self::Unprocessable(body.to_string()),
            429 => Self::RateLimited(None),
            408 | 500 | 502 | 503 | 504 => Self::ServerError(status),
            _ => Self::Other(format!("HTTP {status}: {body}")),
        }
    }

    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited(_) | Self::ServerError(_) | Self::Network(_)
        )
    }

    #[must_use]
    pub fn retry_delay(&self) -> Duration {
        match self {
            Self::RateLimited(Some(d)) => *d,
            Self::RateLimited(None) => Duration::from_secs(5),
            Self::ServerError(_) => Duration::from_secs(2),
            _ => Duration::from_secs(0),
        }
    }
}

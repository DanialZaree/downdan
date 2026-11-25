use std::sync::Arc;

use tauri::http::StatusCode;
use tauri_plugin_http::reqwest;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum ClientError {
    #[error("{0}")]
    Reqwest(#[from] Arc<reqwest::Error>),

    #[error("{status}")]
    Http { status: StatusCode },

    #[error("{0}")]
    Deserialize(#[from] Arc<serde_json::Error>),

    #[error("missing or invalid Content-Length header")]
    MissingContentLength,

    #[error("timeout error")]
    StreamTimeout,

    #[error("unexpected chunk hash")]
    UnexpectedChunkHash,

    #[error("server ignored the byte range request")]
    RangeNotSupported,

    #[error("server returned an unexpected Content-Range")]
    UnexpectedContentRange,

    #[error("failed to write to disk: {0}")]
    DiskWrite(String),
}

impl From<reqwest::Error> for ClientError {
    fn from(value: reqwest::Error) -> Self {
        ClientError::Reqwest(Arc::new(value))
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(value: serde_json::Error) -> Self {
        ClientError::Deserialize(Arc::new(value))
    }
}

impl ClientError {
    pub fn is_retryable(&self) -> bool {
        match self {
            // `is_body`/`is_decode` cover connections that drop in the middle of a stream.
            ClientError::Reqwest(e) => {
                e.is_timeout() || e.is_connect() || e.is_request() || e.is_body() || e.is_decode()
            }
            ClientError::Http { status, .. } => {
                matches!(status.as_u16(), 408 | 425 | 429 | 500..=504)
            }
            ClientError::StreamTimeout | ClientError::UnexpectedChunkHash => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_retry_policy() {
        let retry = |code: u16| {
            ClientError::Http {
                status: StatusCode::from_u16(code).unwrap(),
            }
            .is_retryable()
        };

        assert!(retry(429));
        assert!(retry(503));
        assert!(retry(408));
        assert!(!retry(403));
        assert!(!retry(404));
        assert!(!retry(410));
        assert!(!retry(416));
    }

    #[test]
    fn non_retryable_engine_errors() {
        assert!(!ClientError::RangeNotSupported.is_retryable());
        assert!(!ClientError::UnexpectedContentRange.is_retryable());
        assert!(!ClientError::DiskWrite("disk full".into()).is_retryable());
        assert!(ClientError::StreamTimeout.is_retryable());
    }
}

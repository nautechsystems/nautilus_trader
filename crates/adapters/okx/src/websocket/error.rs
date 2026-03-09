//! Error types produced by the OKX WebSocket client implementation.

use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// A typed error enumeration for the OKX WebSocket client.
#[derive(Debug, Clone, Error)]
pub enum OKXWsError {
    #[error("Parsing error: {0}")]
    ParsingError(String),
    /// Errors returned directly by OKX (non-zero code).
    #[error("OKX error {error_code}: {message}")]
    OkxError { error_code: String, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    #[error("Client error: {0}")]
    ClientError(String),
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
    /// Wrapping the underlying HttpClientError from the network crate.
    // #[error("Network error: {0}")]
    // WebSocketClientError(WebSocketClientError),  // TODO: Implement Debug
    /// WebSocket transport error.
    #[error("Tungstenite error: {0}")]
    TungsteniteError(String),
}

impl From<tungstenite::Error> for OKXWsError {
    fn from(error: tungstenite::Error) -> Self {
        Self::TungsteniteError(error.to_string())
    }
}

impl From<String> for OKXWsError {
    fn from(msg: String) -> Self {
        Self::AuthenticationError(msg)
    }
}

//! Error types for Kraken WebSocket client operations.

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum KrakenWsError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    #[error("JSON error: {0}")]
    JsonError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Disconnected: {0}")]
    Disconnected(String),

    #[error("Timeout: {0}")]
    Timeout(String),
}

impl From<serde_json::Error> for KrakenWsError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

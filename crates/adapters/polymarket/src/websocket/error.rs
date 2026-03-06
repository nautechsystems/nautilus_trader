//! WebSocket errors for Polymarket.

use thiserror::Error;

/// Errors for Polymarket WebSocket operations.
#[derive(Debug, Clone, Error)]
pub enum PolymarketWsError {
    #[error("URL parsing failed: {0}")]
    UrlParsing(String),

    #[error("Message serialization failed: {0}")]
    MessageSerialization(String),

    #[error("Message deserialization failed: {0}")]
    MessageDeserialization(String),

    #[error("WebSocket connection failed: {0}")]
    Connection(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Channel send failed: {0}")]
    ChannelSend(String),

    #[error("Tungstenite error: {0}")]
    TungsteniteError(String),
}

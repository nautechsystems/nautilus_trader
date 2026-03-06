//! WebSocket errors for Hyperliquid.

/// Errors that can occur during Hyperliquid WebSocket operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum HyperliquidWsError {
    #[error("URL parsing failed: {0}")]
    UrlParsing(String),

    #[error("Message serialization failed: {0}")]
    MessageSerialization(String),

    #[error("Message deserialization failed: {0}")]
    MessageDeserialization(String),

    #[error("WebSocket connection failed: {0}")]
    Connection(String),

    #[error("Channel send failed: {0}")]
    ChannelSend(String),

    #[error("Client error: {0}")]
    ClientError(String),

    #[error("Tungstenite error: {0}")]
    TungsteniteError(String),
}

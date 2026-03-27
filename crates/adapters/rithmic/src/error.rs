//! Error types for the Rithmic adapter.
//!
//! This module defines error types following NautilusTrader patterns:
//! - `RithmicWsError` for WebSocket-specific errors
//! - `RithmicError` as the top-level aggregating error type

use thiserror::Error;

/// WebSocket-specific error type for Rithmic connections.
///
/// Since Rithmic uses Protocol Buffers over WebSocket for all communication,
/// this error type covers connection, authentication, and message handling.
#[derive(Debug, Error)]
pub enum RithmicWsError {
    /// Connection failed.
    #[error("WebSocket connection failed: {0}")]
    ConnectionFailed(String),

    /// Connection closed unexpectedly.
    #[error("WebSocket connection closed: {0}")]
    ConnectionClosed(String),

    /// Authentication failed.
    #[error("WebSocket authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Message send failed.
    #[error("Failed to send WebSocket message: {0}")]
    SendFailed(String),

    /// Message receive failed.
    #[error("Failed to receive WebSocket message: {0}")]
    ReceiveFailed(String),

    /// Protocol buffer decode error.
    #[error("Protobuf decode error: {0}")]
    DecodeError(String),

    /// Protocol buffer encode error.
    #[error("Protobuf encode error: {0}")]
    EncodeError(String),

    /// Subscription error.
    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    /// Heartbeat timeout.
    #[error("Heartbeat timeout")]
    HeartbeatTimeout,

    /// Reconnection failed.
    #[error("Reconnection failed after {attempts} attempts: {reason}")]
    ReconnectionFailed { attempts: u32, reason: String },
}

impl RithmicWsError {
    /// Returns true if this error is retriable.
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed(_)
                | Self::ConnectionClosed(_)
                | Self::SendFailed(_)
                | Self::ReceiveFailed(_)
                | Self::HeartbeatTimeout
        )
    }
}

/// Top-level Rithmic adapter error type.
///
/// Aggregates all error types from the adapter.
#[derive(Debug, Error)]
pub enum RithmicError {
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// WebSocket error.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] RithmicWsError),

    /// Connection error.
    #[error("Connection error: {0}")]
    Connection(String),

    /// Not connected error.
    #[error("Not connected")]
    NotConnected,

    /// Authentication error.
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Order error.
    #[error("Order error: {0}")]
    Order(String),

    /// Parse error.
    #[error("Parse error: {0}")]
    Parse(String),

    /// Instrument error.
    #[error("Instrument error: {0}")]
    Instrument(String),

    /// Timeout error.
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Channel error (internal messaging).
    #[error("Channel error: {0}")]
    Channel(String),

    /// Underlying rithmic-rs API error.
    #[error("Rithmic API error: {0}")]
    Api(String),
}

/// Result type alias for Rithmic operations.
pub type Result<T> = std::result::Result<T, RithmicError>;

/// Result type alias for WebSocket operations.
pub type WsResult<T> = std::result::Result<T, RithmicWsError>;

impl RithmicError {
    /// Returns true if this is a retriable error.
    pub fn is_retriable(&self) -> bool {
        match self {
            Self::WebSocket(ws_err) => ws_err.is_retriable(),
            Self::Connection(_) | Self::NotConnected | Self::Timeout(_) | Self::Channel(_) => true,
            _ => false,
        }
    }

    /// Returns true if this is an authentication error.
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::Authentication(_) | Self::WebSocket(RithmicWsError::AuthenticationFailed(_))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_error_retriable() {
        assert!(RithmicWsError::ConnectionFailed("test".to_string()).is_retriable());
        assert!(RithmicWsError::HeartbeatTimeout.is_retriable());
        assert!(!RithmicWsError::AuthenticationFailed("test".to_string()).is_retriable());
    }

    #[test]
    fn test_error_from_ws_error() {
        let ws_err = RithmicWsError::ConnectionFailed("test".to_string());
        let err: RithmicError = ws_err.into();
        assert!(err.is_retriable());
    }

    #[test]
    fn test_auth_error_detection() {
        let err = RithmicError::Authentication("invalid credentials".to_string());
        assert!(err.is_auth_error());

        let ws_auth_err = RithmicError::WebSocket(RithmicWsError::AuthenticationFailed(
            "bad token".to_string(),
        ));
        assert!(ws_auth_err.is_auth_error());
    }
}

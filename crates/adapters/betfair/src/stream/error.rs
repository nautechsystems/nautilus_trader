//! Betfair stream client error types.

use std::fmt::Display;

/// Represents stream client errors for the Betfair adapter.
#[derive(Debug, Clone)]
pub enum BetfairStreamError {
    /// Failed to establish a connection.
    ConnectionFailed(String),
    /// Stream authentication failed.
    AuthenticationFailed(String),
    /// Stream protocol error (unexpected message format).
    ProtocolError(String),
    /// JSON serialization/deserialization error.
    JsonError(String),
    /// Connection or read timeout.
    Timeout(String),
    /// Connection was lost.
    Disconnected(String),
}

impl Display for BetfairStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "Connection failed: {msg}"),
            Self::AuthenticationFailed(msg) => write!(f, "Authentication failed: {msg}"),
            Self::ProtocolError(msg) => write!(f, "Protocol error: {msg}"),
            Self::JsonError(msg) => write!(f, "JSON error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Disconnected(msg) => write!(f, "Disconnected: {msg}"),
        }
    }
}

impl std::error::Error for BetfairStreamError {}

impl From<serde_json::Error> for BetfairStreamError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        BetfairStreamError::ConnectionFailed("refused".to_string()),
        "Connection failed: refused"
    )]
    #[case(
        BetfairStreamError::AuthenticationFailed("invalid token".to_string()),
        "Authentication failed: invalid token"
    )]
    #[case(
        BetfairStreamError::ProtocolError("bad frame".to_string()),
        "Protocol error: bad frame"
    )]
    #[case(
        BetfairStreamError::JsonError("parse error".to_string()),
        "JSON error: parse error"
    )]
    #[case(
        BetfairStreamError::Timeout("read".to_string()),
        "Timeout: read"
    )]
    #[case(
        BetfairStreamError::Disconnected("reset".to_string()),
        "Disconnected: reset"
    )]
    fn test_display(#[case] error: BetfairStreamError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_from_serde_error() {
        let json_err = serde_json::from_str::<String>("bad").unwrap_err();
        let err: BetfairStreamError = json_err.into();
        assert!(matches!(err, BetfairStreamError::JsonError(_)));
    }
}

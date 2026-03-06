//! Binance WebSocket error types.

use std::fmt;

/// Binance WebSocket client error type.
#[derive(Debug)]
pub enum BinanceWsError {
    /// General client error.
    ClientError(String),
    /// Authentication failed.
    AuthenticationError(String),
    /// Message parsing error.
    ParseError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Operation timed out.
    Timeout(String),
}

impl fmt::Display for BinanceWsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientError(msg) => write!(f, "Client error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
        }
    }
}

impl std::error::Error for BinanceWsError {}

/// Result type for Binance WebSocket operations.
pub type BinanceWsResult<T> = Result<T, BinanceWsError>;

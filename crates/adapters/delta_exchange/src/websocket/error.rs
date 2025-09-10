// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Error types for Delta Exchange WebSocket client.

use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// A typed error enumeration for the Delta Exchange WebSocket client.
#[derive(Debug, Error)]
pub enum DeltaExchangeWsError {
    /// Connection establishment errors.
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// Authentication errors during WebSocket handshake.
    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    /// Subscription management errors.
    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    /// Message parsing errors.
    #[error("Parsing error: {0}")]
    ParsingError(String),

    /// Errors returned directly by Delta Exchange.
    #[error("Delta Exchange error [{code}]: {message}")]
    DeltaExchangeError { code: String, message: String },

    /// JSON serialization/deserialization errors.
    #[error("JSON error: {0}")]
    JsonError(String),

    /// WebSocket client errors.
    #[error("Client error: {0}")]
    ClientError(String),

    /// Connection timeout errors.
    #[error("Connection timeout: {0}")]
    TimeoutError(String),

    /// Rate limiting errors.
    #[error("Rate limit exceeded: {0}")]
    RateLimitError(String),

    /// Reconnection errors.
    #[error("Reconnection failed: {0}")]
    ReconnectionError(String),

    /// Channel subscription errors.
    #[error("Channel error: {0}")]
    ChannelError(String),

    /// Message queue errors.
    #[error("Message queue error: {0}")]
    MessageQueueError(String),

    /// Underlying tungstenite WebSocket errors.
    #[error("WebSocket error: {0}")]
    TungsteniteError(#[from] tungstenite::Error),

    /// Task execution errors.
    #[error("Task error: {0}")]
    TaskError(String),

    /// Configuration errors.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// State management errors.
    #[error("State error: {0}")]
    StateError(String),
}

impl DeltaExchangeWsError {
    /// Check if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionError(_)
                | Self::TimeoutError(_)
                | Self::RateLimitError(_)
                | Self::TungsteniteError(_)
                | Self::MessageQueueError(_)
        )
    }

    /// Check if the error is due to authentication issues.
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::AuthenticationError(_))
    }

    /// Check if the error is due to rate limiting.
    pub fn is_rate_limit_error(&self) -> bool {
        matches!(self, Self::RateLimitError(_))
    }

    /// Check if the error requires reconnection.
    pub fn requires_reconnection(&self) -> bool {
        matches!(
            self,
            Self::ConnectionError(_)
                | Self::TimeoutError(_)
                | Self::TungsteniteError(_)
                | Self::AuthenticationError(_)
        )
    }

    /// Get the error message for logging.
    pub fn message(&self) -> String {
        match self {
            Self::ConnectionError(msg) => msg.clone(),
            Self::AuthenticationError(msg) => msg.clone(),
            Self::SubscriptionError(msg) => msg.clone(),
            Self::ParsingError(msg) => msg.clone(),
            Self::DeltaExchangeError { message, .. } => message.clone(),
            Self::JsonError(msg) => msg.clone(),
            Self::ClientError(msg) => msg.clone(),
            Self::TimeoutError(msg) => msg.clone(),
            Self::RateLimitError(msg) => msg.clone(),
            Self::ReconnectionError(msg) => msg.clone(),
            Self::ChannelError(msg) => msg.clone(),
            Self::MessageQueueError(msg) => msg.clone(),
            Self::TungsteniteError(err) => err.to_string(),
            Self::TaskError(msg) => msg.clone(),
            Self::ConfigError(msg) => msg.clone(),
            Self::StateError(msg) => msg.clone(),
        }
    }

    /// Create a connection error.
    pub fn connection_error(message: impl Into<String>) -> Self {
        Self::ConnectionError(message.into())
    }

    /// Create an authentication error.
    pub fn auth_error(message: impl Into<String>) -> Self {
        Self::AuthenticationError(message.into())
    }

    /// Create a subscription error.
    pub fn subscription_error(message: impl Into<String>) -> Self {
        Self::SubscriptionError(message.into())
    }

    /// Create a parsing error.
    pub fn parsing_error(message: impl Into<String>) -> Self {
        Self::ParsingError(message.into())
    }

    /// Create a timeout error.
    pub fn timeout_error(message: impl Into<String>) -> Self {
        Self::TimeoutError(message.into())
    }

    /// Create a rate limit error.
    pub fn rate_limit_error(message: impl Into<String>) -> Self {
        Self::RateLimitError(message.into())
    }
}

impl From<serde_json::Error> for DeltaExchangeWsError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<tokio::time::error::Elapsed> for DeltaExchangeWsError {
    fn from(error: tokio::time::error::Elapsed) -> Self {
        Self::TimeoutError(error.to_string())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_retryable() {
        let connection_error = DeltaExchangeWsError::connection_error("Connection failed");
        assert!(connection_error.is_retryable());

        let auth_error = DeltaExchangeWsError::auth_error("Invalid credentials");
        assert!(!auth_error.is_retryable());
    }

    #[test]
    fn test_error_auth_check() {
        let auth_error = DeltaExchangeWsError::auth_error("Authentication failed");
        assert!(auth_error.is_auth_error());

        let connection_error = DeltaExchangeWsError::connection_error("Connection failed");
        assert!(!connection_error.is_auth_error());
    }

    #[test]
    fn test_error_rate_limit_check() {
        let rate_limit_error = DeltaExchangeWsError::rate_limit_error("Too many connections");
        assert!(rate_limit_error.is_rate_limit_error());

        let parsing_error = DeltaExchangeWsError::parsing_error("Invalid JSON");
        assert!(!parsing_error.is_rate_limit_error());
    }

    #[test]
    fn test_error_requires_reconnection() {
        let connection_error = DeltaExchangeWsError::connection_error("Connection lost");
        assert!(connection_error.requires_reconnection());

        let parsing_error = DeltaExchangeWsError::parsing_error("Invalid message");
        assert!(!parsing_error.requires_reconnection());
    }

    #[test]
    fn test_error_message() {
        let error = DeltaExchangeWsError::DeltaExchangeError {
            code: "INVALID_CHANNEL".to_string(),
            message: "Channel not found".to_string(),
        };
        assert_eq!(error.message(), "Channel not found");
    }
}

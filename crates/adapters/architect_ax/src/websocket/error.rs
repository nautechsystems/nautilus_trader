// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Error types produced by the Ax WebSocket client implementation.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// A typed error enumeration for the Ax WebSocket client.
#[derive(Debug, Clone, Error)]
pub enum AxWsError {
    /// Failure to parse incoming message.
    #[error("Parsing error: {0}")]
    ParsingError(String),
    /// Errors returned directly by AX Exchange API.
    #[error("Ax error: {0}")]
    ApiError(String),
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Generic client error.
    #[error("Client error: {0}")]
    ClientError(String),
    /// Authentication error (invalid/expired token, etc.).
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
    /// Connection error during WebSocket setup.
    #[error("Connection error: {0}")]
    ConnectionError(String),
    /// Subscription error (invalid symbol, already subscribed, etc.).
    #[error("Subscription error: {0}")]
    SubscriptionError(String),
    /// Order operation error (rejection, cancellation failure, etc.).
    #[error("Order error: {0}")]
    OrderError(String),
    /// WebSocket transport error.
    #[error("Tungstenite error: {0}")]
    TungsteniteError(String),
    /// Channel communication error.
    #[error("Channel error: {0}")]
    ChannelError(String),
    /// Timeout waiting for response.
    #[error("Timeout: {0}")]
    Timeout(String),
}

impl From<tungstenite::Error> for AxWsError {
    fn from(error: tungstenite::Error) -> Self {
        Self::TungsteniteError(error.to_string())
    }
}

impl From<serde_json::Error> for AxWsError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<String> for AxWsError {
    fn from(msg: String) -> Self {
        Self::ClientError(msg)
    }
}

/// Represents an error response from the Ax WebSocket API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsErrorResponse {
    /// Error code.
    #[serde(default)]
    pub code: Option<String>,
    /// Error message.
    #[serde(default)]
    pub message: Option<String>,
    /// Request ID if available.
    #[serde(default)]
    pub rid: Option<i64>,
}

impl From<AxWsErrorResponse> for AxWsError {
    fn from(error: AxWsErrorResponse) -> Self {
        let message = error
            .message
            .or(error.code)
            .unwrap_or_else(|| "Unknown error".to_string());
        Self::ApiError(message)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_architect_ws_error_display() {
        let error = AxWsError::ParsingError("invalid message format".to_string());
        assert_eq!(error.to_string(), "Parsing error: invalid message format");

        let error = AxWsError::ApiError("INSUFFICIENT_MARGIN".to_string());
        assert_eq!(error.to_string(), "Ax error: INSUFFICIENT_MARGIN");

        let error = AxWsError::AuthenticationError("token expired".to_string());
        assert_eq!(error.to_string(), "Authentication error: token expired");
    }

    #[rstest]
    fn test_architect_ws_error_from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json")
            .expect_err("Should fail to parse");
        let ws_err = AxWsError::from(json_err);

        assert!(matches!(ws_err, AxWsError::JsonError(_)));
    }

    #[rstest]
    fn test_architect_ws_error_from_string() {
        let error = AxWsError::from("Test client error".to_string());
        assert_eq!(error.to_string(), "Client error: Test client error");
    }

    #[rstest]
    fn test_architect_ws_error_response_to_error() {
        let error_response = AxWsErrorResponse {
            code: Some("ORDER_NOT_FOUND".to_string()),
            message: Some("Order does not exist".to_string()),
            rid: Some(123),
        };

        let ws_error = AxWsError::from(error_response);
        assert_eq!(ws_error.to_string(), "Ax error: Order does not exist");
    }

    #[rstest]
    fn test_architect_ws_error_response_fallback_to_code() {
        let error_response = AxWsErrorResponse {
            code: Some("INVALID_REQUEST".to_string()),
            message: None,
            rid: None,
        };

        let ws_error = AxWsError::from(error_response);
        assert_eq!(ws_error.to_string(), "Ax error: INVALID_REQUEST");
    }
}

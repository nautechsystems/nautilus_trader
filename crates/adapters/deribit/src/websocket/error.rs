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

//! Deribit WebSocket client error types.

use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// Error types for the Deribit WebSocket client.
#[derive(Debug, Clone, Error)]
pub enum DeribitWsError {
    /// Client is not connected.
    #[error("Not connected")]
    NotConnected,
    /// Transport-level error during WebSocket communication.
    #[error("Transport error: {0}")]
    Transport(String),
    /// Failed to send message over WebSocket.
    #[error("Send error: {0}")]
    Send(String),
    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(String),
    /// Authentication failed.
    #[error("Authentication error: {0}")]
    Authentication(String),
    /// Generic client error.
    #[error("Client error: {0}")]
    ClientError(String),
    /// Parsing error during message processing.
    #[error("Parsing error: {0}")]
    ParsingError(String),
    /// Error returned by Deribit API (JSON-RPC error response).
    #[error("Deribit error {code}: {message}")]
    DeribitError {
        /// The error code from Deribit.
        code: i64,
        /// The error message from Deribit.
        message: String,
    },
    /// WebSocket transport error from tungstenite.
    #[error("Tungstenite error: {0}")]
    TungsteniteError(String),
    /// Request timeout.
    #[error("Timeout: {0}")]
    Timeout(String),
}

impl From<tungstenite::Error> for DeribitWsError {
    fn from(error: tungstenite::Error) -> Self {
        Self::TungsteniteError(error.to_string())
    }
}

impl From<serde_json::Error> for DeribitWsError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}

impl From<String> for DeribitWsError {
    fn from(msg: String) -> Self {
        Self::ClientError(msg)
    }
}

/// Result type alias for Deribit WebSocket operations.
pub type DeribitWsResult<T> = Result<T, DeribitWsError>;

/// Determines if an error should trigger a retry.
#[must_use]
pub fn should_retry_deribit_ws_error(error: &DeribitWsError) -> bool {
    match error {
        DeribitWsError::Transport(_)
        | DeribitWsError::Send(_)
        | DeribitWsError::NotConnected
        | DeribitWsError::Timeout(_) => true,
        DeribitWsError::DeribitError { code, .. } => {
            // Deribit retriable error codes
            matches!(
                code,
                10028 | 10040 | 10041 | 10047 | 10066 | 11051 | 11094 | 13028 | 13888
            )
        }
        DeribitWsError::Json(_)
        | DeribitWsError::Authentication(_)
        | DeribitWsError::ClientError(_)
        | DeribitWsError::ParsingError(_)
        | DeribitWsError::TungsteniteError(_) => false,
    }
}

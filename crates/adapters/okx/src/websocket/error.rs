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

//! Error types produced by the OKX WebSocket client implementation.

use nautilus_network::error::SendError;
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
    #[error("No active WebSocket client")]
    NoActiveClient,
    #[error("Handler not available: {0}")]
    HandlerUnavailable(String),
    /// A typed failure from the shared WebSocket send boundary.
    #[error("WebSocket send error: {0}")]
    TransportSend(#[from] SendError),
    /// A send failure whose delivery outcome is unknown.
    #[error("Send failed: {0}")]
    SendFailed(String),
    #[error("Operation timed out after {timeout_ms}ms")]
    OperationTimeout { timeout_ms: u64 },
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

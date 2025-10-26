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

//! Error types for Bybit WebSocket client operations.

use nautilus_network::error::SendError;
use thiserror::Error;

/// Result alias for Bybit WebSocket operations.
pub type BybitWsResult<T> = Result<T, BybitWsError>;

/// Error type for Bybit WebSocket client failures.
#[derive(Debug, Error)]
pub enum BybitWsError {
    /// The WebSocket client is not currently connected.
    #[error("WebSocket not connected")]
    NotConnected,

    /// Failed to send a message over the WebSocket connection.
    #[error("WebSocket send error: {0}")]
    Send(String),

    /// Underlying transport error from the WebSocket implementation.
    #[error("WebSocket transport error: {0}")]
    Transport(String),

    /// Failed to parse or serialize JSON payloads.
    #[error("JSON error: {0}")]
    Json(String),

    /// Authentication handshake failed or timed out.
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Client-side validation or logic error.
    #[error("Client error: {0}")]
    ClientError(String),
}

impl From<SendError> for BybitWsError {
    fn from(error: SendError) -> Self {
        Self::Send(error.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for BybitWsError {
    fn from(error: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Transport(error.to_string())
    }
}

impl From<serde_json::Error> for BybitWsError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}

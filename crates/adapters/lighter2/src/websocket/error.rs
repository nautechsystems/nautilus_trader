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

//! Error types for Lighter WebSocket client.

use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// Errors that can occur when using the Lighter WebSocket client.
#[derive(Error, Debug)]
pub enum LighterWsError {
    /// WebSocket connection error.
    #[error("WebSocket connection error: {0}")]
    Connection(#[from] tungstenite::Error),

    /// JSON parsing error.
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    /// Authentication error.
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Subscription error.
    #[error("Subscription error: {0}")]
    Subscription(String),

    /// Message send error.
    #[error("Failed to send message: {0}")]
    Send(String),

    /// Connection closed.
    #[error("Connection closed")]
    Closed,

    /// Other error.
    #[error("{0}")]
    Other(String),
}

/// Result type for Lighter WebSocket operations.
pub type LighterWsResult<T> = Result<T, LighterWsError>;

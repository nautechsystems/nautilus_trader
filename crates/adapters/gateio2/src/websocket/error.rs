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

//! WebSocket client error types.

use std::fmt;

/// Result type for Gate.io WebSocket operations.
pub type GateioWsResult<T> = Result<T, GateioWsError>;

/// Errors that can occur during Gate.io WebSocket operations.
#[derive(Debug)]
pub enum GateioWsError {
    /// WebSocket connection error
    ConnectionError(String),
    /// Message parsing error
    ParseError(String),
    /// Subscription error
    SubscriptionError(String),
    /// Authentication error
    AuthError(String),
    /// Channel error
    ChannelError(String),
    /// Other error
    Other(String),
}

impl fmt::Display for GateioWsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionError(msg) => write!(f, "WebSocket connection error: {}", msg),
            Self::ParseError(msg) => write!(f, "Message parse error: {}", msg),
            Self::SubscriptionError(msg) => write!(f, "Subscription error: {}", msg),
            Self::AuthError(msg) => write!(f, "Authentication error: {}", msg),
            Self::ChannelError(msg) => write!(f, "Channel error: {}", msg),
            Self::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for GateioWsError {}

impl From<anyhow::Error> for GateioWsError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err.to_string())
    }
}

impl From<serde_json::Error> for GateioWsError {
    fn from(err: serde_json::Error) -> Self {
        Self::ParseError(err.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for GateioWsError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::ConnectionError(err.to_string())
    }
}

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

//! Hyperliquid error types.

use std::fmt;

/// Hyperliquid HTTP client error
#[derive(Debug)]
pub enum HyperliquidHttpError {
    /// HTTP request error
    HttpRequest(String),
    /// JSON parsing error
    JsonParse(String),
    /// API error response
    ApiError { code: i32, message: String },
    /// Invalid response
    InvalidResponse(String),
    /// Authentication error
    Authentication(String),
    /// Other error
    Other(String),
}

impl fmt::Display for HyperliquidHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HttpRequest(msg) => write!(f, "HTTP request error: {}", msg),
            Self::JsonParse(msg) => write!(f, "JSON parse error: {}", msg),
            Self::ApiError { code, message } => {
                write!(f, "Hyperliquid API error [{}]: {}", code, message)
            }
            Self::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            Self::Authentication(msg) => write!(f, "Authentication error: {}", msg),
            Self::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for HyperliquidHttpError {}

impl From<reqwest::Error> for HyperliquidHttpError {
    fn from(err: reqwest::Error) -> Self {
        Self::HttpRequest(err.to_string())
    }
}

impl From<serde_json::Error> for HyperliquidHttpError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonParse(err.to_string())
    }
}

impl From<anyhow::Error> for HyperliquidHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err.to_string())
    }
}

/// Hyperliquid WebSocket error
#[derive(Debug)]
pub enum HyperliquidWebSocketError {
    /// Connection error
    Connection(String),
    /// Send error
    Send(String),
    /// Receive error
    Receive(String),
    /// Subscription error
    Subscription(String),
    /// JSON parsing error
    JsonParse(String),
    /// Other error
    Other(String),
}

impl fmt::Display for HyperliquidWebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(msg) => write!(f, "WebSocket connection error: {}", msg),
            Self::Send(msg) => write!(f, "WebSocket send error: {}", msg),
            Self::Receive(msg) => write!(f, "WebSocket receive error: {}", msg),
            Self::Subscription(msg) => write!(f, "WebSocket subscription error: {}", msg),
            Self::JsonParse(msg) => write!(f, "JSON parse error: {}", msg),
            Self::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for HyperliquidWebSocketError {}

impl From<serde_json::Error> for HyperliquidWebSocketError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonParse(err.to_string())
    }
}

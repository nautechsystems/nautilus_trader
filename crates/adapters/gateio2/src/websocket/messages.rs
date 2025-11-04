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

//! Gate.io WebSocket message types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// WebSocket subscription/unsubscription request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsRequest {
    /// Request time (Unix timestamp in seconds)
    pub time: i64,
    /// Channel name
    pub channel: String,
    /// Event type (subscribe or unsubscribe)
    pub event: String,
    /// Payload (for authenticated channels)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Vec<String>>,
    /// Authentication info (for private channels)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<WsAuth>,
}

/// Authentication information for WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsAuth {
    /// HTTP method
    pub method: String,
    /// Signature
    pub sign: String,
    /// Timestamp
    pub timestamp: String,
    /// API key
    pub key: String,
}

/// WebSocket response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsResponse {
    /// Response time (Unix timestamp in seconds)
    pub time: i64,
    /// Channel name
    pub channel: String,
    /// Event type
    pub event: String,
    /// Error info (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WsError>,
    /// Result data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

/// WebSocket error message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
}

/// WebSocket ping/pong message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsPing {
    /// Ping time (Unix timestamp in seconds)
    pub time: i64,
    /// Channel (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Event ("ping")
    pub event: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsPong {
    /// Pong time (Unix timestamp in seconds)
    pub time: i64,
    /// Channel (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Event ("pong")
    pub event: String,
}

/// Helper to check if message is a ping
pub fn is_ping(value: &Value) -> bool {
    value
        .get("event")
        .and_then(|v| v.as_str())
        .map(|s| s == "ping")
        .unwrap_or(false)
}

/// Helper to check if message is a pong
pub fn is_pong(value: &Value) -> bool {
    value
        .get("event")
        .and_then(|v| v.as_str())
        .map(|s| s == "pong")
        .unwrap_or(false)
}

/// Helper to check if message is a subscription response
pub fn is_subscription_response(value: &Value) -> bool {
    value
        .get("event")
        .and_then(|v| v.as_str())
        .map(|s| s == "subscribe" || s == "unsubscribe")
        .unwrap_or(false)
}

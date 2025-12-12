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

//! JSON-RPC 2.0 protocol structures shared by HTTP and WebSocket interfaces.

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request envelope.
///
/// Used by both HTTP API calls and WebSocket method invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitJsonRpcRequest<T> {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: &'static str,
    /// Request ID for correlation.
    pub id: u64,
    /// JSON-RPC method name.
    pub method: String,
    /// Method-specific parameters.
    pub params: T,
}

impl<T> DeribitJsonRpcRequest<T> {
    /// Creates a new JSON-RPC request.
    #[must_use]
    pub fn new(id: u64, method: impl Into<String>, params: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response envelope.
///
/// Used by both HTTP API responses and WebSocket method responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitJsonRpcResponse<T> {
    /// JSON-RPC version.
    pub jsonrpc: String,
    /// Request ID (present for request responses).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Success result (mutually exclusive with error).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    /// Error details (mutually exclusive with result).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DeribitJsonRpcError>,
    /// Whether this is from testnet.
    #[serde(default)]
    pub testnet: bool,
    /// Server receive timestamp (microseconds).
    #[serde(rename = "usIn")]
    pub us_in: Option<u64>,
    /// Server send timestamp (microseconds).
    #[serde(rename = "usOut")]
    pub us_out: Option<u64>,
    /// Processing time difference (microseconds).
    #[serde(rename = "usDiff")]
    pub us_diff: Option<u64>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitJsonRpcError {
    /// Error code.
    pub code: i64,
    /// Error message.
    pub message: String,
    /// Additional error data.
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

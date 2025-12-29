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

//! Deribit HTTP client error types.

use std::fmt;

/// Represents HTTP client errors for the Deribit adapter.
#[derive(Debug, Clone)]
pub enum DeribitHttpError {
    /// Missing API credentials
    MissingCredentials,
    /// Deribit-specific error with code and message
    DeribitError { error_code: i64, message: String },
    /// JSON serialization/deserialization error
    JsonError(String),
    /// Input validation error
    ValidationError(String),
    /// Network-related error
    NetworkError(String),
    /// Request timeout
    Timeout(String),
    /// Request canceled
    Canceled(String),
    /// Unexpected HTTP status
    UnexpectedStatus { status: u16, body: String },
}

impl fmt::Display for DeribitHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::DeribitError {
                error_code,
                message,
            } => write!(f, "Deribit error {error_code}: {message}"),
            Self::JsonError(msg) => write!(f, "JSON error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Canceled(msg) => write!(f, "Canceled: {msg}"),
            Self::UnexpectedStatus { status, body } => {
                write!(f, "Unexpected status {status}: {body}")
            }
        }
    }
}

impl std::error::Error for DeribitHttpError {}

impl From<serde_json::Error> for DeribitHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<anyhow::Error> for DeribitHttpError {
    fn from(error: anyhow::Error) -> Self {
        Self::NetworkError(error.to_string())
    }
}

impl DeribitHttpError {
    /// Maps a JSON-RPC error to the appropriate error variant.
    ///
    /// Standard JSON-RPC error codes (-32xxx) are mapped to `ValidationError`,
    /// while Deribit-specific error codes are mapped to `DeribitError`.
    ///
    /// # Arguments
    ///
    /// * `error_code` - The JSON-RPC error code
    /// * `message` - The error message
    /// * `data` - Optional additional error data
    pub fn from_jsonrpc_error(
        error_code: i64,
        message: String,
        data: Option<serde_json::Value>,
    ) -> Self {
        match error_code {
            // JSON-RPC 2.0 standard error codes
            -32700 => Self::ValidationError(format!("Parse error: {message}")),
            -32600 => Self::ValidationError(format!("Invalid request: {message}")),
            -32601 => Self::ValidationError(format!("Method not found: {message}")),
            -32602 => {
                // Try to extract parameter details from data field
                let detail = data
                    .as_ref()
                    .and_then(|d| d.as_object())
                    .and_then(|obj| {
                        let param = obj.get("param")?.as_str()?;
                        let reason = obj.get("reason")?.as_str()?;
                        Some(format!(" (parameter '{param}': {reason})"))
                    })
                    .unwrap_or_default();
                Self::ValidationError(format!("Invalid params: {message}{detail}"))
            }
            -32603 => Self::ValidationError(format!("Internal error: {message}")),
            // All other error codes are Deribit-specific
            _ => Self::DeribitError {
                error_code,
                message,
            },
        }
    }
}

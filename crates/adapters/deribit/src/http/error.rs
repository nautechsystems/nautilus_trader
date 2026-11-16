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

//! Deribit HTTP client error types.

use thiserror::Error;

use crate::common::consts::should_retry_error_code;

/// Represents HTTP client errors for the Deribit adapter.
#[derive(Debug, Clone, Error)]
pub enum DeribitHttpError {
    /// Missing API credentials
    #[error("Missing API credentials")]
    MissingCredentials,
    /// Deribit-specific error with code and message
    #[error("Deribit error {error_code}: {message}")]
    DeribitError { error_code: i64, message: String },
    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Input validation error
    #[error("Validation error: {0}")]
    ValidationError(String),
    /// Network-related error
    #[error("Network error: {0}")]
    NetworkError(String),
    /// Request timeout
    #[error("Timeout: {0}")]
    Timeout(String),
    /// Request canceled
    #[error("Canceled: {0}")]
    Canceled(String),
    /// Unexpected HTTP status
    #[error("Unexpected status {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },
}

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
    /// Returns whether this error is retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::NetworkError(_) => true,
            Self::UnexpectedStatus { status, .. } => *status >= 500 || *status == 429,
            Self::DeribitError { error_code, .. } => should_retry_error_code(*error_code),
            _ => false,
        }
    }

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
        data: Option<&serde_json::Value>,
    ) -> Self {
        match error_code {
            // JSON-RPC 2.0 standard error codes
            -32700 => Self::ValidationError(format!("Parse error: {message}")),
            -32600 => Self::ValidationError(format!("Invalid request: {message}")),
            -32601 => Self::ValidationError(format!("Method not found: {message}")),
            -32602 => {
                // Try to extract parameter details from data field
                let detail = data
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(DeribitHttpError::MissingCredentials, "Missing API credentials")]
    #[case(DeribitHttpError::DeribitError { error_code: 10004, message: "bad".to_string() }, "Deribit error 10004: bad")]
    #[case(DeribitHttpError::JsonError("bad".to_string()), "JSON error: bad")]
    #[case(DeribitHttpError::ValidationError("bad".to_string()), "Validation error: bad")]
    #[case(DeribitHttpError::NetworkError("offline".to_string()), "Network error: offline")]
    #[case(DeribitHttpError::Timeout("slow".to_string()), "Timeout: slow")]
    #[case(DeribitHttpError::Canceled("stop".to_string()), "Canceled: stop")]
    #[case(DeribitHttpError::UnexpectedStatus { status: 418, body: "teapot".to_string() }, "Unexpected status 418: teapot")]
    fn test_display(#[case] error: DeribitHttpError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
        assert!(std::error::Error::source(&error).is_none());
    }

    #[rstest]
    #[case(DeribitHttpError::NetworkError("timeout".to_string()), true)]
    #[case(DeribitHttpError::Timeout("read".to_string()), false)]
    #[case(DeribitHttpError::UnexpectedStatus { status: 500, body: String::new() }, true)]
    #[case(DeribitHttpError::UnexpectedStatus { status: 502, body: String::new() }, true)]
    #[case(DeribitHttpError::UnexpectedStatus { status: 429, body: String::new() }, true)]
    #[case(DeribitHttpError::UnexpectedStatus { status: 403, body: String::new() }, false)]
    #[case(DeribitHttpError::DeribitError { error_code: 10028, message: String::new() }, true)]
    #[case(DeribitHttpError::DeribitError { error_code: 13888, message: String::new() }, true)]
    #[case(DeribitHttpError::DeribitError { error_code: -32600, message: String::new() }, false)]
    #[case(DeribitHttpError::JsonError("bad".to_string()), false)]
    #[case(DeribitHttpError::ValidationError("bad".to_string()), false)]
    #[case(DeribitHttpError::MissingCredentials, false)]
    #[case(DeribitHttpError::Canceled("shutdown".to_string()), false)]
    fn test_is_retryable(#[case] error: DeribitHttpError, #[case] expected: bool) {
        assert_eq!(error.is_retryable(), expected);
    }
}

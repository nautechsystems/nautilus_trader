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

//! Error structures and enumerations for the OKX integration.
//!
//! The JSON error schema is described in the OKX documentation under
//! *REST API > Error Codes* – <https://www.okx.com/docs-v5/en/#error-codes>.
//! The types below mirror that structure and are reused across the entire
//! crate.

use nautilus_network::http::{HttpClientError, StatusCode};
use serde::Deserialize;
use thiserror::Error;

use crate::common::consts::should_retry_error_code;

/// Represents a build error for query parameter validation.
#[derive(Debug, Error)]
pub enum BuildError {
    /// Missing required instrument ID.
    #[error("Missing required instrument ID")]
    MissingInstId,
    /// Missing required bar interval.
    #[error("Missing required bar interval")]
    MissingBar,
    /// Both after and before cursors specified.
    #[error("Cannot specify both 'after' and 'before' cursors")]
    BothCursors,
    /// Invalid time range: after_ms should be greater than before_ms.
    #[error(
        "Invalid time range: after_ms ({after_ms}) must be greater than before_ms ({before_ms})"
    )]
    InvalidTimeRange { after_ms: i64, before_ms: i64 },
    /// Cursor timestamp is in nanoseconds (> 13 digits).
    #[error("Cursor timestamp appears to be in nanoseconds (> 13 digits)")]
    CursorIsNanoseconds,
    /// Limit exceeds maximum allowed value.
    #[error("Limit exceeds maximum of 300")]
    LimitTooHigh,
}

/// Represents the JSON structure of an error response returned by the OKX API.
#[derive(Clone, Debug, Deserialize)]
pub struct OKXErrorResponse {
    /// The top-level error object included in the OKX error response.
    pub error: OKXErrorMessage,
}

/// Contains the specific error details provided by the OKX API.
#[derive(Clone, Debug, Deserialize)]
pub struct OKXErrorMessage {
    /// A human-readable explanation of the error condition.
    pub message: String,
    /// A short identifier or category for the error, as returned by OKX.
    pub name: String,
}

/// A typed error enumeration for the OKX HTTP client.
#[derive(Debug, Error)]
pub enum OKXHttpError {
    /// Error variant when credentials are missing but the request is authenticated.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Errors returned directly by OKX (non-zero code).
    #[error("OKX error {error_code}: {message}")]
    OkxError { error_code: String, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Parameter validation error.
    #[error("Parameter validation error: {0}")]
    ValidationError(String),
    /// Request was canceled, typically due to shutdown or disconnect.
    #[error("Request canceled: {0}")]
    Canceled(String),
    /// Wrapping the underlying HttpClientError from the network crate.
    #[error("Network error: {0}")]
    HttpClientError(#[from] HttpClientError),
    /// Any unknown HTTP status or unexpected response from OKX.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
}

impl From<String> for OKXHttpError {
    fn from(error: String) -> Self {
        Self::ValidationError(error)
    }
}

// Allow use of the `?` operator on `serde_json` results inside the HTTP
// client implementation by converting them into our typed error.
impl From<serde_json::Error> for OKXHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl OKXHttpError {
    /// Returns whether this error is retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::HttpClientError(_) => true,
            Self::UnexpectedStatus { status, .. } => {
                status.as_u16() >= 500 || status.as_u16() == 429
            }
            Self::OkxError { error_code, .. } => should_retry_error_code(error_code),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(OKXHttpError::HttpClientError(HttpClientError::Error("timeout".to_string())), true)]
    #[case(OKXHttpError::UnexpectedStatus { status: StatusCode::INTERNAL_SERVER_ERROR, body: String::new() }, true)]
    #[case(OKXHttpError::UnexpectedStatus { status: StatusCode::TOO_MANY_REQUESTS, body: String::new() }, true)]
    #[case(OKXHttpError::UnexpectedStatus { status: StatusCode::FORBIDDEN, body: String::new() }, false)]
    #[case(OKXHttpError::OkxError { error_code: "50001".to_string(), message: String::new() }, true)]
    #[case(OKXHttpError::OkxError { error_code: "50011".to_string(), message: String::new() }, true)]
    #[case(OKXHttpError::OkxError { error_code: "51000".to_string(), message: String::new() }, false)]
    #[case(OKXHttpError::JsonError("bad".to_string()), false)]
    #[case(OKXHttpError::ValidationError("bad".to_string()), false)]
    #[case(OKXHttpError::MissingCredentials, false)]
    #[case(OKXHttpError::Canceled("shutdown".to_string()), false)]
    fn test_is_retryable(#[case] error: OKXHttpError, #[case] expected: bool) {
        assert_eq!(error.is_retryable(), expected);
    }
}

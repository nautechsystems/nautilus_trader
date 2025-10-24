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

//! Error structures and enumerations for the OKX integration.
//!
//! The JSON error schema is described in the OKX documentation under
//! *REST API > Error Codes* – <https://www.okx.com/docs-v5/en/#error-codes>.
//! The types below mirror that structure and are reused across the entire
//! crate.

use nautilus_network::http::HttpClientError;
use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

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

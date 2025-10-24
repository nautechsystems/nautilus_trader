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

//! Error structures and enumerations for the Bybit integration.
//!
//! The JSON error schema is described in the Bybit documentation under
//! *Error Codes* – <https://bybit-exchange.github.io/docs/v5/error>.
//! The types below mirror that structure and are reused across the entire
//! crate.

use nautilus_network::http::HttpClientError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Build error for query parameter validation.
#[derive(Debug, Clone, Error)]
pub enum BybitBuildError {
    /// Missing required category.
    #[error("Missing required category")]
    MissingCategory,
    /// Missing required symbol.
    #[error("Missing required symbol")]
    MissingSymbol,
    /// Missing required interval.
    #[error("Missing required interval")]
    MissingInterval,
    /// Invalid limit value.
    #[error("Invalid limit: must be between 1 and 1000")]
    InvalidLimit,
    /// Invalid time range: `start` should be less than `end`.
    #[error("Invalid time range: start ({start}) must be less than end ({end})")]
    InvalidTimeRange { start: i64, end: i64 },
    /// Both orderId and orderLinkId specified.
    #[error("Cannot specify both 'orderId' and 'orderLinkId'")]
    BothOrderIds,
    /// Missing required order identifier.
    #[error("Missing required order identifier (orderId or orderLinkId)")]
    MissingOrderId,
}

/// Represents the JSON structure of an error response returned by the Bybit API.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/error>
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitErrorResponse {
    /// Error code returned by Bybit.
    pub ret_code: i32,
    /// A human-readable explanation of the error condition.
    pub ret_msg: String,
    /// Extended error information.
    #[serde(default)]
    pub ret_ext_info: Option<serde_json::Value>,
}

/// A typed error enumeration for the Bybit HTTP client.
#[derive(Debug, Clone, Error)]
pub enum BybitHttpError {
    /// Error variant when credentials are missing but the request is authenticated.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Errors returned directly by Bybit (non-zero code).
    #[error("Bybit error {error_code}: {message}")]
    BybitError { error_code: i32, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Parameter validation error.
    #[error("Parameter validation error: {0}")]
    ValidationError(String),
    /// Build error for query parameters.
    #[error("Build error: {0}")]
    BuildError(#[from] BybitBuildError),
    /// Request was canceled, typically due to shutdown or disconnect.
    #[error("Request canceled: {0}")]
    Canceled(String),
    /// Generic network error (for retries, cancellations, etc).
    #[error("Network error: {0}")]
    NetworkError(String),
    /// Any unknown HTTP status or unexpected response from Bybit.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },
}

impl From<HttpClientError> for BybitHttpError {
    fn from(error: HttpClientError) -> Self {
        Self::NetworkError(error.to_string())
    }
}

impl From<String> for BybitHttpError {
    fn from(error: String) -> Self {
        Self::ValidationError(error)
    }
}

// Allow use of the `?` operator on `serde_json` results inside the HTTP
// client implementation by converting them into our typed error.
impl From<serde_json::Error> for BybitHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<BybitErrorResponse> for BybitHttpError {
    fn from(error: BybitErrorResponse) -> Self {
        Self::BybitError {
            error_code: error.ret_code,
            message: error.ret_msg,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_bybit_build_error_display() {
        let error = BybitBuildError::MissingSymbol;
        assert_eq!(error.to_string(), "Missing required symbol");

        let error = BybitBuildError::InvalidLimit;
        assert_eq!(
            error.to_string(),
            "Invalid limit: must be between 1 and 1000"
        );

        let error = BybitBuildError::InvalidTimeRange {
            start: 100,
            end: 50,
        };
        assert_eq!(
            error.to_string(),
            "Invalid time range: start (100) must be less than end (50)"
        );
    }

    #[rstest]
    fn test_bybit_http_error_from_error_response() {
        let error_response = BybitErrorResponse {
            ret_code: 10001,
            ret_msg: "Parameter error".to_string(),
            ret_ext_info: None,
        };

        let http_error: BybitHttpError = error_response.into();
        assert_eq!(http_error.to_string(), "Bybit error 10001: Parameter error");
    }

    #[rstest]
    fn test_bybit_http_error_from_json_error() {
        let json_err = serde_json::from_str::<BybitErrorResponse>("invalid json").unwrap_err();
        let http_error: BybitHttpError = json_err.into();
        assert!(http_error.to_string().contains("JSON error"));
    }

    #[rstest]
    fn test_bybit_http_error_from_string() {
        let error_msg = "Invalid parameter value".to_string();
        let http_error: BybitHttpError = error_msg.into();
        assert_eq!(
            http_error.to_string(),
            "Parameter validation error: Invalid parameter value"
        );
    }

    #[rstest]
    fn test_unexpected_status_error() {
        let error = BybitHttpError::UnexpectedStatus {
            status: 502,
            body: "Server error".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Unexpected HTTP status code 502: Server error"
        );
    }
}

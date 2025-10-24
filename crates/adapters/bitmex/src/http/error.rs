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

//! Error structures and enumerations for the BitMEX integration.

use nautilus_network::http::HttpClientError;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Build error for query parameter validation.
#[derive(Debug, Clone, Error)]
pub enum BitmexBuildError {
    /// Missing required symbol.
    #[error("Missing required symbol")]
    MissingSymbol,
    /// Invalid count value.
    #[error("Invalid count: must be between 1 and 500")]
    InvalidCount,
    /// Invalid start value.
    #[error("Invalid start: must be non-negative")]
    InvalidStart,
    /// Invalid time range: `start_time` should be less than `end_time`.
    #[error(
        "Invalid time range: start_time ({start_time}) must be less than end_time ({end_time})"
    )]
    InvalidTimeRange { start_time: i64, end_time: i64 },
    /// Both orderID and clOrdID specified.
    #[error("Cannot specify both 'orderID' and 'clOrdID'")]
    BothOrderIds,
    /// Missing required order identifier.
    #[error("Missing required order identifier (orderID or clOrdID)")]
    MissingOrderId,
}

/// Represents the JSON structure of an error response returned by the BitMEX API.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BitmexErrorResponse {
    /// The top-level error object included in the BitMEX error response.
    pub error: BitmexErrorMessage,
}

/// Contains the specific error details provided by the BitMEX API.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BitmexErrorMessage {
    /// A human-readable explanation of the error condition.
    pub message: String,
    /// A short identifier or category for the error, as returned by BitMEX.
    pub name: String,
}

/// A typed error enumeration for the BitMEX HTTP client.
#[derive(Debug, Clone, Error)]
pub enum BitmexHttpError {
    /// Error variant when credentials are missing but the request is authenticated.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Errors returned directly by BitMEX.
    #[error("BitMEX error {error_name}: {message}")]
    BitmexError { error_name: String, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Parameter validation error.
    #[error("Parameter validation error: {0}")]
    ValidationError(String),
    /// Build error for query parameters.
    #[error("Build error: {0}")]
    BuildError(#[from] BitmexBuildError),
    /// Request was canceled, typically due to shutdown or disconnect.
    #[error("Request canceled: {0}")]
    Canceled(String),
    /// Generic network error (for retries, cancellations, etc).
    #[error("Network error: {0}")]
    NetworkError(String),
    /// Any unknown HTTP status or unexpected response from BitMEX.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
}

impl From<HttpClientError> for BitmexHttpError {
    fn from(error: HttpClientError) -> Self {
        Self::NetworkError(error.to_string())
    }
}

impl From<String> for BitmexHttpError {
    fn from(error: String) -> Self {
        Self::ValidationError(error)
    }
}

// Allow use of the `?` operator on `serde_json` results inside the HTTP
// client implementation by converting them into our typed error.
impl From<serde_json::Error> for BitmexHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<BitmexErrorResponse> for BitmexHttpError {
    fn from(error: BitmexErrorResponse) -> Self {
        Self::BitmexError {
            error_name: error.error.name,
            message: error.error.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn test_bitmex_build_error_display() {
        let error = BitmexBuildError::MissingSymbol;
        assert_eq!(error.to_string(), "Missing required symbol");

        let error = BitmexBuildError::InvalidCount;
        assert_eq!(
            error.to_string(),
            "Invalid count: must be between 1 and 500"
        );

        let error = BitmexBuildError::InvalidTimeRange {
            start_time: 100,
            end_time: 50,
        };
        assert_eq!(
            error.to_string(),
            "Invalid time range: start_time (100) must be less than end_time (50)"
        );
    }

    #[rstest]
    fn test_bitmex_error_response_from_json() {
        let json = load_test_json("http_error_response.json");

        let error_response: BitmexErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(error_response.error.message, "Invalid API Key.");
        assert_eq!(error_response.error.name, "HTTPError");
    }

    #[rstest]
    fn test_bitmex_http_error_from_error_response() {
        let error_response = BitmexErrorResponse {
            error: BitmexErrorMessage {
                message: "Rate limit exceeded".to_string(),
                name: "RateLimitError".to_string(),
            },
        };

        let http_error: BitmexHttpError = error_response.into();
        assert_eq!(
            http_error.to_string(),
            "BitMEX error RateLimitError: Rate limit exceeded"
        );
    }

    #[rstest]
    fn test_bitmex_http_error_from_json_error() {
        let json_err = serde_json::from_str::<BitmexErrorResponse>("invalid json").unwrap_err();
        let http_error: BitmexHttpError = json_err.into();
        assert!(http_error.to_string().contains("JSON error"));
    }

    #[rstest]
    fn test_bitmex_http_error_from_string() {
        let error_msg = "Invalid parameter value".to_string();
        let http_error: BitmexHttpError = error_msg.into();
        assert_eq!(
            http_error.to_string(),
            "Parameter validation error: Invalid parameter value"
        );
    }

    #[rstest]
    fn test_unexpected_status_error() {
        let error = BitmexHttpError::UnexpectedStatus {
            status: StatusCode::BAD_GATEWAY,
            body: "Server error".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Unexpected HTTP status code 502 Bad Gateway: Server error"
        );
    }
}

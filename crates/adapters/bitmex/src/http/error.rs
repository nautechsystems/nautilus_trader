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
#[derive(Debug, Error)]
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
#[derive(Debug, Error)]
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
    /// Wrapping the underlying `HttpClientError` from the network crate.
    #[error("Network error: {0}")]
    HttpClientError(#[from] HttpClientError),
    /// Any unknown HTTP status or unexpected response from BitMEX.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
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

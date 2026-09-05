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
//! *REST API > Error Codes* - <https://www.okx.com/docs-v5/en/#error-codes>.
//! The types below mirror that structure and are reused across the entire
//! crate.

use std::time::Duration;

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
    /// Temporary errors returned by OKX.
    #[error("Temporary OKX error {error_code}: {message}")]
    RetryableOkxError {
        error_code: String,
        message: String,
        retry_after: Option<Duration>,
    },
    /// Failure while serializing an outbound request.
    #[error("Request serialization error: {0}")]
    RequestSerialization(String),
    /// The response body is not valid JSON.
    #[error("Malformed response: {0}")]
    MalformedResponse(String),
    /// The response JSON does not match the expected schema.
    #[error("Response decoding error: {0}")]
    ResponseDecoding(String),
    /// Parameter validation error.
    #[error("Parameter validation error: {0}")]
    ValidationError(String),
    /// Request was canceled, typically due to shutdown or disconnect.
    #[error("Request canceled: {0}")]
    Canceled(String),
    /// Wrapping the underlying HttpClientError from the network crate.
    #[error("Network error: {0}")]
    HttpClientError(#[from] HttpClientError),
    /// A temporary HTTP status without a decodable OKX error envelope.
    #[error("Temporary HTTP status code {status}: {body}")]
    RetryableStatus {
        status: StatusCode,
        body: String,
        retry_after: Option<Duration>,
    },
    /// Any permanent unknown HTTP status or unexpected response from OKX.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
    /// A single retry attempt exceeded its configured timeout.
    #[error("Operation timed out after {timeout_ms}ms")]
    OperationTimeout { timeout_ms: u64 },
    /// The retry elapsed-time budget was exhausted.
    #[error("Retry budget exceeded: {0}")]
    RetryBudgetExceeded(String),
    /// The venue returned a successful envelope with no result items.
    #[error("Empty response")]
    EmptyResponse,
}

impl From<String> for OKXHttpError {
    fn from(error: String) -> Self {
        Self::ValidationError(error)
    }
}

// Response decoding is classified explicitly; this conversion handles outbound serialization
impl From<serde_json::Error> for OKXHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::RequestSerialization(error.to_string())
    }
}

impl OKXHttpError {
    pub(crate) fn from_venue_response(
        error_code: String,
        message: String,
        retry_after: Option<Duration>,
    ) -> Self {
        if should_retry_error_code(&error_code) {
            Self::RetryableOkxError {
                error_code,
                message,
                retry_after,
            }
        } else {
            Self::OkxError {
                error_code,
                message,
            }
        }
    }

    /// Returns whether OKX reported that the requested order does not exist.
    #[must_use]
    pub fn is_order_not_found(&self) -> bool {
        matches!(
            self,
            Self::OkxError { error_code, .. } if error_code == "51603"
        )
    }

    /// Returns whether this error is retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::HttpClientError(
                HttpClientError::TransportError(_) | HttpClientError::TimeoutError(_)
            ) | Self::RetryableOkxError { .. }
                | Self::RetryableStatus { .. }
                | Self::OperationTimeout { .. }
        ) || matches!(
            self,
            Self::OkxError { error_code, .. } if should_retry_error_code(error_code)
        )
    }

    /// Returns the venue or transport-provided minimum retry delay.
    #[must_use]
    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RetryableOkxError { retry_after, .. }
            | Self::RetryableStatus { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(OKXHttpError::HttpClientError(HttpClientError::TransportError("reset".to_string())), true)]
    #[case(OKXHttpError::HttpClientError(HttpClientError::Error("invalid header".to_string())), false)]
    #[case(OKXHttpError::RetryableStatus { status: StatusCode::INTERNAL_SERVER_ERROR, body: String::new(), retry_after: None }, true)]
    #[case(OKXHttpError::RetryableStatus { status: StatusCode::TOO_MANY_REQUESTS, body: String::new(), retry_after: None }, true)]
    #[case(OKXHttpError::UnexpectedStatus { status: StatusCode::FORBIDDEN, body: String::new() }, false)]
    #[case(OKXHttpError::RetryableOkxError { error_code: "50001".to_string(), message: String::new(), retry_after: None }, true)]
    #[case(OKXHttpError::RetryableOkxError { error_code: "50011".to_string(), message: String::new(), retry_after: None }, true)]
    #[case(OKXHttpError::OkxError { error_code: "50013".to_string(), message: String::new() }, true)]
    #[case(OKXHttpError::OkxError { error_code: "51000".to_string(), message: String::new() }, false)]
    #[case(OKXHttpError::RequestSerialization("bad".to_string()), false)]
    #[case(OKXHttpError::MalformedResponse("bad".to_string()), false)]
    #[case(OKXHttpError::ResponseDecoding("bad".to_string()), false)]
    #[case(OKXHttpError::ValidationError("bad".to_string()), false)]
    #[case(OKXHttpError::MissingCredentials, false)]
    #[case(OKXHttpError::Canceled("shutdown".to_string()), false)]
    #[case(OKXHttpError::HttpClientError(HttpClientError::InvalidProxy("timeout".to_string())), false)]
    #[case(OKXHttpError::HttpClientError(HttpClientError::ClientBuildError("timeout".to_string())), false)]
    #[case(OKXHttpError::OperationTimeout { timeout_ms: 1_000 }, true)]
    #[case(OKXHttpError::RetryBudgetExceeded("budget".to_string()), false)]
    #[case(OKXHttpError::EmptyResponse, false)]
    fn test_is_retryable(#[case] error: OKXHttpError, #[case] expected: bool) {
        assert_eq!(error.is_retryable(), expected);
    }

    #[rstest]
    fn test_retryability_uses_error_type_not_message() {
        let message = "connection reset".to_string();
        let transport =
            OKXHttpError::HttpClientError(HttpClientError::TransportError(message.clone()));
        let permanent = OKXHttpError::HttpClientError(HttpClientError::Error(message));

        assert!(transport.is_retryable());
        assert!(!permanent.is_retryable());
    }

    #[rstest]
    fn test_from_venue_response_classifies_retryable_code() {
        let delay = Duration::from_secs(2);

        let error = OKXHttpError::from_venue_response(
            "50013".to_string(),
            "System busy".to_string(),
            Some(delay),
        );

        assert!(matches!(
            error,
            OKXHttpError::RetryableOkxError {
                error_code,
                message,
                retry_after: Some(actual_delay),
            } if error_code == "50013" && message == "System busy" && actual_delay == delay
        ));
    }

    #[rstest]
    fn test_retry_after_is_exposed_only_by_retryable_response_errors() {
        let delay = Duration::from_secs(5);
        let rate_limit = OKXHttpError::RetryableOkxError {
            error_code: "50011".to_string(),
            message: "Request too frequent".to_string(),
            retry_after: Some(delay),
        };

        assert_eq!(rate_limit.retry_after(), Some(delay));
        assert_eq!(
            OKXHttpError::MalformedResponse(String::new()).retry_after(),
            None
        );
    }

    #[rstest]
    #[case(OKXHttpError::OkxError {
        error_code: "51603".to_string(),
        message: "Order does not exist".to_string(),
    }, true)]
    #[case(OKXHttpError::OkxError {
        error_code: "51000".to_string(),
        message: "Parameter error".to_string(),
    }, false)]
    #[case(OKXHttpError::ValidationError("bad".to_string()), false)]
    fn test_is_order_not_found(#[case] error: OKXHttpError, #[case] expected: bool) {
        assert_eq!(error.is_order_not_found(), expected);
    }
}

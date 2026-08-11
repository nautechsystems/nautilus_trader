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

//! Error types for Kraken HTTP client operations.

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum KrakenHttpError {
    #[error("Request not started: {0}")]
    RequestNotStarted(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("API error: {}", format_api_errors(.0))]
    ApiError(Vec<String>),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Missing credentials")]
    MissingCredentials,
}

#[derive(Debug, Error)]
pub(crate) enum KrakenSubmitOrderError {
    #[error("Order rejected: {reason}")]
    Rejected { reason: String },

    #[error("No send status in successful response")]
    MissingStatus,

    #[error("Unknown send status: {status}")]
    UnknownStatus { status: String },

    #[error("No order ID in submit response: {detail}")]
    MissingOrderId { detail: String },

    #[error("Order lookup failed after submission: {source}")]
    PostSubmitLookup {
        #[source]
        source: anyhow::Error,
    },
}

#[derive(Debug, Error)]
pub(crate) enum KrakenModifyOrderError {
    #[error("Order modification rejected: {reason}")]
    Rejected { reason: String },

    #[error("Unknown edit status: {status}")]
    UnknownStatus { status: String },

    #[error("No order ID in edit response")]
    MissingOrderId,
}

#[derive(Debug, Error)]
pub(crate) enum KrakenBatchOrderError {
    #[error("Order validation failed: {reason}")]
    Validation { reason: String },

    #[error("Order not sent after an earlier chunk failed")]
    NotAttempted,

    #[error("Batch response item count {actual} did not match request count {expected}")]
    ResponseCount { expected: usize, actual: usize },

    #[error("Missing batch response for {key}")]
    MissingResponse { key: String },

    #[error("Duplicate batch responses for {key}")]
    DuplicateResponse { key: String },
}

/// Formats API error messages, handling empty error arrays.
fn format_api_errors(errors: &[String]) -> String {
    if errors.is_empty() {
        "unknown error (empty error list)".to_string()
    } else {
        errors.join(", ")
    }
}

impl From<anyhow::Error> for KrakenHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

/// Returns `true` if a request producing this error should be retried.
pub fn kraken_http_should_retry(error: &KrakenHttpError) -> bool {
    match error {
        KrakenHttpError::NetworkError(_) => true,
        KrakenHttpError::ApiError(errors) => errors.iter().any(|e| e.contains("Rate limit")),
        KrakenHttpError::RequestNotStarted(_)
        | KrakenHttpError::ParseError(_)
        | KrakenHttpError::AuthenticationError(_)
        | KrakenHttpError::MissingCredentials => false,
    }
}

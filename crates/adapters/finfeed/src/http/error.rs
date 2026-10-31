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

//! Adapter-owned failure and retry classification.

use std::time::Duration;

use nautilus_model::identifiers::InstrumentId;
use nautilus_network::{http::HttpClientError, retry::RetryError};

/// A failure in prediction-market requests or instrument definitions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid request selectors, credentials, identity, or local controls.
    #[error("Invalid FinFeed request: {0}")]
    Validation(String),
    /// Missing, unsupported, or conflicting instrument metadata.
    #[error("Invalid FinFeed definition {exchange_id}/{market_id}: {field} {reason}")]
    Definition {
        /// Provider exchange identifier.
        exchange_id: String,
        /// Provider outcome identifier.
        market_id: String,
        /// Metadata field that failed validation.
        field: &'static str,
        /// Static description of the definition failure.
        reason: &'static str,
    },
    /// Requested instrument is unavailable from the provider.
    #[error("FinFeed instrument {0} is unavailable")]
    Unavailable(InstrumentId),
    /// Unsuccessful HTTP response, retaining status and retry guidance.
    #[error("FinFeed HTTP status {status}")]
    Http {
        /// HTTP response status code.
        status: u16,
        /// Integer `Retry-After` delay in seconds, when supplied.
        retry_after_secs: Option<u64>,
    },
    /// Transport failure with provider or credential-bearing text omitted.
    #[error("FinFeed transport failed")]
    Transport,
    /// HTTP request exceeded its transport timeout.
    #[error("FinFeed transport timed out")]
    Timeout,
    /// HTTP transport or proxy configuration failed.
    #[error("FinFeed transport configuration failed")]
    Configuration,
    /// Response JSON could not be decoded; retains location without response content.
    #[error("FinFeed response parsing failed at line {line}, column {column}")]
    Parse {
        /// One-based line containing the JSON decoding failure.
        line: usize,
        /// Column containing the JSON decoding failure.
        column: usize,
    },
    /// Response violates the expected shape or traversal bounds.
    #[error("Unexpected FinFeed response: {0}")]
    Response(&'static str),
    /// Local managed attempt allowance is exhausted.
    #[error("FinFeed local attempt budget exhausted")]
    AttemptBudget,
    /// Retry operation failed or exceeded its deadline.
    #[error("FinFeed retry control: {0}")]
    Retry(RetryError),
}

impl Error {
    /// Returns whether a read attempt may retry this failure.
    ///
    /// Retryable failures are transport errors, transport or operation timeouts, and HTTP
    /// 429, 500, 502, 503, or 504 responses.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport | Self::Timeout | Self::Retry(RetryError::OperationTimeout { .. })
        ) || matches!(
            self,
            Self::Http {
                status: 429 | 500 | 502 | 503 | 504,
                ..
            }
        )
    }

    pub(super) fn retry_delay(&self) -> Option<Duration> {
        match self {
            Self::Http {
                retry_after_secs, ..
            } => retry_after_secs.map(Duration::from_secs),
            _ => None,
        }
    }
}

impl From<HttpClientError> for Error {
    fn from(e: HttpClientError) -> Self {
        match e {
            HttpClientError::TransportError(_) => Self::Transport,
            HttpClientError::TimeoutError(_) => Self::Timeout,
            _ => Self::Configuration,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Parse {
            line: e.line(),
            column: e.column(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn error_classification_preserves_retry_safety_and_redacts_transport_text() {
        for status in [
            200, 302, 400, 401, 403, 404, 408, 429, 500, 501, 502, 503, 504,
        ] {
            let e = Error::Http {
                status,
                retry_after_secs: None,
            };

            assert_eq!(
                e.is_retryable(),
                matches!(status, 429 | 500 | 502 | 503 | 504)
            );
        }

        for (error, retryable) in [
            (HttpClientError::TransportError("private-key".into()), true),
            (HttpClientError::TimeoutError("private-key".into()), true),
            (
                HttpClientError::ClientBuildError("private-key".into()),
                false,
            ),
            (HttpClientError::InvalidProxy("private-key".into()), false),
            (HttpClientError::Error("private-key".into()), false),
        ] {
            let e = Error::from(error);
            assert_eq!(e.is_retryable(), retryable);
            assert!(!format!("{e:?} {e}").contains("private-key"));
        }
    }
}

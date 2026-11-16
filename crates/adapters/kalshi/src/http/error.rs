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

//! HTTP error taxonomy for Kalshi REST responses.

use std::fmt::Debug;

use nautilus_common::config::ConfigError;
use nautilus_core::string::secret::SecretString;
use nautilus_network::{http::HttpClientError, retry::RetryError};
use thiserror::Error;

use super::models::{Field, VenueError};

/// Errors emitted by the Kalshi HTTP client.
#[derive(Debug, Error)]
pub enum KalshiHttpError {
    /// Missing credentials, invalid signing input, or signature-generation failure.
    #[error("authentication failed: {0}")]
    Authentication(#[from] ConfigError),
    /// Non-success response without potentially sensitive body content.
    #[error("HTTP {0}")]
    Status(u16),
    /// Network-level failure.
    #[error("network error: {0}")]
    Network(String),
    /// HTTP failure retaining a redacted raw body and selected response headers.
    #[error("HTTP {status}")]
    Http {
        status: u16,
        evidence: Box<ResponseEvidence>,
    },
    /// Structured venue failure with its original HTTP status and raw evidence.
    #[error("Kalshi venue failure, HTTP {status}")]
    Venue {
        status: u16,
        evidence: Box<ResponseEvidence>,
    },
    /// Failed to encode a request before transmission.
    #[error("request encoding failed")]
    Encoding,
    /// Invalid request rejected before transmission.
    #[error("invalid request: {0}")]
    Validation(&'static str),
    /// Transport failure whose request may have reached the venue.
    #[error("HTTP transport failed")]
    Transport(#[source] HttpClientError),
    /// Request cancellation; dispatch may already have begun when `dispatched` is true.
    #[error("request canceled, dispatch started: {dispatched}")]
    Canceled { dispatched: bool },
    /// Malformed success payload; the status and raw acknowledgement remain available.
    #[error("response decoding failed, HTTP {status}, line {line}, column {column}")]
    Decode {
        status: u16,
        line: usize,
        column: usize,
        evidence: Box<ResponseEvidence>,
    },
    /// A response whose item count or identities do not match the request.
    ///
    /// The operation may have succeeded; callers reconcile using the retained response evidence.
    #[error("unexpected response, HTTP {status}: {reason}")]
    Response {
        status: u16,
        reason: &'static str,
        evidence: Box<ResponseEvidence>,
    },
    /// A pagination source failed or did not make progress.
    #[error("pagination failed: {0}")]
    Pagination(&'static str),
    /// A later local failure retains possible transmission from an earlier attempt.
    #[error("earlier attempt may have reached Kalshi: {0}")]
    AfterSend(Box<Self>),
    /// Discovery failed before the requested operation could be transmitted.
    #[error("quota discovery failed: {0}")]
    Discovery(Box<Self>),
    /// Local token admission failed before transmission.
    #[error("quota admission failed: {0}")]
    Quota(&'static str),
    /// Retry termination retaining monotonic possible-transmission evidence.
    #[error("retry terminated: {reason}, dispatch started: {dispatched}")]
    Retry {
        reason: RetryError,
        dispatched: bool,
    },
    /// Failed domain parsing after a successful wire decode.
    #[error("parse error: {0}")]
    Parse(String),
}

/// Response evidence with redacted diagnostics and explicit access to private data.
pub struct ResponseEvidence {
    /// Raw response body, potentially containing private data and redacted in debug output.
    pub body: SecretString,
    /// Structured venue error decoded from the response body, when available.
    pub venue: Option<VenueError>,
    /// Value of the `Retry-After` response header, when present.
    pub retry_after: Option<String>,
    /// Value of the `X-Request-ID` response header, when present.
    pub request_id: Option<String>,
}

impl Debug for ResponseEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ResponseEvidence))
            .finish_non_exhaustive()
    }
}

impl KalshiHttpError {
    /// Classifies transient failures independently of operation safety and command outcomes.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::AfterSend(source) => source.is_retryable(),
            Self::Status(500 | 502..=504) => true,
            Self::Transport(
                HttpClientError::TransportError(_) | HttpClientError::TimeoutError(_),
            ) => true,
            Self::Http {
                status: 500 | 502..=504,
                ..
            }
            | Self::Venue {
                status: 500 | 502..=504,
                ..
            } => true,
            Self::Http {
                status: 429,
                evidence,
            }
            | Self::Venue {
                status: 429,
                evidence,
            } => {
                evidence.venue.as_ref().is_some_and(
                    |e| matches!(&e.code, Field::Value(code) if code == "too_many_requests"),
                ) || serde_json::from_str::<serde_json::Value>(evidence.body.expose_secret())
                    .ok()
                    .and_then(|body| {
                        body.get("error")
                            .and_then(serde_json::Value::as_str)
                            .map(|message| message == "too many requests")
                    })
                    .unwrap_or(false)
            }
            Self::Retry {
                reason: RetryError::OperationTimeout { .. },
                ..
            } => true,
            _ => false,
        }
    }

    pub(super) fn is_subaccount_restricted(&self) -> bool {
        const MESSAGE: &str =
            "this API key is restricted to a single sub-account and cannot access this endpoint";

        let evidence = match self {
            Self::Http {
                status: 403,
                evidence,
            }
            | Self::Venue {
                status: 403,
                evidence,
            } => evidence,
            _ => return false,
        };

        if evidence
            .venue
            .as_ref()
            .is_some_and(|e| matches!(&e.message, Field::Value(message) if message == MESSAGE))
        {
            return true;
        }

        let body = evidence.body.expose_secret().trim();
        body == MESSAGE
            || serde_json::from_str::<serde_json::Value>(body).is_ok_and(|value| {
                value.get("error").and_then(serde_json::Value::as_str) == Some(MESSAGE)
            })
    }

    /// Returns whether local evidence proves no attempt reached the send operation.
    ///
    /// Execution callers can map true to `CommandFailure::NotSent`. False preserves an unknown
    /// outcome, including duplicate identities, malformed successes, and transport failures.
    #[must_use]
    pub fn is_definitely_not_sent(&self) -> bool {
        matches!(
            self,
            Self::Discovery(_)
                | Self::Authentication(_)
                | Self::Encoding
                | Self::Validation(_)
                | Self::Quota(_)
                | Self::Canceled { dispatched: false }
                | Self::Retry {
                    dispatched: false,
                    ..
                }
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(403, true)]
    #[case(401, false)]
    #[case(500, false)]
    fn restricted_denial_requires_exact_status_and_message(
        #[case] status: u16,
        #[case] expected: bool,
    ) {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/quota_responses.json")).unwrap();
        let body = serde_json::to_string(&fixtures["subaccount_restricted"]).unwrap();

        let error = KalshiHttpError::Http {
            status,
            evidence: Box::new(ResponseEvidence {
                body: body.into(),
                venue: None,
                retry_after: None,
                request_id: None,
            }),
        };

        assert_eq!(error.is_subaccount_restricted(), expected);

        let unrelated = KalshiHttpError::Http {
            status,
            evidence: Box::new(ResponseEvidence {
                body: serde_json::to_string(&fixtures["malformed_success"])
                    .unwrap()
                    .into(),
                venue: None,
                retry_after: None,
                request_id: None,
            }),
        };

        assert!(!unrelated.is_subaccount_restricted());
    }
}

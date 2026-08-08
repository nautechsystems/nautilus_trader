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

//! HTTP error types for the Polymarket adapter.

use std::time::Duration;

use nautilus_network::http::{HttpClientError, ReqwestError, StatusCode};
use thiserror::Error;

/// Error type for Polymarket HTTP operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("auth error: {0}")]
    Auth(String),

    #[error(
        "Rate limited on {endpoint} (token_cost={token_cost}) retry_after_ms={retry_after_ms:?}"
    )]
    RateLimit {
        endpoint: &'static str,
        token_cost: u32,
        retry_after_ms: Option<u64>,
    },

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error(
        "bad request: {endpoint} token cost {token_cost} exceeds {tier} tier {bucket} burst {burst}"
    )]
    BurstExceeded {
        endpoint: &'static str,
        token_cost: u32,
        tier: String,
        bucket: String,
        burst: u32,
    },

    #[error("exchange error: {0}")]
    Exchange(String),

    #[error("timeout")]
    Timeout,

    #[error("decode error: {0}")]
    Decode(String),

    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    pub fn rate_limit(
        endpoint: &'static str,
        token_cost: u32,
        retry_after_ms: Option<u64>,
    ) -> Self {
        Self::RateLimit {
            endpoint,
            token_cost,
            retry_after_ms,
        }
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn exchange(msg: impl Into<String>) -> Self {
        Self::Exchange(msg.into())
    }

    pub fn decode(msg: impl Into<String>) -> Self {
        Self::Decode(msg.into())
    }

    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Self::Http {
            status,
            message: message.into(),
        }
    }

    /// Classifies an HTTP status code and body into the appropriate error variant.
    pub fn from_http_status(status: StatusCode, body: &[u8]) -> Self {
        Self::from_status_code(status.as_u16(), body)
    }

    /// Classifies a raw status code (as `u16`) and body into the appropriate error variant.
    pub fn from_status_code(status: u16, body: &[u8]) -> Self {
        let message = venue_error_message(body);
        match status {
            401 | 403 => Self::auth(format!("HTTP {status}: {message}")),
            400 => Self::bad_request(format!("HTTP {status}: {message}")),
            429 => Self::rate_limit("unknown", 0, None),
            _ => Self::http(status, message),
        }
    }

    /// Classifies a reqwest error into the appropriate error variant.
    #[expect(clippy::needless_pass_by_value)]
    pub fn from_reqwest(error: ReqwestError) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if let Some(status) = error.status() {
            let status_code = status.as_u16();
            match status_code {
                401 | 403 => Self::auth(format!("HTTP {status_code}: authentication failed")),
                400 => Self::bad_request(format!("HTTP {status_code}: bad request")),
                429 => Self::rate_limit("unknown", 0, None),
                _ => Self::http(status_code, format!("HTTP error: {error}")),
            }
        } else if error.is_connect() || error.is_request() {
            Self::transport(format!("Request error: {error}"))
        } else {
            Self::transport(format!("Unknown reqwest error: {error}"))
        }
    }

    #[expect(clippy::needless_pass_by_value)]
    pub fn from_http_client(error: HttpClientError) -> Self {
        Self::transport(format!("HTTP client error: {error}"))
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout => true,
            Self::RateLimit {
                token_cost,
                retry_after_ms,
                ..
            } => retry_after_ms.is_some() || *token_cost == 0,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// Returns `true` when a submit POST may have reached the venue but the
    /// adapter cannot prove whether the venue accepted or rejected the order.
    pub fn is_submit_outcome_unknown(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout | Self::Serde(_) | Self::Decode(_) | Self::Io(_) => {
                true
            }
            Self::Http { status, .. } => *status >= 500,
            Self::Auth(_)
            | Self::RateLimit { .. }
            | Self::BadRequest(_)
            | Self::BurstExceeded { .. }
            | Self::Exchange(_)
            | Self::UrlParse(_) => false,
        }
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimit {
                retry_after_ms: Some(retry_after_ms),
                ..
            } => Some(Duration::from_millis(*retry_after_ms)),
            _ => None,
        }
    }

    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::Auth(_))
    }

    /// Returns `true` if this error originated from an HTTP status code response
    /// (as opposed to transport, timeout, or local errors).
    pub fn is_http_status_error(&self) -> bool {
        matches!(
            self,
            Self::Auth(_) | Self::BadRequest(_) | Self::RateLimit { .. } | Self::Http { .. }
        )
    }
}

// The CLOB reports a rejected request as `{"error": "..."}` and some endpoints use `errorMsg`,
// alongside fields such as `orderID` that must not reach an `OrderRejected` reason.
fn venue_error_message(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            // Each key must clear the emptiness check on its own, so a blank `error` still falls
            // through to a populated `errorMsg` instead of discarding both for the raw body
            ["error", "errorMsg"].iter().find_map(|key| {
                let message = value.get(key)?.as_str()?;
                (!message.trim().is_empty()).then(|| message.to_string())
            })
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).to_string())
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_error_constructors() {
        let transport_err = Error::transport("Connection failed");
        assert!(matches!(transport_err, Error::Transport(_)));
        assert_eq!(
            transport_err.to_string(),
            "transport error: Connection failed"
        );

        let auth_err = Error::auth("Invalid signature");
        assert!(auth_err.is_auth_error());

        let rate_limit_err = Error::rate_limit("test", 30, Some(30000));
        assert!(rate_limit_err.is_rate_limited());
        assert!(rate_limit_err.is_retryable());
        assert_eq!(rate_limit_err.retry_after(), Some(Duration::from_secs(30)));

        let http_err = Error::http(500, "Internal server error");
        assert!(http_err.is_retryable());
        assert_eq!(http_err.retry_after(), None);
    }

    #[rstest]
    fn test_error_display() {
        let err = Error::RateLimit {
            endpoint: "/orders",
            token_cost: 10,
            retry_after_ms: Some(60000),
        };
        assert_eq!(
            err.to_string(),
            "Rate limited on /orders (token_cost=10) retry_after_ms=Some(60000)"
        );
    }

    #[rstest]
    fn test_retryable_errors() {
        assert!(Error::transport("test").is_retryable());
        assert!(Error::Timeout.is_retryable());
        assert!(Error::rate_limit("/orders", 10, Some(1_000)).is_retryable());
        assert!(Error::rate_limit("unknown", 0, None).is_retryable());
        assert!(Error::http(500, "server error").is_retryable());

        assert!(!Error::rate_limit("/orders", 10, None).is_retryable());
        assert!(!Error::auth("test").is_retryable());
        assert!(!Error::bad_request("test").is_retryable());
        assert!(
            !Error::BurstExceeded {
                endpoint: "/orders",
                token_cost: 121,
                tier: "Standard".to_string(),
                bucket: "cancel".to_string(),
                burst: 120,
            }
            .is_retryable()
        );
        assert!(!Error::decode("test").is_retryable());
    }

    // The CLOB returns rejections as a structured body, so the message must carry the venue text
    // alone. A killed FOK is the canonical case: its body also carries an `orderID` that would
    // otherwise reach the strategy inside `OrderRejected.reason`.
    #[rstest]
    #[case::fok_killed(
        400,
        br#"{"error":"order couldn't be fully filled. FOK orders are fully filled or killed.","orderID":"0x3776d59db9ea1e4bbedf33f6f79ca677cfa6c93c2a44801f5a10516d822cc502"}"#,
        "bad request: HTTP 400: order couldn't be fully filled. FOK orders are fully filled or killed."
    )]
    #[case::error_msg_key(
        400,
        br#"{"errorMsg":"not enough balance / allowance: the balance is not enough"}"#,
        "bad request: HTTP 400: not enough balance / allowance: the balance is not enough"
    )]
    #[case::auth(
        401,
        br#"{"error":"invalid api key"}"#,
        "auth error: HTTP 401: invalid api key"
    )]
    #[case::server_error(
        500,
        br#"{"error":"internal error"}"#,
        "HTTP error 500: internal error"
    )]
    #[case::plain_text_body(400, b"Bad Request", "bad request: HTTP 400: Bad Request")]
    #[case::json_without_error_key(
        400,
        br#"{"foo":"bar"}"#,
        r#"bad request: HTTP 400: {"foo":"bar"}"#
    )]
    #[case::empty_error_value(400, br#"{"error":""}"#, r#"bad request: HTTP 400: {"error":""}"#)]
    #[case::whitespace_error_value(
        400,
        br#"{"error":"   "}"#,
        r#"bad request: HTTP 400: {"error":"   "}"#
    )]
    // A blank `error` must fall through too, otherwise the populated `errorMsg` beside it is
    // discarded and the whole raw body reaches the reason.
    #[case::blank_error_falls_through(
        400,
        br#"{"error":"","errorMsg":"not enough balance / allowance"}"#,
        "bad request: HTTP 400: not enough balance / allowance"
    )]
    // A null `error` must fall through to `errorMsg` rather than ending the lookup: the CLOB sends
    // null for the unused key (see `test_data/http_order_response_ok.json`).
    #[case::null_error_falls_through(
        400,
        br#"{"error":null,"errorMsg":"invalid post-only order: order crosses book"}"#,
        "bad request: HTTP 400: invalid post-only order: order crosses book"
    )]
    // 429 carries its own endpoint-and-cost message, so the body is deliberately dropped
    #[case::rate_limited_ignores_body(
        429,
        br#"{"error":"slow down"}"#,
        "Rate limited on unknown (token_cost=0) retry_after_ms=None"
    )]
    #[case::empty_body(400, b"", "bad request: HTTP 400: ")]
    fn test_from_status_code_message(
        #[case] status: u16,
        #[case] body: &[u8],
        #[case] expected: &str,
    ) {
        assert_eq!(Error::from_status_code(status, body).to_string(), expected);
    }

    // Both entry points classify identically, so a caller cannot get the raw body through one and
    // the venue text through the other.
    #[rstest]
    fn test_from_http_status_matches_from_status_code() {
        let body = br#"{"error":"order couldn't be fully filled. FOK orders are fully filled or killed."}"#;

        let from_status = Error::from_http_status(StatusCode::BAD_REQUEST, body);
        let from_code = Error::from_status_code(400, body);

        assert_eq!(from_status.to_string(), from_code.to_string());
        assert_eq!(
            from_status.to_string(),
            "bad request: HTTP 400: order couldn't be fully filled. FOK orders are fully filled or killed."
        );
    }

    #[rstest]
    fn test_submit_outcome_unknown_errors() {
        assert!(Error::transport("test").is_submit_outcome_unknown());
        assert!(Error::Timeout.is_submit_outcome_unknown());
        assert!(Error::http(500, "server error").is_submit_outcome_unknown());
        assert!(Error::decode("bad json").is_submit_outcome_unknown());

        assert!(!Error::rate_limit("/orders", 10, Some(1_000)).is_submit_outcome_unknown());
        assert!(!Error::auth("test").is_submit_outcome_unknown());
        assert!(!Error::bad_request("test").is_submit_outcome_unknown());
        assert!(!Error::http(404, "not found").is_submit_outcome_unknown());
    }
}

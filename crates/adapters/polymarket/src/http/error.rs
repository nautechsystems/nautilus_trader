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

const ORDER_VERSION_MISMATCH: &str = "order_version_mismatch";
const ORDER_VERSION_MISMATCH_REASON: &str =
    "Polymarket CLOB order version mismatch; adapter supports V2 only";

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
        "HTTP 429 rate limit on {endpoint} (token_cost={token_cost}) retry_after_ms={retry_after_ms:?}: {message}"
    )]
    RateLimit {
        endpoint: &'static str,
        token_cost: u32,
        retry_after_ms: Option<u64>,
        message: String,
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
        Self::rate_limit_response(endpoint, token_cost, retry_after_ms, "rate limit exceeded")
    }

    pub fn rate_limit_response(
        endpoint: &'static str,
        token_cost: u32,
        retry_after_ms: Option<u64>,
        message: impl Into<String>,
    ) -> Self {
        Self::RateLimit {
            endpoint,
            token_cost,
            retry_after_ms,
            message: message.into(),
        }
    }

    pub fn rate_limit_from_body(
        endpoint: &'static str,
        token_cost: u32,
        retry_after_ms: Option<u64>,
        body: &[u8],
    ) -> Self {
        Self::rate_limit_response(
            endpoint,
            token_cost,
            retry_after_ms,
            venue_error_message(body),
        )
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
            429 => Self::rate_limit_response("unknown", 0, None, message),
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
                429 => Self::rate_limit("unknown", 0, None),
                _ => Self::http(status_code, format!("HTTP error: {error}")),
            }
        } else if error.is_connect() || error.is_request() {
            Self::transport(format!("Request error: {error}"))
        } else {
            Self::transport(format!("Unknown reqwest error: {error}"))
        }
    }

    pub fn from_http_client(error: HttpClientError) -> Self {
        match error {
            HttpClientError::TimeoutError(_) => Self::Timeout,
            error => Self::transport(format!("HTTP client error: {error}")),
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout => true,
            Self::RateLimit { .. } => true,
            Self::Http { status, .. } => *status == 425 || *status >= 500,
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
        matches!(
            self,
            Self::Auth(_)
                | Self::Http {
                    status: 401 | 403,
                    ..
                }
        )
    }

    /// Returns `true` if this error originated from an HTTP status code response
    /// (as opposed to transport, timeout, or local errors).
    pub fn is_http_status_error(&self) -> bool {
        matches!(
            self,
            Self::Auth(_) | Self::BadRequest(_) | Self::RateLimit { .. } | Self::Http { .. }
        )
    }

    /// Returns a bounded reason suitable for strategy-facing rejection events.
    #[must_use]
    pub fn strategy_reason(&self) -> String {
        match self {
            Self::Http { message, .. }
            | Self::RateLimit { message, .. }
            | Self::Exchange(message) => strategy_rejection_reason(message),
            _ => strategy_rejection_reason(&self.to_string()),
        }
    }
}

/// Returns a bounded, actionable reason for a strategy-facing rejection event.
#[must_use]
pub(crate) fn strategy_rejection_reason(reason: &str) -> String {
    let reason = sanitize_error_text(reason);

    if reason == ORDER_VERSION_MISMATCH {
        ORDER_VERSION_MISMATCH_REASON.to_string()
    } else {
        reason
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
                (!message.trim().is_empty()).then(|| sanitize_error_text(message))
            })
        })
        .unwrap_or_else(|| sanitize_error_text(&String::from_utf8_lossy(body)))
}

const ERROR_TEXT_MAX_CHARS: usize = 512;
const ERROR_TEXT_TRUNCATED_SUFFIX: &str = " ... [truncated]";

/// Removes unsafe formatting and bounds venue text before logging or emitting it.
#[must_use]
pub(crate) fn sanitize_error_text(text: &str) -> String {
    let visible = if looks_like_html(text) {
        html_title(text).unwrap_or_else(|| strip_html_tags(text))
    } else {
        text.to_string()
    };
    let normalized = normalize_error_text(&visible);
    let normalized = if normalized.is_empty() {
        "empty response body".to_string()
    } else {
        normalized
    };

    if normalized.chars().count() <= ERROR_TEXT_MAX_CHARS {
        return normalized;
    }

    let suffix_chars = ERROR_TEXT_TRUNCATED_SUFFIX.chars().count();
    let mut bounded: String = normalized
        .chars()
        .take(ERROR_TEXT_MAX_CHARS - suffix_chars)
        .collect();
    bounded.push_str(ERROR_TEXT_TRUNCATED_SUFFIX);
    bounded
}

fn looks_like_html(text: &str) -> bool {
    let trimmed = text.trim_start().to_ascii_lowercase();

    trimmed.starts_with("<!doctype html")
        || trimmed.starts_with("<html")
        || trimmed.contains("<html")
}

fn html_title(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let title_start = lower.find("<title")?;
    let content_start = title_start + lower[title_start..].find('>')? + 1;
    let content_end = content_start + lower[content_start..].find("</title>")?;
    let title = text[content_start..content_end].to_string();

    (!title.trim().is_empty()).then_some(title)
}

fn strip_html_tags(text: &str) -> String {
    let mut visible = String::with_capacity(text.len());
    let mut in_tag = false;

    for ch in text.chars() {
        match ch {
            '<' => {
                in_tag = true;
                visible.push(' ');
            }
            '>' => {
                in_tag = false;
                visible.push(' ');
            }
            _ if !in_tag => visible.push(ch),
            _ => {}
        }
    }

    visible
}

fn normalize_error_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_space = true;

    for ch in text.chars() {
        if ch.is_whitespace() || ch.is_control() {
            if !previous_space {
                normalized.push(' ');
                previous_space = true;
            }
        } else {
            normalized.push(ch);
            previous_space = false;
        }
    }

    if normalized.ends_with(' ') {
        normalized.pop();
    }

    normalized
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
            message: "slow down".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "HTTP 429 rate limit on /orders (token_cost=10) retry_after_ms=Some(60000): slow down"
        );
    }

    #[rstest]
    fn test_retryable_errors() {
        assert!(Error::transport("test").is_retryable());
        assert!(Error::Timeout.is_retryable());
        assert!(Error::rate_limit("/orders", 10, Some(1_000)).is_retryable());
        assert!(Error::rate_limit("unknown", 0, None).is_retryable());
        assert!(Error::http(425, "matching engine restarting").is_retryable());
        assert!(Error::http(500, "server error").is_retryable());

        assert!(Error::rate_limit("/orders", 10, None).is_retryable());
        assert!(!Error::auth("test").is_retryable());
        assert!(!Error::bad_request("test").is_retryable());
        assert!(!Error::http(404, "not found").is_retryable());
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
        "HTTP error 400: order couldn't be fully filled. FOK orders are fully filled or killed."
    )]
    #[case::error_msg_key(
        400,
        br#"{"errorMsg":"not enough balance / allowance: the balance is not enough"}"#,
        "HTTP error 400: not enough balance / allowance: the balance is not enough"
    )]
    #[case::auth(
        401,
        br#"{"error":"invalid api key"}"#,
        "HTTP error 401: invalid api key"
    )]
    #[case::server_error(
        500,
        br#"{"error":"internal error"}"#,
        "HTTP error 500: internal error"
    )]
    #[case::plain_text_body(400, b"Bad Request", "HTTP error 400: Bad Request")]
    #[case::json_without_error_key(400, br#"{"foo":"bar"}"#, r#"HTTP error 400: {"foo":"bar"}"#)]
    #[case::empty_error_value(400, br#"{"error":""}"#, r#"HTTP error 400: {"error":""}"#)]
    #[case::whitespace_error_value(400, br#"{"error":"   "}"#, r#"HTTP error 400: {"error":" "}"#)]
    // A blank `error` must fall through too, otherwise the populated `errorMsg` beside it is
    // discarded and the whole raw body reaches the reason.
    #[case::blank_error_falls_through(
        400,
        br#"{"error":"","errorMsg":"not enough balance / allowance"}"#,
        "HTTP error 400: not enough balance / allowance"
    )]
    // A null `error` must fall through to `errorMsg` rather than ending the lookup: the CLOB sends
    // null for the unused key (see `test_data/http_order_response_ok.json`).
    #[case::null_error_falls_through(
        400,
        br#"{"error":null,"errorMsg":"invalid post-only order: order crosses book"}"#,
        "HTTP error 400: invalid post-only order: order crosses book"
    )]
    #[case::rate_limited_preserves_body(
        429,
        br#"{"error":"slow down"}"#,
        "HTTP 429 rate limit on unknown (token_cost=0) retry_after_ms=None: slow down"
    )]
    #[case::empty_body(400, b"", "HTTP error 400: empty response body")]
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
            "HTTP error 400: order couldn't be fully filled. FOK orders are fully filled or killed."
        );
    }

    #[rstest]
    #[case::bad_request(400, false, false, false)]
    #[case::unauthorized(401, false, false, true)]
    #[case::forbidden(403, false, false, true)]
    #[case::not_found(404, false, false, false)]
    #[case::too_early(425, true, false, false)]
    #[case::rate_limited(429, true, false, false)]
    #[case::server_error(500, true, true, false)]
    #[case::service_unavailable(503, true, true, false)]
    fn test_http_status_classification(
        #[case] status: u16,
        #[case] retryable: bool,
        #[case] outcome_unknown: bool,
        #[case] auth: bool,
    ) {
        let error = Error::from_status_code(status, br#"{"error":"venue message"}"#);

        assert_eq!(error.is_retryable(), retryable);
        assert_eq!(error.is_submit_outcome_unknown(), outcome_unknown);
        assert_eq!(error.is_auth_error(), auth);
        assert!(error.is_http_status_error());
        assert_eq!(error.strategy_reason(), "venue message");
    }

    #[rstest]
    fn test_order_version_mismatch_strategy_reason_is_actionable() {
        let error = Error::from_status_code(400, br#"{"error":"order_version_mismatch"}"#);

        assert!(!error.is_submit_outcome_unknown());
        assert_eq!(
            error.strategy_reason(),
            "Polymarket CLOB order version mismatch; adapter supports V2 only"
        );
        assert_eq!(
            strategy_rejection_reason(" order_version_mismatch "),
            "Polymarket CLOB order version mismatch; adapter supports V2 only"
        );
        assert_eq!(
            strategy_rejection_reason("ORDER_VERSION_MISMATCH"),
            "ORDER_VERSION_MISMATCH"
        );
    }

    #[rstest]
    #[case::empty(b"", "empty response body")]
    #[case::plain_text(b"  Bad\r\nRequest  ", "Bad Request")]
    #[case::malformed_json(br#"{"error":"broken"#, r#"{"error":"broken"#)]
    #[case::structured_error(br#"{"error":"  insufficient\n balance  "}"#, "insufficient balance")]
    #[case::structured_error_msg(br#"{"errorMsg":"placement failed"}"#, "placement failed")]
    #[case::unknown_json(
        br#"{"message":"not a verified key"}"#,
        r#"{"message":"not a verified key"}"#
    )]
    #[case::html(
        b"<!doctype html><html><head><title>502 Bad Gateway</title></head><body><h1>cloud proxy</h1></body></html>",
        "502 Bad Gateway"
    )]
    fn test_venue_error_message_shapes(#[case] body: &[u8], #[case] expected: &str) {
        assert_eq!(venue_error_message(body), expected);
    }

    #[rstest]
    fn test_venue_error_message_is_bounded_by_unicode_characters() {
        let body = format!(r#"{{"error":"{}"}}"#, "é".repeat(600));
        let message = venue_error_message(body.as_bytes());

        assert_eq!(message.chars().count(), ERROR_TEXT_MAX_CHARS);
        assert!(message.ends_with(ERROR_TEXT_TRUNCATED_SUFFIX));
    }

    #[rstest]
    fn test_raw_fallback_is_lossy_and_bounded() {
        let mut body = vec![b'x'; 600];
        body[10] = 0xff;

        let message = venue_error_message(&body);

        assert_eq!(message.chars().count(), ERROR_TEXT_MAX_CHARS);
        assert_eq!(message.chars().nth(10), Some(char::REPLACEMENT_CHARACTER));
        assert!(message.ends_with(ERROR_TEXT_TRUNCATED_SUFFIX));
    }

    #[rstest]
    fn test_from_http_client_preserves_timeout_classification() {
        let timeout = Error::from_http_client(HttpClientError::TimeoutError("late".to_string()));
        let transport = Error::from_http_client(HttpClientError::Error("reset".to_string()));

        assert!(matches!(timeout, Error::Timeout));
        assert!(matches!(transport, Error::Transport(_)));
        assert!(timeout.is_retryable());
        assert!(timeout.is_submit_outcome_unknown());
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
        assert!(!Error::http(425, "too early").is_submit_outcome_unknown());
        assert!(!Error::http(404, "not found").is_submit_outcome_unknown());
    }
}

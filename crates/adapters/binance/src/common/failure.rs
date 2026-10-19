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

//! Binance mapping from transport failures to [`CommandFailure`].
//!
//! Classification happens once at the execution boundary using the typed evidence preserved by
//! the HTTP and WebSocket transports. Retryability is a separate axis handled by the error types'
//! `is_retryable` methods and the retry manager. Strategy-facing reasons collapse whitespace,
//! remove control characters, and limit output length.
//!
//! Rate limiting, server errors, and execution-status-unknown codes do not prove that a command
//! was unapplied. Missing venue error evidence likewise leaves the command outcome ambiguous.

use nautilus_live::execution::failure::CommandFailure;

use crate::{
    common::error::{is_ambiguous_venue_code, is_retryable_http_status, is_retryable_venue_code},
    futures::http::error::BinanceFuturesHttpError,
    spot::http::error::BinanceSpotHttpError,
};

const MAX_REASON_CHARS: usize = 256;

const TRUNCATED_SUFFIX: &str = "...";

pub(crate) fn sanitize_reason(text: &str) -> String {
    let normalized = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.chars().count() <= MAX_REASON_CHARS {
        return normalized;
    }

    let mut bounded: String = normalized
        .chars()
        .take(MAX_REASON_CHARS - TRUNCATED_SUFFIX.len())
        .collect();
    bounded.push_str(TRUNCATED_SUFFIX);
    bounded
}

#[must_use]
pub(crate) fn classify_venue_failure(
    code: Option<i64>,
    status: Option<u16>,
    reason: impl Into<String>,
) -> CommandFailure {
    let reason = reason.into();

    let Some(code) = code else {
        return CommandFailure::Ambiguous(reason);
    };

    if let Some(status) = status
        && is_retryable_http_status(status)
    {
        return CommandFailure::Ambiguous(reason);
    }

    if is_ambiguous_venue_code(code) || is_retryable_venue_code(code) {
        CommandFailure::Ambiguous(reason)
    } else {
        CommandFailure::VenueRejected(reason)
    }
}

#[must_use]
pub(crate) fn classify_spot_http_failure(error: &BinanceSpotHttpError) -> CommandFailure {
    let reason = error.to_string();

    match error {
        BinanceSpotHttpError::MissingCredentials | BinanceSpotHttpError::ValidationError(_) => {
            CommandFailure::NotSent(reason)
        }
        BinanceSpotHttpError::BinanceError { code, status, .. } => {
            classify_venue_failure(Some(*code), Some(*status), reason)
        }
        BinanceSpotHttpError::SbeDecodeError(_)
        | BinanceSpotHttpError::JsonError(_)
        | BinanceSpotHttpError::ResponseParseError(_)
        | BinanceSpotHttpError::NetworkError(_)
        | BinanceSpotHttpError::Timeout(_)
        | BinanceSpotHttpError::Canceled(_)
        | BinanceSpotHttpError::RetryBudgetExceeded(_)
        | BinanceSpotHttpError::UnexpectedStatus { .. } => CommandFailure::Ambiguous(reason),
    }
}

#[must_use]
pub(crate) fn classify_futures_http_failure(error: &BinanceFuturesHttpError) -> CommandFailure {
    let reason = error.to_string();

    match error {
        BinanceFuturesHttpError::MissingCredentials
        | BinanceFuturesHttpError::ValidationError(_) => CommandFailure::NotSent(reason),
        BinanceFuturesHttpError::BinanceError { code, status, .. } => {
            classify_venue_failure(Some(*code), Some(*status), reason)
        }
        BinanceFuturesHttpError::JsonError(_)
        | BinanceFuturesHttpError::NetworkError(_)
        | BinanceFuturesHttpError::Timeout(_)
        | BinanceFuturesHttpError::Canceled(_)
        | BinanceFuturesHttpError::RetryBudgetExceeded(_)
        | BinanceFuturesHttpError::UnexpectedStatus { .. } => CommandFailure::Ambiguous(reason),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::missing_credentials(
        BinanceSpotHttpError::MissingCredentials,
        CommandFailure::NotSent("Missing API credentials".to_string())
    )]
    #[case::validation(
        BinanceSpotHttpError::ValidationError("bad param".to_string()),
        CommandFailure::NotSent("Validation error: bad param".to_string())
    )]
    fn test_classify_spot_http_not_sent(
        #[case] error: BinanceSpotHttpError,
        #[case] expected: CommandFailure,
    ) {
        assert_eq!(classify_spot_http_failure(&error), expected);
    }

    #[rstest]
    fn test_classify_spot_http_venue_rejected() {
        let error = BinanceSpotHttpError::BinanceError {
            code: -2010,
            message: "NEW_ORDER_REJECTED".to_string(),
            status: 400,
            retry_after: None,
        };

        assert!(matches!(
            classify_spot_http_failure(&error),
            CommandFailure::VenueRejected(_)
        ));
    }

    #[rstest]
    #[case::rate_limit_code(-1003, 429)]
    #[case::order_rate_limit_code(-1015, 400)]
    #[case::server_error_with_venue_body(-1000, 500)]
    #[case::status_unknown(-1007, 400)]
    #[case::unexpected_response(-1006, 400)]
    fn test_classify_spot_http_ambiguous(#[case] code: i64, #[case] status: u16) {
        let error = BinanceSpotHttpError::BinanceError {
            code,
            message: "test".to_string(),
            status,
            retry_after: None,
        };

        assert!(matches!(
            classify_spot_http_failure(&error),
            CommandFailure::Ambiguous(_)
        ));
    }

    #[rstest]
    #[case::timeout(BinanceSpotHttpError::Timeout("t".to_string()))]
    #[case::network(BinanceSpotHttpError::NetworkError("n".to_string()))]
    #[case::canceled(BinanceSpotHttpError::Canceled("c".to_string()))]
    #[case::budget(BinanceSpotHttpError::RetryBudgetExceeded("b".to_string()))]
    #[case::parse(BinanceSpotHttpError::ResponseParseError("p".to_string()))]
    #[case::unexpected_status(BinanceSpotHttpError::UnexpectedStatus {
        status: 400,
        body: "garbage".to_string(),
        retry_after: None,
    })]
    fn test_classify_spot_http_ambiguous_variants(#[case] error: BinanceSpotHttpError) {
        assert!(matches!(
            classify_spot_http_failure(&error),
            CommandFailure::Ambiguous(_)
        ));
    }

    #[rstest]
    fn test_classify_futures_http_venue_rejected() {
        let error = BinanceFuturesHttpError::BinanceError {
            code: -2011,
            message: "Unknown order sent.".to_string(),
            status: 400,
            retry_after: None,
        };

        assert!(matches!(
            classify_futures_http_failure(&error),
            CommandFailure::VenueRejected(_)
        ));
    }

    #[rstest]
    fn test_classify_futures_http_server_error_with_body_is_ambiguous() {
        let error = BinanceFuturesHttpError::BinanceError {
            code: -1000,
            message: "An unknown error occurred while processing the request.".to_string(),
            status: 500,
            retry_after: None,
        };

        assert!(matches!(
            classify_futures_http_failure(&error),
            CommandFailure::Ambiguous(_)
        ));
    }

    #[rstest]
    #[case::venue_rejected(Some(-2010), Some(400), true)]
    #[case::rate_limit_status(Some(-1003), Some(429), false)]
    #[case::server_error_status(Some(-1000), Some(500), false)]
    #[case::unknown_status_code(Some(-1006), Some(400), false)]
    #[case::rate_limit_code(Some(-1015), Some(400), false)]
    #[case::missing_code(None, Some(400), false)]
    #[case::missing_code_and_status(None, None, false)]
    fn test_classify_venue_failure(
        #[case] code: Option<i64>,
        #[case] status: Option<u16>,
        #[case] expect_rejected: bool,
    ) {
        let failure = classify_venue_failure(code, status, "reason");

        if expect_rejected {
            assert!(matches!(failure, CommandFailure::VenueRejected(_)));
        } else {
            assert!(matches!(failure, CommandFailure::Ambiguous(_)));
        }
    }

    #[rstest]
    fn test_sanitize_reason_strips_control_characters() {
        assert_eq!(sanitize_reason("bad\x00\x07text"), "bad text");
        assert_eq!(sanitize_reason("line1\nline2\ttab"), "line1 line2 tab");
    }

    #[rstest]
    fn test_sanitize_reason_collapses_whitespace() {
        assert_eq!(sanitize_reason("  multiple   spaces  "), "multiple spaces");
    }

    #[rstest]
    fn test_sanitize_reason_truncates() {
        let long = "x".repeat(400);
        let result = sanitize_reason(&long);
        assert_eq!(result.chars().count(), MAX_REASON_CHARS);
        assert!(result.ends_with(TRUNCATED_SUFFIX));
    }

    #[rstest]
    fn test_sanitize_reason_short_text_unchanged() {
        assert_eq!(
            sanitize_reason("Insufficient balance"),
            "Insufficient balance"
        );
    }
}

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

//! Retry classification for the Derive adapter.
//!
//! Splits [`DeriveHttpError`] and [`DeriveWsError`] into retryable, terminal,
//! and fatal categories. The HTTP client routes errors through these helpers
//! when driving [`nautilus_network::retry::RetryManager`]; the adapter-level
//! [`crate::common::error::DeriveError`] reuses them for `is_retryable` /
//! `is_fatal`.

use nautilus_network::retry::RetryConfig;

use crate::{http::DeriveHttpError, websocket::DeriveWsError};

/// Builds a [`RetryConfig`] for Derive HTTP calls from the adapter's config
/// fields.
///
/// `max_retries` is the budget; `initial_delay_ms` and `max_delay_ms` bound
/// the exponential backoff. Other fields use values tuned for Derive: a
/// 60-second per-attempt timeout (REST endpoints can return slow during venue
/// load), a 3-minute overall budget, and 1s of jitter to avoid synchronizing
/// retry storms across processes.
#[must_use]
pub fn http_retry_config(
    max_retries: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
) -> RetryConfig {
    RetryConfig {
        max_retries,
        initial_delay_ms,
        max_delay_ms,
        backoff_factor: 2.0,
        jitter_ms: 1_000,
        operation_timeout_ms: Some(60_000),
        immediate_first: false,
        max_elapsed_ms: Some(180_000),
    }
}

/// Returns `true` for HTTP errors that can safely be retried with backoff.
///
/// Retryable categories:
///
/// - Transport failures (connection reset, timeout, DNS).
/// - HTTP 5xx and 408 / 429.
/// - JSON-RPC `Server error` codes in the `-32099..=-32000` range.
///
/// Everything else (validation, signed-fee-too-low, insufficient-margin,
/// auth failure) is terminal and must not be retried.
#[must_use]
pub fn should_retry_http_error(error: &DeriveHttpError) -> bool {
    match error {
        DeriveHttpError::Transport(_) => true,
        DeriveHttpError::Http { status, .. } => is_retryable_status(*status),
        DeriveHttpError::JsonRpc { code, .. } => is_retryable_jsonrpc_code(*code),
        DeriveHttpError::MissingResult { .. }
        | DeriveHttpError::Decode(_)
        | DeriveHttpError::Serde(_)
        | DeriveHttpError::Auth(_)
        | DeriveHttpError::MissingCredentials { .. } => false,
    }
}

/// Returns `true` for HTTP errors that signal a fatal session state requiring
/// operator intervention (auth header rejection, session key deregistered,
/// subaccount withdrawn).
///
/// Fatal errors are a subset of non-retryable: they should also short-circuit
/// any caller-level retry budgets.
#[must_use]
pub fn is_fatal_http_error(error: &DeriveHttpError) -> bool {
    match error {
        DeriveHttpError::Auth(_) | DeriveHttpError::MissingCredentials { .. } => true,
        DeriveHttpError::Http { status, .. } => matches!(*status, 401 | 403),
        DeriveHttpError::JsonRpc { code, .. } => is_fatal_jsonrpc_code(*code),
        _ => false,
    }
}

/// Returns `true` for WebSocket errors that can safely be retried.
#[must_use]
pub fn should_retry_ws_error(error: &DeriveWsError) -> bool {
    match error {
        DeriveWsError::Transport(_)
        | DeriveWsError::RequestCancelled { .. }
        | DeriveWsError::Timeout { .. } => true,
        DeriveWsError::JsonRpc { code, .. } => is_retryable_jsonrpc_code(*code),
        DeriveWsError::NotConnected
        | DeriveWsError::Serde(_)
        | DeriveWsError::Auth(_)
        | DeriveWsError::MissingCredentials { .. } => false,
    }
}

/// Returns `true` for WebSocket errors that indicate a fatal session state.
#[must_use]
pub fn is_fatal_ws_error(error: &DeriveWsError) -> bool {
    match error {
        DeriveWsError::Auth(_) | DeriveWsError::MissingCredentials { .. } => true,
        DeriveWsError::JsonRpc { code, .. } => is_fatal_jsonrpc_code(*code),
        _ => false,
    }
}

/// Classifies an HTTP status code.
#[must_use]
fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429) || (500..600).contains(&status)
}

/// Classifies a JSON-RPC error code.
///
/// Derive does not publish a stable retry classification for its venue codes,
/// so the policy is conservative: only generic transient categories retry
/// (the JSON-RPC `Server error` range, plus internal error `-32603` which the
/// venue uses for transient backend faults). Signed-action rejections such as
/// `signed_max_fee_too_low` and `insufficient_margin` arrive as standard
/// invalid-params errors and are intentionally not retried; the caller has to
/// reprice or refund collateral before resubmission.
#[must_use]
pub(crate) fn is_retryable_jsonrpc_code(code: i64) -> bool {
    code == -32603 || (-32099..=-32000).contains(&code)
}

/// Returns `true` only for JSON-RPC codes where the *outcome of a state-changing
/// write* is genuinely ambiguous: the venue may have processed the request and
/// merely failed to respond. Strictly narrower than [`is_retryable_jsonrpc_code`].
///
/// The retry classifier covers transient transport-style failures, including
/// venue-defined codes like `-32000 Rate limit exceeded`. Rate-limit (and most
/// other Derive server errors) is a **definitive** rejection: the gateway threw
/// the request out before the matching engine saw it. Treating those as
/// ambiguous leaves the order hanging in `Submitted` forever because no WS
/// frame will come for an order that was never placed.
///
/// The current entry is `-32603` (generic JSON-RPC internal error): the only
/// code where the venue's own process is known to have run for some unknown
/// distance before failing. Extend this list only with evidence that a code
/// genuinely leaves outcome unknown.
#[must_use]
pub(crate) fn is_write_outcome_ambiguous_jsonrpc(code: i64) -> bool {
    code == -32603
}

/// Returns `true` for non-JSON-RPC HTTP statuses where a state-changing
/// write failed before the matching engine could accept it.
///
/// HTTP 4xx responses come from gateway, auth, throttling, or request-shape
/// rejection paths. They are definitive for submit/cancel/modify outcomes,
/// even when an idempotent read would retry some of them. HTTP 5xx and
/// transport failures remain ambiguous for writes.
///
/// Retained for the HTTP order-write path (the execution client now writes over
/// the WebSocket and classifies outcomes via `is_write_outcome_ambiguous_ws`).
#[must_use]
pub fn is_write_outcome_definitive_http_status(status: u16) -> bool {
    (400..500).contains(&status)
}

/// Returns `true` when a WebSocket write's outcome is unknown (sent, but no
/// clear venue verdict), so the caller emits no terminal event and lets
/// reconciliation settle the order. `JsonRpc` defers to the shared code policy
/// in [`is_write_outcome_ambiguous_jsonrpc`] (only `-32603`).
///
/// Two non-obvious calls: `Serde` is ambiguous because it is a failure to decode
/// the *response* (the request cannot fail to serialize), so the action may have
/// been processed; `NotConnected` is definitive because it is returned before
/// the frame is sent, so the order was never placed.
#[must_use]
pub(crate) fn is_write_outcome_ambiguous_ws(error: &DeriveWsError) -> bool {
    match error {
        DeriveWsError::Transport(_)
        | DeriveWsError::RequestCancelled { .. }
        | DeriveWsError::Timeout { .. }
        | DeriveWsError::Serde(_) => true,
        DeriveWsError::JsonRpc { code, .. } => is_write_outcome_ambiguous_jsonrpc(*code),
        DeriveWsError::NotConnected
        | DeriveWsError::Auth(_)
        | DeriveWsError::MissingCredentials { .. } => false,
    }
}

/// Classifies a JSON-RPC error code as fatal. Derive currently does not
/// expose a dedicated session-killed code, so this only flags the standard
/// invalid-request shape used for unrecoverable framing problems.
#[must_use]
fn is_fatal_jsonrpc_code(code: i64) -> bool {
    matches!(code, -32600 | -32700)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    #[rstest]
    fn test_transport_error_retryable() {
        let err = DeriveHttpError::transport("conn reset");
        assert!(should_retry_http_error(&err));
        assert!(!is_fatal_http_error(&err));
    }

    #[rstest]
    #[case(500, true)]
    #[case(502, true)]
    #[case(503, true)]
    #[case(504, true)]
    #[case(429, true)]
    #[case(408, true)]
    #[case(400, false)]
    #[case(404, false)]
    #[case(409, false)]
    #[case(422, false)]
    fn test_http_status_retry_classification(#[case] status: u16, #[case] retryable: bool) {
        let err = DeriveHttpError::http(status, "body");
        assert_eq!(should_retry_http_error(&err), retryable);
    }

    #[rstest]
    #[case(401)]
    #[case(403)]
    fn test_http_auth_status_is_fatal(#[case] status: u16) {
        let err = DeriveHttpError::http(status, "Unauthorized");
        assert!(is_fatal_http_error(&err));
        assert!(!should_retry_http_error(&err));
    }

    #[rstest]
    fn test_jsonrpc_invalid_params_not_retryable() {
        // Venue surfaces `signed_max_fee_too_low`, `insufficient_margin`, etc.
        // as standard JSON-RPC -32602 invalid-params payloads. These reflect
        // caller-side state and must never be retried.
        let err = DeriveHttpError::JsonRpc {
            code: -32602,
            message: "signed_max_fee_too_low".into(),
            data: None,
        };
        assert!(!should_retry_http_error(&err));
        assert!(!is_fatal_http_error(&err));
    }

    #[rstest]
    fn test_jsonrpc_server_error_range_retryable() {
        let err = DeriveHttpError::JsonRpc {
            code: -32050,
            message: "Server busy".into(),
            data: None,
        };
        assert!(should_retry_http_error(&err));
    }

    #[rstest]
    fn test_jsonrpc_internal_error_retryable() {
        let err = DeriveHttpError::JsonRpc {
            code: -32603,
            message: "Internal error".into(),
            data: None,
        };
        assert!(should_retry_http_error(&err));
    }

    #[rstest]
    #[case(400, true)]
    #[case(401, true)]
    #[case(403, true)]
    #[case(408, true)]
    #[case(429, true)]
    #[case(500, false)]
    #[case(503, false)]
    fn test_http_status_write_outcome_classification(
        #[case] status: u16,
        #[case] definitive: bool,
    ) {
        assert_eq!(is_write_outcome_definitive_http_status(status), definitive);
    }

    #[rstest]
    fn test_jsonrpc_invalid_request_is_fatal() {
        let err = DeriveHttpError::JsonRpc {
            code: -32600,
            message: "Invalid request".into(),
            data: Some(Value::Null),
        };
        assert!(is_fatal_http_error(&err));
        assert!(!should_retry_http_error(&err));
    }

    #[rstest]
    fn test_missing_credentials_terminal() {
        let err = DeriveHttpError::MissingCredentials {
            method: "private/order".into(),
        };
        assert!(!should_retry_http_error(&err));
        assert!(is_fatal_http_error(&err));
    }

    #[rstest]
    fn test_ws_transport_retryable() {
        let err = DeriveWsError::transport("send failed");
        assert!(should_retry_ws_error(&err));
    }

    #[rstest]
    fn test_ws_not_connected_terminal() {
        let err = DeriveWsError::NotConnected;
        assert!(!should_retry_ws_error(&err));
        assert!(!is_fatal_ws_error(&err));
    }

    #[rstest]
    fn test_ws_request_cancelled_retryable() {
        // The handler drops the oneshot on reconnect; the caller can re-issue
        // after the new session is up.
        let err = DeriveWsError::RequestCancelled {
            method: "subscribe".into(),
        };
        assert!(should_retry_ws_error(&err));
    }

    #[rstest]
    fn test_ws_timeout_retryable_not_fatal() {
        let err = DeriveWsError::Timeout {
            method: "private/order".into(),
        };
        assert!(should_retry_ws_error(&err));
        assert!(!is_fatal_ws_error(&err));
    }

    #[rstest]
    fn test_ws_write_outcome_ambiguous_classification() {
        // Sent-but-unconfirmed outcomes are ambiguous; everything else is a
        // definitive rejection the caller can surface as a terminal event.
        let ambiguous = [
            DeriveWsError::transport("send failed"),
            DeriveWsError::RequestCancelled {
                method: "private/order".into(),
            },
            DeriveWsError::Timeout {
                method: "private/order".into(),
            },
            // A response the client cannot decode: the action may have been
            // processed, so await reconciliation rather than reject.
            DeriveWsError::Serde(serde_json::from_str::<Value>("{").unwrap_err()),
            DeriveWsError::JsonRpc {
                code: -32603,
                message: "Internal error".into(),
                data: None,
            },
        ];
        let definitive = [
            DeriveWsError::NotConnected,
            DeriveWsError::JsonRpc {
                code: -32602,
                message: "signed_max_fee_too_low".into(),
                data: None,
            },
            DeriveWsError::MissingCredentials {
                operation: "private/order".into(),
            },
        ];

        for err in &ambiguous {
            assert!(
                is_write_outcome_ambiguous_ws(err),
                "expected ambiguous: {err}"
            );
        }

        for err in &definitive {
            assert!(
                !is_write_outcome_ambiguous_ws(err),
                "expected definitive: {err}",
            );
        }
    }
}

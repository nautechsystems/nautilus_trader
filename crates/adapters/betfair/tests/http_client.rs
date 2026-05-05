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

//! Integration tests for `BetfairHttpClient`.

#[allow(dead_code)]
mod common;

use nautilus_betfair::{
    common::consts::{METHOD_GET_ACCOUNT_FUNDS, METHOD_LIST_MARKET_CATALOGUE},
    http::error::BetfairHttpError,
};
use rstest::rstest;
use serde_json::Value;

use crate::common::*;

#[rstest]
#[tokio::test]
async fn test_connect_stores_session_token() {
    let (addr, state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();

    assert!(client.is_connected().await);
    assert!(client.session_token().await.is_some());
    assert!(state.login_count.load(std::sync::atomic::Ordering::Relaxed) > 0);
}

#[rstest]
#[tokio::test]
async fn test_connect_login_failure() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/login_failure.json");
    *state.login_response_override.lock().unwrap() = Some(fixture);

    let client = create_test_http_client(addr);

    let result = client.connect().await;
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), BetfairHttpError::LoginFailed { .. }),
        "Expected LoginFailed error"
    );
    assert!(!client.is_connected().await);
}

#[rstest]
#[tokio::test]
async fn test_disconnect_clears_token() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();
    assert!(client.is_connected().await);

    client.disconnect().await;
    assert!(!client.is_connected().await);
    assert!(client.session_token().await.is_none());
}

#[rstest]
#[tokio::test]
async fn test_reconnect_refreshes_token() {
    let (addr, state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        1
    );

    client.reconnect().await.unwrap();
    assert!(client.is_connected().await);
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}

#[rstest]
#[tokio::test]
async fn test_keep_alive_refreshes_token() {
    let (addr, state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();
    assert_eq!(
        state
            .keep_alive_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );

    client.keep_alive().await.unwrap();
    assert!(client.is_connected().await);
    assert_eq!(
        state
            .keep_alive_count
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[rstest]
#[tokio::test]
async fn test_send_betting_returns_parsed_response() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();

    let result: Value = client
        .send_betting(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await
        .unwrap();

    assert!(!result.is_null(), "Expected non-null betting response");
}

#[rstest]
#[tokio::test]
async fn test_send_accounts_returns_parsed_response() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();

    let result: Value = client
        .send_accounts(METHOD_GET_ACCOUNT_FUNDS, &serde_json::json!({}))
        .await
        .unwrap();

    assert!(!result.is_null(), "Expected non-null accounts response");
}

#[rstest]
#[tokio::test]
async fn test_send_navigation_returns_parsed_response() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();

    let result: Value = client.send_navigation().await.unwrap();

    assert!(!result.is_null(), "Expected non-null navigation response");
}

/// When `keep_alive` fails (e.g. NO_SESSION), `reconnect` performs a full
/// re-login and restores the session token.
#[rstest]
#[tokio::test]
async fn test_keep_alive_failure_then_reconnect_restores_session() {
    let (addr, state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();
    assert!(client.is_connected().await);

    // Make keep_alive fail
    *state.keep_alive_response_override.lock().unwrap() =
        Some(r#"{"token":"","product":"","status":"FAIL","error":"NO_SESSION"}"#.to_string());
    let ka_result = client.keep_alive().await;
    assert!(ka_result.is_err(), "keep_alive should fail with NO_SESSION");

    // Token should still exist (keep_alive failure does not clear it)
    assert!(client.session_token().await.is_some());

    // Full reconnect should restore the session
    client.reconnect().await.unwrap();
    assert!(client.is_connected().await);
    assert!(client.session_token().await.is_some());

    // Login should have been called twice (initial + reconnect)
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}

/// A transient keep-alive failure (e.g. malformed response) returns a non-LoginFailed
/// error and must NOT clear the session token or trigger a re-login.
#[rstest]
#[tokio::test]
async fn test_keep_alive_transient_error_preserves_session() {
    let (addr, state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();
    let initial_token = client.session_token().await.clone();
    assert!(initial_token.is_some());

    // Return invalid JSON to simulate a transient failure (produces JsonError)
    *state.keep_alive_response_override.lock().unwrap() = Some("not-json".to_string());
    let ka_result = client.keep_alive().await;
    assert!(ka_result.is_err());
    assert!(
        !ka_result.as_ref().unwrap_err().is_login_failed(),
        "transient error should not be LoginFailed"
    );

    // Session token must be preserved (not cleared)
    assert_eq!(client.session_token().await, initial_token);

    // No re-login should have occurred
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "transient keep-alive failure must not trigger re-login"
    );
}

#[rstest]
#[tokio::test]
async fn test_send_betting_without_session_returns_error() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    let result: Result<Value, _> = client
        .send_betting(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await;

    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), BetfairHttpError::MissingCredentials),
        "Expected MissingCredentials error"
    );
}

/// A JSON-RPC error envelope must surface as `BetfairError` with the
/// venue's code and message preserved verbatim so callers can branch on it.
#[rstest]
#[tokio::test]
async fn test_send_betting_jsonrpc_error_returns_betfair_error() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().unwrap().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        (-32602, "DSC-018".to_string()),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let result: Result<Value, _> = client
        .send_betting(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await;

    match result {
        Err(BetfairHttpError::BetfairError { code, message }) => {
            assert_eq!(code, -32602);
            assert_eq!(message, "DSC-018");
        }
        other => panic!("Expected BetfairError, was {other:?}"),
    }
}

/// JSON-RPC errors mentioning `NO_SESSION` (or `INVALID_SESSION_INFORMATION`)
/// must be classifiable as session errors so callers can trigger a re-login.
#[rstest]
#[tokio::test]
async fn test_send_betting_session_error_classified_as_session() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().unwrap().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        (-1, "NO_SESSION".to_string()),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let err = client
        .send_betting::<Value, _>(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await
        .expect_err("expected NO_SESSION to surface as error");

    assert!(err.is_session_error(), "expected session error: {err}");
    assert!(
        !err.is_login_failed(),
        "session error must not be login-failed"
    );
}

/// `TOO_MANY_REQUESTS` JSON-RPC errors must classify as rate-limit so the
/// caller's retry policy can back off rather than treat it as a hard failure.
#[rstest]
#[tokio::test]
async fn test_send_betting_too_many_requests_classified_as_rate_limit() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().unwrap().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        (-1, "TOO_MANY_REQUESTS".to_string()),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let err = client
        .send_betting::<Value, _>(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await
        .expect_err("expected TOO_MANY_REQUESTS to surface as error");

    assert!(
        err.is_rate_limit_error(),
        "expected rate-limit classification: {err}"
    );
    assert!(
        !err.is_session_error(),
        "rate-limit must not be a session error"
    );
}

/// 5xx errors are retryable. With `max_retries=1` (the test fixture default)
/// a permanently-failing endpoint must hit the server twice: one initial call
/// plus one retry. Anything fewer means the retry loop is broken.
#[rstest]
#[tokio::test]
async fn test_send_betting_5xx_retries_once_before_giving_up() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_MARKET_CATALOGUE.to_string(), 503);

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let initial_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);

    let _ = client
        .send_betting::<Value, _>(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await
        .expect_err("expected 503 to surface as error");

    let final_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        final_count - initial_count,
        2,
        "expected 1 initial call + 1 retry under max_retries=1"
    );
}

/// `connect()` is idempotent: a second call on an already-connected client
/// must not trigger a second login. Mirrors Python's `asyncio.Lock` + token
/// short-circuit so callers can re-issue connect without burning logins.
#[rstest]
#[tokio::test]
async fn test_http_client_connect_is_idempotent() {
    let (addr, state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();
    let after_first = state.login_count.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(after_first, 1);

    client.connect().await.unwrap();
    let after_second = state.login_count.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        after_second, 1,
        "second connect on a live client must not re-login"
    );
}

/// Concurrent `connect()` calls must serialise under the connect lock so the
/// venue sees exactly one login regardless of how many tasks raced.
#[rstest]
#[tokio::test]
async fn test_http_client_connect_concurrent_calls_only_login_once() {
    let (addr, state) = start_mock_http().await;
    let client = std::sync::Arc::new(create_test_http_client(addr));

    let mut handles = Vec::new();

    for _ in 0..5 {
        let c = std::sync::Arc::clone(&client);
        handles.push(tokio::spawn(async move { c.connect().await }));
    }

    for h in handles {
        h.await.unwrap().unwrap();
    }

    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "concurrent connects must serialise to a single login round-trip"
    );
}

/// After `disconnect()`, the client's cancellation token must be a fresh
/// (uncancelled) instance so the next session can run new requests, while
/// any holders of the prior token observe cancellation.
#[rstest]
#[tokio::test]
async fn test_disconnect_cancels_and_refreshes_cancellation_token() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.connect().await.unwrap();
    let pre = client.cancellation_token();
    assert!(!pre.is_cancelled(), "fresh token must not be cancelled");

    client.disconnect().await;

    assert!(
        pre.is_cancelled(),
        "the pre-disconnect token must now be cancelled so any in-flight retries unblock"
    );

    let post = client.cancellation_token();
    assert!(
        !post.is_cancelled(),
        "disconnect must install a fresh token for the next session"
    );
}

/// `disconnect()` on a never-connected client is a no-op rather than an error
/// so caller cleanup paths can be unconditional.
#[rstest]
#[tokio::test]
async fn test_http_client_disconnect_when_never_connected_is_noop() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    client.disconnect().await;
    assert!(!client.is_connected().await);
    assert!(client.session_token().await.is_none());

    client.disconnect().await;
    assert!(!client.is_connected().await);
}

/// Server-side 5xx status codes exhaust retries and surface as
/// `UnexpectedStatus` rather than being silently swallowed.
#[rstest]
#[tokio::test]
async fn test_send_betting_unexpected_status_surfaces_error() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_MARKET_CATALOGUE.to_string(), 503);

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let err = client
        .send_betting::<Value, _>(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await
        .expect_err("expected 5xx to surface as error after retries");

    match err {
        BetfairHttpError::UnexpectedStatus { status, .. } => {
            assert_eq!(status, 503);
        }
        other => panic!("Expected UnexpectedStatus, was {other:?}"),
    }
}

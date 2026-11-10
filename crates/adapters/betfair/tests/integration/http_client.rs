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

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use nautilus_betfair::{
    common::{
        consts::{METHOD_GET_ACCOUNT_FUNDS, METHOD_LIST_MARKET_CATALOGUE, METHOD_PLACE_ORDERS},
        credential::BetfairCredential,
    },
    http::{client::BetfairHttpClient, error::BetfairHttpError},
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
    *state.login_response_override.lock() = Some(fixture);

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
    *state.keep_alive_response_override.lock() =
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
    *state.keep_alive_response_override.lock() = Some("not-json".to_string());
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
    state.betting_error_overrides.lock().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        load_json_fixture("rest/betting_jsonrpc_error_invalid_params_live.json"),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let result: Result<Value, _> = client
        .send_betting(METHOD_LIST_MARKET_CATALOGUE, &serde_json::json!({}))
        .await;

    match result {
        Err(BetfairHttpError::BetfairError {
            code,
            message,
            api_error_code,
            api_error_details,
        }) => {
            assert_eq!(code, -32602);
            assert_eq!(message, "DSC-0018");
            assert_eq!(api_error_code, None);
            assert_eq!(api_error_details, None);
        }
        other => panic!("Expected BetfairError, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_send_betting_order_definitive_error_remains_betfair_error() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().insert(
        METHOD_PLACE_ORDERS.to_string(),
        load_json_fixture("rest/betting_jsonrpc_error_invalid_params_live.json"),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let result = client
        .send_betting_order::<Value, _>(METHOD_PLACE_ORDERS, &serde_json::json!({}))
        .await;

    assert!(matches!(
        result,
        Err(BetfairHttpError::BetfairError {
            code: -32602,
            ref message,
            api_error_code: None,
            api_error_details: None,
        }) if message == "DSC-0018"
    ));
}

/// JSON-RPC API exceptions carrying `NO_SESSION` (or
/// `INVALID_SESSION_INFORMATION`) must be classifiable as session errors so
/// callers can trigger a re-login.
#[rstest]
#[tokio::test]
async fn test_send_betting_session_error_classified_as_session() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        betting_api_error("NO_SESSION"),
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
    assert!(matches!(
        &err,
        BetfairHttpError::BetfairError {
            code: -32099,
            message,
            api_error_code: Some(api_error_code),
            api_error_details: Some(api_error_details),
        } if message == "ANGX-0003"
            && api_error_code == "NO_SESSION"
            && api_error_details.is_empty()
    ));
}

/// `TOO_MANY_REQUESTS` JSON-RPC errors must classify as rate-limit so the
/// caller's retry policy can back off rather than treat it as a hard failure.
#[rstest]
#[tokio::test]
async fn test_send_betting_too_many_requests_classified_as_rate_limit() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        betting_api_error("TOO_MANY_REQUESTS"),
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

#[rstest]
#[tokio::test]
async fn test_send_betting_api_error_preserves_live_wire_fields() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        load_json_fixture("rest/betting_jsonrpc_error_too_much_data_live.json"),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let err = client
        .send_betting::<Value, _>(METHOD_LIST_MARKET_CATALOGUE, serde_json::json!({}))
        .await
        .expect_err("expected TOO_MUCH_DATA to surface as an error");

    assert!(matches!(
        &err,
        BetfairHttpError::BetfairError {
            code: -32099,
            message,
            api_error_code: Some(api_error_code),
            api_error_details: Some(api_error_details),
        } if message == "ANGX-0001"
            && api_error_code == "TOO_MUCH_DATA"
            && api_error_details == "MaxResults must be less than or equal to 1000"
    ));
    assert!(!err.is_retryable());
    assert!(!err.is_session_error());
}

#[rstest]
#[tokio::test]
async fn test_send_accounts_selects_exception_named_by_wire_data() {
    let (addr, state) = start_mock_http().await;
    state.accounts_error_overrides.lock().insert(
        METHOD_GET_ACCOUNT_FUNDS.to_string(),
        load_json_fixture("rest/account_jsonrpc_error_invalid_input_live.json"),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let err = client
        .send_accounts::<Value, _>(METHOD_GET_ACCOUNT_FUNDS, serde_json::json!({}))
        .await
        .expect_err("expected INVALID_INPUT_DATA to surface as an error");

    assert!(matches!(
        &err,
        BetfairHttpError::BetfairError {
            code: -32099,
            message,
            api_error_code: Some(api_error_code),
            api_error_details: Some(api_error_details),
        } if message == "AANGX-0001"
            && api_error_code == "INVALID_INPUT_DATA"
            && api_error_details == "The request was not valid."
    ));
    assert!(!err.is_retryable());
    assert!(!err.is_session_error());
}

#[rstest]
#[case(None)]
#[case(Some(serde_json::Value::String("not an exception object".to_string())))]
#[case(Some(serde_json::json!({"exceptionname": "MissingException"})))]
#[tokio::test]
async fn test_send_betting_malformed_api_error_data_preserves_outer_error(
    #[case] data: Option<serde_json::Value>,
) {
    let (addr, state) = start_mock_http().await;
    let mut response = load_json_fixture("rest/betting_jsonrpc_error_invalid_session_live.json");
    if let Some(data) = data {
        response["error"]["data"] = data;
    } else {
        response["error"].as_object_mut().unwrap().remove("data");
    }
    state
        .betting_error_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), response);

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();

    let err = client
        .send_betting_order::<Value, _>(METHOD_PLACE_ORDERS, serde_json::json!({}))
        .await
        .expect_err("expected malformed API error data to preserve the outer error");

    assert!(matches!(
        &err,
        BetfairHttpError::BetfairError {
            code: -32099,
            message,
            api_error_code: None,
            api_error_details: None,
        } if message == "ANGX-0003"
    ));
    assert!(err.is_order_ambiguous());
    assert!(!err.is_order_retryable());
}

#[rstest]
#[case("TOO_MANY_REQUESTS")]
#[case("SERVICE_BUSY")]
#[case("UNEXPECTED_ERROR")]
#[tokio::test]
async fn test_send_betting_order_retries_documented_transient_api_error(
    #[case] api_error_code: &str,
) {
    let (addr, state) = start_mock_http().await;
    state.betting_error_one_shot_overrides.lock().insert(
        METHOD_PLACE_ORDERS.to_string(),
        betting_api_error(api_error_code),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();
    let initial_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);

    let result: Value = client
        .send_betting_order(
            METHOD_PLACE_ORDERS,
            serde_json::json!({ "customerRef": "retry-safe-ref" }),
        )
        .await
        .unwrap();
    let final_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let expected = load_json_fixture("rest/betting_place_order_success.json");

    assert_eq!(result, expected["result"]);
    assert_eq!(final_count - initial_count, 2);
}

#[rstest]
#[case("TIMEOUT_ERROR", false)]
#[case("FUTURE_ERROR", false)]
#[case("UNEXPECTED_ERROR", true)]
#[tokio::test]
async fn test_send_betting_order_does_not_retry_ambiguous_api_error(
    #[case] api_error_code: &str,
    #[case] retryable: bool,
) {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().insert(
        METHOD_PLACE_ORDERS.to_string(),
        betting_api_error(api_error_code),
    );

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();
    let initial_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);

    let err = client
        .send_betting_order::<Value, _>(METHOD_PLACE_ORDERS, serde_json::json!({}))
        .await
        .expect_err("ambiguous API errors must not be retried for an order write");
    let final_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);

    assert!(err.is_order_ambiguous());
    assert_eq!(err.is_order_retryable(), retryable);
    assert_eq!(final_count - initial_count, 1);
}

#[rstest]
#[tokio::test]
async fn test_send_betting_order_does_not_retry_ambiguous_error_without_customer_ref() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), 502);

    let client = create_test_http_client(addr);
    client.connect().await.unwrap();
    let initial_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);

    let error = client
        .send_betting_order::<Value, _>(METHOD_PLACE_ORDERS, serde_json::json!({}))
        .await
        .expect_err("an ambiguous write without customerRef must not be retried");
    let final_count = state
        .betting_request_count
        .load(std::sync::atomic::Ordering::Relaxed);

    assert!(matches!(
        error,
        BetfairHttpError::UnexpectedStatus {
            status: 502,
            ref body,
        } if body.is_empty()
    ));
    assert_eq!(final_count - initial_count, 1);
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

/// Concurrent `connect()` calls must serialize under the connect lock so the
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
        "concurrent connects must serialize to a single login round-trip"
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

#[rstest]
#[tokio::test]
async fn test_cert_login_stores_session_token_and_sends_form() {
    let (addr, state) = start_mock_http().await;
    let (client, _certs) = create_cert_http_client(addr);

    client.connect().await.unwrap();

    let token = client.session_token().await.unwrap();
    assert_eq!(token.expose_secret(), "xxxsessionTokenxxx=");
    assert!(client.is_connected().await);
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    let requests = state.cert_login_requests.lock();
    assert_eq!(requests.len(), 1);
    let (headers, body) = &requests[0];
    assert_eq!(headers["X-Application"], "cert-app-key");
    assert_eq!(headers["Accept"], "application/json");
    assert_eq!(headers["Content-Type"], "application/x-www-form-urlencoded");
    assert!(!headers.contains_key("X-Authentication"));
    assert_eq!(
        body.as_ref(),
        b"username=cert%2Buser%40example.com&password=p%26ss%3Dword%2B%20%25"
    );
}

#[rstest]
#[case("CERT_AUTH_REQUIRED")]
#[case("INVALID_USERNAME_OR_PASSWORD")]
#[case("NO_ERROR")]
#[case("SOME_FUTURE_CODE")]
#[tokio::test]
async fn test_cert_login_failure_returns_status(#[case] status: &str) {
    let (addr, state) = start_mock_http().await;
    let mut response = load_json_fixture("rest/cert_login.json");
    response["loginStatus"] = Value::String(status.to_string());
    *state.login_response_override.lock() = Some(response.to_string());
    let (client, _certs) = create_cert_http_client(addr);

    let result = client.connect().await;

    assert!(
        matches!(result, Err(BetfairHttpError::LoginFailed { status: actual }) if actual == status)
    );
    assert_eq!(client.session_token().await, None);
    assert!(!client.is_connected().await);
    assert_eq!(state.cert_login_requests.lock().len(), 1);
}

#[rstest]
#[case(None)]
#[case(Some(""))]
#[case(Some(" \t"))]
#[case(Some("token\r\ninjected: value"))]
#[case(Some("token\0"))]
#[tokio::test]
async fn test_cert_login_success_with_invalid_token_fails(#[case] token: Option<&str>) {
    let (addr, state) = start_mock_http().await;
    let mut response = load_json_fixture("rest/cert_login.json");
    response["sessionToken"] = token.map_or(Value::Null, |value| Value::String(value.to_string()));
    *state.login_response_override.lock() = Some(response.to_string());
    let (client, _certs) = create_cert_http_client(addr);

    let result = client.connect().await;

    assert!(
        matches!(result, Err(BetfairHttpError::LoginFailed { status }) if status == "SUCCESS without a valid session token")
    );
    assert_eq!(client.session_token().await, None);
}

#[rstest]
#[tokio::test]
async fn test_cert_login_reconnect_refreshes_token_and_keep_alive_works() {
    let (addr, state) = start_mock_http().await;
    let (client, _certs) = create_cert_http_client(addr);
    client.connect().await.unwrap();
    let initial_token = client.session_token().await.unwrap();
    let mut response = load_json_fixture("rest/cert_login.json");
    response["sessionToken"] = Value::String("cert-replacement-token".to_string());
    *state.login_response_override.lock() = Some(response.to_string());

    client.reconnect().await.unwrap();
    let replacement_token = client.session_token().await.unwrap();
    client.keep_alive().await.unwrap();

    assert_eq!(initial_token.expose_secret(), "xxxsessionTokenxxx=");
    assert_eq!(replacement_token.expose_secret(), "cert-replacement-token");
    assert_eq!(state.cert_login_requests.lock().len(), 2);
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    assert_eq!(
        state
            .keep_alive_count
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_cert_login_concurrent_connect_stress() {
    for _ in 0..16 {
        let (addr, state) = start_mock_http().await;
        let (client, _certs) = create_cert_http_client(addr);
        let client = Arc::new(client);
        let barrier = Arc::new(tokio::sync::Barrier::new(65));
        let mut tasks = tokio::task::JoinSet::new();

        for _ in 0..64 {
            let client = Arc::clone(&client);
            let barrier = Arc::clone(&barrier);
            tasks.spawn(async move {
                barrier.wait().await;
                client.connect().await.unwrap();
                client.session_token().await.unwrap()
            });
        }

        barrier.wait().await;

        while let Some(token) = tasks.join_next().await {
            assert_eq!(token.unwrap().expose_secret(), "xxxsessionTokenxxx=");
        }

        assert_eq!(state.cert_login_requests.lock().len(), 1);
        assert_eq!(state.login_count.load(Ordering::Relaxed), 0);
    }
}

#[rstest]
#[tokio::test]
async fn test_cert_login_disconnect_waits_for_pending_login() {
    let (addr, state) = start_mock_http().await;
    let (client, _certs) = create_cert_http_client(addr);
    let client = Arc::new(client);
    let token = client.cancellation_token();

    let gate = MockResponseGate {
        method: "certlogin".to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };

    *state.identity_response_gate.lock() = Some(gate.clone());

    let connection = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });

    wait_for_mock_state(&state, "certificate login response held", |_| {
        gate.waiters.load(Ordering::Relaxed) == 1
    })
    .await;

    let disconnect = client.disconnect();
    tokio::pin!(disconnect);
    assert!(futures_util::poll!(&mut disconnect).is_pending());

    gate.semaphore.add_permits(1);
    connection.await.unwrap().unwrap();
    disconnect.await;

    assert_eq!(client.session_token().await, None);
    assert!(token.is_cancelled());
    assert!(!client.cancellation_token().is_cancelled());
    assert_eq!(state.cert_login_requests.lock().len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_cert_login_reconnect_waits_for_older_keep_alive() {
    let (addr, state) = start_mock_http().await;
    let (client, _certs) = create_cert_http_client(addr);
    let client = Arc::new(client);
    client.connect().await.unwrap();
    let mut keep_alive_response = load_json_fixture("rest/login_success.json");
    keep_alive_response["token"] = Value::String("xxxsessionTokenxxx=".to_string());
    *state.keep_alive_response_override.lock() = Some(keep_alive_response.to_string());

    let gate = MockResponseGate {
        method: "keepAlive".to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };

    *state.identity_response_gate.lock() = Some(gate.clone());

    let renewal = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.keep_alive().await }
    });

    wait_for_mock_state(&state, "keep-alive response held", |_| {
        gate.waiters.load(Ordering::Relaxed) == 1
    })
    .await;

    let mut response = load_json_fixture("rest/cert_login.json");
    response["sessionToken"] = Value::String("new-cert-token".to_string());
    *state.login_response_override.lock() = Some(response.to_string());
    let reconnect = client.reconnect();
    tokio::pin!(reconnect);
    assert!(futures_util::poll!(&mut reconnect).is_pending());
    assert_eq!(
        client
            .session_token()
            .await
            .as_ref()
            .map(|token| token.expose_secret()),
        Some("xxxsessionTokenxxx="),
    );

    gate.semaphore.add_permits(1);
    renewal.await.unwrap().unwrap();
    reconnect.await.unwrap();

    assert_eq!(
        client.session_token().await.unwrap().expose_secret(),
        "new-cert-token"
    );
    assert_eq!(state.cert_login_requests.lock().len(), 2);
    assert_eq!(state.keep_alive_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_cert_login_failed_reconnect_clears_token_then_recovers() {
    let (addr, state) = start_mock_http().await;
    let (client, _certs) = create_cert_http_client(addr);
    client.connect().await.unwrap();
    let mut response = load_json_fixture("rest/cert_login.json");
    response["loginStatus"] = Value::String("CERT_AUTH_REQUIRED".to_string());
    *state.login_response_override.lock() = Some(response.to_string());

    let result = client.reconnect().await;
    let failed_token = client.session_token().await;
    *state.login_response_override.lock() = None;
    client.connect().await.unwrap();

    assert!(
        matches!(result, Err(BetfairHttpError::LoginFailed { status }) if status == "CERT_AUTH_REQUIRED")
    );
    assert_eq!(failed_token, None);
    assert_eq!(
        client.session_token().await.unwrap().expose_secret(),
        "xxxsessionTokenxxx="
    );
    assert_eq!(state.cert_login_requests.lock().len(), 3);
}

#[rstest]
fn test_cert_login_requires_client_key() {
    let directory = tempfile::tempdir().unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    std::fs::write(directory.path().join("client.crt"), cert.pem()).unwrap();
    let credential =
        test_credential().with_certs_dir(directory.path().to_str().unwrap().to_string());

    let result = BetfairHttpClient::new(credential, Some(5), None, None, None, None, None);

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("client auth required")
    );
}

#[rstest]
#[case("invalid_json")]
#[case("missing_status")]
#[case("null_status")]
#[case("number_status")]
#[case("number_token")]
#[tokio::test]
async fn test_cert_login_malformed_response_does_not_authenticate(#[case] case: &str) {
    let (addr, state) = start_mock_http().await;
    let (client, _certs) = create_cert_http_client(addr);
    let mut response = load_json_fixture("rest/cert_login.json");

    match case {
        "missing_status" => {
            response.as_object_mut().unwrap().remove("loginStatus");
        }
        "null_status" => response["loginStatus"] = Value::Null,
        "number_status" => response["loginStatus"] = Value::from(42),
        "number_token" => response["sessionToken"] = Value::from(42),
        "invalid_json" => {}
        _ => unreachable!(),
    }

    let mut body = response.to_string();
    if case == "invalid_json" {
        body.truncate(1);
    }

    *state.login_response_override.lock() = Some(body);

    let result = client.connect().await;

    assert!(matches!(result, Err(BetfairHttpError::JsonError(_))));
    assert_eq!(client.session_token().await, None);
    assert_eq!(state.cert_login_requests.lock().len(), 1);
}

fn create_cert_http_client(addr: SocketAddr) -> (BetfairHttpClient, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    std::fs::write(directory.path().join("client.crt"), cert.pem()).unwrap();
    std::fs::write(directory.path().join("client.key"), key.serialize_pem()).unwrap();
    let credential = BetfairCredential::new(
        "cert+user@example.com".to_string(),
        "p&ss=word+ %".to_string(),
        "cert-app-key".to_string(),
    )
    .with_certs_dir(directory.path().to_str().unwrap().to_string());
    let client = BetfairHttpClient::new(credential, Some(5), Some(0), None, None, None, None)
        .unwrap()
        .with_urls(
            format!("http://{addr}/api/login"),
            format!("http://{addr}/betting"),
            format!("http://{addr}/accounts"),
            format!("http://{addr}/navigation"),
        );
    (client, directory)
}

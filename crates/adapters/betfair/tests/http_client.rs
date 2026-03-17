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

use nautilus_betfair::http::error::BetfairHttpError;
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
        .send_betting(
            "SportsAPING/v1.0/listMarketCatalogue",
            &serde_json::json!({}),
        )
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
        .send_accounts("AccountAPING/v1.0/getAccountFunds", &serde_json::json!({}))
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

#[rstest]
#[tokio::test]
async fn test_send_betting_without_session_returns_error() {
    let (addr, _state) = start_mock_http().await;
    let client = create_test_http_client(addr);

    let result: Result<Value, _> = client
        .send_betting(
            "SportsAPING/v1.0/listMarketCatalogue",
            &serde_json::json!({}),
        )
        .await;

    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), BetfairHttpError::MissingCredentials),
        "Expected MissingCredentials error"
    );
}

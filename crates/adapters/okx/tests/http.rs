// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Integration tests for the OKX HTTP client using a mock Axum server.

use std::{net::SocketAddr, path::PathBuf};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
};
use nautilus_okx::{
    common::enums::OKXInstrumentType,
    http::{client::OKXHttpInnerClient, error::OKXHttpError, query::GetInstrumentsParamsBuilder},
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct TestServerState {
    request_count: std::sync::Arc<Mutex<usize>>,
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(path).expect("failed to read test data");
    serde_json::from_str(&content).expect("failed to parse test data")
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("ok-access-key")
        && headers.contains_key("ok-access-passphrase")
        && headers.contains_key("ok-access-timestamp")
        && headers.contains_key("ok-access-sign")
}

async fn handle_get_instruments() -> impl IntoResponse {
    Json(load_test_data("http_get_instruments_spot.json"))
}

async fn handle_get_instruments_with_state(State(state): State<TestServerState>) -> Response {
    let mut count = state.request_count.lock().await;
    *count += 1;

    if *count > 3 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "code": "50116",
                "msg": "Rate limit reached",
                "data": [],
            })),
        )
            .into_response();
    }

    Json(load_test_data("http_get_instruments_spot.json")).into_response()
}

async fn handle_get_mark_price() -> impl IntoResponse {
    Json(load_test_data("http_get_mark_price.json"))
}

async fn handle_get_balance(headers: HeaderMap) -> Response {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "401",
                "msg": "Missing authentication headers",
                "data": [],
            })),
        )
            .into_response();
    }

    Json(load_test_data("http_get_account_balance.json")).into_response()
}

fn create_router(state: Option<TestServerState>) -> Router {
    if let Some(state) = state {
        Router::new()
            .route(
                "/api/v5/public/instruments",
                get(handle_get_instruments_with_state),
            )
            .route("/api/v5/public/mark-price", get(handle_get_mark_price))
            .route("/api/v5/account/balance", get(handle_get_balance))
            .with_state(state)
    } else {
        Router::new()
            .route("/api/v5/public/instruments", get(handle_get_instruments))
            .route("/api/v5/public/mark-price", get(handle_get_mark_price))
            .route("/api/v5/account/balance", get(handle_get_balance))
    }
}

async fn start_test_server(state: Option<TestServerState>) -> SocketAddr {
    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test server");
    let addr = listener.local_addr().expect("missing local addr");

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("test server failed");
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

#[rstest]
#[tokio::test]
async fn test_http_get_instruments_returns_data() {
    let addr = start_test_server(None).await;
    let base_url = format!("http://{}", addr);

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .expect("failed to build instrument params");
    let client = OKXHttpInnerClient::new(Some(base_url.clone()), Some(60), None, None, None)
        .expect("failed to create http client");

    let instruments = client
        .http_get_instruments(params)
        .await
        .expect("failed to fetch instruments");

    assert!(!instruments.is_empty());
    assert_eq!(instruments[0].inst_type, OKXInstrumentType::Spot);
}

#[rstest]
#[tokio::test]
async fn test_http_get_balance_requires_credentials() {
    let addr = start_test_server(None).await;
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::new(Some(base_url), Some(60), None, None, None)
        .expect("failed to create http client");

    let result = client.http_get_balance().await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error, got {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_balance_with_credentials_succeeds() {
    let addr = start_test_server(None).await;
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "passphrase".to_string(),
        base_url.clone(),
        Some(60),
        None,
        None,
        None,
    )
    .expect("failed to create authenticated client");

    let accounts = client
        .http_get_balance()
        .await
        .expect("expected balance response");

    assert!(!accounts.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_instruments_handles_rate_limit_error() {
    let state = TestServerState::default();
    let addr = start_test_server(Some(state.clone())).await;
    let base_url = format!("http://{}", addr);

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .expect("failed to build instrument params");
    let client = OKXHttpInnerClient::new(Some(base_url.clone()), Some(60), Some(0), None, None)
        .expect("failed to create http client");

    let mut last_error = None;
    for _ in 0..5 {
        match client.http_get_instruments(params.clone()).await {
            Ok(_) => continue,
            Err(err) => {
                last_error = Some(err);
                break;
            }
        }
    }

    match last_error.expect("expected rate limit error") {
        OKXHttpError::OkxError { error_code, .. } => assert_eq!(error_code, "50116"),
        other => panic!("expected OkxError, got {other:?}"),
    }
}

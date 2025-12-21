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

//! Integration tests for Deribit public HTTP API using a mock Axum server.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use dashmap::DashMap;
use nautilus_common::testing::wait_until_async;
use nautilus_deribit::http::{
    client::DeribitRawHttpClient,
    error::DeribitHttpError,
    models::{DeribitCurrency, DeribitInstrumentKind},
    query::{GetInstrumentParams, GetInstrumentsParams, GetLastTradesByInstrumentAndTimeParams},
};
use nautilus_network::http::HttpClient;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct TestServerState {
    request_counts: Arc<DashMap<String, usize>>,
    last_request_params: Arc<DashMap<String, serde_json::Value>>,
}

fn load_test_data(filename: &str) -> serde_json::Value {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest_dir}/test_data/{filename}");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Failed to load test data: {filename}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|_| panic!("Failed to parse test data: {filename}"))
}

async fn start_test_server(state: TestServerState) -> SocketAddr {
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    let addr = listener.local_addr().expect("Failed to get local addr");

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("Test server failed");
    });

    addr
}

async fn wait_for_server(addr: SocketAddr) {
    let health_url = format!("http://{addr}/health");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, Some(1), None).unwrap();

    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;
}

// ============================================================================
// JSON-RPC Handlers
// ============================================================================

async fn handle_jsonrpc_request(
    State(state): State<TestServerState>,
    Json(request): Json<serde_json::Value>,
) -> axum::response::Response {
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
    let params = request.get("params").cloned();

    // Track request
    state
        .request_counts
        .entry(method.to_string())
        .and_modify(|count| *count += 1)
        .or_insert(1);

    if let Some(ref params) = params {
        state
            .last_request_params
            .insert(method.to_string(), params.clone());
    }

    // Route based on method field
    match method {
        "public/get_instrument" => handle_get_instrument(id, params).await,
        "public/get_instruments" => handle_get_instruments(id, params).await,
        "public/get_last_trades_by_instrument_and_time" => handle_get_last_trades(id, params).await,
        _ => handle_method_not_found(id).await,
    }
}

async fn handle_get_instrument(id: u64, params: Option<Value>) -> axum::response::Response {
    let instrument_name = params
        .as_ref()
        .and_then(|p| p.get("instrument_name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    match instrument_name.as_deref() {
        Some("BTC-PERPETUAL") => {
            let mut data = load_test_data("http_get_instrument.json");
            data["id"] = json!(id);
            Json(data).into_response()
        }
        Some("INVALID") => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32602,
                "message": "Invalid params",
                "data": {
                    "param": "instrument_name",
                    "reason": "wrong format"
                }
            },
            "testnet": true
        }))
        .into_response(),
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": 13020,
                "message": "instrument_not_found"
            },
            "testnet": true
        }))
        .into_response(),
    }
}

async fn handle_get_instruments(id: u64, params: Option<Value>) -> axum::response::Response {
    let currency = params
        .as_ref()
        .and_then(|p| p.get("currency"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());

    let kind = params
        .as_ref()
        .and_then(|p| p.get("kind"))
        .and_then(|k| k.as_str())
        .map(|s| s.to_string());

    match currency.as_deref() {
        Some("BTC") => {
            let mut data = load_test_data("http_get_instruments.json");
            data["id"] = json!(id);

            // If kind is specified, filter the results
            if let Some(kind_str) = kind
                && let Some(result) = data.get_mut("result")
                && let Some(instruments) = result.as_array_mut()
            {
                instruments.retain(|inst| {
                    inst.get("kind")
                        .and_then(|k| k.as_str())
                        .is_some_and(|k| k == kind_str)
                });
            }

            Json(data).into_response()
        }
        Some("INVALID") => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32602,
                "message": "Invalid params",
                "data": {
                    "param": "currency",
                    "reason": "invalid currency"
                }
            },
            "testnet": true
        }))
        .into_response(),
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": [],
            "testnet": true
        }))
        .into_response(),
    }
}

async fn handle_get_last_trades(id: u64, params: Option<Value>) -> axum::response::Response {
    let instrument_name = params
        .as_ref()
        .and_then(|p| p.get("instrument_name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    match instrument_name.as_deref() {
        Some("ETH-PERPETUAL") => {
            let mut data = load_test_data("http_get_last_trades.json");
            data["id"] = json!(id);
            Json(data).into_response()
        }
        Some("INVALID") => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32602,
                "message": "Invalid params",
                "data": {
                    "param": "instrument_name",
                    "reason": "wrong format"
                }
            },
            "testnet": false
        }))
        .into_response(),
        Some("NONEXISTENT") => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": 13020,
                "message": "instrument_not_found"
            },
            "testnet": false
        }))
        .into_response(),
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "has_more": false,
                "trades": []
            },
            "testnet": false
        }))
        .into_response(),
    }
}

async fn handle_method_not_found(id: u64) -> axum::response::Response {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32601,
            "message": "Method not found"
        },
        "testnet": true
    }))
    .into_response()
}

// ============================================================================
// Router
// ============================================================================

fn create_router(state: TestServerState) -> Router {
    Router::new()
        .route("/api/v2", post(handle_jsonrpc_request))
        .route("/health", get(|| async { "OK" }))
        .with_state(state)
}

#[tokio::test]
async fn test_get_instrument_success() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client =
        DeribitRawHttpClient::new(Some(base_url), false, Some(5), None, None, None, None).unwrap();
    let params = GetInstrumentParams {
        instrument_name: "BTC-PERPETUAL".to_string(),
    };
    let result = client.get_instrument(params).await;

    assert!(result.is_ok(), "Request should succeed");
    let response = result.unwrap();
    let instrument = response.result.expect("Response should have result");

    assert_eq!(instrument.instrument_name.as_str(), "BTC-PERPETUAL");
    assert_eq!(instrument.instrument_id, 124972);
    assert_eq!(instrument.base_currency.as_str(), "BTC");
    assert_eq!(instrument.quote_currency.as_str(), "USD");
    assert_eq!(instrument.contract_size, 10.0);
    assert_eq!(instrument.tick_size, 0.5);
    assert_eq!(instrument.min_trade_amount, 10.0);
    assert!(instrument.is_active);
    assert_eq!(instrument.kind, DeribitInstrumentKind::Future);

    assert_eq!(
        *state
            .request_counts
            .get("public/get_instrument")
            .expect("Request count should be tracked"),
        1
    );
}

#[tokio::test]
async fn test_get_instrument_invalid_params() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params = GetInstrumentParams {
        instrument_name: "INVALID".to_string(),
    };
    let result = client.get_instrument(params).await;

    assert!(result.is_err());
    let err = result.unwrap_err();

    match err {
        DeribitHttpError::ValidationError(msg) => {
            assert!(msg.contains("Invalid params"));
            assert!(msg.contains("instrument_name"));
        }
        other => panic!("Expected ValidationError, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_get_instrument_not_found() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params = GetInstrumentParams {
        instrument_name: "NONEXISTENT-INSTRUMENT".to_string(),
    };
    let result = client.get_instrument(params).await;

    assert!(result.is_err());
    let err = result.unwrap_err();

    match err {
        DeribitHttpError::DeribitError {
            error_code,
            message,
        } => {
            assert_eq!(error_code, 13020);
            assert!(message.contains("instrument_not_found"));
        }
        other => panic!("Expected DeribitError with code 13020, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_get_instruments_success() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params = GetInstrumentsParams::new(DeribitCurrency::BTC);
    let result = client.get_instruments(params).await;

    assert!(result.is_ok(), "Request should succeed");
    let response = result.unwrap();
    let instruments = response.result.expect("Response should have result");

    assert_eq!(instruments.len(), 4, "Should return 4 instruments");

    let perpetual = &instruments[0];
    assert_eq!(perpetual.instrument_name.as_str(), "BTC-PERPETUAL");
    assert_eq!(perpetual.kind, DeribitInstrumentKind::Future);
    assert_eq!(perpetual.base_currency.as_str(), "BTC");
    assert!(perpetual.is_active);

    let future = &instruments[1];
    assert_eq!(future.instrument_name.as_str(), "BTC-27DEC24");
    assert_eq!(future.kind, DeribitInstrumentKind::Future);
    assert_eq!(future.expiration_timestamp, Some(1735300800000));

    let option = &instruments[2];
    assert_eq!(option.instrument_name.as_str(), "BTC-27DEC24-100000-C");
    assert_eq!(option.kind, DeribitInstrumentKind::Option);
    assert_eq!(option.strike, Some(100000.0));

    let combo = &instruments[3];
    assert_eq!(combo.instrument_name.as_str(), "BTC-COMBO-1");
    assert_eq!(combo.kind, DeribitInstrumentKind::FutureCombo);

    assert_eq!(
        *state
            .request_counts
            .get("public/get_instruments")
            .expect("Request count should be tracked"),
        1
    );
}

#[tokio::test]
async fn test_get_instruments_with_kind_filter() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params =
        GetInstrumentsParams::with_kind(DeribitCurrency::BTC, DeribitInstrumentKind::Option);
    let result = client.get_instruments(params).await;

    assert!(result.is_ok(), "Request should succeed");
    let response = result.unwrap();
    let instruments = response.result.expect("Response should have result");

    assert_eq!(instruments.len(), 1);

    let option = &instruments[0];
    assert_eq!(option.instrument_name.as_str(), "BTC-27DEC24-100000-C");
    assert_eq!(option.kind, DeribitInstrumentKind::Option);
}

#[tokio::test]
async fn test_get_instruments_empty_result() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params = GetInstrumentsParams::new(DeribitCurrency::ETH);
    let result = client.get_instruments(params).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    let instruments = response.result.expect("Response should have result");
    assert_eq!(instruments.len(), 0);
}

#[tokio::test]
async fn test_get_last_trades_success() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params = GetLastTradesByInstrumentAndTimeParams::new(
        "ETH-PERPETUAL",
        1766332000000, // start_timestamp
        1766332100000, // end_timestamp
        Some(10),      // count
        Some("asc".to_string()),
    );
    let result = client.get_last_trades_by_instrument_and_time(params).await;

    assert!(result.is_ok(), "Request should succeed");
    let response = result.unwrap();
    let trades_response = response.result.expect("Response should have result");

    assert!(trades_response.has_more, "has_more should be true");
    assert_eq!(trades_response.trades.len(), 10, "Should return 10 trades");

    // Verify first trade
    let first_trade = &trades_response.trades[0];
    assert_eq!(first_trade.instrument_name, "ETH-PERPETUAL");
    assert_eq!(first_trade.direction, "sell");
    assert_eq!(first_trade.price, 2968.3);
    assert_eq!(first_trade.amount, 1.0);
    assert_eq!(first_trade.trade_id, "ETH-284830839");
    assert_eq!(first_trade.trade_seq, 203024587);
    assert_eq!(first_trade.tick_direction, 0);
    assert_eq!(first_trade.index_price, 2967.73);
    assert_eq!(first_trade.mark_price, 2968.01);

    // Verify last trade (buy order with larger size)
    let last_trade = &trades_response.trades[9];
    assert_eq!(last_trade.direction, "buy");
    assert_eq!(last_trade.amount, 106.0);
    assert_eq!(last_trade.trade_id, "ETH-284830854");

    // Verify request was tracked
    assert_eq!(
        *state
            .request_counts
            .get("public/get_last_trades_by_instrument_and_time")
            .expect("Request count should be tracked"),
        1
    );

    // Verify params were captured correctly
    let captured_params = state
        .last_request_params
        .get("public/get_last_trades_by_instrument_and_time")
        .expect("Params should be captured");
    assert_eq!(
        captured_params.get("instrument_name").unwrap().as_str(),
        Some("ETH-PERPETUAL")
    );
    assert_eq!(
        captured_params.get("start_timestamp").unwrap().as_i64(),
        Some(1766332000000)
    );
    assert_eq!(
        captured_params.get("end_timestamp").unwrap().as_i64(),
        Some(1766332100000)
    );
}

#[tokio::test]
async fn test_get_last_trades_invalid_params() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params = GetLastTradesByInstrumentAndTimeParams::new(
        "INVALID",
        1766332000000,
        1766332100000,
        None,
        None,
    );
    let result = client.get_last_trades_by_instrument_and_time(params).await;

    assert!(result.is_err());
    let err = result.unwrap_err();

    match err {
        DeribitHttpError::ValidationError(msg) => {
            assert!(msg.contains("Invalid params"));
            assert!(msg.contains("instrument_name"));
        }
        other => panic!("Expected ValidationError, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_get_last_trades_instrument_not_found() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new(
        Some(base_url),
        false,   // is_testnet
        Some(5), // timeout_secs
        None,    // max_retries
        None,    // retry_delay_ms
        None,    // retry_delay_max_ms
        None,    // proxy_url
    )
    .unwrap();

    let params = GetLastTradesByInstrumentAndTimeParams::new(
        "NONEXISTENT",
        1766332000000,
        1766332100000,
        None,
        None,
    );
    let result = client.get_last_trades_by_instrument_and_time(params).await;

    assert!(result.is_err());
    let err = result.unwrap_err();

    match err {
        DeribitHttpError::DeribitError {
            error_code,
            message,
        } => {
            assert_eq!(error_code, 13020);
            assert!(message.contains("instrument_not_found"));
        }
        other => panic!("Expected DeribitError with code 13020, got: {other:?}"),
    }
}

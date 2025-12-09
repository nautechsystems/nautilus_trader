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

//! Integration tests for the Deribit HTTP client using a mock Axum server.

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
    query::{GetInstrumentParams, GetInstrumentsParams},
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
    let http_client = HttpClient::new(
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        None,
        Some(1), // 1 second timeout
        None,
    )
    .unwrap();

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
    // Extract JSON-RPC method from request body
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
            data["id"] = json!(id); // Match request ID
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
            data["id"] = json!(id); // Match request ID

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
        // Single POST endpoint for all JSON-RPC requests
        .route("/api/v2", post(handle_jsonrpc_request))
        // Health check for server readiness
        .route("/health", get(|| async { "OK" }))
        .with_state(state)
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_get_instrument_success() {
    // Setup
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    // Create client pointing to test server
    // Note: client.send_request() POSTs directly to base_url
    // The /public is part of the method name (public/get_instrument), not the URL
    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new_with_base_url(base_url, Some(5)).unwrap();

    // Execute
    let params = GetInstrumentParams {
        instrument_name: "BTC-PERPETUAL".to_string(),
    };
    let result = client.get_instrument(params).await;

    assert!(result.is_ok(), "Request should succeed");
    let instrument = result.unwrap();

    assert_eq!(instrument.instrument_name.as_str(), "BTC-PERPETUAL");
    assert_eq!(instrument.instrument_id, 124972);
    assert_eq!(instrument.base_currency.as_str(), "BTC");
    assert_eq!(instrument.quote_currency.as_str(), "USD");
    assert_eq!(instrument.contract_size, 10.0);
    assert_eq!(instrument.tick_size, 0.5);
    assert_eq!(instrument.min_trade_amount, 10.0);
    assert!(instrument.is_active);
    assert_eq!(instrument.kind, DeribitInstrumentKind::Future);

    // Verify state tracking
    assert_eq!(
        *state
            .request_counts
            .get("public/get_instrument")
            .expect("Request count should be tracked"),
        1,
        "Should track request count"
    );

    let last_params = state
        .last_request_params
        .get("public/get_instrument")
        .expect("Request params should be tracked")
        .clone();
    assert_eq!(
        last_params
            .get("instrument_name")
            .unwrap()
            .as_str()
            .unwrap(),
        "BTC-PERPETUAL"
    );
}

#[tokio::test]
async fn test_get_instrument_invalid_params() {
    // Setup
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new_with_base_url(base_url, Some(5)).unwrap();

    // Test with invalid instrument name
    let params = GetInstrumentParams {
        instrument_name: "INVALID".to_string(),
    };
    let result = client.get_instrument(params).await;

    // Should fail with ValidationError
    assert!(result.is_err());
    let err = result.unwrap_err();

    match err {
        DeribitHttpError::ValidationError(msg) => {
            assert!(
                msg.contains("Invalid params"),
                "Error message should contain 'Invalid params': {msg}"
            );
            assert!(
                msg.contains("instrument_name"),
                "Error should mention parameter name: {msg}"
            );
        }
        other => panic!("Expected ValidationError, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_get_instrument_not_found() {
    // Setup
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new_with_base_url(base_url, Some(5)).unwrap();

    // Test with nonexistent instrument
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
            assert_eq!(error_code, 13020, "Should be instrument_not_found error");
            assert!(message.contains("instrument_not_found"));
        }
        other => panic!("Expected DeribitError with code 13020, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_get_instruments_success() {
    // Setup
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new_with_base_url(base_url, Some(5)).unwrap();

    // Execute - get all BTC instruments
    let params = GetInstrumentsParams::new(DeribitCurrency::BTC);
    let result = client.get_instruments(params).await;

    assert!(result.is_ok(), "Request should succeed");
    let instruments = result.unwrap();

    // Should return 4 instruments (perpetual, future, option, combo)
    assert_eq!(instruments.len(), 4, "Should return 4 instruments");

    // Verify first instrument (BTC-PERPETUAL)
    let perpetual = &instruments[0];
    assert_eq!(perpetual.instrument_name.as_str(), "BTC-PERPETUAL");
    assert_eq!(perpetual.kind, DeribitInstrumentKind::Future);
    assert_eq!(perpetual.base_currency.as_str(), "BTC");
    assert!(perpetual.is_active);

    // Verify second instrument (BTC-27DEC24 future)
    let future = &instruments[1];
    assert_eq!(future.instrument_name.as_str(), "BTC-27DEC24");
    assert_eq!(future.kind, DeribitInstrumentKind::Future);
    assert_eq!(future.expiration_timestamp, Some(1735300800000));

    // Verify third instrument (option)
    let option = &instruments[2];
    assert_eq!(option.instrument_name.as_str(), "BTC-27DEC24-100000-C");
    assert_eq!(option.kind, DeribitInstrumentKind::Option);
    assert_eq!(option.strike, Some(100000.0));

    // Verify fourth instrument (combo)
    let combo = &instruments[3];
    assert_eq!(combo.instrument_name.as_str(), "BTC-COMBO-1");
    assert_eq!(combo.kind, DeribitInstrumentKind::FutureCombo);

    // Verify state tracking
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
    // Setup
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new_with_base_url(base_url, Some(5)).unwrap();

    // Execute - get only option instruments
    let params =
        GetInstrumentsParams::with_kind(DeribitCurrency::BTC, DeribitInstrumentKind::Option);
    let result = client.get_instruments(params).await;

    assert!(result.is_ok(), "Request should succeed");
    let instruments = result.unwrap();

    // Should return only 1 option instrument
    assert_eq!(
        instruments.len(),
        1,
        "Should return only 1 option instrument"
    );

    let option = &instruments[0];
    assert_eq!(option.instrument_name.as_str(), "BTC-27DEC24-100000-C");
    assert_eq!(option.kind, DeribitInstrumentKind::Option);
}

#[tokio::test]
async fn test_get_instruments_empty_result() {
    // Setup
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::new_with_base_url(base_url, Some(5)).unwrap();

    // Execute - request instruments for ETH (not in our test data)
    let params = GetInstrumentsParams::new(DeribitCurrency::ETH);
    let result = client.get_instruments(params).await;

    assert!(
        result.is_ok(),
        "Request should succeed even with empty result"
    );
    let instruments = result.unwrap();
    assert_eq!(instruments.len(), 0, "Should return empty array");
}

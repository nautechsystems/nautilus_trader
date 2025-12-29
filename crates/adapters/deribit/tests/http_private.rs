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

//! Integration tests for Deribit private HTTP API using a mock Axum server.

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
    client::DeribitRawHttpClient, error::DeribitHttpError, query::GetAccountSummariesParams,
};
use nautilus_network::http::{HttpClient, Method};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct TestServerState {
    request_counts: Arc<DashMap<String, usize>>,
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
    headers: axum::http::HeaderMap,
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

    // Route based on method field
    match method {
        "private/get_account_summaries" => handle_get_account_summaries(id, params, headers).await,
        _ => handle_method_not_found(id).await,
    }
}

async fn handle_get_account_summaries(
    id: u64,
    _params: Option<Value>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    // Verify Authorization header exists
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok());

    if auth_header.is_none() {
        return Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": 10000,
                "message": "authorization_required"
            },
            "testnet": true
        }))
        .into_response();
    }

    // Verify the header has content
    let auth_value = auth_header.unwrap();

    if auth_value.is_empty() {
        return Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": 13004,
                "message": "invalid_credentials"
            },
            "testnet": true
        }))
        .into_response();
    }

    // Return mock account summaries
    let mut data = load_test_data("http_get_account_summaries.json");
    data["id"] = json!(id);
    Json(data).into_response()
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
async fn test_get_account_summaries_success() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    let base_url = format!("http://{addr}/api/v2");
    let client = DeribitRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        false,
        Some(5),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let params = GetAccountSummariesParams::default();
    let result = client.get_account_summaries(params).await;

    assert!(result.is_ok(), "Request should succeed: {result:?}");
    let response = result.unwrap();
    let response_data = response.result.expect("Response should have result");
    let summaries = &response_data.summaries;

    assert_eq!(summaries.len(), 2, "Should return 2 account summaries");

    // Verify BTC summary
    let btc = &summaries[0];
    assert_eq!(btc.currency.as_str(), "BTC");
    assert_eq!(btc.equity, 302.61869214);
    assert_eq!(btc.balance, 302.60065765);
    assert_eq!(btc.available_funds, 301.38059622);
    assert_eq!(btc.margin_balance, 302.62729214);
    assert_eq!(btc.initial_margin, Some(1.24669592));
    assert_eq!(btc.maintenance_margin, Some(0.8857841));
    assert_eq!(btc.total_pl, Some(-0.33084225));

    // Verify ETH summary
    let eth = &summaries[1];
    assert_eq!(eth.currency.as_str(), "ETH");
    assert_eq!(eth.equity, 100.0);
    assert_eq!(eth.balance, 100.0);

    assert_eq!(
        *state
            .request_counts
            .get("private/get_account_summaries")
            .expect("Request count should be tracked"),
        1
    );
}

#[tokio::test]
async fn test_get_account_summaries_missing_credentials() {
    let base_url = "http://127.0.0.1:0/api/v2".to_string();
    let client =
        DeribitRawHttpClient::new(Some(base_url), false, Some(5), None, None, None, None).unwrap();

    let params = GetAccountSummariesParams::default();
    let result = client.get_account_summaries(params).await;

    assert!(result.is_err(), "Request should fail without credentials");
    match result.unwrap_err() {
        DeribitHttpError::MissingCredentials => {
            // Expected error type
        }
        other => panic!("Expected MissingCredentials, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_get_account_summaries_authorization_required() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    wait_for_server(addr).await;

    // Create a custom HTTP request without authentication
    let base_url = format!("http://{addr}/api/v2");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, Some(5), None).unwrap();

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "private/get_account_summaries",
        "params": {
            "currency": "all",
            "extended": false
        }
    });

    let body_bytes = serde_json::to_vec(&body).unwrap();
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    let response = http_client
        .request(
            Method::POST,
            base_url,
            None,
            Some(headers),
            Some(body_bytes),
            None,
            None,
        )
        .await
        .unwrap();

    // Parse JSON-RPC response
    let json_resp: Value = serde_json::from_slice(&response.body).unwrap();

    // Verify error response
    assert!(json_resp.get("error").is_some(), "Should have error field");
    let error = json_resp.get("error").unwrap();
    assert_eq!(
        error.get("code").unwrap().as_i64().unwrap(),
        10000,
        "Should return authorization_required error code"
    );
    assert_eq!(
        error.get("message").unwrap().as_str().unwrap(),
        "authorization_required"
    );
}

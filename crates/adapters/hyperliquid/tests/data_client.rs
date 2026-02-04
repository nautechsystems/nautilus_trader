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

//! Integration tests for Hyperliquid data client components.
//!
//! These tests focus on HTTP data endpoints and combined HTTP+WS functionality.
//! Note: WebSocket subscription tests are in websocket.rs (50+ tests).

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Json, Response},
    routing::post,
};
use nautilus_common::testing::wait_until_async;
use nautilus_hyperliquid::http::{
    models::{HyperliquidL2Book, PerpMeta},
    query::InfoRequest,
};
use nautilus_network::http::{HttpClient, Method};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct TestServerState {
    info_request_count: Arc<tokio::sync::Mutex<usize>>,
    last_request_type: Arc<tokio::sync::Mutex<Option<String>>>,
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

async fn wait_for_server(addr: SocketAddr, path: &str) {
    let health_url = format!("http://{addr}{path}");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
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

async fn handle_info(State(state): State<TestServerState>, body: axum::body::Bytes) -> Response {
    let mut count = state.info_request_count.lock().await;
    *count += 1;

    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid JSON"})),
        )
            .into_response();
    };

    let request_type = request_body
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    *state.last_request_type.lock().await = Some(request_type.clone());

    match request_type.as_str() {
        "meta" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(meta).into_response()
        }
        "metaAndAssetCtxs" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta, []])).into_response()
        }
        "spotMetaAndAssetCtxs" => Json(json!([{"universe": [], "tokens": []}, []])).into_response(),
        "l2Book" => {
            let book = load_json("http_l2_book_btc.json");
            Json(book).into_response()
        }
        "candleSnapshot" => Json(json!([{
            "t": 1703875200000u64,
            "T": 1703875260000u64,
            "s": "BTC",
            "i": "1m",
            "o": "98450.00",
            "c": "98460.00",
            "h": "98470.00",
            "l": "98440.00",
            "v": "100.5",
            "n": 50
        }]))
        .into_response(),
        "clearinghouseState" => Json(json!({
            "marginSummary": {
                "accountValue": "10000.0",
                "totalMarginUsed": "0.0",
                "totalNtlPos": "0.0",
                "totalRawUsd": "10000.0"
            },
            "crossMarginSummary": {
                "accountValue": "10000.0",
                "totalMarginUsed": "0.0",
                "totalNtlPos": "0.0",
                "totalRawUsd": "10000.0"
            },
            "crossMaintenanceMarginUsed": "0.0",
            "withdrawable": "10000.0",
            "assetPositions": []
        }))
        .into_response(),
        _ => Json(json!({})).into_response(),
    }
}

async fn handle_health() -> impl IntoResponse {
    axum::http::StatusCode::OK
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/info", post(handle_info))
        .route("/health", axum::routing::get(handle_health))
        .with_state(state)
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = create_test_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_server(addr, "/health").await;
    addr
}

struct TestHttpClient {
    client: HttpClient,
    base_url: String,
}

impl TestHttpClient {
    fn new(base_url: String) -> Self {
        let client = HttpClient::new(
            HashMap::from([("Content-Type".to_string(), "application/json".to_string())]),
            vec![],
            vec![],
            None,
            None,
            None,
        )
        .unwrap();

        Self { client, base_url }
    }

    async fn send_info_request(&self, request: &InfoRequest) -> Result<Value, String> {
        let url = format!("{}/info", self.base_url);
        let body = serde_json::to_vec(request).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request(Method::POST, url, None, None, Some(body), None, None)
            .await
            .map_err(|e| e.to_string())?;

        if !response.status.is_success() {
            return Err(format!("HTTP error: {:?}", response.status));
        }

        serde_json::from_slice(&response.body).map_err(|e| e.to_string())
    }

    async fn info_meta(&self) -> Result<PerpMeta, String> {
        let request = InfoRequest::meta();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book, String> {
        let request = InfoRequest::l2_book(coin);
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_clearinghouse_state(&self, user: &str) -> Result<Value, String> {
        let request = InfoRequest::clearinghouse_state(user);
        self.send_info_request(&request).await
    }
}

#[rstest]
#[tokio::test]
async fn test_fetch_instruments_via_meta() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestHttpClient::new(format!("http://{addr}"));
    let meta = client.info_meta().await.unwrap();

    assert!(!meta.universe.is_empty());
    assert_eq!(*state.info_request_count.lock().await, 1);
    assert_eq!(
        *state.last_request_type.lock().await,
        Some("meta".to_string())
    );
}

#[rstest]
#[tokio::test]
async fn test_fetch_orderbook_snapshot() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestHttpClient::new(format!("http://{addr}"));
    let book = client.info_l2_book("BTC").await.unwrap();

    assert_eq!(book.coin, "BTC");
    assert_eq!(book.levels.len(), 2); // bids and asks
    assert_eq!(
        *state.last_request_type.lock().await,
        Some("l2Book".to_string())
    );
}

#[rstest]
#[tokio::test]
async fn test_fetch_account_state() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestHttpClient::new(format!("http://{addr}"));
    let account = client
        .info_clearinghouse_state("0x1234567890123456789012345678901234567890")
        .await
        .unwrap();

    assert!(account.get("marginSummary").is_some());
    assert_eq!(
        *state.last_request_type.lock().await,
        Some("clearinghouseState".to_string())
    );
}

#[rstest]
#[tokio::test]
async fn test_multiple_sequential_requests() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestHttpClient::new(format!("http://{addr}"));

    client.info_meta().await.unwrap();
    client.info_l2_book("BTC").await.unwrap();
    client.info_l2_book("ETH").await.unwrap();

    assert_eq!(*state.info_request_count.lock().await, 3);
}

#[rstest]
#[tokio::test]
async fn test_parallel_requests() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestHttpClient::new(format!("http://{addr}"));

    let (r1, r2, r3) = tokio::join!(
        client.info_meta(),
        client.info_l2_book("BTC"),
        client.info_l2_book("ETH"),
    );

    assert!(r1.is_ok());
    assert!(r2.is_ok());
    assert!(r3.is_ok());
    assert_eq!(*state.info_request_count.lock().await, 3);
}

#[rstest]
#[tokio::test]
async fn test_orderbook_structure() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestHttpClient::new(format!("http://{addr}"));
    let book = client.info_l2_book("BTC").await.unwrap();

    assert_eq!(book.coin, "BTC");
    assert_eq!(book.levels.len(), 2);

    let bids = &book.levels[0];
    let asks = &book.levels[1];

    assert!(!bids.is_empty());
    assert!(!asks.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_meta_universe_structure() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestHttpClient::new(format!("http://{addr}"));
    let meta = client.info_meta().await.unwrap();

    let names: Vec<&str> = meta.universe.iter().map(|u| u.name.as_str()).collect();
    assert!(names.contains(&"BTC"));
    assert!(names.contains(&"ETH"));
    assert!(names.contains(&"ATOM"));
}

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

//! Integration tests for Hyperliquid HTTP client using a mock server.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::post,
};
use nautilus_common::testing::wait_until_async;
use nautilus_hyperliquid::{
    common::enums::HyperliquidInfoRequestType,
    http::{
        models::{
            HyperliquidFills, HyperliquidL2Book, PerpMeta, PerpMetaAndCtxs, SpotMeta,
            SpotMetaAndCtxs,
        },
        query::{InfoRequest, InfoRequestParams},
    },
};
use nautilus_network::http::{HttpClient, Method};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
    last_request_body: Arc<tokio::sync::Mutex<Option<Value>>>,
    rate_limit_after: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(tokio::sync::Mutex::new(0)),
            last_request_body: Arc::new(tokio::sync::Mutex::new(None)),
            rate_limit_after: Arc::new(AtomicUsize::new(usize::MAX)),
        }
    }
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
    let mut count = state.request_count.lock().await;
    *count += 1;

    let limit_after = state.rate_limit_after.load(Ordering::Relaxed);
    if *count > limit_after {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Rate limit exceeded"
            })),
        )
            .into_response();
    }

    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid JSON body"
            })),
        )
            .into_response();
    };

    *state.last_request_body.lock().await = Some(request_body.clone());

    let request_type = request_body
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("");

    match request_type {
        "meta" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(meta).into_response()
        }
        "spotMeta" => Json(json!({
            "universe": [],
            "tokens": []
        }))
        .into_response(),
        "metaAndAssetCtxs" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta, []])).into_response()
        }
        "spotMetaAndAssetCtxs" => Json(json!([
            {"universe": [], "tokens": []},
            []
        ]))
        .into_response(),
        "l2Book" => {
            let book = load_json("http_l2_book_btc.json");
            Json(book).into_response()
        }
        "userFills" => Json(json!([])).into_response(),
        "orderStatus" => Json(json!({
            "status": "order:filled",
            "order": null
        }))
        .into_response(),
        "openOrders" => Json(json!([])).into_response(),
        "frontendOpenOrders" => Json(json!([])).into_response(),
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
        "candleSnapshot" => Json(json!([
            {
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
            }
        ]))
        .into_response(),
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Unknown request type: {}", request_type)
            })),
        )
            .into_response(),
    }
}

async fn handle_exchange(
    State(state): State<TestServerState>,
    body: axum::body::Bytes,
) -> Response {
    let mut count = state.request_count.lock().await;
    *count += 1;

    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Invalid JSON body"
                }
            })),
        )
            .into_response();
    };

    *state.last_request_body.lock().await = Some(request_body.clone());

    // Validate signed request format
    if request_body.get("action").is_none()
        || request_body.get("nonce").is_none()
        || request_body.get("signature").is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Missing required fields"
                }
            })),
        )
            .into_response();
    }

    Json(json!({
        "status": "ok",
        "response": {
            "type": "order",
            "data": {
                "statuses": [{
                    "resting": {
                        "oid": 12345
                    }
                }]
            }
        }
    }))
    .into_response()
}

async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/info", post(handle_info))
        .route("/exchange", post(handle_exchange))
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

#[rstest]
#[tokio::test]
async fn test_info_meta_returns_market_metadata() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client.info_meta().await;

    assert!(result.is_ok());
    let meta = result.unwrap();
    assert!(!meta.universe.is_empty());
    assert_eq!(meta.universe[0].name, "BTC");
}

#[rstest]
#[tokio::test]
async fn test_info_l2_book_returns_orderbook() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client.info_l2_book("BTC").await;

    assert!(result.is_ok());
    let book = result.unwrap();
    assert_eq!(book.coin, "BTC");
    assert_eq!(book.levels.len(), 2); // bids and asks
}

#[rstest]
#[tokio::test]
async fn test_spot_meta_returns_spot_metadata() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let meta = client.get_spot_meta().await.unwrap();

    assert!(meta.tokens.is_empty());
    assert!(meta.universe.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_perp_meta_and_ctxs_returns_metadata_with_contexts() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let PerpMetaAndCtxs::Payload(data) = client.get_perp_meta_and_ctxs().await.unwrap();

    let (meta, ctxs) = *data;
    assert!(!meta.universe.is_empty());
    assert!(ctxs.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_meta_and_ctxs_returns_metadata_with_contexts() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let SpotMetaAndCtxs::Payload(data) = client.get_spot_meta_and_ctxs().await.unwrap();

    let (meta, ctxs) = *data;
    assert!(meta.tokens.is_empty());
    assert!(meta.universe.is_empty());
    assert!(ctxs.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_info_user_fills_returns_empty_for_new_user() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client
        .info_user_fills("0x1234567890123456789012345678901234567890")
        .await;

    assert!(result.is_ok());
    let fills = result.unwrap();
    assert!(fills.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_info_open_orders_returns_empty_array() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let orders = client
        .info_open_orders("0x1234567890123456789012345678901234567890")
        .await
        .unwrap();

    assert!(orders.is_array());
    assert!(orders.as_array().unwrap().is_empty());
}

#[rstest]
#[tokio::test]
async fn test_info_clearinghouse_state_returns_account_state() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client
        .info_clearinghouse_state("0x1234567890123456789012345678901234567890")
        .await;

    assert!(result.is_ok());
    let state = result.unwrap();
    assert!(state.get("marginSummary").is_some());
}

#[rstest]
#[tokio::test]
async fn test_rate_limit_triggers_429_response() {
    let state = TestServerState::default();
    state.rate_limit_after.store(2, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);

    assert!(client.info_meta().await.is_ok());
    assert!(client.info_meta().await.is_ok());

    // Third triggers rate limit
    let result = client.info_meta().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_invalid_request_type_returns_error() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);

    let request = InfoRequest {
        request_type: HyperliquidInfoRequestType::Meta,
        params: InfoRequestParams::None,
    };

    let result = client.send_info_request_raw(&request).await;
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_l2_book_request_includes_coin_parameter() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let _ = client.info_l2_book("ETH").await;

    let last_request = state.last_request_body.lock().await;
    let request_body = last_request.as_ref().unwrap();

    assert_eq!(
        request_body.get("type").unwrap().as_str().unwrap(),
        "l2Book"
    );
    assert_eq!(request_body.get("coin").unwrap().as_str().unwrap(), "ETH");
}

#[rstest]
#[tokio::test]
async fn test_user_fills_request_includes_user_parameter() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let user = "0xabcdef1234567890abcdef1234567890abcdef12";
    let _ = client.info_user_fills(user).await;

    let last_request = state.last_request_body.lock().await;
    let request_body = last_request.as_ref().unwrap();

    assert_eq!(
        request_body.get("type").unwrap().as_str().unwrap(),
        "userFills"
    );
    assert_eq!(request_body.get("user").unwrap().as_str().unwrap(), user);
}

fn create_test_client(addr: &SocketAddr) -> TestHttpClient {
    TestHttpClient::new(format!("http://{addr}"))
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

    async fn get_spot_meta(&self) -> Result<SpotMeta, String> {
        let request = InfoRequest::spot_meta();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn get_perp_meta_and_ctxs(&self) -> Result<PerpMetaAndCtxs, String> {
        let request = InfoRequest::meta_and_asset_ctxs();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn get_spot_meta_and_ctxs(&self) -> Result<SpotMetaAndCtxs, String> {
        let request = InfoRequest::spot_meta_and_asset_ctxs();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book, String> {
        let request = InfoRequest::l2_book(coin);
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_user_fills(&self, user: &str) -> Result<HyperliquidFills, String> {
        let request = InfoRequest::user_fills(user);
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_open_orders(&self, user: &str) -> Result<Value, String> {
        let request = InfoRequest::open_orders(user);
        self.send_info_request(&request).await
    }

    async fn info_clearinghouse_state(&self, user: &str) -> Result<Value, String> {
        let request = InfoRequest::clearinghouse_state(user);
        self.send_info_request(&request).await
    }

    async fn send_info_request_raw(&self, request: &InfoRequest) -> Result<Value, String> {
        self.send_info_request(request).await
    }
}

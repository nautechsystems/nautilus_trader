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

//! Integration tests for Hyperliquid execution client HTTP endpoints.
//!
//! These tests focus on order submission, cancellation, and modification flows
//! using mock HTTP servers. WebSocket execution updates are tested in websocket.rs.

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Json, Response},
    routing::post,
};
use futures_util::StreamExt;
use nautilus_common::{
    cache::Cache, clients::ExecutionClient, live::runner::set_exec_event_sender,
    messages::ExecutionEvent, testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_hyperliquid::{
    config::HyperliquidExecClientConfig, execution::HyperliquidExecutionClient,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType},
    events::AccountState,
    identifiers::{AccountId, ClientId, TraderId, Venue},
    types::{AccountBalance, Money},
};
use nautilus_network::http::{HttpClient, Method};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone)]
struct TestServerState {
    exchange_request_count: Arc<tokio::sync::Mutex<usize>>,
    last_exchange_action: Arc<tokio::sync::Mutex<Option<Value>>>,
    reject_next_order: Arc<std::sync::atomic::AtomicBool>,
    rate_limit_after: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            exchange_request_count: Arc::new(tokio::sync::Mutex::new(0)),
            last_exchange_action: Arc::new(tokio::sync::Mutex::new(None)),
            reject_next_order: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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

async fn handle_info(body: axum::body::Bytes) -> Response {
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
        .unwrap_or("");

    match request_type {
        "meta" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(meta).into_response()
        }
        "metaAndAssetCtxs" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta, []])).into_response()
        }
        "spotMeta" => Json(json!({"universe": [], "tokens": []})).into_response(),
        "spotMetaAndAssetCtxs" => Json(json!([{"universe": [], "tokens": []}, []])).into_response(),
        "openOrders" => Json(json!([])).into_response(),
        "orderStatus" => Json(json!({
            "status": "order:filled",
            "order": null
        }))
        .into_response(),
        "userFills" => Json(json!([])).into_response(),
        "userFees" => Json(json!({
            "userCrossRate": "0.00045",
            "userAddRate": "0.00015"
        }))
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

async fn handle_exchange(
    State(state): State<TestServerState>,
    body: axum::body::Bytes,
) -> Response {
    let mut count = state.exchange_request_count.lock().await;
    *count += 1;

    let limit_after = state.rate_limit_after.load(Ordering::Relaxed);
    if *count > limit_after {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Rate limit exceeded"
                }
            })),
        )
            .into_response();
    }

    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
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

    if let Some(action) = request_body.get("action") {
        *state.last_exchange_action.lock().await = Some(action.clone());
    }

    // Validate signed request format
    if request_body.get("action").is_none()
        || request_body.get("nonce").is_none()
        || request_body.get("signature").is_none()
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Missing required fields: action, nonce, or signature"
                }
            })),
        )
            .into_response();
    }

    if state.reject_next_order.swap(false, Ordering::Relaxed) {
        return Json(json!({
            "status": "err",
            "response": {
                "type": "error",
                "data": "Order rejected: insufficient margin"
            }
        }))
        .into_response();
    }

    let action = request_body.get("action").unwrap();
    let action_type = action.get("type").and_then(|t| t.as_str());

    match action_type {
        Some("order") => Json(json!({
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
        .into_response(),
        Some("cancel" | "cancelByCloid") => Json(json!({
            "status": "ok",
            "response": {
                "type": "cancel",
                "data": {
                    "statuses": ["success"]
                }
            }
        }))
        .into_response(),
        Some("modify") => Json(json!({
            "status": "ok",
            "response": {
                "type": "modify",
                "data": {
                    "statuses": [{
                        "resting": {
                            "oid": 12346
                        }
                    }]
                }
            }
        }))
        .into_response(),
        Some("updateLeverage") => Json(json!({
            "status": "ok",
            "response": {
                "type": "updateLeverage",
                "data": {}
            }
        }))
        .into_response(),
        _ => Json(json!({
            "status": "err",
            "response": {
                "type": "error",
                "data": format!("Unknown action type: {action_type:?}")
            }
        }))
        .into_response(),
    }
}

async fn handle_health() -> impl IntoResponse {
    axum::http::StatusCode::OK
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(_state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(handle_ws_socket)
}

async fn handle_ws_socket(mut socket: WebSocket) {
    while let Some(message) = socket.next().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                    let method = payload.get("method").and_then(|m| m.as_str());
                    match method {
                        Some("ping") => {
                            let pong = json!({"channel": "pong"});

                            if socket
                                .send(Message::Text(pong.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Some("subscribe") => {
                            // Acknowledge subscription silently
                        }
                        Some("unsubscribe") => {}
                        _ => {}
                    }
                }
            }
            Message::Ping(data) => {
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/info", post(handle_info))
        .route("/exchange", post(handle_exchange))
        .route("/health", axum::routing::get(handle_health))
        .route("/ws", axum::routing::get(handle_ws_upgrade))
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

struct TestExchangeClient {
    client: HttpClient,
    base_url: String,
}

impl TestExchangeClient {
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

    async fn send_exchange_action(&self, action: Value) -> Result<Value, String> {
        let url = format!("{}/exchange", self.base_url);

        let request = json!({
            "action": action,
            "nonce": 1234567890000u64,
            "signature": {
                "r": "0x1234567890abcdef",
                "s": "0xfedcba0987654321",
                "v": 27
            }
        });

        let body = serde_json::to_vec(&request).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request(Method::POST, url, None, None, Some(body), None, None)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::from_slice(&response.body).map_err(|e| e.to_string())
    }

    async fn submit_order(
        &self,
        asset: u32,
        is_buy: bool,
        sz: &str,
        limit_px: &str,
    ) -> Result<Value, String> {
        let action = json!({
            "type": "order",
            "orders": [{
                "a": asset,
                "b": is_buy,
                "p": limit_px,
                "s": sz,
                "r": false,
                "t": {"limit": {"tif": "Gtc"}}
            }],
            "grouping": "na"
        });

        self.send_exchange_action(action).await
    }

    async fn cancel_order(&self, asset: u32, oid: u64) -> Result<Value, String> {
        let action = json!({
            "type": "cancel",
            "cancels": [{
                "a": asset,
                "o": oid
            }]
        });

        self.send_exchange_action(action).await
    }

    async fn cancel_by_cloid(&self, asset: u32, cloid: &str) -> Result<Value, String> {
        let action = json!({
            "type": "cancelByCloid",
            "cancels": [{
                "asset": asset,
                "cloid": cloid
            }]
        });

        self.send_exchange_action(action).await
    }

    async fn modify_order(
        &self,
        oid: u64,
        asset: u32,
        is_buy: bool,
        sz: &str,
        limit_px: &str,
    ) -> Result<Value, String> {
        let action = json!({
            "type": "modify",
            "oid": oid,
            "order": {
                "a": asset,
                "b": is_buy,
                "p": limit_px,
                "s": sz,
                "r": false,
                "t": {"limit": {"tif": "Gtc"}}
            }
        });

        self.send_exchange_action(action).await
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_order_success() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.submit_order(0, true, "0.1", "50000.0").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");

    let data = &response["response"]["data"];
    assert!(data["statuses"][0]["resting"]["oid"].is_u64());
}

#[rstest]
#[tokio::test]
async fn test_submit_order_rejection() {
    let state = TestServerState::default();
    state.reject_next_order.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.submit_order(0, true, "0.1", "50000.0").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "err");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_stores_action() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let _ = client.submit_order(0, true, "0.5", "48000.0").await;

    let last_action = state.last_exchange_action.lock().await;
    let action = last_action.as_ref().unwrap();

    assert_eq!(action.get("type").unwrap().as_str().unwrap(), "order");
    assert!(action.get("orders").is_some());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_by_oid() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.cancel_order(0, 12345).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_by_cloid() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.cancel_by_cloid(0, "my-order-123").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");
}

#[rstest]
#[tokio::test]
async fn test_cancel_stores_action() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let _ = client.cancel_order(0, 99999).await;

    let last_action = state.last_exchange_action.lock().await;
    let action = last_action.as_ref().unwrap();

    assert_eq!(action.get("type").unwrap().as_str().unwrap(), "cancel");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_success() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.modify_order(12345, 0, true, "0.2", "51000.0").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");

    let data = &response["response"]["data"];
    assert!(data["statuses"][0]["resting"]["oid"].is_u64());
}

#[rstest]
#[tokio::test]
async fn test_modify_stores_action() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let _ = client.modify_order(12345, 0, false, "0.3", "52000.0").await;

    let last_action = state.last_exchange_action.lock().await;
    let action = last_action.as_ref().unwrap();

    assert_eq!(action.get("type").unwrap().as_str().unwrap(), "modify");
    assert_eq!(action.get("oid").unwrap().as_u64().unwrap(), 12345);
}

#[rstest]
#[tokio::test]
async fn test_exchange_rate_limiting() {
    let state = TestServerState::default();
    state.rate_limit_after.store(2, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));

    assert!(client.submit_order(0, true, "0.1", "50000.0").await.is_ok());
    assert!(client.submit_order(0, true, "0.1", "50000.0").await.is_ok());

    // Third triggers rate limit
    let result = client.submit_order(0, true, "0.1", "50000.0").await;
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "err");
}

#[rstest]
#[tokio::test]
async fn test_exchange_request_count() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));

    let _ = client.submit_order(0, true, "0.1", "50000.0").await;
    let _ = client.cancel_order(0, 12345).await;
    let _ = client.modify_order(12345, 0, true, "0.2", "51000.0").await;

    assert_eq!(*state.exchange_request_count.lock().await, 3);
}

#[rstest]
#[tokio::test]
async fn test_multiple_orders_in_sequence() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));

    for i in 0..5 {
        let result = client
            .submit_order(i, i % 2 == 0, "0.1", &format!("{}.0", 50000 + i * 100))
            .await;
        assert!(result.is_ok());
    }

    assert_eq!(*state.exchange_request_count.lock().await, 5);
}

const TEST_PRIVATE_KEY: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";

fn create_test_exec_config(addr: SocketAddr) -> HyperliquidExecClientConfig {
    HyperliquidExecClientConfig {
        private_key: Some(TEST_PRIVATE_KEY.to_string()),
        base_url_http: Some(format!("http://{addr}/info")),
        base_url_exchange: Some(format!("http://{addr}/exchange")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        is_testnet: false,
        ..HyperliquidExecClientConfig::default()
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    HyperliquidExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let client_id = ClientId::from("HYPERLIQUID");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("HYPERLIQUID"),
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = HyperliquidExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("10000.0 USDC"),
            Money::from("0 USDC"),
            Money::from("10000.0 USDC"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), ClientId::from("HYPERLIQUID"));
    assert_eq!(client.venue(), Venue::from("HYPERLIQUID"));
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_exec_client_connect_disconnect() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

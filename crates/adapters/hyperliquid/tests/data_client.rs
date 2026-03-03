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
//! These tests focus on HTTP data endpoints, combined HTTP+WS functionality,
//! and full `HyperliquidDataClient` lifecycle including connection, subscription,
//! and event emission.
//! Note: WebSocket subscription tests are in websocket.rs (50+ tests).

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

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
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, SubscribeQuotes, SubscribeTrades},
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_hyperliquid::{
    config::HyperliquidDataClientConfig,
    data::HyperliquidDataClient,
    http::{
        models::{HyperliquidL2Book, PerpMeta},
        query::InfoRequest,
    },
};
use nautilus_model::{
    data::Data,
    enums::BookType,
    identifiers::{ClientId, InstrumentId},
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
        "spotMeta" => Json(json!({"universe": [], "tokens": []})).into_response(),
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
                            if let Some(subscription) = payload.get("subscription") {
                                let sub_type = subscription
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("");

                                let data_msg = match sub_type {
                                    "trades" => json!({
                                        "channel": "trades",
                                        "data": [{
                                            "coin": "BTC",
                                            "side": "B",
                                            "px": "98450.00",
                                            "sz": "0.5",
                                            "hash": "0xabc123",
                                            "time": 1703875200000u64,
                                            "tid": 100001u64,
                                            "users": ["0xbuyer", "0xseller"]
                                        }]
                                    }),
                                    "bbo" => json!({
                                        "channel": "bbo",
                                        "data": {
                                            "coin": "BTC",
                                            "time": 1703875200000u64,
                                            "bbo": [
                                                {"px": "98450.00", "sz": "1.5", "n": 3},
                                                {"px": "98451.00", "sz": "2.0", "n": 2}
                                            ]
                                        }
                                    }),
                                    "l2Book" => {
                                        let book_data = load_json("ws_book_data.json");
                                        json!({"channel": "l2Book", "data": book_data})
                                    }
                                    _ => json!({"channel": sub_type, "data": {}}),
                                };

                                if socket
                                    .send(Message::Text(data_msg.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
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

fn create_data_client_config(addr: SocketAddr) -> HyperliquidDataClientConfig {
    HyperliquidDataClientConfig {
        base_url_http: Some(format!("http://{addr}/info")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        is_testnet: false,
        ..HyperliquidDataClientConfig::default()
    }
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(ClientId::new("HYPERLIQUID"), config).unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instruments_on_connect() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(ClientId::new("HYPERLIQUID"), config).unwrap();
    client.connect().await.unwrap();

    let mut instrument_count = 0;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, DataEvent::Instrument(_)) {
            instrument_count += 1;
        }
    }

    assert!(
        instrument_count > 0,
        "Expected instrument events on connect"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_subscribe_trades() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(ClientId::new("HYPERLIQUID"), config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = SubscribeTrades::new(
        instrument_id,
        Some(ClientId::new("HYPERLIQUID")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(&cmd).unwrap();

    // Drain until we get a trade (subscription is async via get_runtime)
    wait_until_async(
        || {
            let found = loop {
                match rx.try_recv() {
                    Ok(DataEvent::Data(Data::Trade(_))) => break true,
                    Ok(_) => {}
                    Err(_) => break false,
                }
            };
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_quotes() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(ClientId::new("HYPERLIQUID"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = SubscribeQuotes::new(
        instrument_id,
        Some(ClientId::new("HYPERLIQUID")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(&cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for quote event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Quote(_))),
        "Expected Quote event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_book_deltas() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(ClientId::new("HYPERLIQUID"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(ClientId::new("HYPERLIQUID")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    client.subscribe_book_deltas(&cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book deltas event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Deltas(_))),
        "Expected Deltas event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_reset_clears_state() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(ClientId::new("HYPERLIQUID"), config).unwrap();

    client.reset().unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.reset().unwrap();
    assert!(!client.is_connected());
}

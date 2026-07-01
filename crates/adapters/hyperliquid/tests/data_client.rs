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

use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, OnceLock},
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
use log::{Level, LevelFilter, Log, Metadata, Record};
use nautilus_common::{
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent, DataResponse,
        data::{
            RequestBookSnapshot, RequestFundingRates, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBookDeltas, SubscribeCustomData, SubscribeMarkPrices,
            SubscribeQuotes, SubscribeTrades, UnsubscribeCustomData, UnsubscribeMarkPrices,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_hyperliquid::{
    common::{
        consts::{HYPERLIQUID_CLIENT_ID, HYPERLIQUID_VENUE},
        enums::HyperliquidEnvironment,
    },
    config::HyperliquidDataClientConfig,
    data::HyperliquidDataClient,
    data_types::{HyperliquidAllDexsAssetCtxs, HyperliquidOpenInterest},
    http::{
        models::{HyperliquidL2Book, PerpMeta},
        query::InfoRequest,
    },
};
use nautilus_model::{
    data::{Data, DataType},
    enums::BookType,
    identifiers::InstrumentId,
    instruments::Instrument,
};
use nautilus_network::http::{HttpClient, Method};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct TestServerState {
    info_request_count: Arc<tokio::sync::Mutex<usize>>,
    last_request_type: Arc<tokio::sync::Mutex<Option<String>>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscriptions: Arc<tokio::sync::Mutex<Vec<Value>>>,
    asset_context_updates: Arc<tokio::sync::Notify>,
    bbo_updates: Arc<tokio::sync::Notify>,
    withhold_l2_book: Arc<tokio::sync::Mutex<bool>>,
    // When set, the `recentTrades` info endpoint responds with HTTP 422 to
    // emulate a node without the Hyperliquid indexer.
    recent_trades_unavailable: Arc<tokio::sync::Mutex<bool>>,
}

#[derive(Default)]
struct CapturingWarnLogger {
    messages: StdMutex<Vec<String>>,
}

impl CapturingWarnLogger {
    fn clear(&self) {
        self.messages
            .lock()
            .expect("log collector mutex poisoned")
            .clear();
    }

    fn messages(&self) -> Vec<String> {
        self.messages
            .lock()
            .expect("log collector mutex poisoned")
            .clone()
    }
}

impl Log for CapturingWarnLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Warn
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.messages
                .lock()
                .expect("log collector mutex poisoned")
                .push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

static CAPTURING_WARN_LOGGER: OnceLock<CapturingWarnLogger> = OnceLock::new();

fn install_capturing_warn_logger() -> &'static CapturingWarnLogger {
    let logger = CAPTURING_WARN_LOGGER.get_or_init(CapturingWarnLogger::default);
    let _ = log::set_logger(logger);
    log::set_max_level(LevelFilter::Warn);
    logger.clear();
    logger
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

// Minimal spot meta fixture: one canonical USDC token + one canonical
// PURR token, quoted pair PURR/USDC, so the instrument provider
// bootstraps a `PURR-USDC-SPOT` CurrencyPair instrument.
fn spot_meta_fixture() -> Value {
    json!({
        "tokens": [
            {"name": "USDC", "szDecimals": 6, "weiDecimals": 6, "index": 0, "tokenId": "0x1", "isCanonical": true},
            {"name": "PURR", "szDecimals": 0, "weiDecimals": 5, "index": 1, "tokenId": "0x2", "isCanonical": true},
            {"name": "USDH", "szDecimals": 2, "weiDecimals": 8, "index": 360, "tokenId": "0x168", "isCanonical": true},
        ],
        "universe": [
            {"name": "PURR/USDC", "tokens": [1, 0], "index": 0, "isCanonical": true},
        ]
    })
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
        "allPerpMetas" => {
            let standard_meta = load_json("http_meta_perp_sample.json");
            let hip3_meta = json!({
                "collateralToken": 360,
                "universe": [
                    {"name": "xyz:XYZ100", "szDecimals": 4, "maxLeverage": 30, "growthMode": "enabled"},
                    {"name": "xyz:TSLA", "szDecimals": 3, "maxLeverage": 10, "growthMode": "enabled", "marginMode": "strictIsolated"},
                    {"name": "xyz:NVDA", "szDecimals": 3, "maxLeverage": 20}
                ]
            });
            Json(json!([standard_meta, hip3_meta])).into_response()
        }
        "perpDexs" => Json(json!([null, {"name": "xyz"}])).into_response(),
        "metaAndAssetCtxs" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta, []])).into_response()
        }
        "spotMeta" => Json(spot_meta_fixture()).into_response(),
        "spotMetaAndAssetCtxs" => Json(json!([spot_meta_fixture(), []])).into_response(),
        "fundingHistory" => Json(load_json("http_funding_history.json")).into_response(),
        "l2Book" => {
            let book = load_json("http_l2_book_btc.json");
            Json(book).into_response()
        }
        "recentTrades" => {
            if *state.recent_trades_unavailable.lock().await {
                return (
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({"error": "recentTrades unavailable"})),
                )
                    .into_response();
            }
            Json(load_json("http_recent_trades_btc.json")).into_response()
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

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_socket(mut socket: WebSocket, state: TestServerState) {
    loop {
        let message = tokio::select! {
            message = socket.next() => message,
            () = state.asset_context_updates.notified() => {
                if socket
                    .send(Message::Text(active_asset_ctx_message().to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }

                continue;
            }
            () = state.bbo_updates.notified() => {
                if socket
                    .send(Message::Text(bbo_message().to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }

                continue;
            }
        };

        let Some(message) = message else { break };
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
                                state.subscriptions.lock().await.push(subscription.clone());
                                let sub_type = subscription
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("");

                                let data_msg = match sub_type {
                                    "trades" => Some(json!({
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
                                    })),
                                    "bbo" => Some(bbo_message()),
                                    "l2Book" => {
                                        if *state.withhold_l2_book.lock().await {
                                            None
                                        } else {
                                            let book_data = load_json("ws_book_data.json");
                                            Some(json!({"channel": "l2Book", "data": book_data}))
                                        }
                                    }
                                    "activeAssetCtx" => Some(active_asset_ctx_message()),
                                    "allDexsAssetCtxs" => {
                                        Some(load_json("ws_all_dexs_asset_ctxs.json"))
                                    }
                                    _ => Some(json!({"channel": sub_type, "data": {}})),
                                };

                                if let Some(data_msg) = data_msg
                                    && socket
                                        .send(Message::Text(data_msg.to_string().into()))
                                        .await
                                        .is_err()
                                {
                                    break;
                                }
                            }
                        }
                        Some("unsubscribe") => {
                            if let Some(subscription) = payload.get("subscription") {
                                state
                                    .unsubscriptions
                                    .lock()
                                    .await
                                    .push(subscription.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            // Inner if consumes `data`, cannot hoist into a match guard
            #[allow(clippy::collapsible_match)]
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

fn active_asset_ctx_message() -> Value {
    json!({
        "channel": "activeAssetCtx",
        "data": {
            "coin": "BTC",
            "ctx": {
                "dayNtlVlm": "1000000.0",
                "prevDayPx": "97000.0",
                "markPx": "98455.5",
                "midPx": "98455.0",
                "impactPxs": ["98454.0", "98456.0"],
                "dayBaseVlm": "100.0",
                "funding": "0.0001",
                "openInterest": "1500.0",
                "oraclePx": "98460.0",
                "premium": "-0.0001"
            }
        }
    })
}

fn bbo_message() -> Value {
    json!({
        "channel": "bbo",
        "data": {
            "coin": "BTC",
            "time": 1703875200000u64,
            "bbo": [
                {"px": "98450.00", "sz": "1.5", "n": 3},
                {"px": "98451.00", "sz": "2.0", "n": 2}
            ]
        }
    })
}

fn create_data_client_config(addr: SocketAddr) -> HyperliquidDataClientConfig {
    HyperliquidDataClientConfig {
        base_url_http: Some(format!("http://{addr}/info")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        environment: HyperliquidEnvironment::Mainnet,
        ..HyperliquidDataClientConfig::default()
    }
}

fn open_interest_data_type(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    DataType::new(
        "HyperliquidOpenInterest",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

async fn drain_initial_events(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) {
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|event| matches!(event, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}
}

async fn wait_for_open_interest_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    instrument_id: InstrumentId,
    data_type: DataType,
) {
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|event| is_open_interest_event(event, instrument_id, &data_type));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

async fn wait_for_open_interest_event_after_asset_context_update(
    state: &TestServerState,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    instrument_id: InstrumentId,
    data_type: DataType,
) {
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|event| is_open_interest_event(event, instrument_id, &data_type));

            if !found {
                state.asset_context_updates.notify_one();
            }

            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn is_open_interest_event(
    event: DataEvent,
    instrument_id: InstrumentId,
    data_type: &DataType,
) -> bool {
    let DataEvent::Data(Data::Custom(custom)) = event else {
        return false;
    };

    custom
        .data
        .as_any()
        .downcast_ref::<HyperliquidOpenInterest>()
        .is_some_and(|open_interest| {
            open_interest.instrument_id == instrument_id
                && open_interest.open_interest.to_string() == "1500.0"
                && custom.data_type == *data_type
        })
}

async fn wait_for_all_dex_asset_ctxs_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<HyperliquidAllDexsAssetCtxs>()
                    .is_some_and(|payload| {
                        payload.entries.iter().any(|entry| {
                            entry.instrument_id == InstrumentId::from("BTC-USD-PERP.HYPERLIQUID")
                                && entry.mark_price.to_string() == "77562.0"
                        }) && payload.entries.iter().any(|entry| {
                            entry.instrument_id
                                == InstrumentId::from("xyz:TSLA-USD-PERP.HYPERLIQUID")
                                && entry.dex == "xyz"
                        })
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
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
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
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
#[tokio::test]
async fn test_data_client_emits_hip3_instruments() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    let mut standard_perp_symbols = Vec::new();
    let mut hip3_symbols = Vec::new();
    let mut hip3_settlements = Vec::new();
    let mut spot_symbols = Vec::new();

    while let Ok(event) = rx.try_recv() {
        if let DataEvent::Instrument(instrument) = event {
            let symbol = instrument.id().symbol.to_string();
            if symbol.contains(':') {
                hip3_settlements.push((
                    symbol.clone(),
                    instrument.settlement_currency().code.to_string(),
                ));
                hip3_symbols.push(symbol);
            } else if symbol.ends_with("-SPOT") {
                spot_symbols.push(symbol);
            } else {
                standard_perp_symbols.push(symbol);
            }
        }
    }

    // Mock returns 3 standard perps (BTC, ETH, ATOM), 3 HIP-3 (xyz:XYZ100, xyz:TSLA, xyz:NVDA),
    // and 1 spot (PURR-USDC-SPOT).
    assert_eq!(standard_perp_symbols.len(), 3);
    assert_eq!(hip3_symbols.len(), 3);
    assert_eq!(spot_symbols.len(), 1);
    assert!(hip3_symbols.contains(&"xyz:XYZ100-USD-PERP".to_string()));
    assert!(hip3_symbols.contains(&"xyz:TSLA-USD-PERP".to_string()));
    assert!(hip3_symbols.contains(&"xyz:NVDA-USD-PERP".to_string()));
    assert!(hip3_settlements.contains(&("xyz:TSLA-USD-PERP".to_string(), "USDH".to_string(),)));

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
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = SubscribeTrades::new(
        instrument_id,
        Some(*HYPERLIQUID_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(cmd).unwrap();

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
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = SubscribeQuotes::new(
        instrument_id,
        Some(*HYPERLIQUID_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(cmd).unwrap();

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
async fn test_data_client_subscribe_custom_open_interest() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();
    drain_initial_events(&mut rx).await;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let data_type = open_interest_data_type(instrument_id);
    client
        .subscribe(SubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state.unsubscriptions.lock().await.is_empty()
                    && state.subscriptions.lock().await.iter().any(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("activeAssetCtx")
                    })
            }
        },
        Duration::from_secs(5),
    )
    .await;

    wait_for_open_interest_event(&mut rx, instrument_id, data_type.clone()).await;

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .unsubscriptions
                    .lock()
                    .await
                    .iter()
                    .any(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("activeAssetCtx")
                    })
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_shared_asset_context_subscription_with_open_interest() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();
    drain_initial_events(&mut rx).await;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    client
        .subscribe_mark_prices(SubscribeMarkPrices::new(
            instrument_id,
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscriptions
                    .lock()
                    .await
                    .iter()
                    .filter(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("activeAssetCtx")
                    })
                    .count()
                    == 1
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let data_type = open_interest_data_type(instrument_id);
    client
        .subscribe(SubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscriptions
                    .lock()
                    .await
                    .iter()
                    .filter(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("activeAssetCtx")
                    })
                    .count()
                    == 1
            }
        },
        Duration::from_secs(5),
    )
    .await;

    wait_for_open_interest_event_after_asset_context_update(
        &state,
        &mut rx,
        instrument_id,
        data_type.clone(),
    )
    .await;

    let active_asset_ctx_subscriptions = state
        .subscriptions
        .lock()
        .await
        .iter()
        .filter(|subscription| {
            subscription.get("type").and_then(|value| value.as_str()) == Some("activeAssetCtx")
        })
        .count();
    assert_eq!(active_asset_ctx_subscriptions, 1);

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .unsubscriptions
                    .lock()
                    .await
                    .iter()
                    .filter(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("activeAssetCtx")
                    })
                    .count()
                    == 0
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe_mark_prices(&UnsubscribeMarkPrices::new(
            instrument_id,
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .unsubscriptions
                    .lock()
                    .await
                    .iter()
                    .filter(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("activeAssetCtx")
                    })
                    .count()
                    == 1
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_resubscribe_custom_open_interest_emits_initial_value_again() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();
    drain_initial_events(&mut rx).await;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let data_type = open_interest_data_type(instrument_id);

    client
        .subscribe(SubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_for_open_interest_event(&mut rx, instrument_id, data_type.clone()).await;

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .unsubscriptions
                    .lock()
                    .await
                    .iter()
                    .filter(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("activeAssetCtx")
                    })
                    .count()
                    == 1
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(SubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_for_open_interest_event(&mut rx, instrument_id, data_type).await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_all_dex_asset_ctxs_custom_data() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();
    drain_initial_events(&mut rx).await;

    let data_type = DataType::new("HyperliquidAllDexsAssetCtxs", None, None);
    client
        .subscribe(SubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state.subscriptions.lock().await.iter().any(|subscription| {
                    subscription.get("type").and_then(|value| value.as_str())
                        == Some("allDexsAssetCtxs")
                })
            }
        },
        Duration::from_secs(5),
    )
    .await;

    wait_for_all_dex_asset_ctxs_event(&mut rx).await;

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .unsubscriptions
                    .lock()
                    .await
                    .iter()
                    .any(|subscription| {
                        subscription.get("type").and_then(|value| value.as_str())
                            == Some("allDexsAssetCtxs")
                    })
            }
        },
        Duration::from_secs(5),
    )
    .await;

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
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*HYPERLIQUID_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    client.subscribe_book_deltas(cmd).unwrap();

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
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_reports_stale_book_deltas_while_quotes_flow() {
    let logger = install_capturing_warn_logger();
    let state = TestServerState::default();
    *state.withhold_l2_book.lock().await = true;
    let addr = start_mock_server(state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_data_client_config(addr);
    config.stale_stream_receive_timeout_secs = 1;
    config.stream_health_check_interval_secs = 1;
    config.stale_stream_warning_cooldown_secs = 60;

    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();
    drain_initial_events(&mut rx).await;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let bbo_updates = Arc::clone(&state.bbo_updates);

    let bbo_pump = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            bbo_updates.notify_waiters();
        }
    });

    wait_until_async(
        || {
            let found = loop {
                match rx.try_recv() {
                    Ok(DataEvent::Data(Data::Quote(_))) => break true,
                    Ok(_) => {}
                    Err(_) => break false,
                }
            };
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    wait_until_async(
        || {
            let messages = logger.messages();
            let found_stale_book = messages.iter().any(|message| {
                message.contains("Hyperliquid market data stream stale")
                    && message.contains("channel=deltas")
                    && message.contains("instrument_id=BTC-USD-PERP.HYPERLIQUID")
                    && message.contains("receive_age_ms=")
                    && message.contains("venue_age_ms=n/a")
                    && message.contains("stale_count=1")
            });
            let found_stale_quote = messages.iter().any(|message| {
                message.contains("Hyperliquid market data stream stale")
                    && message.contains("channel=quote")
                    && message.contains("instrument_id=BTC-USD-PERP.HYPERLIQUID")
            });
            async move { found_stale_book && !found_stale_quote }
        },
        Duration::from_secs(5),
    )
    .await;

    bbo_pump.abort();

    let messages = logger.messages();
    assert!(
        messages.iter().any(|message| {
            message.contains("Hyperliquid market data stream stale")
                && message.contains("channel=deltas")
                && message.contains("instrument_id=BTC-USD-PERP.HYPERLIQUID")
        }),
        "stale book-deltas warning should be logged, messages were: {messages:?}",
    );
    assert!(
        messages.iter().all(|message| {
            !message.contains("Hyperliquid market data stream stale")
                || !message.contains("channel=quote")
        }),
        "flowing quote stream should not be reported stale, messages were: {messages:?}",
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
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();

    client.reset().unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.reset().unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_instruments() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let request = RequestInstruments::new(
        None,
        None,
        Some(*HYPERLIQUID_CLIENT_ID),
        Some(*HYPERLIQUID_VENUE),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_instruments(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for instruments response")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Response(DataResponse::Instruments(_))),
        "Expected Instruments response, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_instrument() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let request = RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(*HYPERLIQUID_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_instrument(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for instrument response")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Response(DataResponse::Instrument(_))),
        "Expected Instrument response, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_book_snapshot() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let request = RequestBookSnapshot::new(
        instrument_id,
        None,
        Some(*HYPERLIQUID_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_book_snapshot(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book snapshot response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Book(book_response)) => {
            assert_eq!(book_response.instrument_id, instrument_id);
            let book = &book_response.data;
            assert!(book.best_bid_price().is_some(), "book should have bids");
            assert!(book.best_ask_price().is_some(), "book should have asks");
        }
        other => panic!("Expected Book response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_book_snapshot_with_depth() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let request = RequestBookSnapshot::new(
        instrument_id,
        Some(NonZeroUsize::new(2).unwrap()),
        Some(*HYPERLIQUID_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_book_snapshot(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book snapshot response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Book(book_response)) => {
            let book = &book_response.data;
            // The fixture has 5 levels per side; depth=2 should limit to 2
            let bid_count = book.bids(None).count();
            let ask_count = book.asks(None).count();
            assert_eq!(bid_count, 2, "should have 2 bid levels with depth=2");
            assert_eq!(ask_count, 2, "should have 2 ask levels with depth=2");
        }
        other => panic!("Expected Book response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_trades() {
    // `request_trades` fetches the `recentTrades` snapshot and always emits a
    // `TradesResponse` so the awaiting caller completes.
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(*HYPERLIQUID_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_trades(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for trades response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Trades(trades_response)) => {
            assert_eq!(trades_response.instrument_id, instrument_id);
            // Fixture carries three trades; the response is sorted ascending.
            assert_eq!(trades_response.data.len(), 3);
            assert_eq!(trades_response.data[0].trade_id.to_string(), "300001");
            assert_eq!(trades_response.data[2].trade_id.to_string(), "300003");
            assert!(
                trades_response.data[0].ts_event <= trades_response.data[2].ts_event,
                "trades should be ascending by ts_event",
            );
        }
        other => panic!("Expected Trades response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_trades_endpoint_unavailable() {
    // A node without the indexer returns HTTP 422 for `recentTrades`; the
    // client must still emit an empty `TradesResponse` rather than erroring.
    let state = TestServerState::default();
    *state.recent_trades_unavailable.lock().await = true;
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(*HYPERLIQUID_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_trades(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for trades response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Trades(trades_response)) => {
            assert_eq!(trades_response.instrument_id, instrument_id);
            assert!(
                trades_response.data.is_empty(),
                "422 should yield an empty response, was: {:?}",
                trades_response.data,
            );
        }
        other => panic!("Expected empty Trades response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_request_funding_rates_non_perp_bails() {
    // Spot instruments do not have funding rates. The guard inside
    // `request_funding_rates` must reject them before any HTTP round-trip.
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    let spot_id = InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID");
    let cmd = RequestFundingRates::new(
        spot_id,
        None,
        None,
        None,
        Some(*HYPERLIQUID_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let result = client.request_funding_rates(cmd);
    let err = result.expect_err("non-perpetual instrument must bail");
    assert!(
        err.to_string().to_lowercase().contains("perpetual"),
        "error should explain why: {err}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_request_funding_rates_emits_data_response_from_mock() {
    // End-to-end: mock serves the on-disk `http_funding_history.json` fixture
    // (3 entries for BTC), the exec path parses it, and emits
    // `DataResponse::FundingRates` with 3 `FundingRateUpdate`s back on the
    // data event channel.
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events emitted on connect.
    while rx.try_recv().is_ok() {}

    let perp_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = RequestFundingRates::new(
        perp_id,
        None,
        None,
        None,
        Some(*HYPERLIQUID_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_funding_rates(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for funding rates response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::FundingRates(response)) => {
            assert_eq!(response.instrument_id, perp_id);
            assert_eq!(response.data.len(), 3, "fixture carries three entries");

            let rates: Vec<_> = response.data.iter().map(|u| u.rate.to_string()).collect();
            assert!(rates.contains(&"0.0000125".to_string()));
            assert!(rates.contains(&"-0.0000081".to_string()));
            assert!(rates.contains(&"0.0000033".to_string()));

            for update in &response.data {
                assert_eq!(update.interval, Some(60), "Hyperliquid funds hourly");
                assert!(update.next_funding_ns.is_none());
            }
        }
        other => panic!("expected FundingRates response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

// Reconnect must install a fresh `CancellationToken` so the new
// `spawn_ws` task does not clone a pre-cancelled token from the prior
// session and exit on the first poll. Regression for the round-2 P1
// finding in the review-fix loop.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_reconnect_after_disconnect_resumes_stream() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();

    // First lifecycle: connect, then disconnect (cancels the token).
    client.connect().await.unwrap();
    assert!(client.is_connected());
    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    // Drain any residual events from the first cycle.
    while rx.try_recv().is_ok() {}

    // Second lifecycle: the new connect must reset the token, otherwise
    // the consumption loop spawns into a cancelled future and no trades
    // ever arrive.
    client.connect().await.unwrap();
    assert!(client.is_connected());

    // Drain instrument events from the second connect.
    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let cmd = SubscribeTrades::new(
        instrument_id,
        Some(*HYPERLIQUID_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(cmd).unwrap();

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

// `disconnect()` must abort tracked subscribe tasks: after the call
// completes, the spawned subscribe futures must not continue to surface
// data events. This pins `abort_pending_tasks()` to an observable
// behavior rather than relying on the cancellation token absorbing the
// race.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_disconnect_stops_event_flow() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");

    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let disconnect_deadline = Duration::from_secs(5);
    tokio::time::timeout(disconnect_deadline, client.disconnect())
        .await
        .expect("disconnect must complete promptly even with pending subscribes")
        .unwrap();

    assert!(!client.is_connected());

    // Drain anything that arrived during the disconnect window, then
    // assert the stream is quiet after disconnect returned.
    while rx.try_recv().is_ok() {}

    let quiet_window = Duration::from_millis(200);
    let maybe_event = tokio::time::timeout(quiet_window, rx.recv()).await;
    assert!(
        maybe_event.is_err(),
        "no data events should arrive after disconnect, was: {maybe_event:?}",
    );
}

// `reset()` on a connected client with a live ws stream and an in-flight
// subscribe handle must succeed without panic and leave the client in a
// state where `connect()` is permitted again (matching the pre-existing
// `reset()` contract). The existing `test_data_client_reset_clears_state`
// never spawns the ws task before reset and so leaves the new branches
// (`abort_pending_tasks` + `ws_stream_handle.take().abort()`) untested.
//
// Note: full data-flow restart after reset is not asserted because
// `reset()` does not currently disconnect the inner `HyperliquidWebSocketClient`;
// `disconnect()` is the supported path for that.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_reset_after_subscribe_clears_state() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = HyperliquidDataClient::new(*HYPERLIQUID_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(*HYPERLIQUID_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    // Reset must succeed even with a live ws stream handle and an
    // in-flight pending subscribe task outstanding. This exercises both
    // `abort_pending_tasks()` and the `ws_stream_handle.take().abort()`
    // branch added in `reset()`.
    client.reset().unwrap();
    assert!(!client.is_connected());

    // The client must be willing to accept a fresh connect after reset;
    // a stale cancellation token or undropped stream handle would surface
    // here as a hang or error.
    let reconnect = tokio::time::timeout(Duration::from_secs(5), client.connect())
        .await
        .expect("connect after reset must complete promptly");
    reconnect.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
}

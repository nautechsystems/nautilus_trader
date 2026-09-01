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

//! Integration tests for `OKXDataClient`.

use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        Query, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent, DataResponse,
        data::{
            BookResponse, InstrumentResponse, InstrumentsResponse, RequestBookSnapshot,
            RequestInstrument, RequestInstruments,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_okx::{
    common::{
        consts::{OKX_CLIENT_ID, resolve_book_depth},
        enums::{OKXEnvironment, OKXInstrumentType},
    },
    config::OKXDataClientConfig,
    data::OKXDataClient,
};
use nautilus_testkit::events::{collect_data_events_until_response, drain_data_events};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct TestServerState {
    instrument_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    rpi_book_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    spread_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    fail_spreads: bool,
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("failed to read {}", path.display()));
    serde_json::from_str(&content).expect("invalid json fixture")
}

fn spread_response(params: &HashMap<String, String>) -> Value {
    let mut payload = load_test_data("http_get_spreads.json");

    if let Some(sprd_id) = params.get("sprdId")
        && let Some(data) = payload.get_mut("data").and_then(Value::as_array_mut)
    {
        data.retain(|item| item.get("sprdId").and_then(Value::as_str) == Some(sprd_id));
    }

    payload
}

fn create_router(state: TestServerState) -> Router {
    let instruments_state = state.clone();
    let rpi_book_state = state.clone();
    let spreads_state = state;

    Router::new()
        .route(
            "/api/v5/public/instruments",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = instruments_state.clone();
                async move {
                    state.instrument_queries.lock().await.push(params);
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/books-rpi",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = rpi_book_state.clone();
                async move {
                    state.rpi_book_queries.lock().await.push(params);
                    Json(load_test_data("http_get_rpi_order_book.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/sprd/spreads",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = spreads_state.clone();
                async move {
                    state.spread_queries.lock().await.push(params.clone());

                    if state.fail_spreads {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({
                                "code": "50000",
                                "msg": "spread endpoint unavailable",
                                "data": []
                            })),
                        )
                            .into_response();
                    }

                    Json(spread_response(&params)).into_response()
                }
            }),
        )
}

async fn start_test_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr");
    let router = create_router(state);
    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
    addr
}

#[derive(Clone, Default)]
struct WsTeardownState {
    opened: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
}

async fn handle_public_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WsTeardownState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_public_ws_socket(socket, state))
}

async fn handle_public_ws_socket(mut socket: WebSocket, state: Arc<WsTeardownState>) {
    state.opened.fetch_add(1, Ordering::Relaxed);
    while socket.next().await.is_some() {}
    state.closed.fetch_add(1, Ordering::Relaxed);
}

async fn start_data_session_failure_server() -> (SocketAddr, Arc<WsTeardownState>) {
    let ws_state = Arc::new(WsTeardownState::default());
    let router = create_router(TestServerState::default()).route(
        "/ws/public",
        get(handle_public_ws_upgrade).with_state(Arc::clone(&ws_state)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
    (addr, ws_state)
}

fn create_test_data_client(
    addr: SocketAddr,
    load_spreads: bool,
) -> (
    OKXDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(tx);

    let base_url = format!("http://{addr}");
    let config = OKXDataClientConfig {
        instrument_types: vec![OKXInstrumentType::Spot],
        load_spreads,
        base_url_http: Some(base_url),
        base_url_ws_public: Some(format!("ws://{addr}/ws/public")),
        base_url_ws_business: Some(format!("ws://{addr}/ws/business")),
        environment: OKXEnvironment::Live,
        http_timeout_secs: 5,
        max_retries: 0,
        retry_delay_initial_ms: 1,
        retry_delay_max_ms: 1,
        ..OKXDataClientConfig::default()
    };

    let client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("OKX data client");
    (client, rx)
}

#[tokio::test]
async fn test_failed_connect_tears_down_public_websocket_before_retry() {
    let (addr, ws_state) = start_data_session_failure_server().await;
    let (mut client, _rx) = create_test_data_client(addr, true);

    for attempt in 1..=2 {
        let error = client.connect().await.unwrap_err();
        assert!(
            error.to_string().contains("business websocket"),
            "expected business WebSocket failure after public transport started: {error}"
        );

        wait_until_async(
            || {
                let ws_state = Arc::clone(&ws_state);
                async move { ws_state.closed.load(Ordering::Relaxed) >= attempt }
            },
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(
            ws_state.opened.load(Ordering::Relaxed),
            attempt,
            "each connect attempt must open a fresh public WebSocket"
        );
        assert_eq!(
            ws_state.closed.load(Ordering::Relaxed),
            attempt,
            "failed connect must close the public WebSocket before retry"
        );
    }
}

fn request_instruments() -> RequestInstruments {
    RequestInstruments::new(
        None,
        None,
        Some(*OKX_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn request_instrument(instrument_id: &str) -> RequestInstrument {
    RequestInstrument::new(
        InstrumentId::from(instrument_id),
        None,
        None,
        Some(*OKX_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn instruments_response(events: &[DataEvent]) -> &InstrumentsResponse {
    events
        .iter()
        .find_map(|event| match event {
            DataEvent::Response(DataResponse::Instruments(response)) => Some(response),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected DataResponse::Instruments, received {events:?}"))
}

fn instrument_response(events: &[DataEvent]) -> Option<&InstrumentResponse> {
    events.iter().find_map(|event| match event {
        DataEvent::Response(DataResponse::Instrument(response)) => Some(response.as_ref()),
        _ => None,
    })
}

fn book_response(events: &[DataEvent]) -> Option<&BookResponse> {
    events.iter().find_map(|event| match event {
        DataEvent::Response(DataResponse::Book(response)) => Some(response),
        _ => None,
    })
}

#[rstest]
#[case::depth_0_passes_through(0, 0)]
#[case::depth_400_passes_through(400, 400)]
#[case::depth_50_passes_through(50, 50)]
#[case::depth_1_clamps_to_50(1, 50)]
#[case::depth_5_clamps_to_50(5, 50)]
#[case::depth_10_clamps_to_50(10, 50)]
#[case::depth_25_clamps_to_50(25, 50)]
#[case::depth_49_clamps_to_50(49, 50)]
#[case::depth_51_clamps_to_400(51, 400)]
#[case::depth_100_clamps_to_400(100, 400)]
#[case::depth_200_clamps_to_400(200, 400)]
#[case::depth_500_clamps_to_400(500, 400)]
#[case::depth_1000_clamps_to_400(1000, 400)]
fn test_resolve_book_depth(#[case] raw_depth: usize, #[case] expected: usize) {
    assert_eq!(resolve_book_depth(raw_depth), expected);
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_includes_spreads_when_enabled() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr, true);

    let request = request_instruments();
    let request_id = request.request_id;
    client
        .request_instruments(request)
        .expect("request_instruments");

    let events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;
    let response = instruments_response(&events);
    let rpi_instrument = response
        .data
        .iter()
        .find(|instrument| instrument.id() == InstrumentId::from("BTC-USD.OKX"))
        .expect("BTC-USD instrument");
    let InstrumentAny::CurrencyPair(rpi_pair) = rpi_instrument else {
        panic!("expected BTC-USD currency pair");
    };
    let rpi_info = rpi_pair.info.as_ref().expect("RPI spacing info");
    let spread_ids = response
        .data
        .iter()
        .filter_map(|instrument| match instrument {
            InstrumentAny::CryptoFuturesSpread(spread) => Some(spread.id.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let instrument_queries = state.instrument_queries.lock().await;
    let spread_queries = state.spread_queries.lock().await;

    assert_eq!(spread_ids.len(), 2);
    assert_eq!(rpi_info.get_u64("okx_rpi_min_level"), Some(5));
    assert_eq!(rpi_info.get_str("okx_rpi_min_px_band"), Some("20"));
    assert!(spread_ids.contains(&"ETH-USD-SWAP_ETH-USD-231229.OKX".to_string()));
    assert!(spread_ids.contains(&"BTC-USDT_BTC-USDT-SWAP.OKX".to_string()));
    assert_eq!(instrument_queries.len(), 1);
    assert_eq!(
        instrument_queries[0].get("instType").map(String::as_str),
        Some("SPOT")
    );
    assert_eq!(spread_queries.len(), 1);
    assert_eq!(
        spread_queries[0].get("state").map(String::as_str),
        Some("live")
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_continues_when_spread_endpoint_fails() {
    let state = TestServerState {
        fail_spreads: true,
        ..TestServerState::default()
    };
    let addr = start_test_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr, true);

    let request = request_instruments();
    let request_id = request.request_id;
    client
        .request_instruments(request)
        .expect("request_instruments");

    let events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;
    let response = instruments_response(&events);
    let spread_count = response
        .data
        .iter()
        .filter(|instrument| matches!(instrument, InstrumentAny::CryptoFuturesSpread(_)))
        .count();
    let spot_count = response
        .data
        .iter()
        .filter(|instrument| matches!(instrument, InstrumentAny::CurrencyPair(_)))
        .count();
    let spread_queries = state.spread_queries.lock().await;

    assert_eq!(spread_count, 0);
    assert_eq!(spot_count, 5);
    assert_eq!(spread_queries.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_returns_spread_when_enabled() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr, true);

    let request = request_instrument("BTC-USDT_BTC-USDT-SWAP.OKX");
    let request_id = request.request_id;
    client
        .request_instrument(request)
        .expect("request_instrument");

    let events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;
    let response = instrument_response(&events).expect("spread response must be emitted");
    let spread_queries = state.spread_queries.lock().await;

    assert_eq!(
        response.instrument_id,
        InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX")
    );
    assert!(matches!(
        response.data,
        InstrumentAny::CryptoFuturesSpread(_)
    ));
    assert_eq!(spread_queries.len(), 1);
    assert_eq!(
        spread_queries[0].get("sprdId").map(String::as_str),
        Some("BTC-USDT_BTC-USDT-SWAP")
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_emits_no_spread_when_disabled() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr, false);

    client
        .request_instrument(request_instrument("BTC-USDT_BTC-USDT-SWAP.OKX"))
        .expect("request_instrument");

    // This path emits no response, so retain a bounded absence window.
    let events = drain_data_events(&mut rx, Duration::from_secs(1)).await;
    let spread_queries = state.spread_queries.lock().await;

    assert!(instrument_response(&events).is_none());
    assert_eq!(spread_queries.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_rpi_request_book_snapshot_routes_to_rpi_endpoint() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr, false);

    let instruments_request = request_instruments();
    let instruments_request_id = instruments_request.request_id;
    client
        .request_instruments(instruments_request)
        .expect("request instruments");
    let _ =
        collect_data_events_until_response(&mut rx, instruments_request_id, Duration::from_secs(5))
            .await;

    let mut params = Params::new();
    params.insert("rpi".to_string(), json!(true));
    let request = RequestBookSnapshot::new(
        InstrumentId::from("BTC-USD.OKX"),
        NonZeroUsize::new(2),
        Some(*OKX_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );
    let request_id = request.request_id;
    client
        .request_book_snapshot(request)
        .expect("request RPI book snapshot");

    let events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;
    let response = book_response(&events).expect("RPI book response");
    let queries = state.rpi_book_queries.lock().await;

    assert_eq!(response.instrument_id, InstrumentId::from("BTC-USD.OKX"));
    assert_eq!(response.data.bids(None).count(), 2);
    assert_eq!(response.data.asks(None).count(), 2);
    assert_eq!(response.data.best_bid_price(), Some(Price::from("64619.9")));
    assert_eq!(response.data.best_ask_price(), Some(Price::from("64620")));
    assert_eq!(
        response.data.best_bid_size().expect("best bid quantity"),
        Quantity::from("3.48451281")
    );
    assert_eq!(
        response
            .params
            .as_ref()
            .and_then(|params| params.get_bool("rpi")),
        Some(true)
    );
    assert_eq!(queries.len(), 1);
    assert_eq!(
        queries[0].get("instId").map(String::as_str),
        Some("BTC-USD")
    );
    assert_eq!(queries[0].get("sz").map(String::as_str), Some("2"));
}

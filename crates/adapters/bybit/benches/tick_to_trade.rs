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

//! Real Bybit clients and live engines between two loopback WebSocket boundaries.

#![cfg(not(madsim))]

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use nautilus_bybit::{
    common::{enums::BybitProductType, parse::parse_linear_instrument},
    config::BybitDataClientConfig,
    factories::{BybitDataClientFactory, BybitExecutionClientFactory},
    http::models::BybitInstrumentLinearResponse,
};
use nautilus_core::UnixNanos;
use nautilus_live::builder::LiveNodeBuilder;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::AccountId,
    types::{Price, Quantity},
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};

#[path = "../tests/common/mock_venue.rs"]
mod mock_venue;

#[path = "../../../live/tests/common/tick_to_trade.rs"]
mod workload;

use mock_venue::{
    TestServerState, create_test_exec_config, create_test_router, handle_order_message,
    handle_text_message, load_test_data,
};
use workload::{Case, DATA_CLIENT, EXEC_CLIENT, Observation};

#[rstest]
#[case::quote_checked(true, false)]
#[case::quote_bypass(true, true)]
#[case::trade_checked(false, false)]
#[case::trade_bypass(false, true)]
#[ignore = "Wall-clock benchmark; run optimized with process isolation"]
#[tokio::test(flavor = "current_thread")]
async fn tick_to_trade_bybit(
    #[case] quote: bool,
    #[case] bypass: bool,
    #[values(false, true)] stream: bool,
) {
    Box::pin(run(Case::new(quote, bypass, false, stream))).await;
}

#[cfg(feature = "python")]
#[rstest]
#[case::quote_checked(true, false)]
#[case::quote_bypass(true, true)]
#[case::trade_checked(false, false)]
#[case::trade_bypass(false, true)]
#[ignore = "Wall-clock benchmark; run optimized with process isolation"]
#[tokio::test(flavor = "current_thread")]
async fn tick_to_trade_bybit_python(
    #[case] quote: bool,
    #[case] bypass: bool,
    #[values(false, true)] stream: bool,
) {
    Box::pin(run(Case::new(quote, bypass, true, stream))).await;
}

#[rstest]
#[case::quote(true)]
#[case::trade(false)]
#[tokio::test]
async fn public_socket_stays_open_after_feed(#[case] quote: bool) {
    let (observations, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let (feed, feed_rx) = tokio::sync::mpsc::unbounded_channel();

    let state = BenchServer {
        base: TestServerState::default(),
        observations,
        feed: Arc::new(tokio::sync::Mutex::new(feed_rx)),
        updates: tokio::sync::broadcast::channel(1).0,
        quote,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/bench/public", get(public_socket))
        .with_state(state);
    let (stop, stopped) = tokio::sync::oneshot::channel();

    let server_task = async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                stopped.await.unwrap();
            })
            .await
            .unwrap();
    };

    let server = tokio::spawn(server_task); // tokio-import-ok: Use this test's runtime

    let expected_frame = feed_frame(quote, 3);

    let (ack, frame, pong) = tokio::time::timeout(Duration::from_secs(5), async {
        let (mut socket, _) = connect_async(format!("ws://{address}/bench/public"))
            .await
            .unwrap();
        socket
            .send(ClientMessage::text(
                json!({"op": "subscribe", "req_id": "subscription-1", "args": [expected_frame["topic"]]})
                    .to_string(),
            ))
            .await
            .unwrap();
        let ack: Value = serde_json::from_str(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        feed.send((3, Instant::now())).unwrap();
        drop(feed);
        let frame: Value = serde_json::from_str(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        socket
            .send(ClientMessage::text(
                json!({"op": "ping", "req_id": "after-feed-2"}).to_string(),
            ))
            .await
            .unwrap();
        let pong: Value = serde_json::from_str(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        socket.close(None).await.unwrap();
        (ack, frame, pong)
    })
    .await
    .expect("public socket did not respond after feed completion");

    stop.send(()).unwrap();
    server.await.unwrap();

    assert_eq!(
        ack,
        json!({"success": true, "ret_msg": "", "conn_id": "test-conn-id", "req_id": "subscription-1", "op": "subscribe"})
    );
    assert_eq!(frame, expected_frame);
    assert_eq!(
        pong,
        json!({"success": true, "ret_msg": "pong", "conn_id": "test-conn-id", "req_id": "after-feed-2", "op": "pong"})
    );
}

async fn run(case: Case) {
    let (observations, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (feed, feed_rx) = tokio::sync::mpsc::unbounded_channel();

    let state = BenchServer {
        base: TestServerState {
            trade_order_header: Some(json!({
                "X-Bapi-Limit": 1_000_000,
                "X-Bapi-Limit-Status": 1_000_000,
            })),
            ..Default::default()
        },
        observations,
        feed: Arc::new(tokio::sync::Mutex::new(feed_rx)),
        updates: tokio::sync::broadcast::channel(16_384).0,
        quote: case.quote,
    };

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let router = create_test_router(state.base.clone()).merge(
        Router::new()
            .route("/bench/public", get(public_socket))
            .route("/bench/private", get(private_socket))
            .route("/bench/trade", get(trade_socket))
            .with_state(state),
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();

    let server = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        stopped.await.unwrap();
                    })
                    .await
                    .unwrap();
            });
    });

    let mut exec_config = create_test_exec_config(address);
    exec_config.base_url_ws_private = Some(format!("ws://{address}/bench/private"));
    exec_config.base_url_ws_trade = Some(format!("ws://{address}/bench/trade"));
    exec_config.account_id = Some(AccountId::from("BYBIT-001"));

    let data_config = BybitDataClientConfig {
        api_key: exec_config.api_key.clone(),
        api_secret: exec_config.api_secret.clone(),
        product_types: vec![BybitProductType::Linear],
        base_url_http: Some(format!("http://{address}")),
        base_url_ws_public: Some(format!("ws://{address}/bench/public")),
        ..Default::default()
    };

    let node = LiveNodeBuilder::from_config(workload::node_config(case))
        .unwrap()
        .add_data_client(
            Some(DATA_CLIENT.to_string()),
            Box::new(BybitDataClientFactory::new()),
            Box::new(data_config),
        )
        .unwrap()
        .add_exec_client(
            Some(EXEC_CLIENT.to_string()),
            Box::new(BybitExecutionClientFactory::new()),
            Box::new(exec_config),
        )
        .unwrap()
        .build()
        .unwrap();
    let definition: BybitInstrumentLinearResponse =
        serde_json::from_value(load_test_data("http_get_instruments_linear.json")).unwrap();
    let instrument = parse_linear_instrument(
        &definition.result.list[0],
        UnixNanos::default(),
        UnixNanos::default(),
    )
    .unwrap();
    workload::measure(
        node,
        instrument,
        case,
        "bybit_loopback",
        include_str!("tick_to_trade.rs"),
        receiver,
        move |ack| {
            workload::produce(case, ack, move |index, scheduled| {
                feed.send((index, scheduled)).unwrap();
            })
        },
    )
    .await;

    stop.send(()).unwrap();
    server.join().unwrap();
}

#[derive(Clone)]
struct BenchServer {
    base: TestServerState,
    observations: tokio::sync::mpsc::UnboundedSender<Observation>,
    feed: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<(usize, Instant)>>>,
    updates: tokio::sync::broadcast::Sender<String>,
    quote: bool,
}

async fn public_socket(ws: WebSocketUpgrade, State(state): State<BenchServer>) -> Response {
    ws.on_upgrade(move |socket| serve_public(socket, state))
}

async fn private_socket(ws: WebSocketUpgrade, State(state): State<BenchServer>) -> Response {
    ws.on_upgrade(move |socket| serve_private(socket, state))
}

async fn trade_socket(ws: WebSocketUpgrade, State(state): State<BenchServer>) -> Response {
    ws.on_upgrade(move |socket| serve_trade(socket, state))
}

async fn serve_public(mut socket: WebSocket, state: BenchServer) {
    let mut feed = state.feed.lock().await;
    let mut subscribed = false;

    loop {
        tokio::select! {
            message = socket.recv() => {
                let Some(Ok(Message::Text(text))) = message else { break; };
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["op"] == "subscribe" {
                    let expected = if state.quote { "orderbook.1.BTCUSDT" } else { "publicTrade.BTCUSDT" };
                    assert_eq!(value["args"], json!([expected]));
                    subscribed = true;
                }

                if !handle_text_message(&mut socket, &state.base, &text).await { break; }
            }
            Some((index, scheduled)) = feed.recv(), if subscribed => {
                let frame = feed_frame(state.quote, index).to_string();
                let sent = Instant::now();
                socket.send(Message::Text(frame.into())).await.unwrap();
                state.observations.send(Observation::Sent { index, scheduled, sent }).unwrap();
            }
        }
    }
}

async fn serve_private(mut socket: WebSocket, state: BenchServer) {
    let mut updates = state.updates.subscribe();

    loop {
        tokio::select! {
            message = socket.recv() => {
                let Some(Ok(Message::Text(text))) = message else { break; };
                if !handle_text_message(&mut socket, &state.base, &text).await { break; }
            }
            update = updates.recv() => {
                if socket.send(Message::Text(update.unwrap().into())).await.is_err() { break; }
            }
        }
    }
}

async fn serve_trade(mut socket: WebSocket, state: BenchServer) {
    let mut request_ids = std::collections::HashSet::new();
    let template = load_test_data("ws_account_order.json");

    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        let received = Instant::now();
        let value: Value = serde_json::from_str(&text).unwrap();
        if value["op"] == "order.create" {
            assert!(request_ids.insert(value["reqId"].as_str().unwrap().to_string()));
            assert!(!value["reqId"].as_str().unwrap().is_empty());
            assert_eq!(value["args"].as_array().unwrap().len(), 1);
            let args = &value["args"][0];
            assert_eq!(args["category"], "linear");
            assert_eq!(args["symbol"], "BTCUSDT");
            assert_eq!(args["orderType"], "Limit");
            assert_eq!(args["side"], "Buy");
            assert_eq!(args["timeInForce"], "GTC");
            assert_eq!(args.get("reduceOnly"), None);
            let index = workload::order_index(args["orderLinkId"].as_str().unwrap());
            let quantity = Quantity::from(args["qty"].as_str().unwrap()).as_decimal();
            let price = Price::from(args["price"].as_str().unwrap()).as_decimal();
            let order = json!({
                "id": args["orderLinkId"], "side": OrderSide::Buy,
                "quantity": quantity.normalize(),
                "price": price.normalize(),
                "time_in_force": TimeInForce::Gtc, "post_only": false,
                "reduce_only": false, "quote_quantity": false,
            });
            state
                .observations
                .send(Observation::Received {
                    index,
                    received,
                    order,
                })
                .unwrap();

            if !handle_order_message(&mut socket, &state.base, "order.create", &value).await {
                break;
            }

            let mut update = template.clone();
            let order = &mut update["data"][0];
            order["orderId"] = json!(format!("V-TTT-{index}"));
            order["orderLinkId"] = args["orderLinkId"].clone();
            order["price"] = args["price"].clone();
            order["qty"] = args["qty"].clone();
            order["leavesQty"] = args["qty"].clone();
            order["leavesValue"] = json!((quantity * price).normalize().to_string());
            order["stopOrderType"] = json!("");
            order["triggerBy"] = json!("");
            order["orderStatus"] = json!("New");
            state.updates.send(update.to_string()).unwrap();
            update["data"][0]["orderStatus"] = json!("Cancelled");
            state.updates.send(update.to_string()).unwrap();
        } else if !handle_text_message(&mut socket, &state.base, &text).await {
            break;
        }
    }
}

fn feed_frame(quote: bool, index: usize) -> Value {
    let ts = 1_700_000_000_000_u64 + index as u64;

    if quote {
        json!({
            "topic": "orderbook.1.BTCUSDT", "type": "snapshot", "ts": ts,
            "data": {"s": "BTCUSDT", "b": [[workload::price(index).to_string(), workload::SIZE_BID]],
                "a": [[workload::price_ask(index).to_string(), workload::SIZE_ASK]],
                "u": index + 1, "seq": index + 1},
        })
    } else {
        json!({
            "topic": "publicTrade.BTCUSDT", "type": "snapshot", "ts": ts,
            "data": [{"T": ts, "s": "BTCUSDT", "S": "Buy", "v": workload::SIZE_BID,
                "p": workload::price(index).to_string(), "i": format!("T-TTT-{index}"), "BT": false}],
        })
    }
}

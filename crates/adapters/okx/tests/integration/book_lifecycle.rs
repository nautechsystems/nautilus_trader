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

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, UnsubscribeBookDeltas},
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::SocketReconnectRegistry;
use nautilus_model::{
    data::Data,
    enums::{BookType, RecordFlag},
    identifiers::InstrumentId,
    orderbook::OrderBook,
};
use nautilus_network::mode::ReconnectRequestOutcome;
use nautilus_okx::{
    common::{consts::OKX_CLIENT_ID, enums::OKXInstrumentType},
    config::OKXDataClientConfig,
    data::OKXDataClient,
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use ustr::Ustr;

#[derive(Default)]
struct BookWire {
    connections: AtomicUsize,
    subscriptions: AtomicUsize,
    unsubscriptions: AtomicUsize,
    late_snapshot: AtomicBool,
    drop_snapshots: AtomicUsize,
    dropped: AtomicUsize,
    corrupt_updates: AtomicUsize,
    gaps: AtomicUsize,
    reject: AtomicBool,
    push: tokio::sync::Notify,
}

async fn upgrade(ws: WebSocketUpgrade, State(state): State<Arc<BookWire>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| mock(socket, state))
}

fn filter_frame(text: &str, state: &BookWire) -> Option<String> {
    let Ok(mut frame) = serde_json::from_str::<Value>(text) else {
        return Some(text.to_string());
    };

    if !matches!(
        frame["arg"]["channel"].as_str(),
        Some("books" | "sprd-books5")
    ) {
        return Some(text.to_string());
    }

    if (frame["action"] == "snapshot" || frame["arg"]["channel"] == "sprd-books5")
        && state
            .drop_snapshots
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    {
        state.dropped.fetch_add(1, Ordering::SeqCst);
        return None;
    }

    if frame["action"] == "update"
        && state
            .corrupt_updates
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    {
        frame["data"][0]["prevSeqId"] = json!(i64::MAX);
        state.gaps.fetch_add(1, Ordering::SeqCst);
    }

    Some(frame.to_string())
}

fn book_frame(snapshot: bool, generation: usize, channel: &str) -> Value {
    let raw = if snapshot {
        include_str!("../../test_data/ws_books_snapshot.json")
    } else {
        include_str!("../../test_data/ws_books_update.json")
    };

    let mut frame: Value = serde_json::from_str(raw).unwrap();
    frame["arg"]["instId"] = json!("BTC-USD");
    let shift = i64::try_from(generation).unwrap() * 100;

    for (side, base, direction) in [("asks", 8_500_i64, 1), ("bids", 8_400, -1)] {
        for (index, level) in frame["data"][0][side]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            level[0] = json!((base + shift + direction * index as i64).to_string());
        }
    }

    if channel == "sprd-books5" {
        frame["arg"] = json!({"channel": channel, "sprdId": "BTC-USDT_BTC-USDT-SWAP"});
        frame.as_object_mut().unwrap().remove("action");

        for side in ["asks", "bids"] {
            let levels = frame["data"][0][side].as_array_mut().unwrap();
            levels.truncate(5);

            for level in levels {
                let level = level.as_array_mut().unwrap();
                level.truncate(3);
                level[1] = json!("1.000");
            }
        }
    }

    frame
}

async fn mock(mut socket: WebSocket, state: Arc<BookWire>) {
    state.connections.fetch_add(1, Ordering::SeqCst);

    loop {
        tokio::select! {
            () = state.push.notified() => {
                let frame = book_frame(false, 0, "books").to_string();
                if let Some(frame) = filter_frame(&frame, &state)
                    && socket.send(Message::Text(frame.into())).await.is_err()
                {
                    break;
                }
            }
            message = socket.recv() => {
                let Some(Ok(Message::Text(text))) = message else {
                    break;
                };

                if text == "ping" {
                    if socket.send(Message::Text("pong".into())).await.is_err() {
                        break;
                    }
                    continue;
                }
                let request: Value = serde_json::from_str(&text).unwrap();
                let op = request["op"].as_str().unwrap();
                for arg in request["args"].as_array().unwrap() {
                    let ack = json!({"event": op, "connId": "mock", "arg": arg});
                    if socket.send(Message::Text(ack.to_string().into())).await.is_err() {
                        return;
                    }

                    let channel = arg["channel"].as_str().unwrap();
                    if !matches!(channel, "books" | "sprd-books5") {
                        continue;
                    }

                    if channel == "sprd-books5" {
                        assert_eq!(arg["sprdId"], "BTC-USDT_BTC-USDT-SWAP");
                        assert!(arg.get("instId").is_none());
                    }

                    if op == "unsubscribe" {
                        state.unsubscriptions.fetch_add(1, Ordering::SeqCst);
                    }

                    if op != "subscribe" {
                        continue;
                    }
                    state.subscriptions.fetch_add(1, Ordering::SeqCst);
                    let frame = if state.reject.load(Ordering::SeqCst) {
                        json!({"event": "error", "code": "60018", "msg": "Channel does not exist", "arg": arg}).to_string()
                    } else {
                        book_frame(true, state.subscriptions.load(Ordering::SeqCst), channel).to_string()
                    };

                    if let Some(frame) = filter_frame(&frame, &state)
                        && socket.send(Message::Text(frame.into())).await.is_err()
                    {
                        return;
                    }

                    if state.late_snapshot.load(Ordering::SeqCst)
                        && socket.send(Message::Text(book_frame(true, state.subscriptions.load(Ordering::SeqCst), channel).to_string().into())).await.is_err()
                    {
                        return;
                    }
                }
            }
        }
    }
}

struct BookClient {
    client: OKXDataClient,
    events: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    registry: SocketReconnectRegistry,
    wire: Arc<BookWire>,
    business: Arc<BookWire>,
    server: tokio::task::JoinHandle<()>,
    books: HashMap<InstrumentId, OrderBook>,
    snapshots: HashMap<InstrumentId, usize>,
}

impl BookClient {
    async fn connect(snapshot_timeout: u64) -> Self {
        let wire = Arc::new(BookWire::default());
        let business = Arc::new(BookWire::default());

        let router = Router::new()
            .route("/ws", get(upgrade))
            .route("/business", get(upgrade).with_state(Arc::clone(&business)))
            .route(
                "/api/v5/public/instruments",
                get(|| async {
                    axum::Json(
                        serde_json::from_str::<Value>(include_str!(
                            "../../test_data/http_get_instruments_spot.json"
                        ))
                        .unwrap(),
                    )
                }),
            )
            .route(
                "/api/v5/sprd/spreads",
                get(|| async {
                    axum::Json(
                        serde_json::from_str::<Value>(include_str!(
                            "../../test_data/http_get_spreads.json"
                        ))
                        .unwrap(),
                    )
                }),
            )
            .with_state(Arc::clone(&wire));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let (sender, events) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let registry = SocketReconnectRegistry::default();

        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot],
            load_spreads: true,
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws_public: Some(format!("ws://{addr}/ws")),
            base_url_ws_business: Some(format!("ws://{addr}/business")),
            update_instruments_interval_mins: 0,
            book_stale_check_interval_secs: 0,
            book_snapshot_timeout_secs: snapshot_timeout,
            ..OKXDataClientConfig::default()
        };

        let mut client = registry.scope(|| OKXDataClient::new(*OKX_CLIENT_ID, config).unwrap());
        client.connect().await.unwrap();

        Self {
            client,
            events,
            registry,
            wire,
            business,
            server,
            books: HashMap::new(),
            snapshots: HashMap::new(),
        }
    }

    fn subscribe(&mut self, instrument_id: InstrumentId) {
        self.client
            .subscribe_book_deltas(SubscribeBookDeltas::new(
                instrument_id,
                BookType::L2_MBP,
                Some(*OKX_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                true,
                None,
                None,
            ))
            .unwrap();
        self.books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));
    }

    fn unsubscribe(&mut self, instrument_id: InstrumentId) {
        self.client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                instrument_id,
                Some(*OKX_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    }

    fn reconnect(&self) {
        let handle = self
            .registry
            .handle(*OKX_CLIENT_ID, Ustr::from("okx-public-data-streams"))
            .unwrap();
        assert_eq!(
            handle.request_reconnect(),
            ReconnectRequestOutcome::Accepted
        );
    }

    async fn snapshots(&mut self, instruments: &[InstrumentId], count: usize) {
        tokio::time::timeout(Duration::from_secs(75), async {
            while instruments
                .iter()
                .any(|id| self.snapshots.get(id).copied().unwrap_or(0) < count)
            {
                let event = self.events.recv().await.expect("data event channel");
                self.apply(event);
            }
        })
        .await
        .expect("accepted snapshots for every subscribed instrument");
    }

    fn apply(&mut self, event: DataEvent) {
        if let DataEvent::Data(Data::BookDeltas(deltas)) = event {
            let snapshot = deltas
                .deltas
                .first()
                .is_some_and(|delta| RecordFlag::F_SNAPSHOT.matches(delta.flags));
            if snapshot {
                *self.snapshots.entry(deltas.instrument_id).or_default() += 1;
            }

            let book = self
                .books
                .get_mut(&deltas.instrument_id)
                .expect("subscribed instrument");
            book.apply_deltas(&deltas).unwrap();
            let bid = book.best_bid_price().expect("nonempty bids");
            let ask = book.best_ask_price().expect("nonempty asks");
            assert!(
                bid < ask,
                "crossed book for {}: {bid} >= {ask}",
                deltas.instrument_id
            );
        }
    }

    async fn stop(mut self) {
        tokio::time::timeout(Duration::from_secs(10), self.client.disconnect())
            .await
            .expect("bounded disconnect")
            .unwrap();
        assert!(self.client.is_disconnected());
        self.server.abort();
    }
}

#[rstest]
#[case::snapshot_deadline(1)]
#[case::disabled_deadline(0)]
#[tokio::test]
async fn recovery_survives_reconnect_with_snapshot_pending(#[case] timeout: u64) {
    let mut session = BookClient::connect(timeout).await;
    let id = InstrumentId::from("BTC-USD.OKX");
    session.subscribe(id);
    session.snapshots(&[id], 1).await;
    session.wire.drop_snapshots.store(1, Ordering::SeqCst);
    session.wire.corrupt_updates.store(1, Ordering::SeqCst);
    session.wire.push.notify_one();
    wait_until_async(
        || async { session.wire.dropped.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(5),
    )
    .await;
    session.reconnect();
    session.snapshots(&[id], 2).await;
    assert_eq!(session.wire.connections.load(Ordering::SeqCst), 2);
    assert_eq!(session.wire.gaps.load(Ordering::SeqCst), 1);
    assert_eq!(session.wire.subscriptions.load(Ordering::SeqCst), 3);
    session.stop().await;
}

#[rstest]
#[case::snapshot_deadline(1)]
#[case::disabled_deadline(0)]
#[tokio::test]
async fn unsubscribe_during_recovery_does_not_replay_book(#[case] timeout: u64) {
    let mut session = BookClient::connect(timeout).await;
    let id = InstrumentId::from("BTC-USD.OKX");
    session.subscribe(id);
    session.snapshots(&[id], 1).await;
    session.wire.drop_snapshots.store(1, Ordering::SeqCst);
    session.wire.corrupt_updates.store(1, Ordering::SeqCst);
    session.wire.push.notify_one();
    wait_until_async(
        || async { session.wire.dropped.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(5),
    )
    .await;
    session.unsubscribe(id);
    wait_until_async(
        || async { session.wire.unsubscriptions.load(Ordering::SeqCst) == 2 },
        Duration::from_secs(5),
    )
    .await;
    session.reconnect();
    wait_until_async(
        || async { session.wire.connections.load(Ordering::SeqCst) == 2 },
        Duration::from_secs(5),
    )
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(300), session.events.recv())
            .await
            .is_err()
    );
    assert_eq!(session.wire.subscriptions.load(Ordering::SeqCst), 2);
    session.subscribe(id);
    session.snapshots(&[id], 2).await;
    assert_eq!(session.wire.subscriptions.load(Ordering::SeqCst), 3);
    session.stop().await;
}

#[tokio::test]
async fn permanent_rejection_suppresses_late_snapshot_until_reconnect() {
    let mut session = BookClient::connect(1).await;
    let id = InstrumentId::from("BTC-USD.OKX");
    session.subscribe(id);
    session.snapshots(&[id], 1).await;
    session.wire.reject.store(true, Ordering::SeqCst);
    session.wire.late_snapshot.store(true, Ordering::SeqCst);
    session.wire.corrupt_updates.store(1, Ordering::SeqCst);
    session.wire.push.notify_one();
    wait_until_async(
        || async { session.wire.subscriptions.load(Ordering::SeqCst) == 2 },
        Duration::from_secs(5),
    )
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(300), session.events.recv())
            .await
            .is_err()
    );
    session.wire.reject.store(false, Ordering::SeqCst);
    session.wire.late_snapshot.store(false, Ordering::SeqCst);
    session.reconnect();
    session.snapshots(&[id], 2).await;
    assert_eq!(session.wire.subscriptions.load(Ordering::SeqCst), 3);
    assert_eq!(session.snapshots[&id], 2);
    session.stop().await;
}

#[tokio::test]
async fn spread_recovery_retries_missing_snapshot_on_business_socket() {
    let mut session = BookClient::connect(1).await;
    let id = InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX");
    session.subscribe(id);
    session.snapshots(&[id], 1).await;
    session.business.drop_snapshots.store(2, Ordering::SeqCst);
    let handle = session
        .registry
        .handle(*OKX_CLIENT_ID, Ustr::from("okx-business-data-streams"))
        .unwrap();
    assert_eq!(
        handle.request_reconnect(),
        ReconnectRequestOutcome::Accepted
    );
    session.snapshots(&[id], 2).await;

    assert_eq!(session.business.connections.load(Ordering::SeqCst), 2);
    assert_eq!(session.business.subscriptions.load(Ordering::SeqCst), 4);
    assert_eq!(session.business.unsubscriptions.load(Ordering::SeqCst), 2);
    assert_eq!(session.business.dropped.load(Ordering::SeqCst), 2);
    assert_eq!(session.wire.connections.load(Ordering::SeqCst), 1);
    assert_eq!(session.wire.subscriptions.load(Ordering::SeqCst), 0);
    assert_eq!(session.snapshots[&id], 2);
    let book = &session.books[&id];
    assert_eq!(
        book.bids_as_map(None).into_iter().collect::<Vec<_>>(),
        (0..5)
            .map(|n| (Decimal::from(8_800 - n), Decimal::ONE))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        book.asks_as_map(None).into_iter().collect::<Vec<_>>(),
        (0..5)
            .map(|n| (Decimal::from(8_900 + n), Decimal::ONE))
            .collect::<Vec<_>>()
    );
    session.stop().await;
}

#[rstest]
#[case::public(false)]
#[case::business(true)]
#[tokio::test]
async fn initial_snapshot_loss_recovers_without_reconnect(#[case] spread: bool) {
    let mut session = BookClient::connect(1).await;

    let (id, wire) = if spread {
        (
            InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX"),
            Arc::clone(&session.business),
        )
    } else {
        (InstrumentId::from("BTC-USD.OKX"), Arc::clone(&session.wire))
    };

    wire.drop_snapshots.store(1, Ordering::SeqCst);
    session.subscribe(id);
    session.snapshots(&[id], 1).await;
    assert_eq!(wire.connections.load(Ordering::SeqCst), 1);
    assert_eq!(wire.subscriptions.load(Ordering::SeqCst), 2);
    assert_eq!(wire.unsubscriptions.load(Ordering::SeqCst), 1);
    assert_eq!(wire.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(session.snapshots[&id], 1);
    assert_eq!(
        session.books[&id].best_bid_price().unwrap().as_decimal(),
        Decimal::from(8600)
    );
    assert_eq!(
        session.books[&id].best_ask_price().unwrap().as_decimal(),
        Decimal::from(8700)
    );
    session.stop().await;
}

#[rstest]
#[case::public(false)]
#[case::business(true)]
#[tokio::test]
async fn initial_snapshot_wait_cancels_on_unsubscribe_and_resubscribe(#[case] spread: bool) {
    let mut session = BookClient::connect(1).await;

    let (id, wire) = if spread {
        (
            InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX"),
            Arc::clone(&session.business),
        )
    } else {
        (InstrumentId::from("BTC-USD.OKX"), Arc::clone(&session.wire))
    };

    wire.drop_snapshots.store(1, Ordering::SeqCst);
    session.subscribe(id);
    wait_until_async(
        || async { wire.dropped.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(3),
    )
    .await;
    session.unsubscribe(id);
    wait_until_async(
        || async { wire.unsubscriptions.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(3),
    )
    .await;
    session.subscribe(id);
    session.snapshots(&[id], 1).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(1200), session.events.recv())
            .await
            .is_err()
    );
    assert_eq!(wire.connections.load(Ordering::SeqCst), 1);
    assert_eq!(wire.subscriptions.load(Ordering::SeqCst), 2);
    assert_eq!(wire.unsubscriptions.load(Ordering::SeqCst), 1);
    assert_eq!(wire.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(session.snapshots[&id], 1);
    session.stop().await;
}

#[rstest]
#[case::public(false)]
#[case::business(true)]
#[tokio::test]
async fn initial_snapshot_disabled_deadline_waits_without_recovery(#[case] spread: bool) {
    let mut session = BookClient::connect(0).await;

    let (id, wire) = if spread {
        (
            InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX"),
            Arc::clone(&session.business),
        )
    } else {
        (InstrumentId::from("BTC-USD.OKX"), Arc::clone(&session.wire))
    };

    wire.drop_snapshots.store(1, Ordering::SeqCst);
    session.subscribe(id);
    wait_until_async(
        || async { wire.dropped.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(3),
    )
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(1200), async {
            while let Some(event) = session.events.recv().await {
                if matches!(event, DataEvent::Data(Data::BookDeltas(_))) {
                    return;
                }
            }
        })
        .await
        .is_err()
    );
    assert_eq!(wire.connections.load(Ordering::SeqCst), 1);
    assert_eq!(wire.subscriptions.load(Ordering::SeqCst), 1);
    assert_eq!(wire.unsubscriptions.load(Ordering::SeqCst), 0);
    assert_eq!(wire.dropped.load(Ordering::SeqCst), 1);
    session.stop().await;
}

#[rstest]
#[case::public(false)]
#[case::business(true)]
#[tokio::test]
async fn initial_snapshot_arrival_cancels_deadline(#[case] spread: bool) {
    let mut session = BookClient::connect(1).await;

    let (id, wire) = if spread {
        (
            InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX"),
            Arc::clone(&session.business),
        )
    } else {
        (InstrumentId::from("BTC-USD.OKX"), Arc::clone(&session.wire))
    };

    session.subscribe(id);
    session.snapshots(&[id], 1).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(1200), session.events.recv())
            .await
            .is_err()
    );
    assert_eq!(wire.connections.load(Ordering::SeqCst), 1);
    assert_eq!(wire.subscriptions.load(Ordering::SeqCst), 1);
    assert_eq!(wire.unsubscriptions.load(Ordering::SeqCst), 0);
    assert_eq!(session.snapshots[&id], 1);
    session.stop().await;
}

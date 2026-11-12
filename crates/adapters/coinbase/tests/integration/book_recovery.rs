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

//! Order book sequence-gap recovery against a replayed Coinbase capture.
//!
//! The mock venue replays `ws_l2_resubscribe_capture.jsonl` once both `level2` subscriptions
//! arrive, then answers a later `level2` subscribe with a fresh snapshot and one update, numbered
//! after the replay. Like Coinbase, it sends nothing for a duplicate subscribe, so only an
//! unsubscribe and subscribe pair recovers a book. Dropping one envelope from the replay
//! simulates a lost frame.

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
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
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use nautilus_coinbase::{
    CoinbaseDataClient, CoinbaseDataClientConfig, common::consts::COINBASE_CLIENT_ID,
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
use nautilus_model::{
    data::{Data, OrderBookDeltas},
    enums::{BookAction, BookType},
    identifiers::InstrumentId,
};
use rstest::rstest;
use serde_json::{Value, json};

const API_PREFIX: &str = "/api/v3/brokerage";
const PRODUCTS: [&str; 2] = ["BTC-USD", "ETH-USD"];

// Ends before the capture's own unsubscribe of BTC-USD, so every later snapshot is a recovery's
const REPLAY_LAST: usize = 200;

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn capture_frames() -> Vec<String> {
    std::fs::read_to_string(data_path().join("ws_l2_resubscribe_capture.jsonl"))
        .unwrap()
        .lines()
        .take(REPLAY_LAST + 1)
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Default)]
struct Venue {
    skip_sequence_num: Option<u64>,
    // Product whose recovery snapshot is held until an unsubscribe arrives after `release` is set
    hold_product: Option<&'static str>,
    release: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<(String, String)>>>,
}

impl Venue {
    fn requests(&self, product_id: &str) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, product)| product == product_id)
            .map(|(action, _)| action.clone())
            .collect()
    }
}

async fn handle_products() -> impl IntoResponse {
    let mut payload: Value = serde_json::from_str(
        &std::fs::read_to_string(data_path().join("http_products.json")).unwrap(),
    )
    .unwrap();

    let btc = payload["products"][0].clone();
    let mut eth = btc.clone();
    eth["product_id"] = json!("ETH-USD");
    eth["base_currency_id"] = json!("ETH");
    eth["base_display_symbol"] = json!("ETH");
    eth["base_name"] = json!("Ethereum");
    eth["display_name"] = json!("ETH-USD");
    eth["alias_to"] = json!([]);
    payload["products"] = json!([btc, eth]);
    payload["num_products"] = json!(2);
    Json(payload)
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(venue): State<Venue>) -> Response {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, venue))
}

fn level2_request(text: &str) -> Option<(String, Vec<String>)> {
    let payload: Value = serde_json::from_str(text).ok()?;
    if payload["channel"] != "level2" {
        return None;
    }

    let action = payload["type"].as_str()?.to_string();
    let products = payload["product_ids"]
        .as_array()?
        .iter()
        .filter_map(|product| product.as_str().map(str::to_owned))
        .collect();
    Some((action, products))
}

fn recovery_frames(product_id: &str, sequence_num: u64) -> [String; 2] {
    let snapshot = json!({
        "channel": "l2_data",
        "timestamp": "2026-10-09T07:02:00.000000000Z",
        "sequence_num": sequence_num,
        "events": [{
            "type": "snapshot",
            "product_id": product_id,
            "updates": [
                {"side": "bid", "event_time": "2026-10-09T07:02:00Z", "price_level": "100.00", "new_quantity": "1.0"},
                {"side": "offer", "event_time": "2026-10-09T07:02:00Z", "price_level": "101.00", "new_quantity": "1.0"},
            ],
        }],
    });
    let update = json!({
        "channel": "l2_data",
        "timestamp": "2026-10-09T07:02:00.100000000Z",
        "sequence_num": sequence_num + 1,
        "events": [{
            "type": "update",
            "product_id": product_id,
            "updates": [
                {"side": "bid", "event_time": "2026-10-09T07:02:00.1Z", "price_level": "100.00", "new_quantity": "2.0"},
            ],
        }],
    });
    [snapshot.to_string(), update.to_string()]
}

async fn handle_ws_socket(socket: WebSocket, venue: Venue) {
    let (mut tx, mut rx) = socket.split();
    let mut pending: Vec<String> = PRODUCTS.iter().map(|p| (*p).to_string()).collect();
    let mut sequence_num = REPLAY_LAST as u64 + 1;
    let mut replayed = false;
    let mut subscribed: Vec<String> = PRODUCTS.iter().map(|p| (*p).to_string()).collect();

    while let Some(Ok(message)) = rx.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Some((action, products)) = level2_request(&text) else {
            continue;
        };

        for product in &products {
            venue
                .requests
                .lock()
                .unwrap()
                .push((action.clone(), product.clone()));
        }

        if !replayed {
            pending.retain(|product| !products.contains(product));
            if pending.is_empty() {
                replayed = true;

                for frame in capture_frames() {
                    let envelope: Value = serde_json::from_str(&frame).unwrap();
                    if envelope["sequence_num"].as_u64() == venue.skip_sequence_num {
                        continue;
                    }

                    if tx.send(Message::Text(frame.into())).await.is_err() {
                        return;
                    }
                }
            }
            continue;
        }

        for product in products {
            let duplicate = action == "subscribe" && subscribed.contains(&product);
            if action == "subscribe" {
                subscribed.push(product.clone());
            } else {
                subscribed.retain(|p| p != &product);
            }

            let held = venue.hold_product == Some(product.as_str());
            let respond = if held {
                action == "unsubscribe" && venue.release.load(Ordering::SeqCst)
            } else {
                action == "subscribe" && !duplicate
            };

            if !respond {
                continue;
            }

            for frame in recovery_frames(&product, sequence_num) {
                if tx.send(Message::Text(frame.into())).await.is_err() {
                    return;
                }
            }
            sequence_num += 2;
        }
    }
}

async fn start_venue(venue: Venue) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route(
            &format!("{API_PREFIX}/market/products"),
            get(handle_products),
        )
        .route("/ws", get(handle_ws_upgrade))
        .with_state(venue);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    addr
}

fn subscribe_book(instrument_id: InstrumentId) -> SubscribeBookDeltas {
    SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*COINBASE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    )
}

fn instrument_id(product_id: &str) -> InstrumentId {
    InstrumentId::from(format!("{product_id}.COINBASE").as_str())
}

fn is_snapshot(deltas: &OrderBookDeltas) -> bool {
    deltas.deltas[0].action == BookAction::Clear
}

fn is_recovery_snapshot(deltas: &OrderBookDeltas) -> bool {
    is_snapshot(deltas)
        && deltas
            .deltas
            .iter()
            .any(|delta| delta.order.price.to_string() == "100.00")
}

// Number of `level2` events for `product_id` in the replay before `before`
fn captured_events(product_id: &str, kind: &str, before: u64) -> usize {
    capture_frames()
        .iter()
        .map(|frame| serde_json::from_str::<Value>(frame).unwrap())
        .filter(|envelope| {
            envelope["channel"] == "l2_data" && envelope["sequence_num"].as_u64().unwrap() < before
        })
        .flat_map(|envelope| envelope["events"].as_array().unwrap().clone())
        .filter(|event| event["product_id"] == product_id && event["type"] == kind)
        .count()
}

struct Session {
    client: CoinbaseDataClient,
    rx: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    books: Vec<OrderBookDeltas>,
}

impl Session {
    async fn start(venue: Venue) -> Self {
        Self::start_with_timeout(venue, 5).await
    }

    async fn start_with_timeout(venue: Venue, book_snapshot_timeout_secs: u64) -> Self {
        let addr = start_venue(venue).await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(tx);

        let config = CoinbaseDataClientConfig {
            base_url_rest: Some(format!("http://{addr}")),
            base_url_ws: Some(format!("ws://{addr}/ws")),
            book_snapshot_timeout_secs,
            ..CoinbaseDataClientConfig::default()
        };
        let mut client = CoinbaseDataClient::new(*COINBASE_CLIENT_ID, config).unwrap();
        client.connect().await.unwrap();

        for product in PRODUCTS {
            client
                .subscribe_book_deltas(subscribe_book(instrument_id(product)))
                .unwrap();
        }

        Self {
            client,
            rx,
            books: Vec::new(),
        }
    }

    async fn collect_until(&mut self, done: impl Fn(&[OrderBookDeltas]) -> bool) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !done(&self.books) {
                if let Some(DataEvent::Data(Data::BookDeltas(deltas))) = self.rx.recv().await {
                    self.books.push(*deltas);
                }
            }
        })
        .await
        .expect("timed out waiting for book deltas");
    }

    fn drain(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            if let DataEvent::Data(Data::BookDeltas(deltas)) = event {
                self.books.push(*deltas);
            }
        }
    }

    fn emitted(&self, product_id: &str) -> Vec<&OrderBookDeltas> {
        let instrument_id = instrument_id(product_id);
        self.books
            .iter()
            .filter(|deltas| deltas.instrument_id == instrument_id)
            .collect()
    }
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_capture_replay_emits_every_book_event_without_recovery() {
    let venue = Venue::default();
    let mut session = Session::start(venue.clone()).await;
    let expected: usize = PRODUCTS
        .iter()
        .map(|p| captured_events(p, "snapshot", u64::MAX) + captured_events(p, "update", u64::MAX))
        .sum();

    session.collect_until(|books| books.len() >= expected).await;

    // Allow any spurious recovery write to reach the venue before checking
    tokio::time::sleep(Duration::from_millis(200)).await;

    for product in PRODUCTS {
        let emitted = session.emitted(product);
        assert!(
            is_snapshot(emitted[0]),
            "{product} must start with a snapshot"
        );
        assert_eq!(
            emitted.len(),
            captured_events(product, "snapshot", u64::MAX)
                + captured_events(product, "update", u64::MAX),
        );
        assert_eq!(venue.requests(product), ["subscribe"], "{product}");
    }

    session.client.disconnect().await.unwrap();
}

// A lost envelope could have belonged to either book, so both stop at the gap and resume only
// after their own unsubscribe and subscribe pair delivers a fresh snapshot.
#[rstest]
#[case::btc_update(150)]
#[case::eth_update(151)]
#[case::heartbeat(139)]
#[tokio::test(flavor = "multi_thread")]
async fn test_skipped_envelope_recovers_each_book(#[case] skipped: u64) {
    let venue = Venue {
        skip_sequence_num: Some(skipped),
        ..Venue::default()
    };
    let mut session = Session::start(venue.clone()).await;

    session
        .collect_until(|books| {
            PRODUCTS.iter().all(|product| {
                let id = instrument_id(product);
                let recovered = books
                    .iter()
                    .filter(|deltas| deltas.instrument_id == id)
                    .skip_while(|deltas| !is_recovery_snapshot(deltas))
                    .count();
                recovered >= 2
            })
        })
        .await;

    for product in PRODUCTS {
        let emitted = session.emitted(product);
        let resumed = emitted
            .iter()
            .position(|deltas| is_recovery_snapshot(deltas))
            .unwrap();

        // Output stops at the gap: nothing replayed after the lost envelope reaches the book
        assert_eq!(
            resumed,
            captured_events(product, "snapshot", skipped)
                + captured_events(product, "update", skipped),
            "{product}"
        );
        assert!(!is_snapshot(emitted[resumed + 1]), "{product}");
        assert_eq!(
            venue.requests(product),
            ["subscribe", "unsubscribe", "subscribe"],
            "{product}"
        );
    }

    session.client.disconnect().await.unwrap();
}

// The venue holds the ETH-USD recovery snapshot and releases it as the client's unsubscribe
// arrives, so the snapshot races the unsubscribe. Nothing reaches the book afterwards, and
// recovery stops instead of retrying once its snapshot deadline passes.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_unsubscribe_during_recovery_stops_book() {
    let skipped = 150;
    let venue = Venue {
        skip_sequence_num: Some(skipped),
        hold_product: Some("ETH-USD"),
        ..Venue::default()
    };
    let mut session = Session::start_with_timeout(venue.clone(), 2).await;
    let eth = instrument_id("ETH-USD");

    session
        .collect_until(|books| {
            books
                .iter()
                .filter(|deltas| deltas.instrument_id == instrument_id("BTC-USD"))
                .skip_while(|deltas| !is_recovery_snapshot(deltas))
                .count()
                >= 2
        })
        .await;
    wait_until_async(
        || async { venue.requests("ETH-USD").len() == 3 },
        Duration::from_secs(5),
    )
    .await;
    session.drain();

    // ETH-USD stopped at the gap and is still waiting on its recovery
    let suppressed = session.emitted("ETH-USD").len();
    assert_eq!(
        suppressed,
        captured_events("ETH-USD", "snapshot", skipped)
            + captured_events("ETH-USD", "update", skipped),
    );

    venue.release.store(true, Ordering::SeqCst);
    session
        .client
        .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
            eth,
            Some(*COINBASE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until_async(
        || async { venue.requests("ETH-USD").len() == 4 },
        Duration::from_secs(5),
    )
    .await;

    // Past the snapshot deadline, a recovery still running would have retried
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    session.drain();

    assert_eq!(session.emitted("ETH-USD").len(), suppressed);
    assert_eq!(
        venue.requests("ETH-USD"),
        ["subscribe", "unsubscribe", "subscribe", "unsubscribe"]
    );

    session.client.disconnect().await.unwrap();
}

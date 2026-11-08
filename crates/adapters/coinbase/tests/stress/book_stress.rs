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

//! Public `level2` book validation through a fault-injecting relay.
//!
//! Coinbase numbers every envelope on a connection with one `sequence_num`, so any frame the relay
//! drops, holds, or rewrites is a gap for every subscribed book. Each emitted batch is compared
//! with a book rebuilt from the raw relayed frames, aligned by envelope timestamp.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};

use nautilus_coinbase::{
    CoinbaseDataClient, CoinbaseDataClientConfig, common::consts::COINBASE_CLIENT_ID,
};
use nautilus_common::clients::DataClient;
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    identifiers::{ClientId, InstrumentId},
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, FrameKind, Route, Session, StressArgs, StressVenue, Upstream, WireBook,
    WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const STREAM: &str = "ws";
const ENDPOINT: &str = "coinbase-data-streams";
const DEPTH: usize = 20;

// Emitted deltas carry no venue sequence, so views align on envelope timestamp alone
const VIEW_SEQUENCE: u64 = 0;

type CoinbaseSession = Session<Coinbase>;

fn main() {
    stress::run::<Coinbase, _, _>(|args| async move {
        let ids = [
            InstrumentId::from("DOGE-USD.COINBASE"),
            InstrumentId::from("LINK-USD.COINBASE"),
            InstrumentId::from("BCH-USD.COINBASE"),
        ];
        let mut session = CoinbaseSession::connect(&args).await;
        subscribe_books(&mut session, &ids, args.scenario() == "initial").await;

        if args.scenario() == "edges" {
            recover_budget(&mut session, &ids).await;
            recover_rejected(&mut session, &ids).await;
        } else {
            for round in 0..args.rounds() {
                if args.scenario() == "disabled" {
                    disabled_round(&mut session, &ids, round).await;
                } else {
                    run_round(&mut session, &ids, round).await;
                }

                session.round(round, &format!("books={}", ids.len()));
            }
        }

        interrupt_recovery(&mut session, &ids).await;
        session.stop().await
    });
}

async fn subscribe_books(session: &mut CoinbaseSession, ids: &[InstrumentId], missing: bool) {
    if missing {
        for id in ids {
            session.fault(id).silence = true;
        }
    }

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
    if missing {
        let attempts = ids
            .iter()
            .map(|id| session.fault(id).unsubscribes)
            .collect::<Vec<_>>();

        // Each silenced frame is also a gap for the other books, so a book may recover twice
        assert!(attempts.iter().all(|count| *count >= 1), "{attempts:?}");
        assert_eq!(session.proxy().connections(STREAM), 1);
        stress::check(
            "initial",
            format!(
                "missing_snapshots={} attempts={attempts:?} reconnects=0",
                ids.len()
            ),
        );
    }
}

async fn run_round(session: &mut CoinbaseSession, ids: &[InstrumentId], round: usize) {
    match round % 6 {
        0 => recover_gap(session, ids, round).await,
        1 => recover_missing_snapshot(session, ids).await,
        2 => held_frames(session, ids).await,
        3 => reconnect_recovery(session, ids).await,
        4 => resubscribe(session, ids).await,
        _ => corrupt_sequence(session, ids).await,
    }
}

async fn disabled_round(session: &mut CoinbaseSession, ids: &[InstrumentId], round: usize) {
    match round % 4 {
        0 => recover_gap(session, ids, round).await,
        1 => reconnect_recovery(session, ids).await,
        2 => resubscribe(session, ids).await,
        _ => corrupt_sequence(session, ids).await,
    }
}

fn unsubscribes(session: &CoinbaseSession, ids: &[InstrumentId]) -> Vec<usize> {
    ids.iter()
        .map(|id| session.fault(id).unsubscribes)
        .collect()
}

// Drops every snapshot and one update, so the gap starts a recovery for each book that cannot
// complete. Silence isn't enough here since the relay lifts it on the first frame after a
// reconnect.
fn starve_recoveries(session: &CoinbaseSession, ids: &[InstrumentId]) -> Vec<usize> {
    for id in ids {
        session.fault(id).drop_snapshots = usize::MAX;
    }
    session.fault(&ids[0]).drop_updates = 1;
    unsubscribes(session, ids)
}

async fn wait_for_recoveries(
    session: &mut CoinbaseSession,
    ids: &[InstrumentId],
    before: &[usize],
    attempts: usize,
    limit: Duration,
    reason: &str,
) {
    session
        .until(limit, reason, |s| {
            ids.iter()
                .zip(before)
                .all(|(id, count)| s.fault(id).unsubscribes >= count + attempts)
        })
        .await;
}

// Drops one update of a single book; the gap must recover every book
async fn recover_gap(session: &mut CoinbaseSession, ids: &[InstrumentId], round: usize) {
    let connections = session.proxy().connections(STREAM);
    let before = unsubscribes(session, ids);
    let target = ids[round / 6 % ids.len()];
    session.expect_all();
    let dropped = {
        let mut fault = session.fault(&target);
        fault.drop_updates = 1;
        fault.dropped
    };

    session
        .until(Duration::from_secs(20), "update dropped", |s| {
            s.fault(&target).dropped > dropped
        })
        .await;
    wait_for_recoveries(
        session,
        ids,
        &before,
        1,
        Duration::from_secs(20),
        "every book replaced after gap",
    )
    .await;

    session.healthy(ids).await;
    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check(
        "gap",
        format!(
            "dropped={} books_recovered={} reconnects=0",
            target.symbol,
            ids.len()
        ),
    );
}

// Recovery snapshots are dropped until each book's deadline has forced a retry
async fn recover_missing_snapshot(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    let connections = session.proxy().connections(STREAM);
    let before = unsubscribes(session, ids);
    session.expect_all();

    for id in ids {
        session.fault(id).drop_snapshots = usize::MAX;
    }
    session.fault(&ids[0]).drop_updates = 1;

    session
        .until(Duration::from_secs(60), "snapshot deadline retries", |s| {
            let mut ready = true;

            for (id, count) in ids.iter().zip(&before) {
                let mut fault = s.fault(id);
                if fault.unsubscribes >= count + 2 {
                    fault.drop_snapshots = 0;
                } else {
                    ready = false;
                }
            }

            ready
        })
        .await;

    session.healthy(ids).await;
    let attempts = ids
        .iter()
        .zip(before)
        .map(|(id, count)| session.fault(id).unsubscribes - count)
        .collect::<Vec<_>>();

    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check("missing", format!("reconnects=0 attempts={attempts:?}"));
}

// Holds each recovery snapshot and everything behind it; the released frames arrive with
// obsolete sequence numbers and must not reach a book
async fn held_frames(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    session.expect_all();
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.hold_snapshot = true;
            (fault.unsubscribes, fault.held)
        })
        .collect::<Vec<_>>();
    session.fault(&ids[0]).drop_updates = 1;

    session
        .until(
            Duration::from_secs(40),
            "held snapshots force recovery",
            |s| {
                ids.iter().zip(&before).all(|(id, (unsubscribes, held))| {
                    let fault = s.fault(id);
                    fault.unsubscribes > *unsubscribes && fault.held > *held
                })
            },
        )
        .await;

    for id in ids {
        let mut fault = session.fault(id);
        fault.hold_snapshot = false;
        fault.hold = false;
    }

    session.proxy().release();
    session.healthy(ids).await;
    stress::check("delayed", "obsolete_frames_released=true");
}

async fn reconnect_recovery(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    let before = starve_recoveries(session, ids);

    wait_for_recoveries(
        session,
        ids,
        &before,
        1,
        Duration::from_secs(20),
        "recovery replacements before reconnect",
    )
    .await;

    session.venue_mut().expected_epoch = session.proxy().connections(STREAM) + 1;
    session.reconnect(ENDPOINT);
    for id in ids {
        session.fault(id).drop_snapshots = 0;
    }

    session.healthy(ids).await;
    stress::check(
        "reconnect",
        format!("mid_recovery=true fresh_snapshots={}", ids.len()),
    );
}

async fn resubscribe(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    for id in ids {
        session.unsubscribe(*id);
    }

    session.observe(Duration::from_secs(1)).await;
    for id in ids {
        session.close(*id);
    }

    session.observe(Duration::from_secs(1)).await;
    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
}

// Repeats one envelope's `sequence_num`, which is a gap without dropping anything
async fn corrupt_sequence(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    let before = unsubscribes(session, ids);
    session.expect_all();
    let corrupted = {
        let mut fault = session.fault(&ids[0]);
        fault.corrupt = 1;
        fault.corrupted
    };

    session
        .until(Duration::from_secs(20), "sequence rewritten", |s| {
            s.fault(&ids[0]).corrupted > corrupted
        })
        .await;
    wait_for_recoveries(
        session,
        ids,
        &before,
        1,
        Duration::from_secs(20),
        "every book replaced after repeated sequence",
    )
    .await;

    session.healthy(ids).await;
    stress::check("corrupt", "repeated_sequence_recovered=true");
}

// Leaves recoveries running into the shutdown, which must cancel them
async fn interrupt_recovery(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    let before = starve_recoveries(session, ids);
    wait_for_recoveries(
        session,
        ids,
        &before,
        1,
        Duration::from_secs(20),
        "recoveries active before shutdown",
    )
    .await;

    session.reconnect(ENDPOINT);
}

async fn recover_budget(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    session.expect_all();
    let before = starve_recoveries(session, ids);

    session
        .until(Duration::from_secs(240), "eight recovery attempts", |s| {
            ids.iter()
                .zip(&before)
                .all(|(id, count)| s.fault(id).unsubscribes >= count + 8)
        })
        .await;

    session.observe(Duration::from_secs(5)).await;

    for (id, count) in ids.iter().zip(&before) {
        assert_eq!(session.fault(id).unsubscribes, count + 8);
        session.fault(id).drop_snapshots = 0;
    }

    session.healthy_within(ids, Duration::from_secs(90)).await;
    for (id, count) in ids.iter().zip(before) {
        assert_eq!(session.fault(id).unsubscribes, count + 9);
    }

    stress::check(
        "budget",
        "attempts=8 late_snapshot_restored_at_ceiling=true",
    );
}

// Coinbase reports a refused subscribe only as an error frame without a product, so the
// recovery sees a missing snapshot and retries within its budget
async fn recover_rejected(session: &mut CoinbaseSession, ids: &[InstrumentId]) {
    session.expect_all();
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.reject = 1;
            fault.rejected
        })
        .collect::<Vec<_>>();
    session.fault(&ids[0]).drop_updates = 1;

    session
        .until(
            Duration::from_secs(20),
            "replacement subscribes rejected",
            |s| {
                ids.iter()
                    .zip(&before)
                    .all(|(id, rejected)| s.fault(id).rejected > *rejected)
            },
        )
        .await;

    // The unsubscribe before each rejected subscribe went through, so a book can only stream
    // again once a retry subscribes it
    session.healthy_within(ids, Duration::from_secs(60)).await;
    let rejected = ids
        .iter()
        .zip(&before)
        .map(|(id, count)| session.fault(id).rejected - count)
        .collect::<Vec<_>>();

    stress::check(
        "rejected",
        format!("retried_after_rejection=true rejected={rejected:?}"),
    );
}

struct Coinbase {
    wire: CoinbaseWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Coinbase {
    const NAME: &'static str = "coinbase";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "disabled", "edges"];
    const ROUNDS: usize = 18;
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(30);

    fn self_check() {
        let wire = CoinbaseWire::default();
        let route = Route {
            name: STREAM,
            path: "/ws",
            upstream: String::new(),
            endpoint: ENDPOINT,
            headers: &[],
        };
        let mut connection = wire.open(&route, 3);

        let setup: Vec<Value> = serde_json::from_str(include_str!(
            "../../test_data/ws_l2_resubscribe_subscriptions.json"
        ))
        .unwrap();
        for step in &setup {
            let request = Message::Text(step["request"].to_string().into());
            let unsubscribed = connection.client(&request);
            let expected: &[&str] = if step["request"]["type"] == "unsubscribe" {
                &["BTC-USD"]
            } else {
                &[]
            };
            assert_eq!(unsubscribed, expected);
        }

        let frames = include_str!("../../test_data/ws_l2_resubscribe_capture.jsonl")
            .lines()
            .collect::<Vec<_>>();
        let mut snapshots = Vec::new();
        let mut unsubscribed = Vec::new();

        for (index, frame) in frames.iter().enumerate() {
            match connection.upstream(&Message::Text((*frame).into())) {
                Upstream::Book {
                    key,
                    kind: FrameKind::Snapshot,
                } => snapshots.push((index, key)),
                Upstream::Unsubscribed(key) => unsubscribed.push((index, key)),
                _ => {}
            }
        }

        // Only the unsubscribe ack removes a product from the set
        assert_eq!(
            snapshots,
            [
                (0, "BTC-USD".to_string()),
                (7, "ETH-USD".to_string()),
                (281, "BTC-USD".to_string()),
            ]
        );
        assert_eq!(unsubscribed, [(238, "BTC-USD".to_string())]);

        // The resubscribe snapshot replaces the book rather than adding to it
        let envelope: Value = serde_json::from_str(frames[281]).unwrap();
        let timestamp = parse_timestamp(envelope["timestamp"].as_str().unwrap());
        let view = wire
            .views
            .find("BTC-USD", VIEW_SEQUENCE, timestamp)
            .expect("view recorded for snapshot");
        let (bids, asks) = parse_levels(&envelope["events"][0]["updates"]);
        assert_eq!(view.epoch, 3);
        assert_eq!(view.book.bids, bids.into_iter().collect());
        assert_eq!(view.book.asks, asks.into_iter().collect());

        // A rate-limited request gets an error reply instead of an ack
        let mut limited = wire.open(&route, 2);
        let replies = [
            subscriptions_ack(&["BTC-USD", "ETH-USD"]),
            json!({"type": "error", "message": "rate limit exceeded"}),
            subscriptions_ack(&["ETH-USD"]),
        ];
        let classified = replies
            .iter()
            .map(|reply| limited.upstream(&Message::Text(reply.to_string().into())))
            .collect::<Vec<_>>();
        assert_eq!(
            classified,
            [
                Upstream::Other,
                Upstream::Other,
                Upstream::Unsubscribed("BTC-USD".to_string()),
            ]
        );

        let mut rejected = wire.open(&route, 1);
        let request = json!({"type": "subscribe", "channel": "level2", "product_ids": ["ETH-USD"]});
        let (key, _) = rejected
            .reject(&Message::Text(request.to_string().into()))
            .expect("level2 subscribe can be rejected");
        assert_eq!(key, "ETH-USD");

        let mut repeated = Message::Text(frames[20].into());
        assert!(rejected.corrupt(&mut repeated, "BTC-USD", FrameKind::Update));
        let repeated: Value = serde_json::from_str(repeated.to_text().unwrap()).unwrap();
        assert_eq!(repeated["sequence_num"], 19);
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: CoinbaseWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *COINBASE_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "/ws",
            upstream: "wss://advanced-trade-ws.coinbase.com".to_owned(),
            endpoint: ENDPOINT,
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        if args.scenario() == "disabled" {
            anyhow::ensure!(
                args.timeout_secs() == 0,
                "disabled scenario requires --timeout 0"
            );
        }

        let config = CoinbaseDataClientConfig {
            base_url_ws: Some(format!("ws://{proxy}/ws")),
            book_snapshot_timeout_secs: args.timeout_secs(),
            ..CoinbaseDataClientConfig::default()
        };
        Ok(Box::new(CoinbaseDataClient::new(
            *COINBASE_CLIENT_ID,
            config,
        )?))
    }

    fn key(&self, id: &InstrumentId) -> String {
        id.symbol.to_string()
    }

    fn endpoint(&self, _id: &InstrumentId) -> &'static str {
        ENDPOINT
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let key = self.key(&id);
        let Some(view) = self
            .wire
            .views
            .find(&key, VIEW_SEQUENCE, deltas.ts_event.as_u64())
        else {
            let recorded = self.wire.recorded.lock().get(&key).copied();
            panic!(
                "wire oracle at emitted timestamp: key={key} ts_event={} deltas={} recorded={recorded:?}",
                deltas.ts_event,
                deltas.deltas.len(),
            );
        };
        checker
            .verify(id, DEPTH, &view.book.bids, &view.book.asks)
            .expect("book equals relayed venue book");
        self.epochs.insert(id, view.epoch);
    }

    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches >= start.batches + 2
    }
}

#[derive(Clone, Default)]
struct CoinbaseWire {
    views: WireViews,
    // Views recorded and the newest timestamp per key, for oracle misses
    recorded: Arc<parking_lot::Mutex<HashMap<String, (usize, u64)>>>,
}

impl WireCodec for CoinbaseWire {
    fn open(&self, _route: &Route, epoch: usize) -> Box<dyn WireConnection> {
        Box::new(CoinbaseConnection {
            views: self.views.clone(),
            recorded: Arc::clone(&self.recorded),
            epoch,
            books: HashMap::new(),
            level2: Vec::new(),
        })
    }
}

struct CoinbaseConnection {
    views: WireViews,
    recorded: Arc<parking_lot::Mutex<HashMap<String, (usize, u64)>>>,
    epoch: usize,
    books: HashMap<String, WireBook>,
    // Products in the latest `subscriptions` ack, which always lists the full set. Rate-limited
    // requests get an error reply instead of an ack, so acks can't be matched to requests by order
    level2: Vec<String>,
}

impl CoinbaseConnection {
    fn record(&mut self, timestamp: u64, event: &Value) -> Option<(String, FrameKind)> {
        let product = event["product_id"].as_str()?.to_owned();
        let kind = match event["type"].as_str()? {
            "snapshot" => FrameKind::Snapshot,
            "update" => FrameKind::Update,
            _ => return None,
        };

        let book = self.books.entry(product.clone()).or_default();
        if kind == FrameKind::Snapshot {
            *book = WireBook::default();
        }

        let (bids, asks) = parse_levels(&event["updates"]);
        book.apply(&bids, &asks);
        let mut recorded = self.recorded.lock();
        let entry = recorded.entry(product.clone()).or_default();
        *entry = (entry.0 + 1, timestamp);
        drop(recorded);
        self.views.record(
            &product,
            WireView {
                epoch: self.epoch,
                sequence: VIEW_SEQUENCE,
                timestamp,
                book: book.top(DEPTH),
            },
        );

        Some((product, kind))
    }
}

impl WireConnection for CoinbaseConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(envelope) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        match envelope["channel"].as_str() {
            Some("subscriptions") => {
                let level2 = envelope["events"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|event| {
                        event["subscriptions"]["level2"]
                            .as_array()
                            .into_iter()
                            .flatten()
                    })
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let previous = std::mem::replace(&mut self.level2, level2);

                match previous
                    .into_iter()
                    .find(|product| !self.level2.contains(product))
                {
                    Some(product) => Upstream::Unsubscribed(product),
                    None => Upstream::Other,
                }
            }
            Some("l2_data") => {
                let timestamp = parse_timestamp(envelope["timestamp"].as_str().unwrap());
                let mut first = None;

                for event in envelope["events"].as_array().into_iter().flatten() {
                    let recorded = self.record(timestamp, event);
                    first = first.or(recorded);
                }

                match first {
                    Some((key, kind)) => Upstream::Book { key, kind },
                    None => Upstream::Other,
                }
            }
            _ => Upstream::Other,
        }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        let Message::Text(text) = message else {
            return Vec::new();
        };

        let Ok(request) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };

        let products = request["product_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();

        if request["type"] == "unsubscribe" && request["channel"] == "level2" {
            products
        } else {
            Vec::new()
        }
    }

    fn corrupt(&mut self, message: &mut Message, _key: &str, _kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        let mut envelope: Value = serde_json::from_str(text).unwrap();
        let Some(repeated) = envelope["sequence_num"]
            .as_u64()
            .and_then(|sequence_num| sequence_num.checked_sub(1))
        else {
            return false;
        };

        envelope["sequence_num"] = json!(repeated);
        *message = Message::Text(envelope.to_string().into());
        true
    }

    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let Message::Text(text) = message else {
            return None;
        };

        let request: Value = serde_json::from_str(text).ok()?;
        if request["type"] != "subscribe" || request["channel"] != "level2" {
            return None;
        }

        let product = request["product_ids"][0].as_str()?;
        let reply = json!({"type": "error", "message": "Failed to subscribe"});
        Some((product.to_owned(), Message::Text(reply.to_string().into())))
    }
}

fn subscriptions_ack(level2: &[&str]) -> Value {
    json!({"channel": "subscriptions", "events": [{"subscriptions": {"level2": level2}}]})
}

fn parse_timestamp(timestamp: &str) -> u64 {
    let nanos = jiff::Timestamp::from_str(timestamp)
        .expect("envelope timestamp parses")
        .as_nanosecond();
    u64::try_from(nanos).expect("timestamp after the epoch")
}

type Levels = Vec<(Decimal, Decimal)>;

fn parse_levels(updates: &Value) -> (Levels, Levels) {
    let mut bids = Vec::new();
    let mut asks = Vec::new();

    for update in updates.as_array().into_iter().flatten() {
        let level = (
            Decimal::from_str(update["price_level"].as_str().unwrap()).unwrap(),
            Decimal::from_str(update["new_quantity"].as_str().unwrap()).unwrap(),
        );

        match update["side"].as_str() {
            Some("bid") => bids.push(level),
            Some("offer") => asks.push(level),
            side => panic!("unexpected level2 side {side:?}"),
        }
    }

    (bids, asks)
}

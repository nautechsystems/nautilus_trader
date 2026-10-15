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

//! Production market-data fault injection with an independent decimal order book oracle.
//!
//! Requires `COINAPI_API_KEY` with book stream access and a free connection slot. Run with:
//! `cargo test -p nautilus-coinapi --features examples --test coinapi-book-stress -- --timeout 10 --rounds 14`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates seven fault phases over `--rounds` rounds.
//! - `initial`: silences one book's first snapshots in each of `--rounds` fresh sessions.
//! - `boundaries`: probes retry exhaustion into the retry ceiling, the reconnect wake at the
//!   ceiling, unsubscribe during recovery, and shutdown during a reconnect.
//! - `freeze`: pauses all traffic long enough for the idle timeout to reconnect, in each of
//!   `--rounds` rounds.
//!
//! No orders are submitted. CoinAPI streams one book type per connection and replays a pair's
//! snapshot only when the pair leaves the session scope and returns, so every recovery shows as a
//! scope without the book's symbol. Every emitted batch passes through the shared
//! `BookStreamChecker`, which requires sequences to rise within each snapshot episode, and is
//! verified against a reference book rebuilt from the raw frames the proxy relays; a session fails
//! unless every snapshot episode was verified.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use jiff::Timestamp;
use nautilus_coinapi::{
    config::{CoinApiDataClientConfig, CoinApiInstrumentSelection},
    data::CoinApiDataClient,
    http::query::SymbolsQuery,
};
use nautilus_common::clients::DataClient;
use nautilus_core::serialization::decimal;
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    enums::BookAction,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::websocket::TransportBackend;
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, FrameKind, Route, Session, StressArgs, StressVenue, Upstream, WireBook,
    WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

// Instrument and CoinAPI symbol, which also filters the metadata that loads it. Each CoinAPI price
// resolution covers the prices Coinbase quotes, and each book updates at most about 150 times a
// second, so full-depth checking stays within the oracle's view window after a snapshot burst.
const BOOKS: [(&str, &str); 3] = [
    ("DOGE-USD.COINBASE", "COINBASE_SPOT_DOGE_USD"),
    ("LINK-USD.COINBASE", "COINBASE_SPOT_LINK_USD"),
    ("BCH-USD.COINBASE", "COINBASE_SPOT_BCH_USD"),
];

const STREAM: &str = "stream";
const ENDPOINT: &str = "coinapi-market-data";
const DEPTH: usize = 20;
// Replacement attempts in the shared retry budget, including the first
const BUDGET: usize = 8;
const FREEZE: Duration = Duration::from_secs(40);
const SNAPSHOTS_MAX: usize = 64;

type CoinApiSession = Session<CoinApi>;

fn main() {
    stress::run::<CoinApi, _, _>(|args| async move {
        let ids = BOOKS.map(|(id, _)| InstrumentId::from(id));
        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "initial" => initial(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
            "freeze" => freeze(&args, &ids).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let drops = usize::from(args.timeout_secs() > 0);
    let mut total = 0;
    let mut session = CoinApiSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    for round in 0..args.rounds() {
        let phase = round % 7;
        let phase_started = Instant::now();

        match phase {
            // A dropped update is a sequence gap that restarts only the book's pair
            0 => recover(&mut session, ids, |fault| fault.drop_updates = 1, 0).await,
            // A broken sequence link is a gap, and a lost replacement snapshot needs the deadline
            1 => {
                recover(
                    &mut session,
                    ids,
                    |fault| {
                        fault.corrupt = 1;
                        fault.drop_snapshots = 2 * drops;
                    },
                    drops,
                )
                .await;
            }
            // A reconnect during recovery keeps each running recovery and resyncs every book
            2 => {
                let before = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.corrupt = 1;
                        fault.drop_snapshots = 2;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(60),
                        "simultaneous recoveries lose snapshots",
                        |s| {
                            before
                                .iter()
                                .all(|(id, dropped)| s.fault(id).dropped > *dropped)
                        },
                    )
                    .await;

                for id in ids {
                    session.fault(id).drop_snapshots = 0;
                }

                reconnect(&mut session);
                session.healthy(ids).await;
            }
            // Frames held across a reconnect arrive late and must not reach the resynced books
            3 => {
                for id in ids {
                    session.fault(id).hold = true;
                }

                session.observe(Duration::from_secs(2)).await;
                reconnect(&mut session);
                session.observe(Duration::from_secs(4)).await;
                for id in ids {
                    session.fault(id).hold = false;
                }

                session.proxy().release();
                session.healthy(ids).await;
            }
            // One book leaves and rejoins while the others stream
            4 => {
                let id = ids[round / 7 % ids.len()];
                session.unsubscribe(id);
                session.observe(Duration::from_secs(2)).await;
                session.close(id);
                session.observe(Duration::from_secs(3)).await;
                session.subscribe(id);
                session.healthy(&[id]).await;
            }
            // Removing every book closes the connection; subscribing again opens a new one
            5 => {
                for id in ids {
                    session.unsubscribe(*id);
                }

                session.observe(Duration::from_secs(2)).await;
                for id in ids {
                    session.close(*id);
                }

                session.observe(Duration::from_secs(3)).await;
                session.venue_mut().expected_epoch = session.proxy().connections(STREAM) + 1;
                subscribe_all(&mut session, ids);
                session.healthy(ids).await;
            }
            // Shutdown while recoveries still wait for snapshots
            6 => {
                let before = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.corrupt = 1;
                        fault.drop_snapshots = 3;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(60),
                        "shutdown with recovery snapshots missing",
                        |s| {
                            before
                                .iter()
                                .all(|(id, dropped)| s.fault(id).dropped > *dropped)
                        },
                    )
                    .await;

                total += session.batches();
                session = session.restart().await;
                subscribe_all(&mut session, ids);
                session.healthy(ids).await;
            }
            _ => unreachable!(),
        }

        session.observe(Duration::from_secs(5)).await;
        session.round(
            round,
            &format!(
                "phase={phase} phase_ms={} batches_total={}",
                phase_started.elapsed().as_millis(),
                total + session.batches()
            ),
        );
    }

    total += session.batches();
    session.stop().await;
    format!("books={} batches_total={total}", ids.len())
}

async fn initial(args: &StressArgs, ids: &[InstrumentId]) -> String {
    assert!(
        args.timeout_secs() > 0,
        "without a snapshot deadline a silenced book stays dark by design"
    );
    let mut total = 0;

    for round in 0..args.rounds() {
        let mut session = CoinApiSession::connect(args).await;

        // The other books keep the connection busy, so only the snapshot deadline can recover
        // the silenced one; silencing every book would trip the idle timeout first
        let id = ids[round % ids.len()];
        session.fault(&id).silence = true;

        subscribe_all(&mut session, ids);
        session.healthy(ids).await;

        let fault = session.fault(&id).clone();
        assert!(fault.dropped == 0 && !fault.silence);
        assert!(
            session.venue().restarts(&id) >= 1,
            "{id} restarted its pair"
        );
        assert_eq!(session.proxy().connections(STREAM), 1);
        session.observe(Duration::from_secs(2)).await;
        total += session.batches();
        session.stop().await;
        stress::check("initial", format!("round={} instrument={id}", round + 1));
    }

    format!("batches_total={total}")
}

async fn boundaries(args: &StressArgs, ids: &[InstrumentId]) -> String {
    assert!(
        args.timeout_secs() > 0,
        "retry exhaustion needs a snapshot deadline to fail each attempt"
    );
    let mut session = CoinApiSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    // A budget of lost snapshots leaves the next attempt to the one-minute retry ceiling
    let id = ids[0];
    let last = exhaust(&mut session, id).await;
    session.fault(&id).drop_snapshots = 0;
    session.expect_snapshot(id);
    session
        .healthy_within(&[id], Duration::from_secs(150))
        .await;
    let ceiling = session.venue().restarted_at(&id).duration_since(last);
    assert!(
        ceiling >= Duration::from_secs(55),
        "{id} restarted {ceiling:?} after its budget ended; expected the one-minute ceiling"
    );
    stress::check(
        "ceiling",
        format!(
            "instrument={id} attempts={BUDGET} ceiling_s={}",
            ceiling.as_secs()
        ),
    );

    // A reconnect ends the ceiling wait, and the replayed scope restarts the book at once
    let id = ids[1];
    exhaust(&mut session, id).await;
    session.fault(&id).drop_snapshots = 0;
    session.observe(Duration::from_secs(5)).await;
    let woken = Instant::now();
    reconnect(&mut session);
    session.healthy(ids).await;
    let wake = woken.elapsed();
    assert!(
        wake < Duration::from_secs(30),
        "{id} recovered {wake:?} after the reconnect; expected an immediate retry"
    );
    stress::check(
        "reconnect_wake",
        format!("instrument={id} wake_s={}", wake.as_secs()),
    );

    // Unsubscribing a recovering book stops its restarts
    let id = ids[2];

    let restarts = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.drop_snapshots = usize::MAX;
        session.venue().restarts(&id)
    };

    session
        .until(
            Duration::from_secs(60),
            "restarts lose snapshots before unsubscribe",
            |s| s.venue().restarts(&id) >= restarts + 2,
        )
        .await;
    session.unsubscribe(id);
    session.observe(Duration::from_secs(2)).await;
    session.close(id);
    let restarts = session.venue().restarts(&id);
    session.observe(Duration::from_secs(30)).await;
    assert_eq!(
        session.venue().restarts(&id),
        restarts,
        "{id} restarted after its unsubscribe"
    );
    session.fault(&id).drop_snapshots = 0;
    session.subscribe(id);
    session.healthy(&[id]).await;
    stress::check("unsubscribe", format!("instrument={id}"));

    let cuts = session.proxy().cuts();
    session.proxy().cut(Some(STREAM), FrameKind::Snapshot, 10);
    reconnect(&mut session);
    session
        .until(
            Duration::from_secs(60),
            "reconnect interrupted by shutdown",
            |s| s.proxy().cuts() > cuts,
        )
        .await;
    let batches = session.batches();
    session.stop().await;
    format!("batches_total={batches}")
}

// A one-connection tier may refuse the replacement while CoinAPI still holds the frozen socket
async fn freeze(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let mut session = CoinApiSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    for round in 0..args.rounds() {
        let started = Instant::now();
        session.venue_mut().expected_epoch = session.proxy().connections(STREAM) + 1;
        session.expect_all();
        session.proxy().freeze(FREEZE);
        session
            .healthy_within(ids, FREEZE + Duration::from_secs(150))
            .await;
        session.observe(Duration::from_secs(5)).await;
        session.round(
            round,
            &format!(
                "freeze_s={} heal_ms={} connections={}",
                FREEZE.as_secs(),
                started.elapsed().as_millis(),
                session.proxy().connections(STREAM)
            ),
        );
    }

    let batches = session.batches();
    session.stop().await;
    format!("batches_total={batches}")
}

// Applies `fault` to every book and waits for each to recover through its own restarts, then
// checks that no reconnect happened
async fn recover(
    session: &mut CoinApiSession,
    ids: &[InstrumentId],
    fault: impl Fn(&mut stress::Fault),
    extra_restarts: usize,
) {
    let connections = session.proxy().connections(STREAM);
    let before = ids
        .iter()
        .map(|id| {
            fault(&mut session.fault(id));
            (*id, session.venue().restarts(id))
        })
        .collect::<Vec<_>>();

    session.expect_all();
    session.healthy(ids).await;
    for (id, restarts) in before {
        let restarted = session.venue().restarts(&id) - restarts;
        assert!(
            restarted > extra_restarts,
            "{id} restarted {restarted} times; expected more than {extra_restarts}"
        );
    }

    assert_eq!(session.proxy().connections(STREAM), connections);
}

// Starts a gap recovery whose every snapshot is lost, returning once the attempt that ends the
// retry budget has missed its snapshot deadline
async fn exhaust(session: &mut CoinApiSession, id: InstrumentId) -> Instant {
    let restarts = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.drop_snapshots = usize::MAX;
        session.venue().restarts(&id)
    };

    session
        .until(Duration::from_secs(240), "retry budget exhausted", |s| {
            s.venue().restarts(&id) >= restarts + BUDGET
        })
        .await;

    // The last attempt waits two paced hellos and its snapshot deadline before it fails
    let last = session.venue().restarted_at(&id);
    let deadline = session.venue().snapshot_timeout + Duration::from_secs(5);
    session.observe(deadline).await;
    assert_eq!(
        session.venue().restarts(&id),
        restarts + BUDGET,
        "{id} restarted before the retry ceiling"
    );
    last
}

// Cuts the connection at its next update, requiring every book to resync from its successor, or
// from the connection after any pending cuts
fn reconnect(session: &mut CoinApiSession) {
    let expected = session.proxy().connections(STREAM) + 1 + session.proxy().cuts_pending();
    session.venue_mut().expected_epoch = expected;
    session.expect_all();
    session.proxy().cut(Some(STREAM), FrameKind::Update, 1);
}

fn subscribe_all(session: &mut CoinApiSession, ids: &[InstrumentId]) {
    for id in ids {
        session.subscribe(*id);
    }
}

fn key(symbol_id: &str) -> String {
    format!("book:{symbol_id}")
}

struct CoinApi {
    wire: CoinApiWire,
    snapshot_timeout: Duration,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
    // Emitted minus venue sequence for each book's current snapshot episode
    offsets: HashMap<InstrumentId, u64>,
}

impl CoinApi {
    fn restarts(&self, id: &InstrumentId) -> usize {
        self.wire
            .restarts
            .lock()
            .get(&self.key(id))
            .map_or(0, Vec::len)
    }

    fn restarted_at(&self, id: &InstrumentId) -> Instant {
        *self.wire.restarts.lock()[&self.key(id)]
            .last()
            .expect("book restarted")
    }
}

impl StressVenue for CoinApi {
    const NAME: &'static str = "coinapi";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "boundaries", "freeze"];
    const ROUNDS: usize = 14;
    const SEQUENCED: bool = true;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(90);

    fn self_check() {
        check_wire_oracle();
    }

    fn new(args: &StressArgs) -> Self {
        Self {
            wire: CoinApiWire::default(),
            snapshot_timeout: Duration::from_secs(args.timeout_secs()),
            expected_epoch: 1,
            epochs: HashMap::new(),
            offsets: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        ClientId::new("COINAPI")
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "/v1/",
            upstream: "wss://ws.coinapi.io/v1/".to_string(),
            endpoint: ENDPOINT,
            headers: &["x-coinapi-key", "x-referrer"],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let instruments = BOOKS
            .iter()
            .map(|(id, symbol_id)| CoinApiInstrumentSelection {
                exchange_id: "COINBASE".into(),
                query: SymbolsQuery {
                    filter_symbol_id: Some((*symbol_id).to_string()),
                    ..SymbolsQuery::default()
                },
                instrument_ids: vec![InstrumentId::from(*id)],
            })
            .collect();

        // Leaving api_key unset resolves COINAPI_API_KEY
        let config = CoinApiDataClientConfig::builder()
            .base_url_ws(format!("ws://{proxy}/v1/"))
            .instruments(instruments)
            .credit_budget(BOOKS.len() as u64)
            .transport_backend(TransportBackend::Tungstenite)
            .book_snapshot_timeout_secs(args.timeout_secs())
            .build();

        Ok(Box::new(CoinApiDataClient::new(self.client_id(), config)?))
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        let (_, symbol_id) = BOOKS
            .iter()
            .find(|(id, _)| InstrumentId::from(*id) == *instrument_id)
            .unwrap_or_else(|| panic!("{instrument_id} is a stressed book"));
        key(symbol_id)
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let key = self.key(&id);
        let ts = deltas.ts_event.as_u64();
        let snapshot = deltas.deltas[0].action == BookAction::Clear;

        // The adapter offsets venue sequences so they keep rising, and each snapshot sets the
        // offset. Snapshots can share a timestamp, so the emitted book picks among them.
        if snapshot {
            let candidates = self
                .wire
                .snapshots
                .lock()
                .get(&key)
                .map(|snapshots| {
                    snapshots
                        .iter()
                        .rev()
                        .filter(|(sequence, at)| *at == ts && *sequence <= deltas.sequence)
                        .map(|(sequence, _)| *sequence)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let book = checker.book(id).expect("checked book");

            let emitted = WireBook {
                bids: book.bids_as_map(Some(DEPTH)).into_iter().collect(),
                asks: book.asks_as_map(Some(DEPTH)).into_iter().collect(),
            };

            let sequence = candidates
                .iter()
                .copied()
                .find(|sequence| {
                    self.wire
                        .views
                        .find(&key, *sequence, ts)
                        .is_some_and(|view| view.book == emitted)
                })
                .or_else(|| candidates.first().copied())
                .unwrap_or_else(|| panic!("wire snapshot for {id} at ts={ts}"));

            self.offsets.insert(id, deltas.sequence - sequence);
        }

        let sequence = deltas.sequence - self.offsets[&id];

        let Some(view) = self.wire.views.find(&key, sequence, ts) else {
            panic!(
                "no wire oracle view for {id} venue_seq={sequence} ts={ts} snapshot={snapshot}; \
                 latest {key} frame {:?}",
                self.wire.latest.lock().get(&key),
            );
        };

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            let book = checker.book(id).expect("checked book");
            let top =
                |levels: Vec<(Decimal, Decimal)>| levels.into_iter().take(3).collect::<Vec<_>>();
            panic!(
                "wire oracle mismatch {id} seq={} venue_seq={sequence} offset={} epoch={} ts={}: \
                 {violation}; emitted bids={:?} asks={:?}; oracle bids={:?} asks={:?}; \
                 snapshots={:?}",
                deltas.sequence,
                self.offsets[&id],
                view.epoch,
                deltas.ts_event,
                top(book.bids_as_map(Some(DEPTH)).into_iter().collect()),
                top(book.asks_as_map(Some(DEPTH)).into_iter().collect()),
                top(view.book.bids.iter().rev().map(|(p, q)| (*p, *q)).collect()),
                top(view.book.asks.iter().map(|(p, q)| (*p, *q)).collect()),
                self.wire.snapshots.lock().get(&key),
            );
        }

        self.epochs.insert(id, view.epoch);
    }

    // A healthy book resynced on the expected connection and streams updates again
    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches >= start.batches + 3
    }

    fn stats(&self) -> String {
        let restarts: usize = self.wire.restarts.lock().values().map(Vec::len).sum();
        format!("restarts={restarts}")
    }
}

#[derive(Clone, Default)]
struct CoinApiWire {
    views: WireViews,
    // Epoch, sequence, and snapshot flag of each book's latest frame, for failure reports
    latest: Arc<Mutex<HashMap<String, (usize, u64, bool)>>>,
    // Recent snapshot frames of each book as (sequence, timestamp)
    snapshots: Arc<Mutex<HashMap<String, VecDeque<(u64, u64)>>>>,
    // Times each book left the session scope, across every connection
    restarts: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl WireCodec for CoinApiWire {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(CoinApiConnection {
            wire: self.clone(),
            route: route.name,
            epoch: number,
            books: HashMap::new(),
            scope: HashSet::new(),
        })
    }
}

struct CoinApiConnection {
    wire: CoinApiWire,
    route: &'static str,
    epoch: usize,
    books: HashMap<String, WireBook>,
    // Book fault keys the latest adapter hello listed
    scope: HashSet<String>,
}

// Decoded with the core exact-decimal deserializer, independent of the adapter's models
#[derive(Deserialize)]
struct WireFrame {
    #[serde(rename = "type")]
    kind: String,
    symbol_id: String,
    sequence: u64,
    time_exchange: Timestamp,
    is_snapshot: Option<bool>,
    asks: Vec<WireLevel>,
    bids: Vec<WireLevel>,
}

#[derive(Deserialize)]
struct WireLevel {
    #[serde(deserialize_with = "decimal::deserialize_json")]
    price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    size: Decimal,
}

impl WireConnection for CoinApiConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        if text.contains(r#""type":"error""#) {
            eprintln!("Venue error on route {}: {text}", self.route);
        }

        let Ok(frame) = serde_json::from_str::<WireFrame>(text) else {
            return Upstream::Other;
        };

        if frame.kind != "book" {
            return Upstream::Other;
        }

        let key = key(&frame.symbol_id);
        let snapshot = frame.is_snapshot == Some(true);
        let book = self.books.entry(key.clone()).or_default();
        if snapshot {
            *book = WireBook::default();
        }

        book.apply(&levels(&frame.bids), &levels(&frame.asks));
        let timestamp = u64::try_from(frame.time_exchange.as_nanosecond()).unwrap();
        self.wire
            .latest
            .lock()
            .insert(key.clone(), (self.epoch, frame.sequence, snapshot));

        if snapshot {
            let mut snapshots = self.wire.snapshots.lock();
            let snapshots = snapshots.entry(key.clone()).or_default();
            snapshots.push_back((frame.sequence, timestamp));
            if snapshots.len() > SNAPSHOTS_MAX {
                snapshots.pop_front();
            }
        }

        self.wire.views.record(
            &key,
            WireView {
                epoch: self.epoch,
                sequence: frame.sequence,
                timestamp,
                book: book.top(DEPTH),
            },
        );

        let kind = if snapshot {
            FrameKind::Snapshot
        } else {
            FrameKind::Update
        };

        Upstream::Book { key, kind }
    }

    // A hello that drops a book's pair from the scope restarts or unsubscribes that book
    fn client(&mut self, message: &Message) -> Vec<String> {
        let Some(scope) = hello_books(message) else {
            return Vec::new();
        };

        let removed = self.scope.difference(&scope).cloned().collect::<Vec<_>>();
        let now = Instant::now();
        let mut restarts = self.wire.restarts.lock();
        for key in &removed {
            restarts.entry(key.clone()).or_default().push(now);
        }

        self.scope = scope;
        removed
    }

    // Skips sequence numbers in an incremental frame, which the adapter must treat as a gap
    fn corrupt(&mut self, message: &mut Message, _key: &str, kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        if kind == FrameKind::Snapshot {
            return false;
        }

        let mut frame = serde_json::from_str::<Value>(text).unwrap();
        let sequence = frame["sequence"].as_u64().unwrap();
        frame["sequence"] = json!(sequence + 10);
        *message = Message::Text(frame.to_string().into());
        true
    }
}

// Returns the book fault keys a hello's scope includes, or `None` for any other message
fn hello_books(message: &Message) -> Option<HashSet<String>> {
    let Message::Text(text) = message else {
        return None;
    };

    let frame = serde_json::from_str::<Value>(text).ok()?;

    if frame["type"] != "hello" {
        return None;
    }

    let lists_book = frame["subscribe_data_type"]
        .as_array()?
        .iter()
        .any(|data_type| data_type == "book");

    if !lists_book {
        return Some(HashSet::new());
    }

    Some(
        frame["subscribe_filter_symbol_id"]
            .as_array()?
            .iter()
            .filter_map(|symbol| symbol.as_str()?.strip_suffix('$').map(key))
            .collect(),
    )
}

fn levels(levels: &[WireLevel]) -> Vec<(Decimal, Decimal)> {
    levels
        .iter()
        .map(|level| (level.price, level.size))
        .collect()
}

// Proves the wire oracle before any venue traffic, since a wrong oracle would pass a wrong book
fn check_wire_oracle() {
    let wire = CoinApiWire::default();

    let route = Route {
        name: STREAM,
        path: "/v1/",
        upstream: String::new(),
        endpoint: ENDPOINT,
        headers: &[],
    };

    let mut connection = wire.open(&route, 3);
    let text = |value: &str| Message::Text(value.into());
    let btc = key("COINBASE_SPOT_BTC_USD");
    let eth = key("COINBASE_SPOT_ETH_USD");
    let nanos =
        |time: &str| u64::try_from(time.parse::<Timestamp>().unwrap().as_nanosecond()).unwrap();

    let snapshot = text(
        r#"{"type":"book","symbol_id":"COINBASE_SPOT_BTC_USD","sequence":1,"time_exchange":"2026-09-27T10:52:27.0000005Z","time_coinapi":"2026-09-27T10:52:27.1Z","is_snapshot":true,"asks":[{"price":11,"size":4}],"bids":[{"price":10.0,"size":2},{"price":9,"size":3.10}]}"#,
    );
    let mut update = text(
        r#"{"type":"book","symbol_id":"COINBASE_SPOT_BTC_USD","sequence":2,"time_exchange":"2026-09-27T10:52:27.0000006Z","time_coinapi":"2026-09-27T10:52:27.1Z","is_snapshot":false,"asks":[{"price":12,"size":5}],"bids":[{"price":10,"size":0}]}"#,
    );
    let empty = text(
        r#"{"type":"book","symbol_id":"COINBASE_SPOT_BTC_USD","sequence":1,"time_exchange":"2026-09-27T10:52:28Z","time_coinapi":"2026-09-27T10:52:28.1Z","is_snapshot":true,"asks":[],"bids":[]}"#,
    );
    let depth = text(
        r#"{"type":"book5","symbol_id":"COINBASE_SPOT_BTC_USD","sequence":3,"time_exchange":"2026-09-27T10:52:28Z","time_coinapi":"2026-09-27T10:52:28.1Z","asks":[],"bids":[]}"#,
    );
    let heartbeat = text(r#"{"type":"hearbeat"}"#);

    let hello = |types: &str, symbols: &str| {
        text(&format!(
            r#"{{"type":"hello","heartbeat":true,"subscribe_data_type":[{types}],"subscribe_filter_symbol_id":[{symbols}]}}"#
        ))
    };

    let both = r#""COINBASE_SPOT_BTC_USD$","COINBASE_SPOT_ETH_USD$""#;

    let book = |kind| Upstream::Book {
        key: btc.clone(),
        kind,
    };

    assert_eq!(connection.upstream(&snapshot), book(FrameKind::Snapshot));
    assert_eq!(connection.upstream(&update), book(FrameKind::Update));
    assert_eq!(connection.upstream(&depth), Upstream::Other);
    assert_eq!(connection.upstream(&heartbeat), Upstream::Other);

    assert_eq!(
        [
            wire.views
                .find(&btc, 1, nanos("2026-09-27T10:52:27.0000005Z")),
            wire.views
                .find(&btc, 2, nanos("2026-09-27T10:52:27.0000006Z")),
        ],
        [
            Some(WireView {
                epoch: 3,
                sequence: 1,
                timestamp: nanos("2026-09-27T10:52:27.0000005Z"),
                book: WireBook {
                    bids: [
                        (Decimal::from(9), Decimal::new(31, 1)),
                        (Decimal::from(10), Decimal::from(2)),
                    ]
                    .into(),
                    asks: [(Decimal::from(11), Decimal::from(4))].into(),
                },
            }),
            Some(WireView {
                epoch: 3,
                sequence: 2,
                timestamp: nanos("2026-09-27T10:52:27.0000006Z"),
                book: WireBook {
                    bids: [(Decimal::from(9), Decimal::new(31, 1))].into(),
                    asks: [
                        (Decimal::from(11), Decimal::from(4)),
                        (Decimal::from(12), Decimal::from(5)),
                    ]
                    .into(),
                },
            }),
        ]
    );

    assert_eq!(connection.upstream(&empty), book(FrameKind::Snapshot));
    assert_eq!(
        wire.views.find(&btc, 1, nanos("2026-09-27T10:52:28Z")),
        Some(WireView {
            epoch: 3,
            sequence: 1,
            timestamp: nanos("2026-09-27T10:52:28Z"),
            book: WireBook::default(),
        })
    );

    // Adding books removes nothing, dropping the book type removes every pair, and dropping a
    // symbol removes its pair
    assert!(
        connection
            .client(&hello(r#""book","trade""#, both))
            .is_empty()
    );
    let mut removed = connection.client(&hello(r#""trade""#, both));
    removed.sort();
    assert_eq!(removed, [btc.clone(), eth.clone()]);
    assert!(connection.client(&hello(r#""book""#, both)).is_empty());
    assert_eq!(
        connection.client(&hello(r#""book""#, r#""COINBASE_SPOT_ETH_USD$""#)),
        [btc.clone()]
    );
    assert!(connection.client(&heartbeat).is_empty());
    assert_eq!(wire.restarts.lock()[&btc].len(), 2);
    assert_eq!(wire.restarts.lock()[&eth].len(), 1);

    let mut unchanged = snapshot.clone();
    assert!(!connection.corrupt(&mut unchanged, &btc, FrameKind::Snapshot));
    assert_eq!(unchanged, snapshot);
    assert!(connection.corrupt(&mut update, &btc, FrameKind::Update));
    let corrupted = serde_json::from_str::<Value>(update.to_text().unwrap()).unwrap();
    assert_eq!(corrupted["sequence"], json!(12));
}

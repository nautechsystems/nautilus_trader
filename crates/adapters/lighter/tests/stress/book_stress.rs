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

//! Mainnet market-data fault injection with an independent decimal order book oracle.
//!
//! Run with adapter credentials unset:
//! `cargo test -p nautilus-lighter --features examples --test lighter-book-stress -- --timeout 10 --rounds 12`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates six fault phases over `--rounds` rounds.
//! - `initial`: drops first snapshots in each of `--rounds` fresh sessions.
//! - `boundaries`: probes retry exhaustion into the retry ceiling, the reconnect wake at the
//!   ceiling, unsubscribe during recovery, and shutdown during a reconnect.
//!
//! No orders are submitted. Every emitted batch passes through the shared `BookStreamChecker`, which
//! requires nonces to rise within each snapshot episode, and is verified against a reference book
//! rebuilt from the raw frames the proxy relays; a session fails unless every snapshot episode was
//! verified.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use nautilus_common::clients::DataClient;
use nautilus_lighter::{
    common::{
        consts::LIGHTER_CLIENT_ID,
        enums::{LighterEnvironment, LighterProductType},
    },
    config::LighterDataClientConfig,
    data::LighterDataClient,
    http::{client::LighterRawHttpClient, query::LighterOrderBooksQuery},
};
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::mode::ReconnectRequestOutcome;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, FrameKind, Route, Session, StressArgs, StressVenue, Upstream, WireBook,
    WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const SYMBOLS: [&str; 6] = ["BTC", "ETH", "SOL", "DOGE", "XRP", "HYPE"];

const STREAM: &str = "stream";
const ENDPOINT: &str = "lighter-data-streams";
const DEPTH: usize = 20;
// Replacement attempts in the shared retry budget, including the first
const BUDGET: usize = 8;

// Market indices by instrument, loaded once from the venue before any session connects
static MARKETS: OnceLock<HashMap<InstrumentId, i64>> = OnceLock::new();

type LighterSession = Session<Lighter>;

fn main() {
    stress::run::<Lighter, _, _>(|args| async move {
        let markets = load_markets().await;

        let ids = SYMBOLS.map(|symbol| {
            let id = InstrumentId::from(format!("{symbol}-PERP.LIGHTER"));
            assert!(markets.contains_key(&id), "Lighter lists {id}");
            id
        });

        MARKETS.set(markets).expect("markets load once");

        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "initial" => initial(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let timeout = args.timeout_secs();
    let mut total = 0;
    let mut session = LighterSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;
    recover_without_reconnect(&mut session, ids, usize::from(timeout > 0)).await;

    for round in 0..args.rounds() {
        let phase = round % 6;
        let phase_started = Instant::now();

        match phase {
            0 => {
                let before = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.drop_updates = 1;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                session.expect_all();
                session.healthy(ids).await;

                for (id, dropped) in before {
                    assert_eq!(session.fault(&id).dropped, dropped + 1);
                }
            }
            1 => {
                let before = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.corrupt = 1;
                        fault.drop_snapshots = 1;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(20),
                        "simultaneous recoveries lose snapshots",
                        |s| {
                            before
                                .iter()
                                .all(|(id, dropped)| s.fault(id).dropped > *dropped)
                        },
                    )
                    .await;

                reconnect(&mut session);
                session.healthy(ids).await;
            }
            2 => {
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
            3 => {
                let id = ids[round / 6 % ids.len()];

                let rejected = {
                    let mut fault = session.fault(&id);
                    fault.corrupt = 1;
                    fault.reject = 1;
                    fault.rejected
                };

                session.expect_snapshot(id);
                session.healthy(&[id]).await;
                assert_eq!(session.fault(&id).rejected, rejected + 1);
            }
            4 => {
                session.proxy().cut(Some(STREAM), FrameKind::Snapshot, 2);
                let cuts = session.proxy().cuts();
                reconnect(&mut session);
                session
                    .until(
                        Duration::from_secs(120),
                        "two reconnects cut before snapshots",
                        |s| s.proxy().cuts() == cuts + 2,
                    )
                    .await;
                session.healthy(ids).await;
            }
            5 => {
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
                        Duration::from_secs(20),
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
    let timeout = args.timeout_secs();
    let mut total = 0;

    for round in 0..args.rounds() {
        let mut session = LighterSession::connect(args).await;

        for id in ids {
            let mut fault = session.fault(id);
            fault.drop_snapshots = 1;
            fault.silence = true;
        }

        subscribe_all(&mut session, ids);
        session
            .until(Duration::from_secs(20), "initial snapshots dropped", |s| {
                ids.iter().all(|id| s.fault(id).dropped == 1)
            })
            .await;

        if timeout == 0 {
            session.observe(Duration::from_secs(5)).await;
            assert_eq!(session.batches(), 0);
            assert!(
                session
                    .proxy()
                    .faults()
                    .values()
                    .all(|fault| fault.unsubscribes == 0)
            );
            reconnect(&mut session);
        }

        session.healthy(ids).await;

        for id in ids {
            let fault = session.fault(id).clone();
            assert_eq!(fault.corrupted, 0);
            assert_eq!(fault.dropped, 1);
            assert_eq!(fault.unsubscribes, usize::from(timeout > 0));
            assert_eq!(session.book(id).snapshots, 1);
        }

        let connections = if timeout > 0 { 1 } else { 2 };
        assert_eq!(session.proxy().connections(STREAM), connections);
        session.observe(Duration::from_secs(2)).await;
        total += session.batches();
        session.stop().await;
        stress::check(
            "initial",
            format!("round={} books={}", round + 1, ids.len()),
        );
    }

    format!("batches_total={total}")
}

async fn boundaries(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let mut session = LighterSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    // A rejected budget leaves the next attempt to the one-minute retry ceiling
    let id = ids[0];
    exhaust(&mut session, id).await;
    let exhausted = Instant::now();
    session.expect_snapshot(id);
    session
        .healthy_within(&[id], Duration::from_secs(150))
        .await;
    let ceiling = exhausted.elapsed();
    assert!(
        ceiling >= Duration::from_secs(55),
        "{id} recovered {ceiling:?} after its budget ended; expected the one-minute ceiling"
    );
    stress::check(
        "ceiling",
        format!(
            "instrument={id} rejected={BUDGET} ceiling_s={}",
            ceiling.as_secs()
        ),
    );

    // A reconnect ends the ceiling wait, so the recovery retries at once on the new connection
    let id = ids[1];
    exhaust(&mut session, id).await;
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

    // Unsubscribing a recovering book stops its replacement writes
    let id = ids[2];

    let rejected = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.reject = usize::MAX;
        fault.rejected
    };

    session
        .until(
            Duration::from_secs(30),
            "replacements rejected before unsubscribe",
            |s| s.fault(&id).rejected >= rejected + 2,
        )
        .await;
    session.unsubscribe(id);
    session.observe(Duration::from_secs(2)).await;
    session.close(id);
    let rejected = session.fault(&id).rejected;
    session.observe(Duration::from_secs(15)).await;
    assert_eq!(
        session.fault(&id).rejected,
        rejected,
        "{id} sent a replacement after its unsubscribe"
    );
    session.fault(&id).reject = 0;
    session.subscribe(id);
    session.healthy(&[id]).await;
    stress::check("unsubscribe", format!("instrument={id}"));

    let cuts = session.proxy().cuts();
    session.proxy().cut(Some(STREAM), FrameKind::Snapshot, 10);
    reconnect(&mut session);
    session
        .until(
            Duration::from_secs(30),
            "reconnect interrupted by shutdown",
            |s| s.proxy().cuts() > cuts,
        )
        .await;
    let batches = session.batches();
    session.stop().await;
    format!("batches_total={batches}")
}

// Forces gaps whose replacement snapshots are dropped `drops` times, then checks that each book
// recovered through its own resubscribes without a reconnect
async fn recover_without_reconnect(
    session: &mut LighterSession,
    ids: &[InstrumentId],
    drops: usize,
) {
    let connections = session.proxy().connections(STREAM);

    let before = ids
        .iter()
        .map(|id| {
            let snapshots = session.book(id).snapshots;
            let mut fault = session.fault(id);
            fault.corrupt = 1;
            fault.drop_snapshots = drops;
            (
                *id,
                fault.corrupted,
                fault.dropped,
                fault.unsubscribes,
                snapshots,
            )
        })
        .collect::<Vec<_>>();

    session.expect_all();
    session.healthy(ids).await;
    session.observe(Duration::from_secs(5)).await;

    for (id, corrupted, dropped, unsubscribes, snapshots) in before {
        let fault = session.fault(&id).clone();
        assert_eq!(fault.corrupted, corrupted + 1);
        assert_eq!(fault.dropped, dropped + drops);
        assert_eq!(fault.unsubscribes, unsubscribes + 1 + drops);
        assert_eq!(session.book(&id).snapshots, snapshots + 1);
    }

    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check(
        "autonomous",
        format!(
            "books={} dropped={} reconnects=0",
            ids.len(),
            ids.len() * drops
        ),
    );
}

// Starts a gap recovery and rejects every attempt in its retry budget, returning once the last
// rejection is sent
async fn exhaust(session: &mut LighterSession, id: InstrumentId) {
    let rejected = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.reject = BUDGET;
        fault.rejected
    };

    session
        .until(Duration::from_secs(120), "retry budget rejected", |s| {
            s.fault(&id).rejected == rejected + BUDGET
        })
        .await;
}

// Reconnects the stream, requiring every book to resync from its successor, or from the
// connection after any pending cuts
fn reconnect(session: &mut LighterSession) {
    let expected = session.proxy().connections(STREAM) + 1 + session.proxy().cuts_pending();
    session.venue_mut().expected_epoch = expected;

    assert_eq!(
        session.reconnect(ENDPOINT),
        ReconnectRequestOutcome::Accepted
    );
}

fn subscribe_all(session: &mut LighterSession, ids: &[InstrumentId]) {
    for id in ids {
        session.subscribe(*id);
    }
}

async fn load_markets() -> HashMap<InstrumentId, i64> {
    let client = LighterRawHttpClient::new(LighterEnvironment::Mainnet, None, 10, None)
        .expect("Lighter HTTP client builds");
    let books = client
        .get_order_books(&LighterOrderBooksQuery::default())
        .await
        .expect("Lighter order books load");

    books
        .order_books
        .iter()
        .filter(|book| book.market_type == LighterProductType::Perp)
        .map(|book| {
            let id = InstrumentId::from(format!("{}-PERP.LIGHTER", book.symbol));
            (id, book.market_id)
        })
        .collect()
}

struct Lighter {
    wire: LighterWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Lighter {
    const NAME: &'static str = "lighter";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "boundaries"];
    const ROUNDS: usize = 12;
    const SEQUENCED: bool = true;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(90);

    fn self_check() {
        check_wire_oracle();
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: LighterWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *LIGHTER_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "/stream",
            upstream: "wss://mainnet.zklighter.elliot.ai/stream?readonly=true".to_string(),
            endpoint: ENDPOINT,
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let config = LighterDataClientConfig::builder()
            .environment(LighterEnvironment::Mainnet)
            .base_url_ws(format!("ws://{proxy}/stream"))
            .update_instruments_interval_mins(0)
            .book_snapshot_timeout_secs(args.timeout_secs())
            .build();

        Ok(Box::new(LighterDataClient::new(
            *LIGHTER_CLIENT_ID,
            config,
        )?))
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        let markets = MARKETS.get().expect("markets load before sessions");
        format!("order_book:{}", markets[instrument_id])
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let view = self
            .wire
            .views
            .find(&self.key(&id), deltas.sequence, deltas.ts_event.as_u64())
            .expect("wire oracle at emitted nonce and timestamp");

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            panic!(
                "wire oracle mismatch {id} seq={} ts={}: {violation}",
                deltas.sequence, deltas.ts_event
            );
        }

        self.epochs.insert(id, view.epoch);
    }

    // A healthy book resynced on the expected connection and streams updates again
    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches >= start.batches + 3
    }
}

#[derive(Clone, Default)]
struct LighterWire {
    views: WireViews,
}

impl WireCodec for LighterWire {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(LighterConnection {
            views: self.views.clone(),
            route: route.name,
            epoch: number,
            books: HashMap::new(),
        })
    }
}

struct LighterConnection {
    views: WireViews,
    route: &'static str,
    epoch: usize,
    books: HashMap<String, WireBook>,
}

impl WireConnection for LighterConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        if frame["type"] == "error" || frame.get("error").is_some() {
            eprintln!("Venue error on route {}: {frame}", self.route);
        }

        let Some(key) = frame["channel"]
            .as_str()
            .filter(|channel| channel.starts_with("order_book:"))
            .map(ToString::to_string)
        else {
            return Upstream::Other;
        };

        let kind = match frame["type"].as_str() {
            Some("subscribed/order_book") => FrameKind::Snapshot,
            Some("update/order_book") => FrameKind::Update,
            Some("unsubscribed") => return Upstream::Unsubscribed(key),
            _ => return Upstream::Other,
        };

        let data = &frame["order_book"];
        let book = self.books.entry(key.clone()).or_default();
        apply(book, data, kind == FrameKind::Snapshot);
        self.views.record(
            &key,
            WireView {
                epoch: self.epoch,
                sequence: data["nonce"].as_u64().unwrap(),
                timestamp: frame["timestamp"].as_u64().unwrap() * 1_000_000,
                book: book.top(DEPTH),
            },
        );

        Upstream::Book { key, kind }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        book_command(message, "unsubscribe").into_iter().collect()
    }

    // Breaks the nonce link of an incremental frame, which the adapter must treat as a gap
    fn corrupt(&mut self, message: &mut Message, _key: &str, kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        if kind == FrameKind::Snapshot {
            return false;
        }

        let mut frame = serde_json::from_str::<Value>(text).unwrap();
        frame["order_book"]["begin_nonce"] = json!(-1);
        *message = Message::Text(frame.to_string().into());
        true
    }

    // Answers a book subscribe as the venue does when it rate-limits subscriptions, which the
    // adapter retries within its budget
    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let key = book_command(message, "subscribe")?;
        let reply = json!({"type": "error", "code": 30009, "message": "rate limit exceeded"});
        Some((key, Message::Text(reply.to_string().into())))
    }
}

// Returns the fault key of an adapter book command of type `kind`, such as `order_book/1`
fn book_command(message: &Message, kind: &str) -> Option<String> {
    let Message::Text(text) = message else {
        return None;
    };

    let frame = serde_json::from_str::<Value>(text).ok()?;

    if frame["type"] != kind {
        return None;
    }

    let index = frame["channel"].as_str()?.strip_prefix("order_book/")?;
    Some(format!("order_book:{index}"))
}

fn apply(book: &mut WireBook, data: &Value, snapshot: bool) {
    if snapshot {
        *book = WireBook::default();
    }

    book.apply(&levels(&data["bids"]), &levels(&data["asks"]));
}

fn levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                Decimal::from_str(row["price"].as_str().unwrap()).unwrap(),
                Decimal::from_str(row["size"].as_str().unwrap()).unwrap(),
            )
        })
        .collect()
}

// Proves the wire oracle before any venue traffic, since a wrong oracle would pass a wrong book
fn check_wire_oracle() {
    let wire = LighterWire::default();

    let route = Route {
        name: STREAM,
        path: "/stream",
        upstream: String::new(),
        endpoint: ENDPOINT,
        headers: &[],
    };

    let mut connection = wire.open(&route, 3);
    let text = |value: Value| Message::Text(value.to_string().into());

    let frame = |kind: &str, nonce: u64, timestamp: u64, bids: Value, asks: Value| {
        text(json!({
            "type": kind,
            "channel": "order_book:1",
            "timestamp": timestamp,
            "offset": 1,
            "order_book": {
                "code": 0,
                "bids": bids,
                "asks": asks,
                "offset": 1,
                "nonce": nonce,
                "last_updated_at": 1,
                "begin_nonce": nonce - 1,
            },
        }))
    };

    let snapshot = frame(
        "subscribed/order_book",
        7,
        5,
        json!([{"price": "10.0", "size": "2"}, {"price": "9", "size": "3"}]),
        json!([{"price": "11", "size": "4"}]),
    );
    let mut update = frame(
        "update/order_book",
        8,
        6,
        json!([{"price": "10", "size": "0"}]),
        json!([{"price": "12", "size": "5"}]),
    );
    let empty = frame("subscribed/order_book", 9, 7, json!([]), json!([]));
    let ack = text(json!({"type": "unsubscribed", "channel": "order_book:1"}));
    let ticker = text(json!({"type": "update/ticker", "channel": "ticker:1"}));
    let subscribe = text(json!({"type": "subscribe", "channel": "order_book/1"}));
    let unsubscribe = text(json!({"type": "unsubscribe", "channel": "order_book/1"}));

    let book = |kind| Upstream::Book {
        key: "order_book:1".to_string(),
        kind,
    };

    assert_eq!(connection.upstream(&snapshot), book(FrameKind::Snapshot));
    assert_eq!(connection.upstream(&update), book(FrameKind::Update));
    assert_eq!(
        connection.upstream(&ack),
        Upstream::Unsubscribed("order_book:1".to_string())
    );
    assert_eq!(connection.upstream(&ticker), Upstream::Other);
    assert_eq!(connection.upstream(&subscribe), Upstream::Other);
    assert_eq!(connection.client(&unsubscribe), ["order_book:1"]);
    assert!(connection.client(&subscribe).is_empty());
    assert_eq!(connection.reject(&unsubscribe), None);
    assert_eq!(
        connection.reject(&subscribe),
        Some((
            "order_book:1".to_string(),
            text(json!({"type": "error", "code": 30009, "message": "rate limit exceeded"})),
        ))
    );

    assert_eq!(
        [
            wire.views.find("order_book:1", 7, 5_000_000),
            wire.views.find("order_book:1", 8, 6_000_000),
        ],
        [
            Some(WireView {
                epoch: 3,
                sequence: 7,
                timestamp: 5_000_000,
                book: WireBook {
                    bids: [
                        (Decimal::from(9), Decimal::from(3)),
                        (Decimal::from(10), Decimal::from(2)),
                    ]
                    .into(),
                    asks: [(Decimal::from(11), Decimal::from(4))].into(),
                },
            }),
            Some(WireView {
                epoch: 3,
                sequence: 8,
                timestamp: 6_000_000,
                book: WireBook {
                    bids: [(Decimal::from(9), Decimal::from(3))].into(),
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
        wire.views.find("order_book:1", 9, 7_000_000),
        Some(WireView {
            epoch: 3,
            sequence: 9,
            timestamp: 7_000_000,
            book: WireBook::default(),
        })
    );

    let mut unchanged = snapshot.clone();
    assert!(!connection.corrupt(&mut unchanged, "order_book:1", FrameKind::Snapshot));
    assert_eq!(unchanged, snapshot);
    assert!(connection.corrupt(&mut update, "order_book:1", FrameKind::Update));
    let corrupted = serde_json::from_str::<Value>(update.to_text().unwrap()).unwrap();
    assert_eq!(corrupted["order_book"]["begin_nonce"], json!(-1));
}

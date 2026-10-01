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
//! `cargo test -p nautilus-hyperliquid --features examples --test hyperliquid-book-stress -- --timeout 10 --rounds 12`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates six fault phases over `--rounds` rounds.
//! - `initial`: silences first snapshots in each of `--rounds` fresh sessions.
//! - `boundaries`: probes retry exhaustion into the retry ceiling, the reconnect wake at the
//!   ceiling, unsubscribe during recovery, and shutdown during a reconnect.
//!
//! No orders are submitted. Every `l2Book` frame is a full snapshot with no sequence, so the
//! shared `BookStreamChecker` checks each emitted batch as a snapshot, and each batch is verified
//! against the book in the raw frame the proxy relayed at the same venue time. A session fails
//! unless every snapshot episode was verified.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use nautilus_common::clients::DataClient;
use nautilus_hyperliquid::{
    common::{consts::HYPERLIQUID_CLIENT_ID, enums::HyperliquidEnvironment},
    config::HyperliquidDataClientConfig,
    data::HyperliquidDataClient,
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

const COINS: [&str; 6] = ["BTC", "ETH", "SOL", "HYPE", "XRP", "DOGE"];

const STREAM: &str = "ws";
const ENDPOINT: &str = "hyperliquid-data-streams";
const DEPTH: usize = 20;
// Replacement attempts in the shared retry budget, including the first
const BUDGET: usize = 8;
// The venue pushes `l2Book` about every five seconds, so a stale stream needs a wider margin
const STALE_SECS: u64 = 20;

type HyperliquidSession = Session<Hyperliquid>;

fn main() {
    stress::run::<Hyperliquid, _, _>(|args| async move {
        let ids = COINS.map(instrument_id);

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
    let mut session = HyperliquidSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;
    recover_without_reconnect(&mut session, ids).await;

    for round in 0..args.rounds() {
        let phase = round % 6;
        let phase_started = Instant::now();

        match phase {
            0 => {
                // Every book loses its stream at once; the stale monitor replaces each one
                let connections = session.proxy().connections(STREAM);

                let before = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.silence = true;
                        (*id, fault.unsubscribes)
                    })
                    .collect::<Vec<_>>();

                session.expect_all();
                session
                    .healthy_within(ids, Duration::from_secs(STALE_SECS + 60))
                    .await;

                for (id, unsubscribes) in before {
                    assert_eq!(session.fault(&id).unsubscribes, unsubscribes + 1);
                }

                assert_eq!(session.proxy().connections(STREAM), connections);
            }
            1 => {
                // Every frame is a snapshot, so the drop must follow the corruption it recovers
                let corrupted = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.corrupt = 1;
                        (*id, fault.corrupted)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(30),
                        "simultaneous recoveries start",
                        |s| {
                            corrupted
                                .iter()
                                .all(|(id, corrupted)| s.fault(id).corrupted > *corrupted)
                        },
                    )
                    .await;

                let dropped = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.drop_snapshots = 1;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(30),
                        "simultaneous recoveries lose snapshots",
                        |s| {
                            dropped
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
                // A rejected replacement delivers no snapshot, so the deadline retries it; with
                // deadlines disabled the attempt waits out the whole budget instead.
                let id = ids[round / 6 % ids.len()];

                let (corrupted, rejected) = {
                    let mut fault = session.fault(&id);
                    fault.corrupt = 1;
                    fault.reject = usize::from(timeout > 0);
                    (fault.corrupted, fault.rejected)
                };

                session.expect_snapshot(id);
                session.healthy(&[id]).await;
                let fault = session.fault(&id).clone();
                assert_eq!(fault.corrupted, corrupted + 1);
                assert_eq!(fault.rejected, rejected + usize::from(timeout > 0));
            }
            4 => {
                // Arming the cuts before the old connection closes can spend one on its final
                // frames, which leaves the expected connection count one short.
                let connections = session.proxy().connections(STREAM);
                reconnect(&mut session);
                session
                    .until(
                        Duration::from_secs(30),
                        "reconnect opens a connection",
                        |s| s.proxy().connections(STREAM) > connections,
                    )
                    .await;

                let cuts = session.proxy().cuts();
                session.proxy().cut(Some(STREAM), FrameKind::Snapshot, 2);
                session.venue_mut().expected_epoch = connections + 3;
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
                        fault.reject = usize::MAX;
                        (*id, fault.rejected)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(30),
                        "shutdown with recovery snapshots missing",
                        |s| {
                            before
                                .iter()
                                .all(|(id, rejected)| s.fault(id).rejected > *rejected)
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
        let mut session = HyperliquidSession::connect(args).await;

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

        // Without deadlines only the stale monitor can recover a book that never streamed
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
        }

        session
            .healthy_within(ids, Duration::from_secs(STALE_SECS + 60))
            .await;

        for id in ids {
            let fault = session.fault(id).clone();
            assert_eq!(fault.corrupted, 0);
            assert_eq!(fault.dropped, 1);
            assert_eq!(fault.unsubscribes, 1);
            assert!(session.book(id).snapshots >= 1);
        }

        assert_eq!(session.proxy().connections(STREAM), 1);
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
    assert!(
        args.timeout_secs() > 0,
        "boundaries exhausts the retry budget through snapshot deadlines"
    );

    let mut session = HyperliquidSession::connect(args).await;
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
            Duration::from_secs(60),
            "replacements rejected before unsubscribe",
            |s| s.fault(&id).rejected >= rejected + 2,
        )
        .await;
    session.unsubscribe(id);
    session.observe(Duration::from_secs(2)).await;
    session.close(id);
    let rejected = session.fault(&id).rejected;
    session.observe(Duration::from_secs(30)).await;
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

// Corrupts one frame per book, then checks that each book recovered through its own
// replacement without a reconnect.
async fn recover_without_reconnect(session: &mut HyperliquidSession, ids: &[InstrumentId]) {
    let connections = session.proxy().connections(STREAM);

    let before = ids
        .iter()
        .map(|id| {
            let snapshots = session.book(id).snapshots;
            let mut fault = session.fault(id);
            fault.corrupt = 1;
            (*id, fault.corrupted, fault.unsubscribes, snapshots)
        })
        .collect::<Vec<_>>();

    session.expect_all();
    session.healthy(ids).await;
    session.observe(Duration::from_secs(5)).await;

    for (id, corrupted, unsubscribes, snapshots) in before {
        let fault = session.fault(&id).clone();
        assert_eq!(fault.corrupted, corrupted + 1);
        assert_eq!(fault.unsubscribes, unsubscribes + 1);
        assert!(session.book(&id).snapshots > snapshots);
    }

    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check(
        "autonomous",
        format!("books={} corrupted={} reconnects=0", ids.len(), ids.len()),
    );
}

// Starts a recovery and rejects every attempt in its retry budget, returning once the last
// rejection is sent.
async fn exhaust(session: &mut HyperliquidSession, id: InstrumentId) {
    let rejected = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.reject = BUDGET;
        fault.rejected
    };

    session
        .until(Duration::from_secs(240), "retry budget rejected", |s| {
            s.fault(&id).rejected == rejected + BUDGET
        })
        .await;
}

// Reconnects the stream, requiring every book to resync from its successor, or from the
// connection after any pending cuts.
fn reconnect(session: &mut HyperliquidSession) {
    let expected = session.proxy().connections(STREAM) + 1 + session.proxy().cuts_pending();
    session.venue_mut().expected_epoch = expected;

    assert_eq!(
        session.reconnect(ENDPOINT),
        ReconnectRequestOutcome::Accepted
    );
}

fn subscribe_all(session: &mut HyperliquidSession, ids: &[InstrumentId]) {
    for id in ids {
        assert!(session.instruments().contains(id), "Hyperliquid lists {id}");
        session.subscribe(*id);
    }
}

fn instrument_id(coin: &str) -> InstrumentId {
    InstrumentId::from(format!("{coin}-USD-PERP.HYPERLIQUID").as_str())
}

struct Hyperliquid {
    wire: HyperliquidWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Hyperliquid {
    const NAME: &'static str = "hyperliquid";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "boundaries"];
    const ROUNDS: usize = 12;
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(90);

    fn self_check() {
        check_wire_oracle();
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: HyperliquidWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *HYPERLIQUID_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "/ws",
            upstream: "wss://api.hyperliquid.xyz/ws".to_string(),
            endpoint: ENDPOINT,
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let config = HyperliquidDataClientConfig::builder()
            .environment(HyperliquidEnvironment::Mainnet)
            .base_url_ws(format!("ws://{proxy}/ws"))
            .update_instruments_interval_mins(0)
            .book_snapshot_timeout_secs(args.timeout_secs())
            .stale_stream_receive_timeout_secs(STALE_SECS)
            .stream_health_check_interval_secs(1)
            .stale_stream_warning_cooldown_secs(STALE_SECS)
            .stale_stream_recovery_enabled(true)
            .stale_stream_recovery_cooldown_secs(1)
            .build();

        Ok(Box::new(HyperliquidDataClient::new(
            *HYPERLIQUID_CLIENT_ID,
            config,
        )?))
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        let coin = instrument_id
            .symbol
            .as_str()
            .strip_suffix("-USD-PERP")
            .expect("perpetual symbol");
        format!("l2Book:{coin}")
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        // The adapter converts venue milliseconds through `f64`, which can shift nanoseconds
        let millis = (deltas.ts_event.as_u64() + 500_000) / 1_000_000;
        let view = self
            .wire
            .views
            .find(&self.key(&id), 0, millis * 1_000_000)
            .expect("wire oracle at emitted venue time");

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            panic!(
                "wire oracle mismatch {id} ts={}: {violation}",
                deltas.ts_event
            );
        }

        self.epochs.insert(id, view.epoch);
    }

    // A healthy book resynced on the expected connection and streams snapshots again
    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches >= start.batches + 2
    }
}

#[derive(Clone, Default)]
struct HyperliquidWire {
    views: WireViews,
}

impl WireCodec for HyperliquidWire {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(HyperliquidConnection {
            views: self.views.clone(),
            route: route.name,
            epoch: number,
        })
    }
}

struct HyperliquidConnection {
    views: WireViews,
    route: &'static str,
    epoch: usize,
}

impl WireConnection for HyperliquidConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        match frame["channel"].as_str() {
            Some("l2Book") => {
                let data = &frame["data"];
                let key = format!("l2Book:{}", data["coin"].as_str().unwrap());
                let mut book = WireBook::default();
                book.apply(&levels(&data["levels"][0]), &levels(&data["levels"][1]));
                self.views.record(
                    &key,
                    WireView {
                        epoch: self.epoch,
                        sequence: 0,
                        timestamp: data["time"].as_u64().unwrap() * 1_000_000,
                        book: book.top(DEPTH),
                    },
                );

                Upstream::Book {
                    key,
                    kind: FrameKind::Snapshot,
                }
            }
            Some("subscriptionResponse") if frame["data"]["method"] == "unsubscribe" => {
                book_key(&frame["data"]["subscription"])
                    .map_or(Upstream::Other, Upstream::Unsubscribed)
            }
            Some("error") => {
                eprintln!("Venue error on route {}: {frame}", self.route);
                Upstream::Other
            }
            _ => Upstream::Other,
        }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        book_command(message, "unsubscribe").into_iter().collect()
    }

    // Moves the frame time past the nanosecond range, so the adapter cannot parse the book
    fn corrupt(&mut self, message: &mut Message, _key: &str, _kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        let mut frame = serde_json::from_str::<Value>(text).unwrap();
        frame["data"]["time"] = json!(u64::MAX);
        *message = Message::Text(frame.to_string().into());
        true
    }

    // Answers a book subscribe as the venue answers a duplicate one, with no snapshot
    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let key = book_command(message, "subscribe")?;
        let reply = json!({"channel": "error", "data": format!("Already subscribed: {key}")});
        Some((key, Message::Text(reply.to_string().into())))
    }
}

// Returns the fault key of an adapter book command with `method`, such as `l2Book:BTC`
fn book_command(message: &Message, method: &str) -> Option<String> {
    let Message::Text(text) = message else {
        return None;
    };

    let frame = serde_json::from_str::<Value>(text).ok()?;

    if frame["method"] != method {
        return None;
    }

    book_key(&frame["subscription"])
}

fn book_key(subscription: &Value) -> Option<String> {
    if subscription["type"] != "l2Book" {
        return None;
    }

    Some(format!("l2Book:{}", subscription["coin"].as_str()?))
}

fn levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                Decimal::from_str(row["px"].as_str().unwrap()).unwrap(),
                Decimal::from_str(row["sz"].as_str().unwrap()).unwrap(),
            )
        })
        .collect()
}

// Proves the wire oracle before any venue traffic, since a wrong oracle would pass a wrong book
fn check_wire_oracle() {
    let wire = HyperliquidWire::default();

    let route = Route {
        name: STREAM,
        path: "/ws",
        upstream: String::new(),
        endpoint: ENDPOINT,
        headers: &[],
    };

    let mut connection = wire.open(&route, 3);
    let text = |value: Value| Message::Text(value.to_string().into());

    let frame = |time: u64, bids: Value, asks: Value| {
        text(json!({
            "channel": "l2Book",
            "data": {"coin": "BTC", "time": time, "levels": [bids, asks]},
        }))
    };

    let snapshot = frame(
        5,
        json!([{"px": "10.0", "sz": "2", "n": 1}, {"px": "9", "sz": "3", "n": 2}]),
        json!([{"px": "11", "sz": "4", "n": 1}]),
    );
    let replacement = frame(6, json!([{"px": "9", "sz": "3", "n": 2}]), json!([]));
    let empty = frame(7, json!([]), json!([]));
    let subscription = json!({
        "type": "l2Book", "coin": "BTC", "nSigFigs": null, "mantissa": null, "fast": false,
    });
    let unsubscribed = text(json!({
        "channel": "subscriptionResponse",
        "data": {"method": "unsubscribe", "subscription": subscription},
    }));
    let subscribed = text(json!({
        "channel": "subscriptionResponse",
        "data": {"method": "subscribe", "subscription": subscription},
    }));
    let bbo = text(json!({"channel": "bbo", "data": {"coin": "BTC"}}));
    let subscribe =
        text(json!({"method": "subscribe", "subscription": {"type": "l2Book", "coin": "BTC"}}));
    let unsubscribe =
        text(json!({"method": "unsubscribe", "subscription": {"type": "l2Book", "coin": "BTC"}}));
    let unsubscribe_bbo =
        text(json!({"method": "unsubscribe", "subscription": {"type": "bbo", "coin": "BTC"}}));

    let book = Upstream::Book {
        key: "l2Book:BTC".to_string(),
        kind: FrameKind::Snapshot,
    };

    assert_eq!(connection.upstream(&snapshot), book);
    assert_eq!(connection.upstream(&replacement), book);
    assert_eq!(
        connection.upstream(&unsubscribed),
        Upstream::Unsubscribed("l2Book:BTC".to_string())
    );
    assert_eq!(connection.upstream(&subscribed), Upstream::Other);
    assert_eq!(connection.upstream(&bbo), Upstream::Other);
    assert_eq!(connection.upstream(&subscribe), Upstream::Other);
    assert_eq!(connection.client(&unsubscribe), ["l2Book:BTC"]);
    assert!(connection.client(&subscribe).is_empty());
    assert!(connection.client(&unsubscribe_bbo).is_empty());
    assert_eq!(connection.reject(&unsubscribe), None);
    assert_eq!(
        connection.reject(&subscribe),
        Some((
            "l2Book:BTC".to_string(),
            text(json!({"channel": "error", "data": "Already subscribed: l2Book:BTC"})),
        ))
    );

    // Each frame replaces the whole book, so levels absent from the next frame disappear
    assert_eq!(
        [
            wire.views.find("l2Book:BTC", 0, 5_000_000),
            wire.views.find("l2Book:BTC", 0, 6_000_000),
        ],
        [
            Some(WireView {
                epoch: 3,
                sequence: 0,
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
                sequence: 0,
                timestamp: 6_000_000,
                book: WireBook {
                    bids: [(Decimal::from(9), Decimal::from(3))].into(),
                    asks: [].into(),
                },
            }),
        ]
    );

    assert_eq!(connection.upstream(&empty), book);
    assert_eq!(
        wire.views.find("l2Book:BTC", 0, 7_000_000),
        Some(WireView {
            epoch: 3,
            sequence: 0,
            timestamp: 7_000_000,
            book: WireBook::default(),
        })
    );

    let mut corrupted = snapshot;
    assert!(connection.corrupt(&mut corrupted, "l2Book:BTC", FrameKind::Snapshot));
    let corrupted = serde_json::from_str::<Value>(corrupted.to_text().unwrap()).unwrap();
    assert_eq!(corrupted["data"]["time"], json!(u64::MAX));
    assert_eq!(corrupted["data"]["coin"], json!("BTC"));
}

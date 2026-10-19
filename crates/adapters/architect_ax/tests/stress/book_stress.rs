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

//! Sandbox market-data fault injection with an independent decimal order book oracle.
//!
//! The AX market data stream rejects unauthenticated connections, so the run needs sandbox API
//! credentials in `AX_API_KEY` and `AX_API_SECRET`:
//! `cargo test -p nautilus-architect-ax --features examples --test ax-book-stress -- --timeout 10 --rounds 14`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates seven fault phases over `--rounds` rounds.
//! - `initial`: silences first snapshots in each of `--rounds` fresh sessions.
//! - `turnover`: unsubscribes and resubscribes a recovering book at three points of its recovery.
//! - `boundaries`: probes retry exhaustion into the retry ceiling, the reconnect wake at the
//!   ceiling, unsubscribe during recovery, and shutdown during a reconnect.
//!
//! No orders are submitted. Every L2 frame is a full snapshot with no sequence, so the shared
//! `BookStreamChecker` checks each emitted batch as a snapshot, and each batch is verified against
//! the book in the raw frame the proxy relayed at the same venue time. A session fails unless
//! every snapshot episode was verified. The sandbox market maker quotes only some instruments, and
//! a book that stops streaming fails the run, so `--symbols` selects books that stream.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    rc::Rc,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use nautilus_architect_ax::{
    common::{
        consts::{AX_CLIENT_ID, AX_WS_SANDBOX_PUBLIC_URL},
        enums::AxEnvironment,
    },
    config::AxDataClientConfig,
    factories::AxDataClientFactory,
};
use nautilus_common::{
    cache::{Cache, CacheView},
    clients::DataClient,
    clock::VirtualClock,
    factories::DataClientFactory,
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
    BookProgress, Coverage, Flag, FrameKind, Route, Session, StressArgs, StressVenue, Upstream,
    WireBook, WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const STREAM: &str = "md";
const ENDPOINT: &str = "architect-ax-data-streams";
// AX sends every aggregated level, so the comparison depth only needs to exceed the deepest book
const DEPTH: usize = 100;
// Replacement attempts in the shared retry budget, including the first
const BUDGET: usize = 8;
// A held snapshot released this long after a reconnect beats a deadline of twice this length
const LATE_SECS: u64 = 4;

type AxSession = Session<Ax>;

fn main() {
    stress::run::<Ax, _, _>(|args| async move {
        let ids = instrument_ids(&args);

        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "initial" => initial(&args, &ids).await,
            "turnover" => turnover(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let timeout = args.timeout_secs();
    let mut total = 0;
    let mut session = AxSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    for round in 0..args.rounds() {
        let phase = round % 7;
        let phase_started = Instant::now();

        match phase {
            0 => recover_without_reconnect(&mut session, ids).await,
            1 => recover_missing_snapshots(&mut session, ids, timeout).await,
            2 => accept_late_snapshots(&mut session, ids, timeout).await,
            3 => {
                // A rejected replacement delivers no snapshot, so its deadline or an invalid frame
                // still in flight fails the attempt and recovery retries; with deadlines disabled
                // the attempt can wait out the whole budget instead.
                let id = ids[round / 7 % ids.len()];

                let (corrupted, rejected, unsubscribes) = {
                    let mut fault = session.fault(&id);
                    fault.corrupt = usize::MAX;
                    fault.reject = usize::from(timeout > 0);
                    (fault.corrupted, fault.rejected, fault.unsubscribes)
                };

                session
                    .until(
                        Duration::from_secs(60),
                        "replacement unsubscribes the stream",
                        |s| {
                            let fault = s.fault(&id);
                            fault.unsubscribes > unsubscribes
                                && fault.rejected == rejected + usize::from(timeout > 0)
                        },
                    )
                    .await;

                session.fault(&id).corrupt = 0;
                session.expect_snapshot(id);
                session.healthy(&[id]).await;
                let fault = session.fault(&id).clone();
                assert!(fault.corrupted > corrupted);
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
                        fault.corrupt = usize::MAX;
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
            6 => {
                // Traffic freeze without closing either socket
                let freeze = Duration::from_secs(40);
                session.proxy().freeze(freeze);
                session.observe(freeze + Duration::from_secs(2)).await;
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
        let mut session = AxSession::connect(args).await;

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

        // Without deadlines a silent book stays dark until a reconnect replays its subscription
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

        session
            .healthy_within(ids, Duration::from_secs(timeout + 60))
            .await;

        for id in ids {
            let fault = session.fault(id).clone();
            assert_eq!(fault.corrupted, 0);
            assert_eq!(fault.dropped, 1);
            assert_eq!(fault.unsubscribes, usize::from(timeout > 0));
            assert!(session.book(id).snapshots >= 1);
        }

        assert_eq!(
            session.proxy().connections(STREAM),
            1 + usize::from(timeout == 0)
        );
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

// Turns a recovering book's subscription over at three points of its recovery: just after it
// starts, after its replacement reaches the venue, and after a rejected replacement's deadline.
// The resubscribe then races the unsubscribe and the cancelled recovery's writes.
async fn turnover(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let timeout = args.timeout_secs();
    let mut session = AxSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    for round in 0..args.rounds() {
        let id = ids[round % ids.len()];
        let boundary = round % 3;

        // With deadlines disabled a rejected replacement waits out the whole budget
        let reject = boundary == 2 && timeout > 0;

        let (corrupted, rejected, unsubscribes) = {
            let mut fault = session.fault(&id);
            fault.corrupt = usize::MAX;
            fault.reject = usize::from(reject);
            (fault.corrupted, fault.rejected, fault.unsubscribes)
        };

        session
            .until(Duration::from_secs(30), "turnover recovery boundary", |s| {
                let fault = s.fault(&id);

                if boundary == 0 {
                    fault.corrupted > corrupted
                } else {
                    fault.unsubscribes > unsubscribes
                        && fault.rejected == rejected + usize::from(reject)
                }
            })
            .await;

        if reject {
            session
                .observe(Duration::from_secs(timeout) + Duration::from_millis(200))
                .await;
        }

        {
            let mut fault = session.fault(&id);
            fault.corrupt = 0;
            fault.reject = 0;
        }

        session.unsubscribe(id);
        session.subscribe(id);
        session.healthy(&[id]).await;

        // Every frame is a snapshot, so the book can heal on its old stream before the venue
        // applies the turnover; once the venue settles, the new subscription keeps streaming
        session.observe(Duration::from_secs(5)).await;
        let unsubscribes = session.fault(&id).unsubscribes;
        session.healthy(&[id]).await;
        assert_eq!(
            session.fault(&id).unsubscribes,
            unsubscribes,
            "old recovery does not replace the new subscription"
        );
        session.round(round, &format!("boundary={boundary} instrument={id}"));
    }

    session.healthy(ids).await;
    let batches = session.batches();
    session.stop().await;
    format!("batches_total={batches}")
}

async fn boundaries(args: &StressArgs, ids: &[InstrumentId]) -> String {
    assert!(
        args.timeout_secs() > 0,
        "boundaries exhausts the retry budget through snapshot deadlines"
    );
    assert!(ids.len() >= 3, "boundaries probes three books");

    let mut session = AxSession::connect(args).await;
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

    // A reconnect ends the ceiling wait, so the recovery retries at once on the new connection.
    // The last rejected attempt holds its deadline first, so the wait outlasts it.
    let id = ids[1];
    exhaust(&mut session, id).await;
    session
        .observe(Duration::from_secs(args.timeout_secs() + 5))
        .await;
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
        fault.corrupt = usize::MAX;
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

    {
        let mut fault = session.fault(&id);
        fault.corrupt = 0;
        fault.reject = 0;
    }

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

// Invalidates every book's frames until its replacement unsubscribes the venue stream, then
// checks that each book recovered through its own replacement without a reconnect. The venue
// stops a stream before acknowledging its unsubscribe, so no earlier valid frame can heal a book.
async fn recover_without_reconnect(session: &mut AxSession, ids: &[InstrumentId]) {
    let connections = session.proxy().connections(STREAM);

    let before = ids
        .iter()
        .map(|id| {
            let snapshots = session.book(id).snapshots;
            let mut fault = session.fault(id);
            fault.corrupt = usize::MAX;
            (*id, fault.corrupted, fault.unsubscribes, snapshots)
        })
        .collect::<Vec<_>>();

    session
        .until(
            Duration::from_secs(30),
            "invalid frames start replacements",
            |s| {
                before
                    .iter()
                    .all(|(id, _, unsubscribes, _)| s.fault(id).unsubscribes > *unsubscribes)
            },
        )
        .await;

    for id in ids {
        session.fault(id).corrupt = 0;
    }

    session.expect_all();
    session.healthy(ids).await;

    for (id, corrupted, unsubscribes, snapshots) in before {
        let fault = session.fault(&id).clone();
        assert!(fault.corrupted > corrupted);
        assert!(fault.unsubscribes > unsubscribes);
        assert!(session.book(&id).snapshots > snapshots);
    }

    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check("autonomous", format!("books={} reconnects=0", ids.len()));
}

// Holds replayed snapshots past their deadlines, so each book replaces its subscription; with
// deadlines disabled nothing recovers until the release.
async fn recover_missing_snapshots(session: &mut AxSession, ids: &[InstrumentId], timeout: u64) {
    let before = hold_all(session, ids);
    reconnect(session);

    if timeout > 0 {
        session
            .until(
                Duration::from_secs(timeout + 60),
                "missing snapshots start recoveries",
                |s| {
                    before
                        .iter()
                        .all(|(id, unsubscribes)| s.fault(id).unsubscribes > *unsubscribes)
                },
            )
            .await;
    } else {
        session.observe(Duration::from_secs(10)).await;

        for (id, unsubscribes) in &before {
            assert_eq!(session.fault(id).unsubscribes, *unsubscribes);
        }
    }

    release_all(session, ids);
    session.healthy(ids).await;
}

// Releases replayed snapshots inside their deadlines, which needs no replacement
async fn accept_late_snapshots(session: &mut AxSession, ids: &[InstrumentId], timeout: u64) {
    let before = hold_all(session, ids);
    session.observe(Duration::from_secs(2)).await;
    reconnect(session);
    session.observe(Duration::from_secs(LATE_SECS)).await;
    release_all(session, ids);
    session.healthy(ids).await;

    if timeout == 0 || timeout >= 2 * LATE_SECS {
        for (id, unsubscribes) in before {
            assert_eq!(session.fault(&id).unsubscribes, unsubscribes);
        }
    }
}

// Starts a recovery and rejects every attempt in its retry budget, returning once the last
// rejection is sent. Frames stay invalid until the venue acknowledges the first replacement's
// unsubscribe, after which the stream is down and only a snapshot deadline ends each attempt.
async fn exhaust(session: &mut AxSession, id: InstrumentId) {
    let (rejected, unsubscribes) = {
        let mut fault = session.fault(&id);
        fault.corrupt = usize::MAX;
        fault.reject = BUDGET;
        (fault.rejected, fault.unsubscribes)
    };

    session
        .until(
            Duration::from_secs(30),
            "first replacement unsubscribes the stream",
            |s| {
                let fault = s.fault(&id);
                fault.rejected > rejected && fault.unsubscribes > unsubscribes
            },
        )
        .await;

    session.fault(&id).corrupt = 0;
    session
        .until(Duration::from_secs(240), "retry budget rejected", |s| {
            s.fault(&id).rejected == rejected + BUDGET
        })
        .await;
}

// Holds every book's frames, returning each book's unsubscribes so far
fn hold_all(session: &AxSession, ids: &[InstrumentId]) -> Vec<(InstrumentId, usize)> {
    ids.iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.hold = true;
            (*id, fault.unsubscribes)
        })
        .collect()
}

fn release_all(session: &AxSession, ids: &[InstrumentId]) {
    for id in ids {
        session.fault(id).hold = false;
    }

    session.proxy().release();
}

// Reconnects the stream, requiring every book to resync from its successor, or from the
// connection after any pending cuts.
fn reconnect(session: &mut AxSession) {
    let expected = session.proxy().connections(STREAM) + 1 + session.proxy().cuts_pending();
    session.venue_mut().expected_epoch = expected;

    assert_eq!(
        session.reconnect(ENDPOINT),
        ReconnectRequestOutcome::Accepted
    );
}

fn subscribe_all(session: &mut AxSession, ids: &[InstrumentId]) {
    for id in ids {
        assert!(session.instruments().contains(id), "AX lists {id}");
        session.subscribe(*id);
    }
}

fn instrument_ids(args: &StressArgs) -> Vec<InstrumentId> {
    args.flag("symbols")
        .split(',')
        .map(|symbol| InstrumentId::from(format!("{}.AX", symbol.trim()).as_str()))
        .collect()
}

struct Ax {
    wire: AxWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Ax {
    const NAME: &'static str = "ax";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "turnover", "boundaries"];
    const ROUNDS: usize = 14;
    const FLAGS: &'static [Flag] = &[Flag {
        name: "symbols",
        default: "GBPUSD-PERP,EURUSD-PERP,JPYUSD-PERP,XAG-PERP,BRLUSD-PERP",
        help: "Comma-separated symbols whose sandbox books stream",
    }];
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(90);

    fn self_check() {
        check_wire_oracle();
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: AxWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *AX_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "/md/ws",
            upstream: AX_WS_SANDBOX_PUBLIC_URL.to_string(),
            endpoint: ENDPOINT,
            headers: &["authorization"],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let config = AxDataClientConfig::builder()
            .environment(AxEnvironment::Sandbox)
            .base_url_ws_public(format!("ws://{proxy}/md/ws"))
            .update_instruments_interval_mins(0)
            .book_snapshot_timeout_secs(args.timeout_secs())
            .build();

        anyhow::ensure!(
            config.has_api_credentials(),
            "AX_API_KEY and AX_API_SECRET must hold sandbox API credentials"
        );

        let cache = CacheView::from(Rc::new(RefCell::new(Cache::default())));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));

        AxDataClientFactory.create(AX_CLIENT_ID.as_str(), &config, cache, clock)
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        instrument_id.symbol.to_string()
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let view = self
            .wire
            .views
            .find(&self.key(&id), 0, deltas.ts_event.as_u64())
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
struct AxWire {
    views: WireViews,
}

impl WireCodec for AxWire {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(AxConnection {
            views: self.views.clone(),
            route: route.name,
            epoch: number,
        })
    }
}

struct AxConnection {
    views: WireViews,
    route: &'static str,
    epoch: usize,
}

impl WireConnection for AxConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        if frame["t"] == "2" {
            let key = frame["s"].as_str().unwrap().to_string();
            let mut book = WireBook::default();
            book.apply(&levels(&frame["b"]), &levels(&frame["a"]));
            let seconds = frame["ts"].as_u64().unwrap();
            let nanos = frame["tn"].as_u64().unwrap();
            self.views.record(
                &key,
                WireView {
                    epoch: self.epoch,
                    sequence: 0,
                    timestamp: seconds * 1_000_000_000 + nanos,
                    book: book.top(DEPTH),
                },
            );

            return Upstream::Book {
                key,
                kind: FrameKind::Snapshot,
            };
        }

        if let Some(symbol) = frame["result"]["unsubscribed"].as_str() {
            return Upstream::Unsubscribed(symbol.to_string());
        }

        if !frame["error"].is_null() {
            eprintln!("Venue error on route {}: {frame}", self.route);
        }

        Upstream::Other
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        book_command(message, "unsubscribe")
            .map(|(symbol, _)| symbol)
            .into_iter()
            .collect()
    }

    // Marks the frame incremental, which the adapter cannot convert to a snapshot
    fn corrupt(&mut self, message: &mut Message, _key: &str, _kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        let mut frame = serde_json::from_str::<Value>(text).unwrap();
        frame["st"] = json!(false);
        *message = Message::Text(frame.to_string().into());
        true
    }

    // Answers a subscribe as the venue answers a duplicate one, with no snapshot
    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let (symbol, rid) = book_command(message, "subscribe")?;
        let reply = json!({
            "rid": rid,
            "error": {"message": format!("Symbol {symbol} is already subscribed"), "code": 500},
        });
        Some((symbol, Message::Text(reply.to_string().into())))
    }
}

// Returns the symbol and request ID of an adapter market data command of `request_type`
fn book_command(message: &Message, request_type: &str) -> Option<(String, Value)> {
    let Message::Text(text) = message else {
        return None;
    };

    let frame = serde_json::from_str::<Value>(text).ok()?;

    if frame["type"] != request_type {
        return None;
    }

    Some((frame["symbol"].as_str()?.to_string(), frame["rid"].clone()))
}

fn levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                Decimal::from_str(row["p"].as_str().unwrap()).unwrap(),
                Decimal::from(row["q"].as_u64().unwrap()),
            )
        })
        .collect()
}

// Proves the wire oracle before any venue traffic, since a wrong oracle would pass a wrong book
fn check_wire_oracle() {
    let wire = AxWire::default();

    let route = Route {
        name: STREAM,
        path: "/md/ws",
        upstream: String::new(),
        endpoint: ENDPOINT,
        headers: &[],
    };

    let mut connection = wire.open(&route, 3);
    let text = |value: Value| Message::Text(value.to_string().into());

    let frame = |tn: u64, bids: Value, asks: Value| {
        text(
            json!({"t": "2", "ts": 5, "tn": tn, "s": "GBPUSD-PERP", "st": true, "b": bids, "a": asks}),
        )
    };

    let snapshot = frame(
        7,
        json!([{"p": "1.3410", "q": 2}, {"p": "1.3409", "q": 3}]),
        json!([{"p": "1.3412", "q": 4}]),
    );
    let replacement = frame(8, json!([{"p": "1.3409", "q": 3}]), json!([]));
    let empty = frame(9, json!([]), json!([]));
    let unsubscribed = text(json!({"rid": 3, "result": {"unsubscribed": "GBPUSD-PERP"}}));
    let subscribed = text(json!({"rid": 4, "result": {"subscribed": "GBPUSD-PERP"}}));
    let not_subscribed = text(json!({
        "rid": 5,
        "error": {"message": "Symbol GBPUSD-PERP is not subscribed", "code": 400},
    }));
    let heartbeat = text(json!({"t": "h", "ts": 5, "tn": 0}));
    let l1 = text(json!({"t": "1", "ts": 5, "tn": 0, "s": "GBPUSD-PERP", "b": [], "a": []}));
    let subscribe = text(json!({
        "rid": 6, "type": "subscribe", "symbol": "GBPUSD-PERP", "level": "LEVEL_2",
        "trades": false, "ticker": false,
    }));
    let unsubscribe = text(json!({"rid": 7, "type": "unsubscribe", "symbol": "GBPUSD-PERP"}));
    let subscribe_candles = text(json!({
        "rid": 8, "type": "subscribe_candles", "symbol": "GBPUSD-PERP", "width": "1m",
    }));

    let book = Upstream::Book {
        key: "GBPUSD-PERP".to_string(),
        kind: FrameKind::Snapshot,
    };

    assert_eq!(connection.upstream(&snapshot), book);
    assert_eq!(connection.upstream(&replacement), book);
    assert_eq!(
        connection.upstream(&unsubscribed),
        Upstream::Unsubscribed("GBPUSD-PERP".to_string())
    );
    assert_eq!(connection.upstream(&subscribed), Upstream::Other);
    assert_eq!(connection.upstream(&not_subscribed), Upstream::Other);
    assert_eq!(connection.upstream(&heartbeat), Upstream::Other);
    assert_eq!(connection.upstream(&l1), Upstream::Other);
    assert_eq!(connection.client(&unsubscribe), ["GBPUSD-PERP"]);
    assert!(connection.client(&subscribe).is_empty());
    assert!(connection.client(&subscribe_candles).is_empty());
    assert_eq!(connection.reject(&unsubscribe), None);
    assert_eq!(connection.reject(&subscribe_candles), None);
    assert_eq!(
        connection.reject(&subscribe),
        Some((
            "GBPUSD-PERP".to_string(),
            text(json!({
                "rid": 6,
                "error": {"message": "Symbol GBPUSD-PERP is already subscribed", "code": 500},
            })),
        ))
    );

    // Each frame replaces the whole book, so levels absent from the next frame disappear
    assert_eq!(
        [
            wire.views.find("GBPUSD-PERP", 0, 5_000_000_007),
            wire.views.find("GBPUSD-PERP", 0, 5_000_000_008),
        ],
        [
            Some(WireView {
                epoch: 3,
                sequence: 0,
                timestamp: 5_000_000_007,
                book: WireBook {
                    bids: [
                        (Decimal::from_str("1.3409").unwrap(), Decimal::from(3)),
                        (Decimal::from_str("1.3410").unwrap(), Decimal::from(2)),
                    ]
                    .into(),
                    asks: [(Decimal::from_str("1.3412").unwrap(), Decimal::from(4))].into(),
                },
            }),
            Some(WireView {
                epoch: 3,
                sequence: 0,
                timestamp: 5_000_000_008,
                book: WireBook {
                    bids: [(Decimal::from_str("1.3409").unwrap(), Decimal::from(3))].into(),
                    asks: [].into(),
                },
            }),
        ]
    );

    assert_eq!(connection.upstream(&empty), book);
    assert_eq!(
        wire.views.find("GBPUSD-PERP", 0, 5_000_000_009),
        Some(WireView {
            epoch: 3,
            sequence: 0,
            timestamp: 5_000_000_009,
            book: WireBook::default(),
        })
    );

    let mut corrupted = snapshot;
    assert!(connection.corrupt(&mut corrupted, "GBPUSD-PERP", FrameKind::Snapshot));
    let corrupted = serde_json::from_str::<Value>(corrupted.to_text().unwrap()).unwrap();
    assert_eq!(corrupted["st"], json!(false));
    assert_eq!(corrupted["s"], json!("GBPUSD-PERP"));
    assert_eq!(corrupted["b"][0]["p"], json!("1.3410"));
}

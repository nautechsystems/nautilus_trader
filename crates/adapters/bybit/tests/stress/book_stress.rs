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

//! Mainnet market-data fault injection with a wire reconstruction and a separate public connection.
//!
//! Run with adapter credentials unset:
//! `cargo test -p nautilus-bybit --features examples --test bybit-book-stress -- --timeout 10 --rounds 21`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates seven fault phases over `--rounds` rounds.
//! - `initial`: drops first snapshots in each of `--rounds` fresh sessions.
//! - `turnover`: unsubscribes and resubscribes during recovery at three boundaries.
//! - `boundaries`: probes replacement cuts, retry exhaustion, rejected subscriptions, and shutdown
//!   during reconnect.
//!
//! No orders are submitted. Books use the linear socket at the default depth of 50. Every emitted
//! batch passes through the shared `BookStreamChecker` and is verified against a reference book
//! rebuilt from the raw frames the proxy relays; a session fails unless every snapshot episode
//! was verified. A separate public order book connection compares those levels at the same cross
//! sequence, and a session fails unless each book has an aligned comparison.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use nautilus_bybit::{
    common::{consts::BYBIT_CLIENT_ID, enums::BybitProductType, parse::extract_raw_symbol},
    config::BybitDataClientConfig,
    data::BybitDataClient,
};
use nautilus_common::clients::DataClient;
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    identifiers::{ClientId, InstrumentId},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, FrameKind, Route, Session, StressArgs, StressVenue, Upstream, WireBook,
    WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const SYMBOLS: [&str; 6] = [
    "BTCUSDT-LINEAR",
    "ETHUSDT-LINEAR",
    "SOLUSDT-LINEAR",
    "XRPUSDT-LINEAR",
    "DOGEUSDT-LINEAR",
    "BNBUSDT-LINEAR",
];

const LINEAR: &str = "linear";
const LINEAR_ENDPOINT: &str = "bybit-linear-data-streams";
const DEPTH: usize = 50;

type BybitSession = Session<Bybit>;

fn main() {
    stress::run::<Bybit, _, _>(|args| async move {
        let ids = SYMBOLS.map(|s| InstrumentId::from(format!("{s}.BYBIT")));

        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "initial" => initial(&args, &ids).await,
            "turnover" => turnover(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId; 6]) -> String {
    let mut total = 0;
    let mut session = BybitSession::connect(args).await;
    subscribe_all(&mut session, ids).await;
    recover_without_reconnect(&mut session, ids, usize::from(args.timeout_secs() > 0)).await;

    for round in 0..args.rounds() {
        let phase = round % 7;
        let phase_started = Instant::now();

        match phase {
            0 => reconnect_during_recoveries(&mut session, ids).await,
            1 => reconnect_while_held(&mut session, ids).await,
            2 => {
                let targets = [ids[round / 7 % 3], ids[3 + round / 7 % 3]];
                unsubscribe_during_recovery(&mut session, ids, targets).await;
            }
            3 => cut_reconnects(&mut session, ids).await,
            4 => drop_replay_snapshots(&mut session, ids).await,
            5 => {
                drop_recovery_snapshots(
                    &mut session,
                    ids,
                    3,
                    "shutdown with recovery snapshots missing",
                )
                .await;
                total += session.batches();
                session = session.restart().await;
                subscribe_all(&mut session, ids).await;
            }
            6 => freeze_traffic(&mut session, ids).await,
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

async fn initial(args: &StressArgs, ids: &[InstrumentId; 6]) -> String {
    let timeout = args.timeout_secs();
    let mut total = 0;

    for round in 0..args.rounds() {
        let mut session = BybitSession::connect(args).await;

        for id in ids {
            let mut fault = session.fault(id);
            fault.drop_snapshots = 1;
            fault.silence = true;
        }

        for id in ids {
            session.subscribe(*id);
        }

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

        assert_eq!(
            session.proxy().connections(LINEAR),
            if timeout > 0 { 1 } else { 2 }
        );
        session.observe(Duration::from_secs(2)).await;
        total += session.batches();
        session.stop().await;
        stress::check("initial", format!("round={} books=6", round + 1));
    }

    format!("batches_total={total}")
}

async fn turnover(args: &StressArgs, ids: &[InstrumentId; 6]) -> String {
    let timeout = args.timeout_secs();
    let mut session = BybitSession::connect(args).await;
    silence_initial_snapshots(&mut session, ids, [ids[0], ids[3]], timeout).await;

    for round in 0..args.rounds() {
        let id = ids[round % 2];

        let drops = if timeout > 0 { 3 } else { 1 };

        let (corrupted, dropped) = {
            let mut fault = session.fault(&id);
            fault.corrupt = 1;
            fault.drop_snapshots = drops;
            (fault.corrupted, fault.dropped)
        };

        session
            .until(Duration::from_secs(30), "turnover recovery boundary", |s| {
                let fault = s.fault(&id);

                if round % 3 == 0 {
                    fault.corrupted > corrupted
                } else {
                    fault.dropped == dropped + drops
                }
            })
            .await;

        if round % 3 == 2 && timeout > 0 {
            session
                .observe(Duration::from_millis(timeout * 1_000 + 200))
                .await;
        }

        session.fault(&id).drop_snapshots = 0;
        session.unsubscribe(id);
        session.subscribe(id);
        session.healthy(&[id]).await;

        // Updates the old subscription sent before the venue processed the unsubscribe reach the
        // new book ahead of its snapshot, so the new book can finish one recovery of its own
        session.observe(Duration::from_secs(2)).await;
        let unsubscribes = session.fault(&id).unsubscribes;
        assert!(unsubscribes > 0, "venue acknowledged the unsubscribe");

        // Outlasts a snapshot deadline, so a surviving old recovery would attempt again
        session
            .observe(Duration::from_secs(5.max(timeout + 2)))
            .await;
        assert_eq!(
            session.fault(&id).unsubscribes,
            unsubscribes,
            "old recovery does not replace new subscription"
        );
        session.round(round, &format!("boundary={} instrument={id}", round % 3));
    }

    session.healthy(ids).await;
    let batches = session.batches();
    session.stop().await;
    format!("batches_total={batches}")
}

async fn boundaries(args: &StressArgs, ids: &[InstrumentId; 6]) -> String {
    let timeout = args.timeout_secs();
    let mut session = BybitSession::connect(args).await;

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
    recover_without_reconnect(&mut session, ids, if timeout > 0 { 3 } else { 0 }).await;

    let targets = [ids[0], ids[3]];

    for id in targets {
        let connections = session.proxy().connections(LINEAR);
        let cuts = session.proxy().cuts();
        {
            let mut fault = session.fault(&id);
            fault.corrupt = 1;
            fault.cut_unsubscribe = true;
        }

        session.venue_mut().expected_epoch = connections + 1;
        session.expect_all();
        session.healthy(ids).await;
        assert_eq!(session.proxy().cuts(), cuts + 1);
        assert_eq!(session.proxy().connections(LINEAR), connections + 1);
        stress::check("replacement_cut", format!("instrument={id}"));
    }

    let connections_before = session.proxy().connections(LINEAR);

    // Drops replacement snapshots rather than holding them: a depth-50 feed pushes every 20ms,
    // so frames held across the window would outrun the oracle's retained views
    let before = targets.map(|id| {
        let mut fault = session.fault(&id);
        let unsubscribes = fault.unsubscribes;
        fault.corrupt = 1;
        fault.drop_snapshots = usize::MAX;
        (id, unsubscribes, fault.dropped)
    });

    session
        .until(
            Duration::from_secs(20),
            "replacement snapshots dropped",
            |s| {
                before
                    .iter()
                    .all(|(id, _, dropped)| s.fault(id).dropped > *dropped)
            },
        )
        .await;

    // The retry budget ends within the window, including the 180-second budget with deadlines
    // disabled; recovery then continues at the one-minute ceiling.
    let window = Duration::from_secs(185);
    session.observe(window).await;

    let budget = if timeout > 0 { 8 } else { 1 };
    let ceiling_max = window.as_secs() as usize / 60;

    for (id, unsubscribes, _) in before {
        {
            let mut fault = session.fault(&id);
            let attempts = fault.unsubscribes - unsubscribes;
            assert!(
                (budget..=budget + ceiling_max).contains(&attempts),
                "{id} made {attempts} attempts; expected the budget of {budget} plus at most \
                 one ceiling attempt per minute"
            );
            fault.drop_snapshots = 0;
        }

        session.expect_snapshot(id);
    }

    // The next ceiling attempt completes each exhausted recovery without a reconnect; an attempt
    // just before the window ends loses its snapshot, and the one after follows the doubled interval
    session.healthy_within(ids, Duration::from_secs(180)).await;
    assert_eq!(session.proxy().connections(LINEAR), connections_before);
    stress::check(
        "exhaustion",
        "books=2 recovered_at_ceiling=true reconnects=0",
    );

    let id = targets[0];
    let unsubscribes = session.fault(&id).unsubscribes;
    session.unsubscribe(id);
    session
        .until(Duration::from_secs(10), "recovered book unsubscribe", |s| {
            s.fault(&id).unsubscribes == unsubscribes + 1
        })
        .await;

    session.close(id);
    session.observe(Duration::from_secs(1)).await;
    session.subscribe(id);
    session.healthy(&[id]).await;
    stress::check("resubscribe", format!("instrument={id}"));
    reconnect(&mut session);
    session.healthy(ids).await;
    stress::check("reconnect", "books=6");

    // Rejections are not routed to the recovery, so each rejected attempt ends at its deadline
    if timeout > 0 {
        let id = ids[1];

        let rejected = {
            let mut fault = session.fault(&id);
            fault.corrupt = 1;
            fault.reject = 2;
            fault.rejected
        };

        session.expect_snapshot(id);
        session.healthy(&[id]).await;
        assert_eq!(session.fault(&id).rejected, rejected + 2);
        stress::check(
            "rejected_replacement",
            format!("instrument={id} rejected=2"),
        );

        let id = ids[2];

        let rejected = {
            let mut fault = session.fault(&id);
            fault.reject = 1;
            fault.rejected
        };

        reconnect(&mut session);
        session.healthy(ids).await;
        assert_eq!(session.fault(&id).rejected, rejected + 1);
        stress::check("rejected_replay", format!("instrument={id} rejected=1"));
    }

    let cuts = session.proxy().cuts();
    session.proxy().cut(Some(LINEAR), FrameKind::Snapshot, 10);
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

// Forces a gap on every book whose replacement snapshot is dropped, then reconnects
async fn reconnect_during_recoveries(session: &mut BybitSession, ids: &[InstrumentId]) {
    drop_recovery_snapshots(session, ids, 1, "simultaneous recoveries lose snapshots").await;
    reconnect(session);
    session.healthy(ids).await;
}

async fn reconnect_while_held(session: &mut BybitSession, ids: &[InstrumentId]) {
    for id in ids {
        session.fault(id).hold = true;
    }

    session.observe(Duration::from_secs(2)).await;
    reconnect(session);
    session.observe(Duration::from_secs(4)).await;

    for id in ids {
        session.fault(id).hold = false;
    }

    session.proxy().release();
    session.healthy(ids).await;
}

// Unsubscribes two books while their replacement snapshots are held, then resubscribes them
async fn unsubscribe_during_recovery(
    session: &mut BybitSession,
    ids: &[InstrumentId],
    targets: [InstrumentId; 2],
) {
    for id in &targets {
        let mut fault = session.fault(id);
        fault.corrupt = 1;
        fault.hold_snapshot = true;
    }

    session
        .until(
            Duration::from_secs(20),
            "replacement snapshots held before unsubscribe",
            |s| targets.iter().all(|id| s.fault(id).hold),
        )
        .await;

    let before = targets
        .iter()
        .map(|id| (*id, session.fault(id).unsubscribes))
        .collect::<Vec<_>>();

    for id in &targets {
        session.unsubscribe(*id);
    }

    session
        .until(
            Duration::from_secs(10),
            "explicit unsubscribe acknowledged",
            |s| {
                before
                    .iter()
                    .all(|(id, count)| s.fault(id).unsubscribes > *count)
            },
        )
        .await;

    for id in &targets {
        session.close(*id);
        session.fault(id).hold = false;
    }

    session.proxy().release();
    reconnect(session);
    let remaining = ids
        .iter()
        .copied()
        .filter(|id| !targets.contains(id))
        .collect::<Vec<_>>();
    session.healthy(&remaining).await;

    for id in &targets {
        session.subscribe(*id);
    }

    session.healthy(&targets).await;
}

// Cuts the first two connections after a reconnect before their snapshots arrive
async fn cut_reconnects(session: &mut BybitSession, ids: &[InstrumentId]) {
    session.proxy().cut(Some(LINEAR), FrameKind::Snapshot, 2);
    let cuts = session.proxy().cuts();
    reconnect(session);
    session
        .until(
            Duration::from_secs(120),
            "two reconnects cut before snapshots",
            |s| s.proxy().cuts() == cuts + 2,
        )
        .await;
    session.healthy(ids).await;
}

// Without deadlines, each book recovers from the first update after its dropped replay snapshot
async fn drop_replay_snapshots(session: &mut BybitSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.drop_snapshots = 1;
            (*id, fault.dropped)
        })
        .collect::<Vec<_>>();

    reconnect(session);
    session
        .until(Duration::from_secs(60), "replay snapshots dropped", |s| {
            before
                .iter()
                .all(|(id, dropped)| s.fault(id).dropped > *dropped)
        })
        .await;

    session.healthy(ids).await;
}

// Freezes traffic without closing the socket, then requires every book to stream again
async fn freeze_traffic(session: &mut BybitSession, ids: &[InstrumentId]) {
    let freeze = Duration::from_secs(40);
    let before = ids
        .iter()
        .map(|id| (*id, session.book(id).batches))
        .collect::<Vec<_>>();

    session.proxy().freeze(freeze);
    session.observe(freeze + Duration::from_secs(2)).await;
    session
        .until(Duration::from_secs(120), "books stream after freeze", |s| {
            before
                .iter()
                .all(|(id, batches)| s.book(id).batches >= batches + 3)
        })
        .await;
}

// Forces a gap on every book and drops its next `drops` snapshots
async fn drop_recovery_snapshots(
    session: &mut BybitSession,
    ids: &[InstrumentId],
    drops: usize,
    label: &str,
) {
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.corrupt = 1;
            fault.drop_snapshots = drops;
            (*id, fault.dropped)
        })
        .collect::<Vec<_>>();

    session
        .until(Duration::from_secs(20), label, |s| {
            before
                .iter()
                .all(|(id, dropped)| s.fault(id).dropped > *dropped)
        })
        .await;
}

async fn subscribe_all(session: &mut BybitSession, ids: &[InstrumentId]) {
    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
}

// Holds two books' initial snapshots past the snapshot deadline, which starts recovery unless
// deadlines are disabled
async fn silence_initial_snapshots(
    session: &mut BybitSession,
    ids: &[InstrumentId],
    quiet: [InstrumentId; 2],
    timeout: u64,
) {
    for id in &quiet {
        session.fault(id).hold = true;
    }

    for id in ids {
        session.subscribe(*id);
    }

    // The snapshot deadline starts once the subscribe write completes, after this window opens
    session
        .observe(Duration::from_secs(10.max(timeout + 5)))
        .await;

    for id in &quiet {
        let mut fault = session.fault(id);
        assert!(fault.held > 0);
        assert_eq!(session.book(id).batches, 0);

        if timeout == 0 {
            assert_eq!(fault.unsubscribes, 0);
        } else {
            assert!(
                fault.unsubscribes > 0,
                "initial silence starts recovery for {id}"
            );
        }

        fault.hold = false;
    }

    stress::check("initial_silence", "books=2");
    session.proxy().release();
    session.healthy(ids).await;
}

// Forces gaps whose replacement snapshots are dropped `drops` times, then checks that each book
// recovered through its own resubscribes without a reconnect
async fn recover_without_reconnect(session: &mut BybitSession, ids: &[InstrumentId], drops: usize) {
    let connections_before = session.proxy().connections(LINEAR);

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

    for (id, ..) in &before {
        session.expect_snapshot(*id);
    }

    session.healthy(ids).await;
    session.observe(Duration::from_secs(5)).await;

    for (id, corrupted, dropped, unsubscribes, snapshots) in before {
        let fault = session.fault(&id).clone();
        assert_eq!(fault.corrupted, corrupted + 1);
        assert_eq!(fault.dropped, dropped + drops);
        assert_eq!(fault.unsubscribes, unsubscribes + 1 + drops);
        assert_eq!(session.book(&id).snapshots, snapshots + 1);
    }

    assert_eq!(session.proxy().connections(LINEAR), connections_before);
    stress::check(
        "autonomous",
        format!(
            "books={} dropped={} reconnects=0",
            ids.len(),
            ids.len() * drops
        ),
    );
}

// Reconnects the linear socket, requiring every book to resync from its successor, or from the
// connection after any pending cuts. A venue closure can leave a reconnect in flight; the request
// then joins it, and the proxy counts that connection only once it reaches the venue.
fn reconnect(session: &mut BybitSession) {
    let pending = session.proxy().cuts_pending();
    let expected = session.proxy().connections(LINEAR) + 1 + pending;
    session.venue_mut().expected_epoch = expected;
    session.reconnect(LINEAR_ENDPOINT);
}

struct Bybit {
    wire: BybitWire,
    oracle: VenueOracle,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
    wire_empty: usize,
}

impl StressVenue for Bybit {
    const NAME: &'static str = "bybit";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "turnover", "boundaries"];
    const ROUNDS: usize = 21;
    const SEQUENCED: bool = true;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(90);

    fn self_check() {
        check_wire_oracle();
        check_sequence_books();
        check_pending_alignment();
    }

    fn new(_args: &StressArgs) -> Self {
        let wire = BybitWire::default();
        let oracle = VenueOracle::start(wire.sequences.clone());

        Self {
            wire,
            oracle,
            expected_epoch: 1,
            epochs: HashMap::new(),
            wire_empty: 0,
        }
    }

    fn client_id(&self) -> ClientId {
        *BYBIT_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: LINEAR,
            path: "/v5/public/linear",
            upstream: "wss://stream.bybit.com/v5/public/linear".to_string(),
            endpoint: LINEAR_ENDPOINT,
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let config = BybitDataClientConfig {
            product_types: vec![BybitProductType::Linear],
            base_url_ws_public: Some(format!("ws://{proxy}/v5/public/linear")),
            update_instruments_interval_mins: None,
            instrument_poll_interval_secs: None,
            book_snapshot_timeout_secs: args.timeout_secs(),
            ..BybitDataClientConfig::default()
        };

        Ok(Box::new(BybitDataClient::new(*BYBIT_CLIENT_ID, config)?))
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        extract_raw_symbol(instrument_id.symbol.as_str()).to_string()
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let view = self
            .wire
            .views
            .find(&self.key(&id), deltas.sequence, deltas.ts_event.as_u64())
            .expect("wire oracle at emitted sequence and timestamp");

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            panic!(
                "wire oracle mismatch {id} seq={} ts={}: {violation}",
                deltas.sequence, deltas.ts_event
            );
        }

        if view.book.bids.is_empty() && view.book.asks.is_empty() {
            self.wire_empty += 1;
        }

        self.epochs.insert(id, view.epoch);
    }

    // A healthy book resynced on the expected connection and streams updates
    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches >= start.batches + 5
    }

    fn stats(&self) -> String {
        format!("{} wire_empty={}", self.oracle.stats(), self.wire_empty)
    }

    fn finish(&mut self) {
        self.oracle.abort();

        for symbol in SYMBOLS {
            let raw = raw_symbol(symbol);
            let checks = self.oracle.checks.lock().get(raw).copied().unwrap_or(0);
            assert!(checks > 0, "{raw} has no aligned venue comparison");
        }
    }
}

#[derive(Clone, Default)]
struct BybitWire {
    views: WireViews,
    sequences: SequenceBooks,
}

impl WireCodec for BybitWire {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(BybitConnection {
            views: self.views.clone(),
            sequences: self.sequences.clone(),
            route: route.name,
            epoch: number,
            books: HashMap::new(),
        })
    }
}

struct BybitConnection {
    views: WireViews,
    sequences: SequenceBooks,
    route: &'static str,
    epoch: usize,
    books: HashMap<String, WireBook>,
}

impl WireConnection for BybitConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        if frame["success"] == false {
            eprintln!("Venue error on route {}: {frame}", self.route);
        }

        if frame["op"] == "unsubscribe"
            && frame["success"] == true
            && let Some(key) = frame["req_id"].as_str().and_then(book_key)
        {
            return Upstream::Unsubscribed(key.to_string());
        }

        let Some(key) = frame["topic"].as_str().and_then(book_key) else {
            return Upstream::Other;
        };

        let key = key.to_string();
        let snapshot = frame["type"] == "snapshot";
        let book = self.books.entry(key.clone()).or_default();
        apply(book, &frame["data"], snapshot);
        let sequence = frame["data"]["seq"].as_u64().unwrap();
        let update_id = frame["data"]["u"].as_u64().unwrap();
        let top = book.top(DEPTH);
        self.sequences
            .record(&key, update_id, sequence, top.clone(), snapshot);
        self.views.record(
            &key,
            WireView {
                epoch: self.epoch,
                sequence,
                timestamp: frame["ts"].as_u64().unwrap() * 1_000_000,
                book: top,
            },
        );

        let kind = if snapshot {
            FrameKind::Snapshot
        } else {
            FrameKind::Update
        };

        Upstream::Book { key, kind }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        let Message::Text(text) = message else {
            return Vec::new();
        };

        serde_json::from_str::<Value>(text)
            .ok()
            .filter(|frame| frame["op"] == "unsubscribe")
            .and_then(|frame| {
                frame["args"].as_array().map(|args| {
                    args.iter()
                        .filter_map(|arg| arg.as_str().and_then(book_key))
                        .map(ToString::to_string)
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let Message::Text(text) = message else {
            return None;
        };

        let frame = serde_json::from_str::<Value>(text).ok()?;

        if frame["op"] != "subscribe" {
            return None;
        }

        let topic = frame["args"][0].as_str()?;
        let key = book_key(topic)?.to_string();
        let reply = json!({
            "success": false,
            "ret_msg": format!("error:handler not found,topic:{topic}"),
            "conn_id": "stress",
            "req_id": frame["req_id"],
            "op": "subscribe",
        });

        Some((key, Message::Text(reply.to_string().into())))
    }

    // Moves a delta's update ID off its successor, which the adapter must treat as a gap
    fn corrupt(&mut self, message: &mut Message, _key: &str, kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        if kind == FrameKind::Snapshot {
            return false;
        }

        let mut frame = serde_json::from_str::<Value>(text).unwrap();
        frame["data"]["u"] = json!(i64::MAX);
        *message = Message::Text(frame.to_string().into());
        true
    }
}

// Returns the symbol of an order book topic, such as `BTCUSDT` for `orderbook.50.BTCUSDT`
fn book_key(topic: &str) -> Option<&str> {
    topic
        .strip_prefix("orderbook.")
        .and_then(|rest| rest.split_once('.'))
        .map(|(_, symbol)| symbol)
}

fn apply(book: &mut WireBook, data: &Value, snapshot: bool) {
    if snapshot {
        *book = WireBook::default();
    }

    book.apply(&levels(&data["b"]), &levels(&data["a"]));
}

fn levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                Decimal::from_str(row[0].as_str().unwrap()).unwrap(),
                Decimal::from_str(row[1].as_str().unwrap()).unwrap(),
            )
        })
        .collect()
}

// Proves the wire oracle before any venue traffic, since a wrong oracle would pass a wrong book
fn check_wire_oracle() {
    {
        let mut book = WireBook::default();
        apply(
            &mut book,
            &json!({"b": [["10", "2"], ["9", "3"]], "a": [["11", "4"], ["12", "5"]]}),
            true,
        );
        apply(
            &mut book,
            &json!({"b": [["10", "0"], ["9", "7"]], "a": [["11", "8"], ["13", "6"]]}),
            false,
        );
        assert_eq!(
            book,
            WireBook {
                bids: [(Decimal::from(9), Decimal::from(7))].into(),
                asks: [
                    (Decimal::from(11), Decimal::from(8)),
                    (Decimal::from(12), Decimal::from(5)),
                    (Decimal::from(13), Decimal::from(6))
                ]
                .into(),
            }
        );

        apply(&mut book, &json!({"b": [["8", "9"]], "a": []}), true);
        assert_eq!(
            book,
            WireBook {
                bids: [(Decimal::from(8), Decimal::from(9))].into(),
                asks: [].into(),
            }
        );
        apply(&mut book, &json!({"b": [], "a": []}), true);
        assert_eq!(book, WireBook::default());
    }

    {
        let wire = BybitWire::default();

        let route = Route {
            name: LINEAR,
            path: "/v5/public/linear",
            upstream: String::new(),
            endpoint: LINEAR_ENDPOINT,
            headers: &[],
        };

        let mut connection = wire.open(&route, 3);
        let sequences = wire.sequences.clone();
        let text = |value: Value| Message::Text(value.to_string().into());

        let snapshot = text(json!({
            "topic": "orderbook.50.BTCUSDT",
            "type": "snapshot",
            "ts": 5,
            "data": {"s": "BTCUSDT", "b": [["10", "2"]], "a": [["11", "4"]], "u": 7, "seq": 70},
        }));
        let mut update = text(json!({
            "topic": "orderbook.50.BTCUSDT",
            "type": "delta",
            "ts": 6,
            "data": {"s": "BTCUSDT", "b": [["10", "0"]], "a": [], "u": 8, "seq": 80},
        }));
        let ack = text(json!({
            "success": true,
            "ret_msg": "",
            "conn_id": "test",
            "req_id": "orderbook.50.BTCUSDT",
            "op": "unsubscribe",
        }));
        let command = text(json!({
            "op": "unsubscribe",
            "args": ["orderbook.50.BTCUSDT", "orderbook.1.ETHUSDT", "publicTrade.BTCUSDT"],
            "req_id": "orderbook.50.BTCUSDT",
        }));

        let book = |key: &str, kind| Upstream::Book {
            key: key.to_string(),
            kind,
        };

        assert_eq!(
            connection.upstream(&snapshot),
            book("BTCUSDT", FrameKind::Snapshot)
        );
        assert_eq!(
            connection.upstream(&update),
            book("BTCUSDT", FrameKind::Update)
        );
        assert_eq!(
            connection.upstream(&ack),
            Upstream::Unsubscribed("BTCUSDT".to_string())
        );
        assert_eq!(connection.upstream(&command), Upstream::Other);
        assert_eq!(connection.client(&command), ["BTCUSDT", "ETHUSDT"]);
        assert!(connection.client(&snapshot).is_empty());

        let subscribe = text(json!({
            "op": "subscribe",
            "args": ["orderbook.50.BTCUSDT"],
            "req_id": "orderbook.50.BTCUSDT",
        }));
        let (key, reply) = connection.reject(&subscribe).unwrap();
        let reply = serde_json::from_str::<Value>(reply.to_text().unwrap()).unwrap();
        assert_eq!(key, "BTCUSDT");
        assert_eq!(reply["success"], json!(false));
        assert_eq!(reply["op"], json!("subscribe"));
        assert_eq!(reply["req_id"], json!("orderbook.50.BTCUSDT"));
        assert!(connection.reject(&command).is_none());

        assert_eq!(
            [
                wire.views.find("BTCUSDT", 70, 5_000_000),
                wire.views.find("BTCUSDT", 80, 6_000_000),
            ],
            [
                Some(WireView {
                    epoch: 3,
                    sequence: 70,
                    timestamp: 5_000_000,
                    book: WireBook {
                        bids: [(Decimal::from(10), Decimal::from(2))].into(),
                        asks: [(Decimal::from(11), Decimal::from(4))].into(),
                    },
                }),
                Some(WireView {
                    epoch: 3,
                    sequence: 80,
                    timestamp: 6_000_000,
                    book: WireBook {
                        bids: [].into(),
                        asks: [(Decimal::from(11), Decimal::from(4))].into(),
                    },
                }),
            ]
        );

        let mut unchanged = snapshot.clone();
        assert!(!connection.corrupt(&mut unchanged, "BTCUSDT", FrameKind::Snapshot));
        assert_eq!(unchanged, snapshot);
        assert!(connection.corrupt(&mut update, "BTCUSDT", FrameKind::Update));
        let corrupted = serde_json::from_str::<Value>(update.to_text().unwrap()).unwrap();
        assert_eq!(corrupted["data"]["u"], json!(i64::MAX));
        assert_eq!(
            sequences
                .find("BTCUSDT", 70)
                .expect("snapshot sequence")
                .bids,
            [(Decimal::from(10), Decimal::from(2))].into()
        );
        assert_eq!(
            sequences.find("BTCUSDT", 80).expect("linked update").bids,
            [].into()
        );
    }
}

const SEQUENCES_MAX: usize = 2048;
const VENUE_WS: &str = "wss://stream.bybit.com/v5/public/linear";

// Books rebuilt from a contiguous update-id chain, keyed by cross sequence for venue alignment
#[derive(Clone, Default)]
struct SequenceBooks {
    symbols: Arc<Mutex<HashMap<String, SymbolBooks>>>,
}

#[derive(Default)]
struct SymbolBooks {
    books: VecDeque<(u64, WireBook)>,
    last_update_id: Option<u64>,
}

impl SequenceBooks {
    fn record(&self, key: &str, update_id: u64, sequence: u64, book: WireBook, snapshot: bool) {
        let mut symbols = self.symbols.lock();
        let symbol = symbols.entry(key.to_string()).or_default();
        let linked = snapshot
            || symbol
                .last_update_id
                .is_some_and(|last| last.checked_add(1) == Some(update_id));

        if !linked {
            symbol.last_update_id = None;
            return;
        }

        symbol.last_update_id = Some(update_id);
        symbol.books.push_back((sequence, book));
        if symbol.books.len() > SEQUENCES_MAX {
            symbol.books.pop_front();
        }
    }

    fn find(&self, key: &str, sequence: u64) -> Option<WireBook> {
        self.symbols
            .lock()
            .get(key)?
            .books
            .iter()
            .rev()
            .find(|(recorded, _)| *recorded == sequence)
            .map(|(_, book)| book.clone())
    }
}

struct VenueOracle {
    checks: Arc<Mutex<HashMap<String, usize>>>,
    skipped: Arc<AtomicUsize>,
    empty: Arc<AtomicUsize>,
    errors: Arc<AtomicUsize>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl VenueOracle {
    fn start(books: SequenceBooks) -> Self {
        let checks = Arc::new(Mutex::new(HashMap::new()));
        let skipped = Arc::new(AtomicUsize::new(0));
        let empty = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(AtomicUsize::new(0));
        let handle = watch_venue(
            books,
            Arc::clone(&checks),
            Arc::clone(&skipped),
            Arc::clone(&empty),
            Arc::clone(&errors),
        );

        Self {
            checks,
            skipped,
            empty,
            errors,
            handle: Some(handle),
        }
    }

    fn abort(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }

    fn stats(&self) -> String {
        let checks = self.checks.lock();
        let aligned = checks.values().sum::<usize>();

        format!(
            "venue_checks={aligned} venue_books={} venue_skipped={} venue_empty={} venue_errors={}",
            checks.len(),
            self.skipped.load(Ordering::Relaxed),
            self.empty.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
        )
    }
}

impl Drop for VenueOracle {
    fn drop(&mut self) {
        self.abort();
    }
}

fn watch_venue(
    books: SequenceBooks,
    checks: Arc<Mutex<HashMap<String, usize>>>,
    skipped: Arc<AtomicUsize>,
    empty: Arc<AtomicUsize>,
    errors: Arc<AtomicUsize>,
) -> tokio::task::JoinHandle<()> {
    let task = async move {
        let topics = SYMBOLS.map(|symbol| format!("orderbook.{DEPTH}.{}", raw_symbol(symbol)));
        let subscribe = json!({"op": "subscribe", "args": topics}).to_string();
        loop {
            if let Err(e) = watch_venue_once(&books, &checks, &skipped, &empty, &subscribe).await {
                errors.fetch_add(1, Ordering::Relaxed);
                eprintln!("Venue oracle: {e}");
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    tokio::spawn(task) // tokio-import-ok: Standalone runtime
}

async fn watch_venue_once(
    books: &SequenceBooks,
    checks: &Mutex<HashMap<String, usize>>,
    skipped: &AtomicUsize,
    empty: &AtomicUsize,
    subscribe: &str,
) -> Result<(), String> {
    let (mut stream, _) = tokio_tungstenite::connect_async(VENUE_WS)
        .await
        .map_err(|e| e.to_string())?;
    stream
        .send(Message::Text(subscribe.to_string().into()))
        .await
        .map_err(|e| e.to_string())?;

    let mut local = HashMap::<String, (WireBook, Option<u64>)>::new();
    let mut pending = VecDeque::new();
    let mut ping = tokio::time::interval(Duration::from_secs(20));
    ping.tick().await;

    loop {
        tokio::select! {
            _ = ping.tick() => {
                stream
                    .send(Message::Text(r#"{"op":"ping"}"#.into()))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            message = stream.next() => {
                let Some(message) = message else {
                    return Ok(());
                };
                let Message::Text(text) = message.map_err(|e| e.to_string())? else {
                    continue;
                };
                let Ok(frame) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let Some(symbol) = frame["topic"].as_str().and_then(book_key) else {
                    continue;
                };
                let data = &frame["data"];
                let Some(update_id) = data["u"].as_u64() else {
                    continue;
                };
                let Some(sequence) = data["seq"].as_u64() else {
                    continue;
                };
                let snapshot = frame["type"] == "snapshot";
                let entry = local.entry(symbol.to_string()).or_default();
                let linked = snapshot || entry.1.is_some_and(|last| last.checked_add(1) == Some(update_id));

                if !linked {
                    entry.1 = None;
                    continue;
                }

                apply(&mut entry.0, data, snapshot);
                entry.1 = Some(update_id);
                let book = entry.0.top(DEPTH);
                pending.push_back(PendingView {
                    symbol: symbol.to_string(),
                    sequence,
                    book,
                    at: Instant::now(),
                });
                settle_pending(books, checks, skipped, empty, &mut pending);
            }
        }
    }
}

struct PendingView {
    symbol: String,
    sequence: u64,
    book: WireBook,
    at: Instant,
}

fn settle_pending(
    books: &SequenceBooks,
    checks: &Mutex<HashMap<String, usize>>,
    skipped: &AtomicUsize,
    empty: &AtomicUsize,
    pending: &mut VecDeque<PendingView>,
) {
    let now = Instant::now();

    pending.retain(|item| match books.find(&item.symbol, item.sequence) {
        Some(expected) if expected == item.book => {
            *checks.lock().entry(item.symbol.clone()).or_default() += 1;

            if item.book.bids.is_empty() && item.book.asks.is_empty() {
                empty.fetch_add(1, Ordering::Relaxed);
            }

            false
        }
        Some(expected) => panic!(
            "venue oracle mismatch {} seq={}: wire={expected:?}, venue={:?}",
            item.symbol, item.sequence, item.book
        ),
        None if now.duration_since(item.at) >= Duration::from_secs(2) => {
            skipped.fetch_add(1, Ordering::Relaxed);
            false
        }
        None => true,
    });
}

fn raw_symbol(symbol: &str) -> &str {
    symbol.split_once('-').map_or(symbol, |(raw, _)| raw)
}

fn check_sequence_books() {
    let books = SequenceBooks::default();

    let book = WireBook {
        bids: [(Decimal::from(10), Decimal::from(2))].into(),
        asks: [].into(),
    };

    books.record("BTCUSDT", 5, 50, book.clone(), true);
    books.record("BTCUSDT", 7, 70, book.clone(), false);
    books.record("BTCUSDT", 6, 60, book.clone(), false);
    assert_eq!(books.find("BTCUSDT", 50), Some(book.clone()));
    assert!(books.find("BTCUSDT", 70).is_none());
    assert!(books.find("BTCUSDT", 60).is_none());

    books.record("BTCUSDT", 8, 80, book.clone(), true);
    books.record("BTCUSDT", 9, 90, book.clone(), false);
    assert_eq!(books.find("BTCUSDT", 80), Some(book.clone()));
    assert_eq!(books.find("BTCUSDT", 90), Some(book));
}

fn check_pending_alignment() {
    let books = SequenceBooks::default();

    let book = WireBook {
        bids: [(Decimal::from(10), Decimal::from(2))].into(),
        asks: [].into(),
    };

    let empty_book = WireBook::default();
    books.record("BTCUSDT", 5, 50, book.clone(), true);
    books.record("ETHUSDT", 1, 10, empty_book.clone(), true);

    let checks = Mutex::new(HashMap::new());
    let skipped = AtomicUsize::new(0);
    let empty = AtomicUsize::new(0);

    let mut pending = VecDeque::from([
        PendingView {
            symbol: "BTCUSDT".to_string(),
            sequence: 50,
            book: book.clone(),
            at: Instant::now(),
        },
        PendingView {
            symbol: "ETHUSDT".to_string(),
            sequence: 10,
            book: empty_book,
            at: Instant::now(),
        },
    ]);

    settle_pending(&books, &checks, &skipped, &empty, &mut pending);

    assert!(pending.is_empty());
    assert_eq!(checks.lock().get("BTCUSDT").copied(), Some(1));
    assert_eq!(checks.lock().get("ETHUSDT").copied(), Some(1));
    assert_eq!(empty.load(Ordering::Relaxed), 1);
    assert_eq!(skipped.load(Ordering::Relaxed), 0);

    pending.push_back(PendingView {
        symbol: "BTCUSDT".to_string(),
        sequence: 99,
        book: book.clone(),
        at: Instant::now(),
    });

    settle_pending(&books, &checks, &skipped, &empty, &mut pending);
    assert_eq!(pending.len(), 1);
    assert_eq!(skipped.load(Ordering::Relaxed), 0);

    pending[0].at = Instant::now()
        .checked_sub(Duration::from_secs(3))
        .expect("clock supports a 3s lookback");
    settle_pending(&books, &checks, &skipped, &empty, &mut pending);
    assert!(pending.is_empty());
    assert_eq!(skipped.load(Ordering::Relaxed), 1);
}

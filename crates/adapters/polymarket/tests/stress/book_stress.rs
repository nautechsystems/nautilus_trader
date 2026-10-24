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
//! Run from a network location Polymarket serves, with adapter credentials unset:
//! `cargo test -p nautilus-polymarket --features examples --test polymarket-book-stress -- --timeout 10 --rounds 12`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates five fault phases over `--rounds` rounds.
//! - `boundaries`: probes retry exhaustion into the retry ceiling, a reconnect while recovery waits at
//!   the ceiling, unsubscribe during recovery, and shutdown during a reconnect.
//! - `disabled`: drops initial snapshots with deadlines disabled, then explicitly resubscribes.
//! - `auto-load`: drops initial snapshots after automatic instrument loading, with deadlines enabled.
//!
//! The harness subscribes one outcome token of each of the most traded open markets, or the
//! outcome tokens of six distinct markets given with `--tokens`, one per socket, so every frame on
//! a socket belongs to one book. No orders are submitted. Every emitted batch passes through the
//! shared `BookStreamChecker` and is verified against a reference book rebuilt from the raw frames
//! the proxy relays; a session fails unless every snapshot episode was verified. Separate venue
//! connections provide additional timestamp-aligned comparisons for every book, including empty books.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    rc::Rc,
    str::FromStr,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use nautilus_common::{
    cache::{Cache, CacheView},
    clients::DataClient,
    clock::VirtualClock,
    factories::DataClientFactory,
};
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    enums::BookAction,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::{mode::ReconnectRequestOutcome, retry::RetryConfig};
use nautilus_polymarket::{
    common::{consts::POLYMARKET_CLIENT_ID, urls::clob_ws_market_url},
    config::{PolymarketDataClientConfig, PolymarketInstrumentProviderConfig},
    factories::PolymarketDataClientFactory,
    http::{gamma::PolymarketGammaHttpClient, query::GetGammaMarketsParams},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, Flag, FrameKind, Route, Session, StressArgs, StressVenue, Upstream,
    WireBook, WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const BOOKS: usize = 6;

const MARKET: &str = "market";
// Discovery is disabled, so the primary shard stays closed and data shards start at 1
const SHARD_ENDPOINTS: [&str; BOOKS] = [
    "polymarket-market-streams-1",
    "polymarket-market-streams-2",
    "polymarket-market-streams-3",
    "polymarket-market-streams-4",
    "polymarket-market-streams-5",
    "polymarket-market-streams-6",
];
const DEPTH: usize = 20;
// Replacement attempts in the shared retry budget, including the first
const BUDGET: usize = 8;
const ORACLE_SAMPLES_MAX: usize = 2048;

// Token IDs by instrument, selected once before any session connects
static TOKENS: OnceLock<HashMap<InstrumentId, String>> = OnceLock::new();

type PolymarketSession = Session<Polymarket>;
type WireEventKey = (String, u64, usize);

fn main() {
    stress::run::<Polymarket, _, _>(|args| async move {
        let tokens = select_markets(&args).await;
        let mut ids = tokens.keys().copied().collect::<Vec<_>>();
        ids.sort_by_key(ToString::to_string);
        TOKENS.set(tokens).expect("markets select once");

        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
            "disabled" => disabled(&args, &ids).await,
            "auto-load" => auto_load(&args, &ids).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let mut total = 0;
    let mut session = PolymarketSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    for round in 0..args.rounds() {
        let phase = round % 5;
        let phase_started = Instant::now();
        let id = ids[round / 5 % ids.len()];

        match phase {
            // A valid replay can arrive before the snapshot deadline, so a cycle is not required
            0 => {
                let corrupted = {
                    let mut fault = session.fault(&id);
                    fault.corrupt = 1;
                    fault.corrupted
                };

                resubscribe(&mut session, id).await;
                let attempts = session.venue().attempts(&id);
                session.healthy(&[id]).await;
                assert_eq!(session.fault(&id).corrupted, corrupted + 1);
                stress::check(
                    "corrupt_snapshot",
                    format!(
                        "instrument={id} attempts={}",
                        session.venue().attempts(&id) - attempts
                    ),
                );
            }
            // A lost initial snapshot recovers on its deadline or a later update
            1 => {
                let dropped = {
                    let mut fault = session.fault(&id);
                    fault.drop_snapshots = 1;
                    fault.dropped
                };

                resubscribe(&mut session, id).await;
                session.healthy(&[id]).await;
                assert_eq!(session.fault(&id).dropped, dropped + 1);
            }
            // A reconnect while a recovering book's events are held
            2 => {
                start_held_recovery(&mut session, id).await;

                reconnect_all(&mut session);
                session.fault(&id).hold = false;
                session.healthy(ids).await;
            }
            // Replayed dumps lost after a reconnect; without deadlines the next update recovers
            3 => {
                for id in ids {
                    session.fault(id).drop_snapshots = 1;
                }

                reconnect_all(&mut session);
                session.healthy(ids).await;
            }
            // A restart while recovery waits for a dump
            4 => {
                start_held_recovery(&mut session, id).await;
                wait_oracle(&mut session).await;
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
                "phase={phase} instrument={id} phase_ms={} batches_total={}",
                phase_started.elapsed().as_millis(),
                total + session.batches()
            ),
        );
    }

    wait_oracle(&mut session).await;
    total += session.batches();
    session.stop().await;
    format!("books={} batches_total={total}", ids.len())
}

async fn boundaries(args: &StressArgs, ids: &[InstrumentId]) -> String {
    let timeout = args.timeout_secs();
    let mut session = PolymarketSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;

    // Held events spend the retry budget; a released snapshot completes the recovery
    let id = ids[0];
    let attempts = start_held_recovery(&mut session, id).await;
    let window = Duration::from_secs(185);
    session.observe(window).await;

    let budget = if timeout > 0 { BUDGET } else { 1 };
    let ceiling_max = window.as_secs() as usize / 60;
    let made = session.venue().attempts(&id) - attempts;
    assert!(
        (budget..=budget + ceiling_max).contains(&made),
        "{id} made {made} attempts; expected the budget of {budget} plus at most one ceiling \
         attempt per minute"
    );

    let connections = session.proxy().connections(MARKET);
    session.fault(&id).hold = false;
    session.expect_snapshot(id);
    session.proxy().release();
    session.healthy(&[id]).await;
    assert_eq!(session.proxy().connections(MARKET), connections);
    stress::check("exhaustion", format!("instrument={id} attempts={made}"));

    // A reconnect restores a book whose recovery waits at the ceiling; reconnect replay also
    // resubscribes the book, so this does not isolate the ceiling wake
    let id = ids[1];
    let attempts = start_held_recovery(&mut session, id).await;
    let started = Instant::now();

    session
        .until(Duration::from_secs(180), "retry budget spent", |s| {
            s.venue().attempts(&id) >= attempts + budget
        })
        .await;

    // The last budgeted attempt ends at its snapshot deadline, or at the 180-second budget when
    // deadlines are disabled, before the ceiling wait begins
    let settle = if timeout > 0 {
        Duration::from_secs(timeout + 5)
    } else {
        Duration::from_secs(185).saturating_sub(started.elapsed())
    };

    session.observe(settle).await;
    session.fault(&id).hold = false;
    let reconnected = Instant::now();
    reconnect_all(&mut session);
    session.healthy(ids).await;
    let recovered = reconnected.elapsed();
    assert!(
        recovered < Duration::from_secs(45),
        "{id} recovered {recovered:?} after the reconnect; expected well inside the ceiling wait"
    );
    stress::check(
        "ceiling_reconnect",
        format!("instrument={id} recovered_s={}", recovered.as_secs()),
    );

    // Unsubscribing a recovering book stops its replacement writes
    let id = ids[2];
    let attempts = start_held_recovery(&mut session, id).await;

    session
        .until(
            Duration::from_secs(60),
            "replacement attempted before unsubscribe",
            |s| s.venue().attempts(&id) > attempts,
        )
        .await;
    session.unsubscribe(id);
    session.observe(Duration::from_secs(2)).await;
    session.close(id);
    let attempts = session.venue().attempts(&id);
    session.observe(Duration::from_secs(30)).await;
    assert_eq!(
        session.venue().attempts(&id),
        attempts,
        "{id} sent a replacement after its unsubscribe"
    );
    session.fault(&id).hold = false;
    session.proxy().release();
    session.observe(Duration::from_secs(2)).await;
    session.subscribe(id);
    session.healthy(&[id]).await;
    stress::check("unsubscribe", format!("instrument={id}"));

    wait_oracle(&mut session).await;
    let cuts = session.proxy().cuts();
    session.proxy().cut(Some(MARKET), FrameKind::Snapshot, 10);
    reconnect_all(&mut session);
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

async fn disabled(args: &StressArgs, ids: &[InstrumentId]) -> String {
    assert_eq!(
        args.timeout_secs(),
        0,
        "disabled probe requires --timeout 0"
    );
    let mut session = PolymarketSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;
    for id in ids {
        let dropped = session.fault(id).dropped;
        session.fault(id).drop_snapshots = 1;
        resubscribe(&mut session, *id).await;
        session
            .until(Duration::from_secs(30), "initial snapshot dropped", |s| {
                s.fault(id).dropped > dropped
            })
            .await;

        let attempts = session.venue().attempts(id);
        let updates_before = session.venue().updates_received(id, None);
        session.observe(Duration::from_secs(15)).await;
        let updates = session.venue().updates_received(id, None) - updates_before;
        if updates == 0 {
            assert_eq!(
                session.venue().attempts(id),
                attempts,
                "{id} retried a silent book with snapshot deadlines disabled"
            );
        }

        resubscribe(&mut session, *id).await;
        session.healthy(&[*id]).await;
        stress::check(
            "disabled_deadline",
            format!("instrument={id} wire_updates={updates}"),
        );
    }

    wait_oracle(&mut session).await;
    session.stop().await
}

async fn auto_load(args: &StressArgs, ids: &[InstrumentId]) -> String {
    assert!(
        args.timeout_secs() > 0,
        "auto-load probe requires a positive timeout"
    );
    let mut session = PolymarketSession::connect(args).await;
    for id in ids {
        session.fault(id).drop_snapshots = 1;
    }

    subscribe_all(&mut session, ids);
    session.healthy(ids).await;
    for id in ids {
        assert_eq!(session.fault(id).dropped, 1);
        stress::check(
            "auto_load_deadline",
            format!("instrument={id} attempts={}", session.venue().attempts(id)),
        );
    }

    wait_oracle(&mut session).await;
    session.stop().await
}

async fn wait_oracle(session: &mut PolymarketSession) {
    session
        .until(
            Polymarket::HEALTHY_LIMIT,
            "all books have aligned independent venue comparisons",
            |s| s.venue().oracle_checks.len() == BOOKS,
        )
        .await;
}

// Breaks the hash of a resubscribed book's snapshot so it needs recovery, then holds every
// later event for the book so none can complete it; returns the book's replacement attempts
// before the recovery. A book event relayed before the hold starts can still complete the
// recovery, which then fails the caller's wait
async fn start_held_recovery(session: &mut PolymarketSession, id: InstrumentId) -> usize {
    let corrupted = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.corrupted
    };

    resubscribe(session, id).await;
    let attempts = session.venue().attempts(&id);

    session
        .until(
            Duration::from_secs(30),
            "subscribe snapshot rejected",
            |s| s.fault(&id).corrupted > corrupted,
        )
        .await;

    session.fault(&id).hold = true;
    session
        .until(Duration::from_secs(60), "replacement attempted", |s| {
            s.venue().attempts(&id) > attempts
        })
        .await;

    attempts
}

// Replaces a book's subscription, requiring a new snapshot from the fresh subscribe
async fn resubscribe(session: &mut PolymarketSession, id: InstrumentId) {
    session.unsubscribe(id);
    session.observe(Duration::from_secs(2)).await;
    session.close(id);
    session.subscribe(id);
}

// Reconnects every shard, requiring each book to resync from a newer connection; every book
// reports the first endpoint, so the first request expects a snapshot from all of them
fn reconnect_all(session: &mut PolymarketSession) {
    let expected = session.proxy().connections(MARKET) + 1;
    session.venue_mut().expected_epoch = expected;

    for endpoint in SHARD_ENDPOINTS {
        assert_eq!(
            session.reconnect(endpoint),
            ReconnectRequestOutcome::Accepted
        );
    }
}

fn subscribe_all(session: &mut PolymarketSession, ids: &[InstrumentId]) {
    for id in ids {
        session.subscribe(*id);
    }
}

// Selects the `--tokens` outcome tokens, or else the first outcome token of the most traded open
// markets with an order book
async fn select_markets(args: &StressArgs) -> HashMap<InstrumentId, String> {
    let client = PolymarketGammaHttpClient::new(None, 30, RetryConfig::default())
        .expect("Gamma client builds");

    let requested = args
        .flag("tokens")
        .split(',')
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    let params = if requested.is_empty() {
        GetGammaMarketsParams {
            active: Some(true),
            closed: Some(false),
            order: Some("volume24hr".to_string()),
            ascending: Some(false),
            max_markets: Some(50),
            ..Default::default()
        }
    } else {
        GetGammaMarketsParams {
            clob_token_ids: Some(requested.clone()),
            ..Default::default()
        }
    };

    let markets = client
        .request_markets_by_params(params)
        .await
        .expect("Gamma markets load");

    let tokens = markets
        .iter()
        .filter(|market| {
            market.enable_order_book == Some(true) && market.accepting_orders == Some(true)
        })
        .filter_map(|market| {
            let tokens = serde_json::from_str::<Vec<String>>(&market.clob_token_ids).ok()?;

            let token = if requested.is_empty() {
                tokens.into_iter().next()?
            } else {
                tokens.into_iter().find(|token| requested.contains(token))?
            };

            let id = InstrumentId::from(format!("{}-{token}.POLYMARKET", market.condition_id));
            Some((id, token))
        })
        .take(BOOKS)
        .collect::<HashMap<_, _>>();

    assert_eq!(tokens.len(), BOOKS, "Gamma lists {BOOKS} open order books");
    tokens
}

#[derive(Default)]
struct Polymarket {
    wire: PolymarketWire,
    oracle: PolymarketWire,
    oracle_tasks: Vec<tokio::task::JoinHandle<()>>,
    oracle_pending: VecDeque<(String, WireView)>,
    oracle_update: HashMap<InstrumentId, (String, WireView)>,
    oracle_positions: HashMap<InstrumentId, (u64, bool)>,
    oracle_skipped: usize,
    oracle_checks: HashMap<String, usize>,
    oracle_checks_empty: usize,
    wire_checks: usize,
    wire_snapshots_empty: usize,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
    snapshots_first: HashMap<InstrumentId, u64>,
    updates: HashMap<InstrumentId, usize>,
}

impl Polymarket {
    // Replacement attempts the adapter made for a book, counted by their unsubscribe legs
    fn attempts(&self, id: &InstrumentId) -> usize {
        self.wire
            .unsubscribes
            .lock()
            .get(&self.key(id))
            .copied()
            .unwrap_or(0)
    }

    fn updates_received(&self, id: &InstrumentId, after: Option<u64>) -> usize {
        self.wire.updates_received(&self.key(id), after)
    }

    fn queue_oracle(
        &mut self,
        id: InstrumentId,
        snapshot: bool,
        sample: Option<(String, WireView)>,
    ) {
        let previous = self.oracle_update.remove(&id);

        let Some((key, state)) = sample else {
            return;
        };

        if let Some(previous) = previous
            && !snapshot
            && previous.1.timestamp < state.timestamp
        {
            self.oracle_pending.push_back(previous);
        }

        if snapshot {
            self.oracle_pending.push_back((key, state));
        } else {
            self.oracle_update.insert(id, (key, state));
        }

        if self.oracle_pending.len() > ORACLE_SAMPLES_MAX {
            self.oracle_pending.pop_front();
        }
    }
}

impl StressVenue for Polymarket {
    const NAME: &'static str = "polymarket";
    const SCENARIOS: &'static [&'static str] = &["churn", "boundaries", "disabled", "auto-load"];
    const ROUNDS: usize = 12;
    const FLAGS: &'static [Flag] = &[Flag {
        name: "tokens",
        default: "",
        help: "Comma-separated outcome token IDs from six distinct open markets to test instead",
    }];
    // Polymarket books carry no sequence, so the wire oracle verifies content
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(120);

    fn self_check() {
        check_wire_oracle();
        check_book_burst();
        check_oracle_samples();
        check_lagging_dump();
    }

    fn new(_args: &StressArgs) -> Self {
        let oracle = PolymarketWire::default();
        let oracle_tasks = TOKENS
            .get()
            .expect("markets select before sessions")
            .values()
            .map(|token| watch_oracle(oracle.clone(), token.clone()))
            .collect();

        Self {
            oracle,
            oracle_tasks,
            expected_epoch: 1,
            ..Self::default()
        }
    }

    fn client_id(&self) -> ClientId {
        *POLYMARKET_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: MARKET,
            path: "/ws/market",
            upstream: clob_ws_market_url().to_string(),
            endpoint: SHARD_ENDPOINTS[0],
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let tokens = TOKENS.get().expect("markets select before sessions");

        let config = PolymarketDataClientConfig {
            instrument_config: Some(PolymarketInstrumentProviderConfig {
                load_ids: Some(if args.scenario() == "auto-load" {
                    Vec::new()
                } else {
                    tokens.keys().copied().collect()
                }),
                ..Default::default()
            }),
            base_url_ws: Some(format!("ws://{proxy}/ws/market")),
            ws_max_subscriptions: 1,
            update_instruments_interval_mins: None,
            book_snapshot_timeout_secs: args.timeout_secs(),
            ..Default::default()
        };

        let cache = CacheView::from(Rc::new(RefCell::new(Cache::default())));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));

        PolymarketDataClientFactory.create(POLYMARKET_CLIENT_ID.as_str(), &config, cache, clock)
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        TOKENS.get().expect("markets select before sessions")[instrument_id].clone()
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        assert_eq!(
            deltas.sequence, 0,
            "{id} emitted a sequence for an unsequenced book"
        );

        let snapshot = deltas
            .deltas
            .first()
            .is_some_and(|delta| delta.action == BookAction::Clear);

        if !snapshot {
            *self.updates.entry(id).or_default() += 1;
        }

        let token = self.key(&id);
        let key = view_key(&token, snapshot);
        let timestamp = deltas.ts_event.as_u64();

        // A view evicted while events were held cannot be compared, so the sample is skipped
        let Some(source) = self.wire.view(&key, deltas.sequence, timestamp) else {
            self.queue_oracle(id, snapshot, None);
            assert!(
                self.wire
                    .events
                    .lock()
                    .keys()
                    .any(|(seen_key, seen_ts, _)| { seen_key == &key && *seen_ts == timestamp }),
                "wire oracle has no {key} event at emitted ts={timestamp}"
            );
            return;
        };

        assert_eq!(
            source.snapshot, snapshot,
            "source event kind matches emitted batch"
        );
        let view = source.view;
        let position = self.oracle_positions.entry(id).or_default();
        let comparable = advance_oracle_position(position, timestamp, snapshot);
        let sample = source.hash.and_then(|hash| {
            let comparable = comparable && source.comparable;
            self.oracle_skipped += usize::from(!comparable);

            if !comparable {
                return None;
            }

            let key = format!("{}:{hash}", self.key(&id));
            let book = checker.book(id).expect("the checker accepted this batch");

            let state = WireView {
                epoch: view.epoch,
                sequence: 0,
                timestamp,
                book: WireBook {
                    bids: book.bids_as_map(Some(DEPTH)).into_iter().collect(),
                    asks: book.asks_as_map(Some(DEPTH)).into_iter().collect(),
                },
            };

            Some((key, state))
        });

        self.queue_oracle(id, snapshot, sample);

        self.poll();

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            eprintln!(
                "DIAGNOSTIC source_comparable={} expected={view:?} emitted={deltas:?} actual_bids={:?} actual_asks={:?} wire={:?}",
                source.comparable,
                checker.book(id).unwrap().bids_as_map(Some(DEPTH)),
                checker.book(id).unwrap().asks_as_map(Some(DEPTH)),
                self.wire.traces.lock().get(&self.key(&id))
            );
            panic!(
                "wire oracle mismatch {id} ts={}: {violation}",
                deltas.ts_event
            );
        }

        self.epochs.insert(id, view.epoch);
        if snapshot {
            self.snapshots_first.entry(id).or_insert(timestamp);
        }

        self.wire_checks += 1;

        if snapshot && view.book.bids.is_empty() && view.book.asks.is_empty() {
            self.wire_snapshots_empty += 1;
        }
    }

    // A quiet book needs no second event after its accepted, verified snapshot
    fn streaming(&self, id: &InstrumentId, _book: &BookProgress, _start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
    }

    fn poll(&mut self) {
        self.oracle_pending.retain(|(key, expected)| {
            let Some(actual) = self.oracle.view_hashed(key, expected.timestamp) else {
                return true;
            };

            assert_eq!(
                actual.book,
                expected.book,
                "independent venue oracle mismatch {key} at {}, wire={:?}, oracle={:?}",
                expected.timestamp,
                self.wire.traces.lock().get(key.split(':').next().unwrap()),
                self.oracle
                    .traces
                    .lock()
                    .get(key.split(':').next().unwrap()),
            );
            let token = key.split(':').next().unwrap();
            *self.oracle_checks.entry(token.to_string()).or_default() += 1;

            if actual.book.bids.is_empty() && actual.book.asks.is_empty() {
                self.oracle_checks_empty += 1;
            }

            false
        });
    }

    fn stats(&self) -> String {
        format!(
            "wire_checks={} venue_checks={} venue_books={} oracle_skipped={} wire_snapshots_empty={} venue_checks_empty={}",
            self.wire_checks,
            self.oracle_checks.values().sum::<usize>(),
            self.oracle_checks.len(),
            self.oracle_skipped,
            self.wire_snapshots_empty,
            self.oracle_checks_empty,
        )
    }

    // Snapshot-only recovery can mask a stalled incremental stream
    // Count post-baseline venue positions before faults, including held updates
    fn finish(&mut self) {
        self.poll();

        for (id, token) in TOKENS.get().expect("markets select before sessions") {
            let sent = self.updates_received(id, self.snapshots_first.get(id).copied());
            let emitted = self.updates.get(id).copied().unwrap_or(0);

            assert!(
                sent < 10 || emitted > 0,
                "{id} emitted no incremental updates although the venue sent {sent} after its first snapshot"
            );
            let checks = self.oracle_checks.get(token).copied().unwrap_or(0);
            assert!(
                checks > 0,
                "{id} has no aligned independent venue comparison"
            );
            stress::check(
                "book_coverage",
                format!(
                    "instrument={id} venue_checks={checks} updates={emitted} wire_updates={sent}"
                ),
            );
        }

        for task in &self.oracle_tasks {
            task.abort();
        }
    }
}

fn advance_oracle_position(position: &mut (u64, bool), timestamp: u64, snapshot: bool) -> bool {
    if timestamp < position.0 {
        position.1 = true;
    } else if snapshot {
        position.1 = false;
    }

    position.0 = position.0.max(timestamp);
    snapshot || !position.1
}

#[derive(Clone, Default)]
struct PolymarketWire {
    views: WireViews,
    ambiguous: Arc<Mutex<HashSet<(String, u64)>>>,
    // Recorded events by view key and timestamp, which tell an ambiguous or evicted view from a
    // missing one
    events: Arc<Mutex<HashMap<WireEventKey, usize>>>,
    history: Arc<Mutex<HashMap<String, VecDeque<SourceView>>>>,
    traces: Arc<Mutex<HashMap<String, VecDeque<Value>>>>,
    unsubscribes: Arc<Mutex<HashMap<String, usize>>>,
}

impl PolymarketWire {
    fn view(&self, key: &str, sequence: u64, timestamp: u64) -> Option<SourceView> {
        let samples = self.history.lock();

        if self
            .ambiguous
            .lock()
            .contains(&(key.to_string(), timestamp))
        {
            return None;
        }

        samples
            .get(key)?
            .iter()
            .rev()
            .find(|sample| sample.view.timestamp == timestamp && sample.view.sequence == sequence)
            .cloned()
    }

    fn view_hashed(&self, key: &str, timestamp: u64) -> Option<WireView> {
        let (token, hash) = key.split_once(':')?;
        let history = self.history.lock();
        let snapshot = view_key(token, true);

        [snapshot.as_str(), token].into_iter().find_map(|key| {
            history
                .get(key)?
                .iter()
                .rev()
                .find(|sample| {
                    sample.hash.as_deref() == Some(hash) && sample.view.timestamp == timestamp
                })
                .map(|sample| sample.view.clone())
        })
    }

    fn updates_received(&self, token: &str, after: Option<u64>) -> usize {
        self.events
            .lock()
            .iter()
            .filter(|((key, timestamp, _), _)| {
                key == token && after.is_none_or(|after| *timestamp > after)
            })
            .map(|(_, count)| count)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceView {
    snapshot: bool,
    hash: Option<String>,
    comparable: bool,
    view: WireView,
}

impl WireCodec for PolymarketWire {
    fn open(&self, _route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(PolymarketConnection {
            wire: self.clone(),
            epoch: number,
            tokens: HashSet::new(),
            books: HashMap::new(),
            position: (0, false),
        })
    }
}

fn watch_oracle(wire: PolymarketWire, token: String) -> tokio::task::JoinHandle<()> {
    let task = async move {
        let mut epoch = 0;
        loop {
            epoch += 1;
            let (mut stream, _) = tokio::time::timeout(
                Duration::from_secs(30),
                tokio_tungstenite::connect_async(clob_ws_market_url()),
            )
            .await
            .expect("independent oracle connects within 30 seconds")
            .expect("independent oracle connects");

            let mut connection = PolymarketConnection {
                wire: wire.clone(),
                epoch,
                tokens: HashSet::from([token.clone()]),
                books: HashMap::new(),
                position: (0, false),
            };

            let request = json!({"assets_ids": [token], "type": "market", "initial_dump": true});
            stream
                .send(Message::Text(request.to_string().into()))
                .await
                .expect("independent oracle subscribes");
            let mut ping = tokio::time::interval(Duration::from_secs(10));
            ping.tick().await;
            let unsubscribe = json!({"assets_ids": [token], "operation": "unsubscribe"});
            let subscribe =
                json!({"assets_ids": [token], "operation": "subscribe", "initial_dump": true});

            'connected: loop {
                tokio::select! {
                    _ = ping.tick() => {
                        for text in ["PING".to_string(), unsubscribe.to_string(), subscribe.to_string()] {
                            if let Err(e) = stream.send(Message::Text(text.into())).await {
                                eprintln!("Oracle send ended token={token}: {e}");
                                break 'connected;
                            }
                        }
                    }
                    message = stream.next() => {
                        match message {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(frame) = serde_json::from_str::<Value>(&text) {
                                    for event in events(&frame).into_iter().filter(|event| event["event_type"] == "book") {
                                        connection.apply(event);
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => break 'connected,
                            Some(Err(e)) => {
                                eprintln!("Oracle stream ended token={token}: {e}");
                                break 'connected;
                            }
                            _ => {}
                        }
                    }
                }
            }

            eprintln!("ORACLE_RECONNECT token={token} epoch={epoch}");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    tokio::spawn(task) // tokio-import-ok: Standalone runtime
}

struct PolymarketConnection {
    wire: PolymarketWire,
    epoch: usize,
    tokens: HashSet<String>,
    books: HashMap<String, WireBook>,
    position: (u64, bool),
}

impl PolymarketConnection {
    // Applies one market event to the reference book of a subscribed token, returning the token
    // and whether the event was a hash-verifiable dump
    fn apply(&mut self, event: &Value) -> Option<(String, bool)> {
        let timestamp = event["timestamp"].as_str()?.parse::<u64>().ok()? * 1_000_000;
        let stale_snapshot = event["event_type"] == "book" && timestamp < self.position.0;

        let (key, snapshot, dump, hash) = match event["event_type"].as_str()? {
            "book" => {
                let key = self.subscribed(event["asset_id"].as_str()?)?;
                if !stale_snapshot {
                    let book = self.books.entry(key.clone()).or_default();
                    *book = WireBook::default();
                    book.apply(&levels(&event["bids"]), &levels(&event["asks"]));
                }

                (key, true, is_dump(event), event["hash"].as_str())
            }
            "price_change" => {
                let changes = event["price_changes"].as_array()?;
                let key = changes
                    .iter()
                    .find_map(|change| self.subscribed(change["asset_id"].as_str()?))?;
                let book = self.books.entry(key.clone()).or_default();

                for change in changes
                    .iter()
                    .filter(|change| change["asset_id"] == key.as_str())
                {
                    let level = [(decimal(&change["price"]), decimal(&change["size"]))];

                    match change["side"].as_str()? {
                        "BUY" => book.apply(&level, &[]),
                        _ => book.apply(&[], &level),
                    }
                }

                let change = changes
                    .iter()
                    .rev()
                    .find(|change| change["asset_id"] == key.as_str())
                    .expect("the frame contains the selected token");

                prune_crossed(book, change);

                let hash = change["hash"].as_str();
                (key, false, false, hash)
            }
            _ => return None,
        };

        let comparable = advance_oracle_position(&mut self.position, timestamp, snapshot);
        let view = view_key(&key, snapshot);
        {
            let mut traces = self.wire.traces.lock();
            let trace = traces.entry(key.clone()).or_default();
            trace.push_back(json!({"epoch": self.epoch, "event": event}));
            if trace.len() > 64 {
                trace.pop_front();
            }
        }

        *self
            .wire
            .events
            .lock()
            .entry((view.clone(), timestamp, self.epoch))
            .or_default() += 1;

        if stale_snapshot {
            return Some((key, dump));
        }

        let book = self.books[&key].top(DEPTH);
        // Publish ambiguity, source levels, and hash metadata as one history transaction
        let mut history = self.wire.history.lock();

        if let Some(prior) = self.wire.views.find(&view, 0, timestamp)
            && prior.book != book
        {
            self.wire.ambiguous.lock().insert((view.clone(), timestamp));
        }

        let state = WireView {
            epoch: self.epoch,
            sequence: 0,
            timestamp,
            book,
        };

        self.wire.views.record(&view, state.clone());

        let samples = history.entry(view).or_default();
        samples.push_back(SourceView {
            snapshot,
            hash: hash
                .filter(|hash| !hash.is_empty())
                .map(ToString::to_string),
            comparable,
            view: state,
        });

        if samples.len() > ORACLE_SAMPLES_MAX {
            samples.pop_front();
        }

        Some((key, dump))
    }

    // Returns the token as a fault key when the adapter subscribed it on this connection
    fn subscribed(&self, token: &str) -> Option<String> {
        self.tokens.contains(token).then(|| token.to_string())
    }
}

impl WireConnection for PolymarketConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        let mut book = None;

        for event in events(&frame) {
            if let Some((key, _)) = self.apply(event) {
                let kind = book.as_ref().map_or(FrameKind::Update, |(_, kind)| *kind);

                let kind = if event["event_type"] == "book" {
                    FrameKind::Snapshot
                } else {
                    kind
                };

                book = Some((key, kind));
            }
        }

        book.map_or(Upstream::Other, |(key, kind)| Upstream::Book { key, kind })
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        let Message::Text(text) = message else {
            return Vec::new();
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };

        let assets = frame["assets_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToString::to_string);

        if frame["operation"] != "unsubscribe" {
            self.tokens.extend(assets);
            return Vec::new();
        }

        let keys = assets
            .filter(|asset| self.tokens.contains(asset))
            .collect::<Vec<_>>();
        let mut unsubscribes = self.wire.unsubscribes.lock();

        for key in &keys {
            *unsubscribes.entry(key.clone()).or_default() += 1;
        }

        keys
    }

    // Breaks the hash of a dump, which the adapter must reject and recover from
    fn corrupt(&mut self, message: &mut Message, key: &str, kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        if kind == FrameKind::Update {
            return false;
        }

        let mut frame = serde_json::from_str::<Value>(text).unwrap();

        let dump = match &mut frame {
            Value::Array(events) => events.iter_mut().find(|event| is_dump_of(event, key)),
            event => Some(event).filter(|event| is_dump_of(event, key)),
        };

        let Some(dump) = dump else {
            return false;
        };

        dump["hash"] = json!("0000000000000000000000000000000000000000");
        *message = Message::Text(frame.to_string().into());
        true
    }
}

fn prune_crossed(book: &mut WireBook, change: &Value) {
    if !book
        .bids
        .last_key_value()
        .zip(book.asks.first_key_value())
        .is_some_and(|((bid, _), (ask, _))| bid > ask)
    {
        return;
    }

    if let Some(top) = change["best_bid"]
        .as_str()
        .and_then(|s| Decimal::from_str(s).ok())
    {
        book.bids.retain(|price, _| *price <= top);
    }

    if let Some(top) = change["best_ask"]
        .as_str()
        .and_then(|s| Decimal::from_str(s).ok())
    {
        book.asks.retain(|price, _| *price >= top);
    }
}

// Emitted snapshots align with `book` events and updates with `price_change` events, since the
// venue stamps both kinds to the millisecond and often at the same time
fn view_key(token: &str, snapshot: bool) -> String {
    if snapshot {
        format!("{token}:book")
    } else {
        token.to_string()
    }
}

// The venue sends one event or an array of events per frame
fn events(frame: &Value) -> Vec<&Value> {
    match frame {
        Value::Array(events) => events.iter().collect(),
        event => vec![event],
    }
}

// A subscribe dump carries the full hash preimage; later book events omit part of it, so the
// adapter cannot verify them
fn is_dump(event: &Value) -> bool {
    event["event_type"] == "book"
        && event["hash"].is_string()
        && event["tick_size"].is_string()
        && event["last_trade_price"].is_string()
}

fn is_dump_of(event: &Value, key: &str) -> bool {
    is_dump(event) && event["asset_id"] == key
}

fn levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    rows.as_array()
        .into_iter()
        .flatten()
        .map(|row| (decimal(&row["price"]), decimal(&row["size"])))
        .collect()
}

fn decimal(value: &Value) -> Decimal {
    Decimal::from_str(value.as_str().unwrap()).unwrap()
}

fn check_book_burst() {
    let mut position = (0, false);
    let comparable = [
        (10, true),
        (12, false),
        (11, true),
        (13, false),
        (12, true),
        (14, false),
        (14, true),
        (15, false),
    ]
    .map(|(timestamp, snapshot)| advance_oracle_position(&mut position, timestamp, snapshot));
    assert_eq!(
        comparable,
        [true, true, true, false, true, false, true, true]
    );
    assert_eq!(position, (15, false));

    // A burst's intermediate changes can carry its completed book hash
    let fixture: Value =
        serde_json::from_str(include_str!("../../test_data/ws_market_book_burst.json")).unwrap();
    let snapshot = &fixture["snapshot"];
    let token = snapshot["asset_id"].as_str().unwrap();
    let wire = PolymarketWire::default();

    let mut connection = PolymarketConnection {
        wire: wire.clone(),
        epoch: 1,
        tokens: HashSet::from([token.to_string()]),
        books: HashMap::new(),
        position: (0, false),
    };

    connection.apply(snapshot);
    for event in fixture["updates"].as_array().unwrap() {
        connection.apply(event);
    }

    let oracle = &fixture["oracle_snapshot"];
    let hash = oracle["hash"].as_str().unwrap();
    let timestamp = oracle["timestamp"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap()
        * 1_000_000;
    let emitted = wire
        .view_hashed(&format!("{token}:{hash}"), timestamp)
        .unwrap();
    let mut expected = WireBook::default();
    expected.apply(&levels(&oracle["bids"]), &levels(&oracle["asks"]));
    let mut expected = expected.top(DEPTH);
    let price = Decimal::from_str("0.172").unwrap();
    let mut actual = emitted.book;
    assert_eq!(actual.bids.remove(&price), Some(Decimal::from(135)));
    assert_eq!(expected.bids.remove(&price), Some(Decimal::from(349)));
    assert_eq!(actual, expected);
    connection.apply(oracle);
    let closed = wire
        .view_hashed(&format!("{token}:{hash}"), timestamp)
        .unwrap();
    assert_eq!(closed.book.bids.get(&price), Some(&Decimal::from(349)));
    expected.bids.insert(price, Decimal::from(349));
    assert_eq!(closed.book, expected);

    let crossing: Vec<Value> = serde_json::from_str(include_str!(
        "../../test_data/ws_market_price_change_crossing_burst.json"
    ))
    .unwrap();
    let token = crossing[0]["asset_id"].as_str().unwrap();
    connection.tokens = HashSet::from([token.to_string()]);
    connection.position = (0, false);
    for (index, event) in crossing.iter().enumerate() {
        connection.apply(event);
        let book = &connection.books[token];

        if index == crossing.len() - 4 {
            assert_eq!(
                book.bids.last_key_value().map(|(price, _)| *price),
                Some(Decimal::from_str("0.29").unwrap())
            );
            assert_eq!(
                book.asks.first_key_value().map(|(price, _)| *price),
                Some(Decimal::from_str("0.301").unwrap())
            );
        }
    }
}

fn check_oracle_samples() {
    let id = InstrumentId::from("0xm-11.POLYMARKET");

    let state = |epoch, timestamp, size| WireView {
        epoch,
        sequence: 0,
        timestamp,
        book: WireBook {
            bids: [(Decimal::new(172, 3), Decimal::from(size))].into(),
            asks: Default::default(),
        },
    };

    let mut venue = Polymarket::default();
    venue.queue_oracle(id, false, Some(("partial".to_string(), state(1, 10, 135))));
    venue.queue_oracle(id, true, Some(("baseline".to_string(), state(2, 11, 349))));
    assert_eq!(
        venue.oracle_pending,
        VecDeque::from([("baseline".to_string(), state(2, 11, 349))])
    );
    assert!(venue.oracle_update.is_empty());

    venue.queue_oracle(id, false, Some(("complete".to_string(), state(2, 12, 401))));
    venue.queue_oracle(id, false, Some(("next".to_string(), state(2, 13, 402))));
    assert_eq!(
        venue.oracle_pending,
        VecDeque::from([
            ("baseline".to_string(), state(2, 11, 349)),
            ("complete".to_string(), state(2, 12, 401)),
        ])
    );
    assert_eq!(
        venue.oracle_update,
        HashMap::from([(id, ("next".to_string(), state(2, 13, 402)))])
    );

    venue.queue_oracle(id, true, None);
    assert!(venue.oracle_update.is_empty());
    assert_eq!(
        venue.oracle_pending,
        VecDeque::from([
            ("baseline".to_string(), state(2, 11, 349)),
            ("complete".to_string(), state(2, 12, 401)),
        ])
    );

    let wire = PolymarketWire::default();
    let route = &venue.routes()[0];
    let mut busy = wire.open(route, 1);
    let mut quiet = wire.open(route, 2);
    let text = |value: Value| Message::Text(value.to_string().into());
    busy.client(&text(json!({"assets_ids": ["11"]})));
    quiet.client(&text(json!({"assets_ids": ["22"]})));
    busy.upstream(&text(json!({
        "event_type": "book", "asset_id": "11", "timestamp": "1", "hash": "baseline",
        "bids": [{"price": "0.40", "size": "9"}], "asks": [],
    })));
    quiet.upstream(&text(json!({
        "event_type": "book", "asset_id": "22", "timestamp": "1", "hash": "quiet",
        "bids": [{"price": "0.40", "size": "7"}], "asks": [],
    })));

    for index in 0..=ORACLE_SAMPLES_MAX {
        busy.upstream(&text(json!({
            "event_type": "price_change", "timestamp": (index + 2).to_string(),
            "price_changes": [{
                "asset_id": "11", "price": "0.40", "size": (index + 1).to_string(),
                "side": "BUY", "hash": index.to_string(),
            }],
        })));
    }

    assert_eq!(
        wire.view("11:book", 0, 1_000_000),
        Some(SourceView {
            snapshot: true,
            hash: Some("baseline".to_string()),
            comparable: true,
            view: WireView {
                epoch: 1,
                sequence: 0,
                timestamp: 1_000_000,
                book: WireBook {
                    bids: [(Decimal::new(40, 2), Decimal::from(9))].into(),
                    asks: Default::default(),
                },
            },
        })
    );
    assert_eq!(wire.history.lock().len(), 3);
    assert_eq!(wire.history.lock()["11"].len(), ORACLE_SAMPLES_MAX);
    assert_eq!(wire.history.lock()["11:book"].len(), 1);
    assert_eq!(wire.history.lock()["22:book"].len(), 1);
    assert_eq!(wire.view("11", 0, 2_000_000), None);
    assert_eq!(wire.view_hashed("11:0", 2_000_000), None);
    let latest = (ORACLE_SAMPLES_MAX as u64 + 2) * 1_000_000;
    assert_eq!(
        wire.view("11", 0, latest).map(|sample| (
            sample.hash,
            sample.comparable,
            sample.view.epoch
        )),
        Some((Some(ORACLE_SAMPLES_MAX.to_string()), true, 1))
    );
    assert_eq!(
        wire.view_hashed("22:quiet", 1_000_000),
        Some(WireView {
            epoch: 2,
            sequence: 0,
            timestamp: 1_000_000,
            book: WireBook {
                bids: [(Decimal::new(40, 2), Decimal::from(7))].into(),
                asks: Default::default(),
            },
        })
    );
}

fn check_lagging_dump() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../test_data/ws_market_book_integrity.json"
    ))
    .unwrap();
    let captured = &fixture["lagging_dump"];
    let snapshot = &captured["snapshot"];
    let token = snapshot["asset_id"].as_str().unwrap();
    let wire = PolymarketWire::default();

    let mut connection = PolymarketConnection {
        wire: wire.clone(),
        epoch: 1,
        tokens: HashSet::from([token.to_string()]),
        books: HashMap::new(),
        position: (0, false),
    };

    connection.apply(snapshot);
    connection.apply(&captured["newer_update"]);
    let expected = connection.books[token].clone();
    assert_eq!(expected.asks[&Decimal::new(7, 2)], Decimal::new(6991471, 1));
    let timestamp = snapshot["timestamp"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap()
        * 1_000_000;
    let snapshot_view = wire.view(&view_key(token, true), 0, timestamp).unwrap();
    connection.apply(snapshot);
    assert_eq!(connection.books[token], expected);
    assert_eq!(
        wire.events.lock()[&(view_key(token, true), timestamp, 1)],
        2
    );
    assert_eq!(
        wire.view(&view_key(token, true), 0, timestamp),
        Some(snapshot_view)
    );
    assert_eq!(wire.history.lock()[&view_key(token, true)].len(), 1);
    let following = captured["following_updates"].as_array().unwrap();
    assert_eq!(following.len(), 2);

    for (event, size) in following.iter().zip([8833274, 8992774]) {
        connection.apply(event);
        let timestamp = event["timestamp"].as_str().unwrap().parse::<u64>().unwrap() * 1_000_000;
        let mut book = expected.top(DEPTH);
        book.bids.insert(Decimal::new(5, 2), Decimal::new(size, 2));
        let hash = event["price_changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|change| change["asset_id"] == token)
            .unwrap()["hash"]
            .as_str()
            .unwrap();
        assert_eq!(
            wire.view(token, 0, timestamp),
            Some(SourceView {
                snapshot: false,
                hash: Some(hash.to_string()),
                comparable: false,
                view: WireView {
                    epoch: 1,
                    sequence: 0,
                    timestamp,
                    book
                },
            })
        );
    }
}

// Proves the wire oracle before any venue traffic, since a wrong oracle would pass a wrong book
fn check_wire_oracle() {
    let wire = PolymarketWire::default();

    let route = Route {
        name: MARKET,
        path: "/ws/market",
        upstream: String::new(),
        endpoint: SHARD_ENDPOINTS[0],
        headers: &[],
    };

    let mut connection = wire.open(&route, 2);
    let text = |value: Value| Message::Text(value.to_string().into());

    let dump = json!({
        "event_type": "book",
        "market": "0xm",
        "asset_id": "11",
        "timestamp": "5",
        "hash": "abc",
        "tick_size": "0.01",
        "last_trade_price": "0.50",
        "bids": [{"price": "0.40", "size": "2"}, {"price": "0.39", "size": "3"}],
        "asks": [{"price": "0.60", "size": "4"}],
    });

    let change = |timestamp: &str, price: &str, side: &str, size: &str| {
        json!({
            "event_type": "price_change",
            "market": "0xm",
            "timestamp": timestamp,
            "price_changes": [
                {"asset_id": "22", "price": "0.1", "side": "BUY", "size": "9", "hash": "h"},
                {"asset_id": "11", "price": price, "side": side, "size": size, "hash": "h"},
            ],
        })
    };

    let traded = json!({
        "event_type": "book",
        "market": "0xm",
        "asset_id": "11",
        "timestamp": "6",
        "hash": "def",
        "bids": [{"price": "0.45", "size": "1"}],
        "asks": [],
    });

    let mut snapshot = text(json!([dump, change("6", "0.40", "BUY", "0")]));

    let book = |kind| Upstream::Book {
        key: "11".to_string(),
        kind,
    };

    let initial = text(json!({"assets_ids": ["11"], "type": "market", "initial_dump": true}));
    let subscribe = text(json!({"assets_ids": ["11"], "operation": "subscribe"}));
    let unsubscribe = text(json!({"assets_ids": ["11"], "operation": "unsubscribe"}));

    // Frames before the adapter subscribes a token carry no book
    assert_eq!(connection.upstream(&snapshot), Upstream::Other);
    assert!(connection.client(&initial).is_empty());

    assert_eq!(
        connection.upstream(&snapshot.clone()),
        book(FrameKind::Snapshot)
    );
    assert_eq!(
        connection.upstream(&text(change("6", "0.61", "SELL", "5"))),
        book(FrameKind::Update)
    );
    assert_eq!(
        connection.upstream(&text(traded)),
        book(FrameKind::Snapshot)
    );
    assert_eq!(
        connection.upstream(&text(json!({
            "event_type": "price_change",
            "market": "0xm",
            "timestamp": "7",
            "price_changes": [
                {"asset_id": "11", "price": "0.45", "side": "BUY", "size": "0", "hash": "h"},
                {"asset_id": "22", "price": "0.2", "side": "SELL", "size": "3", "hash": "h"},
                {"asset_id": "11", "price": "0.62", "side": "SELL", "size": "7", "hash": "h"},
            ],
        }))),
        book(FrameKind::Update)
    );
    assert_eq!(
        connection.upstream(&Message::Text("PONG".into())),
        Upstream::Other
    );
    assert_eq!(
        connection.upstream(&text(
            json!({"event_type": "last_trade_price", "asset_id": "11"})
        )),
        Upstream::Other
    );

    assert_eq!(connection.client(&unsubscribe), ["11"]);
    assert!(connection.client(&subscribe).is_empty());
    assert_eq!(wire.unsubscribes.lock()["11"], 1);

    let level = |price: &str, size: &str| {
        (
            Decimal::from_str(price).unwrap(),
            Decimal::from_str(size).unwrap(),
        )
    };

    let view = |timestamp, bids: Vec<(Decimal, Decimal)>, asks: Vec<(Decimal, Decimal)>| {
        Some(WireView {
            epoch: 2,
            sequence: 0,
            timestamp,
            book: WireBook {
                bids: bids.into_iter().collect(),
                asks: asks.into_iter().collect(),
            },
        })
    };

    assert_eq!(
        wire.views.find("11:book", 0, 5_000_000),
        view(
            5_000_000,
            vec![level("0.40", "2"), level("0.39", "3")],
            vec![level("0.60", "4")]
        )
    );
    assert_eq!(
        wire.views.find("11", 0, 6_000_000),
        view(
            6_000_000,
            vec![level("0.39", "3")],
            vec![level("0.60", "4"), level("0.61", "5")]
        )
    );
    assert_eq!(
        wire.views.find("11:book", 0, 6_000_000),
        view(6_000_000, vec![level("0.45", "1")], vec![])
    );
    assert_eq!(
        wire.views.find("11", 0, 7_000_000),
        view(7_000_000, vec![], vec![level("0.62", "7")])
    );
    assert_eq!(wire.views.find("11", 0, 5_000_000), None);
    assert_eq!(wire.views.find("22", 0, 6_000_000), None);
    assert_eq!(
        *wire.events.lock(),
        HashMap::from([
            (("11:book".to_string(), 5_000_000, 2), 1),
            (("11".to_string(), 6_000_000, 2), 2),
            (("11:book".to_string(), 6_000_000, 2), 1),
            (("11".to_string(), 7_000_000, 2), 1),
        ])
    );
    assert_eq!(wire.updates_received("11", None), 3);
    assert_eq!(wire.updates_received("11", Some(5_000_000)), 3);
    assert_eq!(wire.updates_received("11", Some(6_000_000)), 1);
    assert_eq!(wire.updates_received("11", Some(7_000_000)), 0);

    let mut update = text(change("9", "0.40", "BUY", "1"));
    let unchanged = update.clone();
    assert!(!connection.corrupt(&mut update, "11", FrameKind::Update));
    assert_eq!(update, unchanged);
    assert!(connection.corrupt(&mut snapshot, "11", FrameKind::Snapshot));
    let corrupted = serde_json::from_str::<Value>(snapshot.to_text().unwrap()).unwrap();
    assert_eq!(
        corrupted[0]["hash"],
        json!("0000000000000000000000000000000000000000")
    );
    assert_eq!(corrupted[1], change("6", "0.40", "BUY", "0"));

    assert!(
        wire.ambiguous
            .lock()
            .contains(&("11".to_string(), 6_000_000))
    );
    assert!(
        !wire
            .ambiguous
            .lock()
            .contains(&("11:book".to_string(), 5_000_000))
    );
    let mut replay = wire.open(&route, 3);
    replay.client(&initial);
    replay.upstream(&snapshot);
    replay.upstream(&snapshot);
    assert!(
        !wire
            .ambiguous
            .lock()
            .contains(&("11:book".to_string(), 5_000_000))
    );
    assert_eq!(wire.views.find("11:book", 0, 5_000_000).unwrap().epoch, 3);
    assert_eq!(wire.view("11", 0, 6_000_000), None);
    assert_eq!(
        wire.view("11:book", 0, 5_000_000).map(|sample| sample.view),
        wire.views.find("11:book", 0, 5_000_000)
    );

    let mut first_snapshot = corrupted[0].clone();
    first_snapshot["timestamp"] = json!("20");
    first_snapshot["hash"] = json!("first-snapshot");
    replay.upstream(&text(first_snapshot.clone()));
    let admitted = wire.view("11:book", 0, 20_000_000).unwrap();
    let mut second_snapshot = first_snapshot;
    second_snapshot["hash"] = json!("second-snapshot");
    second_snapshot["bids"][0]["size"] = json!("19");
    let samples = wire.history.lock();

    let writer = std::thread::spawn(move || {
        replay.upstream(&Message::Text(second_snapshot.to_string().into()));
        replay
    });

    let started = Instant::now();

    while wire
        .events
        .lock()
        .get(&("11:book".to_string(), 20_000_000, 3))
        != Some(&2)
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }

    let observed = Instant::now();
    while observed.elapsed() < Duration::from_millis(100) {
        assert_eq!(
            wire.views.find("11:book", 0, 20_000_000),
            Some(admitted.view.clone())
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    drop(samples);
    let mut replay = writer.join().expect("source publisher finishes");
    assert_eq!(
        admitted,
        SourceView {
            snapshot: true,
            hash: Some("first-snapshot".to_string()),
            comparable: true,
            view: WireView {
                epoch: 3,
                sequence: 0,
                timestamp: 20_000_000,
                book: WireBook {
                    bids: [level("0.40", "2"), level("0.39", "3")].into(),
                    asks: [level("0.60", "4")].into(),
                },
            },
        }
    );
    assert_eq!(wire.view("11:book", 0, 20_000_000), None);

    let mut unhashed = corrupted[0].clone();
    unhashed["timestamp"] = json!("21");
    unhashed["bids"][0]["size"] = json!("23");
    unhashed.as_object_mut().unwrap().remove("hash");
    replay.upstream(&text(unhashed));
    assert_eq!(
        wire.view("11:book", 0, 21_000_000),
        Some(SourceView {
            snapshot: true,
            hash: None,
            comparable: true,
            view: WireView {
                epoch: 3,
                sequence: 0,
                timestamp: 21_000_000,
                book: WireBook {
                    bids: [level("0.40", "23"), level("0.39", "3")].into(),
                    asks: [level("0.60", "4")].into(),
                },
            },
        })
    );

    // Gated updates can precede an older initial snapshot without any emitted regression
    let mut gated = wire.open(&route, 4);
    gated.client(&initial);
    gated.upstream(&text(change("16", "0.40", "BUY", "8")));
    let mut delayed = corrupted[0].clone();
    delayed["timestamp"] = json!("15");
    gated.upstream(&text(delayed.clone()));
    gated.upstream(&text(change("17", "0.61", "SELL", "9")));
    assert_eq!(
        wire.view("11", 0, 17_000_000).map(|sample| (
            sample.hash,
            sample.comparable,
            sample.view.epoch
        )),
        Some((Some("h".to_string()), false, 4))
    );
    delayed["timestamp"] = json!("18");
    gated.upstream(&text(delayed));
    gated.upstream(&text(change("19", "0.61", "SELL", "10")));
    assert_eq!(
        wire.view("11", 0, 19_000_000).map(|sample| (
            sample.hash,
            sample.comparable,
            sample.view.epoch
        )),
        Some((Some("h".to_string()), true, 4))
    );

    let mut first = change("12", "0.40", "BUY", "2");
    first["price_changes"][1]["hash"] = json!("first");
    let mut second = change("12", "0.41", "BUY", "3");
    second["price_changes"][1]["hash"] = json!("second");
    connection.upstream(&text(first));
    connection.upstream(&text(second));
    assert_eq!(
        wire.view_hashed("11:first", 12_000_000),
        view(
            12_000_000,
            vec![level("0.40", "2")],
            vec![level("0.62", "7")],
        )
    );
    assert_eq!(
        wire.view_hashed("11:second", 12_000_000),
        view(
            12_000_000,
            vec![level("0.40", "2"), level("0.41", "3")],
            vec![level("0.62", "7")],
        )
    );
}

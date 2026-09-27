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
//!
//! The harness subscribes one outcome token of each of the most traded open markets, one per
//! socket, so every frame on a socket belongs to one book. No orders are submitted. Every emitted
//! batch passes through the shared `BookStreamChecker` and is verified against a reference book
//! rebuilt from the raw frames the proxy relays; a session fails unless every snapshot episode was
//! verified.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    net::SocketAddr,
    rc::Rc,
    str::FromStr,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
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
    enums::BookAction,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::{mode::ReconnectRequestOutcome, retry::RetryConfig};
use nautilus_polymarket::{
    common::consts::POLYMARKET_CLIENT_ID,
    config::{PolymarketDataClientConfig, PolymarketInstrumentProviderConfig},
    factories::PolymarketDataClientFactory,
    http::{gamma::PolymarketGammaHttpClient, query::GetGammaMarketsParams},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, FrameKind, Route, Session, StressArgs, StressVenue, Upstream, WireBook,
    WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const BOOKS: usize = 6;

const MARKET: &str = "market";
// One shard per book: the primary endpoint, then one per secondary shard ID
const SHARD_ENDPOINTS: [&str; BOOKS] = [
    "polymarket-market-streams",
    "polymarket-market-streams-1",
    "polymarket-market-streams-2",
    "polymarket-market-streams-3",
    "polymarket-market-streams-4",
    "polymarket-market-streams-5",
];
const DEPTH: usize = 20;
// Replacement attempts in the shared retry budget, including the first
const BUDGET: usize = 8;

// Token IDs by instrument, selected once before any session connects
static TOKENS: OnceLock<HashMap<InstrumentId, String>> = OnceLock::new();

type PolymarketSession = Session<Polymarket>;

fn main() {
    stress::run::<Polymarket, _, _>(|args| async move {
        let tokens = select_markets().await;
        let ids = tokens.keys().copied().collect::<Vec<_>>();
        TOKENS.set(tokens).expect("markets select once");

        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
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
            // A book dump whose hash fails verification starts recovery
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
                assert!(
                    session.venue().attempts(&id) > attempts,
                    "{id} accepted a snapshot whose hash fails verification"
                );
            }
            // An update before any snapshot starts recovery
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
                let attempts = start_held_recovery(&mut session, id).await;
                session
                    .until(Duration::from_secs(60), "replacement attempted", |s| {
                        s.venue().attempts(&id) > attempts
                    })
                    .await;

                session.fault(&id).hold = false;
                reconnect_all(&mut session);
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
                let dropped = {
                    let mut fault = session.fault(&id);
                    fault.drop_snapshots = 2;
                    fault.dropped
                };

                resubscribe(&mut session, id).await;
                session
                    .until(
                        Duration::from_secs(60),
                        "shutdown with recovery dumps missing",
                        |s| s.fault(&id).dropped == dropped + 2,
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
                "phase={phase} instrument={id} phase_ms={} batches_total={}",
                phase_started.elapsed().as_millis(),
                total + session.batches()
            ),
        );
    }

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
    session.subscribe(id);
    session.healthy(&[id]).await;
    stress::check("unsubscribe", format!("instrument={id}"));

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

// Breaks the hash of a resubscribed book's snapshot so recovery starts at once, then holds every
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
// reports the primary endpoint, so the first request expects a snapshot from all of them
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

// Selects the first outcome token of the most traded open markets with an order book
async fn select_markets() -> HashMap<InstrumentId, String> {
    let client = PolymarketGammaHttpClient::new(None, 30, RetryConfig::default())
        .expect("Gamma client builds");

    let params = GetGammaMarketsParams {
        active: Some(true),
        closed: Some(false),
        order: Some("volume24hr".to_string()),
        ascending: Some(false),
        max_markets: Some(50),
        ..Default::default()
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
            let token = tokens.into_iter().next()?;
            let id = InstrumentId::from(format!("{}-{token}.POLYMARKET", market.condition_id));
            Some((id, token))
        })
        .take(BOOKS)
        .collect::<HashMap<_, _>>();

    assert_eq!(tokens.len(), BOOKS, "Gamma lists {BOOKS} open order books");
    tokens
}

struct Polymarket {
    wire: PolymarketWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
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
}

impl StressVenue for Polymarket {
    const NAME: &'static str = "polymarket";
    const SCENARIOS: &'static [&'static str] = &["churn", "boundaries"];
    const ROUNDS: usize = 12;
    // Polymarket books carry no sequence, so the wire oracle verifies content
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(120);

    fn self_check() {
        check_wire_oracle();
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: PolymarketWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
            updates: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *POLYMARKET_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: MARKET,
            path: "/ws/market",
            upstream: "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_string(),
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
                load_ids: Some(tokens.keys().copied().collect()),
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

        let key = view_key(&self.key(&id), snapshot);
        let timestamp = deltas.ts_event.as_u64();

        let events = self
            .wire
            .events
            .lock()
            .get(&(key.clone(), timestamp))
            .copied()
            .unwrap_or(0);

        // A timestamp two events of the same kind share cannot be aligned, so the sample is skipped
        if events > 1 {
            return;
        }

        // A view evicted while events were held cannot be compared, so the sample is skipped
        let Some(view) = self.wire.views.find(&key, deltas.sequence, timestamp) else {
            assert_eq!(
                events, 1,
                "wire oracle has no {key} event at emitted ts={}",
                deltas.ts_event
            );
            return;
        };

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            panic!(
                "wire oracle mismatch {id} ts={}: {violation}",
                deltas.ts_event
            );
        }

        self.epochs.insert(id, view.epoch);
    }

    // A healthy book resynced on a connection at least as new as expected and streams again;
    // some markets send only `book` events, so the batches can all be snapshots
    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches >= start.batches + 2
    }

    // Snapshots alone can satisfy every recovery wait, so a book whose venue sent incremental
    // events must also have emitted incremental updates. Events are counted before faults apply,
    // so a book whose only incremental events were held fails this check
    fn finish(&mut self) {
        let events = self.wire.events.lock();

        for (id, token) in TOKENS.get().expect("markets select before sessions") {
            let sent = events
                .iter()
                .filter(|((key, _), _)| key == token)
                .map(|(_, count)| count)
                .sum::<usize>();
            let emitted = self.updates.get(id).copied().unwrap_or(0);

            assert!(
                sent < 10 || emitted > 0,
                "{id} emitted no incremental updates although the venue sent {sent}"
            );
        }
    }
}

#[derive(Clone, Default)]
struct PolymarketWire {
    views: WireViews,
    // Recorded events by view key and timestamp, which tell an ambiguous or evicted view from a
    // missing one
    events: Arc<Mutex<HashMap<(String, u64), usize>>>,
    unsubscribes: Arc<Mutex<HashMap<String, usize>>>,
}

impl WireCodec for PolymarketWire {
    fn open(&self, _route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(PolymarketConnection {
            wire: self.clone(),
            epoch: number,
            tokens: HashSet::new(),
            books: HashMap::new(),
        })
    }
}

struct PolymarketConnection {
    wire: PolymarketWire,
    epoch: usize,
    tokens: HashSet<String>,
    books: HashMap<String, WireBook>,
}

impl PolymarketConnection {
    // Applies one market event to the reference book of a subscribed token, returning the token
    // and whether the event was a hash-verifiable dump
    fn apply(&mut self, event: &Value) -> Option<(String, bool)> {
        let timestamp = event["timestamp"].as_str()?.parse::<u64>().ok()? * 1_000_000;

        let (key, snapshot, dump) = match event["event_type"].as_str()? {
            "book" => {
                let key = self.subscribed(event["asset_id"].as_str()?)?;
                let book = self.books.entry(key.clone()).or_default();
                *book = WireBook::default();
                book.apply(&levels(&event["bids"]), &levels(&event["asks"]));
                (key, true, is_dump(event))
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

                (key, false, false)
            }
            _ => return None,
        };

        let view = view_key(&key, snapshot);

        *self
            .wire
            .events
            .lock()
            .entry((view.clone(), timestamp))
            .or_default() += 1;

        self.wire.views.record(
            &view,
            WireView {
                epoch: self.epoch,
                sequence: 0,
                timestamp,
                book: self.books[&key].top(DEPTH),
            },
        );

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
            if let Some((key, dump)) = self.apply(event) {
                let kind = book.as_ref().map_or(FrameKind::Update, |(_, kind)| *kind);

                let kind = if dump { FrameKind::Snapshot } else { kind };
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
    assert_eq!(connection.upstream(&text(traded)), book(FrameKind::Update));
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
            (("11:book".to_string(), 5_000_000), 1),
            (("11".to_string(), 6_000_000), 2),
            (("11:book".to_string(), 6_000_000), 1),
            (("11".to_string(), 7_000_000), 1),
        ])
    );

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
}

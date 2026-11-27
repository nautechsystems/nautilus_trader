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

//! Mainnet L2 book recovery conformance for Kraken Spot and Futures.

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
use nautilus_common::clients::DataClient;
use nautilus_kraken::{
    KrakenDataClientConfig, KrakenFuturesDataClient, KrakenSpotDataClient,
    common::{consts::KRAKEN_CLIENT_ID, enums::KrakenProductType},
};
use nautilus_live::book::{DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS, conformance::BookStreamChecker};
use nautilus_model::{
    data::OrderBookDeltas,
    enums::RecordFlag,
    identifiers::{ClientId, InstrumentId},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, Flag, FrameKind, Route, Session, StressArgs, StressVenue, Upstream,
    WireBook, WireCodec, WireConnection, WireView,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const ROUTE: &str = "book";
const DEPTH: usize = 10;
const VIEWS_MAX: usize = 16_384;
const STEP_LIMIT: Duration = Duration::from_secs(35);

type Views = Arc<Mutex<HashMap<String, BookHistory>>>;

struct ObservedBook {
    view: WireView,
    recent: [Option<UpdatePosition>; 4],
}

struct PendingEpisode {
    deadline: Instant,
    samples: VecDeque<ObservedBook>,
}

#[derive(Default)]
struct PendingEpisodes {
    current: Option<PendingEpisode>,
    previous: Vec<PendingEpisode>,
}

#[derive(Default)]
struct BookHistory {
    arrivals: VecDeque<(u64, Instant)>,
    books: HashMap<u64, VecDeque<ObservedBook>>,
}

impl BookHistory {
    fn record(&mut self, observed: ObservedBook, now: Instant, min_age: Duration) {
        let timestamp = observed.view.timestamp;
        self.arrivals.push_back((timestamp, now));
        self.books.entry(timestamp).or_default().push_back(observed);

        while self.arrivals.len() > VIEWS_MAX
            && self
                .arrivals
                .front()
                .is_some_and(|&(_, arrival)| now.duration_since(arrival) >= min_age)
        {
            let (oldest, _) = self.arrivals.pop_front().expect("retained timestamp");
            let books = self.books.get_mut(&oldest).expect("retained books");
            books.pop_front();
            if books.is_empty() {
                self.books.remove(&oldest);
            }
        }
    }

    fn at(&self, timestamp: u64) -> impl Iterator<Item = &ObservedBook> {
        self.books.get(&timestamp).into_iter().flatten().rev()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UpdateIdentity {
    is_bid: bool,
    price: Decimal,
    qty: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UpdatePosition {
    timestamp: u64,
    update: UpdateIdentity,
}

fn main() {
    stress::run::<Kraken, _, _>(|args| async move {
        let product = product(&args);
        let symbols = symbols(product);
        let ids = symbols.map(|symbol| InstrumentId::from(format!("{symbol}.KRAKEN").as_str()));
        let mut session = Session::<Kraken>::connect(&args).await;
        subscribe_initial(&mut session, &ids, product, args.scenario()).await;
        match args.scenario() {
            "boundaries" => {
                boundaries(
                    &mut session,
                    &ids,
                    product == KrakenProductType::Spot,
                    args.timeout_secs(),
                )
                .await
            }
            "held" => held_replacement(&mut session, &ids, product).await,
            "churn" => {
                for round in 0..args.rounds() {
                    churn_round(&mut session, &ids, product, round).await;
                    session.observe(Duration::from_secs(12)).await;
                    session.round(round, "books=2");
                }
            }
            "initial" | "disabled" => {}
            other => unreachable!("Unexpected scenario {other}"),
        }

        let verified = |s: &Session<Kraken>| {
            s.venue().comparisons > 0 && s.venue().independent_pending.is_empty()
        };

        let deadline = Instant::now() + STEP_LIMIT;

        loop {
            session
                .until(
                    deadline.saturating_duration_since(Instant::now()),
                    "independent book comparisons",
                    verified,
                )
                .await;

            if verified(&session) {
                break;
            }
        }

        session.stop().await
    });
}

async fn subscribe_initial(
    session: &mut Session<Kraken>,
    ids: &[InstrumentId],
    product: KrakenProductType,
    scenario: &str,
) {
    for &id in ids {
        if matches!(scenario, "initial" | "disabled") {
            let mut fault = session.fault(&id);
            fault.drop_snapshots = 1;
            if scenario == "disabled" {
                fault.drop_updates = usize::MAX;
            }
        }

        session.subscribe(id);
    }

    if scenario == "disabled" {
        session
            .until(STEP_LIMIT, "initial snapshots dropped", |s| {
                ids.iter().all(|id| s.fault(id).drop_snapshots == 0)
            })
            .await;

        session
            .observe(Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS + 2))
            .await;

        for id in ids {
            assert_eq!(session.book(id).batches, 0);
            session.fault(id).drop_updates = 0;
        }

        assert_eq!(
            session.venue().wire.book_commands.load(Ordering::Relaxed),
            ids.len()
        );
        assert_eq!(session.proxy().connections(ROUTE), 1);
        session.reconnect(endpoint(product));
    }

    session.healthy(ids).await;
}

async fn churn_round(
    session: &mut Session<Kraken>,
    ids: &[InstrumentId],
    product: KrakenProductType,
    round: usize,
) {
    let id = ids[round % ids.len()];
    match round % 6 {
        0 => {
            session.expect_snapshot(id);
            let mut fault = session.fault(&id);
            match product {
                KrakenProductType::Spot => fault.corrupt = 1,
                KrakenProductType::Futures => fault.drop_updates = 1,
            }
        }
        1 => {
            session.fault(&id).drop_snapshots = 1;
            session.reconnect(endpoint(product));
        }
        2 => reconnect_during_recovery(session, id, product).await,
        3 => turnover(session, id).await,
        4 => {
            session.proxy().cut(Some(ROUTE), FrameKind::Snapshot, 1);
            session.reconnect(endpoint(product));
        }
        _ => {
            session.proxy().freeze(Duration::from_secs(3));
            session.reconnect(endpoint(product));
        }
    }

    session.healthy(ids).await;
}

async fn reconnect_during_recovery(
    session: &mut Session<Kraken>,
    id: InstrumentId,
    product: KrakenProductType,
) {
    session.expect_snapshot(id);

    let dropped = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.drop_snapshots = usize::MAX;
        fault.dropped
    };

    session
        .until(STEP_LIMIT, "replacement snapshot dropped", |s| {
            s.fault(&id).dropped > dropped
        })
        .await;

    session.reconnect(endpoint(product));
    session.fault(&id).drop_snapshots = 0;
}

async fn turnover(session: &mut Session<Kraken>, id: InstrumentId) {
    let before = session.fault(&id).unsubscribes;
    session.unsubscribe(id);
    session
        .until(STEP_LIMIT, "unsubscribe acknowledged", |s| {
            s.fault(&id).unsubscribes > before
        })
        .await;

    session.close(id);
    session.observe(Duration::from_millis(250)).await;
    session.subscribe(id);
}

async fn held_replacement(
    session: &mut Session<Kraken>,
    ids: &[InstrumentId],
    product: KrakenProductType,
) {
    let id = ids[0];
    session.expect_snapshot(id);

    {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.hold_snapshot_after_corrupt = true;
    }

    session
        .until(STEP_LIMIT, "replacement snapshot held", |s| {
            s.fault(&id).held > 0
        })
        .await;

    let before = session.book(&id);
    let unsubs = session.fault(&id).unsubscribes;
    session.unsubscribe(id);
    session
        .until(STEP_LIMIT, "unsubscribe during recovery", |s| {
            s.fault(&id).unsubscribes > unsubs
        })
        .await;

    session.close(id);
    session.fault(&id).hold = false;
    session.proxy().release();
    session.observe(Duration::from_secs(2)).await;
    assert_eq!(session.book(&id), before);
    session.subscribe(id);
    session.healthy(ids).await;

    reconnect_during_recovery(session, id, product).await;
    session.healthy(ids).await;
}

async fn boundaries(
    session: &mut Session<Kraken>,
    ids: &[InstrumentId],
    spot: bool,
    timeout_secs: u64,
) {
    session.observe(Duration::from_secs(3)).await;
    let id = ids[0];
    session.expect_snapshot(id);
    let previous = session.book(&id).snapshots;
    // Four attempts precede reconnect. Fail nine replacement snapshots in
    // total, plus the automatic replay, so the next retry must wait at the ceiling.
    let mut fault = session.fault(&id);
    let base = if spot {
        fault.reject = 10;
        fault.corrupt = 1;
        fault.rejected
    } else {
        fault.drop_snapshots = 10;
        fault.drop_updates = 1;
        fault.dropped + 1
    };

    drop(fault);

    let failures = |s: &Session<Kraken>| {
        let fault = s.fault(&id);
        (if spot { fault.rejected } else { fault.dropped }).saturating_sub(base)
    };

    session
        .until(Duration::from_secs(90), "four failed replacements", |s| {
            failures(s) == 4
        })
        .await;

    let connections = session.proxy().connections(ROUTE);
    session.reconnect(endpoint(if spot {
        KrakenProductType::Spot
    } else {
        KrakenProductType::Futures
    }));

    session
        .until(STEP_LIMIT, "reconnect with retry budget spent", |s| {
            s.proxy().connections(ROUTE) > connections && failures(s) >= 5
        })
        .await;

    session
        .until(
            Duration::from_secs(180),
            "replacement failures before and after reconnect",
            |s| failures(s) == 10,
        )
        .await;

    session.fault(&id).hold_snapshot = true;
    let quiet_commands = session.venue().wire.book_commands.load(Ordering::Relaxed);
    // The reconnect wakes the first ceiling retry. The next delay is 120s plus jitter.
    session.observe(Duration::from_secs(100)).await;
    assert_eq!(
        session.venue().wire.book_commands.load(Ordering::Relaxed),
        quiet_commands,
        "recoverer honors ceiling backoff"
    );
    assert_eq!(session.book(&id).snapshots, previous);
    session
        .until(
            STEP_LIMIT,
            "replacement snapshot held at the retry ceiling",
            |s| s.fault(&id).held > 0,
        )
        .await;
    let commands = session.venue().wire.book_commands.load(Ordering::Relaxed);
    session
        .observe(Duration::from_secs(timeout_secs).saturating_add(Duration::from_secs(2)))
        .await;
    session.fault(&id).hold = false;
    session.proxy().release();
    session.healthy_within(ids, STEP_LIMIT).await;
    assert_eq!(
        session.venue().wire.book_commands.load(Ordering::Relaxed),
        commands
    );
    assert_eq!(session.proxy().connections(ROUTE), connections + 1);
    stress::check(
        "retry_ceiling",
        "reconnect preserved the retry budget and the late snapshot ended ceiling backoff",
    );
}

struct Kraken {
    product: KrakenProductType,
    wire: Arc<KrakenWire>,
    independent: Views,
    oracle: tokio::task::JoinHandle<()>,
    futures_sequences: HashMap<InstrumentId, u64>,
    comparisons: usize,
    independent_pending: HashMap<InstrumentId, PendingEpisodes>,
    independent_verified: usize,
}

impl StressVenue for Kraken {
    const NAME: &'static str = "kraken";
    const SCENARIOS: &'static [&'static str] =
        &["churn", "initial", "disabled", "boundaries", "held"];
    const ROUNDS: usize = 60;
    const FLAGS: &'static [Flag] = &[Flag {
        name: "product",
        default: "spot",
        help: "Mainnet product: spot or futures",
    }];
    const SEQUENCED: bool = true;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = STEP_LIMIT;

    fn self_check() {
        let view = WireBook::default();
        assert!(view.top(DEPTH).bids.is_empty() && view.top(DEPTH).asks.is_empty());
        let mut spot: Value =
            serde_json::from_str(include_str!("../../test_data/ws_book_snapshot.json")).unwrap();
        let mut second = spot["data"][0].clone();
        second["symbol"] = json!("ETH/USD");
        spot["data"].as_array_mut().unwrap().push(second);

        let mut relay = KrakenConnection {
            product: KrakenProductType::Spot,
            epoch: 0,
            history_min_age: STEP_LIMIT,
            views: Views::default(),
            book_commands: Arc::default(),
            books: HashMap::new(),
            positions: HashMap::new(),
        };

        assert_eq!(
            relay.record_message(&spot),
            Some(("BTC/USD".into(), FrameKind::Snapshot))
        );

        for symbol in ["BTC/USD", "ETH/USD"] {
            assert_eq!(relay.books[symbol].bids.len(), 10);
        }

        let futures = parse_futures_frame(
            &serde_json::from_str(include_str!("../../test_data/ws_futures_book_delta.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(futures.kind, FrameKind::Update);
        assert_eq!(futures.sequence, 326_094_134);

        check_relay_history();
    }

    fn new(args: &StressArgs) -> Self {
        let product = product(args);

        let wire = Arc::new(KrakenWire {
            product,
            views: Views::default(),
            book_commands: Arc::default(),
        });

        let independent = Views::default();
        let oracle = tokio::spawn(independent_feed(product, independent.clone()));

        Self {
            product,
            wire,
            independent,
            oracle,
            futures_sequences: HashMap::new(),
            comparisons: 0,
            independent_pending: HashMap::new(),
            independent_verified: 0,
        }
    }

    fn client_id(&self) -> ClientId {
        *KRAKEN_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: ROUTE,
            path: "/book",
            upstream: url(self.product).into(),
            endpoint: endpoint(self.product),
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        self.wire.clone()
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let config = KrakenDataClientConfig {
            product_type: self.product,
            ws_public_url: Some(format!("ws://{proxy}/book")),
            book_snapshot_timeout_secs: args.timeout_secs(),
            ..Default::default()
        };

        match self.product {
            KrakenProductType::Spot => Ok(Box::new(KrakenSpotDataClient::new(
                *KRAKEN_CLIENT_ID,
                config,
            )?)),
            KrakenProductType::Futures => Ok(Box::new(KrakenFuturesDataClient::new(
                *KRAKEN_CLIENT_ID,
                config,
            )?)),
        }
    }

    fn key(&self, id: &InstrumentId) -> String {
        id.symbol.to_string()
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let snapshot = RecordFlag::F_SNAPSHOT.matches(deltas.flags);
        if self.product == KrakenProductType::Futures {
            let previous = self.futures_sequences.insert(id, deltas.sequence);

            if !snapshot {
                assert_eq!(
                    Some(deltas.sequence),
                    previous.and_then(|sequence| sequence.checked_add(1)),
                    "incremental {id} crossed a sequence gap before a replacement snapshot",
                );
            }
        }

        if snapshot {
            let pending = self.independent_pending.entry(id).or_default();

            let previous = pending.current.replace(PendingEpisode {
                deadline: Instant::now() + STEP_LIMIT,
                samples: VecDeque::new(),
            });

            pending.previous.extend(previous);
        }

        let key = self.key(&id);
        let timestamp = deltas.ts_event.as_u64();

        let (view, recent) = {
            let views = self.wire.views.lock();
            let views = views.get(&key).expect("relayed frames for the instrument");

            let mut candidates = views.at(timestamp).filter(|observed| {
                self.product == KrakenProductType::Spot || observed.view.sequence == deltas.sequence
            });

            let matched = candidates
                .find(|observed| {
                    let view = &observed.view;
                    checker
                        .verify(id, DEPTH, &view.book.bids, &view.book.asks)
                        .is_ok()
                })
                .unwrap_or_else(|| {
                    let retained = views
                        .arrivals
                        .front()
                        .zip(views.arrivals.back())
                        .map(|(&(first, _), &(last, _))| (first, last));

                    let expected = views.at(timestamp)
                        .find(|observed| observed.view.sequence == deltas.sequence)
                        .map(|observed| &observed.view.book);

                    let book = checker.book(id).expect("integrity checked book");

                    let actual = WireBook {
                        bids: book.bids_as_map(Some(DEPTH)).into_iter().collect(),
                        asks: book.asks_as_map(Some(DEPTH)).into_iter().collect(),
                    };

                    panic!(
                        "No matching wire book for {id} at {timestamp}, sequence={}, retained={retained:?}, expected={expected:?}, actual={actual:?}",
                        deltas.sequence,
                    );
                });

            (matched.view.clone(), matched.recent)
        };

        let independent = self.independent.lock();
        if let Some(other) = independent
            .get(&key)
            .and_then(|observed| aligned_book(observed, self.product, &view, recent))
        {
            assert_eq!(
                view.book, other.book,
                "independent book at {id} / {timestamp}"
            );
            self.comparisons += 1;

            if let Some(pending) = self.independent_pending.get_mut(&id) {
                self.independent_verified += usize::from(pending.current.take().is_some());
                if pending.previous.is_empty() {
                    self.independent_pending.remove(&id);
                }
            }
        } else if let Some(episode) = self
            .independent_pending
            .get_mut(&id)
            .and_then(|pending| pending.current.as_mut())
        {
            if episode.samples.len() == VIEWS_MAX {
                let _ = episode.samples.pop_front();
            }

            episode.samples.push_back(ObservedBook { view, recent });
        }
    }

    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        book.batches >= start.batches + 2 && !self.independent_pending.contains_key(id)
    }

    fn poll(&mut self) {
        let independent = self.independent.lock();
        let now = Instant::now();
        self.independent_pending.retain(|id, pending| {
            let observed = independent.get(id.symbol.as_str());

            let mut matched = |episode: &PendingEpisode| {
                let alignment = episode.samples.iter().find_map(|sample| {
                    let other = aligned_book(observed?, self.product, &sample.view, sample.recent)?;
                    Some((sample, other))
                });

                if let Some((sample, other)) = alignment {
                    assert_eq!(
                        sample.view.book, other.book,
                        "independent book at {id} / {}",
                        sample.view.timestamp
                    );
                    self.comparisons += 1;
                    self.independent_verified += 1;
                    return true;
                }

                assert!(
                    now < episode.deadline,
                    "no independent comparison for {id} snapshot"
                );
                false
            };

            pending.previous.retain(|episode| !matched(episode));
            if pending.current.as_ref().is_some_and(&mut matched) {
                pending.current = None;
            }

            pending.current.is_some() || !pending.previous.is_empty()
        });
    }

    fn stats(&self) -> String {
        format!(
            "independent_comparisons={} independent_verified_episodes={}",
            self.comparisons, self.independent_verified
        )
    }

    fn finish(&mut self) {
        self.oracle.abort();
        assert!(self.comparisons > 0, "no aligned independent comparisons");
        assert!(
            self.independent_pending.is_empty(),
            "no independent comparison for {:?}",
            self.independent_pending.keys().collect::<Vec<_>>()
        );
    }
}

fn check_relay_history() {
    let now = Instant::now();
    let timestamp = 1_000_000;

    let observed = |sequence| ObservedBook {
        view: WireView {
            epoch: 1,
            sequence,
            timestamp,
            book: WireBook::default(),
        },
        recent: [None; 4],
    };

    let mut relay = BookHistory::default();
    let mut independent = BookHistory::default();

    for sequence in 0..=VIEWS_MAX as u64 {
        relay.record(observed(sequence), now, STEP_LIMIT);
        independent.record(observed(sequence), now, Duration::ZERO);
    }

    let oldest = |history: &BookHistory| history.at(timestamp).last().unwrap().view.sequence;
    assert_eq!(oldest(&relay), 0);
    assert_eq!(oldest(&independent), 1);

    relay.record(observed(VIEWS_MAX as u64 + 1), now + STEP_LIMIT, STEP_LIMIT);
    assert_eq!(oldest(&relay), 2);
    assert_eq!(relay.arrivals.len(), VIEWS_MAX);
}

fn aligned_book<'a>(
    observed: &'a BookHistory,
    product: KrakenProductType,
    view: &WireView,
    recent: [Option<UpdatePosition>; 4],
) -> Option<&'a WireView> {
    let mut aligned = observed
        .at(view.timestamp)
        .filter(|observation| match product {
            KrakenProductType::Spot => observation.view.sequence == view.sequence,
            KrakenProductType::Futures => recent[0].is_some() && observation.recent == recent,
        });

    let first = aligned.next()?;
    // Several wire events can share one timestamp. Compare only when their resulting books agree.
    aligned
        .all(|other| other.view.book == first.view.book)
        .then_some(&first.view)
}

impl Drop for Kraken {
    fn drop(&mut self) {
        self.oracle.abort();
    }
}

struct KrakenWire {
    product: KrakenProductType,
    views: Views,
    book_commands: Arc<AtomicUsize>,
}

impl WireCodec for KrakenWire {
    fn open(&self, _route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(KrakenConnection {
            product: self.product,
            epoch: number,
            // The proxy can record a burst ahead of the checker, including held frames.
            history_min_age: STEP_LIMIT,
            views: self.views.clone(),
            book_commands: self.book_commands.clone(),
            books: HashMap::new(),
            positions: HashMap::new(),
        })
    }
}

struct KrakenConnection {
    product: KrakenProductType,
    epoch: usize,
    history_min_age: Duration,
    views: Views,
    book_commands: Arc<AtomicUsize>,
    books: HashMap<String, WireBook>,
    positions: HashMap<String, [Option<UpdatePosition>; 4]>,
}

impl WireConnection for KrakenConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Some(value) = message
            .to_text()
            .ok()
            .and_then(|text| serde_json::from_str(text).ok())
        else {
            return Upstream::Other;
        };

        if let Some((key, kind)) = self.record_message(&value) {
            return Upstream::Book { key, kind };
        }

        let unsubscribed = match self.product {
            KrakenProductType::Spot
                if value["method"] == "unsubscribe" && value["success"] == true =>
            {
                value["result"]["symbol"].as_str()
            }
            KrakenProductType::Futures
                if value["event"] == "unsubscribed" && value["feed"] == "book" =>
            {
                value["product_ids"][0].as_str()
            }
            _ => None,
        };

        if let Some(key) = unsubscribed {
            self.books.remove(key);
            self.positions.remove(key);
            return Upstream::Unsubscribed(key.into());
        }

        Upstream::Other
    }

    fn corrupt(&mut self, message: &mut Message, key: &str, kind: FrameKind) -> bool {
        if kind != FrameKind::Update {
            return false;
        }

        let mut value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();

        match self.product {
            KrakenProductType::Spot => {
                let Some(book) = value["data"]
                    .as_array_mut()
                    .and_then(|books| books.iter_mut().find(|book| book["symbol"] == key))
                else {
                    return false;
                };

                let checksum = book["checksum"].as_u64().unwrap();
                book["checksum"] = json!(checksum ^ 1);
            }
            KrakenProductType::Futures => {
                value["seq"] = json!(value["seq"].as_u64().unwrap() + 1);
            }
        }

        *message = Message::Text(value.to_string().into());
        true
    }

    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let value: Value = serde_json::from_str(message.to_text().ok()?).ok()?;
        let (channel, command) = if self.product == KrakenProductType::Spot {
            (&value["params"]["channel"], value["method"].as_str())
        } else {
            (&value["feed"], value["event"].as_str())
        };

        if channel != "book" || !matches!(command, Some("subscribe" | "unsubscribe")) {
            return None;
        }

        self.book_commands.fetch_add(1, Ordering::Relaxed);

        if self.product != KrakenProductType::Spot || command != Some("subscribe") {
            return None;
        }

        let key = value["params"]["symbol"][0].as_str()?;
        let answer = json!({
            "method": "subscribe", "success": false, "req_id": value["req_id"], "symbol": key,
            "error": "Injected temporary subscription rejection",
        });
        Some((key.into(), Message::Text(answer.to_string().into())))
    }
}

impl KrakenConnection {
    fn record_message(&mut self, value: &Value) -> Option<(String, FrameKind)> {
        if self.product == KrakenProductType::Futures {
            let frame = parse_futures_frame(value)?;
            let first = (frame.key.clone(), frame.kind);
            self.record(frame);
            return Some(first);
        }

        if value["channel"] != "book" {
            return None;
        }

        let kind = match value["type"].as_str()? {
            "snapshot" => FrameKind::Snapshot,
            "update" => FrameKind::Update,
            _ => return None,
        };

        let mut frames = value["data"]
            .as_array()?
            .iter()
            .filter_map(|data| parse_spot_frame(data, kind));
        let first = frames.next()?;
        let identity = (first.key.clone(), first.kind);
        for frame in std::iter::once(first).chain(frames) {
            self.record(frame);
        }

        Some(identity)
    }

    fn record(&mut self, frame: Frame) {
        let book = self.books.entry(frame.key.clone()).or_default();
        let recent = self.positions.entry(frame.key.clone()).or_default();
        if let Some(update) = frame.update {
            recent.rotate_left(1);
            recent[3] = Some(UpdatePosition {
                timestamp: frame.timestamp,
                update,
            });
        } else {
            *recent = [None; 4];
        }

        if frame.kind == FrameKind::Snapshot {
            *book = WireBook::default();
        }

        book.apply(&frame.bids, &frame.asks);
        let top = book.top(DEPTH);
        if self.product == KrakenProductType::Spot {
            *book = top.clone();
        }

        self.views.lock().entry(frame.key).or_default().record(
            ObservedBook {
                view: WireView {
                    epoch: self.epoch,
                    sequence: frame.sequence,
                    timestamp: frame.timestamp,
                    book: top,
                },
                recent: *recent,
            },
            Instant::now(),
            self.history_min_age,
        );
    }
}

struct Frame {
    key: String,
    kind: FrameKind,
    timestamp: u64,
    sequence: u64,
    update: Option<UpdateIdentity>,
    bids: Vec<(Decimal, Decimal)>,
    asks: Vec<(Decimal, Decimal)>,
}

fn parse_spot_frame(data: &Value, kind: FrameKind) -> Option<Frame> {
    let timestamp: jiff::Timestamp = data["timestamp"].as_str()?.parse().ok()?;
    Some(Frame {
        key: data["symbol"].as_str()?.into(),
        kind,
        timestamp: timestamp.as_nanosecond().try_into().ok()?,
        sequence: data["checksum"].as_u64()?,
        update: None,
        bids: levels(&data["bids"]),
        asks: levels(&data["asks"]),
    })
}

fn parse_futures_frame(value: &Value) -> Option<Frame> {
    match value["feed"].as_str()? {
        "book_snapshot" => Some(Frame {
            key: value["product_id"].as_str()?.into(),
            kind: FrameKind::Snapshot,
            timestamp: value["timestamp"].as_u64()? * 1_000_000,
            sequence: value["seq"].as_u64()?,
            update: None,
            bids: levels(&value["bids"]),
            asks: levels(&value["asks"]),
        }),
        "book" => {
            let level = (decimal(&value["price"])?, decimal(&value["qty"])?);
            let (bids, asks) = match value["side"].as_str()? {
                "buy" => (vec![level], Vec::new()),
                "sell" => (Vec::new(), vec![level]),
                _ => return None,
            };

            Some(Frame {
                key: value["product_id"].as_str()?.into(),
                kind: FrameKind::Update,
                timestamp: value["timestamp"].as_u64()? * 1_000_000,
                sequence: value["seq"].as_u64()?,
                update: Some(UpdateIdentity {
                    is_bid: !bids.is_empty(),
                    price: level.0,
                    qty: level.1,
                }),
                bids,
                asks,
            })
        }
        _ => None,
    }
}

fn levels(value: &Value) -> Vec<(Decimal, Decimal)> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|level| {
            (
                decimal(&level["price"]).unwrap(),
                decimal(&level["qty"]).unwrap(),
            )
        })
        .collect()
}

fn decimal(value: &Value) -> Option<Decimal> {
    Decimal::from_str(value.as_str().unwrap_or(&value.to_string())).ok()
}

async fn independent_feed(product: KrakenProductType, views: Views) {
    let (mut ws_client, _) = connect_async(url(product))
        .await
        .expect("independent venue socket");
    let symbols = symbols(product);
    let request = match product {
        KrakenProductType::Spot => json!({
            "method": "subscribe", "params": { "channel": "book", "symbol": symbols, "depth": DEPTH },
        }),
        KrakenProductType::Futures => json!({
            "event": "subscribe", "feed": "book", "product_ids": symbols,
        }),
    };

    ws_client
        .send(Message::Text(request.to_string().into()))
        .await
        .unwrap();

    let mut connection = KrakenConnection {
        product,
        epoch: 0,
        history_min_age: Duration::ZERO,
        views,
        book_commands: Arc::default(),
        books: HashMap::new(),
        positions: HashMap::new(),
    };

    while let Some(Ok(message)) = ws_client.next().await {
        let Some(value) = message
            .to_text()
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
        else {
            continue;
        };

        let _ = connection.record_message(&value);
    }
}

fn product(args: &StressArgs) -> KrakenProductType {
    match args.flag("product") {
        "spot" => KrakenProductType::Spot,
        "futures" => KrakenProductType::Futures,
        value => panic!("Unsupported Kraken product {value}"),
    }
}

fn symbols(product: KrakenProductType) -> [&'static str; 2] {
    match product {
        KrakenProductType::Spot => ["BTC/USD", "ETH/USD"],
        KrakenProductType::Futures => ["PF_XBTUSD", "PF_ETHUSD"],
    }
}

fn url(product: KrakenProductType) -> &'static str {
    match product {
        KrakenProductType::Spot => "wss://ws.kraken.com/v2",
        KrakenProductType::Futures => "wss://futures.kraken.com/ws/v1",
    }
}

fn endpoint(product: KrakenProductType) -> &'static str {
    match product {
        KrakenProductType::Spot => "kraken-spot-data-streams",
        KrakenProductType::Futures => "kraken-futures-data-streams",
    }
}

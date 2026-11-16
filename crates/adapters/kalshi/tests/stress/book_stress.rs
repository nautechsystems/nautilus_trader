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

//! Bounded autonomous market-data fault injection with an independent decimal order book oracle.
//!
//! Live validation requires explicit `--tickers`, `--api-key-fd`, and `--private-key-fd` arguments.
//! The two descriptors supply the API key ID and PEM private key without environment credentials.
//! Churn requires live delta activity on the first ticker to drive shared-SID faults for every book.
//! Run `--scenario self-check` to prove the wire oracle without credentials or venue connections.
//!
//! `churn` is the only live scenario and the default. It requires `--rounds 0`: connect, subscribe,
//! verify healthy books, recover from a shared-SID sequence gap and one dropped recovery snapshot
//! per book without reconnecting, wait for final oracle coverage, and stop.
//!
//! Reconnect-dependent live scenarios are unavailable while transparent Kalshi WebSocket
//! reconnects are disabled. No orders are submitted. Every emitted batch passes through the shared
//! `BookStreamChecker` and is verified against raw frames before proxy faults.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    io::Read,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use nautilus_common::{clients::DataClient, live::runner::get_data_event_sender};
use nautilus_core::{string::secret::SecretString, time::get_atomic_clock_realtime};
use nautilus_kalshi::{
    common::{
        consts::{
            KALSHI_ACCESS_KEY_HEADER, KALSHI_ACCESS_SIGNATURE_HEADER,
            KALSHI_ACCESS_TIMESTAMP_HEADER, KALSHI_CLIENT_ID, KALSHI_WS_SIGN_PATH,
        },
        credential::KalshiCredential,
        enums::KalshiEnvironment,
        parse::parse_instrument_id,
        urls::kalshi_ws_url,
    },
    config::KalshiDataClientConfig,
    data::KalshiDataClient,
};
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    enums::BookAction,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::{
    transport::Message as TransportMessage,
    websocket::{TransportBackend, client::WebSocketClientInner},
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, Flag, FrameKind, Route, Session, StressArgs, StressVenue, Upstream,
    WireBook, WireCodec, WireConnection, WireView,
};
use tokio_tungstenite::tungstenite::Message;

const STREAM: &str = "kalshi";
const ENDPOINT: &str = "kalshi-data-stream";
const DEPTH: usize = 100;
const ORACLE_EVENTS_MAX: usize = 16_384;
const ORACLE_CLOCKS_MAX: usize = 4096;
const ORACLE_VIEWS_MAX: usize = 65_536;
const ORACLE_VENUE_VIEWS_MAX: usize = 1;

static CREDENTIALS: OnceLock<Credentials> = OnceLock::new();

type KalshiSession = Session<Kalshi>;

fn main() {
    stress::run::<Kalshi, _, _>(|args| async move {
        if args.scenario() == "self-check" {
            return "synthetic=passed".to_string();
        }

        let credentials = read_credentials(&args).expect("test credentials load from descriptors");
        assert!(
            CREDENTIALS.set(credentials).is_ok(),
            "credentials initialize once"
        );
        let ids = selected_instruments(&args);
        churn(&args, &ids).await
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId]) -> String {
    assert_eq!(
        args.rounds(),
        0,
        "churn requires --rounds 0 while transparent Kalshi WebSocket reconnects are disabled"
    );

    let mut session = KalshiSession::connect(args).await;
    subscribe_all(&mut session, ids);
    session.healthy(ids).await;
    recover_without_reconnect(&mut session, ids).await;
    wait_oracle(&mut session).await;
    let batches = session.batches();
    session.stop().await;
    format!("books={} batches_total={batches}", ids.len())
}

fn subscribe_all(session: &mut KalshiSession, ids: &[InstrumentId]) {
    for id in ids {
        session.subscribe(*id);
    }
}

async fn recover_without_reconnect(session: &mut KalshiSession, ids: &[InstrumentId]) {
    let connections = session.proxy().connections(STREAM);
    let id = ids[0];

    let before = ids
        .iter()
        .map(|id| {
            let snapshots = session.book(id).snapshots;
            let attempts = session.venue().attempts(id);
            let mut fault = session.fault(id);
            fault.drop_snapshots = 1;
            (*id, fault.dropped, snapshots, attempts)
        })
        .collect::<Vec<_>>();

    let corrupted = {
        let mut fault = session.fault(&id);
        fault.corrupt = 1;
        fault.corrupted
    };

    session.expect_all();
    session.healthy(ids).await;
    for (id, dropped, snapshots, attempts) in before {
        let fault = session.fault(&id).clone();
        assert_eq!(fault.dropped, dropped + 1);
        assert_eq!(session.venue().unsubscribes(&id), 0);
        assert_eq!(session.venue().attempts(&id), attempts + 2);
        assert_eq!(session.book(&id).snapshots, snapshots + 1);
    }

    assert_eq!(session.fault(&id).corrupted, corrupted + 1);
    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check("autonomous", format!("books={} reconnects=0", ids.len()));
}

fn selected_instruments(args: &StressArgs) -> Vec<InstrumentId> {
    let mut unique = HashSet::new();

    let ids = args
        .flag("tickers")
        .split(',')
        .map(str::trim)
        .filter(|ticker| !ticker.is_empty())
        .map(|ticker| {
            parse_instrument_id(ticker).unwrap_or_else(|e| panic!("invalid ticker {ticker}: {e}"))
        })
        .collect::<Vec<_>>();

    assert!(
        ids.len() >= 3,
        "--tickers requires at least three distinct open markets"
    );
    assert!(
        ids.iter().all(|id| unique.insert(*id)),
        "--tickers requires distinct markets"
    );
    ids
}

fn read_credentials(args: &StressArgs) -> anyhow::Result<Credentials> {
    let api_key_fd = parse_fd(args.flag("api-key-fd"), "--api-key-fd")?;
    let private_key_fd = parse_fd(args.flag("private-key-fd"), "--private-key-fd")?;
    anyhow::ensure!(
        api_key_fd != private_key_fd,
        "credential descriptors must differ"
    );

    let mut api_key_id = read_secret(api_key_fd, "API key")?;
    while matches!(api_key_id.as_bytes().last(), Some(b'\r' | b'\n')) {
        api_key_id.pop();
    }

    let private_key = read_secret(private_key_fd, "private key")?;
    anyhow::ensure!(!api_key_id.trim().is_empty(), "API key is empty");
    anyhow::ensure!(!private_key.trim().is_empty(), "private key is empty");

    Ok(Credentials {
        api_key_id: SecretString::from(api_key_id),
        private_key: SecretString::from(private_key),
    })
}

fn parse_fd(value: &str, name: &str) -> anyhow::Result<i32> {
    anyhow::ensure!(!value.is_empty(), "{name} is required");
    let fd = value
        .parse::<i32>()
        .with_context(|| format!("invalid numeric descriptor for {name}"))?;
    anyhow::ensure!(fd > 2, "{name} must refer to an inherited descriptor");
    Ok(fd)
}

#[cfg(unix)]
fn read_secret(fd: i32, name: &str) -> anyhow::Result<String> {
    // SAFETY: the descriptors are distinct inherited FDs, with ownership transferred to each file once,
    // so closing either file cannot close the other descriptor.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut value = String::new();
    file.read_to_string(&mut value)
        .with_context(|| format!("failed to read {name} descriptor"))?;
    Ok(value)
}

#[cfg(not(unix))]
fn read_secret(_fd: i32, _name: &str) -> anyhow::Result<String> {
    anyhow::bail!("inherited credential descriptors require Unix")
}

struct Credentials {
    api_key_id: SecretString,
    private_key: SecretString,
}

struct Kalshi {
    wire: KalshiWire,
    venue_wire: KalshiWire,
    venue_task: Option<tokio::task::JoinHandle<()>>,
    venue_pending: VecDeque<(String, u64, WireView)>,
    venue_checks: HashMap<String, usize>,
    venue_skipped: usize,
    environment: KalshiEnvironment,
    instruments: Vec<InstrumentId>,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
    verified_batches: HashMap<InstrumentId, usize>,
    published_updates: HashMap<InstrumentId, usize>,
    unmatched: usize,
    snapshots_empty: usize,
    snapshots_one_sided: usize,
}

impl Kalshi {
    fn attempts(&self, id: &InstrumentId) -> usize {
        self.wire.attempts(&self.key(id))
    }

    fn unsubscribes(&self, id: &InstrumentId) -> usize {
        self.wire.unsubscribes(&self.key(id))
    }
}

impl StressVenue for Kalshi {
    const NAME: &'static str = "kalshi";
    const SCENARIOS: &'static [&'static str] = &["churn", "self-check"];
    const ROUNDS: usize = 0;
    const FLAGS: &'static [Flag] = &[
        Flag {
            name: "environment",
            default: "demo",
            help: "demo or production",
        },
        Flag {
            name: "tickers",
            default: "",
            help: "Comma-separated tickers for at least three distinct open markets, active delta source first",
        },
        Flag {
            name: "api-key-fd",
            default: "",
            help: "Inherited descriptor containing the API key ID",
        },
        Flag {
            name: "private-key-fd",
            default: "",
            help: "Inherited descriptor containing the PEM private key",
        },
    ];
    const SEQUENCED: bool = true;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(120);

    fn self_check() {
        check_wire_views();
        check_wire_oracle();
        check_clock_views();
        check_snapshot_rejection();
        check_streaming_health();
    }

    fn new(args: &StressArgs) -> Self {
        let environment = KalshiEnvironment::from_str(args.flag("environment"))
            .expect("argument parsing accepts a declared Kalshi environment");

        let instruments = selected_instruments(args);
        let wire = KalshiWire::default();

        let venue_wire = KalshiWire {
            views: WireViews::with_limit(ORACLE_VENUE_VIEWS_MAX),
            ..KalshiWire::default()
        };

        let venue_task = Some(watch_venue(environment, &instruments, venue_wire.clone()));

        Self {
            wire,
            venue_wire,
            venue_task,
            venue_pending: VecDeque::new(),
            venue_checks: HashMap::new(),
            venue_skipped: 0,
            environment,
            instruments,
            expected_epoch: 1,
            epochs: HashMap::new(),
            verified_batches: HashMap::new(),
            published_updates: HashMap::new(),
            unmatched: 0,
            snapshots_empty: 0,
            snapshots_one_sided: 0,
        }
    }

    fn client_id(&self) -> ClientId {
        *KALSHI_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: KALSHI_WS_SIGN_PATH,
            upstream: kalshi_ws_url(self.environment).to_string(),
            endpoint: ENDPOINT,
            headers: &[
                KALSHI_ACCESS_KEY_HEADER,
                KALSHI_ACCESS_TIMESTAMP_HEADER,
                KALSHI_ACCESS_SIGNATURE_HEADER,
            ],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let credentials = CREDENTIALS
            .get()
            .context("test credentials initialize before sessions")?;

        let config = KalshiDataClientConfig {
            api_key_id: Some(credentials.api_key_id.clone()),
            private_key: Some(credentials.private_key.clone()),
            environment: self.environment,
            base_url_http: None,
            base_url_ws: Some(SecretString::from(format!(
                "ws://{proxy}{KALSHI_WS_SIGN_PATH}"
            ))),
            proxy_url: None,
            http_timeout_secs: 30,
            ws_timeout_secs: 30,
            book_snapshot_timeout_secs: args.timeout_secs(),
            update_instruments_interval_mins: 60,
            transport_backend: TransportBackend::Sockudo,
            instrument_ids: self.instruments.clone(),
        };

        let sender = get_data_event_sender();
        let client =
            KalshiDataClient::new(*KALSHI_CLIENT_ID, config, self.instruments.clone(), sender)?;

        Ok(Box::new(client))
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        instrument_id.symbol.to_string()
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;

        let Some(view) =
            self.wire
                .views
                .find(&self.key(&id), deltas.sequence, deltas.ts_event.as_u64())
        else {
            self.unmatched += 1;
            return;
        };

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            panic!(
                "wire oracle mismatch {id} seq={} ts={}: {violation}",
                deltas.sequence, deltas.ts_event
            );
        }

        if deltas.deltas[0].action == BookAction::Clear {
            self.snapshots_empty +=
                usize::from(view.book.bids.is_empty() && view.book.asks.is_empty());
            self.snapshots_one_sided +=
                usize::from(view.book.bids.is_empty() != view.book.asks.is_empty());
        }

        if let Some((key, clock)) = self.wire.clocks.lock().unwrap().position(&view) {
            self.venue_pending.push_back((key, clock, view.clone()));

            if self.venue_pending.len() > ORACLE_EVENTS_MAX {
                self.venue_pending.pop_front();
                self.venue_skipped += 1;
            }
        } else {
            self.venue_skipped += 1;
        }

        self.epochs.insert(id, view.epoch);
        *self.verified_batches.entry(id).or_default() += 1;
        if deltas
            .deltas
            .first()
            .is_some_and(|delta| delta.action != BookAction::Clear)
        {
            *self.published_updates.entry(id).or_default() += 1;
        }
    }

    fn streaming(&self, id: &InstrumentId, book: &BookProgress, _start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && self.verified_batches.get(id).copied().unwrap_or(0) == book.batches
    }

    fn stats(&self) -> String {
        format!(
            "snapshot_attempts={} unmatched={} venue_checks={} venue_books={} venue_skipped={} venue_pending={} snapshots_empty={} snapshots_one_sided={}",
            self.wire.total_attempts(),
            self.unmatched,
            self.venue_checks.values().sum::<usize>(),
            self.venue_checks.len(),
            self.venue_skipped,
            self.venue_pending.len(),
            self.snapshots_empty,
            self.snapshots_one_sided,
        )
    }

    fn poll(&mut self) {
        assert!(
            self.venue_task
                .as_ref()
                .is_none_or(|task| !task.is_finished()),
            "independent oracle task stopped"
        );
        let source = self.wire.clocks.lock().unwrap();
        let venue = self.venue_wire.clocks.lock().unwrap();
        self.venue_pending.retain(|(key, clock, expected)| {
            let Some(complete) = source.complete(key, *clock) else {
                if source.passed(key, *clock) {
                    self.venue_skipped += 1;
                    return false;
                }

                return true;
            };

            // A single exchange action can emit several deltas at the same clock
            if expected.book != complete.book {
                self.venue_skipped += 1;
                return false;
            }

            let Some(actual) = venue.complete(key, *clock) else {
                if venue.passed(key, *clock) {
                    self.venue_skipped += 1;
                    return false;
                }

                return true;
            };

            assert_eq!(
                actual.book, expected.book,
                "independent WebSocket oracle mismatch for {key} clock={clock}"
            );
            *self.venue_checks.entry(key.clone()).or_default() += 1;
            false
        });
    }

    fn finish(&mut self) {
        self.poll();

        if let Some(task) = &self.venue_task {
            task.abort();
        }

        for id in &self.instruments {
            assert!(
                self.venue_checks
                    .get(&self.key(id))
                    .copied()
                    .unwrap_or_default()
                    > 0,
                "{id} needs an aligned independent venue comparison"
            );
        }

        assert_eq!(
            self.unmatched, 0,
            "every emitted batch must align with the wire oracle"
        );

        for id in &self.instruments {
            assert!(
                self.publication_ready(id),
                "{id} forwarded deltas produced no verified incremental batch"
            );
        }
    }
}

impl Kalshi {
    fn publication_ready(&self, id: &InstrumentId) -> bool {
        self.wire.delta_frames(&self.key(id)) <= 1
            || self.published_updates.get(id).copied().unwrap_or_default() > 0
    }
}

fn watch_venue(
    environment: KalshiEnvironment,
    ids: &[InstrumentId],
    wire: KalshiWire,
) -> tokio::task::JoinHandle<()> {
    let tickers = ids
        .iter()
        .map(|id| id.symbol.to_string())
        .collect::<Vec<_>>();

    tokio::spawn(async move {
        let credentials = CREDENTIALS.get().unwrap();
        let credential = KalshiCredential::resolve(
            Some(credentials.api_key_id.clone()),
            Some(credentials.private_key.clone()),
            environment,
        )
        .unwrap();
        let mut epoch = 0;
        loop {
            epoch += 1;
            let headers = credential
                .headers(
                    get_atomic_clock_realtime().get_time_ms(),
                    "GET",
                    KALSHI_WS_SIGN_PATH,
                )
                .unwrap()
                .into_iter()
                .map(|(name, value)| (name, value.expose_secret().to_string()))
                .collect();

            let (mut writer, mut reader) = WebSocketClientInner::connect_with_server(
                kalshi_ws_url(environment),
                headers,
                TransportBackend::Sockudo,
                None,
                None,
                None,
            )
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "independent oracle connection failed: {}",
                    e.redacted_message()
                )
            });

            let request = json!({"id": 1, "cmd": "subscribe", "params": {"channels": ["orderbook_delta"], "market_tickers": tickers, "use_yes_price": true}}).to_string();
            writer.send(TransportMessage::text(request)).await.unwrap();
            let mut connection = test_connection(&wire, epoch);

            let mut snapshots = tokio::time::interval(Duration::from_secs(1));
            let mut request_id = 1;

            loop {
                let message = tokio::select! {
                    message = reader.next() => {
                        let Some(message) = message else { break };
                        message
                    }
                    _ = snapshots.tick() => {
                        if let Some(sid) = connection.sid {
                            request_id += 1;
                            let request = json!({"id": request_id, "cmd": "update_subscription", "params": {"sid": sid, "market_tickers": tickers, "action": "get_snapshot"}}).to_string();
                            writer.send(TransportMessage::text(request)).await.unwrap();
                        }
                        continue;
                    }
                };

                let message = message.unwrap_or_else(|e| {
                    panic!("independent oracle read failed: {}", e.redacted_message())
                });

                match message {
                    TransportMessage::Text(bytes) => {
                        let text = std::str::from_utf8(&bytes).unwrap().to_string();
                        connection.upstream(&Message::Text(text.into()));
                    }
                    TransportMessage::Ping(bytes) => {
                        writer.send(TransportMessage::Pong(bytes)).await.unwrap();
                    }
                    TransportMessage::Close(_) => break,
                    _ => {}
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

#[derive(Default)]
struct ClockViews {
    books: HashMap<String, ClockBook>,
    positions: HashMap<(usize, u64, u64), (String, u64)>,
    order: VecDeque<(usize, u64, u64)>,
}

#[derive(Default)]
struct ClockBook {
    current: Option<(u64, WireView, bool)>,
    completed: BTreeMap<u64, WireView>,
}

impl ClockViews {
    fn record(&mut self, key: &str, clock: u64, snapshot: bool, view: &WireView) {
        let book = self.books.entry(key.to_string()).or_default();
        if let Some((previous, pending, was_snapshot)) = &book.current {
            assert!(clock >= *previous, "exchange clock decreased for {key}");

            if !snapshot {
                assert!(
                    clock > *previous || !was_snapshot,
                    "delta follows snapshot at the same clock for {key}"
                );

                if clock > *previous {
                    book.completed.insert(*previous, pending.clone());
                }
            }
        }

        if snapshot {
            book.completed.insert(clock, view.clone());
        }

        book.current = Some((clock, view.clone(), snapshot));
        while book.completed.len() > ORACLE_CLOCKS_MAX {
            book.completed.pop_first();
        }

        let position = (view.epoch, view.sequence, view.timestamp);
        assert!(
            self.positions
                .insert(position, (key.to_string(), clock))
                .is_none()
        );
        self.order.push_back(position);

        if self.order.len() > ORACLE_EVENTS_MAX {
            self.positions.remove(&self.order.pop_front().unwrap());
        }
    }

    fn position(&self, view: &WireView) -> Option<(String, u64)> {
        self.positions
            .get(&(view.epoch, view.sequence, view.timestamp))
            .cloned()
    }

    fn complete(&self, key: &str, clock: u64) -> Option<&WireView> {
        self.books.get(key)?.completed.get(&clock)
    }

    fn passed(&self, key: &str, clock: u64) -> bool {
        self.books
            .get(key)
            .and_then(|book| book.current.as_ref())
            .is_some_and(|(latest, _, _)| *latest > clock)
    }
}

async fn wait_oracle(session: &mut KalshiSession) {
    session
        .until(
            Duration::from_secs(60),
            "independent venue coverage and incremental publication for every active book",
            |s| {
                s.venue().instruments.iter().all(|id| {
                    s.venue().venue_checks.contains_key(&s.venue().key(id))
                        && s.venue().publication_ready(id)
                })
            },
        )
        .await;
}

fn check_clock_views() {
    let mut views = ClockViews::default();

    let mut view = WireView {
        epoch: 1,
        sequence: 1,
        timestamp: 3,
        book: WireBook::default(),
    };

    views.record("MARKET-A", 0, true, &view);
    assert_eq!(views.complete("MARKET-A", 0), Some(&view));
    assert_eq!(views.position(&view), Some(("MARKET-A".to_string(), 0)));
    view.sequence += 1;
    views.record("MARKET-B", 7, true, &view);
    assert_eq!(views.complete("MARKET-A", 7), None);
    assert_eq!(views.complete("MARKET-B", 7), Some(&view));

    view.sequence += 1;
    view.book.apply(&[(Decimal::new(25, 2), Decimal::ONE)], &[]);
    views.record("MARKET-A", 1, false, &view);
    assert_eq!(views.complete("MARKET-A", 1), None);
    let partial = view.clone();
    view.sequence += 1;
    view.book.apply(&[(Decimal::new(25, 2), Decimal::TWO)], &[]);
    views.record("MARKET-A", 1, false, &view);
    assert_eq!(views.complete("MARKET-A", 1), None);
    let complete = view.clone();
    view.sequence += 1;
    views.record("MARKET-A", 2, false, &view);
    assert_eq!(views.complete("MARKET-A", 1), Some(&complete));
    assert_ne!(views.complete("MARKET-A", 1).unwrap().book, partial.book);
    assert_eq!(views.complete("MARKET-A", 2), None);
    assert!(!views.passed("MARKET-A", 2));

    view.sequence += 1;
    views.record("MARKET-A", 3, true, &view);
    assert_eq!(views.complete("MARKET-A", 2), None);
    assert!(views.passed("MARKET-A", 2));
    assert_eq!(views.complete("MARKET-A", 3), Some(&view));

    for clock in 4..ORACLE_EVENTS_MAX as u64 + 4 {
        view.sequence += 1;
        views.record("MARKET-A", clock, true, &view);
    }

    assert_eq!(views.order.len(), ORACLE_EVENTS_MAX);
    assert_eq!(views.positions.len(), ORACLE_EVENTS_MAX);
    assert_eq!(views.books["MARKET-A"].completed.len(), ORACLE_CLOCKS_MAX);
    assert_eq!(views.complete("MARKET-A", 1), None);
    assert_eq!(views.position(&partial), None);
    assert_eq!(
        views.position(&view),
        Some(("MARKET-A".to_string(), ORACLE_EVENTS_MAX as u64 + 3))
    );
}

#[derive(Clone)]
struct WireViews {
    views: Arc<Mutex<HashMap<String, VecDeque<WireView>>>>,
    limit: usize,
}

impl WireViews {
    fn with_limit(limit: usize) -> Self {
        assert!(limit > 0);

        Self {
            views: Arc::default(),
            limit,
        }
    }

    fn record(&self, key: &str, view: WireView) {
        let mut views = self.views.lock().expect("wire view lock is not poisoned");
        let views = views.entry(key.to_string()).or_default();
        views.push_back(view);
        if views.len() > self.limit {
            views.pop_front();
        }
    }

    fn find(&self, key: &str, sequence: u64, timestamp: u64) -> Option<WireView> {
        self.views
            .lock()
            .expect("wire view lock is not poisoned")
            .get(key)?
            .iter()
            .rev()
            .find(|view| view.sequence == sequence && view.timestamp == timestamp)
            .cloned()
    }
}

#[derive(Clone)]
struct KalshiWire {
    views: WireViews,
    clocks: Arc<Mutex<ClockViews>>,
    attempts: Arc<Mutex<HashMap<String, usize>>>,
    delta_frames: Arc<Mutex<HashMap<String, usize>>>,
    unsubscribes: Arc<Mutex<HashMap<String, usize>>>,
}

impl Default for KalshiWire {
    fn default() -> Self {
        Self {
            views: WireViews::with_limit(ORACLE_VIEWS_MAX),
            clocks: Arc::default(),
            attempts: Arc::default(),
            delta_frames: Arc::default(),
            unsubscribes: Arc::default(),
        }
    }
}

impl KalshiWire {
    fn attempts(&self, key: &str) -> usize {
        self.attempts
            .lock()
            .expect("snapshot attempt lock is not poisoned")
            .get(key)
            .copied()
            .unwrap_or_default()
    }

    fn total_attempts(&self) -> usize {
        self.attempts
            .lock()
            .expect("snapshot attempt lock is not poisoned")
            .values()
            .sum()
    }

    fn delta_frames(&self, key: &str) -> usize {
        self.delta_frames
            .lock()
            .expect("delta frame lock is not poisoned")
            .get(key)
            .copied()
            .unwrap_or_default()
    }

    fn unsubscribes(&self, key: &str) -> usize {
        self.unsubscribes
            .lock()
            .expect("unsubscribe lock is not poisoned")
            .get(key)
            .copied()
            .unwrap_or_default()
    }
}

impl WireCodec for KalshiWire {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(KalshiConnection {
            views: self.views.clone(),
            clocks: Arc::clone(&self.clocks),
            attempts: Arc::clone(&self.attempts),
            delta_frames: Arc::clone(&self.delta_frames),
            unsubscribes: Arc::clone(&self.unsubscribes),
            route: route.name,
            epoch: number,
            sid: None,
            sequence: 0,
            books: HashMap::new(),
        })
    }
}

struct KalshiConnection {
    views: WireViews,
    clocks: Arc<Mutex<ClockViews>>,
    attempts: Arc<Mutex<HashMap<String, usize>>>,
    delta_frames: Arc<Mutex<HashMap<String, usize>>>,
    unsubscribes: Arc<Mutex<HashMap<String, usize>>>,
    route: &'static str,
    epoch: usize,
    sid: Option<u64>,
    sequence: u64,
    books: HashMap<String, WireBook>,
}

impl KalshiConnection {
    fn reset(&mut self, sid: u64) {
        self.sid = Some(sid);
        self.sequence = 0;
        self.books.clear();
    }

    fn advance_control(&mut self, frame: &Value) {
        let sid = frame["sid"].as_u64();
        let seq = frame["seq"].as_u64();

        if let (Some(sid), Some(seq)) = (sid, seq)
            && self.sid == Some(sid)
        {
            self.sequence = seq;
        }
    }
}

impl WireConnection for KalshiConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        let kind = frame["type"].as_str().unwrap_or("");

        if kind == "error" {
            eprintln!("Venue error on route {}: {frame}", self.route);
            return Upstream::Other;
        }

        if kind == "subscribed"
            && frame["msg"]["channel"] == "orderbook_delta"
            && let Some(sid) = frame["msg"]["sid"].as_u64()
        {
            self.reset(sid);
            return Upstream::Other;
        }

        if matches!(kind, "ok" | "unsubscribed") {
            self.advance_control(&frame);

            if kind == "unsubscribed" && self.sid == frame["sid"].as_u64() {
                self.sid = None;
                self.sequence = 0;
                self.books.clear();
            }

            return Upstream::Other;
        }

        if !matches!(kind, "orderbook_snapshot" | "orderbook_delta")
            || self.sid != frame["sid"].as_u64()
        {
            return Upstream::Other;
        }

        let Some(key) = frame["msg"]["market_ticker"]
            .as_str()
            .map(ToString::to_string)
        else {
            return Upstream::Other;
        };

        let snapshot = kind == "orderbook_snapshot";
        let book = self.books.entry(key.clone()).or_default();
        if snapshot {
            *book = WireBook::default();
            book.apply(
                &levels(&frame["msg"]["yes_dollars_fp"]),
                &levels(&frame["msg"]["no_dollars_fp"]),
            );
        } else {
            apply_delta(book, &frame["msg"]);
            *self
                .delta_frames
                .lock()
                .expect("delta frame lock is not poisoned")
                .entry(key.clone())
                .or_default() += 1;
        }

        if let Some(sequence) = frame["seq"].as_u64() {
            self.sequence = sequence;

            let timestamp_ms = if snapshot {
                frame["sending_ts_ms"].as_u64()
            } else {
                frame["msg"]["ts_ms"]
                    .as_u64()
                    .or_else(|| frame["sending_ts_ms"].as_u64())
            };

            if let Some(timestamp) = timestamp_ms.and_then(|ms| ms.checked_mul(1_000_000)) {
                let view = WireView {
                    epoch: self.epoch,
                    sequence,
                    timestamp,
                    book: book.top(DEPTH),
                };

                if let Some(clock) = frame["msg"]["clock"].as_u64() {
                    self.clocks
                        .lock()
                        .unwrap()
                        .record(&key, clock, snapshot, &view);
                }

                self.views.record(&key, view);
            }
        }

        Upstream::Book {
            key,
            kind: if snapshot {
                FrameKind::Snapshot
            } else {
                FrameKind::Update
            },
        }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        let Message::Text(text) = message else {
            return Vec::new();
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };

        if frame["cmd"] == "update_subscription"
            && frame["params"]["action"] == "delete_markets"
            && frame["params"]["sid"].as_u64() == self.sid
        {
            let keys = frame["params"]["market_tickers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|ticker| ticker.as_str().map(ToString::to_string))
                .collect::<Vec<_>>();

            for key in &keys {
                *self
                    .unsubscribes
                    .lock()
                    .expect("unsubscribe lock is not poisoned")
                    .entry(key.clone())
                    .or_default() += 1;
            }

            return keys;
        }

        if frame["cmd"] != "unsubscribe" || frame["params"]["sids"][0].as_u64() != self.sid {
            return Vec::new();
        }

        let keys = self.books.keys().cloned().collect::<Vec<_>>();
        for key in &keys {
            *self
                .unsubscribes
                .lock()
                .expect("unsubscribe lock is not poisoned")
                .entry(key.clone())
                .or_default() += 1;
        }

        keys
    }

    // Advances one delta beyond its raw position, forcing the adapter's shared SID gap path
    fn corrupt(&mut self, message: &mut Message, _key: &str, kind: FrameKind) -> bool {
        if kind != FrameKind::Update {
            return false;
        }

        let Message::Text(text) = message else {
            return false;
        };

        let Ok(mut frame) = serde_json::from_str::<Value>(text) else {
            return false;
        };

        if frame["type"] != "orderbook_delta" {
            return false;
        }

        let Some(sequence) = frame["seq"].as_u64().and_then(|seq| seq.checked_add(1)) else {
            return false;
        };

        frame["seq"] = json!(sequence);
        *message = Message::Text(frame.to_string().into());
        true
    }

    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let Message::Text(text) = message else {
            return None;
        };

        let frame = serde_json::from_str::<Value>(text).ok()?;

        if frame["cmd"] != "update_subscription" || frame["params"]["action"] != "get_snapshot" {
            return None;
        }

        let id = frame["id"].as_u64()?;
        let sid = frame["params"]["sid"].as_u64()?;
        let tickers = frame["params"]["market_tickers"].as_array()?;

        let [ticker] = tickers.as_slice() else {
            return None;
        };

        let key = ticker.as_str()?.to_string();
        *self
            .attempts
            .lock()
            .expect("snapshot attempt lock is not poisoned")
            .entry(key.clone())
            .or_default() += 1;

        let mut reply =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["stress_snapshot_rejection"]
                .clone();
        reply["id"] = serde_json::json!(id);
        reply["sid"] = serde_json::json!(sid);

        Some((key, Message::Text(reply.to_string().into())))
    }
}

fn levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    if rows.is_null() {
        return Vec::new();
    }

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

fn apply_delta(book: &mut WireBook, msg: &Value) {
    let price = Decimal::from_str(msg["price_dollars"].as_str().unwrap()).unwrap();
    let delta = Decimal::from_str(msg["delta_fp"].as_str().unwrap()).unwrap();
    let side = msg["side"].as_str().unwrap();

    let levels = match side {
        "yes" => &mut book.bids,
        "no" => &mut book.asks,
        other => panic!("unknown Kalshi book side {other}"),
    };

    let previous = levels.get(&price).copied().unwrap_or_default();
    let size = previous
        .checked_add(delta)
        .expect("wire size does not overflow");
    assert!(size >= Decimal::ZERO, "wire size stays nonnegative");

    if size.is_zero() {
        levels.remove(&price);
    } else {
        levels.insert(price, size);
    }
}

fn check_wire_views() {
    let view = |epoch, sequence| WireView {
        epoch,
        sequence,
        timestamp: sequence,
        book: WireBook::default(),
    };

    for limit in [ORACLE_VENUE_VIEWS_MAX, ORACLE_VIEWS_MAX] {
        let views = WireViews::with_limit(limit);
        for sequence in 0..=limit as u64 {
            views.record("MARKET-A", view(1, sequence));
        }

        views.record("MARKET-B", view(2, 0));

        assert_eq!(views.find("MARKET-A", 0, 0), None);
        assert_eq!(views.find("MARKET-A", 1, 1), Some(view(1, 1)));
        assert_eq!(views.find("MARKET-B", 0, 0), Some(view(2, 0)));
        let sequence = limit as u64;
        assert_eq!(
            views.find("MARKET-A", sequence, sequence),
            Some(view(1, sequence))
        );
        assert_eq!(views.find("MARKET-A", sequence, sequence + 1), None);

        views.record("MARKET-A", view(2, sequence));
        assert_eq!(
            views.find("MARKET-A", sequence, sequence),
            Some(view(2, sequence))
        );
    }
}

fn check_wire_oracle() {
    let fixtures: Value = serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();
    let controls: Value =
        serde_json::from_str(include_str!("../../test_data/websocket.json")).unwrap();
    let wire = KalshiWire::default();
    let text = |value: Value| Message::Text(value.to_string().into());

    let data = |kind: &str, sid: u64, sequence: u64, timestamp: u64, msg: Value| {
        text(json!({
            "type": kind,
            "sid": sid,
            "seq": sequence,
            "sending_ts_ms": timestamp,
            "msg": msg,
        }))
    };

    let book = |kind| Upstream::Book {
        key: "MARKET-A".to_string(),
        kind,
    };

    let mut connection = test_connection(&wire, 3);
    let subscribed = text(fixtures["subscribed"].clone());
    assert_eq!(connection.upstream(&subscribed), Upstream::Other);

    let snapshot = data("orderbook_snapshot", 7, 1, 5, fixtures["snapshot"].clone());
    assert_eq!(connection.upstream(&snapshot), book(FrameKind::Snapshot));

    let deletion =
        serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
            .unwrap()["delete_market"]
            .clone();
    let deletion = text(deletion);
    assert_eq!(connection.client(&deletion), vec!["MARKET-A".to_string()]);
    assert_eq!(wire.unsubscribes("MARKET-A"), 1);

    let initial = WireBook {
        bids: [
            (
                Decimal::from_str("0.123456").unwrap(),
                Decimal::from_str("7.25").unwrap(),
            ),
            (
                Decimal::from_str("0.213456").unwrap(),
                Decimal::from_str("4.50").unwrap(),
            ),
        ]
        .into(),
        asks: [
            (
                Decimal::from_str("0.713456").unwrap(),
                Decimal::from_str("9.75").unwrap(),
            ),
            (
                Decimal::from_str("0.813456").unwrap(),
                Decimal::from_str("6.25").unwrap(),
            ),
        ]
        .into(),
    };

    assert_eq!(
        wire.views.find("MARKET-A", 1, 5_000_000),
        Some(WireView {
            epoch: 3,
            sequence: 1,
            timestamp: 5_000_000,
            book: initial,
        })
    );

    for (sequence, step) in fixtures["steps"]
        .as_array()
        .unwrap()
        .iter()
        .take(2)
        .enumerate()
    {
        let sequence = sequence as u64 + 2;
        let delta = data("orderbook_delta", 7, sequence, 100, step.clone());
        assert_eq!(connection.upstream(&delta), book(FrameKind::Update));
    }

    assert_eq!(
        wire.views.find("MARKET-A", 3, 1_700_000_000_124_000_000),
        Some(WireView {
            epoch: 3,
            sequence: 3,
            timestamp: 1_700_000_000_124_000_000,
            book: WireBook {
                bids: [
                    (
                        Decimal::from_str("0.123456").unwrap(),
                        Decimal::from_str("9.75").unwrap()
                    ),
                    (
                        Decimal::from_str("0.213456").unwrap(),
                        Decimal::from_str("4.50").unwrap()
                    ),
                ]
                .into(),
                asks: [
                    (
                        Decimal::from_str("0.713456").unwrap(),
                        Decimal::from_str("6.50").unwrap()
                    ),
                    (
                        Decimal::from_str("0.813456").unwrap(),
                        Decimal::from_str("6.25").unwrap()
                    ),
                ]
                .into(),
            },
        })
    );

    let mut control: Value =
        serde_json::from_str(controls["snapshot_ok"].as_str().unwrap()).unwrap();
    control["sid"] = json!(7);
    let control = text(control);
    let before_control = connection.books.get("MARKET-A").cloned().unwrap();
    assert_eq!(connection.upstream(&control), Upstream::Other);
    assert_eq!(connection.books.get("MARKET-A"), Some(&before_control));
    assert_eq!(connection.sequence, 4);
    assert_eq!(wire.views.find("MARKET-A", 4, 0), None);

    for (offset, step) in fixtures["steps"]
        .as_array()
        .unwrap()
        .iter()
        .skip(2)
        .enumerate()
    {
        let sequence = offset as u64 + 5;
        let delta = data("orderbook_delta", 7, sequence, 100, step.clone());
        assert_eq!(connection.upstream(&delta), book(FrameKind::Update));
    }

    assert_eq!(
        wire.views.find("MARKET-A", 6, 1_700_000_000_126_000_000),
        Some(WireView {
            epoch: 3,
            sequence: 6,
            timestamp: 1_700_000_000_126_000_000,
            book: WireBook {
                bids: [(
                    Decimal::from_str("0.123456").unwrap(),
                    Decimal::from_str("9.75").unwrap()
                )]
                .into(),
                asks: [
                    (
                        Decimal::from_str("0.613456").unwrap(),
                        Decimal::from_str("8.25").unwrap()
                    ),
                    (
                        Decimal::from_str("0.713456").unwrap(),
                        Decimal::from_str("6.50").unwrap()
                    ),
                    (
                        Decimal::from_str("0.813456").unwrap(),
                        Decimal::from_str("6.25").unwrap()
                    ),
                ]
                .into(),
            },
        })
    );

    let one_sided = data("orderbook_snapshot", 7, 7, 9, fixtures["one_sided"].clone());
    assert_eq!(connection.upstream(&one_sided), book(FrameKind::Snapshot));
    assert_eq!(
        wire.views.find("MARKET-A", 7, 9_000_000),
        Some(WireView {
            epoch: 3,
            sequence: 7,
            timestamp: 9_000_000,
            book: WireBook {
                bids: [(
                    Decimal::from_str("0.223456").unwrap(),
                    Decimal::from_str("1.25").unwrap()
                )]
                .into(),
                asks: [].into(),
            },
        })
    );

    let empty = data("orderbook_snapshot", 7, 8, 10, fixtures["empty"].clone());
    assert_eq!(connection.upstream(&empty), book(FrameKind::Snapshot));
    assert_eq!(
        wire.views.find("MARKET-A", 8, 10_000_000),
        Some(WireView {
            epoch: 3,
            sequence: 8,
            timestamp: 10_000_000,
            book: WireBook::default(),
        })
    );
    assert_eq!(wire.views.find("MARKET-A", 8, 11_000_000), None);
    assert_eq!(wire.views.find("MARKET-A", 99, 10_000_000), None);

    let mut unchanged = snapshot.clone();
    assert!(!connection.corrupt(&mut unchanged, "MARKET-A", FrameKind::Snapshot));
    assert_eq!(unchanged, snapshot);
    let mut delta = data("orderbook_delta", 7, 9, 100, fixtures["steps"][0].clone());
    assert!(connection.corrupt(&mut delta, "MARKET-A", FrameKind::Update));
    let corrupted = serde_json::from_str::<Value>(delta.to_text().unwrap()).unwrap();
    assert_eq!(corrupted["seq"], json!(10));

    let resubscribed = text(
        serde_json::from_str::<Value>(controls["book_resubscribed_3"].as_str().unwrap()).unwrap(),
    );
    assert_eq!(connection.upstream(&resubscribed), Upstream::Other);
    assert!(connection.books.is_empty());
    assert_eq!(connection.sequence, 0);
    assert_eq!(connection.sid, Some(21));

    let mut next_epoch = test_connection(&wire, 4);
    assert_eq!(next_epoch.upstream(&subscribed), Upstream::Other);
    assert!(next_epoch.books.is_empty());
    assert_eq!(next_epoch.sequence, 0);
    assert_eq!(next_epoch.sid, Some(7));
    assert_eq!(next_epoch.upstream(&empty), book(FrameKind::Snapshot));
    assert_eq!(
        wire.views.find("MARKET-A", 8, 10_000_000),
        Some(WireView {
            epoch: 4,
            sequence: 8,
            timestamp: 10_000_000,
            book: WireBook::default(),
        })
    );

    let mut unmatched = test_connection(&wire, 5);
    assert_eq!(unmatched.upstream(&subscribed), Upstream::Other);
    let without_time = text(json!({
        "type": "orderbook_snapshot",
        "sid": 7,
        "seq": 1,
        "msg": fixtures["empty"],
    }));
    assert_eq!(unmatched.upstream(&without_time), book(FrameKind::Snapshot));
    assert_eq!(wire.views.find("MARKET-A", 1, 0), None);
}

fn check_snapshot_rejection() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/websocket.json")).unwrap();
    let wire = KalshiWire::default();

    let route = Route {
        name: STREAM,
        path: KALSHI_WS_SIGN_PATH,
        upstream: String::new(),
        endpoint: ENDPOINT,
        headers: &[],
    };

    let mut connection = wire.open(&route, 1);
    let message = |name: &str| Message::Text(fixtures[name].as_str().unwrap().into());
    let (key, reply) = connection.reject(&message("snapshot")).unwrap();
    let reply = serde_json::from_str::<Value>(reply.to_text().unwrap()).unwrap();

    assert_eq!(key, "MARKET-C");
    assert_eq!(reply["id"], json!(4));
    assert_eq!(reply["sid"], json!(11));
    assert_eq!(reply["type"], json!("error"));
    assert_eq!(reply["msg"]["code"], json!(27));
    assert_eq!(wire.attempts("MARKET-C"), 1);

    for name in ["book_subscribe", "add_market", "delete_market"] {
        assert_eq!(connection.reject(&message(name)), None, "boundary {name}");
    }

    assert_eq!(wire.attempts("MARKET-C"), 1);
    assert_eq!(wire.total_attempts(), 1);
}

fn check_streaming_health() {
    let id = parse_instrument_id("MARKET-A").unwrap();

    let mut venue = Kalshi {
        wire: KalshiWire::default(),
        venue_wire: KalshiWire::default(),
        venue_task: None,
        venue_pending: VecDeque::new(),
        venue_checks: HashMap::new(),
        venue_skipped: 0,
        environment: KalshiEnvironment::Demo,
        instruments: vec![id],
        expected_epoch: 1,
        epochs: HashMap::new(),
        verified_batches: HashMap::new(),
        published_updates: HashMap::new(),
        unmatched: 0,
        snapshots_empty: 0,
        snapshots_one_sided: 0,
    };

    let start = BookProgress::default();

    let mut book = BookProgress {
        snapshots: 1,
        batches: 1,
        updates: 0,
    };

    assert!(!venue.streaming(&id, &book, &start));
    venue.epochs.insert(id, 1);
    venue.verified_batches.insert(id, 1);
    assert!(venue.streaming(&id, &book, &start));
    book.batches = 2;
    assert!(!venue.streaming(&id, &book, &start));
    venue
        .wire
        .delta_frames
        .lock()
        .expect("delta frame lock is not poisoned")
        .insert("MARKET-A".to_string(), 2);
    assert!(!venue.publication_ready(&id));
    venue.published_updates.insert(id, 1);
    assert!(venue.publication_ready(&id));

    let mut view = WireView {
        epoch: 1,
        sequence: 1,
        timestamp: 3,
        book: WireBook::default(),
    };

    venue
        .wire
        .clocks
        .lock()
        .unwrap()
        .record("MARKET-A", 1, false, &view);
    venue
        .venue_pending
        .push_back(("MARKET-A".to_string(), 1, view.clone()));
    venue.poll();
    assert_eq!(venue.venue_pending.len(), 1);
    assert!(venue.venue_checks.is_empty());

    view.sequence += 1;
    view.book.apply(&[(Decimal::new(25, 2), Decimal::ONE)], &[]);
    venue
        .wire
        .clocks
        .lock()
        .unwrap()
        .record("MARKET-A", 1, false, &view);
    venue
        .venue_pending
        .push_back(("MARKET-A".to_string(), 1, view.clone()));
    view.sequence += 1;
    venue
        .wire
        .clocks
        .lock()
        .unwrap()
        .record("MARKET-A", 2, false, &view);
    venue.poll();
    assert_eq!(venue.venue_pending.len(), 1);
    assert_eq!(venue.venue_skipped, 1);
    assert!(venue.venue_checks.is_empty());

    view.epoch = 2;
    venue
        .venue_wire
        .clocks
        .lock()
        .unwrap()
        .record("MARKET-A", 1, true, &view);
    venue.poll();
    assert!(venue.venue_pending.is_empty());
    assert_eq!(venue.venue_checks["MARKET-A"], 1);
    venue.finish();
}

fn test_connection(wire: &KalshiWire, epoch: usize) -> KalshiConnection {
    KalshiConnection {
        views: wire.views.clone(),
        clocks: Arc::clone(&wire.clocks),
        attempts: Arc::clone(&wire.attempts),
        delta_frames: Arc::clone(&wire.delta_frames),
        unsubscribes: Arc::clone(&wire.unsubscribes),
        route: STREAM,
        epoch,
        sid: None,
        sequence: 0,
        books: HashMap::new(),
    }
}

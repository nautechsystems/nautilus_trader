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

//! Market-data fault injection with independent order book oracles.
//!
//! Run with adapter credentials unset, except `spot-sbe`, which reads an Ed25519 key from
//! `BINANCE_API_KEY` and `BINANCE_API_SECRET`:
//! `cargo test -p nautilus-binance --features examples --test binance-book-stress -- spot 10 14`
//!
//! Arguments are the product, snapshot timeout seconds, rounds, an optional mode, and an optional
//! book count:
//!
//! - Products: `spot` (Spot mainnet JSON streams), `spot-sbe` (Spot mainnet SBE streams),
//!   `futures` (USD-M testnet), and `coinm` (COIN-M testnet).
//! - Modes: the default rotates gap, reconnect, churn, cut, and freeze faults; `boundaries` probes
//!   deadlines and recovery at the retry ceiling; `resubscribe` races an unsubscribe with an
//!   immediate resubscribe once per round; `quiet` watches `count` thinly traded books for
//!   `rounds` minutes; `crowd` subscribes `count` liquid books and reconnects `rounds` times so
//!   snapshot pacing engages.
//!
//! A local proxy carries the adapter's WebSocket and REST traffic to inject faults, and refuses REST
//! requests before venue request weight nears its limit. Two oracles check every emitted book:
//! `<symbol>@depth20@100ms` read directly from the venue compares the top 20 levels at matching
//! update IDs, and a reference book the proxy rebuilds from every raw diff and the REST snapshots
//! it forwards compares a checksum of the top levels. Every emitted batch also passes through the
//! shared `BookStreamChecker`. No orders are submitted.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt::Display,
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use nautilus_binance::{
    common::{
        consts::BINANCE_CLIENT_ID,
        enums::{BinanceEnvironment, BinanceProductType},
    },
    config::{BinanceDataClientConfig, BinanceSpotMarketDataMode},
    futures::BinanceFuturesDataClient,
    spot::{
        BinanceSpotDataClient,
        http::{BinancePriceLevel, parse::decode_depth},
        sbe::stream::{DepthDiffStreamEvent, MessageHeader, PriceLevel, template_id},
    },
};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    logging::{init_logging, logger::LoggerConfig},
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, UnsubscribeBookDeltas},
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{SocketReconnectRegistry, book::conformance::BookStreamChecker};
use nautilus_model::{
    data::Data,
    enums::{BookAction, BookType},
    identifiers::{InstrumentId, TraderId},
    instruments::Instrument,
};
use nautilus_network::{
    http::{HttpClient, Method},
    mode::ReconnectRequestOutcome,
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{
    Message as UpstreamMessage, client::IntoClientRequest, http::HeaderValue,
};
use ustr::Ustr;

const VIEWS_MAX: usize = 8_192;
const CHECKSUMS_MAX: usize = 32_768;
const REFERENCE_BUFFER_MAX: usize = 20_000;

struct Product {
    name: &'static str,
    symbols: [&'static str; 4],
    instrument_suffix: &'static str,
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
    market_data_mode: BinanceSpotMarketDataMode,
    rest_upstream: &'static str,
    ping_path: &'static str,
    depth_path: &'static str,
    ticker_path: &'static str,
    ticker_suffix: &'static str,
    ws_upstream: &'static str,
    oracle_upstream: &'static str,
    endpoint: &'static str,
    linked_by_pu: bool,
    // Half the REST snapshot depth, where any two valid snapshots agree on every level
    deep_levels: usize,
    // Used weight above which the harness pauses before forcing more snapshots
    weight_guard: u64,
    // Used weight at which the proxy refuses REST requests to protect the IP
    weight_cap: u64,
    weight_limit: u64,
}

const SPOT: Product = Product {
    name: "spot",
    symbols: ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT"],
    instrument_suffix: "",
    product_type: BinanceProductType::Spot,
    environment: BinanceEnvironment::Live,
    market_data_mode: BinanceSpotMarketDataMode::Json,
    rest_upstream: "https://api.binance.com",
    ping_path: "/api/v3/ping",
    depth_path: "/api/v3/depth",
    ticker_path: "/api/v3/ticker/24hr",
    ticker_suffix: "USDT",
    ws_upstream: "wss://stream.binance.com:9443",
    oracle_upstream: "wss://stream.binance.com:9443",
    endpoint: "binance-spot-json-data-streams",
    linked_by_pu: false,
    deep_levels: 2_500,
    weight_guard: 2_500,
    weight_cap: 4_800,
    weight_limit: 6_000,
};

const SPOT_SBE: Product = Product {
    name: "spot-sbe",
    market_data_mode: BinanceSpotMarketDataMode::Sbe,
    ws_upstream: "wss://stream-sbe.binance.com",
    endpoint: "binance-spot-sbe-data-streams",
    ..SPOT
};

const FUTURES: Product = Product {
    name: "futures",
    symbols: ["BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT"],
    instrument_suffix: "-PERP",
    product_type: BinanceProductType::UsdM,
    environment: BinanceEnvironment::Testnet,
    market_data_mode: BinanceSpotMarketDataMode::Json,
    rest_upstream: "https://testnet.binancefuture.com",
    ping_path: "/fapi/v1/ping",
    depth_path: "/fapi/v1/depth",
    ticker_path: "/fapi/v1/ticker/24hr",
    ticker_suffix: "USDT",
    ws_upstream: "wss://fstream.binancefuture.com",
    oracle_upstream: "wss://fstream.binancefuture.com",
    endpoint: "binance-futures-public-streams",
    linked_by_pu: true,
    deep_levels: 500,
    weight_guard: 1_200,
    weight_cap: 2_000,
    weight_limit: 2_400,
};

const COINM: Product = Product {
    name: "coinm",
    symbols: ["BTCUSD_PERP", "ETHUSD_PERP", "XRPUSD_PERP", "SOLUSD_PERP"],
    instrument_suffix: "",
    product_type: BinanceProductType::CoinM,
    ping_path: "/dapi/v1/ping",
    depth_path: "/dapi/v1/depth",
    ticker_path: "/dapi/v1/ticker/24hr",
    ticker_suffix: "_PERP",
    ws_upstream: "wss://dstream.binancefuture.com",
    oracle_upstream: "wss://dstream.binancefuture.com",
    ..FUTURES
};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let _log_guard = init_logging(
        TraderId::from("STRESS-001"),
        UUID4::new(),
        LoggerConfig {
            stdout_level: log::LevelFilter::Info,
            is_colored: false,
            ..LoggerConfig::default()
        },
        Default::default(),
    )
    .unwrap();

    let args = std::env::args().collect::<Vec<_>>();

    let product = match args.get(1).map(String::as_str) {
        Some("spot") | None => &SPOT,
        Some("spot-sbe") => &SPOT_SBE,
        Some("futures") => &FUTURES,
        Some("coinm") => &COINM,
        Some(other) => panic!("unknown product {other}; use spot, spot-sbe, futures, or coinm"),
    };

    let timeout = args.get(2).map_or(10, |v| v.parse::<u64>().unwrap());
    let rounds = args.get(3).map_or(14, |v| v.parse::<usize>().unwrap());
    let mode = args.get(4).map_or("churn", String::as_str);
    let count = args.get(5).map_or(4, |v| v.parse::<usize>().unwrap());
    let ids = product.symbols.map(|s| instrument_id(product, s));

    match mode {
        "churn" => churn(product, timeout, rounds, &ids).await,
        "boundaries" => boundaries(product, timeout, &ids).await,
        "resubscribe" => resubscribe(product, timeout, rounds, &ids).await,
        "quiet" => quiet(product, timeout, rounds, count).await,
        "crowd" => crowd(product, timeout, rounds, count).await,
        other => panic!("unknown mode {other}"),
    }
}

async fn churn(product: &'static Product, timeout: u64, rounds: usize, ids: &[InstrumentId; 4]) {
    let started = Instant::now();
    let mut session = Session::connect(product, timeout, None).await;
    session.watch(ids);

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;

    for round in 0..rounds {
        let phase = round % 7;
        let phase_started = Instant::now();
        session.weight_guard().await;

        run_phase(&mut session, phase, round, ids).await;

        session.observe(Duration::from_secs(5)).await;
        eprintln!(
            "product={} timeout={timeout} round={} phase={phase} phase_ms={} {} elapsed_s={}",
            product.name,
            round + 1,
            phase_started.elapsed().as_millis(),
            session.stats(),
            started.elapsed().as_secs()
        );
    }

    let totals = session.stop().await;
    eprintln!(
        "PASS product={} timeout={timeout} rounds={rounds} instruments={} {totals} elapsed_s={}",
        product.name,
        ids.len(),
        started.elapsed().as_secs()
    );
}

// Runs one churn round's fault scenario, leaving every book healthy
async fn run_phase(session: &mut Session, phase: usize, round: usize, ids: &[InstrumentId; 4]) {
    match phase {
        0 => {
            // Forced gaps recover through REST without reconnecting
            let targets = [ids[round / 7 % 2], ids[2 + round / 7 % 2]];
            let connections = session.wire.connections.load(Ordering::SeqCst);
            let requests = session.snapshot_requests(&targets);
            {
                let mut control = session.wire.control.lock();

                for id in &targets {
                    let fault = control.faults.entry(symbol(id)).or_default();
                    fault.drop = 1;
                    fault.fail = usize::from(round % 2 == 1);
                }
            }

            for id in &targets {
                session.expect_snapshot(*id);
            }

            session.healthy(ids).await;
            assert_eq!(
                session.wire.connections.load(Ordering::SeqCst),
                connections,
                "gap recovery must not reconnect"
            );

            for (id, before) in targets.iter().zip(requests) {
                assert!(session.snapshot_requests(&[*id])[0] > before);
            }
        }
        1 => {
            session.reconnect();
            session.healthy(ids).await;
        }
        2 => {
            // Subscribe churn: settled unsubscribe must stay quiet until resubscribed
            let targets = [ids[round / 7 % 4], ids[(round / 7 + 1) % 4]];
            for id in &targets {
                session.unsubscribe(*id);
            }

            session.observe(Duration::from_secs(3)).await;

            for id in &targets {
                session.subscribe(*id);
            }

            session.healthy(ids).await;
        }
        3 => {
            // Unsubscribe while a recovery snapshot is in flight
            let target = ids[round / 7 % 4];
            let held = session
                .hold_recovery(&[target], Duration::from_secs(3))
                .await;
            session.unsubscribe(target);
            session.observe(Duration::from_secs(4)).await;
            session.release(&[target]);
            assert!(held[0] >= 1);
            session.subscribe(target);
            session.healthy(ids).await;
        }
        4 => {
            // Connection cut while recoveries wait on held snapshots
            let targets = [ids[0], ids[3]];
            session
                .hold_recovery(&targets, Duration::from_secs(3))
                .await;
            let cuts = session.wire.cuts.load(Ordering::SeqCst);
            session.wire.control.lock().cut = true;
            session
                .until(Duration::from_secs(30), "connection cut", |s| {
                    s.wire.cuts.load(Ordering::SeqCst) > cuts
                })
                .await;

            session.release(&targets);
            session.expect_all();
            session.healthy(ids).await;
        }
        5 => {
            // Client reconnect while snapshots are held, then again mid-recovery
            session.delay(ids, Duration::from_secs(2));
            session.reconnect();
            session.observe(Duration::from_secs(1)).await;
            session.reconnect();
            session.observe(Duration::from_secs(1)).await;
            session.release(ids);
            session.healthy(ids).await;
        }
        6 => {
            // Traffic freeze without closing either socket
            let freeze = Duration::from_secs(40);
            *session.wire.freeze_until.lock() = Some(Instant::now() + freeze);
            session.observe(freeze + Duration::from_secs(2)).await;
            session.healthy_updates(ids).await;
        }
        _ => unreachable!(),
    }
}

async fn boundaries(product: &'static Product, timeout: u64, ids: &[InstrumentId; 4]) {
    let started = Instant::now();
    let mut session = Session::connect(product, timeout, Some(0)).await;
    session.watch(ids);

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;

    // Held snapshots expire the deadline twice before a third attempt succeeds
    let target = ids[0];
    let delay = Duration::from_secs(timeout + 1);
    let before = session.snapshot_requests(&[target])[0];
    {
        let mut control = session.wire.control.lock();
        let fault = control.faults.entry(symbol(&target)).or_default();
        fault.delay = Some(delay);
        fault.delays = 2;
        fault.drop = 1;
    }

    session.expect_snapshot(target);
    session.healthy(ids).await;

    let expected = if timeout == 0 { 1 } else { 3 };
    assert_eq!(session.snapshot_requests(&[target])[0], before + expected);
    eprintln!("DEADLINE PASS timeout={timeout} attempts={expected}");

    // An exhausted budget moves to the retry ceiling, which restores the book once the venue does
    let target = ids[1];
    let before = session.snapshot_requests(&[target])[0];
    {
        let mut control = session.wire.control.lock();
        let fault = control.faults.entry(symbol(&target)).or_default();
        fault.fail = usize::MAX;
        fault.drop = 1;
    }

    session
        .until(Duration::from_secs(240), "eight failed attempts", |s| {
            s.snapshot_requests(&[target])[0] >= before + 8
        })
        .await;

    session.observe(Duration::from_secs(2)).await;
    session.suppressed.insert(target);
    session.observe(Duration::from_secs(10)).await;
    assert_eq!(session.snapshot_requests(&[target])[0], before + 8);
    session.suppressed.remove(&target);
    session.release(&[target]);
    session.expect_snapshot(target);
    session.healthy(ids).await;
    assert_eq!(session.snapshot_requests(&[target])[0], before + 9);
    eprintln!("EXHAUSTION PASS attempts=8 recovered_at_ceiling=true");

    // A permanent rejection moves straight to the retry ceiling, which restores the book
    let target = ids[2];
    let before = session.snapshot_requests(&[target])[0];
    {
        let mut control = session.wire.control.lock();
        let fault = control.faults.entry(symbol(&target)).or_default();
        fault.reject = true;
        fault.drop = 1;
    }

    session
        .until(Duration::from_secs(30), "permanent rejection", |s| {
            s.snapshot_requests(&[target])[0] > before
        })
        .await;

    session.observe(Duration::from_secs(2)).await;
    session.suppressed.insert(target);
    session.observe(Duration::from_secs(10)).await;
    assert_eq!(session.snapshot_requests(&[target])[0], before + 1);
    session.suppressed.remove(&target);
    session.release(&[target]);
    session.expect_snapshot(target);
    session.healthy(ids).await;
    assert_eq!(session.snapshot_requests(&[target])[0], before + 2);
    eprintln!("REJECTION PASS attempts=1 recovered_at_ceiling=true");

    session.reconnect();
    session.healthy(ids).await;
    eprintln!("RECONNECT PASS books={}", ids.len());

    let totals = session.stop().await;
    eprintln!(
        "PASS product={} timeout={timeout} boundaries {totals} elapsed_s={}",
        product.name,
        started.elapsed().as_secs()
    );
}

// Resubscribes before the unsubscribe settles, racing the two pool commands
async fn resubscribe(
    product: &'static Product,
    timeout: u64,
    rounds: usize,
    ids: &[InstrumentId; 4],
) {
    let started = Instant::now();
    let mut session = Session::connect(product, timeout, None).await;
    session.watch(ids);

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;

    for round in 0..rounds {
        session.weight_guard().await;
        let target = ids[round % ids.len()];
        session.unsubscribe(target);
        session.subscribe(target);
        session.healthy(&[target]).await;
        eprintln!(
            "product={} round={} target={target} {}",
            product.name,
            round + 1,
            session.stats()
        );
    }

    let totals = session.stop().await;
    eprintln!(
        "PASS product={} timeout={timeout} resubscribe rounds={rounds} {totals} elapsed_s={}",
        product.name,
        started.elapsed().as_secs()
    );
}

// Thin books must sync once their first diff arrives and never fetch a snapshot before it
async fn quiet(product: &'static Product, timeout: u64, minutes: usize, count: usize) {
    let started = Instant::now();
    let mut session = Session::connect(product, timeout, None).await;
    let ids = session.select(count, true).await;
    session.watch(&ids);

    for id in &ids {
        session.subscribe(*id);
    }

    let end = Instant::now() + Duration::from_secs(minutes as u64 * 60);
    while Instant::now() < end {
        session.observe(Duration::from_secs(60)).await;
        eprintln!(
            "product={} quiet elapsed_s={} {}",
            product.name,
            started.elapsed().as_secs(),
            session.stats()
        );
    }

    let mut synced = 0;
    {
        let control = session.wire.control.lock();

        for id in &ids {
            let fault = control.faults.get(&symbol(id));
            let forwarded = fault.map_or(0, |f| f.forwarded);
            let requests = fault.map_or(0, |f| f.requests);
            let first = fault.and_then(|f| f.first_forwarded);
            let snapshots = session.snapshots.get(id).copied().unwrap_or(0);
            eprintln!(
                "quiet book {id}: forwarded_diffs={forwarded} snapshot_requests={requests} \
                 snapshots={snapshots} updates={}",
                session.updates.get(id).copied().unwrap_or(0)
            );

            if forwarded == 0 {
                assert_eq!(requests, 0, "snapshot requested before any diff: {id}");
            } else if first.is_some_and(|first| first.elapsed() >= Duration::from_secs(30)) {
                assert!(snapshots >= 1, "dark quiet book: {id}");
                synced += 1;
            }
        }
    }

    let totals = session.stop().await;
    eprintln!(
        "PASS product={} timeout={timeout} quiet minutes={minutes} books={} synced={synced} \
         {totals} elapsed_s={}",
        product.name,
        ids.len(),
        started.elapsed().as_secs()
    );
}

// Many books resync at once, so snapshot pacing must hold venue weight under its limit
async fn crowd(product: &'static Product, timeout: u64, rounds: usize, count: usize) {
    let started = Instant::now();
    let mut session = Session::connect(product, timeout, None).await;
    let ids = session.select(count, false).await;
    session.watch(&ids);
    let limit = Duration::from_secs(600);

    for id in &ids {
        session.subscribe(*id);
    }

    session.healthy_within(&ids, limit).await;
    eprintln!(
        "product={} crowd books={} initial_sync_ms={} {}",
        product.name,
        ids.len(),
        session.heal_ms_max,
        session.stats()
    );

    for round in 0..rounds {
        session.weight_guard().await;
        let resync_started = Instant::now();
        session.reconnect();
        session.healthy_within(&ids, limit).await;
        eprintln!(
            "product={} crowd round={} resync_ms={} {}",
            product.name,
            round + 1,
            resync_started.elapsed().as_millis(),
            session.stats()
        );
    }

    let totals = session.stop().await;
    eprintln!(
        "PASS product={} timeout={timeout} crowd books={} rounds={rounds} {totals} elapsed_s={}",
        product.name,
        ids.len(),
        started.elapsed().as_secs()
    );
}

fn instrument_id(product: &Product, symbol: &str) -> InstrumentId {
    InstrumentId::from(format!("{symbol}{}.BINANCE", product.instrument_suffix))
}

fn symbol(id: &InstrumentId) -> String {
    id.symbol.as_str().trim_end_matches("-PERP").to_string()
}

#[derive(Default)]
struct Fault {
    drop: usize,
    dropped: usize,
    forwarded: usize,
    first_forwarded: Option<Instant>,
    fail: usize,
    reject: bool,
    delay: Option<Duration>,
    delays: usize,
    requests: usize,
    held: usize,
}

#[derive(Default)]
struct Control {
    faults: HashMap<String, Fault>,
    cut: bool,
    rest_snapshots: HashMap<String, WireSnapshot>,
    references: HashMap<String, Reference>,
    checksums: HashMap<String, VecDeque<(u64, u64)>>,
    reference_gaps: usize,
}

struct Wire {
    product: &'static Product,
    http: HttpClient,
    connections: AtomicUsize,
    active: AtomicUsize,
    cuts: AtomicUsize,
    weight: Mutex<(u64, Instant)>,
    weight_peak: AtomicU64,
    weight_refusals: AtomicUsize,
    throttled: AtomicUsize,
    freeze_until: Mutex<Option<Instant>>,
    control: Mutex<Control>,
}

impl Wire {
    fn record_weight(&self, headers: &HashMap<String, String>) {
        if let Some(used) = headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("x-mbx-used-weight-1m"))
            .and_then(|(_, value)| value.parse::<u64>().ok())
        {
            *self.weight.lock() = (used, Instant::now());
            self.weight_peak.fetch_max(used, Ordering::SeqCst);
        }
    }

    fn used_weight(&self) -> u64 {
        self.weight.lock().0
    }

    // Applies a raw diff to the reference book before any fault drops it
    fn reference(&self, connection: usize, symbol: &str, diff: WireDiff) {
        let mut control = self.control.lock();
        let Control {
            rest_snapshots,
            references,
            checksums,
            reference_gaps,
            ..
        } = &mut *control;
        let reference = references
            .entry(symbol.to_string())
            .or_insert_with(|| Reference::new(connection));

        if reference.connection != connection {
            *reference = Reference::new(connection);
        }

        let history = checksums.entry(symbol.to_string()).or_default();

        if reference.push(diff, rest_snapshots.get(symbol), self.product, history) {
            *reference_gaps += 1;
        }
    }

    // Seeds the reference book from a forwarded snapshot and keeps it for later seeding
    fn record_snapshot(&self, symbol: String, snapshot: WireSnapshot) {
        let mut control = self.control.lock();
        let Control {
            rest_snapshots,
            references,
            checksums,
            ..
        } = &mut *control;

        if let Some(reference) = references.get_mut(&symbol) {
            reference.seed(
                &snapshot,
                self.product,
                checksums.entry(symbol.clone()).or_default(),
            );
        }

        rest_snapshots.insert(symbol, snapshot);
    }

    // Applies any rejection, outage, or hold configured for a depth snapshot request
    async fn inject_depth_faults(&self, symbol: &str) -> Option<Response> {
        let (reject, fail, delay) = {
            let mut control = self.control.lock();
            let fault = control.faults.entry(symbol.to_string()).or_default();
            fault.requests += 1;
            let fail = fault.fail > 0;
            if fail {
                fault.fail -= 1;
            }

            let delay = fault.delay.filter(|_| fault.delays > 0);
            if delay.is_some() {
                fault.delays -= 1;
                fault.held += 1;
            }

            (fault.reject, fail, delay)
        };

        if reject {
            let body = json!({"code": -1121, "msg": "Invalid symbol."}).to_string();
            return Some(
                (
                    StatusCode::BAD_REQUEST,
                    [(header::CONTENT_TYPE, "application/json")],
                    body,
                )
                    .into_response(),
            );
        }

        if fail {
            return Some((StatusCode::SERVICE_UNAVAILABLE, "injected outage").into_response());
        }

        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }

        None
    }
}

struct ProxyConnection(Arc<Wire>);

impl Drop for ProxyConnection {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn ws(
    ws: WebSocketUpgrade,
    State(wire): State<Arc<Wire>>,
    uri: Uri,
    headers: HeaderMap,
) -> impl IntoResponse {
    let url = format!("{}{}", wire.product.ws_upstream, uri.path());
    let api_key = headers
        .get("x-mbx-apikey")
        .and_then(|value| HeaderValue::from_bytes(value.as_bytes()).ok());
    ws.on_upgrade(move |socket| proxy(socket, wire, url, api_key))
}

async fn proxy(mut socket: WebSocket, wire: Arc<Wire>, url: String, api_key: Option<HeaderValue>) {
    let mut request = url.as_str().into_client_request().unwrap();

    // SBE streams authenticate the handshake with the API key
    if let Some(api_key) = api_key {
        request.headers_mut().insert("X-MBX-APIKEY", api_key);
    }

    let (mut upstream, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("venue websocket");
    wire.active.fetch_add(1, Ordering::SeqCst);
    let connection = wire.connections.fetch_add(1, Ordering::SeqCst) + 1;
    let _connection = ProxyConnection(Arc::clone(&wire));
    let mut depth_requests = HashSet::new();

    loop {
        let freeze = *wire.freeze_until.lock();
        if let Some(until) = freeze {
            if Instant::now() < until {
                tokio::time::sleep_until(until.into()).await;
            }

            *wire.freeze_until.lock() = None;
        }

        tokio::select! {
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(frame) = serde_json::from_str::<Value>(&text)
                        && let Some(method) = frame["method"].as_str()
                        && frame["params"].to_string().contains("@depth")
                    {
                        let id = frame["id"].as_u64().unwrap_or_default();
                        depth_requests.insert(id);
                        eprintln!("wire: client {method} {} id={id}", frame["params"]);
                    }

                    if upstream.send(UpstreamMessage::Text(text.to_string().into())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                _ => {}
            },
            message = upstream.next() => {
                let (diff, forward) = match message {
                    Some(Ok(UpstreamMessage::Text(text))) => {
                        let frame = serde_json::from_str::<Value>(&text).ok();

                        if let Some(id) = frame.as_ref().and_then(|frame| frame["id"].as_u64())
                            && depth_requests.remove(&id)
                        {
                            eprintln!("wire: venue response id={id} {}", frame.as_ref().unwrap());
                        }

                        let diff = frame.as_ref().and_then(json_diff);
                        (diff, Message::Text(text.to_string().into()))
                    }
                    Some(Ok(UpstreamMessage::Binary(bytes))) => {
                        (sbe_diff(&bytes), Message::Binary(bytes))
                    }
                    Some(Ok(UpstreamMessage::Close(_)) | Err(_)) | None => break,
                    _ => continue,
                };

                if let Some((id, diff)) = diff {
                    wire.reference(connection, &id, diff);
                    let mut control = wire.control.lock();

                    if std::mem::take(&mut control.cut) {
                        wire.cuts.fetch_add(1, Ordering::SeqCst);
                        break;
                    }

                    let fault = control.faults.entry(id).or_default();
                    if fault.drop > 0 {
                        fault.drop -= 1;
                        fault.dropped += 1;
                        continue;
                    }

                    fault.forwarded += 1;
                    fault.first_forwarded.get_or_insert_with(Instant::now);
                }

                if socket.send(forward).await.is_err() {
                    break;
                }
            }
        }
    }

    let _ = tokio::time::timeout(Duration::from_secs(1), upstream.close(None)).await;
}

async fn rest(State(wire): State<Arc<Wire>>, uri: Uri, headers: HeaderMap) -> Response {
    let path_and_query = uri.path_and_query().map_or(uri.path(), |p| p.as_str());

    let depth_symbol = (uri.path() == wire.product.depth_path).then(|| {
        uri.query()
            .and_then(|query| {
                query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("symbol="))
            })
            .unwrap_or_default()
            .to_string()
    });

    if let Some(symbol) = &depth_symbol
        && let Some(response) = wire.inject_depth_faults(symbol).await
    {
        return response;
    }

    let (used, seen) = *wire.weight.lock();
    if used >= wire.product.weight_cap && seen.elapsed() < Duration::from_secs(10) {
        wire.weight_refusals.fetch_add(1, Ordering::SeqCst);
        eprintln!("weight cap: refusing {path_and_query} at used_weight_1m={used}");
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "10")],
            "harness weight cap",
        )
            .into_response();
    }

    // Forward content negotiation such as the Spot SBE accept headers
    let headers = headers
        .iter()
        .filter(|(name, _)| {
            ![
                header::HOST,
                header::CONNECTION,
                header::CONTENT_LENGTH,
                header::ACCEPT_ENCODING,
            ]
            .contains(name)
        })
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
        .collect::<HashMap<_, _>>();

    let url = format!("{}{path_and_query}", wire.product.rest_upstream);

    let response = match wire
        .http
        .request(Method::GET, url, None, Some(headers), None, Some(30), None)
        .await
    {
        Ok(response) => response,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };

    wire.record_weight(&response.headers);
    let status = response.status.as_u16();

    if status == 429 || status == 418 {
        wire.throttled.fetch_add(1, Ordering::SeqCst);
        eprintln!("venue throttled {path_and_query}: status={status}");
    }

    let content_type = response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .map_or_else(
            || "application/json".to_string(),
            |(_, value)| value.clone(),
        );

    if let Some(symbol) = depth_symbol
        && status == 200
        && let Some(snapshot) = WireSnapshot::parse(&content_type, &response.body)
    {
        wire.record_snapshot(symbol, snapshot);
    }

    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (
        status,
        [(header::CONTENT_TYPE, content_type)],
        response.body.to_vec(),
    )
        .into_response()
}

// One raw diff as the venue sent it, before adapter parsing
struct WireDiff {
    first: u64,
    last: u64,
    prev: Option<u64>,
    bids: Vec<(Decimal, Decimal)>,
    asks: Vec<(Decimal, Decimal)>,
}

impl WireDiff {
    // Futures diffs link through `pu`, Spot diffs start at the next update ID
    fn follows(&self, last: u64, product: &Product) -> bool {
        if product.linked_by_pu {
            self.prev == Some(last)
        } else {
            self.first == last + 1
        }
    }
}

fn json_diff(frame: &Value) -> Option<(String, WireDiff)> {
    // Combined-stream frames carry the payload under `data`
    let payload = frame.get("data").unwrap_or(frame);
    if payload["e"] != "depthUpdate" {
        return None;
    }

    let levels = |side: &str| {
        payload[side]
            .as_array()
            .map(|rows| rows.iter().filter_map(json_level).collect())
            .unwrap_or_default()
    };

    let diff = WireDiff {
        first: payload["U"].as_u64()?,
        last: payload["u"].as_u64()?,
        prev: payload["pu"].as_u64(),
        bids: levels("b"),
        asks: levels("a"),
    };

    Some((payload["s"].as_str()?.to_string(), diff))
}

fn sbe_diff(bytes: &[u8]) -> Option<(String, WireDiff)> {
    let header = MessageHeader::decode(bytes).ok()?;
    if header.template_id != template_id::DEPTH_DIFF_STREAM_EVENT {
        return None;
    }

    let event = DepthDiffStreamEvent::decode(bytes).ok()?;

    let levels = |levels: &[PriceLevel]| {
        levels
            .iter()
            .map(|l| {
                (
                    mantissa_decimal(l.price_mantissa, event.price_exponent),
                    mantissa_decimal(l.qty_mantissa, event.qty_exponent),
                )
            })
            .collect()
    };

    let diff = WireDiff {
        first: event.first_book_update_id as u64,
        last: event.last_book_update_id as u64,
        prev: None,
        bids: levels(&event.bids),
        asks: levels(&event.asks),
    };

    Some((event.symbol.to_string(), diff))
}

fn mantissa_decimal(mantissa: i64, exponent: i8) -> Decimal {
    let value = Decimal::from(mantissa);
    let scale = Decimal::from(10_i64.pow(u32::from(exponent.unsigned_abs())));
    if exponent < 0 {
        value / scale
    } else {
        value * scale
    }
}

fn json_level(row: &Value) -> Option<(Decimal, Decimal)> {
    Some((
        Decimal::from_str(row[0].as_str()?).ok()?,
        Decimal::from_str(row[1].as_str()?).ok()?,
    ))
}

struct WireSnapshot {
    last_update_id: u64,
    book: WireBook,
}

impl WireSnapshot {
    // Spot REST snapshots arrive SBE encoded; Futures snapshots arrive as JSON
    fn parse(content_type: &str, body: &[u8]) -> Option<Self> {
        if content_type.contains("sbe") {
            let depth = decode_depth(body).ok()?;

            let levels = |levels: &[BinancePriceLevel]| {
                levels
                    .iter()
                    .map(|l| {
                        (
                            mantissa_decimal(l.price_mantissa, depth.price_exponent),
                            mantissa_decimal(l.qty_mantissa, depth.qty_exponent),
                        )
                    })
                    .collect::<Vec<_>>()
            };

            let mut book = WireBook::default();
            book.apply(&levels(&depth.bids), &levels(&depth.asks));
            return Some(Self {
                last_update_id: depth.last_update_id as u64,
                book,
            });
        }

        let body = serde_json::from_slice::<Value>(body).ok()?;
        let mut book = WireBook::default();
        book.apply(
            &body["bids"]
                .as_array()?
                .iter()
                .filter_map(json_level)
                .collect::<Vec<_>>(),
            &body["asks"]
                .as_array()?
                .iter()
                .filter_map(json_level)
                .collect::<Vec<_>>(),
        );
        Some(Self {
            last_update_id: body["lastUpdateId"].as_u64()?,
            book,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WireBook {
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
}

impl WireBook {
    fn apply(&mut self, bids: &[(Decimal, Decimal)], asks: &[(Decimal, Decimal)]) {
        for (levels, updates) in [(&mut self.bids, bids), (&mut self.asks, asks)] {
            for (price, size) in updates {
                if size.is_zero() {
                    levels.remove(price);
                } else {
                    levels.insert(*price, *size);
                }
            }
        }
    }

    fn checksum(&self, depth: usize) -> u64 {
        checksum(
            self.bids.iter().rev().take(depth).map(|(p, s)| (*p, *s)),
            self.asks.iter().take(depth).map(|(p, s)| (*p, *s)),
        )
    }
}

fn checksum(
    bids: impl Iterator<Item = (Decimal, Decimal)>,
    asks: impl Iterator<Item = (Decimal, Decimal)>,
) -> u64 {
    let mut hasher = DefaultHasher::new();

    for (side, levels) in [(0_u8, bids.collect::<Vec<_>>()), (1, asks.collect())] {
        side.hash(&mut hasher);

        for (price, size) in levels {
            price.normalize().hash(&mut hasher);
            size.normalize().hash(&mut hasher);
        }
    }

    hasher.finish()
}

// A book rebuilt from raw diffs with the venue's documented rules, independent of the adapter
struct Reference {
    connection: usize,
    buffer: VecDeque<WireDiff>,
    synced: Option<(WireBook, u64)>,
}

impl Reference {
    fn new(connection: usize) -> Self {
        Self {
            connection,
            buffer: VecDeque::new(),
            synced: None,
        }
    }

    // Returns whether the diff broke continuity with the synced book
    fn push(
        &mut self,
        diff: WireDiff,
        snapshot: Option<&WireSnapshot>,
        product: &Product,
        history: &mut VecDeque<(u64, u64)>,
    ) -> bool {
        if let Some((book, last)) = &mut self.synced {
            let stale = diff.last <= *last;

            let linked = diff.follows(*last, product);

            if linked {
                book.apply(&diff.bids, &diff.asks);
                *last = diff.last;
                push_bounded(
                    history,
                    (diff.last, book.checksum(product.deep_levels)),
                    CHECKSUMS_MAX,
                );
                return false;
            }

            if stale {
                return false;
            }

            self.synced = None;
            self.buffer.clear();
            self.buffer.push_back(diff);
            return true;
        }

        push_bounded(&mut self.buffer, diff, REFERENCE_BUFFER_MAX);

        if let Some(snapshot) = snapshot {
            self.seed(snapshot, product, history);
        }

        false
    }

    // Seeds from `snapshot` once a buffered diff bridges it
    fn seed(
        &mut self,
        snapshot: &WireSnapshot,
        product: &Product,
        history: &mut VecDeque<(u64, u64)>,
    ) {
        if self.synced.is_some() {
            return;
        }

        let l = snapshot.last_update_id;

        let Some(start) = self.buffer.iter().position(|diff| {
            if product.linked_by_pu {
                diff.last >= l
            } else {
                diff.last > l
            }
        }) else {
            return;
        };

        let first = &self.buffer[start];

        let bridges = if product.linked_by_pu {
            first.first <= l
        } else {
            first.first <= l + 1
        };

        if !bridges {
            return;
        }

        let mut book = snapshot.book.clone();
        let mut entries = vec![(l, book.checksum(product.deep_levels))];
        let mut last = l;

        for (index, diff) in self.buffer.iter().enumerate().skip(start) {
            let linked = index == start || diff.follows(last, product);

            // A gap inside the buffer needs a newer snapshot
            if !linked {
                return;
            }

            book.apply(&diff.bids, &diff.asks);
            last = diff.last;
            entries.push((last, book.checksum(product.deep_levels)));
        }

        for (update_id, sum) in entries {
            push_bounded(history, (update_id, sum), CHECKSUMS_MAX);
        }

        self.buffer.clear();
        self.synced = Some((book, last));
    }
}

// Appends `item`, dropping the oldest entry once `queue` holds more than `max`
fn push_bounded<T>(queue: &mut VecDeque<T>, item: T, max: usize) {
    queue.push_back(item);

    if queue.len() > max {
        queue.pop_front();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct View {
    update_id: u64,
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
}

// Partial-depth frames read directly from the venue, independent of the adapter's parsing
struct Oracle {
    views: Mutex<HashMap<String, VecDeque<View>>>,
    frames: AtomicUsize,
}

impl Oracle {
    fn watch(self: &Arc<Self>, product: &Product, ids: &[InstrumentId]) {
        let streams = ids
            .iter()
            .map(|id| format!("{}@depth20@100ms", symbol(id).to_lowercase()))
            .collect::<Vec<_>>()
            .join("/");
        let url = format!("{}/stream?streams={streams}", product.oracle_upstream);
        let oracle = Arc::clone(self);

        let task = async move {
            loop {
                match tokio_tungstenite::connect_async(url.as_str()).await {
                    Ok((mut stream, _)) => {
                        while let Some(Ok(message)) = stream.next().await {
                            if let UpstreamMessage::Text(text) = message {
                                oracle.record(&text);
                            }
                        }

                        eprintln!("oracle stream closed, reconnecting");
                    }
                    Err(e) => eprintln!("oracle connect failed: {e}"),
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        };

        tokio::spawn(task); // tokio-import-ok: Standalone runtime
    }

    fn record(&self, text: &str) {
        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return;
        };

        let Some(stream) = frame["stream"].as_str() else {
            return;
        };

        let data = &frame["data"];

        let Some(update_id) = data["lastUpdateId"].as_u64().or_else(|| data["u"].as_u64()) else {
            return;
        };

        let symbol = stream.split('@').next().unwrap_or_default().to_uppercase();

        let levels = |primary: &str, secondary: &str| {
            data[primary]
                .as_array()
                .or_else(|| data[secondary].as_array())
                .map(|rows| {
                    rows.iter()
                        .filter_map(json_level)
                        .filter(|(_, size)| !size.is_zero())
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default()
        };

        let view = View {
            update_id,
            bids: levels("bids", "b"),
            asks: levels("asks", "a"),
        };

        let mut views = self.views.lock();
        push_bounded(views.entry(symbol).or_default(), view, VIEWS_MAX);

        self.frames.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Totals {
    applied: usize,
    snapshots: usize,
    checks: usize,
    unmatched: usize,
    deep_checks: usize,
    deep_unmatched: usize,
    reference_gaps: usize,
    dropped: usize,
    requests: usize,
    connections: usize,
    weight_peak: u64,
}

impl Display for Totals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "applied={} snapshots={} oracle_checks={} oracle_unmatched={} deep_checks={} \
             deep_unmatched={} reference_gaps={} dropped_diffs={} snapshot_requests={} \
             connections={} weight_peak={}",
            self.applied,
            self.snapshots,
            self.checks,
            self.unmatched,
            self.deep_checks,
            self.deep_unmatched,
            self.reference_gaps,
            self.dropped,
            self.requests,
            self.connections,
            self.weight_peak,
        )
    }
}

struct Session {
    product: &'static Product,
    client: Box<dyn DataClient>,
    wire: Arc<Wire>,
    oracle: Arc<Oracle>,
    registry: SocketReconnectRegistry,
    events: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    server: tokio::task::JoinHandle<()>,
    loaded: HashSet<InstrumentId>,
    checker: BookStreamChecker,
    requested: HashSet<InstrumentId>,
    snapshots: HashMap<InstrumentId, usize>,
    expected_snapshots: HashMap<InstrumentId, usize>,
    updates: HashMap<InstrumentId, usize>,
    emitted: HashMap<InstrumentId, usize>,
    managed: HashMap<String, VecDeque<View>>,
    pending: HashMap<String, VecDeque<View>>,
    deep_pending: HashMap<String, VecDeque<(u64, u64)>>,
    disabled: HashSet<InstrumentId>,
    suppressed: HashSet<InstrumentId>,
    applied: usize,
    checks: usize,
    unmatched: usize,
    deep_checks: usize,
    deep_unmatched: usize,
    heal_ms_max: u128,
}

impl Session {
    async fn connect(
        product: &'static Product,
        snapshot_timeout: u64,
        max_retries: Option<u32>,
    ) -> Self {
        let wire = Arc::new(Wire {
            product,
            http: HttpClient::builder()
                .header_keys(vec![
                    "content-type".to_string(),
                    "x-mbx-used-weight-1m".to_string(),
                ])
                .timeout_secs(30)
                .build()
                .unwrap(),
            connections: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            cuts: AtomicUsize::new(0),
            weight: Mutex::new((0, Instant::now())),
            weight_peak: AtomicU64::new(0),
            weight_refusals: AtomicUsize::new(0),
            throttled: AtomicUsize::new(0),
            freeze_until: Mutex::new(None),
            control: Mutex::new(Control::default()),
        });

        let router = Router::new()
            .route("/ws", get(ws))
            .route("/stream", get(ws))
            .fallback(rest)
            .with_state(Arc::clone(&wire));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let serve = async move {
            axum::serve(listener, router).await.unwrap();
        };

        let server = tokio::spawn(serve); // tokio-import-ok: Standalone runtime

        let (sender, events) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let registry = SocketReconnectRegistry::default();

        let defaults = BinanceDataClientConfig::default();

        let config = BinanceDataClientConfig {
            product_type: product.product_type,
            environment: product.environment,
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws: Some(format!("ws://{addr}/ws")),
            spot_market_data_mode: product.market_data_mode,
            instrument_refresh_interval_secs: 0,
            instrument_status_poll_secs: 0,
            book_snapshot_timeout_secs: snapshot_timeout,
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            ..defaults
        };

        let mut client: Box<dyn DataClient> = registry.scope(|| match product.product_type {
            BinanceProductType::Spot => {
                Box::new(BinanceSpotDataClient::new(*BINANCE_CLIENT_ID, config).unwrap())
                    as Box<dyn DataClient>
            }
            _ => Box::new(
                BinanceFuturesDataClient::new(*BINANCE_CLIENT_ID, config, product.product_type)
                    .unwrap(),
            ),
        });

        eprintln!(
            "Connecting {} market data, snapshot_timeout={snapshot_timeout}",
            product.name
        );
        tokio::time::timeout(Duration::from_secs(60), client.connect())
            .await
            .expect("bounded connect")
            .unwrap();

        let mut session = Self {
            product,
            client,
            wire,
            oracle: Arc::new(Oracle {
                views: Mutex::new(HashMap::new()),
                frames: AtomicUsize::new(0),
            }),
            registry,
            events,
            server,
            loaded: HashSet::new(),
            checker: BookStreamChecker::new(BookType::L2_MBP, true),
            requested: HashSet::new(),
            snapshots: HashMap::new(),
            expected_snapshots: HashMap::new(),
            updates: HashMap::new(),
            emitted: HashMap::new(),
            managed: HashMap::new(),
            pending: HashMap::new(),
            deep_pending: HashMap::new(),
            disabled: HashSet::new(),
            suppressed: HashSet::new(),
            applied: 0,
            checks: 0,
            unmatched: 0,
            deep_checks: 0,
            deep_unmatched: 0,
            heal_ms_max: 0,
        };

        session.drain();
        session
    }

    fn watch(&self, ids: &[InstrumentId]) {
        self.oracle.watch(self.product, ids);
    }

    // Picks loaded instruments by 24h trade count: the least active or the most liquid
    async fn select(&self, count: usize, quiet: bool) -> Vec<InstrumentId> {
        let url = format!("{}{}", self.product.rest_upstream, self.product.ticker_path);
        let response = self
            .wire
            .http
            .request(Method::GET, url, None, None, None, Some(30), None)
            .await
            .unwrap();
        self.wire.record_weight(&response.headers);
        let tickers = serde_json::from_slice::<Vec<Value>>(&response.body).unwrap();

        let number = |ticker: &Value, field: &str| {
            ticker[field]
                .as_str()
                .and_then(|value| f64::from_str(value).ok())
                .or_else(|| ticker[field].as_f64())
                .unwrap_or_default()
        };

        let mut candidates = tickers
            .iter()
            .filter_map(|ticker| {
                let raw = ticker["symbol"].as_str()?;
                let id = instrument_id(self.product, raw);
                let trades = ticker["count"].as_u64().unwrap_or_default();
                (raw.ends_with(self.product.ticker_suffix)
                    && self.loaded.contains(&id)
                    && number(ticker, "lastPrice") > 0.0
                    && (!quiet || (100..=3_000).contains(&trades)))
                .then_some((id, trades))
            })
            .collect::<Vec<_>>();

        if quiet {
            candidates.sort_by_key(|(_, trades)| *trades);
        } else {
            candidates.sort_by_key(|(_, trades)| std::cmp::Reverse(*trades));
        }

        let ids = candidates
            .into_iter()
            .take(count)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), count, "not enough candidate books");
        eprintln!("selected books: {ids:?}");
        ids
    }

    fn subscribe(&mut self, id: InstrumentId) {
        self.disabled.remove(&id);
        self.expect_snapshot(id);
        self.requested.insert(id);
        self.checker.open(id);
        self.client
            .subscribe_book_deltas(SubscribeBookDeltas::new(
                id,
                BookType::L2_MBP,
                Some(*BINANCE_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                true,
                None,
                None,
            ))
            .unwrap();
    }

    fn unsubscribe(&mut self, id: InstrumentId) {
        self.client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                id,
                Some(*BINANCE_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        self.drain();
        self.disabled.insert(id);
        self.checker.close(id);
    }

    fn expect_snapshot(&mut self, id: InstrumentId) {
        let snapshots = self.snapshots.get(&id).copied().unwrap_or(0);
        self.expected_snapshots.insert(id, snapshots + 1);
    }

    fn expect_all(&mut self) {
        let ids = self
            .requested
            .iter()
            .filter(|id| !self.disabled.contains(id))
            .copied()
            .collect::<Vec<_>>();

        for id in ids {
            self.expect_snapshot(id);
        }
    }

    fn reconnect(&mut self) {
        self.expect_all();
        let handle = self
            .registry
            .handle(*BINANCE_CLIENT_ID, Ustr::from(self.product.endpoint))
            .unwrap();
        // A request during an in-flight reconnect joins it
        let outcome = handle.request_reconnect();
        assert!(
            matches!(
                outcome,
                ReconnectRequestOutcome::Accepted | ReconnectRequestOutcome::AlreadyReconnecting
            ),
            "reconnect request refused: {outcome:?}"
        );
    }

    // Starts a recovery whose snapshot is held so later actions race it
    async fn hold_recovery(&mut self, ids: &[InstrumentId], delay: Duration) -> Vec<usize> {
        let before = {
            let mut control = self.wire.control.lock();
            ids.iter()
                .map(|id| {
                    let fault = control.faults.entry(symbol(id)).or_default();
                    fault.delay = Some(delay);
                    fault.delays = usize::MAX;
                    fault.drop = 1;
                    fault.held
                })
                .collect::<Vec<_>>()
        };

        for id in ids {
            self.expect_snapshot(*id);
        }

        // Snapshot pacing can queue a recovery behind several other books' snapshots
        self.until(Duration::from_secs(120), "recovery snapshots held", |s| {
            let control = s.wire.control.lock();
            ids.iter()
                .zip(&before)
                .all(|(id, held)| control.faults[&symbol(id)].held > *held)
        })
        .await;

        let control = self.wire.control.lock();
        ids.iter()
            .zip(before)
            .map(|(id, held)| control.faults[&symbol(id)].held - held)
            .collect()
    }

    fn delay(&self, ids: &[InstrumentId], delay: Duration) {
        let mut control = self.wire.control.lock();

        for id in ids {
            let fault = control.faults.entry(symbol(id)).or_default();
            fault.delay = Some(delay);
            fault.delays = usize::MAX;
        }
    }

    fn release(&self, ids: &[InstrumentId]) {
        let mut control = self.wire.control.lock();

        for id in ids {
            if let Some(fault) = control.faults.get_mut(&symbol(id)) {
                fault.delay = None;
                fault.delays = 0;
                fault.fail = 0;
                fault.reject = false;
                fault.drop = 0;
            }
        }
    }

    fn snapshot_requests(&self, ids: &[InstrumentId]) -> Vec<usize> {
        let control = self.wire.control.lock();
        ids.iter()
            .map(|id| control.faults.get(&symbol(id)).map_or(0, |f| f.requests))
            .collect()
    }

    fn apply(&mut self, event: DataEvent) {
        let deltas = match event {
            DataEvent::Instrument(instrument) => {
                self.loaded.insert(instrument.id());
                return;
            }
            DataEvent::Data(Data::BookDeltas(deltas)) => deltas,
            _ => return,
        };

        let id = deltas.instrument_id;
        assert!(
            !self.suppressed.contains(&id),
            "output while the probe expects the book suppressed: {id}"
        );
        let snapshot = deltas
            .deltas
            .first()
            .is_some_and(|delta| delta.action == BookAction::Clear);

        if let Err(violation) = self.checker.apply(&deltas) {
            panic!(
                "book contract violation {id} seq={}: {violation}",
                deltas.sequence
            );
        }

        if snapshot {
            *self.snapshots.entry(id).or_default() += 1;
            self.updates.insert(id, 0);
        } else {
            *self.updates.entry(id).or_default() += 1;
        }

        *self.emitted.entry(id).or_default() += 1;
        let book = self.checker.book(id).expect("requested book");

        let view = View {
            update_id: deltas.sequence,
            bids: book.bids_as_map(Some(20)).into_iter().collect(),
            asks: book.asks_as_map(Some(20)).into_iter().collect(),
        };

        let sum = checksum(
            book.bids_as_map(Some(self.product.deep_levels)).into_iter(),
            book.asks_as_map(Some(self.product.deep_levels)).into_iter(),
        );
        push_bounded(
            self.deep_pending.entry(symbol(&id)).or_default(),
            (deltas.sequence, sum),
            CHECKSUMS_MAX,
        );
        push_bounded(
            self.managed.entry(symbol(&id)).or_default(),
            view,
            VIEWS_MAX,
        );

        self.applied += 1;
    }

    // Compares emitted books with both oracles at matching update IDs
    fn reconcile(&mut self) {
        {
            let mut views = self.oracle.views.lock();
            for (symbol, views) in views.iter_mut() {
                let pending = self.pending.entry(symbol.clone()).or_default();
                pending.extend(views.drain(..));
                while pending.len() > VIEWS_MAX {
                    pending.pop_front();
                }
            }
        }

        for (symbol, pending) in &mut self.pending {
            let Some(managed) = self.managed.get(symbol) else {
                pending.clear();
                continue;
            };

            let (checks, unmatched) = match_pending(
                pending,
                managed,
                |view| view.update_id,
                |view, emitted| {
                    assert_eq!(
                        emitted.bids, view.bids,
                        "bid oracle mismatch {symbol} update_id={}",
                        view.update_id
                    );
                    assert_eq!(
                        emitted.asks, view.asks,
                        "ask oracle mismatch {symbol} update_id={}",
                        view.update_id
                    );
                },
            );

            self.checks += checks;
            self.unmatched += unmatched;
        }

        let control = self.wire.control.lock();

        for (symbol, pending) in &mut self.deep_pending {
            let Some(history) = control.checksums.get(symbol) else {
                continue;
            };

            let (checks, unmatched) = match_pending(
                pending,
                history,
                |(update_id, _)| *update_id,
                |(update_id, sum), (_, expected)| {
                    assert_eq!(
                        sum, expected,
                        "reference book mismatch {symbol} update_id={update_id}"
                    );
                },
            );

            self.deep_checks += checks;
            self.deep_unmatched += unmatched;
        }
    }

    async fn until(&mut self, limit: Duration, label: &str, predicate: impl Fn(&Self) -> bool) {
        let result = tokio::time::timeout(limit, async {
            let mut reconciled = Instant::now();

            while !predicate(self) {
                tokio::select! {
                    event = self.events.recv() => self.apply(event.expect("data stream stays open")),
                    () = tokio::time::sleep(Duration::from_millis(10)) => {}
                }

                // Busy streams must not starve the oracle comparisons
                if reconciled.elapsed() >= Duration::from_millis(50) {
                    self.reconcile();
                    reconciled = Instant::now();
                }
            }
        })
        .await;

        if result.is_err() {
            self.reconcile();
            let control = self.wire.control.lock();

            let faults = control
                .faults
                .iter()
                .map(|(id, f)| {
                    format!(
                        "{id}: forwarded={} dropped={} requests={} held={}",
                        f.forwarded, f.dropped, f.requests, f.held
                    )
                })
                .collect::<Vec<_>>();

            panic!(
                "deadline exceeded: {label}, updates={:?}, snapshots={:?}, expected={:?}, \
                 wire={faults:?}",
                self.updates, self.snapshots, self.expected_snapshots
            );
        }

        self.drain();
    }

    fn drain(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.apply(event);
        }

        self.reconcile();
    }

    async fn observe(&mut self, duration: Duration) {
        let end = Instant::now() + duration;
        self.until(
            duration + Duration::from_secs(2),
            "observation window",
            |_| Instant::now() >= end,
        )
        .await;
    }

    async fn healthy(&mut self, ids: &[InstrumentId]) {
        self.healthy_within(ids, Duration::from_secs(120)).await;
    }

    // Every intended book reaches its expected snapshot and then streams updates
    async fn healthy_within(&mut self, ids: &[InstrumentId], limit: Duration) {
        let started = Instant::now();
        let ids = ids
            .iter()
            .filter(|id| !self.disabled.contains(id))
            .copied()
            .collect::<Vec<_>>();
        self.until(limit, "all intended books recover", |s| {
            ids.iter().all(|id| {
                s.snapshots.get(id).copied().unwrap_or(0) >= s.expected_snapshots[id]
                    && s.updates.get(id).copied().unwrap_or(0) >= 3
            })
        })
        .await;

        self.heal_ms_max = self.heal_ms_max.max(started.elapsed().as_millis());
    }

    async fn healthy_updates(&mut self, ids: &[InstrumentId]) {
        let before = ids
            .iter()
            .map(|id| (*id, self.emitted.get(id).copied().unwrap_or(0)))
            .collect::<Vec<_>>();
        self.until(Duration::from_secs(120), "books stream after freeze", |s| {
            before
                .iter()
                .all(|(id, emitted)| s.emitted.get(id).copied().unwrap_or(0) >= emitted + 3)
        })
        .await;
    }

    // Keeps venue REST weight well inside the per-minute limit before forcing snapshots
    async fn weight_guard(&self) {
        loop {
            let url = format!("{}{}", self.product.rest_upstream, self.product.ping_path);

            if let Ok(response) = self
                .wire
                .http
                .request(Method::GET, url, None, None, None, Some(10), None)
                .await
            {
                self.wire.record_weight(&response.headers);
            }

            let weight = self.wire.used_weight();
            if weight < self.product.weight_guard {
                return;
            }

            eprintln!("weight guard: used_weight_1m={weight}, waiting");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    fn totals(&self) -> Totals {
        let control = self.wire.control.lock();

        Totals {
            applied: self.applied,
            snapshots: self.snapshots.values().sum(),
            checks: self.checks,
            unmatched: self.unmatched,
            deep_checks: self.deep_checks,
            deep_unmatched: self.deep_unmatched,
            reference_gaps: control.reference_gaps,
            dropped: control.faults.values().map(|f| f.dropped).sum(),
            requests: control.faults.values().map(|f| f.requests).sum(),
            connections: self.wire.connections.load(Ordering::SeqCst),
            weight_peak: self.wire.weight_peak.load(Ordering::SeqCst),
        }
    }

    fn stats(&self) -> String {
        format!(
            "{} cuts={} heal_ms_max={} used_weight_1m={} oracle_frames={}",
            self.totals(),
            self.wire.cuts.load(Ordering::SeqCst),
            self.heal_ms_max,
            self.wire.used_weight(),
            self.oracle.frames.load(Ordering::Relaxed),
        )
    }

    async fn stop(mut self) -> Totals {
        let started = Instant::now();
        tokio::time::timeout(Duration::from_secs(10), self.client.disconnect())
            .await
            .expect("bounded disconnect")
            .unwrap();
        assert!(self.client.is_disconnected());

        let deadline = Instant::now() + Duration::from_secs(5);

        while self.wire.active.load(Ordering::SeqCst) > 0 {
            assert!(Instant::now() < deadline, "proxied sockets closed");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert!(
            self.registry
                .handle(*BINANCE_CLIENT_ID, Ustr::from(self.product.endpoint))
                .is_none()
        );

        let totals = self.totals();
        eprintln!(
            "shutdown_ms={} active_proxies=0 {}",
            started.elapsed().as_millis(),
            self.stats()
        );
        assert_eq!(
            self.wire.weight_refusals.load(Ordering::SeqCst),
            0,
            "proxy refused requests at the weight cap"
        );
        assert_eq!(
            self.wire.throttled.load(Ordering::SeqCst),
            0,
            "venue throttled REST requests"
        );
        assert!(totals.weight_peak < self.product.weight_limit);
        assert!(
            totals.checks > 0,
            "depth20 oracle compared no emitted books"
        );
        assert!(
            totals.deep_checks > 0,
            "reference oracle compared no emitted books"
        );
        self.server.abort();
        totals
    }
}

// Pops each pending entry once the history reaches its update ID: an entry the history holds is
// checked against it, and one the history passed without holding counts as unmatched
fn match_pending<T>(
    pending: &mut VecDeque<T>,
    history: &VecDeque<T>,
    update_id: impl Fn(&T) -> u64,
    mut check: impl FnMut(&T, &T),
) -> (usize, usize) {
    let Some(newest) = history.back().map(&update_id) else {
        return (0, 0);
    };

    let mut checks = 0;
    let mut unmatched = 0;

    while let Some(entry) = pending.front() {
        let id = update_id(entry);

        if let Some(expected) = history.iter().rev().find(|item| update_id(item) == id) {
            check(entry, expected);
            checks += 1;
        } else if id <= newest {
            unmatched += 1;
        } else {
            break;
        }

        pending.pop_front();
    }

    (checks, unmatched)
}

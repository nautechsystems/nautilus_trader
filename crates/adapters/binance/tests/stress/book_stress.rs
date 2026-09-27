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
//! `cargo test -p nautilus-binance --features examples --test binance-book-stress -- --product futures`
//!
//! Select the product with `--product`: `spot` (default, Spot mainnet JSON streams), `spot-sbe`
//! (Spot mainnet SBE streams), `futures` (USD-M testnet), or `coinm` (COIN-M testnet). Scenarios,
//! selected with `--scenario`:
//!
//! - `churn` (default): rotates gap, reconnect, churn, cut, and freeze faults over `--rounds`.
//! - `boundaries`: probes deadlines and recovery at the retry ceiling.
//! - `resubscribe`: races an unsubscribe with an immediate resubscribe once per round.
//! - `quiet`: watches `--books` thinly traded books for `--rounds` minutes.
//! - `crowd`: subscribes `--books` liquid books and reconnects `--rounds` times so snapshot pacing
//!   engages.
//!
//! The shared fault proxy carries the adapter's WebSocket traffic, and a REST proxy carries its
//! snapshot requests, refusing them before venue request weight nears its limit. Two oracles check
//! every emitted book: `<symbol>@depth20@100ms` read directly from the venue compares the top 20
//! levels at matching update IDs, and a reference book rebuilt from every raw diff and the REST
//! snapshots the proxy forwards compares a checksum of the top levels. Every emitted batch also
//! passes through the shared `BookStreamChecker`. No orders are submitted.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
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
use nautilus_common::clients::DataClient;
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::http::{HttpClient, Method};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, Flag, FrameKind, Route, Session, StressArgs, StressVenue, Upstream,
    WireBook, WireCodec, WireConnection,
};
use tokio_tungstenite::tungstenite::Message;

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

const PRODUCTS: [&Product; 4] = [&SPOT, &SPOT_SBE, &FUTURES, &COINM];

type BinanceSession = Session<Binance>;

fn main() {
    stress::run::<Binance, _, _>(|args| async move {
        let product = product(&args);
        let ids = product.symbols.map(|s| instrument_id(product, s));

        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
            "resubscribe" => resubscribe(&args, &ids).await,
            "quiet" => quiet(&args).await,
            "crowd" => crowd(&args).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId; 4]) -> String {
    let mut session = BinanceSession::connect(args).await;
    session.venue().watch(ids);

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;

    for round in 0..args.rounds() {
        let phase = round % 7;
        let phase_started = Instant::now();
        session.venue().weight_guard().await;

        run_phase(&mut session, phase, round, ids).await;

        session.observe(Duration::from_secs(5)).await;
        session.round(
            round,
            &format!(
                "phase={phase} phase_ms={}",
                phase_started.elapsed().as_millis()
            ),
        );
    }

    let stats = session.stop().await;
    format!("books={} {stats}", ids.len())
}

// Runs one churn round's fault scenario, leaving every book healthy
async fn run_phase(
    session: &mut BinanceSession,
    phase: usize,
    round: usize,
    ids: &[InstrumentId; 4],
) {
    match phase {
        0 => {
            // Forced gaps recover through REST without reconnecting
            let targets = [ids[round / 7 % 2], ids[2 + round / 7 % 2]];
            let connections = session.proxy().connections_total();
            let requests = session.venue().snapshot_requests(&targets);

            for id in &targets {
                session.venue().depth(id).fail = usize::from(round % 2 == 1);
                session.fault(id).drop_updates = 1;
            }

            for id in &targets {
                session.expect_snapshot(*id);
            }

            session.healthy(ids).await;
            assert_eq!(
                session.proxy().connections_total(),
                connections,
                "gap recovery must not reconnect"
            );

            for (id, before) in targets.iter().zip(requests) {
                assert!(session.venue().snapshot_requests(&[*id])[0] > before);
            }
        }
        1 => {
            reconnect(session);
            session.healthy(ids).await;
        }
        2 => {
            // Subscribe churn: settled unsubscribe must stay quiet until resubscribed
            let targets = [ids[round / 7 % 4], ids[(round / 7 + 1) % 4]];

            for id in &targets {
                unsubscribe(session, *id);
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
            let held = hold_recovery(session, &[target], Duration::from_secs(3)).await;
            unsubscribe(session, target);
            session.observe(Duration::from_secs(4)).await;
            release(session, &[target]);
            assert!(held[0] >= 1);
            session.subscribe(target);
            session.healthy(ids).await;
        }
        4 => {
            // Connection cut while recoveries wait on held snapshots
            let targets = [ids[0], ids[3]];
            hold_recovery(session, &targets, Duration::from_secs(3)).await;
            let cuts = session.proxy().cuts();
            session.proxy().cut(None, FrameKind::Update, 1);
            session
                .until(Duration::from_secs(30), "connection cut", |s| {
                    s.proxy().cuts() > cuts
                })
                .await;

            release(session, &targets);
            session.expect_all();
            session.healthy(ids).await;
        }
        5 => {
            // Client reconnect while snapshots are held, then again mid-recovery
            delay(session, ids, Duration::from_secs(2));
            reconnect(session);
            session.observe(Duration::from_secs(1)).await;
            reconnect(session);
            session.observe(Duration::from_secs(1)).await;
            release(session, ids);
            session.healthy(ids).await;
        }
        6 => {
            // Traffic freeze without closing either socket
            let freeze = Duration::from_secs(40);
            session.proxy().freeze(freeze);
            session.observe(freeze + Duration::from_secs(2)).await;
            healthy_updates(session, ids).await;
        }
        _ => unreachable!(),
    }
}

async fn boundaries(args: &StressArgs, ids: &[InstrumentId; 4]) -> String {
    let timeout = args.timeout_secs();
    let mut session = BinanceSession::connect(args).await;
    session.venue().watch(ids);

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;

    // Held snapshots expire the deadline twice before a third attempt succeeds
    let target = ids[0];
    let before = session.venue().snapshot_requests(&[target])[0];
    {
        let mut depth = session.venue().depth(&target);
        depth.delay = Some(Duration::from_secs(timeout + 1));
        depth.delays = 2;
    }

    session.fault(&target).drop_updates = 1;
    session.expect_snapshot(target);
    session.healthy(ids).await;

    let expected = if timeout == 0 { 1 } else { 3 };
    assert_eq!(
        session.venue().snapshot_requests(&[target])[0],
        before + expected
    );
    stress::check("deadline", format!("attempts={expected}"));

    // An exhausted budget moves to the retry ceiling, which restores the book once the venue does
    let target = ids[1];
    let before = session.venue().snapshot_requests(&[target])[0];
    session.venue().depth(&target).fail = usize::MAX;
    session.fault(&target).drop_updates = 1;

    session
        .until(Duration::from_secs(240), "eight failed attempts", |s| {
            s.venue().snapshot_requests(&[target])[0] >= before + 8
        })
        .await;

    session.observe(Duration::from_secs(2)).await;
    session.venue_mut().suppressed.insert(target);
    session.observe(Duration::from_secs(10)).await;
    assert_eq!(session.venue().snapshot_requests(&[target])[0], before + 8);
    session.venue_mut().suppressed.remove(&target);
    release(&session, &[target]);
    session.expect_snapshot(target);
    session.healthy(ids).await;
    assert_eq!(session.venue().snapshot_requests(&[target])[0], before + 9);
    stress::check("exhaustion", "attempts=8 recovered_at_ceiling=true");

    // A permanent rejection moves straight to the retry ceiling, which restores the book
    let target = ids[2];
    let before = session.venue().snapshot_requests(&[target])[0];
    session.venue().depth(&target).reject = true;
    session.fault(&target).drop_updates = 1;

    session
        .until(Duration::from_secs(30), "permanent rejection", |s| {
            s.venue().snapshot_requests(&[target])[0] > before
        })
        .await;

    session.observe(Duration::from_secs(2)).await;
    session.venue_mut().suppressed.insert(target);
    session.observe(Duration::from_secs(10)).await;
    assert_eq!(session.venue().snapshot_requests(&[target])[0], before + 1);
    session.venue_mut().suppressed.remove(&target);
    release(&session, &[target]);
    session.expect_snapshot(target);
    session.healthy(ids).await;
    assert_eq!(session.venue().snapshot_requests(&[target])[0], before + 2);
    stress::check("rejection", "attempts=1 recovered_at_ceiling=true");

    reconnect(&mut session);
    session.healthy(ids).await;
    stress::check("reconnect", format!("books={}", ids.len()));

    session.stop().await
}

// Resubscribes before the unsubscribe settles, racing the two pool commands
async fn resubscribe(args: &StressArgs, ids: &[InstrumentId; 4]) -> String {
    let mut session = BinanceSession::connect(args).await;
    session.venue().watch(ids);

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;

    for round in 0..args.rounds() {
        session.venue().weight_guard().await;
        let target = ids[round % ids.len()];
        unsubscribe(&mut session, target);
        session.subscribe(target);
        session.healthy(&[target]).await;
        session.round(round, &format!("target={target}"));
    }

    session.stop().await
}

// Thin books must sync once their first diff arrives and never fetch a snapshot before it
async fn quiet(args: &StressArgs) -> String {
    let mut session = BinanceSession::connect(args).await;
    let ids = select(&session, books(args), true).await;
    session.venue().watch(&ids);

    for id in &ids {
        session.subscribe(*id);
    }

    // Each round observes one minute
    for round in 0..args.rounds() {
        session.observe(Duration::from_secs(60)).await;
        session.round(round, "");
    }

    let mut synced = 0;

    for id in &ids {
        let fault = session.fault(id).clone();
        let requests = session.venue().snapshot_requests(&[*id])[0];
        let book = session.book(id);

        if fault.forwarded == 0 {
            assert_eq!(requests, 0, "snapshot requested before any diff: {id}");
        } else if fault
            .first_forwarded
            .is_some_and(|first| first.elapsed() >= Duration::from_secs(30))
        {
            assert!(book.snapshots >= 1, "dark quiet book: {id}");
            synced += 1;
        }

        stress::check(
            "quiet_book",
            format!(
                "instrument={id} forwarded_diffs={} snapshot_requests={requests} snapshots={} \
                 updates={}",
                fault.forwarded, book.snapshots, book.updates
            ),
        );
    }

    let stats = session.stop().await;
    format!("books={} synced={synced} {stats}", ids.len())
}

// Many books resync at once, so snapshot pacing must hold venue weight under its limit
async fn crowd(args: &StressArgs) -> String {
    let mut session = BinanceSession::connect(args).await;
    let ids = select(&session, books(args), false).await;
    session.venue().watch(&ids);
    let limit = Duration::from_secs(600);

    for id in &ids {
        session.subscribe(*id);
    }

    let sync_started = Instant::now();
    session.healthy_within(&ids, limit).await;
    stress::check(
        "initial_sync",
        format!(
            "books={} initial_sync_ms={}",
            ids.len(),
            sync_started.elapsed().as_millis()
        ),
    );

    for round in 0..args.rounds() {
        session.venue().weight_guard().await;
        let resync_started = Instant::now();
        reconnect(&mut session);
        session.healthy_within(&ids, limit).await;
        session.round(
            round,
            &format!("resync_ms={}", resync_started.elapsed().as_millis()),
        );
    }

    let stats = session.stop().await;
    format!("books={} {stats}", ids.len())
}

// Picks loaded instruments by 24h trade count: the least active or the most liquid
async fn select(session: &BinanceSession, count: usize, quiet: bool) -> Vec<InstrumentId> {
    let venue = session.venue();
    let product = venue.product;
    let url = format!("{}{}", product.rest_upstream, product.ticker_path);
    let response = venue
        .wire
        .http
        .request(Method::GET, url, None, None, None, Some(30), None)
        .await
        .unwrap();
    venue.wire.record_weight(&response.headers);
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
            let id = instrument_id(product, raw);
            let trades = ticker["count"].as_u64().unwrap_or_default();
            (raw.ends_with(product.ticker_suffix)
                && session.instruments().contains(&id)
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
    eprintln!("Selected books: {ids:?}");
    ids
}

// Unsubscribes and treats the unsubscribe as settled once queued output is applied
fn unsubscribe(session: &mut BinanceSession, id: InstrumentId) {
    session.unsubscribe(id);
    session.close(id);
}

fn reconnect(session: &mut BinanceSession) {
    let endpoint = session.venue().product.endpoint;
    session.reconnect(endpoint);
}

// Starts a recovery whose snapshot is held so later actions race it
async fn hold_recovery(
    session: &mut BinanceSession,
    ids: &[InstrumentId],
    delay: Duration,
) -> Vec<usize> {
    let before = ids
        .iter()
        .map(|id| {
            let mut depth = session.venue().depth(id);
            depth.delay = Some(delay);
            depth.delays = usize::MAX;
            depth.held
        })
        .collect::<Vec<_>>();

    // The REST hold is armed before the dropped diff forces the gap that requests a snapshot
    for id in ids {
        session.fault(id).drop_updates = 1;
        session.expect_snapshot(*id);
    }

    // Snapshot pacing can queue a recovery behind several other books' snapshots
    session
        .until(Duration::from_secs(120), "recovery snapshots held", |s| {
            ids.iter()
                .zip(&before)
                .all(|(id, held)| s.venue().depth(id).held > *held)
        })
        .await;

    ids.iter()
        .zip(before)
        .map(|(id, held)| session.venue().depth(id).held - held)
        .collect()
}

fn delay(session: &BinanceSession, ids: &[InstrumentId], delay: Duration) {
    for id in ids {
        let mut depth = session.venue().depth(id);
        depth.delay = Some(delay);
        depth.delays = usize::MAX;
    }
}

fn release(session: &BinanceSession, ids: &[InstrumentId]) {
    for id in ids {
        {
            let mut depth = session.venue().depth(id);
            depth.delay = None;
            depth.delays = 0;
            depth.fail = 0;
            depth.reject = false;
        }

        session.fault(id).drop_updates = 0;
    }
}

async fn healthy_updates(session: &mut BinanceSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| (*id, session.book(id).batches))
        .collect::<Vec<_>>();
    session
        .until(Duration::from_secs(120), "books stream after freeze", |s| {
            before
                .iter()
                .all(|(id, batches)| s.book(id).batches >= batches + 3)
        })
        .await;
}

fn product(args: &StressArgs) -> &'static Product {
    let name = args.flag("product");
    PRODUCTS
        .into_iter()
        .find(|product| product.name == name)
        .unwrap_or_else(|| panic!("unknown product {name}; use spot, spot-sbe, futures, or coinm"))
}

fn books(args: &StressArgs) -> usize {
    args.flag("books")
        .parse()
        .unwrap_or_else(|_| panic!("--books takes a count, was {}", args.flag("books")))
}

fn instrument_id(product: &Product, symbol: &str) -> InstrumentId {
    InstrumentId::from(format!("{symbol}{}.BINANCE", product.instrument_suffix))
}

fn symbol(id: &InstrumentId) -> String {
    id.symbol.as_str().trim_end_matches("-PERP").to_string()
}

struct Binance {
    product: &'static Product,
    wire: Arc<BinanceWire>,
    oracle: Arc<Oracle>,
    managed: HashMap<String, VecDeque<View>>,
    pending: HashMap<String, VecDeque<View>>,
    deep_pending: HashMap<String, VecDeque<(u64, u64)>>,
    suppressed: HashSet<InstrumentId>,
    checks: usize,
    unmatched: usize,
    deep_checks: usize,
    deep_unmatched: usize,
}

impl Binance {
    fn watch(&self, ids: &[InstrumentId]) {
        self.oracle.watch(self.product, ids);
    }

    fn depth(&self, id: &InstrumentId) -> MappedMutexGuard<'_, DepthFault> {
        MutexGuard::map(self.wire.control.lock(), |control| {
            control.depth.entry(symbol(id)).or_default()
        })
    }

    fn snapshot_requests(&self, ids: &[InstrumentId]) -> Vec<usize> {
        let control = self.wire.control.lock();
        ids.iter()
            .map(|id| control.depth.get(&symbol(id)).map_or(0, |f| f.requests))
            .collect()
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

            eprintln!("Weight guard: used_weight_1m={weight}, waiting");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}

impl StressVenue for Binance {
    const NAME: &'static str = "binance";
    const SCENARIOS: &'static [&'static str] =
        &["churn", "boundaries", "resubscribe", "quiet", "crowd"];
    const ROUNDS: usize = 14;
    const FLAGS: &'static [Flag] = &[
        Flag {
            name: "product",
            default: "spot",
            help: "spot, spot-sbe, futures, or coinm",
        },
        Flag {
            name: "books",
            default: "4",
            help: "Books the quiet and crowd scenarios select",
        },
    ];
    const SEQUENCED: bool = true;
    // The oracles match emitted books by update ID after the fact
    const COVERAGE: Coverage = Coverage::Samples;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(120);

    fn self_check() {
        check_oracles();
    }

    fn new(args: &StressArgs) -> Self {
        let product = product(args);

        Self {
            product,
            wire: Arc::new(BinanceWire {
                product,
                http: HttpClient::builder()
                    .header_keys(vec![
                        "content-type".to_string(),
                        "x-mbx-used-weight-1m".to_string(),
                    ])
                    .timeout_secs(30)
                    .build()
                    .unwrap(),
                weight: Mutex::new((0, Instant::now())),
                weight_peak: AtomicU64::new(0),
                weight_refusals: AtomicUsize::new(0),
                throttled: AtomicUsize::new(0),
                control: Mutex::new(Control::default()),
            }),
            oracle: Arc::new(Oracle {
                views: Mutex::new(HashMap::new()),
                frames: AtomicUsize::new(0),
            }),
            managed: HashMap::new(),
            pending: HashMap::new(),
            deep_pending: HashMap::new(),
            suppressed: HashSet::new(),
            checks: 0,
            unmatched: 0,
            deep_checks: 0,
            deep_unmatched: 0,
        }
    }

    fn client_id(&self) -> ClientId {
        *BINANCE_CLIENT_ID
    }

    // The Spot JSON client normalizes its URL to the combined `/stream` endpoint
    fn routes(&self) -> Vec<Route> {
        [("ws", "/ws"), ("stream", "/stream")]
            .into_iter()
            .map(|(name, path)| Route {
                name,
                path,
                upstream: format!("{}{path}", self.product.ws_upstream),
                endpoint: self.product.endpoint,
                // SBE streams authenticate the handshake with the API key
                headers: &["x-mbx-apikey"],
            })
            .collect()
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(BinanceCodec(Arc::clone(&self.wire)))
    }

    fn router(&self) -> Option<Router> {
        Some(
            Router::new()
                .fallback(rest)
                .with_state(Arc::clone(&self.wire)),
        )
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let defaults = BinanceDataClientConfig::default();

        let config = BinanceDataClientConfig {
            product_type: self.product.product_type,
            environment: self.product.environment,
            base_url_http: Some(format!("http://{proxy}")),
            base_url_ws: Some(format!("ws://{proxy}/ws")),
            spot_market_data_mode: self.product.market_data_mode,
            instrument_refresh_interval_secs: 0,
            instrument_status_poll_secs: 0,
            book_snapshot_timeout_secs: args.timeout_secs(),
            // Boundary probes count recovery attempts by REST requests, so requests must not retry
            max_retries: if args.scenario() == "boundaries" {
                0
            } else {
                defaults.max_retries
            },
            ..defaults
        };

        Ok(match self.product.product_type {
            BinanceProductType::Spot => {
                Box::new(BinanceSpotDataClient::new(*BINANCE_CLIENT_ID, config)?)
            }
            product_type => Box::new(BinanceFuturesDataClient::new(
                *BINANCE_CLIENT_ID,
                config,
                product_type,
            )?),
        })
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        symbol(instrument_id)
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        assert!(
            !self.suppressed.contains(&id),
            "output while the probe expects the book suppressed: {id}"
        );
        let book = checker.book(id).expect("requested book");

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
    }

    fn streaming(&self, _id: &InstrumentId, book: &BookProgress, _start: &BookProgress) -> bool {
        book.updates >= 3
    }

    // Compares emitted books with both oracles at matching update IDs
    fn poll(&mut self) {
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

    fn stats(&self) -> String {
        let control = self.wire.control.lock();

        format!(
            "oracle_checks={} oracle_unmatched={} deep_checks={} deep_unmatched={} \
             reference_gaps={} snapshot_requests={} weight_peak={} used_weight_1m={} \
             oracle_frames={}",
            self.checks,
            self.unmatched,
            self.deep_checks,
            self.deep_unmatched,
            control.reference_gaps,
            control.depth.values().map(|f| f.requests).sum::<usize>(),
            self.wire.weight_peak.load(Ordering::SeqCst),
            self.wire.used_weight(),
            self.oracle.frames.load(Ordering::Relaxed),
        )
    }

    fn finish(&mut self) {
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
        assert!(self.wire.weight_peak.load(Ordering::SeqCst) < self.product.weight_limit);
        assert!(self.checks > 0, "depth20 oracle compared no emitted books");
        assert!(
            self.deep_checks > 0,
            "reference oracle compared no emitted books"
        );
    }
}

// Faults and counters for one book's REST depth snapshot requests
#[derive(Default)]
struct DepthFault {
    fail: usize,
    reject: bool,
    delay: Option<Duration>,
    delays: usize,
    requests: usize,
    held: usize,
}

#[derive(Default)]
struct Control {
    depth: HashMap<String, DepthFault>,
    rest_snapshots: HashMap<String, WireSnapshot>,
    references: HashMap<String, Reference>,
    checksums: HashMap<String, VecDeque<(u64, u64)>>,
    reference_gaps: usize,
}

// The REST proxy and the reference books the WebSocket relay feeds
struct BinanceWire {
    product: &'static Product,
    http: HttpClient,
    weight: Mutex<(u64, Instant)>,
    weight_peak: AtomicU64,
    weight_refusals: AtomicUsize,
    throttled: AtomicUsize,
    control: Mutex<Control>,
}

impl BinanceWire {
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
    fn reference(&self, connection: ConnectionId, symbol: &str, diff: WireDiff) {
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
            let fault = control.depth.entry(symbol.to_string()).or_default();
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

type ConnectionId = (&'static str, usize);

struct BinanceCodec(Arc<BinanceWire>);

impl WireCodec for BinanceCodec {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(BinanceConnection {
            wire: Arc::clone(&self.0),
            connection: (route.path, number),
            depth_requests: HashSet::new(),
        })
    }
}

struct BinanceConnection {
    wire: Arc<BinanceWire>,
    connection: ConnectionId,
    depth_requests: HashSet<u64>,
}

impl WireConnection for BinanceConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let diff = match message {
            Message::Text(text) => {
                let frame = serde_json::from_str::<Value>(text).ok();

                if let Some(frame) = &frame
                    && let Some(id) = frame["id"].as_u64()
                    && self.depth_requests.remove(&id)
                {
                    eprintln!("wire: venue response id={id} {frame}");
                }

                frame.as_ref().and_then(json_diff)
            }
            Message::Binary(bytes) => sbe_diff(bytes),
            _ => None,
        };

        let Some((key, diff)) = diff else {
            return Upstream::Other;
        };

        self.wire.reference(self.connection, &key, diff);

        Upstream::Book {
            key,
            kind: FrameKind::Update,
        }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        if let Message::Text(text) = message
            && let Ok(frame) = serde_json::from_str::<Value>(text)
            && let Some(method) = frame["method"].as_str()
            && frame["params"].to_string().contains("@depth")
        {
            let id = frame["id"].as_u64().unwrap_or_default();
            self.depth_requests.insert(id);
            eprintln!("wire: client {method} {} id={id}", frame["params"]);
        }

        Vec::new()
    }
}

async fn rest(State(wire): State<Arc<BinanceWire>>, uri: Uri, headers: HeaderMap) -> Response {
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
        eprintln!("Weight cap: refusing {path_and_query} at used_weight_1m={used}");
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
        eprintln!("Venue throttled {path_and_query}: status={status}");
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

fn book_checksum(book: &WireBook, depth: usize) -> u64 {
    checksum(
        book.bids.iter().rev().take(depth).map(|(p, s)| (*p, *s)),
        book.asks.iter().take(depth).map(|(p, s)| (*p, *s)),
    )
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
    connection: ConnectionId,
    buffer: VecDeque<WireDiff>,
    synced: Option<(WireBook, u64)>,
}

impl Reference {
    fn new(connection: ConnectionId) -> Self {
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
                    (diff.last, book_checksum(book, product.deep_levels)),
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
        let mut entries = vec![(l, book_checksum(&book, product.deep_levels))];
        let mut last = l;

        for (index, diff) in self.buffer.iter().enumerate().skip(start) {
            let linked = index == start || diff.follows(last, product);

            // A gap inside the buffer needs a newer snapshot
            if !linked {
                return;
            }

            book.apply(&diff.bids, &diff.asks);
            last = diff.last;
            entries.push((last, book_checksum(&book, product.deep_levels)));
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
                            if let Message::Text(text) = message {
                                oracle.record(&text);
                            }
                        }

                        eprintln!("Oracle stream closed, reconnecting");
                    }
                    Err(e) => eprintln!("Oracle connect failed: {e}"),
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

// Proves the oracles before any venue traffic, since a wrong oracle would pass a wrong book
fn check_oracles() {
    let d = |value: &str| Decimal::from_str(value).unwrap();

    let book = |bids: &[(&str, &str)], asks: &[(&str, &str)]| WireBook {
        bids: bids.iter().map(|(p, s)| (d(p), d(s))).collect(),
        asks: asks.iter().map(|(p, s)| (d(p), d(s))).collect(),
    };

    let diff = |first, last, prev, bids: &[(&str, &str)], asks: &[(&str, &str)]| WireDiff {
        first,
        last,
        prev,
        bids: bids.iter().map(|(p, s)| (d(p), d(s))).collect(),
        asks: asks.iter().map(|(p, s)| (d(p), d(s))).collect(),
    };

    // Raw diffs parse from combined-stream frames, and other events are ignored
    assert_eq!(
        json_diff(&json!({"stream": "btcusdt@depth", "data": {
            "e": "depthUpdate", "s": "BTCUSDT", "U": 5, "u": 7, "pu": 4,
            "b": [["10.5", "1"]], "a": [["11", "0"]],
        }})),
        Some((
            "BTCUSDT".to_string(),
            diff(5, 7, Some(4), &[("10.5", "1")], &[("11", "0")])
        ))
    );
    assert_eq!(json_diff(&json!({"e": "trade", "s": "BTCUSDT"})), None);

    assert_eq!(mantissa_decimal(12_345, -2), d("123.45"));
    assert_eq!(mantissa_decimal(7, 2), d("700"));
    assert_eq!(mantissa_decimal(5, 0), d("5"));

    let snapshot = WireSnapshot::parse(
        "application/json",
        json!({"lastUpdateId": 10, "bids": [["10", "1"], ["9", "2"]], "asks": [["11", "3"]]})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(snapshot.last_update_id, 10);
    assert_eq!(
        snapshot.book,
        book(&[("10", "1"), ("9", "2")], &[("11", "3")])
    );

    // Checksums ignore trailing zeros and see every level on both sides
    assert_eq!(
        book_checksum(&book(&[("10.0", "1.50")], &[("11", "2")]), 10),
        book_checksum(&book(&[("10", "1.5")], &[("11.00", "2")]), 10)
    );
    assert_ne!(
        book_checksum(&book(&[("10", "1")], &[("11", "2")]), 10),
        book_checksum(&book(&[("10", "1")], &[("11", "3")]), 10)
    );

    // Spot: the first diff after the snapshot spans `lastUpdateId + 1`, later diffs continue at
    // `u + 1`, and an early stale diff is dropped
    {
        let mut reference = Reference::new(("/ws", 1));
        let mut history = VecDeque::new();
        assert!(!reference.push(
            diff(8, 9, None, &[("9", "5")], &[]),
            None,
            &SPOT,
            &mut history
        ));
        assert!(!reference.push(
            diff(10, 12, None, &[("10", "4")], &[]),
            None,
            &SPOT,
            &mut history
        ));
        reference.seed(&snapshot, &SPOT, &mut history);
        assert!(!reference.push(
            diff(13, 14, None, &[], &[("11", "0"), ("12", "1")]),
            None,
            &SPOT,
            &mut history
        ));

        let seeded = book(&[("10", "1"), ("9", "2")], &[("11", "3")]);
        let bridged = book(&[("10", "4"), ("9", "2")], &[("11", "3")]);
        let linked = book(&[("10", "4"), ("9", "2")], &[("12", "1")]);
        assert_eq!(
            history,
            [
                (10, book_checksum(&seeded, SPOT.deep_levels)),
                (12, book_checksum(&bridged, SPOT.deep_levels)),
                (14, book_checksum(&linked, SPOT.deep_levels)),
            ]
        );
        assert!(reference.push(diff(16, 17, None, &[], &[]), None, &SPOT, &mut history));
        assert!(reference.synced.is_none());
    }

    // Futures: the first diff spans `lastUpdateId`, and later diffs link through `pu`
    {
        let mut reference = Reference::new(("/ws", 1));
        let mut history = VecDeque::new();
        assert!(!reference.push(
            diff(9, 11, Some(8), &[("10", "4")], &[]),
            Some(&snapshot),
            &FUTURES,
            &mut history
        ));
        assert!(!reference.push(
            diff(12, 13, Some(11), &[("9", "0")], &[]),
            None,
            &FUTURES,
            &mut history
        ));
        assert_eq!(
            reference
                .synced
                .as_ref()
                .map(|(book, last)| (book.clone(), *last)),
            Some((book(&[("10", "4")], &[("11", "3")]), 13))
        );
        assert!(reference.push(
            diff(15, 16, Some(14), &[], &[]),
            None,
            &FUTURES,
            &mut history
        ));
    }

    // A snapshot no buffered diff bridges leaves the reference unsynced
    {
        let mut reference = Reference::new(("/ws", 1));
        let mut history = VecDeque::new();
        reference.push(
            diff(20, 21, None, &[], &[]),
            Some(&snapshot),
            &SPOT,
            &mut history,
        );
        assert!(reference.synced.is_none());
        assert!(history.is_empty());
    }

    // The depth20 oracle keeps nonzero levels from either payload shape
    {
        let oracle = Oracle {
            views: Mutex::new(HashMap::new()),
            frames: AtomicUsize::new(0),
        };

        oracle.record(
            &json!({"stream": "btcusdt@depth20@100ms", "data": {
                "lastUpdateId": 5, "bids": [["10", "1"], ["9", "0"]], "asks": [["11", "2"]],
            }})
            .to_string(),
        );
        oracle.record(
            &json!({"stream": "ethusdt@depth20@100ms", "data": {
                "u": 6, "b": [["20", "1"]], "a": [["21", "2"]],
            }})
            .to_string(),
        );
        let views = oracle.views.lock();
        let snapshot_book = book(&[("10", "1")], &[("11", "2")]);
        assert_eq!(
            views["BTCUSDT"],
            [View {
                update_id: 5,
                bids: snapshot_book.bids,
                asks: snapshot_book.asks,
            }]
        );
        assert_eq!(views["ETHUSDT"][0].update_id, 6);
        assert_eq!(oracle.frames.load(Ordering::Relaxed), 2);
    }

    // Pending samples are checked when the history holds them, count as unmatched when the
    // history passed them, and wait when the history has not reached them
    {
        let history = VecDeque::from([(1, 10), (2, 20), (4, 40)]);
        let mut pending = VecDeque::from([(1, 10), (3, 30), (4, 40), (5, 50)]);
        let mut compared = Vec::new();
        let counts = match_pending(
            &mut pending,
            &history,
            |(update_id, _)| *update_id,
            |entry, expected| compared.push((*entry, *expected)),
        );
        assert_eq!(counts, (2, 1));
        assert_eq!(compared, [((1, 10), (1, 10)), ((4, 40), (4, 40))]);
        assert_eq!(pending, [(5, 50)]);
    }
}

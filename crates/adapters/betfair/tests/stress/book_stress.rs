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
//! Run with Betfair credentials and a live application key set:
//! `cargo test -p nautilus-betfair --features examples --test betfair-book-stress -- --timeout 10 --rounds 10`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates five fault phases over `--rounds` rounds.
//! - `boundaries`: probes retry exhaustion into the retry ceiling and shutdown during reconnects.
//!
//! The harness subscribes every runner of the most traded match odds markets starting within the
//! window, or of the markets given with `--markets`. Betfair images whole markets on one market
//! subscription per connection, so faults target a market, and a replacement resubscribes and
//! re-images every market. A reconnect replays the subscription with its clocks, so Betfair resumes
//! each book in place rather than imaging it again. No orders are submitted. Every emitted batch
//! passes through the shared `BookStreamChecker` and is verified against a reference book rebuilt
//! from the raw lines the proxy relays; a session fails unless every snapshot episode was verified.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    rc::Rc,
    str::FromStr,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use futures_util::future::BoxFuture;
use nautilus_betfair::{
    common::{
        consts::{
            BETFAIR_CLIENT_ID, BETFAIR_STREAM_HOST, BETFAIR_STREAM_PORT,
            METHOD_LIST_MARKET_CATALOGUE,
        },
        enums::MarketSort,
        parse::{extract_market_id, make_instrument_id},
    },
    config::BetfairDataClientConfig,
    factories::BetfairDataClientFactory,
    http::{
        client::BetfairHttpClient,
        models::{ListMarketCatalogueParams, MarketCatalogue, MarketFilter, TimeRange},
    },
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
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, Flag, FrameKind, LineStream, Route, Session, StressArgs, StressVenue,
    Upstream, WireBook, WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

const MARKETS: usize = 4;
const STREAM: &str = "stream";
const STREAM_ENDPOINT: &str = "betfair-data-streams";
// Fault key of the market subscription, which covers every market on the connection
const SUBSCRIPTION: &str = "subscription";
const DEPTH: usize = 10;

// Market IDs, selected once before any session connects
static MARKET_IDS: OnceLock<Vec<String>> = OnceLock::new();

type BetfairSession = Session<Betfair>;

fn main() {
    stress::run::<Betfair, _, _>(|args| async move {
        let markets = select_markets(&args).await;
        MARKET_IDS.set(markets).expect("markets select once");

        match args.scenario() {
            "churn" => churn(&args).await,
            "boundaries" => boundaries(&args).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs) -> String {
    let timeout = args.timeout_secs();
    let markets = market_ids();
    let mut session = BetfairSession::connect(args).await;
    let ids = subscribe_all(&mut session).await;

    for round in 0..args.rounds() {
        let phase = round % 5;
        let phase_started = Instant::now();
        let market = &busiest_market(&session, &ids);

        match phase {
            0 => recover_gap(&mut session, &ids, market).await,
            // Without deadlines a rejected resubscription waits out the 180-second budget
            1 if timeout > 0 => recover_rejected(&mut session, &ids, market).await,
            1 => recover_gap(&mut session, &ids, market).await,
            2 => resume_reconnect(&mut session, &ids).await,
            3 => reconnect_during_recovery(&mut session, &ids, market).await,
            4 if round / 5 % 2 == 0 => freeze_traffic(&mut session, &ids).await,
            4 => cut_reconnect(&mut session, &ids).await,
            _ => unreachable!(),
        }

        session.observe(Duration::from_secs(5)).await;
        session.round(
            round,
            &format!(
                "phase={phase} phase_ms={} market={market}",
                phase_started.elapsed().as_millis()
            ),
        );
    }

    let batches = session.batches();
    session.stop().await;
    format!(
        "markets={} runners={} batches_total={batches}",
        markets.len(),
        ids.len()
    )
}

async fn boundaries(args: &StressArgs) -> String {
    let timeout = args.timeout_secs();
    let mut session = BetfairSession::connect(args).await;
    let ids = subscribe_all(&mut session).await;
    session.observe(Duration::from_secs(10)).await;
    let market = &busiest_market(&session, &ids);
    let targets = runners_of(&ids, market);
    let connections = session.proxy().connections(STREAM);

    let rejected = {
        let mut subscription = session.proxy().fault(SUBSCRIPTION);
        subscription.reject = usize::MAX;
        subscription.rejected
    };

    corrupt_market(&session, market);

    session
        .until(
            Duration::from_secs(60),
            "recovery resubscription rejected",
            |s| s.proxy().fault(SUBSCRIPTION).rejected > rejected,
        )
        .await;

    // The retry budget ends within the window, including the 180-second budget with deadlines
    // disabled; recovery then continues at the one-minute ceiling
    let window = Duration::from_secs(185);
    session.observe(window).await;

    let budget = if timeout > 0 { 8 } else { 1 };
    let ceiling_max = window.as_secs() as usize / 60;

    let attempts = {
        let mut subscription = session.proxy().fault(SUBSCRIPTION);
        subscription.reject = 0;
        subscription.rejected - rejected
    };

    assert!(
        (budget..=budget + ceiling_max).contains(&attempts),
        "recovery made {attempts} attempts; expected the budget of {budget} plus at most one \
         ceiling attempt per minute"
    );

    for id in &targets {
        session.expect_snapshot(*id);
    }

    // The next ceiling attempt completes recovery without a reconnect; an attempt just before the
    // window ends was rejected, and the one after follows the doubled interval
    session
        .healthy_within(&targets, Duration::from_secs(180))
        .await;
    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check(
        "exhaustion",
        format!("market={market} attempts={attempts} reconnects=0"),
    );

    let cuts = session.proxy().cuts();
    session.proxy().cut(Some(STREAM), FrameKind::Update, 10);
    reconnect_resuming(&mut session);
    session
        .until(
            Duration::from_secs(60),
            "reconnect interrupted by shutdown",
            |s| s.proxy().cuts() > cuts,
        )
        .await;
    let batches = session.batches();
    session.stop().await;
    format!("runners={} batches_total={batches}", ids.len())
}

// A runner change the adapter cannot parse leaves its market behind, and one resubscription
// re-images every market
async fn recover_gap(session: &mut BetfairSession, ids: &[InstrumentId], market: &str) {
    let targets = runners_of(ids, market);
    let subscriptions = session.venue().wire.subscriptions();

    let corrupted = corrupt_market(session, market);

    for id in &targets {
        session.expect_snapshot(*id);
    }

    session.healthy(&targets).await;
    assert_eq!(session.proxy().fault(market).corrupted, corrupted + 1);
    assert_eq!(
        session.venue().wire.subscriptions(),
        subscriptions + 1,
        "one resubscription recovers {market}"
    );
}

// Rejected resubscriptions leave every book dark, as the venue keeps streaming the replaced
// subscription, until an attempt after a snapshot deadline reaches the venue
async fn recover_rejected(session: &mut BetfairSession, ids: &[InstrumentId], market: &str) {
    let targets = runners_of(ids, market);
    let subscriptions = session.venue().wire.subscriptions();

    let rejected = {
        let mut subscription = session.proxy().fault(SUBSCRIPTION);
        subscription.reject = 2;
        subscription.rejected
    };

    corrupt_market(session, market);

    for id in &targets {
        session.expect_snapshot(*id);
    }

    session.healthy(&targets).await;
    assert_eq!(session.proxy().fault(SUBSCRIPTION).rejected, rejected + 2);
    assert_eq!(session.venue().wire.subscriptions(), subscriptions + 1);
}

// A reconnect replays the subscription with its clocks, so Betfair patches every book in place
async fn resume_reconnect(session: &mut BetfairSession, ids: &[InstrumentId]) {
    let active = streaming_runners(session, ids).await;
    let subscriptions = session.venue().wire.subscriptions();
    reconnect_resuming(session);
    session.healthy(&active).await;

    let replayed = session.venue().wire.subscription();
    assert!(
        replayed["clk"].is_string() && replayed["initialClk"].is_string(),
        "replayed subscription resumes from its clocks: {replayed}"
    );
    assert_eq!(
        session.venue().wire.subscriptions(),
        subscriptions,
        "a resumed reconnect writes no subscription"
    );
}

// A reconnect during recovery replays the recovery's subscription, which has no clocks, so the
// venue images every market again
async fn reconnect_during_recovery(
    session: &mut BetfairSession,
    ids: &[InstrumentId],
    market: &str,
) {
    let rejected = {
        let mut subscription = session.proxy().fault(SUBSCRIPTION);
        subscription.reject = usize::MAX;
        subscription.rejected
    };

    corrupt_market(session, market);

    session
        .until(
            Duration::from_secs(60),
            "recovery resubscription rejected",
            |s| s.proxy().fault(SUBSCRIPTION).rejected > rejected,
        )
        .await;

    session.proxy().fault(SUBSCRIPTION).reject = 0;
    reconnect(session);
    session.healthy(ids).await;

    let replayed = session.venue().wire.subscription();
    assert_eq!(
        replayed.get("clk"),
        None,
        "recovery subscription has no clocks"
    );
}

// Traffic stops without a close, so dead-peer detection reconnects and the replay resumes every
// book
async fn freeze_traffic(session: &mut BetfairSession, ids: &[InstrumentId]) {
    let active = streaming_runners(session, ids).await;
    let connections = session.proxy().connections(STREAM);
    let freeze = Duration::from_secs(25);

    session.venue_mut().expected_epoch = connections + 1;
    session.proxy().freeze(freeze);
    session.observe(freeze + Duration::from_secs(2)).await;
    session.healthy(&active).await;
    assert!(session.proxy().connections(STREAM) > connections);
}

// A connection cut at an update reconnects at once, and the replay resumes every book
async fn cut_reconnect(session: &mut BetfairSession, ids: &[InstrumentId]) {
    let active = streaming_runners(session, ids).await;
    let connections = session.proxy().connections(STREAM);
    let cuts = session.proxy().cuts();

    session.venue_mut().expected_epoch = connections + 1;
    session.proxy().cut(Some(STREAM), FrameKind::Update, 1);
    session
        .until(
            Duration::from_secs(60),
            "connection cut at an update",
            |s| s.proxy().cuts() > cuts,
        )
        .await;
    session.healthy(&active).await;
}

async fn subscribe_all(session: &mut BetfairSession) -> Vec<InstrumentId> {
    let markets = market_ids();
    let mut ids = session
        .instruments()
        .iter()
        .filter(|id| extract_market_id(id).is_ok_and(|market| markets.contains(&market)))
        .copied()
        .collect::<Vec<_>>();
    ids.sort();
    assert!(!ids.is_empty(), "runners load for {markets:?}");

    for id in &ids {
        session.subscribe(*id);
    }

    session.healthy(&ids).await;
    stress::check(
        "subscribed",
        format!("markets={} runners={}", markets.len(), ids.len()),
    );
    ids
}

// A fault planted in a runner change waits for the market to update, so faults target the market
// that has streamed the most
fn busiest_market(session: &BetfairSession, ids: &[InstrumentId]) -> String {
    market_ids()
        .iter()
        .max_by_key(|market| {
            runners_of(ids, market)
                .iter()
                .map(|id| session.book(id).batches)
                .sum::<usize>()
        })
        .expect("markets select before sessions")
        .clone()
}

// Plants an unparsable level in the market's next runner change, returning the corruptions
// applied before it
fn corrupt_market(session: &BetfairSession, market: &str) -> usize {
    session.venue().wire.state.lock().target = Some(market.to_string());
    let mut fault = session.proxy().fault(market);
    fault.corrupt = 1;
    fault.corrupted
}

fn runners_of(ids: &[InstrumentId], market: &str) -> Vec<InstrumentId> {
    ids.iter()
        .filter(|id| extract_market_id(id).is_ok_and(|id_market| id_market == market))
        .copied()
        .collect()
}

// Runners that emitted during a short window, which a resumed stream must keep updating; a runner
// idle through the window can stay idle past the healthy limit
async fn streaming_runners(
    session: &mut BetfairSession,
    ids: &[InstrumentId],
) -> Vec<InstrumentId> {
    let before = ids
        .iter()
        .map(|id| session.book(id).batches)
        .collect::<Vec<_>>();
    session.observe(Duration::from_secs(15)).await;

    let active = ids
        .iter()
        .zip(before)
        .filter(|(id, batches)| session.book(id).batches > *batches)
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    assert!(
        !active.is_empty(),
        "runners stream updates within 15 seconds"
    );
    active
}

// Reconnects the stream with a clockless replay, requiring every book to resync from a fresh
// image on the successor connection
fn reconnect(session: &mut BetfairSession) {
    let pending = session.proxy().cuts_pending();
    session.venue_mut().expected_epoch = session.proxy().connections(STREAM) + 1 + pending;
    session.reconnect(STREAM_ENDPOINT);
}

// Reconnects the stream with a replay that resumes from its clocks, requiring each book to update
// on the successor connection
fn reconnect_resuming(session: &mut BetfairSession) {
    let pending = session.proxy().cuts_pending();
    session.venue_mut().expected_epoch = session.proxy().connections(STREAM) + 1 + pending;
    session.reconnect_resuming(STREAM_ENDPOINT);
}

fn market_ids() -> &'static [String] {
    MARKET_IDS.get().expect("markets select before sessions")
}

// Selects the most traded match odds markets starting within the window, unless given
async fn select_markets(args: &StressArgs) -> Vec<String> {
    let given = args.flag("markets");

    if !given.is_empty() {
        return given.split(',').map(|id| id.trim().to_string()).collect();
    }

    let credential = BetfairDataClientConfig::default()
        .credential()
        .expect("Betfair credentials set");
    let client = BetfairHttpClient::new(credential, None, None, None, None, None, None)
        .expect("HTTP client builds");
    client.connect().await.expect("Betfair login succeeds");

    let now = jiff::Timestamp::now();
    let from = now - jiff::SignedDuration::from_hours(1);
    let to = now + jiff::SignedDuration::from_hours(12);

    let params = ListMarketCatalogueParams {
        filter: MarketFilter {
            market_type_codes: Some(vec![Ustr::from("MATCH_ODDS")]),
            market_start_time: Some(TimeRange {
                from: Some(from.to_string()),
                to: Some(to.to_string()),
            }),
            ..Default::default()
        },
        market_projection: None,
        sort: Some(MarketSort::MaximumTraded),
        max_results: Some(MARKETS as u32),
        locale: None,
    };

    let catalogues: Vec<MarketCatalogue> = client
        .send_betting(METHOD_LIST_MARKET_CATALOGUE, &params)
        .await
        .expect("market catalogue loads");
    client.disconnect().await;

    let markets = catalogues
        .into_iter()
        .map(|catalogue| catalogue.market_id)
        .collect::<Vec<_>>();
    assert!(
        !markets.is_empty(),
        "an open match odds market is available"
    );
    markets
}

struct Betfair {
    wire: BetfairWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Betfair {
    const NAME: &'static str = "betfair";
    const SCENARIOS: &'static [&'static str] = &["churn", "boundaries"];
    const ROUNDS: usize = 10;
    const FLAGS: &'static [Flag] = &[Flag {
        name: "markets",
        default: "",
        help: "Comma-separated open market IDs to test instead of the most traded match odds",
    }];
    // Betfair books carry the publish time as their sequence, which frames can share
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(120);

    fn self_check() {
        check_wire_oracle();
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: BetfairWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *BETFAIR_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "",
            upstream: format!("tls://{BETFAIR_STREAM_HOST}:{BETFAIR_STREAM_PORT}"),
            endpoint: STREAM_ENDPOINT,
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let config = BetfairDataClientConfig {
            market_ids: Some(market_ids().to_vec()),
            stream_host: Some(proxy.ip().to_string()),
            stream_port: Some(proxy.port()),
            stream_use_tls: false,
            stream_conflate_ms: Some(0),
            book_snapshot_timeout_secs: args.timeout_secs(),
            ..Default::default()
        };

        let cache = CacheView::from(Rc::new(RefCell::new(Cache::default())));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));

        BetfairDataClientFactory::new().create(BETFAIR_CLIENT_ID.as_str(), &config, cache, clock)
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        extract_market_id(instrument_id).expect("Betfair runner instrument")
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let key = id.to_string();

        // A publish time two frames share for one runner cannot be aligned, so the sample is
        // skipped
        if self.wire.events(&key, deltas.sequence) > 1 {
            return;
        }

        let view = self
            .wire
            .views
            .find(&key, deltas.sequence, deltas.ts_event.as_u64())
            .expect("wire oracle at emitted publish time");

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            panic!(
                "wire oracle mismatch {id} pt={} ts={}: {violation}",
                deltas.sequence, deltas.ts_event
            );
        }

        self.epochs.insert(id, view.epoch);
    }

    // A healthy book synced on the expected connection and emitted since the wait began
    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches > start.batches
    }

    fn stats(&self) -> String {
        format!("subscriptions={}", self.wire.subscriptions())
    }
}

#[derive(Clone, Default)]
struct BetfairWire {
    views: WireViews,
    state: Arc<Mutex<WireState>>,
}

// Oracle books outlive a connection, since a reconnect resumes them
#[derive(Default)]
struct WireState {
    // The newest connection carries the adapter's stream
    epoch: usize,
    books: HashMap<String, HashMap<InstrumentId, WireBook>>,
    events: HashMap<(String, u64), usize>,
    // IDs of the market subscriptions the adapter wrote, in order; a reconnect replays the latest
    subscription_ids: Vec<u64>,
    subscription: Value,
    // The market the latest planted corruption targets, which a frame of several markets
    // classifies as so the corruption reaches it
    target: Option<String>,
}

impl BetfairWire {
    // Market subscriptions written, excluding reconnect replays
    fn subscriptions(&self) -> usize {
        self.state.lock().subscription_ids.len()
    }

    // The latest market subscription that reached the venue, which may be a replay
    fn subscription(&self) -> Value {
        self.state.lock().subscription.clone()
    }

    fn events(&self, key: &str, publish_time: u64) -> usize {
        self.state
            .lock()
            .events
            .get(&(key.to_string(), publish_time))
            .copied()
            .unwrap_or(0)
    }
}

impl WireCodec for BetfairWire {
    fn open(&self, _route: &Route, number: usize) -> Box<dyn WireConnection> {
        let mut state = self.state.lock();
        state.epoch = state.epoch.max(number);

        Box::new(BetfairConnection {
            wire: self.clone(),
            epoch: number,
        })
    }

    fn connect(&self, _route: &Route) -> BoxFuture<'static, std::io::Result<Box<dyn LineStream>>> {
        Box::pin(async {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(std::io::Error::other)?
                .with_root_certificates(roots)
                .with_no_client_auth();
            let server = rustls::pki_types::ServerName::try_from(BETFAIR_STREAM_HOST)
                .map_err(std::io::Error::other)?;
            let tcp =
                tokio::net::TcpStream::connect((BETFAIR_STREAM_HOST, BETFAIR_STREAM_PORT)).await?;
            let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
                .connect(server, tcp)
                .await?;
            Ok(Box::new(tls) as Box<dyn LineStream>)
        })
    }
}

struct BetfairConnection {
    wire: BetfairWire,
    epoch: usize,
}

impl WireConnection for BetfairConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Some(frame) = parse(message) else {
            return Upstream::Other;
        };

        if frame["op"] == "status" && frame["statusCode"] == "FAILURE" {
            eprintln!("Venue error on route {STREAM}: {frame}");
        }

        if frame["op"] != "mcm" {
            return Upstream::Other;
        }

        let mut state = self.wire.state.lock();

        // A newer connection replaced this one, so its late frames reach no adapter
        if self.epoch < state.epoch {
            return Upstream::Other;
        }

        let Some(changes) = frame["mc"].as_array().filter(|changes| !changes.is_empty()) else {
            return Upstream::Other;
        };

        let publish_time = frame["pt"].as_u64().unwrap();

        // A subscription image replaces every market
        if frame["ct"] == "SUB_IMAGE"
            && matches!(frame["segmentType"].as_str(), None | Some("SEG_START"))
        {
            state.books.clear();
        }

        for change in changes {
            apply(
                &mut state,
                &self.wire.views,
                change,
                publish_time,
                self.epoch,
            );
        }

        classify(changes, state.target.as_deref())
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        if let Some(frame) = parse(message).filter(|frame| frame["op"] == "marketSubscription") {
            let mut state = self.wire.state.lock();
            let id = frame["id"].as_u64().unwrap();

            if state.subscription_ids.last() != Some(&id) {
                state.subscription_ids.push(id);
            }

            state.subscription = frame;
        }

        Vec::new()
    }

    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let frame = parse(message).filter(|frame| frame["op"] == "marketSubscription")?;
        let reply = json!({
            "op": "status",
            "id": frame["id"],
            "statusCode": "FAILURE",
            "errorCode": "SUBSCRIPTION_LIMIT_EXCEEDED",
            "errorMessage": "stress rejection",
            "connectionClosed": false,
        });

        Some((
            SUBSCRIPTION.to_string(),
            Message::Text(reply.to_string().into()),
        ))
    }

    // Gives a level of the market a negative size, which the adapter cannot parse
    fn corrupt(&mut self, message: &mut Message, key: &str, kind: FrameKind) -> bool {
        if kind == FrameKind::Snapshot {
            return false;
        }

        let Some(mut frame) = parse(message) else {
            return false;
        };

        let has_levels = |levels: &Value| levels.as_array().is_some_and(|rows| !rows.is_empty());
        let runner = frame["mc"]
            .as_array_mut()
            .into_iter()
            .flatten()
            .filter(|change| change["id"] == key)
            .flat_map(|change| change["rc"].as_array_mut().into_iter().flatten())
            .find(|runner| has_levels(&runner["atb"]) || has_levels(&runner["atl"]));

        let Some(runner) = runner else {
            return false;
        };

        let side = if has_levels(&runner["atb"]) {
            "atb"
        } else {
            "atl"
        };

        runner[side][0][1] = json!(-1);
        *message = Message::Text(frame.to_string().into());
        true
    }
}

fn parse(message: &Message) -> Option<Value> {
    let Message::Text(text) = message else {
        return None;
    };

    serde_json::from_str(text).ok()
}

// Applies one market change, recording a view for each runner whose book the adapter emits
fn apply(
    state: &mut WireState,
    views: &WireViews,
    change: &Value,
    publish_time: u64,
    epoch: usize,
) {
    let market = change["id"].as_str().unwrap().to_string();
    let image = change["img"] == true;
    let runners = state.books.entry(market.clone()).or_default();

    if image {
        runners.clear();
    }

    let mut emitted = Vec::new();

    for runner in change["rc"].as_array().into_iter().flatten() {
        let id = runner_id(&market, &runner["id"], &runner["hc"]);
        let bids = levels(&runner["atb"]);
        let asks = levels(&runner["atl"]);

        // The adapter emits a snapshot for every imaged runner, and an update only for levels
        if image || !bids.is_empty() || !asks.is_empty() {
            runners.entry(id).or_default().apply(&bids, &asks);
            emitted.push(id);
        }
    }

    // An image empties a defined runner it omits
    if image {
        let defined = change["marketDefinition"]["runners"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|runner| runner_id(&market, &runner["id"], &runner["hc"]))
            .filter(|id| !emitted.contains(id))
            .collect::<Vec<_>>();
        emitted.extend(defined);
    }

    for id in emitted {
        let key = id.to_string();
        let book = runners
            .get(&id)
            .map(|book| book.top(DEPTH))
            .unwrap_or_default();
        views.record(
            &key,
            WireView {
                epoch,
                sequence: publish_time,
                timestamp: publish_time * 1_000_000,
                book,
            },
        );

        *state.events.entry((key, publish_time)).or_default() += 1;
    }
}

// Classifies a frame by its first imaged market, or else by the target or its first market with
// book levels
fn classify(changes: &[Value], target: Option<&str>) -> Upstream {
    if let Some(change) = changes.iter().find(|change| change["img"] == true) {
        return Upstream::Book {
            key: change["id"].as_str().unwrap().to_string(),
            kind: FrameKind::Snapshot,
        };
    }

    let has_levels =
        |change: &&Value| {
            change["rc"].as_array().into_iter().flatten().any(|runner| {
                !levels(&runner["atb"]).is_empty() || !levels(&runner["atl"]).is_empty()
            })
        };

    let targeted = changes
        .iter()
        .filter(has_levels)
        .find(|change| target.is_some_and(|target| change["id"] == target));

    targeted
        .or_else(|| changes.iter().find(has_levels))
        .map_or(Upstream::Other, |change| Upstream::Book {
            key: change["id"].as_str().unwrap().to_string(),
            kind: FrameKind::Update,
        })
}

fn runner_id(market: &str, selection: &Value, handicap: &Value) -> InstrumentId {
    let selection = selection
        .as_u64()
        .or_else(|| selection.as_str().and_then(|id| id.parse().ok()))
        .unwrap();

    let handicap = if handicap.is_null() {
        Decimal::ZERO
    } else {
        decimal(handicap)
    };

    make_instrument_id(market, selection, handicap)
}

fn levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    rows.as_array()
        .into_iter()
        .flatten()
        .map(|row| (decimal(&row[0]), decimal(&row[1])))
        .collect()
}

// Parses a JSON number through its shortest decimal text, so `2.42` stays exactly 2.42
fn decimal(value: &Value) -> Decimal {
    Decimal::from_str(&value.to_string()).unwrap()
}

// Proves the wire oracle before any venue traffic, since a wrong oracle would pass a wrong book
fn check_wire_oracle() {
    let wire = BetfairWire::default();

    let route = Route {
        name: STREAM,
        path: "",
        upstream: String::new(),
        endpoint: STREAM_ENDPOINT,
        headers: &[],
    };

    let mut first = wire.open(&route, 1);
    let text = |value: Value| Message::Text(value.to_string().into());
    let runner = |selection: u64| runner_id("1.1", &json!(selection), &Value::Null);

    let book = |bids: &[(i64, i64)], asks: &[(i64, i64)]| WireBook {
        bids: bids
            .iter()
            .map(|(price, size)| (Decimal::from(*price), Decimal::from(*size)))
            .collect(),
        asks: asks
            .iter()
            .map(|(price, size)| (Decimal::from(*price), Decimal::from(*size)))
            .collect(),
    };

    let view = |epoch, publish_time: u64, book| WireView {
        epoch,
        sequence: publish_time,
        timestamp: publish_time * 1_000_000,
        book,
    };

    let mut image = text(json!({
        "op": "mcm", "id": 2, "pt": 5, "ct": "SUB_IMAGE",
        "mc": [{
            "id": "1.1", "img": true,
            "marketDefinition": {"runners": [{"id": 10}, {"id": 11}]},
            "rc": [{"id": 10, "atb": [[2, 5], [3, 0]], "atl": [[4, 6]]}],
        }],
    }));
    let mut update = text(json!({
        "op": "mcm", "id": 2, "pt": 6,
        "mc": [{"id": "1.1", "rc": [{"id": 10, "atb": [[2, 0], [3, 7]], "trd": [[3, 1]]}]}],
    }));
    let trades = text(json!({
        "op": "mcm", "id": 2, "pt": 7,
        "mc": [{"id": "1.1", "rc": [{"id": 10, "trd": [[3, 2]]}]}],
    }));
    let heartbeat = text(json!({"op": "mcm", "id": 2, "pt": 8, "ct": "HEARTBEAT"}));
    let subscription = text(json!({"op": "marketSubscription", "id": 3, "clk": "AAA"}));

    assert_eq!(
        first.upstream(&image),
        Upstream::Book {
            key: "1.1".to_string(),
            kind: FrameKind::Snapshot,
        }
    );
    assert_eq!(
        first.upstream(&update),
        Upstream::Book {
            key: "1.1".to_string(),
            kind: FrameKind::Update,
        }
    );
    assert_eq!(first.upstream(&trades), Upstream::Other);
    assert_eq!(first.upstream(&heartbeat), Upstream::Other);
    assert_eq!(
        [
            wire.views.find(&runner(10).to_string(), 5, 5_000_000),
            wire.views.find(&runner(11).to_string(), 5, 5_000_000),
            wire.views.find(&runner(10).to_string(), 6, 6_000_000),
            wire.views.find(&runner(10).to_string(), 7, 7_000_000),
        ],
        [
            Some(view(1, 5, book(&[(2, 5)], &[(4, 6)]))),
            Some(view(1, 5, WireBook::default())),
            Some(view(1, 6, book(&[(3, 7)], &[(4, 6)]))),
            None,
        ]
    );
    assert_eq!(wire.events(&runner(10).to_string(), 6), 1);

    assert!(first.client(&subscription).is_empty());
    assert!(first.client(&subscription).is_empty());
    assert_eq!(wire.subscriptions(), 1);
    assert_eq!(wire.subscription()["clk"], "AAA");
    let (key, reply) = first.reject(&subscription).unwrap();
    let reply = parse(&reply).unwrap();
    assert_eq!(key, SUBSCRIPTION);
    assert_eq!(reply["id"], 3);
    assert_eq!(reply["statusCode"], "FAILURE");
    assert_eq!(reply["connectionClosed"], false);
    assert!(first.reject(&update).is_none());

    assert!(!first.corrupt(&mut image, "1.1", FrameKind::Snapshot));
    assert!(!first.corrupt(&mut update.clone(), "1.2", FrameKind::Update));
    assert!(first.corrupt(&mut update, "1.1", FrameKind::Update));
    assert_eq!(parse(&update).unwrap()["mc"][0]["rc"][0]["atb"][0][1], -1);

    // A resumed connection patches the books the replaced connection built
    let mut second = wire.open(&route, 2);
    let resumed = text(json!({
        "op": "mcm", "id": 3, "pt": 9, "ct": "RESUB_DELTA",
        "mc": [{"id": "1.1", "rc": [{"id": 10, "atl": [[4, 1]]}]}],
    }));
    let late = text(json!({
        "op": "mcm", "id": 2, "pt": 10,
        "mc": [{"id": "1.1", "rc": [{"id": 10, "atl": [[4, 9]]}]}],
    }));
    second.upstream(&resumed);
    assert_eq!(first.upstream(&late), Upstream::Other);
    assert_eq!(
        wire.views.find(&runner(10).to_string(), 9, 9_000_000),
        Some(view(2, 9, book(&[(3, 7)], &[(4, 1)])))
    );
    assert_eq!(
        wire.views.find(&runner(10).to_string(), 10, 10_000_000),
        None
    );

    // A corruption reaches its market when another market leads the frame
    let mut markets = text(json!({
        "op": "mcm", "id": 3, "pt": 11,
        "mc": [
            {"id": "1.1", "rc": [{"id": 10, "atb": [[3, 8]]}]},
            {"id": "1.2", "rc": [{"id": 20, "atl": [[5, 2]]}]},
        ],
    }));

    let classified = |key: &str| Upstream::Book {
        key: key.to_string(),
        kind: FrameKind::Update,
    };

    let untargeted = second.upstream(&markets);
    wire.state.lock().target = Some("1.2".to_string());
    let targeted = second.upstream(&markets);
    assert!(second.corrupt(&mut markets, "1.2", FrameKind::Update));
    let corrupted = parse(&markets).unwrap();
    assert_eq!(
        (untargeted, targeted),
        (classified("1.1"), classified("1.2"))
    );
    assert_eq!(corrupted["mc"][0]["rc"][0]["atb"], json!([[3, 8]]));
    assert_eq!(corrupted["mc"][1]["rc"][0]["atl"], json!([[5, -1]]));
}

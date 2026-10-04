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
//! `cargo test -p nautilus-okx --features examples --test okx-book-stress -- --timeout 10 --rounds 18`
//!
//! Scenarios, selected with `--scenario`:
//!
//! - `churn` (default): rotates six fault phases over `--rounds` rounds.
//! - `initial`: drops first snapshots in each of `--rounds` fresh sessions.
//! - `turnover`: unsubscribes and resubscribes during recovery at three boundaries.
//! - `boundaries`: probes replacement cuts, retry exhaustion, and shutdown during reconnect.
//!
//! No orders are submitted. Every emitted batch passes through the shared `BookStreamChecker` and
//! is verified against a reference book rebuilt from the raw frames the proxy relays; a session
//! fails unless every snapshot episode was verified.

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
use nautilus_core::Params;
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::mode::ReconnectRequestOutcome;
use nautilus_okx::{
    common::{consts::OKX_CLIENT_ID, enums::OKXInstrumentType},
    config::OKXDataClientConfig,
    data::OKXDataClient,
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, FrameKind, Route, Session, StressArgs, StressVenue, Upstream, WireBook,
    WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const SYMBOLS: [&str; 8] = [
    "BTC-USDT",
    "ETH-USDT",
    "SOL-USDT",
    "DOGE-USDT",
    "BTC-USDT-SWAP",
    "ETH-USDT-SWAP",
    "BTC-USDT_BTC-USDT-SWAP",
    "ETH-USDT_ETH-USDT-SWAP",
];

const PUBLIC: &str = "public";
const BUSINESS: &str = "business";
const PUBLIC_ENDPOINT: &str = "okx-public-data-streams";
const BUSINESS_ENDPOINT: &str = "okx-business-data-streams";
const BOOK_CHANNELS: [&str; 3] = ["books", "books-rpi", "sprd-books5"];
const DEPTH: usize = 20;

type OkxSession = Session<Okx>;

fn main() {
    stress::run::<Okx, _, _>(|args| async move {
        let ids = SYMBOLS.map(|s| InstrumentId::from(format!("{s}.OKX")));

        match args.scenario() {
            "churn" => churn(&args, &ids).await,
            "initial" => initial(&args, &ids).await,
            "turnover" => turnover(&args, &ids).await,
            "boundaries" => boundaries(&args, &ids).await,
            other => unreachable!("argument parsing admits only declared scenarios, was {other}"),
        }
    });
}

async fn churn(args: &StressArgs, ids: &[InstrumentId; 8]) -> String {
    let timeout = args.timeout_secs();
    let mut total = 0;
    let mut session = OkxSession::connect(args).await;

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
    recover_without_reconnect(&mut session, &ids[..6], usize::from(timeout > 0)).await;

    for round in 0..args.rounds() {
        let phase = round % 6;
        let phase_started = Instant::now();

        match phase {
            0 => {
                let before = ids[..6]
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.corrupt = 1;
                        fault.drop_snapshots = 1;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(20),
                        "simultaneous recoveries lose snapshots",
                        |s| {
                            before
                                .iter()
                                .all(|(id, dropped)| s.fault(id).dropped > *dropped)
                        },
                    )
                    .await;

                reconnect(&mut session, PUBLIC);
                session.healthy(&ids[..6]).await;
            }
            1 => {
                for id in &ids[..6] {
                    session.fault(id).hold = true;
                }

                session.observe(Duration::from_secs(2)).await;
                reconnect(&mut session, PUBLIC);
                session.observe(Duration::from_secs(4)).await;

                for id in &ids[..6] {
                    session.fault(id).hold = false;
                }

                session.proxy().release();
                session.healthy(&ids[..6]).await;
            }
            2 => {
                let targets = [ids[round / 6 % 2], ids[4 + round / 6 % 2]];

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
                reconnect(&mut session, PUBLIC);
                let remaining = ids[..6]
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
            3 => {
                session.proxy().cut(Some(PUBLIC), FrameKind::Snapshot, 2);
                let cuts = session.proxy().cuts();
                reconnect(&mut session, PUBLIC);
                session
                    .until(
                        Duration::from_secs(120),
                        "two reconnects cut before snapshots",
                        |s| s.proxy().cuts() == cuts + 2,
                    )
                    .await;
                session.healthy(&ids[..6]).await;
            }
            4 => {
                // Without deadlines, sequenced books recover from their next update and spread
                // books, which carry no updates, from another reconnect.
                let before = ids
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.drop_snapshots = 1;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                reconnect(&mut session, PUBLIC);
                reconnect(&mut session, BUSINESS);
                session
                    .until(Duration::from_secs(60), "replay snapshots dropped", |s| {
                        before
                            .iter()
                            .all(|(id, dropped)| s.fault(id).dropped > *dropped)
                    })
                    .await;

                if timeout == 0 {
                    reconnect(&mut session, BUSINESS);
                }

                session.healthy(ids).await;
            }
            5 => {
                let before = ids[..6]
                    .iter()
                    .map(|id| {
                        let mut fault = session.fault(id);
                        fault.corrupt = 1;
                        fault.drop_snapshots = 3;
                        (*id, fault.dropped)
                    })
                    .collect::<Vec<_>>();

                session
                    .until(
                        Duration::from_secs(20),
                        "shutdown with recovery snapshots missing",
                        |s| {
                            before
                                .iter()
                                .all(|(id, dropped)| s.fault(id).dropped > *dropped)
                        },
                    )
                    .await;

                total += session.batches();
                session = session.restart().await;

                for id in ids {
                    session.subscribe(*id);
                }

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

async fn initial(args: &StressArgs, ids: &[InstrumentId; 8]) -> String {
    let timeout = args.timeout_secs();
    let mut total = 0;

    for round in 0..args.rounds() {
        let mut session = OkxSession::connect(args).await;

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
            reconnect(&mut session, PUBLIC);
            reconnect(&mut session, BUSINESS);
        }

        session.healthy(ids).await;

        for id in ids {
            let fault = session.fault(id).clone();
            assert_eq!(fault.corrupted, 0);
            assert_eq!(fault.dropped, 1);
            assert_eq!(fault.unsubscribes, usize::from(timeout > 0));

            if is_spread(id) {
                assert!(session.book(id).snapshots >= 1);
            } else {
                assert_eq!(session.book(id).snapshots, 1);
            }
        }

        assert_eq!(connections(&session), [if timeout > 0 { 1 } else { 2 }; 2]);
        session.observe(Duration::from_secs(2)).await;
        total += session.batches();
        session.stop().await;
        stress::check("initial", format!("round={} books=8", round + 1));
    }

    format!("batches_total={total}")
}

async fn turnover(args: &StressArgs, ids: &[InstrumentId; 8]) -> String {
    let timeout = args.timeout_secs();
    let mut session = OkxSession::connect(args).await;
    let quiet = [ids[0], ids[4], ids[6]];

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

    stress::check("initial_silence", "books=3");
    session.proxy().release();
    session.healthy(ids).await;

    for round in 0..args.rounds() {
        let id = ids[if round % 2 == 0 { 0 } else { 4 }];

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
        let unsubscribes = session.fault(&id).unsubscribes;
        assert!(unsubscribes > 0, "venue acknowledged the unsubscribe");
        session.observe(Duration::from_secs(5)).await;
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

async fn boundaries(args: &StressArgs, ids: &[InstrumentId; 8]) -> String {
    let timeout = args.timeout_secs();
    let mut session = OkxSession::connect(args).await;

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
    recover_without_reconnect(&mut session, &ids[..6], if timeout > 0 { 3 } else { 0 }).await;

    let targets = [ids[0], ids[4]];

    for id in targets {
        let connections = session.proxy().connections(PUBLIC);
        let cuts = session.proxy().cuts();
        {
            let mut fault = session.fault(&id);
            fault.corrupt = 1;
            fault.cut_unsubscribe = true;
        }

        session.venue_mut().expected_epochs[0] = connections + 1;

        for public_id in &ids[..6] {
            session.expect_snapshot(*public_id);
        }

        session.healthy(&ids[..6]).await;
        assert_eq!(session.proxy().cuts(), cuts + 1);
        assert_eq!(session.proxy().connections(PUBLIC), connections + 1);
        stress::check("replacement_cut", format!("instrument={id}"));
    }

    let connections_before = connections(&session);

    let before = targets.map(|id| {
        let mut fault = session.fault(&id);
        let unsubscribes = fault.unsubscribes;
        fault.corrupt = 1;
        fault.hold_snapshot = true;
        (id, unsubscribes, fault.held)
    });

    session
        .until(Duration::from_secs(20), "replacement snapshots held", |s| {
            before.iter().all(|(id, _, held)| s.fault(id).held > *held)
        })
        .await;

    // The retry budget ends within the window, including the 180-second budget with deadlines
    // disabled; recovery then continues at the one-minute ceiling.
    let window = Duration::from_secs(185);
    session.observe(window).await;

    let budget = if timeout > 0 { 8 } else { 1 };
    let ceiling_max = window.as_secs() as usize / 60;

    for (id, unsubscribes, _) in before {
        let mut fault = session.fault(&id);
        let attempts = fault.unsubscribes - unsubscribes;
        assert!(
            (budget..=budget + ceiling_max).contains(&attempts),
            "{id} made {attempts} attempts; expected the budget of {budget} plus at most one \
             ceiling attempt per minute"
        );
        fault.hold = false;
    }

    // Held replacement snapshots never reached the adapter, so each target needs one more
    for id in targets {
        session.expect_snapshot(id);
    }

    // Released snapshots complete the exhausted recoveries without a reconnect or resubscribe
    session.proxy().release();
    session.healthy(ids).await;
    assert_eq!(connections(&session), connections_before);
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
    reconnect(&mut session, PUBLIC);
    session.healthy(&ids[..6]).await;
    stress::check("reconnect", "books=6");

    let cuts = session.proxy().cuts();
    session.proxy().cut(Some(PUBLIC), FrameKind::Snapshot, 10);
    reconnect(&mut session, PUBLIC);
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

// Forces gaps whose replacement snapshots are dropped `drops` times, then checks that each book
// recovered through its own resubscribes without a reconnect
async fn recover_without_reconnect(session: &mut OkxSession, ids: &[InstrumentId], drops: usize) {
    let connections_before = connections(session);

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

    assert_eq!(connections(session), connections_before);
    stress::check(
        "autonomous",
        format!(
            "books={} dropped={} reconnects=0",
            ids.len(),
            ids.len() * drops
        ),
    );
}

// Reconnects one socket, requiring every book on it to resync from that socket's successor, or
// from the connection after any pending cuts
fn reconnect(session: &mut OkxSession, route: &'static str) {
    let business = route == BUSINESS;

    let pending = if business {
        0
    } else {
        session.proxy().cuts_pending()
    };

    let expected = session.proxy().connections(route) + 1 + pending;
    session.venue_mut().expected_epochs[usize::from(business)] = expected;

    let endpoint = if business {
        BUSINESS_ENDPOINT
    } else {
        PUBLIC_ENDPOINT
    };

    assert_eq!(
        session.reconnect(endpoint),
        ReconnectRequestOutcome::Accepted
    );
}

fn connections(session: &OkxSession) -> [usize; 2] {
    [
        session.proxy().connections(PUBLIC),
        session.proxy().connections(BUSINESS),
    ]
}

fn is_spread(id: &InstrumentId) -> bool {
    id.symbol.as_str().contains('_')
}

struct Okx {
    wire: OkxWire,
    expected_epochs: [usize; 2],
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Okx {
    const NAME: &'static str = "okx";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "turnover", "boundaries"];
    const ROUNDS: usize = 18;
    // OKX `seqId` can reset within an episode, so the wire oracle verifies content
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(90);

    fn self_check() {
        check_wire_oracle();
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: OkxWire::default(),
            expected_epochs: [1, 1],
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *OKX_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        [
            (PUBLIC, "/public", PUBLIC_ENDPOINT),
            (BUSINESS, "/business", BUSINESS_ENDPOINT),
        ]
        .into_iter()
        .map(|(name, path, endpoint)| Route {
            name,
            path,
            upstream: format!("wss://ws.okx.com:8443/ws/v5/{name}"),
            endpoint,
            headers: &[],
        })
        .collect()
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot, OKXInstrumentType::Swap],
            load_spreads: true,
            base_url_ws_public: Some(format!("ws://{proxy}/public")),
            base_url_ws_business: Some(format!("ws://{proxy}/business")),
            update_instruments_interval_mins: 0,
            book_stale_check_interval_secs: 0,
            book_snapshot_timeout_secs: args.timeout_secs(),
            ..OKXDataClientConfig::default()
        };

        Ok(Box::new(OKXDataClient::new(*OKX_CLIENT_ID, config)?))
    }

    fn key(&self, instrument_id: &InstrumentId) -> String {
        instrument_id.symbol.to_string()
    }

    fn endpoint(&self, instrument_id: &InstrumentId) -> &'static str {
        if is_spread(instrument_id) {
            BUSINESS_ENDPOINT
        } else {
            PUBLIC_ENDPOINT
        }
    }

    fn params(&self, instrument_id: &InstrumentId) -> Option<Params> {
        let rpi = instrument_id.symbol.as_str().ends_with("-SWAP") && !is_spread(instrument_id);
        rpi.then(|| serde_json::from_value(json!({"rpi": true})).unwrap())
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let view = self
            .wire
            .views
            .find(
                id.symbol.as_str(),
                deltas.sequence,
                deltas.ts_event.as_u64(),
            )
            .expect("wire oracle at emitted sequence and timestamp");

        if let Err(violation) = checker.verify(id, DEPTH, &view.book.bids, &view.book.asks) {
            panic!(
                "wire oracle mismatch {id} seq={} ts={}: {violation}",
                deltas.sequence, deltas.ts_event
            );
        }

        self.epochs.insert(id, view.epoch);
    }

    // A healthy book resynced on the expected socket; public books must also stream updates,
    // while spread books stream snapshots only
    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        let business = is_spread(id);
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epochs[usize::from(business)]
            && (business || book.batches >= start.batches + 5)
    }
}

#[derive(Clone, Default)]
struct OkxWire {
    views: WireViews,
}

impl WireCodec for OkxWire {
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection> {
        Box::new(OkxConnection {
            views: self.views.clone(),
            route: route.name,
            epoch: number,
            books: HashMap::new(),
        })
    }
}

struct OkxConnection {
    views: WireViews,
    route: &'static str,
    epoch: usize,
    books: HashMap<String, WireBook>,
}

impl WireConnection for OkxConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        if frame["event"] == "error" {
            eprintln!("Venue error on route {}: {frame}", self.route);
        }

        if frame["event"] == "unsubscribe"
            && let Some(key) = symbol(&frame["arg"])
        {
            return Upstream::Unsubscribed(key.to_string());
        }

        let channel = frame["arg"]["channel"].as_str().unwrap_or("");

        if !BOOK_CHANNELS.contains(&channel) || !frame["data"].is_array() {
            return Upstream::Other;
        }

        let snapshot = frame["action"] == "snapshot" || channel == "sprd-books5";
        let key = symbol(&frame["arg"]).unwrap().to_string();
        let book = self.books.entry(key.clone()).or_default();

        for data in frame["data"].as_array().unwrap() {
            apply(book, data, snapshot);
            self.views.record(
                &key,
                WireView {
                    epoch: self.epoch,
                    sequence: data["seqId"].as_u64().unwrap_or(0),
                    timestamp: data["ts"].as_str().unwrap().parse::<u64>().unwrap() * 1_000_000,
                    book: book.top(DEPTH),
                },
            );
        }

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
                        .filter_map(symbol)
                        .map(ToString::to_string)
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    // Breaks the `prevSeqId` link of an incremental frame, which the adapter must treat as a gap
    fn corrupt(&mut self, message: &mut Message, _key: &str, kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        if kind == FrameKind::Snapshot {
            return false;
        }

        let mut frame = serde_json::from_str::<Value>(text).unwrap();
        frame["data"][0]["prevSeqId"] = json!(i64::MAX);
        *message = Message::Text(frame.to_string().into());
        true
    }
}

fn symbol(arg: &Value) -> Option<&str> {
    arg["instId"].as_str().or_else(|| arg["sprdId"].as_str())
}

fn apply(book: &mut WireBook, data: &Value, snapshot: bool) {
    if snapshot {
        *book = WireBook::default();
    }

    book.apply(&levels(&data["bids"]), &levels(&data["asks"]));
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
            &json!({"bids": [["10", "2"], ["9", "3"]], "asks": [["11", "4"], ["12", "5"]]}),
            true,
        );
        apply(
            &mut book,
            &json!({"bids": [["10", "0"], ["9", "7"]], "asks": [["11", "8"], ["13", "6"]]}),
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

        apply(&mut book, &json!({"bids": [["8", "9"]], "asks": []}), true);
        assert_eq!(
            book,
            WireBook {
                bids: [(Decimal::from(8), Decimal::from(9))].into(),
                asks: [].into(),
            }
        );
        apply(&mut book, &json!({"bids": [], "asks": []}), true);
        assert_eq!(book, WireBook::default());
    }

    {
        let wire = OkxWire::default();

        let route = Route {
            name: PUBLIC,
            path: "/public",
            upstream: String::new(),
            endpoint: PUBLIC_ENDPOINT,
            headers: &[],
        };

        let mut connection = wire.open(&route, 3);
        let text = |value: Value| Message::Text(value.to_string().into());

        let snapshot = text(json!({
            "arg": {"channel": "books", "instId": "BTC-USDT"},
            "action": "snapshot",
            "data": [{"bids": [["10", "2"]], "asks": [["11", "4"]], "ts": "5", "seqId": 7}],
        }));
        let mut update = text(json!({
            "arg": {"channel": "books", "instId": "BTC-USDT"},
            "action": "update",
            "data": [{"bids": [["10", "0"]], "asks": [], "ts": "6", "seqId": 8, "prevSeqId": 7}],
        }));
        let spread = text(json!({
            "arg": {"channel": "sprd-books5", "sprdId": "BTC-USDT_BTC-USDT-SWAP"},
            "data": [{"bids": [], "asks": [["1", "1"]], "ts": "7"}],
        }));
        let ack = text(
            json!({"event": "unsubscribe", "arg": {"channel": "books", "instId": "BTC-USDT"}}),
        );
        let command = text(json!({
            "op": "unsubscribe",
            "args": [
                {"channel": "books", "instId": "BTC-USDT"},
                {"channel": "sprd-books5", "sprdId": "BTC-USDT_BTC-USDT-SWAP"},
            ],
        }));

        let book = |key: &str, kind| Upstream::Book {
            key: key.to_string(),
            kind,
        };

        assert_eq!(
            connection.upstream(&snapshot),
            book("BTC-USDT", FrameKind::Snapshot)
        );
        assert_eq!(
            connection.upstream(&update),
            book("BTC-USDT", FrameKind::Update)
        );
        assert_eq!(
            connection.upstream(&spread),
            book("BTC-USDT_BTC-USDT-SWAP", FrameKind::Snapshot)
        );
        assert_eq!(
            connection.upstream(&ack),
            Upstream::Unsubscribed("BTC-USDT".to_string())
        );
        assert_eq!(connection.upstream(&command), Upstream::Other);
        assert_eq!(
            connection.client(&command),
            ["BTC-USDT", "BTC-USDT_BTC-USDT-SWAP"]
        );
        assert!(connection.client(&snapshot).is_empty());

        assert_eq!(
            [
                wire.views.find("BTC-USDT", 7, 5_000_000),
                wire.views.find("BTC-USDT", 8, 6_000_000),
            ],
            [
                Some(WireView {
                    epoch: 3,
                    sequence: 7,
                    timestamp: 5_000_000,
                    book: WireBook {
                        bids: [(Decimal::from(10), Decimal::from(2))].into(),
                        asks: [(Decimal::from(11), Decimal::from(4))].into(),
                    },
                }),
                Some(WireView {
                    epoch: 3,
                    sequence: 8,
                    timestamp: 6_000_000,
                    book: WireBook {
                        bids: [].into(),
                        asks: [(Decimal::from(11), Decimal::from(4))].into(),
                    },
                }),
            ]
        );

        let mut unchanged = snapshot.clone();
        assert!(!connection.corrupt(&mut unchanged, "BTC-USDT", FrameKind::Snapshot));
        assert_eq!(unchanged, snapshot);
        assert!(connection.corrupt(&mut update, "BTC-USDT", FrameKind::Update));
        let corrupted = serde_json::from_str::<Value>(update.to_text().unwrap()).unwrap();
        assert_eq!(corrupted["data"][0]["prevSeqId"], json!(i64::MAX));
    }
}

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
//! `cargo run -p nautilus-okx --features examples --example okx-book-sync-stress -- 10 18`
//!
//! Arguments are snapshot timeout seconds and number of stress rounds. Add `boundaries` as the
//! third argument to run exhaustion and replacement-boundary probes instead, or `turnover` for
//! rapid unsubscribe/resubscribe during recovery. Use `initial` for missing first snapshots.
//! No orders are submitted.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    logging::{init_logging, logger::LoggerConfig},
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, UnsubscribeBookDeltas},
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::SocketReconnectRegistry;
use nautilus_model::{
    data::Data,
    enums::{BookType, RecordFlag},
    identifiers::{InstrumentId, TraderId},
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use nautilus_network::mode::ReconnectRequestOutcome;
use nautilus_okx::{
    common::{consts::OKX_CLIENT_ID, enums::OKXInstrumentType},
    config::OKXDataClientConfig,
    data::OKXDataClient,
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;
use ustr::Ustr;

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
    let timeout = args.get(1).map_or(10, |v| v.parse::<u64>().unwrap());
    let rounds = args.get(2).map_or(18, |v| v.parse::<usize>().unwrap());
    let ids = SYMBOLS.map(|s| InstrumentId::from(format!("{s}.OKX")));
    if args.get(3).is_some_and(|arg| arg == "initial") {
        initial(timeout, rounds, &ids).await;
        return;
    }

    if args.get(3).is_some_and(|arg| arg == "turnover") {
        turnover(timeout, rounds, &ids).await;
        return;
    }

    if args.get(3).is_some_and(|arg| arg == "boundaries") {
        boundaries(timeout, &ids).await;
        return;
    }

    let started = Instant::now();
    let mut total_applied = 0;
    let mut session = Session::connect(timeout).await;
    for id in ids {
        session.subscribe(id);
    }

    session.healthy(&ids).await;
    session
        .recover_without_reconnect(&ids[..6], usize::from(timeout > 0))
        .await;

    for round in 0..rounds {
        let phase = round % 6;
        let phase_started = Instant::now();

        match phase {
            0 => {
                let before = {
                    let mut control = session.wire.control.lock();
                    ids[..6]
                        .iter()
                        .map(|id| {
                            let fault = control.faults.entry(id.symbol.to_string()).or_default();
                            fault.corrupt = 1;
                            fault.drop = 1;
                            (id.symbol.to_string(), fault.dropped)
                        })
                        .collect::<Vec<_>>()
                };

                session
                    .until(
                        Duration::from_secs(20),
                        "simultaneous recoveries lose snapshots",
                        |s| {
                            let control = s.wire.control.lock();
                            before
                                .iter()
                                .all(|(id, dropped)| control.faults[id].dropped > *dropped)
                        },
                    )
                    .await;

                session.reconnect(false);
                session.healthy(&ids[..6]).await;
            }
            1 => {
                for id in &ids[..6] {
                    session
                        .wire
                        .control
                        .lock()
                        .faults
                        .entry(id.symbol.to_string())
                        .or_default()
                        .hold = true;
                }

                session.observe(Duration::from_secs(2)).await;
                session.reconnect(false);
                session.observe(Duration::from_secs(4)).await;
                for id in &ids[..6] {
                    session
                        .wire
                        .control
                        .lock()
                        .faults
                        .get_mut(id.symbol.as_str())
                        .unwrap()
                        .hold = false;
                }

                session.wire.flush.send_replace(());
                session.healthy(&ids[..6]).await;
            }
            2 => {
                let targets = [ids[round / 6 % 2], ids[4 + round / 6 % 2]];
                for id in &targets {
                    let mut control = session.wire.control.lock();
                    let fault = control.faults.entry(id.symbol.to_string()).or_default();
                    fault.corrupt = 1;
                    fault.hold_snapshot = true;
                }

                session
                    .until(
                        Duration::from_secs(20),
                        "replacement snapshots held before unsubscribe",
                        |s| {
                            let control = s.wire.control.lock();
                            targets
                                .iter()
                                .all(|id| control.faults[id.symbol.as_str()].hold)
                        },
                    )
                    .await;

                let before = {
                    let control = session.wire.control.lock();
                    targets
                        .iter()
                        .map(|id| {
                            (
                                id.symbol.to_string(),
                                control
                                    .unsubscribed
                                    .get(id.symbol.as_str())
                                    .copied()
                                    .unwrap_or(0),
                            )
                        })
                        .collect::<Vec<_>>()
                };

                for id in &targets {
                    session.unsubscribe(*id);
                }

                session
                    .until(
                        Duration::from_secs(10),
                        "explicit unsubscribe acknowledged",
                        |s| {
                            let control = s.wire.control.lock();
                            before.iter().all(|(id, count)| {
                                control.unsubscribed.get(id).copied().unwrap_or(0) > *count
                            })
                        },
                    )
                    .await;

                for id in &targets {
                    session.disabled.insert(*id);
                    session
                        .wire
                        .control
                        .lock()
                        .faults
                        .get_mut(id.symbol.as_str())
                        .unwrap()
                        .hold = false;
                }

                session.wire.flush.send_replace(());
                session.reconnect(false);
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
                session.wire.cuts_remaining.store(2, Ordering::SeqCst);
                let cuts = session.wire.cuts.load(Ordering::SeqCst);
                session.reconnect(false);
                session
                    .until(
                        Duration::from_secs(120),
                        "two reconnects cut before snapshots",
                        |s| s.wire.cuts.load(Ordering::SeqCst) == cuts + 2,
                    )
                    .await;
                session.healthy(&ids[..6]).await;
            }
            4 => {
                let before = {
                    let mut control = session.wire.control.lock();
                    ids[6..]
                        .iter()
                        .map(|id| {
                            let fault = control.faults.entry(id.symbol.to_string()).or_default();
                            fault.drop = 1;
                            (id.symbol.to_string(), fault.dropped)
                        })
                        .collect::<Vec<_>>()
                };

                session.reconnect(false);
                session.reconnect(true);
                session
                    .until(
                        Duration::from_secs(60),
                        "business replay snapshots dropped",
                        |s| {
                            let control = s.wire.control.lock();
                            before
                                .iter()
                                .all(|(id, dropped)| control.faults[id].dropped > *dropped)
                        },
                    )
                    .await;

                if timeout == 0 {
                    session.reconnect(true);
                }

                session.healthy(&ids).await;
            }
            5 => {
                let before = {
                    let mut control = session.wire.control.lock();
                    ids[..6]
                        .iter()
                        .map(|id| {
                            let fault = control.faults.entry(id.symbol.to_string()).or_default();
                            fault.corrupt = 1;
                            fault.drop = 3;
                            (id.symbol.to_string(), fault.dropped)
                        })
                        .collect::<Vec<_>>()
                };

                session
                    .until(
                        Duration::from_secs(20),
                        "shutdown with recovery snapshots missing",
                        |s| {
                            let control = s.wire.control.lock();
                            before
                                .iter()
                                .all(|(id, dropped)| control.faults[id].dropped > *dropped)
                        },
                    )
                    .await;

                total_applied += session.stop().await;
                session = Session::connect(timeout).await;
                for id in ids {
                    session.subscribe(id);
                }

                session.healthy(&ids).await;
            }
            _ => unreachable!(),
        }

        session.observe(Duration::from_secs(5)).await;
        let control = session.wire.control.lock();
        let gaps: usize = control.faults.values().map(|f| f.gaps).sum();
        let dropped: usize = control.faults.values().map(|f| f.dropped).sum();
        let held: usize = control.faults.values().map(|f| f.held).sum();
        eprintln!(
            "timeout={timeout} round={} phase={phase} phase_ms={} public_connections={} business_connections={} gaps={gaps} dropped={dropped} held={held} cuts={} oracle_batches={} elapsed_s={}",
            round + 1,
            phase_started.elapsed().as_millis(),
            session.wire.connections[0].load(Ordering::SeqCst),
            session.wire.connections[1].load(Ordering::SeqCst),
            session.wire.cuts.load(Ordering::SeqCst),
            total_applied + session.applied,
            started.elapsed().as_secs()
        );
    }

    total_applied += session.stop().await;
    eprintln!(
        "PASS timeout={timeout} rounds={rounds} instruments={} oracle_batches={total_applied} elapsed_s={}",
        ids.len(),
        started.elapsed().as_secs()
    );
}

async fn initial(timeout: u64, rounds: usize, ids: &[InstrumentId; 8]) {
    let mut applied = 0;

    for round in 0..rounds {
        let mut session = Session::connect(timeout).await;
        for id in ids {
            let mut control = session.wire.control.lock();
            let fault = control.faults.entry(id.symbol.to_string()).or_default();
            fault.drop = 1;
            fault.await_snapshot = true;
        }

        for id in ids {
            session.subscribe(*id);
        }

        session
            .until(Duration::from_secs(20), "initial snapshots dropped", |s| {
                let control = s.wire.control.lock();
                ids.iter()
                    .all(|id| control.faults[id.symbol.as_str()].dropped == 1)
            })
            .await;

        if timeout == 0 {
            session.observe(Duration::from_secs(5)).await;
            assert_eq!(session.applied, 0);
            assert!(session.wire.control.lock().unsubscribed.is_empty());
            session.reconnect(false);
            session.reconnect(true);
        }

        session.healthy(ids).await;
        {
            let control = session.wire.control.lock();

            for id in ids {
                assert_eq!(control.faults[id.symbol.as_str()].gaps, 0);
                assert_eq!(control.faults[id.symbol.as_str()].dropped, 1);
                assert_eq!(
                    control
                        .unsubscribed
                        .get(id.symbol.as_str())
                        .copied()
                        .unwrap_or(0),
                    usize::from(timeout > 0)
                );

                if id.symbol.as_str().contains('_') {
                    assert!(session.snapshots[id] >= 1);
                } else {
                    assert_eq!(session.snapshots[id], 1);
                }
            }
        }

        assert_eq!(
            session
                .wire
                .connections
                .each_ref()
                .map(|n| n.load(Ordering::SeqCst)),
            [if timeout > 0 { 1 } else { 2 }; 2]
        );
        session.observe(Duration::from_secs(2)).await;
        applied += session.stop().await;
        eprintln!(
            "INITIAL PASS timeout={timeout} round={} books=8 gaps=0",
            round + 1
        );
    }

    eprintln!("INITIAL COMPLETE timeout={timeout} rounds={rounds} oracle_batches={applied}");
}

async fn turnover(timeout: u64, rounds: usize, ids: &[InstrumentId; 8]) {
    let mut session = Session::connect(timeout).await;
    let quiet = [ids[0], ids[4], ids[6]];
    for id in quiet {
        session
            .wire
            .control
            .lock()
            .faults
            .entry(id.symbol.to_string())
            .or_default()
            .hold = true;
    }

    for id in ids {
        session.subscribe(*id);
    }

    session.observe(Duration::from_secs(10)).await;
    {
        let mut control = session.wire.control.lock();

        for id in quiet {
            assert!(control.faults[id.symbol.as_str()].held > 0);
            assert_eq!(session.updates.get(&id).copied().unwrap_or(0), 0);
            let replacements = control
                .unsubscribed
                .get(id.symbol.as_str())
                .copied()
                .unwrap_or(0);

            if timeout == 0 {
                assert_eq!(replacements, 0);
            } else {
                assert!(replacements > 0, "initial silence starts recovery for {id}");
            }

            control.faults.get_mut(id.symbol.as_str()).unwrap().hold = false;
        }
    }

    eprintln!("INITIAL SILENCE PASS timeout={timeout} books=3");
    session.wire.flush.send_replace(());
    session.healthy(ids).await;

    for round in 0..rounds {
        let id = ids[if round % 2 == 0 { 0 } else { 4 }];

        let drops = if timeout > 0 { 3 } else { 1 };

        let (gaps, dropped) = {
            let mut control = session.wire.control.lock();
            let fault = control.faults.entry(id.symbol.to_string()).or_default();
            fault.corrupt = 1;
            fault.drop = drops;
            (fault.gaps, fault.dropped)
        };

        session
            .until(Duration::from_secs(30), "turnover recovery boundary", |s| {
                let control = s.wire.control.lock();
                let fault = &control.faults[id.symbol.as_str()];
                if round % 3 == 0 {
                    fault.gaps > gaps
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

        session
            .wire
            .control
            .lock()
            .faults
            .get_mut(id.symbol.as_str())
            .unwrap()
            .drop = 0;
        session.unsubscribe(id);
        session.subscribe(id);
        session.healthy(&[id]).await;
        let unsubscribed = session.wire.control.lock().unsubscribed[id.symbol.as_str()];
        session.observe(Duration::from_secs(5)).await;
        assert_eq!(
            session.wire.control.lock().unsubscribed[id.symbol.as_str()],
            unsubscribed,
            "old recovery does not replace new subscription"
        );
        eprintln!(
            "TURNOVER PASS timeout={timeout} round={} boundary={} instrument={id}",
            round + 1,
            round % 3
        );
    }

    session.healthy(ids).await;
    let applied = session.stop().await;
    eprintln!("TURNOVER COMPLETE timeout={timeout} rounds={rounds} oracle_batches={applied}");
}

async fn boundaries(timeout: u64, ids: &[InstrumentId; 8]) {
    let mut session = Session::connect(timeout).await;
    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
    session
        .recover_without_reconnect(&ids[..6], if timeout > 0 { 3 } else { 0 })
        .await;

    let targets = [ids[0], ids[4]];
    for id in targets {
        let connections = session.wire.connections[0].load(Ordering::SeqCst);
        let cuts = session.wire.cuts.load(Ordering::SeqCst);
        {
            let mut control = session.wire.control.lock();
            let fault = control.faults.entry(id.symbol.to_string()).or_default();
            fault.corrupt = 1;
            fault.cut_unsubscribe = true;
        }

        session.expected_epochs[0] = connections + 1;
        for public_id in &ids[..6] {
            session
                .expected_snapshots
                .insert(*public_id, session.snapshots[public_id] + 1);
        }

        session.healthy(&ids[..6]).await;
        assert_eq!(session.wire.cuts.load(Ordering::SeqCst), cuts + 1);
        assert_eq!(
            session.wire.connections[0].load(Ordering::SeqCst),
            connections + 1
        );
        eprintln!("REPLACEMENT CUT PASS instrument={id}");
    }

    let connections = session
        .wire
        .connections
        .each_ref()
        .map(|n| n.load(Ordering::SeqCst));

    let before = {
        let mut control = session.wire.control.lock();
        targets.map(|id| {
            let unsubscribed = control.unsubscribed[id.symbol.as_str()];
            let fault = control.faults.entry(id.symbol.to_string()).or_default();
            fault.corrupt = 1;
            fault.hold_snapshot = true;
            (id, unsubscribed, fault.held)
        })
    };

    session
        .until(Duration::from_secs(20), "replacement snapshots held", |s| {
            let control = s.wire.control.lock();
            before
                .iter()
                .all(|(id, _, held)| control.faults[id.symbol.as_str()].held > *held)
        })
        .await;

    let updates = targets.map(|id| session.updates[&id]);

    // Held snapshots arrive after exhaustion, including the 180-second budget with deadlines disabled
    session.observe(Duration::from_secs(185)).await;
    {
        let mut control = session.wire.control.lock();

        for (id, unsubscribed, _) in before {
            assert_eq!(
                control.unsubscribed[id.symbol.as_str()],
                unsubscribed + if timeout > 0 { 8 } else { 1 }
            );
            control.faults.get_mut(id.symbol.as_str()).unwrap().hold = false;
        }
    }

    session.wire.flush.send_replace(());
    session.observe(Duration::from_secs(5)).await;
    assert_eq!(
        targets.map(|id| session.updates[&id]),
        updates,
        "exhausted books suppress late snapshots and updates"
    );
    assert_eq!(
        session
            .wire
            .connections
            .each_ref()
            .map(|n| n.load(Ordering::SeqCst)),
        connections
    );
    session
        .healthy(&[ids[1], ids[2], ids[3], ids[5], ids[6], ids[7]])
        .await;
    eprintln!("EXHAUSTION PASS timeout={timeout} books=2 late_frames_suppressed=true reconnects=0");

    let id = targets[0];
    let unsubscribed = session.wire.control.lock().unsubscribed[id.symbol.as_str()];
    session.unsubscribe(id);
    session
        .until(Duration::from_secs(10), "exhausted book unsubscribe", |s| {
            s.wire.control.lock().unsubscribed[id.symbol.as_str()] == unsubscribed + 1
        })
        .await;

    session.disabled.insert(id);
    session.observe(Duration::from_secs(1)).await;
    session.subscribe(id);
    session.healthy(&[id]).await;
    assert_eq!(session.updates[&targets[1]], updates[1]);
    eprintln!("EXHAUSTION RESUBSCRIBE PASS instrument={id}");
    session.reconnect(false);
    session.healthy(&ids[..6]).await;
    eprintln!("EXHAUSTION RECONNECT PASS books=6");

    let cuts = session.wire.cuts.load(Ordering::SeqCst);
    session.wire.cuts_remaining.store(10, Ordering::SeqCst);
    session.reconnect(false);
    session
        .until(
            Duration::from_secs(30),
            "reconnect interrupted by shutdown",
            |s| s.wire.cuts.load(Ordering::SeqCst) > cuts,
        )
        .await;
    let applied = session.stop().await;
    eprintln!("BOUNDARIES PASS timeout={timeout} oracle_batches={applied}");
}

#[derive(Default)]
struct Fault {
    corrupt: usize,
    drop: usize,
    hold: bool,
    hold_snapshot: bool,
    await_snapshot: bool,
    cut_unsubscribe: bool,
    gaps: usize,
    dropped: usize,
    held: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WireBook {
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
}

struct View {
    epoch: usize,
    sequence: u64,
    timestamp: u64,
    book: WireBook,
}

#[derive(Default)]
struct Control {
    faults: HashMap<String, Fault>,
    unsubscribed: HashMap<String, usize>,
    views: HashMap<String, VecDeque<View>>,
}

#[derive(Default)]
struct Wire {
    connections: [AtomicUsize; 2],
    active: AtomicUsize,
    cuts_remaining: AtomicUsize,
    cuts: AtomicUsize,
    flush: tokio::sync::watch::Sender<()>,
    control: Mutex<Control>,
}

struct ProxyConnection(Arc<Wire>);

impl Drop for ProxyConnection {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn public(ws: WebSocketUpgrade, State(wire): State<Arc<Wire>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| proxy(socket, wire, false))
}

async fn business(ws: WebSocketUpgrade, State(wire): State<Arc<Wire>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| proxy(socket, wire, true))
}

async fn proxy(mut socket: WebSocket, wire: Arc<Wire>, business: bool) {
    let endpoint = if business { "business" } else { "public" };
    let (mut upstream, _) =
        tokio_tungstenite::connect_async(format!("wss://ws.okx.com:8443/ws/v5/{endpoint}"))
            .await
            .expect("mainnet websocket");
    wire.active.fetch_add(1, Ordering::SeqCst);
    let _connection = ProxyConnection(Arc::clone(&wire));
    let epoch = wire.connections[usize::from(business)].fetch_add(1, Ordering::SeqCst) + 1;
    let mut books = HashMap::<String, WireBook>::new();
    let mut held = VecDeque::<(String, String)>::new();
    let mut flush = wire.flush.subscribe();

    loop {
        tokio::select! {
            Ok(()) = flush.changed() => {
                let mut remaining = VecDeque::new();

                while let Some((id, text)) = held.pop_front() {
                    let holding = wire.control.lock().faults.get(&id).is_some_and(|f| f.hold);
                    if holding {
                        remaining.push_back((id, text));
                    } else if socket.send(Message::Text(text.into())).await.is_err() {
                        return;
                    }
                }
                held = remaining;
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => {
                    if upstream.send(UpstreamMessage::Text(text.to_string().into())).await.is_err() { break; }
                    let cut = serde_json::from_str::<Value>(&text).is_ok_and(|frame| {
                        frame["op"] == "unsubscribe" && frame["args"].as_array().is_some_and(|args| {
                            let mut control = wire.control.lock();
                            let mut cut = false;

                            for id in args.iter().filter_map(symbol) {
                                if let Some(fault) = control.faults.get_mut(id) {
                                    fault.await_snapshot = false;
                                    cut |= std::mem::take(&mut fault.cut_unsubscribe);
                                }
                            }
                            cut
                        })
                    });

                    if cut {
                        wire.cuts.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                _ => {}
            },
            message = upstream.next() => match message {
                Some(Ok(UpstreamMessage::Text(text))) => {
                    let mut frame: Value = match serde_json::from_str(&text) {
                        Ok(frame) => frame,
                        Err(_) => {
                            if socket.send(Message::Text(text.to_string().into())).await.is_err() { break; }
                            continue;
                        }
                    };

                    if frame["event"] == "error" {
                        eprintln!("venue error endpoint={endpoint}: {frame}");
                    }

                    if frame["event"] == "unsubscribe"
                        && let Some(id) = symbol(&frame["arg"])
                    {
                        *wire.control.lock().unsubscribed.entry(id.to_string()).or_default() += 1;
                    }
                    let channel = frame["arg"]["channel"].as_str().unwrap_or("");
                    let is_book = ["books", "books-rpi", "sprd-books5"].contains(&channel) && frame["data"].is_array();
                    if is_book {
                        let snapshot = frame["action"] == "snapshot" || channel == "sprd-books5";
                        if !business && snapshot && wire.cuts_remaining.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
                            wire.cuts.fetch_add(1, Ordering::SeqCst);
                            break;
                        }
                        let id = symbol(&frame["arg"]).unwrap().to_string();
                        let book = books.entry(id.clone()).or_default();
                        let mut drop_frame = false;
                        let holding;
                        {
                            let mut control = wire.control.lock();

                            for data in frame["data"].as_array().unwrap() {
                                book.apply(data, snapshot);
                                let view = View {
                                    epoch,
                                    sequence: data["seqId"].as_u64().unwrap_or(0),
                                    timestamp: data["ts"].as_str().unwrap().parse::<u64>().unwrap() * 1_000_000,
                                    book: book.top(),
                                };
                                let views = control.views.entry(id.clone()).or_default();
                                views.push_back(view);
                                if views.len() > 2048 { views.pop_front(); }
                            }
                            let fault = control.faults.entry(id.clone()).or_default();
                            if !snapshot && fault.corrupt > 0 {
                                frame["data"][0]["prevSeqId"] = json!(i64::MAX);
                                fault.corrupt -= 1;
                                fault.gaps += 1;
                            }

                            if snapshot && fault.drop > 0 {
                                fault.drop -= 1;
                                fault.dropped += 1;
                                drop_frame = true;
                            }

                            if epoch > 1 {
                                fault.await_snapshot = false;
                            }

                            if fault.await_snapshot {
                                drop_frame = true;
                            }

                            if snapshot && fault.hold_snapshot {
                                fault.hold = true;
                                fault.hold_snapshot = false;
                            }
                            holding = fault.hold || held.iter().any(|(held_id, _)| held_id == &id);
                            if holding { fault.held += 1; }
                        }

                        if drop_frame { continue; }

                        if holding {
                            held.push_back((id, frame.to_string()));
                            assert!(held.len() < 20_000, "bounded delayed-frame queue");
                            continue;
                        }
                    }

                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() { break; }
                }
                Some(Ok(UpstreamMessage::Close(_)) | Err(_)) | None => break,
                _ => {}
            }
        }
    }

    let _ = tokio::time::timeout(Duration::from_secs(1), upstream.close(None)).await;
}

fn symbol(arg: &Value) -> Option<&str> {
    arg["instId"].as_str().or_else(|| arg["sprdId"].as_str())
}

impl WireBook {
    fn apply(&mut self, data: &Value, snapshot: bool) {
        if snapshot {
            self.bids.clear();
            self.asks.clear();
        }

        for (side, levels) in [("bids", &mut self.bids), ("asks", &mut self.asks)] {
            for row in data[side].as_array().unwrap() {
                let price = Decimal::from_str(row[0].as_str().unwrap()).unwrap();
                let size = Decimal::from_str(row[1].as_str().unwrap()).unwrap();
                if size.is_zero() {
                    levels.remove(&price);
                } else {
                    levels.insert(price, size);
                }
            }
        }
    }

    fn top(&self) -> Self {
        Self {
            bids: self
                .bids
                .iter()
                .rev()
                .take(20)
                .map(|(p, q)| (*p, *q))
                .collect(),
            asks: self.asks.iter().take(20).map(|(p, q)| (*p, *q)).collect(),
        }
    }
}

struct Session {
    client: OKXDataClient,
    wire: Arc<Wire>,
    registry: SocketReconnectRegistry,
    events: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    server: tokio::task::JoinHandle<()>,
    books: HashMap<InstrumentId, OrderBook>,
    snapshots: HashMap<InstrumentId, usize>,
    updates: HashMap<InstrumentId, usize>,
    disabled: HashSet<InstrumentId>,
    applied: usize,
    expected_epochs: [usize; 2],
    epochs: HashMap<InstrumentId, usize>,
    expected_snapshots: HashMap<InstrumentId, usize>,
}

impl Session {
    async fn connect(snapshot_timeout: u64) -> Self {
        let wire = Arc::new(Wire::default());
        let router = Router::new()
            .route("/public", get(public))
            .route("/business", get(business))
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

        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot, OKXInstrumentType::Swap],
            load_spreads: true,
            base_url_ws_public: Some(format!("ws://{addr}/public")),
            base_url_ws_business: Some(format!("ws://{addr}/business")),
            update_instruments_interval_mins: 0,
            book_stale_check_interval_secs: 0,
            book_snapshot_timeout_secs: snapshot_timeout,
            ..OKXDataClientConfig::default()
        };

        let mut client = registry.scope(|| OKXDataClient::new(*OKX_CLIENT_ID, config).unwrap());
        eprintln!("Connecting mainnet market data, snapshot_timeout={snapshot_timeout}");
        tokio::time::timeout(Duration::from_secs(60), client.connect())
            .await
            .expect("bounded connect")
            .unwrap();
        eprintln!("Connected public and business sockets");

        Self {
            client,
            wire,
            registry,
            events,
            server,
            books: HashMap::new(),
            snapshots: HashMap::new(),
            updates: HashMap::new(),
            disabled: HashSet::new(),
            applied: 0,
            expected_epochs: [1, 1],
            epochs: HashMap::new(),
            expected_snapshots: HashMap::new(),
        }
    }

    fn subscribe(&mut self, id: InstrumentId) {
        let rpi = id.symbol.as_str().ends_with("-SWAP") && !id.symbol.as_str().contains('_');
        let params: Option<Params> =
            rpi.then(|| serde_json::from_value(json!({"rpi": true})).unwrap());
        self.disabled.remove(&id);
        self.expected_snapshots
            .insert(id, self.snapshots.get(&id).copied().unwrap_or(0) + 1);
        self.books
            .entry(id)
            .or_insert_with(|| OrderBook::new(id, BookType::L2_MBP));
        self.client
            .subscribe_book_deltas(SubscribeBookDeltas::new(
                id,
                BookType::L2_MBP,
                Some(*OKX_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                true,
                None,
                params,
            ))
            .unwrap();
    }

    fn unsubscribe(&mut self, id: InstrumentId) {
        self.client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                id,
                Some(*OKX_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    }

    fn reconnect(&mut self, business: bool) {
        self.expected_epochs[usize::from(business)] = self.wire.connections[usize::from(business)]
            .load(Ordering::SeqCst)
            + 1
            + if business {
                0
            } else {
                self.wire.cuts_remaining.load(Ordering::SeqCst)
            };

        for id in self.books.keys().filter(|id| {
            id.symbol.as_str().contains('_') == business && !self.disabled.contains(id)
        }) {
            self.expected_snapshots
                .insert(*id, self.snapshots.get(id).copied().unwrap_or(0) + 1);
        }

        let endpoint = if business {
            "okx-business-data-streams"
        } else {
            "okx-public-data-streams"
        };

        let handle = self
            .registry
            .handle(*OKX_CLIENT_ID, Ustr::from(endpoint))
            .unwrap();
        assert_eq!(
            handle.request_reconnect(),
            ReconnectRequestOutcome::Accepted
        );
    }

    fn apply(&mut self, event: DataEvent) {
        let DataEvent::Data(Data::BookDeltas(deltas)) = event else {
            return;
        };

        let id = deltas.instrument_id;
        assert!(
            !self.disabled.contains(&id),
            "output after settled unsubscribe: {id}"
        );
        let snapshot = deltas
            .deltas
            .first()
            .is_some_and(|d| RecordFlag::F_SNAPSHOT.matches(d.flags));
        if snapshot {
            *self.snapshots.entry(id).or_default() += 1;
        }

        let book = self.books.get_mut(&id).expect("requested book");
        book.apply_deltas(&deltas).unwrap();
        book_check_integrity(book).unwrap();
        let control = self.wire.control.lock();

        let view = control
            .views
            .get(id.symbol.as_str())
            .and_then(|views| {
                views.iter().rev().find(|v| {
                    v.sequence == deltas.sequence && v.timestamp == deltas.ts_event.as_u64()
                })
            })
            .expect("wire oracle at emitted sequence and timestamp");

        assert_eq!(
            book.bids_as_map(Some(20))
                .into_iter()
                .collect::<BTreeMap<_, _>>(),
            view.book.bids,
            "bid oracle mismatch {id} seq={} ts={}",
            deltas.sequence,
            deltas.ts_event
        );
        assert_eq!(
            book.asks_as_map(Some(20))
                .into_iter()
                .collect::<BTreeMap<_, _>>(),
            view.book.asks,
            "ask oracle mismatch {id} seq={} ts={}",
            deltas.sequence,
            deltas.ts_event
        );
        self.epochs.insert(id, view.epoch);
        *self.updates.entry(id).or_default() += 1;
        self.applied += 1;
    }

    async fn until(&mut self, limit: Duration, label: &str, predicate: impl Fn(&Self) -> bool) {
        let result = tokio::time::timeout(limit, async {
            while !predicate(self) {
                tokio::select! {
                    event = self.events.recv() => self.apply(event.expect("data stream stays open")),
                    () = tokio::time::sleep(Duration::from_millis(10)) => {},
                }
            }
        }).await;

        assert!(
            result.is_ok(),
            "deadline exceeded: {label}, updates={:?}, snapshots={:?}",
            self.updates,
            self.snapshots
        );
        self.drain();
    }

    fn drain(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.apply(event);
        }
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
        let before = ids
            .iter()
            .map(|id| (*id, self.updates.get(id).copied().unwrap_or(0)))
            .collect::<Vec<_>>();
        self.until(Duration::from_secs(90), "all intended books recover", |s| {
            before.iter().all(|(id, updates)| {
                let business = id.symbol.as_str().contains('_');
                s.epochs.get(id).copied().unwrap_or(0) >= s.expected_epochs[usize::from(business)]
                    && (business || s.updates.get(id).copied().unwrap_or(0) >= updates + 5)
                    && s.snapshots.get(id).copied().unwrap_or(0) >= s.expected_snapshots[id]
            })
        })
        .await;
    }

    async fn recover_without_reconnect(&mut self, ids: &[InstrumentId], drops: usize) {
        let connections = self
            .wire
            .connections
            .each_ref()
            .map(|n| n.load(Ordering::SeqCst));

        let before = {
            let mut control = self.wire.control.lock();
            ids.iter()
                .map(|id| {
                    let unsubscribed = control
                        .unsubscribed
                        .get(id.symbol.as_str())
                        .copied()
                        .unwrap_or(0);
                    let fault = control.faults.entry(id.symbol.to_string()).or_default();
                    fault.corrupt = 1;
                    fault.drop = drops;
                    (
                        *id,
                        fault.gaps,
                        fault.dropped,
                        unsubscribed,
                        self.snapshots[id],
                    )
                })
                .collect::<Vec<_>>()
        };

        for (id, _, _, _, snapshots) in &before {
            self.expected_snapshots.insert(*id, snapshots + 1);
        }

        self.healthy(ids).await;
        self.observe(Duration::from_secs(5)).await;
        let control = self.wire.control.lock();

        for (id, gaps, dropped, unsubscribed, snapshots) in before {
            let fault = &control.faults[id.symbol.as_str()];
            assert_eq!(fault.gaps, gaps + 1);
            assert_eq!(fault.dropped, dropped + drops);
            assert_eq!(
                control.unsubscribed[id.symbol.as_str()],
                unsubscribed + 1 + drops
            );
            assert_eq!(self.snapshots[&id], snapshots + 1);
        }

        assert_eq!(
            self.wire
                .connections
                .each_ref()
                .map(|n| n.load(Ordering::SeqCst)),
            connections
        );
        eprintln!(
            "AUTONOMOUS PASS books={} dropped={} reconnects=0",
            ids.len(),
            ids.len() * drops
        );
    }

    async fn stop(mut self) -> usize {
        let started = Instant::now();
        tokio::time::timeout(Duration::from_secs(10), self.client.disconnect())
            .await
            .expect("bounded disconnect")
            .unwrap();
        assert!(self.client.is_disconnected());
        wait_until_async(
            || async { self.wire.active.load(Ordering::SeqCst) == 0 },
            Duration::from_secs(5),
        )
        .await;

        for endpoint in ["okx-public-data-streams", "okx-business-data-streams"] {
            assert!(
                self.registry
                    .handle(*OKX_CLIENT_ID, Ustr::from(endpoint))
                    .is_none()
            );
        }

        let control = self.wire.control.lock();
        eprintln!(
            "shutdown_ms={} active_proxies=0 applied={} public_connections={} business_connections={} gaps={} dropped={} held={} cuts={}",
            started.elapsed().as_millis(),
            self.applied,
            self.wire.connections[0].load(Ordering::SeqCst),
            self.wire.connections[1].load(Ordering::SeqCst),
            control.faults.values().map(|f| f.gaps).sum::<usize>(),
            control.faults.values().map(|f| f.dropped).sum::<usize>(),
            control.faults.values().map(|f| f.held).sum::<usize>(),
            self.wire.cuts.load(Ordering::SeqCst),
        );
        self.server.abort();
        self.applied
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn wire_book_applies_updates_and_replaces_snapshots() {
        let mut book = WireBook::default();
        book.apply(
            &json!({"bids": [["10", "2"], ["9", "3"]], "asks": [["11", "4"], ["12", "5"]]}),
            true,
        );
        book.apply(
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

        book.apply(&json!({"bids": [["8", "9"]], "asks": []}), true);
        assert_eq!(
            book,
            WireBook {
                bids: [(Decimal::from(8), Decimal::from(9))].into(),
                asks: BTreeMap::new(),
            }
        );
        book.apply(&json!({"bids": [], "asks": []}), true);
        assert_eq!(book, WireBook::default());
    }

    #[rstest]
    fn wire_book_selects_best_twenty_levels() {
        let book = WireBook {
            bids: (1..=21)
                .map(|n| (Decimal::from(n), Decimal::from(n + 40)))
                .collect(),
            asks: (22..=42)
                .map(|n| (Decimal::from(n), Decimal::from(n + 70)))
                .collect(),
        };

        assert_eq!(
            book.top(),
            WireBook {
                bids: (2..=21)
                    .map(|n| (Decimal::from(n), Decimal::from(n + 40)))
                    .collect(),
                asks: (22..=41)
                    .map(|n| (Decimal::from(n), Decimal::from(n + 70)))
                    .collect(),
            }
        );
    }
}

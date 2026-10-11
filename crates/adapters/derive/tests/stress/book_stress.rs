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

//! Public testnet book validation through a fault-injecting relay.
//! Every emitted snapshot is compared with its raw venue frame, including empty spot books.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};

use nautilus_common::clients::DataClient;
use nautilus_derive::{
    common::{consts::DERIVE_CLIENT_ID, enums::DeriveEnvironment},
    config::DeriveDataClientConfig,
    data::DeriveDataClient,
};
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::OrderBookDeltas,
    identifiers::{ClientId, InstrumentId},
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use stress::{
    BookProgress, Coverage, FrameKind, Route, Session, StressArgs, StressVenue, Upstream, WireBook,
    WireCodec, WireConnection, WireView, WireViews,
};
use tokio_tungstenite::tungstenite::Message;

const STREAM: &str = "ws";
const ENDPOINT: &str = "derive-data-streams";
const DEPTH: usize = 10;
const STALE_SECS: u64 = 4;

type DeriveSession = Session<Derive>;

fn main() {
    stress::run::<Derive, _, _>(|args| async move {
        let ids = [
            InstrumentId::from("ETH-PERP.DERIVE"),
            InstrumentId::from("ETH-USDC.DERIVE"),
        ];
        let mut session = DeriveSession::connect(&args).await;
        subscribe_books(&mut session, &ids, args.scenario() == "initial").await;
        if args.scenario() == "edges" {
            recover_budget(&mut session, &ids).await;
            recover_rejected(&mut session, &ids).await;
        } else {
            for round in 0..args.rounds() {
                if args.scenario() == "disabled" {
                    disabled_round(&mut session, &ids).await;
                } else {
                    run_round(&mut session, &ids, round).await;
                }

                session.round(round, "books=2");
            }
        }

        if args.scenario() != "disabled" {
            interrupt_recovery(&mut session, &ids).await;
        }

        session.stop().await
    });
}

async fn subscribe_books(session: &mut DeriveSession, ids: &[InstrumentId], missing: bool) {
    if missing {
        for id in ids {
            session.fault(id).silence = true;
        }
    }

    for id in ids {
        session.subscribe(*id);
    }

    session.healthy(ids).await;
    if missing {
        for id in ids {
            assert_eq!(session.fault(id).unsubscribes, 1);
        }

        assert_eq!(session.proxy().connections(STREAM), 1);
        stress::check("initial", "missing_snapshots=2 reconnects=0");
    }
}

async fn run_round(session: &mut DeriveSession, ids: &[InstrumentId], round: usize) {
    match round % 5 {
        0 => recover_stale(session, ids).await,
        1 => held_frames(session, ids).await,
        2 => reconnect_recovery(session, ids).await,
        3 => {
            for id in ids {
                session.unsubscribe(*id);
            }

            session.observe(Duration::from_secs(1)).await;
            for id in ids {
                session.close(*id);
            }

            session.observe(Duration::from_secs(1)).await;
            for id in ids {
                session.subscribe(*id);
            }

            session.healthy(ids).await;
        }
        _ => corrupt_frames(session, ids).await,
    }
}

async fn corrupt_frames(session: &mut DeriveSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.corrupt = 1;
            fault.corrupted
        })
        .collect::<Vec<_>>();

    session
        .until(Duration::from_secs(20), "corrupt snapshots injected", |s| {
            ids.iter()
                .zip(&before)
                .all(|(id, corrupted)| s.fault(id).corrupted == *corrupted + 1)
        })
        .await;

    session.expect_all();
    session.healthy(ids).await;
}

async fn interrupt_recovery(session: &mut DeriveSession, ids: &[InstrumentId]) {
    for id in ids {
        session.fault(id).silence = true;
        session.fault(id).drop_snapshots = usize::MAX;
    }

    let before = ids
        .iter()
        .map(|id| session.fault(id).unsubscribes)
        .collect::<Vec<_>>();
    session
        .until(
            Duration::from_secs(20),
            "recoveries active before shutdown",
            |s| {
                ids.iter()
                    .zip(&before)
                    .all(|(id, count)| s.fault(id).unsubscribes > *count)
            },
        )
        .await;

    session.reconnect(ENDPOINT);
}

async fn recover_stale(session: &mut DeriveSession, ids: &[InstrumentId]) {
    let connections = session.proxy().connections(STREAM);
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.drop_snapshots = usize::MAX;
            fault.unsubscribes
        })
        .collect::<Vec<_>>();

    session
        .until(Duration::from_secs(20), "stale book replacements", |s| {
            let mut ready = true;

            for (id, count) in ids.iter().zip(&before) {
                let mut fault = s.fault(id);
                if fault.unsubscribes > *count {
                    fault.drop_snapshots = 0;
                } else {
                    ready = false;
                }
            }

            ready
        })
        .await;

    session.expect_all();
    session.healthy(ids).await;
    let attempts = ids
        .iter()
        .zip(before)
        .map(|(id, count)| session.fault(id).unsubscribes - count)
        .collect::<Vec<_>>();

    assert_eq!(session.proxy().connections(STREAM), connections);
    stress::check("stale", format!("reconnects=0 attempts={attempts:?}"));
}

async fn held_frames(session: &mut DeriveSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.hold_snapshot = true;
            (fault.unsubscribes, fault.held)
        })
        .collect::<Vec<_>>();

    session
        .until(
            Duration::from_secs(20),
            "held snapshots force recovery",
            |s| {
                ids.iter().zip(&before).all(|(id, (unsubscribes, held))| {
                    let f = s.fault(id);
                    f.unsubscribes > *unsubscribes && f.held > *held
                })
            },
        )
        .await;

    for id in ids {
        session.fault(id).hold_snapshot = false;
        session.fault(id).hold = false;
    }

    session.proxy().release();
    session.expect_all();
    session.healthy(ids).await;
    stress::check("delayed", "obsolete_frames_released=true");
}

async fn reconnect_recovery(session: &mut DeriveSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| {
            let mut f = session.fault(id);
            f.silence = true;
            f.drop_snapshots = usize::MAX;
            f.unsubscribes
        })
        .collect::<Vec<_>>();

    session
        .until(
            Duration::from_secs(20),
            "recovery replacements before reconnect",
            |s| {
                ids.iter()
                    .zip(&before)
                    .all(|(id, count)| s.fault(id).unsubscribes > *count)
            },
        )
        .await;

    session.venue_mut().expected_epoch = session.proxy().connections(STREAM) + 1;
    session.reconnect(ENDPOINT);
    for id in ids {
        session.fault(id).drop_snapshots = 0;
    }

    session.healthy(ids).await;
    stress::check("reconnect", "mid_recovery=true fresh_snapshots=2");
}

async fn disabled_round(session: &mut DeriveSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| session.fault(id).unsubscribes)
        .collect::<Vec<_>>();

    for id in ids {
        session.fault(id).drop_snapshots = usize::MAX;
    }

    session.observe(Duration::from_secs(10)).await;
    let snapshots = ids
        .iter()
        .map(|id| session.book(id).snapshots)
        .collect::<Vec<_>>();
    session.venue_mut().expected_epoch = session.proxy().connections(STREAM) + 1;
    session.reconnect(ENDPOINT);
    session.observe(Duration::from_secs(10)).await;

    for ((id, count), snapshots) in ids.iter().zip(before).zip(snapshots) {
        assert_eq!(session.fault(id).unsubscribes, count);
        assert_eq!(session.book(id).snapshots, snapshots);
        session.fault(id).drop_snapshots = 0;
    }

    session.expect_all();
    session.healthy(ids).await;
    stress::check("disabled", "quiet_before_and_after_reconnect=true");
}

async fn recover_budget(session: &mut DeriveSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| {
            let mut f = session.fault(id);
            f.silence = true;
            f.drop_snapshots = usize::MAX;
            f.unsubscribes
        })
        .collect::<Vec<_>>();

    session.expect_all();
    session
        .until(Duration::from_secs(180), "eight recovery attempts", |s| {
            ids.iter()
                .zip(&before)
                .all(|(id, count)| s.fault(id).unsubscribes == count + 8)
        })
        .await;

    session.observe(Duration::from_secs(5)).await;

    for (id, count) in ids.iter().zip(&before) {
        assert_eq!(session.fault(id).unsubscribes, count + 8);
        session.fault(id).drop_snapshots = 0;
    }

    session.healthy(ids).await;
    for (id, count) in ids.iter().zip(before) {
        assert_eq!(session.fault(id).unsubscribes, count + 8);
    }

    stress::check("budget", "attempts=8 late_snapshot_restored=true");
}

async fn recover_rejected(session: &mut DeriveSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| {
            let mut f = session.fault(id);
            f.silence = true;
            f.reject = 1;
            (f.unsubscribes, f.rejected)
        })
        .collect::<Vec<_>>();

    session.expect_all();
    session
        .until(
            Duration::from_secs(20),
            "permanent replacement rejection",
            |s| {
                ids.iter()
                    .zip(&before)
                    .all(|(id, (_, rejected))| s.fault(id).rejected == rejected + 1)
            },
        )
        .await;

    session.observe(Duration::from_secs(5)).await;

    for (id, (count, _)) in ids.iter().zip(&before) {
        assert_eq!(session.fault(id).unsubscribes, count + 1);
    }

    session.healthy_within(ids, Duration::from_secs(90)).await;
    for (id, (count, rejected)) in ids.iter().zip(before) {
        assert_eq!(session.fault(id).unsubscribes, count + 2);
        assert_eq!(session.fault(id).rejected, rejected + 1);
    }

    stress::check("ceiling", "permanent_errors=2 restored_at_ceiling=true");
}

struct Derive {
    wire: DeriveWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Derive {
    const NAME: &'static str = "derive";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "disabled", "edges"];
    const ROUNDS: usize = 20;
    const SEQUENCED: bool = true;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(30);

    fn self_check() {
        let wire = DeriveWire::default();

        let route = Route {
            name: STREAM,
            path: "/ws",
            upstream: String::new(),
            endpoint: ENDPOINT,
            headers: &[],
        };

        let mut connection = wire.open(&route, 3);

        for (symbol, fixture) in [
            (
                "ETH-PERP",
                include_str!("../../test_data/perps/ws_orderbook_eth.json"),
            ),
            (
                "ETH-USDC",
                include_str!("../../test_data/spot/ws_orderbook_eth.json"),
            ),
        ] {
            let data: Value = serde_json::from_str(fixture).unwrap();
            let channel = format!("orderbook.{symbol}.1.10");
            let message = Message::Text(
                json!({"method": "subscription", "params": {"channel": channel, "data": data}})
                    .to_string()
                    .into(),
            );
            assert!(matches!(
                connection.upstream(&message),
                Upstream::Book {
                    kind: FrameKind::Snapshot,
                    ..
                }
            ));
            let timestamp = data["timestamp"].as_u64().unwrap();
            let view = wire
                .views
                .find(&channel, timestamp, timestamp * 1_000_000)
                .unwrap();
            assert_eq!(view.epoch, 3);
            assert_eq!(view.book.bids.len(), data["bids"].as_array().unwrap().len());
            assert_eq!(view.book.asks.len(), data["asks"].as_array().unwrap().len());

            for (side, levels) in [
                (&view.book.bids, &data["bids"]),
                (&view.book.asks, &data["asks"]),
            ] {
                for (price, size) in parse_levels(levels) {
                    assert_eq!(side.get(&price), Some(&size));
                }
            }
        }
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: DeriveWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *DERIVE_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "/ws",
            upstream: "wss://testnet.api.derive.xyz/v3/ws".to_owned(),
            endpoint: ENDPOINT,
            headers: &[],
        }]
    }

    fn codec(&self) -> Arc<dyn WireCodec> {
        Arc::new(self.wire.clone())
    }

    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>> {
        let stale = if args.scenario() == "disabled" {
            anyhow::ensure!(
                args.timeout_secs() == 0,
                "disabled scenario requires --timeout 0"
            );
            0
        } else {
            STALE_SECS
        };

        let config = DeriveDataClientConfig::builder()
            .environment(DeriveEnvironment::Testnet)
            .base_url_ws(format!("ws://{proxy}/ws"))
            .currencies(vec!["ETH".to_owned()])
            .update_instruments_interval_mins(0)
            .book_snapshot_timeout_secs(args.timeout_secs())
            .book_stale_threshold_secs(stale)
            .build();
        Ok(Box::new(DeriveDataClient::new(*DERIVE_CLIENT_ID, config)?))
    }

    fn key(&self, id: &InstrumentId) -> String {
        format!("orderbook.{}.1.10", id.symbol)
    }

    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas) {
        let id = deltas.instrument_id;
        let view = self
            .wire
            .views
            .find(&self.key(&id), deltas.sequence, deltas.ts_event.as_u64())
            .expect("wire oracle at emitted timestamp");
        checker
            .verify(id, DEPTH, &view.book.bids, &view.book.asks)
            .expect("book equals relayed venue snapshot");
        self.epochs.insert(id, view.epoch);
    }

    fn streaming(&self, id: &InstrumentId, book: &BookProgress, start: &BookProgress) -> bool {
        self.epochs.get(id).copied().unwrap_or(0) >= self.expected_epoch
            && book.batches >= start.batches + 2
    }
}

#[derive(Clone, Default)]
struct DeriveWire {
    views: WireViews,
}
impl WireCodec for DeriveWire {
    fn open(&self, _route: &Route, epoch: usize) -> Box<dyn WireConnection> {
        Box::new(DeriveConnection {
            views: self.views.clone(),
            epoch,
            unsubscribes: HashMap::new(),
        })
    }
}
struct DeriveConnection {
    views: WireViews,
    epoch: usize,
    unsubscribes: HashMap<u64, String>,
}
impl WireConnection for DeriveConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        if let Some(id) = frame["id"].as_u64()
            && let Some(channel) = self.unsubscribes.remove(&id)
            && frame["result"]["status"][&channel] == "ok"
        {
            return Upstream::Unsubscribed(channel);
        }

        let params = &frame["params"];

        let Some(channel) = params["channel"]
            .as_str()
            .filter(|channel| channel.starts_with("orderbook."))
        else {
            return Upstream::Other;
        };

        let data = &params["data"];
        let mut book = WireBook::default();
        book.apply(&parse_levels(&data["bids"]), &parse_levels(&data["asks"]));
        let timestamp = data["timestamp"].as_u64().unwrap();
        self.views.record(
            channel,
            WireView {
                epoch: self.epoch,
                sequence: timestamp,
                timestamp: timestamp * 1_000_000,
                book: book.top(DEPTH),
            },
        );

        Upstream::Book {
            key: channel.to_owned(),
            kind: FrameKind::Snapshot,
        }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        let Message::Text(text) = message else {
            return Vec::new();
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };

        if frame["method"] != "unsubscribe" {
            return Vec::new();
        }

        let channels = frame["params"]["channels"].as_array().unwrap();
        let keys = channels
            .iter()
            .filter_map(Value::as_str)
            .filter(|c| c.starts_with("orderbook."))
            .map(str::to_owned)
            .collect::<Vec<_>>();

        if let Some(key) = keys.first() {
            self.unsubscribes
                .insert(frame["id"].as_u64().unwrap(), key.clone());
        }

        keys
    }

    fn corrupt(&mut self, message: &mut Message, _key: &str, _kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        let mut frame: Value = serde_json::from_str(text).unwrap();
        frame["params"]["data"]["timestamp"] = json!(u64::MAX);
        *message = Message::Text(frame.to_string().into());
        true
    }

    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let Message::Text(text) = message else {
            return None;
        };

        let frame: Value = serde_json::from_str(text).ok()?;
        if frame["method"] != "subscribe" {
            return None;
        }

        let channel = frame["params"]["channels"][0].as_str()?;
        if !channel.starts_with("orderbook.") {
            return None;
        }

        let reply =
            json!({"id": frame["id"], "error": {"code": -32602, "message": "subscribe denied"}});
        Some((channel.to_owned(), Message::Text(reply.to_string().into())))
    }
}

fn parse_levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
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

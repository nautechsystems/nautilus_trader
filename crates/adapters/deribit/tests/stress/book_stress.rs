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

//! Public mainnet book validation through a fault-injecting relay.
//! Every emitted grouped snapshot is compared with its raw venue frame.
//!
//! The relay reads Deribit's public mainnet book feeds, which flow continuously; testnet books
//! outside ETH-PERPETUAL sit silent for minutes, so absence faults are untestable there. The
//! harness subscribes read-only public channels and places no orders.

#[path = "../../../../live/tests/book/stress/mod.rs"]
mod stress;

use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};

use nautilus_common::clients::DataClient;
use nautilus_deribit::{
    common::consts::{DERIBIT_BOOK_DEFAULT_DEPTH, DERIBIT_BOOK_DEFAULT_GROUP, DERIBIT_CLIENT_ID},
    common::enums::DeribitEnvironment,
    config::DeribitDataClientConfig,
    data::DeribitDataClient,
    http::models::DeribitProductType,
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
const ENDPOINT: &str = "deribit-data-streams";
const DEPTH: usize = 10;
const STALE_SECS: u64 = 4;

type DeribitSession = Session<Deribit>;

fn main() {
    stress::run::<Deribit, _, _>(|args| async move {
        let ids = [
            InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
            InstrumentId::from("ETH-PERPETUAL.DERIBIT"),
        ];
        let mut session = DeribitSession::connect(&args).await;
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

async fn subscribe_books(session: &mut DeribitSession, ids: &[InstrumentId], missing: bool) {
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

async fn run_round(session: &mut DeribitSession, ids: &[InstrumentId], round: usize) {
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

async fn corrupt_frames(session: &mut DeribitSession, ids: &[InstrumentId]) {
    let before = ids
        .iter()
        .map(|id| {
            let mut fault = session.fault(id);
            fault.corrupt = 1;
            (fault.unsubscribes, fault.corrupted)
        })
        .collect::<Vec<_>>();

    session
        .until(Duration::from_secs(20), "corrupt snapshots injected", |s| {
            ids.iter()
                .zip(&before)
                .all(|(id, (_, corrupted))| s.fault(id).corrupted == *corrupted + 1)
        })
        .await;

    session
        .until(
            Duration::from_secs(20),
            "corrupt snapshots force recovery",
            |s| {
                ids.iter()
                    .zip(&before)
                    .all(|(id, (unsubscribes, _))| s.fault(id).unsubscribes > *unsubscribes)
            },
        )
        .await;

    session.expect_all();
    session.healthy(ids).await;
}

async fn interrupt_recovery(session: &mut DeribitSession, ids: &[InstrumentId]) {
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

async fn recover_stale(session: &mut DeribitSession, ids: &[InstrumentId]) {
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

async fn held_frames(session: &mut DeribitSession, ids: &[InstrumentId]) {
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

async fn reconnect_recovery(session: &mut DeribitSession, ids: &[InstrumentId]) {
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

async fn disabled_round(session: &mut DeribitSession, ids: &[InstrumentId]) {
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

async fn recover_budget(session: &mut DeribitSession, ids: &[InstrumentId]) {
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

async fn recover_rejected(session: &mut DeribitSession, ids: &[InstrumentId]) {
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

struct Deribit {
    wire: DeribitWire,
    expected_epoch: usize,
    epochs: HashMap<InstrumentId, usize>,
}

impl StressVenue for Deribit {
    const NAME: &'static str = "deribit";
    const SCENARIOS: &'static [&'static str] = &["churn", "initial", "disabled", "edges"];
    const ROUNDS: usize = 14;
    const SEQUENCED: bool = false;
    const COVERAGE: Coverage = Coverage::Episodes;
    const HEALTHY_LIMIT: Duration = Duration::from_secs(60);

    fn self_check() {
        let wire = DeribitWire::default();

        let route = Route {
            name: STREAM,
            path: "/ws/api/v2",
            upstream: String::new(),
            endpoint: ENDPOINT,
            headers: &[],
        };

        let mut connection = wire.open(&route, 3);

        for (channel, fixture, kind) in [
            (
                "book.BTC-PERPETUAL.none.10.100ms",
                include_str!("../../test_data/ws_book_grouped_snapshot.json"),
                FrameKind::Snapshot,
            ),
            (
                "book.BTC-PERPETUAL.100ms",
                include_str!("../../test_data/ws_book_snapshot.json"),
                FrameKind::Snapshot,
            ),
            (
                "book.BTC-PERPETUAL.100ms",
                include_str!("../../test_data/ws_book_delta.json"),
                FrameKind::Update,
            ),
        ] {
            let message = Message::Text(fixture.into());
            assert!(
                matches!(connection.upstream(&message), Upstream::Book { key, kind: actual } if key == channel && actual == kind),
                "classifies {channel} fixture",
            );

            let frame: Value = serde_json::from_str(fixture).unwrap();
            let data = &frame["params"]["data"];
            let sequence = data["change_id"].as_u64().unwrap();
            let timestamp = data["timestamp"].as_u64().unwrap() * 1_000_000;
            let view = wire.views.find(channel, sequence, timestamp).unwrap();
            assert_eq!(view.epoch, 3);

            if kind == FrameKind::Snapshot {
                assert_eq!(view.book.bids.len(), data["bids"].as_array().unwrap().len());
                assert_eq!(view.book.asks.len(), data["asks"].as_array().unwrap().len());
            }

            for (side, levels) in [
                (&view.book.bids, &data["bids"]),
                (&view.book.asks, &data["asks"]),
            ] {
                for (price, size) in parse_levels(levels) {
                    if size.is_zero() {
                        assert_eq!(side.get(&price), None);
                    } else {
                        assert_eq!(side.get(&price), Some(&size));
                    }
                }
            }
        }

        let mut corrupted =
            Message::Text(include_str!("../../test_data/ws_book_grouped_snapshot.json").into());
        assert!(connection.corrupt(
            &mut corrupted,
            "book.BTC-PERPETUAL.none.10.100ms",
            FrameKind::Snapshot
        ));
    }

    fn new(_args: &StressArgs) -> Self {
        Self {
            wire: DeribitWire::default(),
            expected_epoch: 1,
            epochs: HashMap::new(),
        }
    }

    fn client_id(&self) -> ClientId {
        *DERIBIT_CLIENT_ID
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            name: STREAM,
            path: "/ws/api/v2",
            upstream: "wss://www.deribit.com/ws/api/v2".to_owned(),
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

        let config = DeribitDataClientConfig::builder()
            .environment(DeribitEnvironment::Mainnet)
            .base_url_ws(format!("ws://{proxy}/ws/api/v2"))
            .product_types(vec![DeribitProductType::Future])
            .update_instruments_interval_mins(0)
            .book_snapshot_timeout_secs(args.timeout_secs())
            .book_stale_threshold_secs(stale)
            .build();
        Ok(Box::new(DeribitDataClient::new(
            *DERIBIT_CLIENT_ID,
            config,
        )?))
    }

    fn key(&self, id: &InstrumentId) -> String {
        format!(
            "book.{}.{DERIBIT_BOOK_DEFAULT_GROUP}.{}.{interval}",
            id.symbol,
            DERIBIT_BOOK_DEFAULT_DEPTH,
            interval = "100ms",
        )
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
struct DeribitWire {
    views: WireViews,
}

impl WireCodec for DeribitWire {
    fn open(&self, _route: &Route, epoch: usize) -> Box<dyn WireConnection> {
        Box::new(DeribitConnection {
            views: self.views.clone(),
            epoch,
            books: HashMap::new(),
            unsubscribes: HashMap::new(),
        })
    }
}

struct DeribitConnection {
    views: WireViews,
    epoch: usize,
    books: HashMap<String, WireBook>,
    unsubscribes: HashMap<u64, String>,
}

impl WireConnection for DeribitConnection {
    fn upstream(&mut self, message: &Message) -> Upstream {
        let Message::Text(text) = message else {
            return Upstream::Other;
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Upstream::Other;
        };

        if let Some(id) = frame["id"].as_u64()
            && let Some(key) = self.unsubscribes.remove(&id)
            && frame["result"]
                .as_array()
                .is_some_and(|result| result.iter().any(|c| c.as_str() == Some(key.as_str())))
        {
            return Upstream::Unsubscribed(key);
        }

        let Some(channel) = frame["params"]["channel"]
            .as_str()
            .filter(|channel| channel.starts_with("book."))
        else {
            return Upstream::Other;
        };

        let data = &frame["params"]["data"];

        let kind = if data["type"].as_str() == Some("change") {
            FrameKind::Update
        } else {
            FrameKind::Snapshot
        };

        let bids = parse_levels(&data["bids"]);
        let asks = parse_levels(&data["asks"]);

        let book = match kind {
            FrameKind::Snapshot => {
                let mut fresh = WireBook::default();
                fresh.apply(&bids, &asks);
                self.books.insert(channel.to_owned(), fresh.clone());
                fresh
            }
            FrameKind::Update => {
                let book = self.books.entry(channel.to_owned()).or_default();
                book.apply(&bids, &asks);
                book.clone()
            }
        };

        let sequence = data["change_id"].as_u64().unwrap_or(0);
        let timestamp = data["timestamp"].as_u64().unwrap_or(0) * 1_000_000;
        self.views.record(
            channel,
            WireView {
                epoch: self.epoch,
                sequence,
                timestamp,
                book: book.top(DEPTH),
            },
        );

        Upstream::Book {
            key: channel.to_owned(),
            kind,
        }
    }

    fn client(&mut self, message: &Message) -> Vec<String> {
        let Message::Text(text) = message else {
            return Vec::new();
        };

        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };

        let method = frame["method"].as_str().unwrap_or("");
        if method != "public/unsubscribe" && method != "private/unsubscribe" {
            return Vec::new();
        }

        let keys = frame["params"]["channels"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .filter(|c| c.starts_with("book."))
            .map(str::to_owned)
            .collect::<Vec<_>>();

        if let (Some(id), Some(key)) = (frame["id"].as_u64(), keys.first()) {
            self.unsubscribes.insert(id, key.clone());
        }

        keys
    }

    fn corrupt(&mut self, message: &mut Message, _key: &str, _kind: FrameKind) -> bool {
        let Message::Text(text) = message else {
            return false;
        };

        let Ok(mut frame) = serde_json::from_str::<Value>(text) else {
            return false;
        };

        let Some(bids) = frame["params"]["data"]["bids"].as_array_mut() else {
            return false;
        };

        let Some(row) = bids.first_mut().and_then(|row| row.as_array_mut()) else {
            return false;
        };

        // Grouped rows are [price, amount]; raw rows are [action, price, amount]
        let price = if row.first().is_some_and(|value| value.is_string()) {
            row.get_mut(1)
        } else {
            row.first_mut()
        };

        let Some(price) = price else {
            return false;
        };

        *price = json!(1e20);
        *message = Message::Text(frame.to_string().into());
        true
    }

    fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
        let Message::Text(text) = message else {
            return None;
        };

        let frame: Value = serde_json::from_str(text).ok()?;
        let method = frame["method"].as_str()?;
        if method != "public/subscribe" && method != "private/subscribe" {
            return None;
        }

        let channel = frame["params"]["channels"][0].as_str()?;
        if !channel.starts_with("book.") {
            return None;
        }

        let reply = json!({
            "jsonrpc": "2.0",
            "id": frame["id"],
            "error": {"code": -32602, "message": "subscribe denied"},
            "testnet": true,
        });
        Some((channel.to_owned(), Message::Text(reply.to_string().into())))
    }
}

fn parse_levels(rows: &Value) -> Vec<(Decimal, Decimal)> {
    let mut levels = Vec::new();

    for row in rows.as_array().cloned().unwrap_or_default() {
        let (action, price, amount) = match row.as_array().map(Vec::as_slice) {
            Some([price, amount]) => ("new", price, amount),
            Some([action, price, amount]) => (action.as_str().unwrap_or("new"), price, amount),
            _ => continue,
        };

        let (Ok(price), Ok(amount)) = (
            Decimal::from_str(&price.to_string()),
            Decimal::from_str(&amount.to_string()),
        ) else {
            continue;
        };

        if action == "delete" {
            levels.push((price, Decimal::ZERO));
        } else if amount > Decimal::ZERO {
            levels.push((price, amount));
        }
    }

    levels
}

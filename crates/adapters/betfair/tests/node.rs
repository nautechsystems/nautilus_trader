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

//! Phase 2 full-`LiveNode` smoke tests for the Betfair execution adapter.
//!
//! Boots a real `LiveNode` via the builder, registers a real `BetfairExecutionClient`
//! (built by a test `ExecutionClientFactory` pointed at the in-process mock venue), runs the
//! node, drives a single order lifecycle through the real run loop, and asserts the node
//! processes it and stops clean.
//!
//! These complement the deterministic seam harness in `live.rs`. The seam harness exercises the
//! routing fork in isolation on a `TestClock` with manual pumping; this exercises the same fork
//! wrapped in the `ExecutionManager` bookkeeping that `LiveNode::run` adds (fill-dedup,
//! post-dispatch close handling), at the cost of a wall-clock run loop. The factory injects the
//! mock URLs because `BetfairExecConfig` has no HTTP base-URL override; everything else (the
//! client, the engines, the run loop, the routing fork) is the production code path.
//!
//! Run with nextest for per-process logging isolation:
//!
//! ```bash
//! cargo nextest run -p nautilus-betfair --test node
//! ```

mod common;

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use nautilus_betfair::{
    common::consts::{BETFAIR, BETFAIR_VENUE, METHOD_CANCEL_ORDERS, METHOD_PLACE_ORDERS},
    config::BetfairExecConfig,
    execution::BetfairExecutionClient,
};
use nautilus_common::{
    actor::DataActor,
    cache::CacheView,
    clients::ExecutionClient,
    enums::Environment,
    factories::{ClientConfig, ExecutionClientFactory},
    testing::wait_until_async,
};
use nautilus_live::{
    ExecutionClientCore,
    builder::LiveNodeBuilder,
    config::{LiveExecEngineConfig, LiveNodeConfig},
    node::{LiveNode, NodeState},
};
use nautilus_model::{
    enums::{AccountType, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderAccepted, OrderCanceled, OrderFilled, OrderRejected},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::{Instrument, InstrumentAny, stubs::betting},
    orders::{Order, OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rstest::rstest;
use rust_decimal::Decimal;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::common::{
    MockState, accept_and_auth, create_test_http_client, load_fixture, plain_stream_config,
    start_mock_http, start_mock_stream, test_credential,
};

const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "BETFAIR-001";
const STRATEGY_ID: &str = "S-001";
const CLIENT_ORDER_ID: &str = "O-1";
const DEADLINE: Duration = Duration::from_secs(5);
const RUN_TIMEOUT: Duration = Duration::from_secs(10);

// Builds the real `BetfairExecutionClient` against the in-process mock by injecting the mock HTTP
// and stream endpoints. Mirrors `BetfairExecutionClientFactory::create`; only the transport URLs
// differ, since `BetfairExecConfig` has no HTTP base-URL override to point at the mock.
#[derive(Debug)]
struct MockBetfairExecFactory {
    trader_id: TraderId,
    account_id: AccountId,
    http_addr: SocketAddr,
    stream_port: u16,
}

impl ExecutionClientFactory for MockBetfairExecFactory {
    fn create(
        &self,
        name: &str,
        _config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            *BETFAIR_VENUE,
            OmsType::Netting,
            self.account_id,
            AccountType::Betting,
            None,
            cache,
        );
        let client = BetfairExecutionClient::new(
            core,
            create_test_http_client(self.http_addr),
            test_credential(),
            plain_stream_config(self.stream_port),
            BetfairExecConfig::default(),
            Currency::GBP(),
        );
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BETFAIR
    }

    fn config_type(&self) -> &'static str {
        stringify!(MockBetfairExecConfig)
    }
}

#[derive(Debug)]
struct MockBetfairExecConfig;

impl ClientConfig for MockBetfairExecConfig {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Bundles the order-lifecycle flags a test observes. The strategy flips these from its order-event
// hooks; the node-driver task (which cannot hold the `!Send` cache) polls them for a deterministic
// stop signal.
#[derive(Debug, Clone, Default)]
struct LifecycleProbe {
    accepted: Arc<AtomicBool>,
    rejected: Arc<AtomicBool>,
    canceled: Arc<AtomicBool>,
    filled: Arc<AtomicBool>,
}

// Submits one passive limit order on start, records each terminal order event via the probe, and
// optionally cancels the order once accepted to exercise the cancel-command path.
#[derive(Debug)]
struct SubmitLimitOnStart {
    core: StrategyCore,
    instrument_id: InstrumentId,
    client_id: ClientId,
    cancel_on_accept: bool,
    probe: LifecycleProbe,
}

impl SubmitLimitOnStart {
    fn new(
        instrument_id: InstrumentId,
        client_id: ClientId,
        cancel_on_accept: bool,
        probe: LifecycleProbe,
    ) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from(STRATEGY_ID)),
                ..Default::default()
            }),
            instrument_id,
            client_id,
            cancel_on_accept,
            probe,
        }
    }
}

impl DataActor for SubmitLimitOnStart {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from(TRADER_ID))
            .strategy_id(StrategyId::from(STRATEGY_ID))
            .instrument_id(self.instrument_id)
            .client_order_id(ClientOrderId::from(CLIENT_ORDER_ID))
            .side(OrderSide::Buy)
            .price(Price::from("3.0"))
            .quantity(Quantity::from("10.0"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let client_id = self.client_id;
        self.submit_order(order, None, Some(client_id), None)?;
        Ok(())
    }
}

nautilus_strategy!(SubmitLimitOnStart, {
    fn on_order_canceled(&mut self, _event: &OrderCanceled) {
        self.probe.canceled.store(true, Ordering::Relaxed);
    }

    fn on_order_filled(&mut self, _event: &OrderFilled) {
        self.probe.filled.store(true, Ordering::Relaxed);
    }

    fn on_order_accepted(&mut self, event: OrderAccepted) {
        self.probe.accepted.store(true, Ordering::Relaxed);

        // Originate a cancelOrders request from our side; the venue confirms via OCM (no direct ack).
        if self.cancel_on_accept {
            let client_id = self.client_id;
            self.cancel_order(event.client_order_id, Some(client_id), None)
                .expect("cancel_order failed");
        }
    }

    fn on_order_rejected(&mut self, _event: OrderRejected) {
        self.probe.rejected.store(true, Ordering::Relaxed);
    }
});

// Accepts and authenticates the mock order stream, then writes OCM frames on demand. Mirrors the
// seam harness `StreamFeeder`; kept local so the node target stays independent of the harness.
struct StreamFeeder {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl StreamFeeder {
    fn spawn(listener: TcpListener) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let (mut reader, mut write_half) = accept_and_auth(&listener).await;
            // Drain the order-subscription line the client sends after auth.
            let mut line = String::new();
            reader.read_line(&mut line).await.ok();

            while let Some(frame) = rx.recv().await {
                write_half
                    .write_all(format!("{}\r\n", frame.trim()).as_bytes())
                    .await
                    .unwrap();
            }
        });
        Self { tx }
    }

    fn feed(&self, fixture_rel_path: &str) {
        self.tx.send(load_fixture(fixture_rel_path)).unwrap();
    }
}

fn build_node(name: &str, http_addr: SocketAddr, stream_port: u16) -> LiveNode {
    let trader_id = TraderId::from(TRADER_ID);
    let config = LiveNodeConfig {
        environment: Environment::Live,
        trader_id,
        exec_engine: LiveExecEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::from_millis(50),
        ..Default::default()
    };

    let factory = MockBetfairExecFactory {
        trader_id,
        account_id: AccountId::from(ACCOUNT_ID),
        http_addr,
        stream_port,
    };

    let node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name(name)
        .add_exec_client(None, Box::new(factory), Box::new(MockBetfairExecConfig))
        .unwrap()
        .build()
        .unwrap();

    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(InstrumentAny::Betting(betting()))
        .unwrap();

    node
}

// Forces the mock to reject placeOrders with the venue error fixture, so a submitted order routes to
// `Rejected` from the HTTP response without an OCM frame.
fn reject_place_orders(mock_state: &MockState) {
    let fixture = load_fixture("rest/betting_place_order_error.json");
    let value: serde_json::Value = serde_json::from_str(&fixture).unwrap();
    mock_state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_PLACE_ORDERS.to_string(), value["result"].clone());
}

// Whether the mock venue has recorded a cancelOrders request yet.
fn cancel_orders_seen(mock_state: &MockState) -> bool {
    mock_state
        .betting_methods
        .lock()
        .unwrap()
        .iter()
        .any(|method| method == METHOD_CANCEL_ORDERS)
}

#[rstest]
#[tokio::test]
async fn node_boots_connects_and_stops_clean() {
    let (addr, mock_state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let _feeder = StreamFeeder::spawn(listener);

    let mut node = build_node("BetfairNodeSmoke", addr, stream_port);
    let handle = node.handle();

    let stop_handle = handle.clone();
    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert_eq!(handle.state(), NodeState::Stopped);
    assert!(
        mock_state.login_count.load(Ordering::Relaxed) > 0,
        "client did not log in to the venue (connect did not run)"
    );
}

#[rstest]
#[tokio::test]
async fn submit_routes_through_node_to_accepted() {
    let (addr, _mock_state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let _feeder = StreamFeeder::spawn(listener);

    let probe = LifecycleProbe::default();
    let instrument_id = betting().id();
    let mut node = build_node("BetfairNodeSubmit", addr, stream_port);
    node.add_strategy(SubmitLimitOnStart::new(
        instrument_id,
        ClientId::from(BETFAIR),
        false,
        probe.clone(),
    ))
    .unwrap();

    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert!(
        probe.accepted.load(Ordering::Relaxed),
        "strategy never received OrderAccepted"
    );
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("submitted order not in cache");
    assert_eq!(order.status(), OrderStatus::Accepted);
}

#[rstest]
#[tokio::test]
async fn submit_venue_error_routes_to_rejected() {
    let (addr, mock_state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let _feeder = StreamFeeder::spawn(listener);
    reject_place_orders(&mock_state);

    let probe = LifecycleProbe::default();
    let instrument_id = betting().id();
    let mut node = build_node("BetfairNodeReject", addr, stream_port);
    node.add_strategy(SubmitLimitOnStart::new(
        instrument_id,
        ClientId::from(BETFAIR),
        false,
        probe.clone(),
    ))
    .unwrap();

    let handle = node.handle();
    let stop_handle = handle.clone();
    let rejected = probe.rejected.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { rejected.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert!(
        probe.rejected.load(Ordering::Relaxed),
        "strategy never received OrderRejected"
    );
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("rejected order not in cache");
    assert_eq!(order.status(), OrderStatus::Rejected);
}

#[rstest]
#[tokio::test]
async fn cancel_routes_through_node() {
    let (addr, mock_state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let feeder = StreamFeeder::spawn(listener);

    let probe = LifecycleProbe::default();
    let instrument_id = betting().id();
    let mut node = build_node("BetfairNodeCancel", addr, stream_port);
    node.add_strategy(SubmitLimitOnStart::new(
        instrument_id,
        ClientId::from(BETFAIR),
        true,
        probe.clone(),
    ))
    .unwrap();

    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();
    let canceled = probe.canceled.clone();
    let cancel_seen = mock_state.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        // Wait for the strategy's cancelOrders request to reach the venue before confirming via the
        // OCM frame, so a pass distinguishes the command path from a venue-initiated cancel.
        wait_until_async(|| async { cancel_orders_seen(&cancel_seen) }, DEADLINE).await;
        feeder.feed("stream/ocm_harness_cancel.json");
        wait_until_async(|| async { canceled.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert!(
        cancel_orders_seen(&mock_state),
        "cancelOrders request was never sent to the venue"
    );
    assert!(
        probe.canceled.load(Ordering::Relaxed),
        "strategy never received OrderCanceled"
    );
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("canceled order not in cache");
    assert_eq!(order.status(), OrderStatus::Canceled);
}

#[rstest]
#[tokio::test]
async fn fill_routes_through_node_execution_manager() {
    let (addr, _mock_state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let feeder = StreamFeeder::spawn(listener);

    let probe = LifecycleProbe::default();
    let instrument_id = betting().id();
    let mut node = build_node("BetfairNodeFill", addr, stream_port);
    node.add_strategy(SubmitLimitOnStart::new(
        instrument_id,
        ClientId::from(BETFAIR),
        false,
        probe.clone(),
    ))
    .unwrap();

    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();
    let filled = probe.filled.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        // The matched OCM fill frame correlates to the placed order by client order id.
        feeder.feed("stream/ocm_harness_fill.json");
        wait_until_async(|| async { filled.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert!(
        probe.filled.load(Ordering::Relaxed),
        "strategy never received OrderFilled"
    );
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("filled order not in cache");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty().as_decimal(), Decimal::from(10));
}

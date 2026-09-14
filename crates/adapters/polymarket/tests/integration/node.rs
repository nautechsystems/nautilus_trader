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

//! Full-`LiveNode` smoke tests for the Polymarket execution adapter.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use nautilus_common::{actor::DataActor, enums::Environment, testing::wait_until_async};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    node::{LiveNode, NodeState},
};
use nautilus_model::{
    enums::OrderStatus,
    events::{OrderAccepted, OrderCanceled, OrderFillVoided, OrderFilled, OrderRejected},
    identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    orders::Order,
    types::Quantity,
};
use nautilus_polymarket::{
    common::consts::POLYMARKET, factories::PolymarketExecutionClientFactory,
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rstest::rstest;
use rust_decimal::Decimal;

use crate::{
    harness,
    mock_venue::{TestServerState, execution_config, load_json, start_mock_server},
};

const CLIENT_ORDER_ID: &str = "O-1";
const DEADLINE: Duration = Duration::from_secs(5);
const RUN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default)]
struct LifecycleProbe {
    accepted: Arc<AtomicBool>,
    rejected: Arc<AtomicBool>,
    canceled: Arc<AtomicBool>,
    filled: Arc<AtomicBool>,
    fill_voided: Arc<AtomicBool>,
}

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
                strategy_id: Some(StrategyId::from(harness::STRATEGY_ID)),
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
        self.submit_order(
            harness::limit_order(self.instrument_id, CLIENT_ORDER_ID),
            None,
            Some(self.client_id),
            None,
        )?;
        Ok(())
    }
}

nautilus_strategy!(SubmitLimitOnStart, {
    fn on_order_accepted(&mut self, event: OrderAccepted) {
        self.probe.accepted.store(true, Ordering::Relaxed);

        if self.cancel_on_accept {
            self.cancel_order(event.client_order_id, Some(self.client_id), None)
                .expect("cancel_order failed");
        }
    }

    fn on_order_rejected(&mut self, _event: OrderRejected) {
        self.probe.rejected.store(true, Ordering::Relaxed);
    }

    fn on_order_canceled(&mut self, _event: &OrderCanceled) {
        self.probe.canceled.store(true, Ordering::Relaxed);
    }

    fn on_order_filled(&mut self, _event: &OrderFilled) {
        self.probe.filled.store(true, Ordering::Relaxed);
    }

    fn on_order_fill_voided(&mut self, _event: &OrderFillVoided) {
        self.probe.fill_voided.store(true, Ordering::Relaxed);
    }
});

async fn start_accepting_mock() -> (SocketAddr, TestServerState) {
    let state = TestServerState::default();
    state.configure_default_order_success().await;
    let addr = start_mock_server(state.clone()).await;
    (addr, state)
}

fn build_node(name: &str, addr: SocketAddr) -> LiveNode {
    let config = LiveNodeConfig {
        environment: Environment::Live,
        trader_id: TraderId::from(harness::TRADER_ID),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::from_millis(50),
        ..Default::default()
    };
    let node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name(name)
        .add_exec_client(
            None,
            Box::new(PolymarketExecutionClientFactory),
            Box::new(execution_config(addr)),
        )
        .unwrap()
        .build()
        .unwrap();
    let instrument = harness::instrument();
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    node.kernel()
        .exec_engine
        .borrow_mut()
        .get_client_adapter_mut(&ClientId::from(POLYMARKET))
        .expect("Polymarket execution client")
        .on_instrument(instrument);
    node
}

fn add_submit_strategy(node: &mut LiveNode, cancel_on_accept: bool, probe: LifecycleProbe) {
    node.add_strategy(SubmitLimitOnStart::new(
        InstrumentId::from(harness::INSTRUMENT_ID),
        ClientId::from(POLYMARKET),
        cancel_on_accept,
        probe,
    ))
    .unwrap();
}

#[rstest]
#[tokio::test]
async fn node_boots_connects_and_stops_clean() {
    let (addr, state) = start_accepting_mock().await;
    let mut node = build_node("PolymarketNodeSmoke", addr);
    let handle = node.handle();
    let stop_handle = handle.clone();
    let socket_count = state.user_socket_count.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(
            || {
                let socket_count = socket_count.clone();
                async move { socket_count.load(Ordering::Acquire) == 1 }
            },
            DEADLINE,
        )
        .await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert_eq!(handle.state(), NodeState::Stopped);
    assert_eq!(
        state.startup_request_paths.lock().await.as_slice(),
        ["/version", "/ws", "/balance-allowance"],
    );
}

#[rstest]
#[tokio::test]
async fn submit_routes_through_node_to_accepted() {
    let (addr, _state) = start_accepting_mock().await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("PolymarketNodeSubmit", addr);
    add_submit_strategy(&mut node, false, probe.clone());
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
        "strategy never received OrderAccepted",
    );
    assert_eq!(handle.state(), NodeState::Stopped);
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(
        order.venue_order_id(),
        Some(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID.into()),
    );
}

#[rstest]
#[tokio::test]
async fn submit_venue_error_routes_through_node_to_rejected() {
    let (addr, state) = start_accepting_mock().await;
    *state.order_response.lock().await = Some(load_json("http_order_response_failed.json"));
    let probe = LifecycleProbe::default();
    let mut node = build_node("PolymarketNodeReject", addr);
    add_submit_strategy(&mut node, false, probe.clone());
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
        "strategy never received OrderRejected",
    );
    assert_eq!(handle.state(), NodeState::Stopped);
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Rejected);
    assert_eq!(order.venue_order_id(), None);
}

#[rstest]
#[tokio::test]
async fn cancel_routes_through_node() {
    let (addr, state) = start_accepting_mock().await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("PolymarketNodeCancel", addr);
    add_submit_strategy(&mut node, true, probe.clone());
    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();
    let canceled = probe.canceled.clone();
    let driver_state = state.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        wait_until_async(
            || {
                let state = driver_state.clone();
                async move { *state.cancel_delete_count.lock().await == 1 }
            },
            DEADLINE,
        )
        .await;
        driver_state
            .feed_user("ws_user_order_cancellation.json")
            .await;
        wait_until_async(|| async { canceled.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert_eq!(*state.cancel_delete_count.lock().await, 1);
    assert!(
        probe.canceled.load(Ordering::Relaxed),
        "strategy never received OrderCanceled",
    );
    assert_eq!(handle.state(), NodeState::Stopped);
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(
        order.venue_order_id(),
        Some(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID.into()),
    );
}

#[rstest]
#[tokio::test]
async fn fill_routes_through_node_execution_manager() {
    let (addr, state) = start_accepting_mock().await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("PolymarketNodeFill", addr);
    add_submit_strategy(&mut node, false, probe.clone());
    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();
    let filled = probe.filled.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        state.feed_user("ws_user_trade_full.json").await;
        wait_until_async(|| async { filled.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert!(
        probe.filled.load(Ordering::Relaxed),
        "strategy never received OrderFilled",
    );
    assert_eq!(handle.state(), NodeState::Stopped);
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty().as_decimal(), Decimal::from(100));
    assert_eq!(
        order.venue_order_id(),
        Some(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID.into()),
    );
    let positions = cache.positions_open(
        None,
        Some(&InstrumentId::from(harness::INSTRUMENT_ID)),
        None,
        None,
        None,
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("100.0000"));
}

#[rstest]
#[tokio::test]
async fn failed_fill_routes_void_through_node_execution_manager() {
    let (addr, state) = start_accepting_mock().await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("PolymarketNodeFillVoid", addr);
    add_submit_strategy(&mut node, false, probe.clone());
    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();
    let filled = probe.filled.clone();
    let fill_voided = probe.fill_voided.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        state.feed_user("ws_user_trade_full.json").await;
        wait_until_async(|| async { filled.load(Ordering::Relaxed) }, DEADLINE).await;
        state.feed_user("ws_user_trade_full_failed.json").await;
        wait_until_async(|| async { fill_voided.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert!(
        probe.filled.load(Ordering::Relaxed),
        "strategy never received OrderFilled",
    );
    assert!(
        probe.fill_voided.load(Ordering::Relaxed),
        "strategy never received OrderFillVoided",
    );
    assert_eq!(handle.state(), NodeState::Stopped);
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Voided);
    assert_eq!(order.filled_qty().as_decimal(), Decimal::ZERO);
    assert_eq!(order.voided_qty().as_decimal(), Decimal::from(100));
    assert!(
        cache
            .positions_open(
                None,
                Some(&InstrumentId::from(harness::INSTRUMENT_ID)),
                None,
                None,
                None,
            )
            .is_empty(),
        "voided fill left an open position",
    );
}

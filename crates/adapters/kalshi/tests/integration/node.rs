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

//! Full `LiveNode` tests for the Kalshi execution adapter.
//!
//! Each test drives a strategy through the real trading stack against the mock venue: the strategy
//! submits, the execution engine routes the command through the risk engine to the Kalshi client, the
//! client signs and sends the request, and the events it reports back reach the strategy and the
//! cache. Nothing is stubbed between the strategy and the venue.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use nautilus_common::{actor::DataActor, enums::Environment, testing::wait_until_async};
use nautilus_kalshi::{
    common::consts::{KALSHI_ACCOUNT_ID, KALSHI_EXEC_CLIENT_ID},
    factories::KalshiExecutionClientFactory,
};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    node::{LiveNode, NodeState},
};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, TimeInForce},
    events::{OrderAccepted, OrderCanceled, OrderFilled, OrderRejected},
    identifiers::{AccountId, ClientId, ClientOrderId, StrategyId, TradeId, TraderId},
    instruments::Instrument,
    orders::Order,
    types::{Currency, Money, Quantity},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rstest::rstest;

use crate::{
    harness::{
        API_KEY_ID, CLIENT_ORDER_ID, FILL_JSON, instrument, limit_order, test_private_key_pem,
    },
    mock_venue::{MockVenue, spawn_mock},
};

const DEADLINE: Duration = Duration::from_secs(5);
const RUN_TIMEOUT: Duration = Duration::from_secs(10);

/// What the strategy observed of the order's life.
#[derive(Debug, Clone, Default)]
struct LifecycleProbe {
    accepted: Arc<AtomicBool>,
    rejected: Arc<AtomicBool>,
    canceled: Arc<AtomicBool>,
    filled: Arc<AtomicBool>,
    fill_trade_id: Arc<parking_lot::Mutex<Option<TradeId>>>,
    fill_quantity: Arc<parking_lot::Mutex<Option<Quantity>>>,
}

/// A strategy that submits one limit order when it starts, and optionally cancels it on acceptance.
#[derive(Debug)]
struct SubmitLimitOnStart {
    core: StrategyCore,
    client_id: ClientId,
    cancel_on_accept: bool,
    probe: LifecycleProbe,
}

impl SubmitLimitOnStart {
    fn new(client_id: ClientId, cancel_on_accept: bool, probe: LifecycleProbe) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("S-001")),
                ..Default::default()
            }),
            client_id,
            cancel_on_accept,
            probe,
        }
    }
}

impl DataActor for SubmitLimitOnStart {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.submit_order(
            limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID),
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

    fn on_order_filled(&mut self, event: &OrderFilled) {
        *self.probe.fill_trade_id.lock() = Some(event.trade_id);
        *self.probe.fill_quantity.lock() = Some(event.last_qty);
        self.probe.filled.store(true, Ordering::Relaxed);
    }
});

/// Builds a node whose execution client points at the mock venue.
///
/// `reconciliation` makes the client adopt what the venue already holds when it connects; the tests
/// leave it off unless they are exercising that pass, so an assertion about a submitted order cannot
/// be satisfied by an adopted one.
fn build_node(
    name: &str,
    addr: SocketAddr,
    poll_interval_millis: u64,
    reconciliation: bool,
) -> LiveNode {
    let config = LiveNodeConfig {
        environment: Environment::Live,
        trader_id: TraderId::from("TESTER-001"),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::from_millis(50),
        ..Default::default()
    };
    let exec_config = nautilus_kalshi::KalshiExecClientConfig::builder()
        .base_url(format!("http://{addr}/trade-api/v2"))
        .api_key_id(API_KEY_ID.to_string())
        .api_key_pem(test_private_key_pem().into())
        .poll_interval_millis(poll_interval_millis)
        .reconciliation(reconciliation)
        .build();
    let node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name(name)
        .add_exec_client(
            None,
            Box::new(KalshiExecutionClientFactory),
            Box::new(exec_config),
        )
        .unwrap()
        .build()
        .unwrap();
    let instrument = instrument();
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    node.kernel()
        .exec_engine
        .borrow_mut()
        .get_client_adapter_mut(&ClientId::from(KALSHI_EXEC_CLIENT_ID))
        .expect("Kalshi execution client")
        .on_instrument(instrument);
    node
}

fn add_submit_strategy(node: &mut LiveNode, cancel_on_accept: bool, probe: LifecycleProbe) {
    node.add_strategy(SubmitLimitOnStart::new(
        ClientId::from(KALSHI_EXEC_CLIENT_ID),
        cancel_on_accept,
        probe,
    ))
    .unwrap();
}

#[rstest]
#[tokio::test]
async fn node_boots_connects_and_stops_clean() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let mut node = build_node("KalshiNodeSmoke", addr, 2_000, true);
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
    // Connecting reads the venue balance and reports it as an account, so a strategy starts against
    // the cash the venue holds rather than against nothing.
    let cache = node.kernel().cache.borrow();
    let account = cache
        .account(&AccountId::from(KALSHI_ACCOUNT_ID))
        .expect("the account reported at connect");
    let balance = account
        .balance(Some(Currency::from("USD")))
        .expect("a USD balance");

    assert_eq!(balance.total, Money::from("9125.00 USD"));
    assert_eq!(balance.free, Money::from("4125.00 USD"));
    assert!(venue.signed("GET /trade-api/v2/portfolio/balance"));
}

#[rstest]
#[tokio::test]
async fn submit_routes_through_node_to_accepted() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("KalshiNodeSubmit", addr, 2_000, false);
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
        "strategy never received OrderAccepted"
    );
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("the submitted order");

    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(
        order.venue_order_id().map(|id| id.to_string()),
        Some("order-1".to_string())
    );
    let creates = venue.creates();

    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0]["client_order_id"], CLIENT_ORDER_ID);
}

#[rstest]
#[tokio::test]
async fn submit_venue_error_routes_through_node_to_rejected() {
    let venue = MockVenue::resting();
    // A refusal is the venue's own answer, not a transport failure.
    venue
        .create_responses
        .lock()
        .push_back(axum::http::StatusCode::BAD_REQUEST);
    let addr = spawn_mock(venue).await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("KalshiNodeReject", addr, 2_000, false);
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
        "strategy never received OrderRejected"
    );
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("the submitted order");

    assert_eq!(order.status(), OrderStatus::Rejected);
    assert_eq!(order.venue_order_id(), None);
}

#[rstest]
#[tokio::test]
async fn fill_routes_through_node_to_the_cache() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("KalshiNodeFill", addr, 20, false);
    add_submit_strategy(&mut node, false, probe.clone());
    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();
    let filled = probe.filled.clone();
    let driver_venue = venue.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        // The order trades at the venue: the client learns of it from its next poll.
        driver_venue.add_fill(FILL_JSON);
        driver_venue.set_order(crate::harness::order_json("executed", "100.00", "0.00"));
        wait_until_async(|| async { filled.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert_eq!(
        probe.fill_trade_id.lock().as_ref().map(|id| id.to_string()),
        Some("fill-1".to_string()),
        "the venue fill identifier reaches the strategy"
    );
    assert_eq!(*probe.fill_quantity.lock(), Some(Quantity::from("100.00")));
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("the submitted order");

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("100.00"));
    // A cash account opens a position from the fill, so the venue's execution reaches the portfolio.
    let instrument_id = instrument().id();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);

    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("100.00"));
}

#[rstest]
#[tokio::test]
async fn cancel_routes_through_node_to_canceled() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let probe = LifecycleProbe::default();
    let mut node = build_node("KalshiNodeCancel", addr, 20, false);
    add_submit_strategy(&mut node, true, probe.clone());
    let handle = node.handle();
    let stop_handle = handle.clone();
    let accepted = probe.accepted.clone();
    let canceled = probe.canceled.clone();
    let driver_venue = venue.clone();

    tokio::spawn(async move {
        wait_until_async(|| async { stop_handle.is_running() }, DEADLINE).await;
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        wait_until_async(
            || {
                let venue = driver_venue.clone();
                async move { *venue.cancel_deletes.lock() == 1 }
            },
            DEADLINE,
        )
        .await;
        // The venue cancels the order and the client learns of it from its next poll.
        driver_venue.set_order(crate::harness::order_json("canceled", "0.00", "100.00"));
        wait_until_async(|| async { canceled.load(Ordering::Relaxed) }, DEADLINE).await;
        stop_handle.stop();
    });

    let result = tokio::time::timeout(RUN_TIMEOUT, node.run()).await;

    assert!(result.is_ok(), "node.run() did not complete within timeout");
    assert!(result.unwrap().is_ok(), "node.run() returned an error");
    assert!(
        probe.canceled.load(Ordering::Relaxed),
        "strategy never received OrderCanceled"
    );
    assert_eq!(*venue.cancel_deletes.lock(), 1);
    assert_eq!(handle.state(), NodeState::Stopped);

    let cache = node.kernel().cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
        .expect("the submitted order");

    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(
        order.venue_order_id().map(|id| id.to_string()),
        Some("order-1".to_string())
    );
}

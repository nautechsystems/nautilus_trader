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

use nautilus_common::{
    actor::DataActor,
    enums::Environment,
    msgbus::{self, MessagingSwitchboard, stubs::get_any_saving_handler},
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    execution::submission::{SubmissionRecoveryExhausted, SubmissionRecoverySource},
    node::{LiveNode, NodeState},
};
use nautilus_model::{
    enums::{AssetClass, OmsType, OrderStatus},
    events::{OrderAccepted, OrderCanceled, OrderFillVoided, OrderFilled, OrderRejected},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TraderId,
    },
    instruments::{BinaryOption, Instrument, InstrumentAny},
    orders::Order,
    position::Position,
    position_operation::{PositionOperationDirection, PositionOperationLeg, PositionOperationType},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_polymarket::{
    common::{consts::POLYMARKET, enums::PolymarketSignatureType},
    config::{
        PolymarketDataClientConfig, PolymarketExecutionClientConfig,
        PolymarketInstrumentProviderConfig,
    },
    factories::{PolymarketDataClientFactory, PolymarketExecutionClientFactory},
    positions::record::{ManagedOperationIntent, PositionOperationRecord},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::json;

use crate::{
    harness,
    mock_venue::{TestServerState, execution_config, load_json, start_mock_server},
    positions::{
        TEST_PRIVATE_KEY as POSITION_PRIVATE_KEY, TEST_SIGNER as POSITION_SIGNER,
        TestServerState as PositionServerState, start_mock_server as start_position_mock_server,
    },
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
    client_id: Option<ClientId>,
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
            client_id: Some(client_id),
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
            self.client_id,
            None,
        )?;
        Ok(())
    }
}

nautilus_strategy!(SubmitLimitOnStart, {
    fn on_order_accepted(&mut self, event: OrderAccepted) {
        self.probe.accepted.store(true, Ordering::Relaxed);

        if self.cancel_on_accept {
            self.cancel_order(event.client_order_id, self.client_id, None)
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
    build_node_with_execution_config(
        name,
        addr,
        LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
    )
}

fn build_node_with_execution_config(
    name: &str,
    addr: SocketAddr,
    exec_engine: LiveExecutionEngineConfig,
) -> LiveNode {
    let config = LiveNodeConfig {
        environment: Environment::Live,
        trader_id: TraderId::from(harness::TRADER_ID),
        exec_engine,
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

#[rstest]
#[tokio::test]
#[expect(
    clippy::await_holding_refcell_ref,
    reason = "the node is stopped before the standalone report request"
)]
async fn startup_mass_status_loads_position_in_past_end_open_market() {
    let state = TestServerState::default();
    let market = load_json("gamma_market_past_end_date_open.json");
    let condition_id = market["conditionId"].as_str().unwrap();
    let tokens: Vec<String> =
        serde_json::from_str(market["clobTokenIds"].as_str().unwrap()).unwrap();
    let instrument_id = InstrumentId::from(format!("{condition_id}-{}.POLYMARKET", tokens[0]));
    *state.positions_response_override.lock().await = Some(json!([{
        "token_id": tokens[0],
        "condition_id": condition_id,
        "current_size": "7.1234",
        "avg_price": "0.321",
    }]));
    *state.gamma_response.lock().await = Some(json!([market]));
    *state.orders_response_override.lock().await = Some(json!({"data": [], "next_cursor": "LTE="}));
    *state.trades_response_override.lock().await = Some(json!({"data": [], "next_cursor": "LTE="}));
    let addr = start_mock_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let config = LiveNodeConfig {
        environment: Environment::Live,
        trader_id: TraderId::from(harness::TRADER_ID),
        timeout_connection: DEADLINE,
        timeout_reconciliation: DEADLINE,
        delay_post_stop: Duration::from_millis(50),
        ..Default::default()
    };

    let data_config = PolymarketDataClientConfig {
        base_url_http: Some(base_url.clone()),
        base_url_gamma: Some(base_url.clone()),
        base_url_data_api: Some(base_url),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        instrument_config: Some(PolymarketInstrumentProviderConfig {
            load_ids: Some(vec![instrument_id]),
            ..Default::default()
        }),
        resolve_poll_enabled: false,
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name("PolymarketPastEndStartup")
        .add_data_client(
            None,
            Box::new(PolymarketDataClientFactory),
            Box::new(data_config),
        )
        .unwrap()
        .add_exec_client(
            None,
            Box::new(PolymarketExecutionClientFactory),
            Box::new(execution_config(addr)),
        )
        .unwrap()
        .build()
        .unwrap();

    let handle = node.handle();

    let driver = async move {
        wait_until_async(
            || async { matches!(handle.state(), NodeState::Running | NodeState::Stopped) },
            DEADLINE,
        )
        .await;
        let running = handle.state();
        handle.stop();
        running
    };

    let (result, running) =
        tokio::time::timeout(RUN_TIMEOUT, async { tokio::join!(node.run(), driver) })
            .await
            .unwrap();
    result.expect("startup mass status");
    let cached = node
        .kernel()
        .cache
        .borrow()
        .instrument(&instrument_id)
        .cloned()
        .unwrap();
    let mass_status = node
        .kernel()
        .exec_engine
        .borrow_mut()
        .generate_mass_status(&ClientId::from(POLYMARKET), None)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(running, NodeState::Running);
    assert!(
        cached.expiration_ns().unwrap()
            < nautilus_core::time::get_atomic_clock_realtime().get_time_ns()
    );
    assert_eq!(mass_status.position_reports().len(), 1);
    let positions = &mass_status.position_reports()[&instrument_id];
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].instrument_id, instrument_id);
    assert_eq!(positions[0].quantity, Quantity::from("7.1234"));
    assert_eq!(
        positions[0].avg_px_open,
        Some(Decimal::from_str_exact("0.321").unwrap())
    );
    assert_eq!(state.orders_get_count.load(Ordering::Acquire), 2);
}

#[rstest]
#[tokio::test]
async fn stalled_submit_survives_inflight_exhaustion_and_accepts_late_response(
    #[values(false, true)] explicit_client: bool,
) {
    let (addr, state) = start_accepting_mock().await;
    state.order_response_gate.enable();
    let probe = LifecycleProbe::default();

    let mut node = build_node_with_execution_config(
        "PolymarketNodeStalledSubmit",
        addr,
        LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_interval_ms: 10,
            inflight_check_threshold_ms: 20,
            inflight_check_retries: 1,
            ..Default::default()
        },
    );

    let mut strategy = SubmitLimitOnStart::new(
        InstrumentId::from(harness::INSTRUMENT_ID),
        ClientId::from(POLYMARKET),
        false,
        probe.clone(),
    );

    if !explicit_client {
        strategy.client_id = None;
    }

    node.add_strategy(strategy).unwrap();
    let (handler, diagnostics) = get_any_saving_handler::<SubmissionRecoveryExhausted>(None);
    let topic = MessagingSwitchboard::submission_recovery_exhausted_topic();
    msgbus::subscribe_any(topic.into(), handler.clone(), None);
    let handle = node.handle();
    let gate = state.order_response_gate.clone();
    let accepted = probe.accepted.clone();
    let rejected = probe.rejected.clone();

    let exhaustion = diagnostics.clone();

    let driver = async move {
        wait_until_async(|| async { gate.started() == 1 }, DEADLINE).await;
        wait_until_async(|| async { !exhaustion.get_messages().is_empty() }, DEADLINE).await;
        assert!(!rejected.load(Ordering::Relaxed));
        gate.release();
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        handle.stop();
    };

    let (result, ()) =
        tokio::time::timeout(RUN_TIMEOUT, async { tokio::join!(node.run(), driver) })
            .await
            .unwrap();
    result.unwrap();

    msgbus::unsubscribe_any(topic.into(), &handler);
    let diagnostics = diagnostics.get_messages();
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.trader_id, TraderId::from(harness::TRADER_ID));
    assert_eq!(diagnostic.client_id, Some(ClientId::from(POLYMARKET)));
    assert_eq!(
        diagnostic.strategy_id,
        StrategyId::from(harness::STRATEGY_ID)
    );
    assert_eq!(
        diagnostic.instrument_id,
        InstrumentId::from(harness::INSTRUMENT_ID)
    );
    assert_eq!(
        diagnostic.client_order_id,
        ClientOrderId::from(CLIENT_ORDER_ID)
    );
    assert_eq!(diagnostic.source, SubmissionRecoverySource::Inflight);
    assert_eq!(diagnostic.retry_count, 1);

    let post_count = *state.order_post_count.lock().await;
    let open_order_ids = state.open_order_ids.lock().await.clone();
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(
        order.venue_order_id(),
        Some(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID.into())
    );
    assert!(!probe.rejected.load(Ordering::Relaxed));
    assert_eq!(post_count, 1);
    assert_eq!(open_order_ids.len(), 1);
    assert!(open_order_ids.contains(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID));
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
async fn inflight_query_resolves_signed_order_while_submit_response_is_stalled() {
    let (addr, state) = start_accepting_mock().await;
    state.order_response_gate.enable();
    state
        .order_response_uses_request_hash
        .store(true, Ordering::Release);
    let probe = LifecycleProbe::default();

    let mut node = build_node_with_execution_config(
        "PolymarketNodeQuerySubmit",
        addr,
        LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_interval_ms: 10,
            inflight_check_threshold_ms: 50,
            inflight_check_retries: 5,
            ..Default::default()
        },
    );

    add_submit_strategy(&mut node, false, probe.clone());
    let handle = node.handle();
    let accepted = probe.accepted.clone();
    let driver_state = state.clone();

    let driver = tokio::spawn(async move {
        wait_until_async(
            || async { driver_state.order_response_gate.started() == 1 },
            DEADLINE,
        )
        .await;
        let venue_order_id = driver_state
            .open_order_ids
            .lock()
            .await
            .iter()
            .next()
            .cloned()
            .unwrap();
        let mut venue_order = load_json("http_open_order.json");
        venue_order["id"] = serde_json::Value::from(venue_order_id.clone());
        venue_order["asset_id"] = serde_json::Value::from(crate::mock_venue::TEST_TOKEN_ID);
        venue_order["market"] = serde_json::Value::from(crate::mock_venue::TEST_CONDITION_ID);
        venue_order["size_matched"] = serde_json::Value::from("0.0000");
        *driver_state.single_order_response.lock().await = Some(venue_order);
        wait_until_async(|| async { accepted.load(Ordering::Relaxed) }, DEADLINE).await;
        driver_state.order_response_gate.release();
        handle.stop();
        venue_order_id
    });

    tokio::time::timeout(RUN_TIMEOUT, node.run())
        .await
        .unwrap()
        .unwrap();
    let venue_order_id = driver.await.unwrap();
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.venue_order_id(), Some(venue_order_id.as_str().into()));
    assert!(!probe.rejected.load(Ordering::Relaxed));
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
        let mut rest_failed = load_json("http_trade_report.json");
        rest_failed["id"] = serde_json::json!("trade-0xfull");
        rest_failed["status"] = serde_json::json!("FAILED");
        rest_failed["size"] = serde_json::json!("100.0000");
        rest_failed["taker_order_id"] =
            serde_json::json!(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID);
        *state.trades_response_override.lock().await = Some(serde_json::json!({
            "data": [rest_failed],
            "next_cursor": "LTE="
        }));
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

const POSITION_MARKET_ID: &str =
    "0x2222222222222222222222222222222222222222222222222222222222222200";
const POSITION_STRATEGY_ID: &str = "S-POSITION-RESTART";

fn position_market() -> serde_json::Value {
    json!({
        "market_id": POSITION_MARKET_ID,
        "fee_bips": 100,
        "conditions": [
            {
                "question_index": 0,
                "condition_id": format!("0x{}{:02x}", "11".repeat(31), 0),
                "yes_token_id": "512",
                "no_token_id": "256",
            },
            {
                "question_index": 1,
                "condition_id": format!("0x{}{:02x}", "11".repeat(31), 1),
                "yes_token_id": "513",
                "no_token_id": "257",
            },
        ],
    })
}

fn position_instrument(condition: &str, token_id: u64, outcome: &str) -> InstrumentAny {
    let market = position_market();
    let info = serde_json::from_value(json!({
        "market_version": "v1",
        "condition_id": condition,
        "token_id": token_id.to_string(),
        "neg_risk": true,
        "neg_risk_market": market,
        "gamma_event": {"negRiskMarketID": POSITION_MARKET_ID},
    }))
    .unwrap();
    let instrument = BinaryOption::builder()
        .instrument_id(InstrumentId::from(
            format!("TOKEN-{token_id}.POLYMARKET").as_str(),
        ))
        .raw_symbol(Symbol::new(format!("TOKEN-{token_id}")))
        .asset_class(AssetClass::Alternative)
        .currency(Currency::pUSD())
        .activation_ns(UnixNanos::default())
        .expiration_ns(UnixNanos::from(4_000_000_000_000_000_000u64))
        .price_precision(4)
        .size_precision(4)
        .price_increment(Price::from("0.0001"))
        .size_increment(Quantity::from("0.0001"))
        .outcome(ustr::Ustr::from(outcome))
        .info(info)
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    InstrumentAny::BinaryOption(instrument)
}

fn seed_operation_position(
    node: &LiveNode,
    instrument: &InstrumentAny,
    quantity: Quantity,
    cost: Decimal,
) {
    let strategy_id = StrategyId::from(POSITION_STRATEGY_ID);
    let account_id = AccountId::from(harness::ACCOUNT_ID);
    let position_id = PositionId::from(format!("{}-{strategy_id}", instrument.id()));
    let leg = PositionOperationLeg {
        operation_id: UUID4::new(),
        operation: PositionOperationType::Convert,
        direction: PositionOperationDirection::Open,
        trader_id: TraderId::from(harness::TRADER_ID),
        strategy_id,
        account_id,
        instrument_id: instrument.id(),
        position_id,
        quantity,
        cost,
        income: None,
        settled: false,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1),
        ts_init: UnixNanos::from(2),
    };
    let position = Position::new_from_operation_checked(instrument, leg).unwrap();
    node.kernel()
        .cache
        .borrow_mut()
        .add_position_without_order(&position, OmsType::Netting)
        .unwrap();
}

fn build_node_with_position_operations(
    name: &str,
    clob_addr: SocketAddr,
    position_addr: SocketAddr,
    funder: String,
) -> LiveNode {
    let config = LiveNodeConfig {
        environment: Environment::Live,
        trader_id: TraderId::from(harness::TRADER_ID),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: true,
            ..Default::default()
        },
        delay_post_stop: Duration::from_millis(50),
        ..Default::default()
    };
    let mut exec_config: PolymarketExecutionClientConfig = execution_config(clob_addr);
    exec_config.private_key = Some(POSITION_PRIVATE_KEY.into());
    exec_config.funder = Some(funder);
    exec_config.signature_type = PolymarketSignatureType::PolyGnosisSafe;
    exec_config.relayer_api_key = Some("relayer-key".into());
    exec_config.relayer_api_key_address = Some(POSITION_SIGNER.into());
    exec_config.base_url_relayer = Some(format!("http://{position_addr}"));
    exec_config.base_url_rpc = Some(format!("http://{position_addr}/"));

    LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name(name)
        .add_exec_client(
            None,
            Box::new(PolymarketExecutionClientFactory),
            Box::new(exec_config),
        )
        .unwrap()
        .build()
        .unwrap()
}

#[rstest]
#[tokio::test]
async fn seeded_position_operation_recovery_applies_before_clob_reconciliation() {
    let mock_state = TestServerState::default();
    mock_state.configure_default_order_success().await;
    mock_state.positions_request_gate.enable();
    mock_state.balance_response.lock().await["balance"] = json!("1099000000");
    *mock_state.orders_response_override.lock().await = Some(json!({
        "data": [],
        "next_cursor": "LTE=",
    }));
    let observed_state = mock_state.clone();
    let clob_addr = start_mock_server(mock_state).await;

    let position_state = PositionServerState::default();
    let position_addr = start_position_mock_server(position_state.clone()).await;
    position_state
        .set_position_balances([200_000_000, 200_000_000])
        .await;
    position_state
        .configure_convert_receipt(&[(256, 100_000_000), (257, 100_000_000)], &[], 99_000_000)
        .await;

    let wallet = position_state.wallet().await;
    let mut node = build_node_with_position_operations(
        "SeededPositionOperationRestart",
        clob_addr,
        position_addr,
        wallet.clone(),
    );

    let default_instrument = harness::instrument();
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(default_instrument.clone())
        .unwrap();
    node.kernel()
        .exec_engine
        .borrow_mut()
        .get_client_adapter_mut(&ClientId::from(POLYMARKET))
        .expect("Polymarket execution client")
        .on_instrument(default_instrument);

    let no0 = position_instrument(&format!("0x{}{:02x}", "11".repeat(31), 0), 256, "No");
    let no1 = position_instrument(&format!("0x{}{:02x}", "11".repeat(31), 1), 257, "No");
    let (no0_id, no1_id) = (no0.id(), no1.id());
    for instrument in [&no0, &no1] {
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
            .on_instrument(instrument.clone());
        seed_operation_position(
            &node,
            instrument,
            Quantity::from("100.0000"),
            Decimal::from(50),
        );
    }

    let operation_id = UUID4::new();
    let intent = ManagedOperationIntent {
        operation_id,
        trader_id: TraderId::from(harness::TRADER_ID),
        strategy_id: StrategyId::from(POSITION_STRATEGY_ID),
        account_id: AccountId::from(harness::ACCOUNT_ID),
        operation: PositionOperationType::Convert,
        instrument_ids: vec![no0_id, no1_id],
        barrier_instrument_ids: vec![no0_id, no1_id],
        quantity: Some(Quantity::from("100.0000")),
        condition_id: None,
        market_id: Some(POSITION_MARKET_ID.to_string()),
        index_set: Some("3".to_string()),
    };
    let mut record = PositionOperationRecord::prepared(
        &intent,
        &wallet,
        "7",
        "1700000000",
        Vec::new(),
        UnixNanos::from(1),
    );
    record.mark_submitted("tx-1".to_string(), UnixNanos::from(2));
    node.kernel()
        .cache
        .borrow_mut()
        .add(
            &PositionOperationRecord::key(&operation_id),
            serde_json::to_vec(&record).unwrap().into(),
        )
        .unwrap();

    let cache_at_request = node.kernel().cache.clone();
    let handle = node.handle();
    let driver = async move {
        let requested = tokio::time::timeout(DEADLINE, async {
            loop {
                if observed_state.positions_request_gate.started() > 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok();
        let applied_at_request = {
            let cache = cache_at_request.borrow();
            [no0_id, no1_id].into_iter().all(|instrument_id| {
                cache
                    .position(&PositionId::from(format!(
                        "{instrument_id}-{POSITION_STRATEGY_ID}"
                    )))
                    .is_some_and(|position| position.is_closed())
            })
        };
        observed_state.positions_request_gate.release();
        assert!(
            requested,
            "startup CLOB position reconciliation was not requested"
        );
        assert!(
            applied_at_request,
            "operation effects must be applied before the first CLOB position reconciliation request",
        );

        wait_until_async(
            || async { matches!(handle.state(), NodeState::Running | NodeState::Stopped) },
            DEADLINE,
        )
        .await;
        let running = handle.state();
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.stop();
        running
    };
    let (result, running) =
        tokio::time::timeout(RUN_TIMEOUT, async { tokio::join!(node.run(), driver) })
            .await
            .unwrap();
    result.expect("node run");
    assert_eq!(running, NodeState::Running);

    let cache = node.kernel().cache.borrow();
    for instrument_id in [no0_id, no1_id] {
        let position = cache
            .position(&PositionId::from(format!(
                "{instrument_id}-{POSITION_STRATEGY_ID}"
            )))
            .expect("position must exist")
            .clone();
        assert!(position.is_closed());
        assert_eq!(
            position.realized_pnl,
            Some(Money::new(-0.5, Currency::pUSD())),
            "CLOB reconciliation must not close {instrument_id} before recovery",
        );
    }
    let record_value: serde_json::Value = serde_json::from_slice(
        cache
            .get(&PositionOperationRecord::key(&operation_id))
            .unwrap()
            .expect("operation record must exist"),
    )
    .unwrap();
    assert_eq!(record_value["application_status"], "Applied");
    let account = cache
        .account(&AccountId::from(harness::ACCOUNT_ID))
        .expect("account state must be refreshed")
        .clone();
    assert_eq!(
        account.balances()[&Currency::pUSD()].total.as_decimal(),
        Decimal::from(1099)
    );
    drop(cache);

    assert!(
        !node
            .kernel()
            .exec_engine
            .borrow()
            .is_position_operation_barred(&no0_id)
    );
    assert_eq!(
        position_state.submit_count(),
        0,
        "recovery must not resubmit"
    );
    assert!(
        position_state.transaction_get_count() > 0,
        "recovery must query the original Relayer transaction",
    );
    assert_eq!(
        node.kernel().portfolio().net_position(&no0_id),
        Decimal::ZERO
    );
}

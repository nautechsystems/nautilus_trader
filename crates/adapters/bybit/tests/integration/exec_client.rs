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

//! Integration tests for `BybitExecutionClient`.
//!
//! These tests verify execution client operations including connection,
//! order submission, cancellation, and event handling.

use std::{
    cell::RefCell, collections::VecDeque, net::SocketAddr, rc::Rc, sync::atomic::Ordering,
    time::Duration,
};

use nautilus_bybit::{
    common::{
        consts::{BYBIT_CLIENT_ID, BYBIT_VENUE},
        enums::{
            BybitEnvironment, BybitMarginMode, BybitOrderSmpType, BybitPositionMode,
            BybitProductType,
        },
    },
    config::BybitExecutionClientConfig,
    data_types::{BybitMarginBorrowResult, BybitMarginRepayResult, BybitMarginStatusResult},
    execution::BybitExecutionClient,
    http::{client::BybitHttpClient, error::BybitCancelOrderError},
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::{replace_system_event_sender, set_exec_event_sender},
    messages::{
        ExecutionEvent, SystemEvent,
        execution::{
            BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, ExecutionReport,
            GenerateFillReports, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, SubmitOrder,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos, params::Params};
use nautilus_live::{ExecutionClientCore, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    data::CustomData,
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::{
        AccountState, OrderCanceled, OrderDenied, OrderEventAny,
        order::spec::OrderPendingCancelSpec,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::stubs::crypto_perpetual_ethusdt,
    orders::{
        LimitIfTouchedOrder, MarketIfTouchedOrder, MarketOrder, Order, OrderAny, StopLimitOrder,
        StopMarketOrder, TrailingStopMarketOrder, builder::OrderTestBuilder,
        stubs::TestOrderEventStubs,
    },
    reports::FillReport,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use ustr::Ustr;

#[path = "../../../../network/tests/integration/common/client_logging.rs"]
#[allow(dead_code, reason = "these tests reuse only the shared log capture")]
mod client_logging;

#[path = "../common/mock_venue.rs"]
mod mock_venue;

use mock_venue::{TestServerState, create_test_exec_config, create_test_router, load_test_data};

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let health_url = format!("http://{addr}/v3/public/time");
    let http_client = HttpClient::builder().build().unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;
    Ok((addr, state))
}

fn create_test_demo_exec_config(addr: SocketAddr) -> BybitExecutionClientConfig {
    let mut config = create_test_exec_config(addr);
    config.environment = BybitEnvironment::Demo;
    config.max_retries = 0;
    config
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_with_config(create_test_exec_config(addr))
}

fn create_test_execution_client_with_config(
    config: BybitExecutionClientConfig,
) -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    // Event channel must be set before creating client due to thread-local storage
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = BybitExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

#[rstest]
#[case(true, 1)]
#[case(false, 0)]
#[tokio::test]
async fn test_exec_client_scoped_spot_position_reports_follow_config(
    #[case] use_spot_position_reports: bool,
    #[case] expected_reports: usize,
) {
    let (addr, _state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Spot];
    config.use_spot_position_reports = use_spot_position_reports;
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();

    let reports = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETHUSDT-SPOT.BYBIT")),
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap();

    assert_eq!(reports.len(), expected_reports);
    if use_spot_position_reports {
        assert_eq!(reports[0].instrument_id, "ETHUSDT-SPOT.BYBIT".into());
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_mixed_unscoped_position_reports_omit_spot() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Linear, BybitProductType::Spot];
    config.use_spot_position_reports = true;
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();

    // Measure against a post-connect baseline, since connect issues requests of its own.
    let positions_before = state.position_requests.load(Ordering::Relaxed);
    let wallet_before = state.wallet_balance_requests.load(Ordering::Relaxed);

    let reports = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("derivative reports must succeed when SPOT coverage is unavailable");

    assert!(
        reports
            .iter()
            .any(|report| report.instrument_id == "BTCUSDT-LINEAR.BYBIT".into())
    );
    // An unscoped LINEAR query is one request per settle coin (USDT, USDC); SPOT adds none.
    assert_eq!(
        state.position_requests.load(Ordering::Relaxed) - positions_before,
        2,
        "the LINEAR product type should still be requested"
    );
    // SPOT is served from wallet balances, so this is what proves it was omitted.
    assert_eq!(
        state.wallet_balance_requests.load(Ordering::Relaxed) - wallet_before,
        0,
        "no SPOT wallet balance request should be issued for a bulk report"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[case(vec![], "BTCUSDT-LINEAR.BYBIT", true)]
#[case(vec![], "BTCUSD-INVERSE.BYBIT", false)]
#[case(vec![], "ETHUSDT-SPOT.BYBIT", false)]
#[case(vec![], "BTCUSDT.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "BTCUSDT-LINEAR.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "BTCUSD-INVERSE.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "ETHUSDT-SPOT.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "BTCUSDT.BYBIT", false)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "BTCUSDT-LINEAR.BYBIT",
    true
)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "BTCUSD-INVERSE.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "ETHUSDT-SPOT.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "BTCUSDT.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "BTCUSDT-LINEAR.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "BTCUSD-INVERSE.BYBIT",
    true
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "ETHUSDT-SPOT.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "BTCUSDT.BYBIT",
    false
)]
#[tokio::test]
async fn test_exec_client_bulk_position_coverage_by_product_type(
    #[case] product_types: Vec<BybitProductType>,
    #[case] instrument_id: &str,
    #[case] expected: bool,
) {
    let (addr, _state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = product_types;
    let (client, _rx, _cache) = create_test_execution_client_with_config(config);

    // An identifier carrying no recognized product-type suffix must fail closed: absence of a
    // report from a bulk request is not evidence the position is flat.
    assert_eq!(
        client.provides_bulk_position_coverage(InstrumentId::from(instrument_id)),
        expected
    );
}

#[rstest]
#[case::targeted(false)]
#[case::mass_status(true)]
#[tokio::test]
async fn test_exec_client_commission_failure_and_recovery(#[case] mass_status: bool) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    let mut response = load_test_data("http_get_executions.json");
    response["result"]["nextPageCursor"] = json!("");
    response["result"]["list"][1]["execFee"] = json!("9999999999999999999999999999");
    *state.execution_response.lock().await = Some(response.clone());
    let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
    let venue_order_id = VenueOrderId::from("7b954230-6a41-3ba8-bc20-26d20ab44b83");

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(instrument_id),
        Some(venue_order_id),
        None,
        None,
        None,
        None,
    );

    let e = if mass_status {
        client.generate_mass_status(None).await.unwrap_err()
    } else {
        client.generate_fill_reports(cmd.clone()).await.unwrap_err()
    };

    assert!(
        e.to_string()
            .contains("Failed to create commission from execFee=")
    );

    response["result"]["list"][1]["execFee"] = json!("0.01234567");
    *state.execution_response.lock().await = Some(response);

    let reports = if mass_status {
        client
            .generate_mass_status(None)
            .await
            .unwrap()
            .unwrap()
            .fill_reports()
            .shift_remove(&venue_order_id)
            .unwrap()
    } else {
        client.generate_fill_reports(cmd).await.unwrap()
    };

    assert_eq!(reports.len(), 1);

    let expected = FillReport::new(
        AccountId::from("BYBIT-001"),
        instrument_id,
        venue_order_id,
        TradeId::from("f1dce92e-1g29-6977-a526-dg420c6ecb4c"),
        OrderSide::Sell,
        Quantity::from("0.50"),
        Price::from("3000.00"),
        Money::from_decimal(dec!(0.01234567), Currency::USDT()).unwrap(),
        LiquiditySide::Taker,
        Some(ClientOrderId::from("test-order-002")),
        None,
        UnixNanos::from(1_672_282_822_529_000_000),
        reports[0].ts_init,
        Some(reports[0].report_id),
    );

    assert_eq!(reports, vec![expected]);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_mass_status_omits_spot_and_preserves_derivative_positions() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Linear, BybitProductType::Spot];
    config.use_spot_position_reports = true;
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    state.wallet_balance_requests.store(0, Ordering::Relaxed);

    let mass_status = client
        .generate_mass_status(None)
        .await
        .expect("mass status must not fail when SPOT coverage is unavailable")
        .expect("mass status must be populated");

    assert!(
        mass_status
            .position_reports()
            .contains_key(&InstrumentId::from("BTCUSDT-LINEAR.BYBIT"))
    );
    assert_eq!(state.wallet_balance_requests.load(Ordering::Relaxed), 0);

    client.disconnect().await.unwrap();
}

fn create_test_demo_execution_client(
    addr: SocketAddr,
) -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = BybitExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("10000.0 USDT"),
            Money::from("0 USDT"),
            Money::from("10000.0 USDT"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

async fn drain_execution_events(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>) {
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}
}

async fn assert_no_cancel_rejected(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    duration: Duration,
) {
    let reject_window = tokio::time::sleep(duration);
    tokio::pin!(reject_window);

    loop {
        tokio::select! {
            () = &mut reject_window => break,
            event = rx.recv() => {
                let event = event.expect("channel closed");
                assert!(
                    !matches!(event, ExecutionEvent::Order(OrderEventAny::CancelRejected(_))),
                    "Ambiguous cancel outcome must not emit OrderCancelRejected: {event:?}",
                );
            }
        }
    }
}

async fn assert_no_modify_rejected(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    duration: Duration,
) {
    let reject_window = tokio::time::sleep(duration);
    tokio::pin!(reject_window);

    loop {
        tokio::select! {
            () = &mut reject_window => break,
            event = rx.recv() => {
                let event = event.expect("channel closed");
                assert!(
                    !matches!(event, ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))),
                    "Ambiguous modify outcome must not emit OrderModifyRejected: {event:?}",
                );
            }
        }
    }
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), *BYBIT_CLIENT_ID);
    assert_eq!(client.venue(), *BYBIT_VENUE);
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert!(!client.is_connected());
}

#[rstest]
#[case::standard(false, "execution")]
#[case::fast(true, "execution.fast")]
#[tokio::test]
async fn test_exec_client_connect_disconnect(
    #[case] use_ws_execution_fast: bool,
    #[case] expected_execution_topic: &str,
) {
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let (addr, state) = start_test_server().await.unwrap();
    let registry = SocketReconnectRegistry::default();
    let mut config = create_test_exec_config(addr);
    config.use_ws_execution_fast = use_ws_execution_fast;
    let (mut client, _rx, cache) =
        registry.scope(|| create_test_execution_client_with_config(config));
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.ws_connection_count.lock().await >= 2 },
        Duration::from_secs(10),
    )
    .await;
    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    let mut connected = Vec::new();
    while connected.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let SystemEvent::SocketState(change) = event;
        if change.state == SocketState::Connected {
            connected.push(change.endpoint);
        }
    }
    connected.sort_unstable();
    let expected = vec![
        Ustr::from("bybit-trading"),
        Ustr::from("bybit-user-streams"),
    ];
    assert!(client.is_connected());
    assert!(state.authenticated.load(Ordering::Relaxed));
    assert_eq!(connected, expected);

    for endpoint in &expected {
        let handle = registry.handle(*BYBIT_CLIENT_ID, *endpoint).unwrap();
        assert_eq!(
            handle.request_reconnect(),
            SocketReconnectRequestOutcome::Accepted
        );
    }

    let mut disconnected = Vec::new();
    while disconnected.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let SystemEvent::SocketState(change) = event;
        if change.state == SocketState::Disconnected {
            disconnected.push(change.endpoint);
        }
    }
    disconnected.sort_unstable();
    assert_eq!(disconnected, expected);

    let subs = state.subscriptions.lock().await;
    assert_eq!(
        *subs,
        vec![
            "order".to_string(),
            expected_execution_topic.to_string(),
            "position".to_string(),
            "wallet".to_string(),
        ]
    );
    drop(subs);

    if use_ws_execution_fast {
        wait_until_async(
            || async { state.fee_rate_requests.load(Ordering::Relaxed) == 1 },
            Duration::from_secs(5),
        )
        .await;
    } else {
        assert_eq!(state.fee_rate_requests.load(Ordering::Relaxed), 0);
    }

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
    assert!(
        expected
            .iter()
            .all(|endpoint| registry.handle(*BYBIT_CLIENT_ID, *endpoint).is_none())
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_reconnect_installs_repay_sender_before_execution_subscription() {
    let _capture = client_logging::capture_logs().await;
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Spot];
    config.auto_repay_spot_borrows = true;
    let (mut client, mut rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::from("ETHUSDT-SPOT.BYBIT");
    let client_order_id = ClientOrderId::from("spot-repay-reconnect");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Ioc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let init = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, Some(*BYBIT_CLIENT_ID), false)
        .unwrap();
    client
        .submit_order(SubmitOrder::new(
            trader_id,
            Some(*BYBIT_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            init,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();
    wait_until_async(
        || async { state.trade_order_payloads.lock().await.len() == 1 },
        Duration::from_secs(5),
    )
    .await;
    client.disconnect().await.unwrap();

    let mut execution = load_test_data("ws_account_execution.json");
    execution["data"][0]["category"] = json!("spot");
    execution["data"][0]["symbol"] = json!("ETHUSDT");
    execution["data"][0]["orderId"] = json!("spot-repay-order");
    execution["data"][0]["orderLinkId"] = json!(client_order_id.as_str());
    execution["data"][0]["execPrice"] = json!("3000");
    execution["data"][0]["execQty"] = json!("0.01");
    execution["data"][0]["execValue"] = json!("30");
    execution["data"][0]["execFee"] = json!("0.00001");
    execution["data"][0]["feeCurrency"] = json!("ETH");
    execution["data"][0]["orderPrice"] = json!("3000");
    execution["data"][0]["orderQty"] = json!("0.01");
    execution["data"][0]["side"] = json!("Buy");
    *state.subscription_execution.lock().await = Some(execution);

    client.connect().await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Filled(filled)))
                    if filled.client_order_id == client_order_id =>
                {
                    break;
                }
                Some(_) => {}
                None => panic!("execution event channel closed"),
            }
        }
    })
    .await
    .unwrap();

    wait_until_async(
        || async { state.repay_requests.load(Ordering::Relaxed) == 1 },
        Duration::from_secs(95),
    )
    .await;
    assert!(
        !client_logging::messages()
            .into_iter()
            .any(|(_, message)| message.contains("Failed to enqueue spot borrow repayment"))
    );
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_open_only_retains_recent_closed() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    state
        .rejected_orders_realtime
        .store(true, Ordering::Relaxed);

    let command = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from("ETHUSDT-LINEAR.BYBIT")),
        None,
        None,
        None,
        None,
    );
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].venue_order_id, "test-order-id-12345".into());
    assert_eq!(reports[0].order_status, OrderStatus::Rejected);
    assert_eq!(reports[1].venue_order_id, "open-order-2".into());
    assert_eq!(reports[1].order_status, OrderStatus::Accepted);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_failed_startup_rolls_back_private_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    state.reject_trade_websocket.store(true, Ordering::Relaxed);
    let registry = SocketReconnectRegistry::default();
    let (mut client, _rx, cache) = registry.scope(|| create_test_execution_client(addr));
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    let error = client.connect().await.unwrap_err();

    wait_until_async(
        || async { *state.ws_connection_count.lock().await == 0 },
        Duration::from_secs(2),
    )
    .await;
    assert!(
        error.to_string().contains("WebSocket transport error"),
        "unexpected startup error: {error:#}",
    );
    assert_eq!(state.private_ws_connections.load(Ordering::Relaxed), 1);
    assert!(!client.is_connected());
    assert!(
        ["bybit-user-streams", "bybit-trading"]
            .into_iter()
            .all(|endpoint| registry
                .handle(*BYBIT_CLIENT_ID, Ustr::from(endpoint))
                .is_none())
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_applies_position_mode_for_derivative_symbols() {
    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );

    let mut position_mode = std::collections::HashMap::new();
    position_mode.insert("ETHUSDT-LINEAR".to_string(), BybitPositionMode::BothSides);
    position_mode.insert(
        "BTCUSD-INVERSE".to_string(),
        BybitPositionMode::MergedSingle,
    );
    // Spot symbol must be filtered out (Bybit rejects switch-mode on Spot).
    position_mode.insert("BTCUSDT-SPOT".to_string(), BybitPositionMode::MergedSingle);

    let mut config = create_test_exec_config(addr);
    config.position_mode = Some(position_mode);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();

    client.connect().await.unwrap();

    wait_until_async(
        || async { state.switch_mode_requests.lock().await.len() >= 2 },
        Duration::from_secs(10),
    )
    .await;

    let requests = state.switch_mode_requests.lock().await;
    assert_eq!(
        requests.len(),
        2,
        "switch-mode should be called for Linear+Inverse only, not Spot",
    );

    let symbols: Vec<&str> = requests
        .iter()
        .filter_map(|r| r.get("symbol").and_then(|v| v.as_str()))
        .collect();
    assert!(symbols.contains(&"ETHUSDT"));
    assert!(symbols.contains(&"BTCUSD"));

    let categories: Vec<&str> = requests
        .iter()
        .filter_map(|r| r.get("category").and_then(|v| v.as_str()))
        .collect();
    assert!(categories.contains(&"linear"));
    assert!(categories.contains(&"inverse"));

    drop(requests);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_applies_leverage_and_margin_mode() {
    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );

    let mut leverages = std::collections::HashMap::new();
    leverages.insert("ETHUSDT-LINEAR".to_string(), 5);

    let mut config = create_test_exec_config(addr);
    config.futures_leverages = Some(leverages);
    config.margin_mode = Some(BybitMarginMode::RegularMargin);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();

    client.connect().await.unwrap();

    wait_until_async(
        || async {
            !state.set_leverage_requests.lock().await.is_empty()
                && !state.set_margin_mode_requests.lock().await.is_empty()
        },
        Duration::from_secs(10),
    )
    .await;

    let leverage_reqs = state.set_leverage_requests.lock().await;
    assert_eq!(leverage_reqs.len(), 1);
    assert_eq!(
        leverage_reqs[0].get("symbol").and_then(|v| v.as_str()),
        Some("ETHUSDT"),
    );
    assert_eq!(
        leverage_reqs[0].get("buyLeverage").and_then(|v| v.as_str()),
        Some("5"),
    );
    drop(leverage_reqs);

    let margin_reqs = state.set_margin_mode_requests.lock().await;
    assert_eq!(margin_reqs.len(), 1);
    assert_eq!(
        margin_reqs[0].get("setMarginMode").and_then(|v| v.as_str()),
        Some("REGULAR_MARGIN"),
    );
    drop(margin_reqs);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_mode_skips_trade_ws() {
    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );

    let config = BybitExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Demo,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/v5/private")),
        base_url_ws_trade: Some(format!("ws://{addr}/v5/trade")),
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        account_id: None,
        use_spot_position_reports: false,
        auto_repay_spot_borrows: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
        ..Default::default()
    };

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();

    // Wait for subscriptions to confirm connection phase is complete
    wait_until_async(
        || async { state.private_ws_connections.load(Ordering::Relaxed) >= 1 },
        Duration::from_secs(10),
    )
    .await;
    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    let private_count = state.private_ws_connections.load(Ordering::Relaxed);
    let trade_count = state.trade_ws_connections.load(Ordering::Relaxed);
    assert_eq!(private_count, 1, "Demo mode should connect to private WS");
    assert_eq!(trade_count, 0, "Demo mode should NOT connect to trade WS");

    assert!(client.is_connected());
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_query_order() {
    use nautilus_common::messages::execution::QueryOrder;

    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(10)).await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BYBIT_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE),
        ClientOrderId::from("client-open-1"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.query_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for query_order event")
        .expect("channel closed");

    match event {
        ExecutionEvent::Report(ExecutionReport::Order(report)) => {
            assert_eq!(
                report.client_order_id,
                Some(ClientOrderId::from("client-open-1")),
            );
        }
        other => panic!("Expected OrderStatusReport, was {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[case::derivatives_config_default(
    "ETHUSDT-LINEAR",
    Some(BybitOrderSmpType::CancelMaker),
    None,
    Some("CancelMaker")
)]
#[case::spot_param_overrides_config(
    "BTCUSDT-SPOT",
    Some(BybitOrderSmpType::CancelMaker),
    Some("CancelBoth"),
    Some("CancelBoth")
)]
#[case::spot_param_without_config("BTCUSDT-SPOT", None, Some("CancelTaker"), Some("CancelTaker"))]
#[case::unset_omits_field("ETHUSDT-LINEAR", None, None, None)]
#[tokio::test]
async fn test_exec_client_submit_order_sends_configured_smp_type(
    #[case] symbol: &str,
    #[case] configured: Option<BybitOrderSmpType>,
    #[case] param: Option<&str>,
    #[case] expected: Option<&str>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Linear, BybitProductType::Spot];
    config.smp_type = configured;

    let (mut client, mut rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let client_id = *BYBIT_CLIENT_ID;
    let instrument_id = InstrumentId::new(Symbol::from(symbol), *BYBIT_VENUE);
    let client_order_id = ClientOrderId::from("test-smp-submit");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let init = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let params = param.map(|value| Params::new([("smp_type".to_string(), json!(value))].into()));

    let command = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        client_order_id,
        init,
        None,
        None,
        params,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.submit_order(command).unwrap();

    wait_until_async(
        || async { state.trade_order_payloads.lock().await.len() == 1 },
        Duration::from_secs(5),
    )
    .await;

    let payloads = state.trade_order_payloads.lock().await;
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["op"].as_str(), Some("order.create"));

    let order_args = &payloads[0]["args"][0];

    assert_eq!(
        order_args.get("orderLinkId").and_then(Value::as_str),
        Some(client_order_id.as_str())
    );
    assert_eq!(order_args.get("smpType").and_then(Value::as_str), expected);

    drop(payloads);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_trade_rate_limit_emits_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;
    state.trade_order_ret_code.store(10006, Ordering::Relaxed);

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let client_id = *BYBIT_CLIENT_ID;
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);
    let client_order_id = ClientOrderId::from("test-ws-rate-limit-reject");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let init = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();
    let command = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        client_order_id,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.submit_order(command).unwrap();

    let submitted = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderSubmitted")
        .expect("channel closed");
    assert!(
        matches!(submitted, ExecutionEvent::Order(OrderEventAny::Submitted(ref event))
            if event.client_order_id == client_order_id),
        "Expected OrderSubmitted for {client_order_id}, was {submitted:?}",
    );
    wait_until_async(
        || async { state.trade_order_requests.load(Ordering::Relaxed) == 1 },
        Duration::from_secs(5),
    )
    .await;
    assert!(state.trade_order_req_id_present.load(Ordering::Relaxed));
    let rejected = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderRejected")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Rejected(event)) = rejected else {
        panic!("Expected OrderRejected, was {rejected:?}");
    };
    assert_eq!(event.client_order_id, client_order_id);
    assert_eq!(event.reason.to_string(), "Too many visits.");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_query_account_does_not_block_within_runtime() {
    use nautilus_common::messages::execution::QueryAccount;

    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(10)).await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*BYBIT_CLIENT_ID),
        AccountId::from("BYBIT-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.query_account(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for query_account event")
        .expect("channel closed");

    assert!(
        matches!(event, ExecutionEvent::Account(_)),
        "Expected ExecutionEvent::Account, was {event:?}"
    );

    client.disconnect().await.unwrap();
}

fn margin_query_account(params: Params) -> QueryAccount {
    QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*BYBIT_CLIENT_ID),
        AccountId::from("BYBIT-001"),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
        None, // correlation_id
    )
}

fn margin_params(entries: &[(&str, Value)]) -> Params {
    Params::new(
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

fn as_margin_data(event: &ExecutionEvent) -> &CustomData {
    let ExecutionEvent::Data(data) = event else {
        panic!("Expected ExecutionEvent::Data, was {event:?}");
    };

    data
}

async fn start_connected_client() -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    TestServerState,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(10)).await;

    (client, rx, state)
}

#[rstest]
#[case::string_amount(json!("1.500"), "1.500")]
#[case::number_amount(json!(1500), "1500")]
#[tokio::test]
async fn test_query_account_borrow_action_borrows_and_publishes_result(
    #[case] amount: Value,
    #[case] expected_qty: &str,
) {
    let (mut client, mut rx, state) = start_connected_client().await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let params = margin_params(&[
        ("action", json!("BORROW")),
        ("coin", json!("ETH")),
        ("amount", amount),
    ]);
    client.query_account(margin_query_account(params)).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for borrow result")
        .expect("channel closed");

    let data = as_margin_data(&event);
    assert_eq!(data.data_type.topic(), "BybitMarginBorrowResult");
    let result = data
        .data
        .as_any()
        .downcast_ref::<BybitMarginBorrowResult>()
        .expect("borrow result payload");
    assert_eq!(result.coin, "ETH");
    assert_eq!(result.amount, expected_qty);
    assert!(result.success, "borrow failed: {}", result.message);
    assert_eq!(result.message, "");

    let bodies = state.borrow_bodies.lock().await;
    assert_eq!(bodies.len(), 1, "expected exactly one borrow request");
    assert_eq!(bodies[0]["coin"], json!("ETH"));
    assert_eq!(bodies[0]["amount"], json!(expected_qty));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_query_account_borrow_action_without_amount_publishes_failed_result() {
    let (mut client, mut rx, state) = start_connected_client().await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let params = margin_params(&[("action", json!("BORROW")), ("coin", json!("ETH"))]);
    client.query_account(margin_query_account(params)).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for borrow result")
        .expect("channel closed");

    let data = as_margin_data(&event);
    let result = data
        .data
        .as_any()
        .downcast_ref::<BybitMarginBorrowResult>()
        .expect("borrow result payload");
    assert_eq!(result.coin, "ETH");
    assert!(!result.success);
    assert!(
        result.message.contains("'amount'"),
        "was: {}",
        result.message
    );

    assert!(
        state.borrow_bodies.lock().await.is_empty(),
        "a borrow without an amount must not reach the venue",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[case::zero(json!(0))]
#[case::negative(json!("-5"))]
#[case::not_a_number(json!("abc"))]
#[case::not_a_numeric_type(json!(true))]
#[tokio::test]
async fn test_query_account_borrow_action_rejects_invalid_amount(#[case] amount: Value) {
    let (mut client, mut rx, state) = start_connected_client().await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let params = margin_params(&[
        ("action", json!("BORROW")),
        ("coin", json!("ETH")),
        ("amount", amount),
    ]);
    client.query_account(margin_query_account(params)).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for borrow result")
        .expect("channel closed");

    let data = as_margin_data(&event);
    let result = data
        .data
        .as_any()
        .downcast_ref::<BybitMarginBorrowResult>()
        .expect("borrow result payload");
    assert_eq!(result.coin, "ETH");
    assert!(!result.success);
    assert!(
        result.message.contains("'amount'"),
        "was: {}",
        result.message
    );

    assert!(
        state.borrow_bodies.lock().await.is_empty(),
        "a borrow with an invalid amount must not reach the venue",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_query_account_borrow_action_without_coin_publishes_no_result() {
    let (mut client, mut rx, state) = start_connected_client().await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let params = margin_params(&[("action", json!("BORROW")), ("amount", json!("1.5"))]);
    client.query_account(margin_query_account(params)).unwrap();

    let received = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await;
    assert!(
        received.is_err(),
        "a borrow without a coin must not publish a result, was {received:?}",
    );

    assert!(
        state.borrow_bodies.lock().await.is_empty(),
        "a borrow without a coin must not reach the venue",
    );

    client.disconnect().await.unwrap();
}

async fn wait_out_repay_blackout() {
    // Manual repay is rejected inside Bybit's hourly mm:04:00-mm:05:30 UTC blackout
    // window (see repay.rs BLACKOUT_START_SEC/BLACKOUT_END_SEC), so wait the window
    // out before issuing one against the mock venue.
    loop {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after the UNIX epoch")
            .as_secs();
        let sec_of_hour = now_secs % 3600;
        if !(240..=330).contains(&sec_of_hour) {
            return;
        }

        tokio::time::sleep(Duration::from_secs(331 - sec_of_hour)).await;
    }
}

#[rstest]
#[case::explicit_amount_su(Some("0.500"), json!("0.500"), "SU")]
#[case::repay_all_processing(None, Value::Null, "P")]
#[tokio::test]
async fn test_query_account_repay_action_uses_no_convert_repay(
    #[case] amount: Option<&str>,
    #[case] expected_body_amount: Value,
    #[case] result_status: &'static str,
) {
    let (mut client, mut rx, state) = start_connected_client().await;

    *state.no_convert_result_status.lock().await = result_status;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    wait_out_repay_blackout().await;

    let mut entries = vec![("action", json!("REPAY")), ("coin", json!("ETH"))];
    if let Some(amount) = amount {
        entries.push(("amount", json!(amount)));
    }

    let params = margin_params(&entries);
    client.query_account(margin_query_account(params)).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for repay result")
        .expect("channel closed");

    let data = as_margin_data(&event);
    assert_eq!(data.data_type.topic(), "BybitMarginRepayResult");
    let result = data
        .data
        .as_any()
        .downcast_ref::<BybitMarginRepayResult>()
        .expect("repay result payload");
    assert_eq!(result.coin, "ETH");
    assert!(result.success, "repay failed: {}", result.message);
    assert_eq!(result.result_status, result_status);
    assert_eq!(result.amount.as_deref(), amount);

    let bodies = state.no_convert_repay_bodies.lock().await;
    assert_eq!(bodies.len(), 1, "expected exactly one no-convert repay");
    assert_eq!(bodies[0]["coin"], json!("ETH"));
    assert_eq!(bodies[0]["amount"], expected_body_amount);
    drop(bodies);

    assert_eq!(
        state.repay_requests.load(Ordering::Relaxed),
        0,
        "manual repay must not use the converting repay endpoint",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_query_account_get_borrow_amount_publishes_status_result() {
    let (mut client, mut rx, _state) = start_connected_client().await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // The wallet balance fixture carries spotBorrow=0.06142 for ETH.
    let params = margin_params(&[
        ("action", json!("GET_BORROW_AMOUNT")),
        ("coin", json!("ETH")),
    ]);
    client.query_account(margin_query_account(params)).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for borrow amount result")
        .expect("channel closed");

    let data = as_margin_data(&event);
    assert_eq!(data.data_type.topic(), "BybitMarginStatusResult");
    let result = data
        .data
        .as_any()
        .downcast_ref::<BybitMarginStatusResult>()
        .expect("status result payload");
    assert_eq!(result.coin, "ETH");
    assert_eq!(result.borrow_amount, "0.06142");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_query_account_unknown_action_refreshes_account_state() {
    let (mut client, mut rx, _addr) = start_connected_client().await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let params = margin_params(&[("action", json!("LEND")), ("coin", json!("ETH"))]);
    client.query_account(margin_query_account(params)).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for account state")
        .expect("channel closed");

    assert!(
        matches!(event, ExecutionEvent::Account(_)),
        "Expected ExecutionEvent::Account, was {event:?}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_demo() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BybitExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Demo,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/v5/private")),
        base_url_ws_trade: Some(format!("ws://{addr}/v5/trade")),
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        account_id: None,
        use_spot_position_reports: false,
        auto_repay_spot_borrows: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
        ..Default::default()
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cid1 = ClientOrderId::from("test-list-order-1");
    let cid2 = ClientOrderId::from("test-list-order-2");

    let order1 = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid1,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let order2 = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid2,
        OrderSide::Sell,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    let init1 = order1.init_event().clone();
    let init2 = order2.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order1, None, Some(client_id), false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-list-1"),
        instrument_id,
        strategy_id,
        vec![cid1, cid2],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init1, init2],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    let mut submitted_count = 0;

    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for OrderSubmitted")
            .expect("channel closed");

        if let ExecutionEvent::Order(ref order_event) = event
            && order_event.to_string().contains("OrderSubmitted")
        {
            submitted_count += 1;
        }
    }

    assert_eq!(submitted_count, 2, "Expected 2 OrderSubmitted events");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_cancel_confirms_without_order_stream(
    #[values(false, true)] batch: bool,
    #[values(false, true)] partial_fill: bool,
) {
    let _capture = client_logging::capture_logs().await;
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_demo_execution_client(addr);
    let account_id = AccountId::from("BYBIT-001");
    add_test_account_to_cache(&cache, account_id);
    client.connect().await.unwrap();
    client.start().unwrap();
    drain_execution_events(&mut rx).await;

    let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");

    let count = if batch { 2 } else { 1 };
    let mut cancels = Vec::new();

    for index in 1..=count {
        let client_order_id = ClientOrderId::from(format!("client-cancel-{index}"));
        let venue_order_id = VenueOrderId::from(format!("venue-cancel-{index}"));
        let strategy_id = StrategyId::from(format!("S-{index}"));
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .strategy_id(strategy_id)
            .quantity(Quantity::from("0.01"))
            .price(Price::from("3930.41"))
            .build();
        let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
        let pending = OrderPendingCancelSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .account_id(account_id)
            .venue_order_id(venue_order_id)
            .build();
        cache
            .borrow_mut()
            .add_order(order, None, Some(*BYBIT_CLIENT_ID), false)
            .unwrap();
        cache.borrow_mut().update_order(&accepted).unwrap();
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::PendingCancel(pending))
            .unwrap();

        let empty = load_test_data("http_get_orders_history_empty.json");
        let mut open = load_test_data("http_get_orders_realtime.json");
        open["result"]["list"].as_array_mut().unwrap().truncate(1);
        let entry = &mut open["result"]["list"][0];
        entry["orderId"] = json!(venue_order_id.to_string());
        entry["orderLinkId"] = json!("");
        let mut canceled = open.clone();
        canceled["result"]["list"][0]["orderStatus"] = json!("Cancelled");
        canceled["result"]["list"][0]["updatedTime"] = json!("1761355906606");
        if partial_fill {
            canceled["result"]["list"][0]["orderStatus"] = json!("PartiallyFilledCanceled");
            canceled["result"]["list"][0]["cumExecQty"] = json!("0.003");
            canceled["result"]["list"][0]["avgPrice"] = json!("3930.41");
        }

        state.cancel_order_reports.lock().await.insert(
            venue_order_id.to_string(),
            VecDeque::from([empty.clone(), empty, open, canceled]),
        );
        cancels.push(CancelOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ));
    }

    let history_before = state.orders_history_requests.load(Ordering::Relaxed);

    if batch {
        client
            .batch_cancel_orders(BatchCancelOrders::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from("S-1"),
                instrument_id,
                cancels.clone(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    } else {
        client.cancel_order(cancels[0].clone()).unwrap();
    }

    let mut canceled = Vec::new();

    for _ in 0..count {
        let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("timed out waiting for OrderCanceled")
            .expect("channel closed");

        match event {
            ExecutionEvent::Order(OrderEventAny::Canceled(event)) => canceled.push(event),
            event => panic!("expected OrderCanceled, was {event:?}"),
        }
    }

    canceled.sort_by_key(|event| event.client_order_id);
    let trade_id = TradeId::from("native-partial");

    for (event, cmd) in canceled.iter().zip(&cancels) {
        let expected = OrderCanceled::new(
            cmd.trader_id,
            cmd.strategy_id,
            instrument_id,
            cmd.client_order_id,
            event.event_id,
            UnixNanos::from(1_761_355_906_606_000_000),
            event.ts_init,
            false,
            cmd.venue_order_id,
            Some(account_id),
            None,
        );
        assert_eq!(*event, expected);
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::Canceled(*event))
            .unwrap();

        if partial_fill {
            let order = cache.borrow().order(&cmd.client_order_id).unwrap().clone();
            let mut instrument = crypto_perpetual_ethusdt();
            instrument.id = instrument_id;
            let filled = TestOrderEventStubs::filled(
                &order,
                &instrument.into(),
                Some(trade_id),
                None,
                Some(Price::from("3930.41")),
                Some(Quantity::from("0.003")),
                None,
                None,
                None,
                Some(account_id),
            );
            cache.borrow_mut().update_order(&filled).unwrap();
        }

        let cache = cache.borrow();
        let order = cache.order(&cmd.client_order_id).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(
            order.filled_qty(),
            Quantity::from(if partial_fill { "0.003" } else { "0.000" })
        );
        assert_eq!(
            order.trade_ids(),
            if partial_fill {
                vec![&trade_id]
            } else {
                vec![]
            }
        );
    }

    assert_eq!(state.cancel_order_requests.load(Ordering::Relaxed), count);
    assert_eq!(
        state.orders_history_requests.load(Ordering::Relaxed) - history_before,
        count * 2
    );
    assert!(rx.try_recv().is_err());
    assert!(
        !client_logging::messages()
            .iter()
            .any(|(_, message)| message.contains("Ambiguous HTTP cancel failure"))
    );
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::missing(true)]
#[case::open(false)]
#[tokio::test]
async fn test_http_cancel_confirmation_timeout(#[case] missing: bool) {
    let (addr, state) = start_test_server().await.unwrap();
    state
        .empty_orders_realtime
        .store(missing, Ordering::Relaxed);
    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(format!("http://{addr}")),
        1,
        0,
        10,
        100,
        5000,
        None,
    )
    .unwrap();
    let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.id = instrument_id;
    client.cache_instrument(instrument.into());
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        client.cancel_order(
            AccountId::from("BYBIT-001"),
            BybitProductType::Linear,
            instrument_id,
            Some(ClientOrderId::from("client-open-1")),
            Some(VenueOrderId::from("open-order-1")),
        ),
    )
    .await
    .expect("cancel confirmation must remain bounded");
    let error = result.unwrap_err();

    let Some(BybitCancelOrderError::PostCancelLookup { source }) = error.downcast_ref() else {
        panic!("expected PostCancelLookup, was {error:?}");
    };

    assert_eq!(
        source.to_string(),
        "timed out confirming Bybit order cancellation"
    );
    assert_eq!(state.cancel_order_requests.load(Ordering::Relaxed), 1);
    assert!(state.orders_realtime_requests.load(Ordering::Relaxed) > 1);
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_cancel_filled_confirmation_emits_no_event() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_demo_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    client.start().unwrap();
    drain_execution_events(&mut rx).await;
    let mut response = load_test_data("http_get_orders_realtime.json");
    response["result"]["list"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let order = &mut response["result"]["list"][0];
    order["orderStatus"] = json!("Filled");
    order["cumExecQty"] = json!("0.010");
    order["avgPrice"] = json!("3930.41");
    order["leavesQty"] = json!("0");
    state
        .cancel_order_reports
        .lock()
        .await
        .insert("open-order-1".to_string(), VecDeque::from([response]));
    let before = state.orders_realtime_requests.load(Ordering::Relaxed);
    client
        .cancel_order(CancelOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            InstrumentId::from("ETHUSDT-LINEAR.BYBIT"),
            ClientOrderId::from("client-open-1"),
            Some(VenueOrderId::from("open-order-1")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until_async(
        || async { state.orders_realtime_requests.load(Ordering::Relaxed) > before },
        Duration::from_secs(3),
    )
    .await;
    let event = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await;
    assert!(
        event.is_err(),
        "filled confirmation must not emit an inferred event: {event:?}"
    );
    assert_eq!(state.cancel_order_requests.load(Ordering::Relaxed), 1);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_batch_cancel_failure_preserves_owning_strategies() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_demo_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    state.cancel_order_ret_code.store(110001, Ordering::Relaxed);

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);
    let cancels: Vec<CancelOrder> = ["S-001", "S-002"]
        .into_iter()
        .enumerate()
        .map(|(index, owner)| {
            CancelOrder::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from(owner),
                instrument_id,
                ClientOrderId::from(format!("O-DEMO-BATCH-{index}")),
                Some(VenueOrderId::from(format!("venue-batch-{index}"))),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        })
        .collect();

    client
        .batch_cancel_orders(BatchCancelOrders::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            instrument_id,
            cancels.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let mut actual = Vec::new();

    for _ in &cancels {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for OrderCancelRejected")
            .expect("channel closed");

        match event {
            ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) => {
                actual.push((rejected.client_order_id, rejected.strategy_id));
            }
            event => panic!("expected OrderCancelRejected, was {event:?}"),
        }
    }

    let mut expected: Vec<_> = cancels
        .iter()
        .map(|cancel| (cancel.client_order_id, cancel.strategy_id))
        .collect();
    actual.sort();
    expected.sort();

    assert_eq!(actual, expected);
    assert_eq!(state.cancel_order_requests.load(Ordering::Relaxed), 2);
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_sided_cancel_all_with_empty_cache_sends_nothing() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_demo_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    state.empty_orders_realtime.store(true, Ordering::Relaxed);

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);
    assert!(
        cache
            .borrow()
            .orders_open(None, Some(&instrument_id), None, None, None)
            .is_empty()
    );

    let cancel_all_before = state.cancel_all_requests.load(Ordering::Relaxed);
    let cancel_before = state.cancel_order_requests.load(Ordering::Relaxed);

    client
        .cancel_all_orders(CancelAllOrders::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            instrument_id,
            Some(OrderSide::Buy),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    assert_no_cancel_rejected(&mut rx, Duration::from_millis(300)).await;
    assert_eq!(
        state.cancel_all_requests.load(Ordering::Relaxed),
        cancel_all_before
    );
    assert_eq!(
        state.cancel_order_requests.load(Ordering::Relaxed),
        cancel_before
    );
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
}

#[rstest]
#[case::demo(BybitEnvironment::Demo, false, false)]
#[case::mainnet(BybitEnvironment::Mainnet, false, false)]
#[case::demo_partial_fill(BybitEnvironment::Demo, true, false)]
#[case::mainnet_partial_fill(BybitEnvironment::Mainnet, true, false)]
#[case::demo_history(BybitEnvironment::Demo, false, true)]
#[case::mainnet_history(BybitEnvironment::Mainnet, false, true)]
#[tokio::test]
async fn test_exec_client_cancel_all_confirms_cancellations(
    #[case] environment: BybitEnvironment,
    #[case] partial_fill: bool,
    #[case] history: bool,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.environment = environment;
    config.retry_delay_initial_ms = 10;
    let (mut client, mut rx, cache) = create_test_execution_client_with_config(config);
    let account_id = AccountId::from("BYBIT-001");
    add_test_account_to_cache(&cache, account_id);
    client.connect().await.unwrap();
    client.start().unwrap();
    drain_execution_events(&mut rx).await;

    let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");

    for (index, side) in [OrderSide::Buy, OrderSide::Sell].into_iter().enumerate() {
        let client_order_id = ClientOrderId::from(format!("client-open-{}", index + 1));
        let venue_order_id = VenueOrderId::from(format!("open-order-{}", index + 1));
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        let order = builder
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .strategy_id(StrategyId::from(format!("S-{}", index + 1)))
            .side(side)
            .quantity(Quantity::from("0.01"))
            .price(Price::from("3930.41"))
            .build();
        let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
        cache
            .borrow_mut()
            .add_order(order, None, Some(*BYBIT_CLIENT_ID), false)
            .unwrap();
        cache.borrow_mut().update_order(&accepted).unwrap();
    }

    state.orders_realtime_requests.store(0, Ordering::Relaxed);
    state
        .cancel_all_delayed_reports
        .store(true, Ordering::Relaxed);
    state
        .cancel_all_partial_fills
        .store(partial_fill, Ordering::Relaxed);
    state.cancel_all_history.store(history, Ordering::Relaxed);
    client
        .cancel_all_orders(CancelAllOrders::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-1"),
            instrument_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let mut canceled = Vec::new();

    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for order cancellation")
            .expect("channel closed");

        match event {
            ExecutionEvent::Order(OrderEventAny::Canceled(event)) => canceled.push(event),
            event => panic!("expected OrderCanceled, was {event:?}"),
        }
    }

    canceled.sort_by_key(|event| event.client_order_id);

    for (index, event) in canceled.iter().enumerate() {
        let client_order_id = ClientOrderId::from(format!("client-open-{}", index + 1));

        let expected = OrderCanceled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from(format!("S-{}", index + 1)),
            instrument_id,
            client_order_id,
            event.event_id,
            UnixNanos::from(1_761_355_906_606_000_000),
            event.ts_init,
            false,
            Some(VenueOrderId::from(format!("open-order-{}", index + 1))),
            Some(account_id),
            None,
        );

        assert_eq!(*event, expected);
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::Canceled(*event))
            .unwrap();

        if partial_fill {
            let order = cache.borrow().order(&client_order_id).unwrap().clone();
            let mut instrument = crypto_perpetual_ethusdt();
            instrument.id = instrument_id;
            let trade_id = TradeId::from(format!("native-fill-{}", index + 1));
            let quantity = Quantity::from(if index == 0 { "0.003" } else { "0.004" });
            let filled = TestOrderEventStubs::filled(
                &order,
                &instrument.into(),
                Some(trade_id),
                None,
                Some(Price::from(if index == 0 { "3930.41" } else { "3940.42" })),
                Some(quantity),
                None,
                None,
                None,
                Some(account_id),
            );
            cache.borrow_mut().update_order(&filled).unwrap();
            let cache = cache.borrow();
            let order = cache.order(&client_order_id).unwrap();

            assert_eq!(order.status(), OrderStatus::Canceled);
            assert_eq!(order.filled_qty(), quantity);
            assert_eq!(order.trade_ids(), vec![&trade_id]);
            assert_eq!(order.strategy_id(), expected.strategy_id);
        }
    }

    assert_eq!(state.cancel_all_requests.load(Ordering::Relaxed), 1);
    assert_eq!(
        state.orders_realtime_requests.load(Ordering::Relaxed),
        if history { 0 } else { 3 }
    );
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::trigger_only(None, None, Some("1732.45"))]
#[case::combined(Some("0.023"), Some("1741.23"), Some("1732.45"))]
#[case::limit(Some("0.023"), Some("1741.23"), None)]
#[tokio::test]
async fn test_exec_client_modify_order_payload(
    #[values(true, false)] demo: bool,
    #[case] quantity: Option<&str>,
    #[case] price: Option<&str>,
    #[case] trigger_price: Option<&str>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let config = if demo {
        create_test_demo_exec_config(addr)
    } else {
        create_test_exec_config(addr)
    };

    let (mut client, mut rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    client.start().unwrap();
    drain_execution_events(&mut rx).await;

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BYBIT_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("ETHUSDT-LINEAR.BYBIT"),
        ClientOrderId::from("test-amend-trigger"),
        Some(VenueOrderId::from("open-order-1")),
        quantity.map(Quantity::from),
        price.map(Price::from),
        trigger_price.map(Price::from),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.modify_order(cmd).unwrap();

    wait_until_async(
        || async {
            if demo {
                !state.amend_order_payloads.lock().await.is_empty()
            } else {
                !state.trade_order_payloads.lock().await.is_empty()
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let mut expected = json!({
        "category": "linear",
        "symbol": "ETHUSDT",
        "orderId": "open-order-1",
    });

    for (key, value) in [
        ("qty", quantity),
        ("price", price),
        ("triggerPrice", trigger_price),
    ] {
        if let Some(value) = value {
            expected[key] = json!(value);
        }
    }

    if demo {
        assert_eq!(*state.amend_order_payloads.lock().await, vec![expected]);
        assert_eq!(state.trade_order_requests.load(Ordering::Relaxed), 0);
    } else {
        expected["orderLinkId"] = json!("test-amend-trigger");
        let payloads = state.trade_order_payloads.lock().await;
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["op"], "order.amend");
        assert_eq!(payloads[0]["args"], json!([expected]));
        assert!(state.amend_order_payloads.lock().await.is_empty());
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_batch_modify_orders_payload(#[values(true, false)] demo: bool) {
    let (addr, state) = start_test_server().await.unwrap();
    let config = if demo {
        create_test_demo_exec_config(addr)
    } else {
        create_test_exec_config(addr)
    };

    let (mut client, mut rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    client.start().unwrap();
    drain_execution_events(&mut rx).await;

    let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
    let trader_id = TraderId::from("TESTER-001");

    let modifies = [
        (
            "S-001",
            "batch-modify-1",
            "venue-mod-1",
            Some("0.021"),
            Some("1741.23"),
            None,
            None,
        ),
        (
            "S-002",
            "batch-modify-2",
            "venue-mod-2",
            Some("0.033"),
            Some("1755.50"),
            Some("1732.45"),
            (!demo).then_some("0.42"),
        ),
    ]
    .into_iter()
    .map(
        |(owner, client_order_id, venue_order_id, quantity, price, trigger_price, order_iv)| {
            let params = order_iv
                .map(|order_iv| Params::new([("order_iv".to_string(), json!(order_iv))].into()));

            ModifyOrder::new(
                trader_id,
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from(owner),
                instrument_id,
                ClientOrderId::from(client_order_id),
                Some(VenueOrderId::from(venue_order_id)),
                quantity.map(Quantity::from),
                price.map(Price::from),
                trigger_price.map(Price::from),
                UUID4::new(),
                UnixNanos::default(),
                params,
                None,
            )
        },
    )
    .collect::<Vec<_>>();

    client
        .batch_modify_orders(BatchModifyOrders::new(
            trader_id,
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            instrument_id,
            modifies,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || async {
            if demo {
                state.amend_order_payloads.lock().await.len() == 2
            } else {
                state.trade_order_payloads.lock().await.len() == 1
            }
        },
        Duration::from_secs(5),
    )
    .await;

    if demo {
        let mut expected_http = vec![
            json!({
                "category": "linear",
                "symbol": "ETHUSDT",
                "orderId": "venue-mod-1",
                "qty": "0.021",
                "price": "1741.23",
            }),
            json!({
                "category": "linear",
                "symbol": "ETHUSDT",
                "orderId": "venue-mod-2",
                "qty": "0.033",
                "price": "1755.50",
                "triggerPrice": "1732.45",
            }),
        ];
        let mut payloads = state.amend_order_payloads.lock().await.clone();
        payloads.sort_by(|left, right| left["orderId"].as_str().cmp(&right["orderId"].as_str()));
        expected_http
            .sort_by(|left, right| left["orderId"].as_str().cmp(&right["orderId"].as_str()));
        assert_eq!(payloads, expected_http);
        assert_eq!(state.trade_order_requests.load(Ordering::Relaxed), 0);
    } else {
        let request = json!({
            "category": "linear",
            "request": [
                {
                    "symbol": "ETHUSDT",
                    "orderId": "venue-mod-1",
                    "orderLinkId": "batch-modify-1",
                    "qty": "0.021",
                    "price": "1741.23",
                },
                {
                    "symbol": "ETHUSDT",
                    "orderId": "venue-mod-2",
                    "orderLinkId": "batch-modify-2",
                    "qty": "0.033",
                    "price": "1755.50",
                    "triggerPrice": "1732.45",
                    "orderIv": "0.42",
                },
            ],
        });
        let payloads = state.trade_order_payloads.lock().await;
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["op"], "order.amend-batch");
        assert!(
            payloads[0]["reqId"]
                .as_str()
                .is_some_and(|req_id| !req_id.is_empty())
        );
        assert_eq!(payloads[0]["args"], json!([request]));
        assert!(state.amend_order_payloads.lock().await.is_empty());
        assert_eq!(state.trade_order_requests.load(Ordering::Relaxed), 1);
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_batch_modify_venue_rejection_uses_amend_correlation() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let trader_id = TraderId::from("TESTER-001");
    let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
    let orders = [
        ("S-001", "batch-reject-1", "venue-reject-1"),
        ("S-002", "batch-reject-2", "venue-reject-2"),
    ];

    for (owner, client_order_id, _) in orders {
        let strategy_id = StrategyId::from(owner);
        let client_order_id = ClientOrderId::from(client_order_id);
        let order = OrderAny::Market(MarketOrder::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Quantity::from("0.01"),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));
        let init = order.init_event().clone();
        cache
            .borrow_mut()
            .add_order(order, None, Some(*BYBIT_CLIENT_ID), false)
            .unwrap();
        client
            .submit_order(SubmitOrder::new(
                trader_id,
                Some(*BYBIT_CLIENT_ID),
                strategy_id,
                instrument_id,
                client_order_id,
                init,
                None,
                None,
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
            ))
            .unwrap();
    }

    wait_until_async(
        || async { state.trade_order_payloads.lock().await.len() == 2 },
        Duration::from_secs(5),
    )
    .await;

    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for OrderSubmitted")
            .expect("channel closed");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
            "expected OrderSubmitted, was {event:?}",
        );
    }

    state.trade_batch_ret_code.store(110001, Ordering::Relaxed);

    let modifies = orders
        .into_iter()
        .map(|(owner, client_order_id, venue_order_id)| {
            ModifyOrder::new(
                trader_id,
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from(owner),
                instrument_id,
                ClientOrderId::from(client_order_id),
                Some(VenueOrderId::from(venue_order_id)),
                Some(Quantity::from("0.02")),
                Some(Price::from("1600.00")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        })
        .collect::<Vec<_>>();

    client
        .batch_modify_orders(BatchModifyOrders::new(
            trader_id,
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            instrument_id,
            modifies.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let mut actual = Vec::new();

    for _ in &modifies {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for OrderModifyRejected")
            .expect("channel closed");

        match event {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) => {
                actual.push((
                    rejected.client_order_id,
                    rejected.strategy_id,
                    rejected.venue_order_id,
                    rejected.reason.to_string(),
                    rejected.causation_id,
                ));
            }
            event => panic!("expected OrderModifyRejected, was {event:?}"),
        }
    }

    actual.sort_by_key(|entry| entry.0);

    let mut expected: Vec<_> = modifies
        .iter()
        .map(|modify| {
            (
                modify.client_order_id,
                modify.strategy_id,
                modify.venue_order_id,
                "Too many visits.".to_string(),
                Some(modify.command_id),
            )
        })
        .collect();

    expected.sort_by_key(|entry| entry.0);
    assert_eq!(actual, expected);

    let payloads = state.trade_order_payloads.lock().await;
    let batch_payloads: Vec<_> = payloads
        .iter()
        .filter(|payload| payload["op"] == "order.amend-batch")
        .collect();
    assert_eq!(batch_payloads.len(), 1);
    assert_eq!(
        batch_payloads[0]["args"][0]["request"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_modify_whole_http_failure_does_not_reject() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_demo_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BYBIT_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE),
        ClientOrderId::from("test-modify-http-ambiguous"),
        Some(VenueOrderId::from("test-order-id-12345")),
        Some(Quantity::from("0.02")),
        Some(Price::from("1600.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.modify_order(cmd).unwrap();

    assert_no_modify_rejected(&mut rx, Duration::from_millis(300)).await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_submit_post_lookup_failure_does_not_reject() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    state.empty_orders_realtime.store(true, Ordering::Relaxed);
    let order_lookup_requests = state.orders_realtime_requests.load(Ordering::Relaxed);

    let cid = ClientOrderId::from("test-unknown-submit-outcome");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let submitted = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderSubmitted")
        .expect("channel closed");
    assert!(
        matches!(submitted, ExecutionEvent::Order(ref event) if event.to_string().contains("OrderSubmitted")),
        "Expected OrderSubmitted, was {submitted:?}",
    );

    wait_until_async(
        || async { state.orders_realtime_requests.load(Ordering::Relaxed) > order_lookup_requests },
        Duration::from_secs(5),
    )
    .await;

    let reject_window = tokio::time::sleep(Duration::from_millis(300));
    tokio::pin!(reject_window);

    loop {
        tokio::select! {
            () = &mut reject_window => break,
            event = rx.recv() => {
                let event = event.expect("channel closed");
                assert!(
                    !matches!(event, ExecutionEvent::Order(ref order_event) if order_event.to_string().contains("OrderRejected")),
                    "Unknown submit outcome must not emit OrderRejected: {event:?}",
                );
            }
        }
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_submit_tp_trigger_price_emits_order_denied() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cid = ClientOrderId::from("test-tp-trigger-denied");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let params = Params::new(
        [
            ("take_profit".to_string(), json!("3000")),
            ("tp_trigger_price".to_string(), json!("2950")),
        ]
        .into(),
    );

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        Some(params),
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    // The demo path cannot carry TP/SL trigger prices, so the first event must be OrderDenied
    // (no OrderSubmitted, no HTTP submission).
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderDenied")
        .expect("channel closed");
    let text = match event {
        ExecutionEvent::Order(ref order_event) => order_event.to_string(),
        other => panic!("Expected OrderDenied, was {other:?}"),
    };
    assert!(
        text.contains("OrderDenied")
            && text.contains("UNSUPPORTED_TP_SL")
            && text.contains("TP/SL trigger prices are not supported in demo mode"),
        "Expected OrderDenied with trigger-price reason, was {text}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_submit_confirmed_rejection_emits_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    state
        .rejected_orders_realtime
        .store(true, Ordering::Relaxed);
    let order_lookup_requests = state.orders_realtime_requests.load(Ordering::Relaxed);

    let cid = ClientOrderId::from("test-confirmed-submit-reject");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let submitted = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderSubmitted")
        .expect("channel closed");
    assert!(
        matches!(submitted, ExecutionEvent::Order(OrderEventAny::Submitted(ref event)) if event.client_order_id == cid),
        "Expected OrderSubmitted for {cid}, was {submitted:?}",
    );

    wait_until_async(
        || async { state.orders_realtime_requests.load(Ordering::Relaxed) > order_lookup_requests },
        Duration::from_secs(5),
    )
    .await;

    let rejected = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderRejected")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Rejected(event)) = rejected else {
        panic!("Expected OrderRejected, was {rejected:?}");
    };

    assert_eq!(event.client_order_id, cid);
    assert_eq!(event.reason.to_string(), "EC_PostOnlyWillTakeLiquidity");
    assert!(!event.reconciliation);
    assert!(event.due_post_only);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_denies_all_on_invalid_leg() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BybitExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Demo,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/v5/private")),
        base_url_ws_trade: Some(format!("ws://{addr}/v5/trade")),
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        account_id: None,
        use_spot_position_reports: false,
        auto_repay_spot_borrows: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
        ..Default::default()
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // Valid market order + unsupported TrailingStopMarket order
    let cid1 = ClientOrderId::from("test-deny-order-1");
    let cid2 = ClientOrderId::from("test-deny-order-2");

    let order1 = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid1,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    let order2 = OrderAny::TrailingStopMarket(TrailingStopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid2,
        OrderSide::Sell,
        Quantity::from("0.01"),
        None,
        Price::from("1500.00"),
        TriggerType::LastPrice,
        rust_decimal::Decimal::new(100, 0),
        TrailingOffsetType::BasisPoints,
        TimeInForce::Gtc,
        None,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));

    let init1 = order1.init_event().clone();
    let init2 = order2.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order1, None, Some(client_id), false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-deny-list-1"),
        instrument_id,
        strategy_id,
        vec![cid1, cid2],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init1, init2],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    // The whole list is denied: the offending TrailingStopMarket leg carries the specific
    // UNSUPPORTED_ORDER_TYPE reason while the valid leg renders ORDER_LIST_DENIED.
    let mut denied = Vec::new();

    for _ in 0..2 {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(ref event)))
                if event.to_string().contains("OrderDenied") =>
            {
                denied.push(event.to_string());
            }
            _ => break,
        }
    }

    assert_eq!(
        denied.len(),
        2,
        "Both orders should be denied when one leg is invalid"
    );

    let offender = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-deny-order-2"))
        .expect("missing denied event for invalid leg");
    assert!(
        offender.contains("UNSUPPORTED_ORDER_TYPE"),
        "offender reason was: {offender}"
    );

    let sibling = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-deny-order-1"))
        .expect("missing denied event for valid leg");
    assert!(
        sibling.contains("ORDER_LIST_DENIED"),
        "sibling reason was: {sibling}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_unsupported_order_type_emits_order_denied() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // Bybit does not support TrailingStopMarket, so the single-order path denies before submit
    let cid = ClientOrderId::from("test-unsupported-order-type");
    let order = OrderAny::TrailingStopMarket(TrailingStopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
        OrderSide::Buy,
        Quantity::from("0.01"),
        None,
        Price::from("1500.00"),
        TriggerType::LastPrice,
        rust_decimal::Decimal::new(100, 0),
        TrailingOffsetType::BasisPoints,
        TimeInForce::Gtc,
        None,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderDenied")
        .expect("channel closed");
    let text = match event {
        ExecutionEvent::Order(ref order_event) => order_event.to_string(),
        other => panic!("Expected OrderDenied, was {other:?}"),
    };
    assert!(
        text.contains("OrderDenied") && text.contains("UNSUPPORTED_ORDER_TYPE"),
        "Expected OrderDenied with unsupported-order-type reason, was {text}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_conditional_order_on_option_denied() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(
        Symbol::from("BTC-25SEP26-86500-C-USDT-OPTION"),
        *BYBIT_VENUE,
    );

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let stop_market = OrderAny::StopMarket(StopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        ClientOrderId::from("test-option-conditional-1"),
        OrderSide::Buy,
        Quantity::from("0.01"),
        Price::from("1500.00"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));

    let stop_limit = OrderAny::StopLimit(StopLimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        ClientOrderId::from("test-option-conditional-2"),
        OrderSide::Buy,
        Quantity::from("0.01"),
        Price::from("1490.00"),
        Price::from("1500.00"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));

    let market_if_touched = OrderAny::MarketIfTouched(MarketIfTouchedOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        ClientOrderId::from("test-option-conditional-3"),
        OrderSide::Sell,
        Quantity::from("0.01"),
        Price::from("1600.00"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));

    let limit_if_touched = OrderAny::LimitIfTouched(LimitIfTouchedOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        ClientOrderId::from("test-option-conditional-4"),
        OrderSide::Sell,
        Quantity::from("0.01"),
        Price::from("1590.00"),
        Price::from("1600.00"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));

    // Bybit does not support conditional orders for options, so each order is
    // denied before submission without an OrderSubmitted event
    for order in [stop_market, stop_limit, market_if_touched, limit_if_touched] {
        let cid = order.client_order_id();
        let init = order.init_event().clone();

        cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();

        let cmd = SubmitOrder::new(
            trader_id,
            Some(client_id),
            strategy_id,
            instrument_id,
            cid,
            init,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None, // correlation_id
        );

        client.submit_order(cmd).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for OrderDenied")
            .expect("channel closed");
        let text = match event {
            ExecutionEvent::Order(ref order_event) => order_event.to_string(),
            other => panic!("Expected OrderDenied, was {other:?}"),
        };
        assert!(
            text.contains("OrderDenied") && text.contains("UNSUPPORTED_ORDER_TYPE"),
            "Expected OrderDenied with unsupported-order-type reason, was {text}",
        );
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_denies_conditional_leg_on_option() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(
        Symbol::from("BTC-25SEP26-86500-C-USDT-OPTION"),
        *BYBIT_VENUE,
    );

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // Valid market leg + StopMarket leg on an option instrument
    let cid1 = ClientOrderId::from("test-option-list-order-1");
    let cid2 = ClientOrderId::from("test-option-list-order-2");

    let order1 = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid1,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let order2 = OrderAny::StopMarket(StopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid2,
        OrderSide::Sell,
        Quantity::from("0.01"),
        Price::from("1400.00"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));

    let init1 = order1.init_event().clone();
    let init2 = order2.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order1, None, Some(client_id), false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-option-list-1"),
        instrument_id,
        strategy_id,
        vec![cid1, cid2],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init1, init2],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    // The whole list is denied: the offending StopMarket leg carries the specific
    // UNSUPPORTED_ORDER_TYPE reason while the valid leg renders ORDER_LIST_DENIED.
    let mut denied = Vec::new();

    for _ in 0..2 {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(ref event)))
                if event.to_string().contains("OrderDenied") =>
            {
                denied.push(event.to_string());
            }
            _ => break,
        }
    }

    assert_eq!(
        denied.len(),
        2,
        "Both orders should be denied when one leg is invalid"
    );

    let offender = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-option-list-order-2"))
        .expect("missing denied event for invalid leg");
    assert!(
        offender.contains("UNSUPPORTED_ORDER_TYPE"),
        "offender reason was: {offender}"
    );

    let sibling = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-option-list-order-1"))
        .expect("missing denied event for valid leg");
    assert!(
        sibling.contains("ORDER_LIST_DENIED"),
        "sibling reason was: {sibling}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_denies_incomplete_when_leg_missing() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // Only the first leg is cached; the second is absent, so the whole list is incomplete
    let cid_present = ClientOrderId::from("test-incomplete-present");
    let cid_missing = ClientOrderId::from("test-incomplete-missing");

    let order_present = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_present,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let order_missing = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_missing,
        OrderSide::Sell,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    let init_present = order_present.init_event().clone();
    let init_missing = order_missing.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order_present, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-incomplete-list"),
        instrument_id,
        strategy_id,
        vec![cid_present, cid_missing],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init_present, init_missing],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    // The missing leg cannot emit (absent from the cache); the cached leg reports the list cause
    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for OrderDenied")
        .expect("channel closed");
    let text = match event {
        ExecutionEvent::Order(ref order_event) => order_event.to_string(),
        other => panic!("Expected OrderDenied, was {other:?}"),
    };
    assert!(
        text.contains("client_order_id=test-incomplete-present")
            && text.contains("ORDER_LIST_INCOMPLETE"),
        "Expected ORDER_LIST_INCOMPLETE for the cached leg, was {text}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_denies_closed_leg() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cid_open = ClientOrderId::from("test-closed-leg-open");
    let cid_closed = ClientOrderId::from("test-closed-leg-closed");

    let order_open = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_open,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    let mut order_closed = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_closed,
        OrderSide::Sell,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    let init_open = order_open.init_event().clone();
    let init_closed = order_closed.init_event().clone();

    // Deny the second leg before submission so it is closed when the list is processed
    order_closed
        .apply(OrderEventAny::Denied(OrderDenied::new(
            trader_id,
            strategy_id,
            instrument_id,
            cid_closed,
            Ustr::from("closed before submission"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        )))
        .unwrap();

    cache
        .borrow_mut()
        .add_order(order_open, None, Some(client_id), false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order_closed, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-closed-leg-list"),
        instrument_id,
        strategy_id,
        vec![cid_open, cid_closed],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init_open, init_closed],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    let mut denied = Vec::new();

    for _ in 0..2 {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(ref event)))
                if event.to_string().contains("OrderDenied") =>
            {
                denied.push(event.to_string());
            }
            _ => break,
        }
    }

    assert_eq!(denied.len(), 2, "Both legs should be denied: {denied:?}");

    let offender = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-closed-leg-closed"))
        .expect("missing denied event for closed leg");
    assert!(
        offender.contains("VALIDATION_FAILED") && offender.contains("cannot submit closed order"),
        "closed-leg reason was: {offender}"
    );

    let sibling = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-closed-leg-open"))
        .expect("missing denied event for open leg");
    assert!(
        sibling.contains("ORDER_LIST_DENIED"),
        "sibling reason was: {sibling}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[case::boolean(json!(true), "was not a string or number")]
#[case::null(json!(null), "was not a string or number")]
#[case::zero(json!(0), "was not a positive value")]
#[case::not_a_number(json!("abc"), "invalid 'amount'")]
#[tokio::test]
async fn test_query_account_repay_action_rejects_invalid_amount(
    #[case] amount: Value,
    #[case] expected_message: &str,
) {
    let (mut client, mut rx, state) = start_connected_client().await;

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    wait_out_repay_blackout().await;

    let params = margin_params(&[
        ("action", json!("REPAY")),
        ("coin", json!("ETH")),
        ("amount", amount),
    ]);
    client.query_account(margin_query_account(params)).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for repay result")
        .expect("channel closed");

    let data = as_margin_data(&event);
    let result = data
        .data
        .as_any()
        .downcast_ref::<BybitMarginRepayResult>()
        .expect("repay result payload");
    assert_eq!(result.coin, "ETH");
    assert!(!result.success);
    assert!(
        result.message.contains(expected_message),
        "was: {}",
        result.message
    );

    assert!(
        state.no_convert_repay_bodies.lock().await.is_empty(),
        "a repay with an invalid amount must not reach the venue or repay all",
    );

    client.disconnect().await.unwrap();
}

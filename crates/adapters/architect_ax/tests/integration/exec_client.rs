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

//! Integration tests for AxExecutionClient.
//!
//! These tests use the mock HTTP+WS server from `common::server` to verify
//! client creation, connection lifecycle, and account state handling.

use std::{cell::RefCell, collections::HashSet, net::SocketAddr, rc::Rc, time::Duration};

use nautilus_architect_ax::{
    common::{
        consts::{AX_CLIENT_ID, AX_VENUE},
        credential::Credential,
        enums::AxEnvironment,
    },
    config::AxExecutionClientConfig,
    execution::AxExecutionClient,
    http::{
        models::{AxFillsResponse, AxInstrumentsResponse, AxOrdersResponse},
        parse::{parse_fill_report, parse_instrument, parse_order_detail_status_report},
    },
};
use nautilus_common::{
    cache::Cache,
    clients::{ExecutionClient, ExecutionReportTask},
    live::runner::{replace_exec_event_sender, replace_system_event_sender, set_exec_event_sender},
    messages::{
        ExecutionEvent, ExecutionReport, SystemEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateFillReportsBuilder, GenerateOrderStatusReport,
            GenerateOrderStatusReportBuilder, GenerateOrderStatusReports,
            GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
            GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{
    ExecutionClientCore, SocketReconnectRegistry, SocketReconnectRequestOutcome,
    manager::{ExecutionManager, ExecutionManagerConfig},
    testing::ExecutionHarness,
};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderCanceled, OrderEventAny, OrderPendingUpdate,
        OrderRejected, OrderSubmitted,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{LimitOrder, Order, OrderAny, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    reports::{ExecutionMassStatus, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal_macros::dec;
use ustr::Ustr;

use crate::common::server::{load_test_data, start_test_server};

fn setup_exec_channel() -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(sender);
    receiver
}

fn create_test_exec_config(addr: SocketAddr) -> AxExecutionClientConfig {
    AxExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
        environment: AxEnvironment::Sandbox,
        base_url_http: Some(format!("http://{addr}")),
        base_url_orders: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/orders/ws")),
        ..Default::default()
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    AxExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("AX-001");
    let client_id = *AX_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *AX_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);
    let rx = setup_exec_channel();
    let client = AxExecutionClient::new(core, config).expect("Failed to create exec client");

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("100000.50 USD"),
            Money::from("0 USD"),
            Money::from("100000.50 USD"),
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

#[rstest]
#[tokio::test]
async fn test_exec_config_creation() {
    let config = AxExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
        environment: AxEnvironment::Sandbox,
        ..Default::default()
    };

    assert_eq!(
        config.api_key.as_ref().map(|value| value.expose_secret()),
        Some("test_api_key")
    );
    assert_eq!(config.environment, AxEnvironment::Sandbox);
    assert_eq!(config.account_id, AccountId::from("AX-001"));
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), *AX_CLIENT_ID);
    assert_eq!(client.venue(), *AX_VENUE);
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert_eq!(client.account_id(), AccountId::from("AX-001"));
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let (addr, _state) = start_test_server().await.unwrap();
    let registry = SocketReconnectRegistry::default();
    let (mut client, _rx, cache) = registry.scope(|| create_test_execution_client(addr));

    // Pre-register account so await_account_registered succeeds
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    assert!(!client.is_connected());

    client.connect().await.expect("Failed to connect");
    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("architect-ax-user-streams");
    let handle = registry.handle(*AX_CLIENT_ID, endpoint).unwrap();

    assert!(client.is_connected());
    assert_eq!(change.client_id, *AX_CLIENT_ID);
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);

    client.disconnect().await.expect("Failed to disconnect");
    assert!(!client.is_connected());
    assert!(registry.handle(*AX_CLIENT_ID, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_aborts_when_fee_lookup_fails() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    state
        .whoami_fail
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let error = client.connect().await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "failed to resolve AX account fee rates",
        "connect must fail rather than cache zero-fee instruments"
    );
    assert!(!client.is_connected());

    // The aborted load must leave the client retryable, so a later connect resolves real rates
    // instead of keeping instruments that were never built.
    let attempts_before = state
        .whoami_count
        .load(std::sync::atomic::Ordering::Relaxed);
    state
        .whoami_fail
        .store(false, std::sync::atomic::Ordering::Relaxed);

    client.connect().await.expect("Failed to reconnect");

    assert!(client.is_connected());
    assert_eq!(
        state
            .whoami_count
            .load(std::sync::atomic::Ordering::Relaxed),
        attempts_before + 1,
        "the retry must re-resolve fees rather than skip an already-initialized load"
    );
    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_exec_client_refreshes_fees_on_reconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.connect().await.unwrap();
    client.disconnect().await.unwrap();
    state
        .whoami_fail
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let error = client.connect().await.unwrap_err();

    assert_eq!(error.to_string(), "failed to resolve AX account fee rates");
    assert!(!client.is_connected());
}

#[rstest]
#[case::maker(false, false, OrderType::Limit, None, OrderSide::Buy, "1000.00 USD")]
#[case::taker(true, false, OrderType::Limit, None, OrderSide::Buy, "12500.00 USD")]
#[case::maker_reconnected(false, true, OrderType::Limit, None, OrderSide::Buy, "3000.00 USD")]
#[case::taker_reconnected(true, true, OrderType::Limit, None, OrderSide::Buy, "6500.00 USD")]
#[case::market(true, false, OrderType::Market, None, OrderSide::Buy, "12500.00 USD")]
#[case::released_limit(
    false,
    false,
    OrderType::Limit,
    Some(OrderType::StopLimit),
    OrderSide::Buy,
    "1000.00 USD"
)]
#[case::released_market(
    true,
    false,
    OrderType::Market,
    Some(OrderType::StopMarket),
    OrderSide::Buy,
    "12500.00 USD"
)]
#[case::maker_opposite_execution_side(
    false,
    false,
    OrderType::Limit,
    None,
    OrderSide::Sell,
    "1000.00 USD"
)]
#[tokio::test]
async fn test_exec_client_stream_fill_uses_account_fee_rates(
    #[case] agg: bool,
    #[case] reconnect: bool,
    #[case] order_type: OrderType,
    #[case] historical_type: Option<OrderType>,
    #[case] execution_side: OrderSide,
    #[case] expected: &str,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    let mut fill = load_test_data("ws_order_filled.json");
    fill["o"]["aid"] = serde_json::json!("01JBXR-7QK2-0000");
    fill["xs"]["aid"] = serde_json::json!("01JBXR-7QK2-0000");
    fill["xs"]["agg"] = serde_json::json!(agg);
    fill["xs"]["d"] = serde_json::json!(if execution_side == OrderSide::Buy {
        "B"
    } else {
        "S"
    });
    *state.order_fill_payload.lock().await = Some(fill);
    client.start().unwrap();
    client.connect().await.unwrap();
    if reconnect {
        client.disconnect().await.unwrap();
        let mut whoami = load_test_data("http_get_whoami.json");
        whoami["accounts"][0]["maker_fee"] = serde_json::json!("0.0006");
        whoami["accounts"][0]["taker_fee"] = serde_json::json!("0.0013");
        *state.whoami_payload.lock().await = Some(whoami);
        client.connect().await.unwrap();
    }

    drain_rx(&mut rx);
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-COMMISSION");
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .price(Price::from("50000.0000"))
        .time_in_force(TimeInForce::Gtc);
    let mut order = builder.build();

    if let Some(historical_type) = historical_type {
        let historical = OrderTestBuilder::new(historical_type)
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .price(Price::from("50000.0000"))
            .trigger_price(Price::from("49999.0000"))
            .build();
        let history = historical.events().into_iter().cloned();
        match &mut order {
            OrderAny::Limit(order) => order.prepend_events(history),
            OrderAny::Market(order) => order.prepend_events(history),
            _ => unreachable!(),
        }

        assert_eq!(order.init_event().order_type, historical_type);
    }

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    let fill = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(ExecutionEvent::Order(OrderEventAny::Filled(fill))) = rx.recv().await {
                break fill;
            }
        }
    })
    .await
    .expect("stream fill should arrive");

    client.disconnect().await.unwrap();

    assert_eq!(fill.order_type, order_type);
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.commission, Some(Money::from(expected)));
    assert_eq!(
        state
            .whoami_count
            .load(std::sync::atomic::Ordering::Relaxed),
        if reconnect { 2 } else { 1 }
    );
    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.instrument_id, instrument_id);
    assert_eq!(fill.account_id, AccountId::from("AX-001"));
    assert_eq!(fill.last_qty.as_decimal(), dec!(100));
    assert_eq!(fill.last_px.as_decimal(), dec!(50000));
    assert_eq!(fill.trade_id.as_str(), "T-01ARZ3NDEKTSV4RRFFQ69G5FAV");
}

#[rstest]
#[case::submitted(OrderStatus::Submitted, false, "EURUSD-PERP", false, None)]
#[case::accepted(OrderStatus::Accepted, false, "EURUSD-PERP", false, None)]
#[case::reconnected_submitted(OrderStatus::Submitted, true, "EURUSD-PERP", false, None)]
#[case::reconnected_accepted(OrderStatus::Accepted, true, "EURUSD-PERP", false, None)]
#[case::uncataloged(OrderStatus::Submitted, false, "RETIRED-PERP", false, None)]
#[case::persisted_instrument(OrderStatus::Submitted, false, "RETIRED-PERP", true, None)]
#[case::released_market(
    OrderStatus::Submitted,
    false,
    "EURUSD-PERP",
    false,
    Some(OrderType::StopMarket)
)]
#[tokio::test]
async fn test_connect_restores_order_before_first_stream_fill(
    #[case] status: OrderStatus,
    #[case] reconnect: bool,
    #[case] symbol: &str,
    #[case] persisted: bool,
    #[case] historical_type: Option<OrderType>,
) {
    let accepted = status == OrderStatus::Accepted;
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    let account_id = AccountId::from("AX-001");
    add_test_account_to_cache(&cache, account_id);
    let instrument_id = InstrumentId::from(format!("{symbol}.AX"));
    let mut instrument = load_test_data("http_get_instruments.json")["instruments"][0].clone();
    instrument["symbol"] = serde_json::json!(symbol);
    instrument["state"] = serde_json::json!("DELISTED");
    if persisted {
        let instrument = serde_json::from_value(instrument.clone()).unwrap();
        let instrument =
            parse_instrument(&instrument, UnixNanos::from(1), UnixNanos::from(2)).unwrap();
        cache.borrow_mut().add_instrument(instrument).unwrap();
    }

    *state.instrument_payload.lock().await = Some(instrument);
    let client_order_id = ClientOrderId::from("O-RESTORED-MARKET");
    let venue_order_id = VenueOrderId::from("OID-RESTORED-MARKET");
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-RESTORED"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .time_in_force(TimeInForce::Ioc)
        .build();

    if let Some(historical_type) = historical_type {
        let historical = OrderTestBuilder::new(historical_type)
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .trigger_price(Price::from("49999.0000"))
            .build();

        let OrderAny::Market(order) = &mut order else {
            unreachable!()
        };

        order.prepend_events(historical.events().into_iter().cloned());
    }

    let order_init = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    let submitted = OrderSubmitted::new(
        order_init.trader_id,
        order_init.strategy_id,
        instrument_id,
        client_order_id,
        account_id,
        UUID4::new(),
        UnixNanos::from(11),
        UnixNanos::from(12),
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Submitted(submitted))
        .unwrap();

    if accepted {
        let event = OrderAccepted::new(
            order_init.trader_id,
            order_init.strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            UUID4::new(),
            UnixNanos::from(13),
            UnixNanos::from(14),
            false,
        );
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::Accepted(event))
            .unwrap();
    }

    let mut fill = load_test_data("ws_order_filled.json");
    fill["o"]["s"] = serde_json::json!(symbol);
    fill["xs"]["s"] = serde_json::json!(symbol);
    fill["o"]["oid"] = serde_json::json!(venue_order_id.as_str());
    fill["o"]["cid"] = serde_json::json!(
        nautilus_architect_ax::common::parse::client_order_id_to_cid(&client_order_id)
    );
    fill["o"]["aid"] = serde_json::json!("01JBXR-7QK2-0000");
    fill["xs"]["aid"] = serde_json::json!("01JBXR-7QK2-0000");

    if reconnect {
        client.start().unwrap();
        client.connect().await.unwrap();
        client.disconnect().await.unwrap();
        drain_rx(&mut rx);
    }

    state.order_initial_events.lock().await.push(fill);
    client.start().unwrap();
    client.connect().await.unwrap();
    let mut order_events = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(ExecutionEvent::Order(event)) = rx.recv().await {
                let complete = matches!(event, OrderEventAny::Filled(_));
                order_events.push(event);

                if complete {
                    break;
                }
            }
        }
    })
    .await
    .expect("restored order must receive tracked fill");

    client.disconnect().await.unwrap();

    let fill = match order_events.last().unwrap() {
        OrderEventAny::Filled(fill) => fill,
        _ => panic!("expected fill"),
    };

    assert_eq!(
        state.instrument_queries.lock().await.len(),
        usize::from(symbol == "RETIRED-PERP" && !persisted)
    );
    assert_eq!(order_events.len(), if accepted { 1 } else { 2 });

    if !accepted {
        let OrderEventAny::Accepted(event) = &order_events[0] else {
            panic!("expected acceptance first");
        };

        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.venue_order_id, venue_order_id);
    }

    assert_eq!(fill.trader_id, order_init.trader_id);
    assert_eq!(fill.strategy_id, order_init.strategy_id);
    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.instrument_id, instrument_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.account_id, account_id);
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.order_type, OrderType::Market);
    assert_eq!(fill.last_qty.as_decimal(), dec!(100));
    assert_eq!(fill.last_px.as_decimal(), dec!(50000));
}

#[rstest]
#[tokio::test]
async fn test_exec_client_emits_account_state_on_connect() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    // start() wires the emitter's sender so events reach the channel
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");

    // The connect flow calls request_account_state and emits via the channel
    let mut found_account = false;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, ExecutionEvent::Account(_)) {
            found_account = true;
            break;
        }
    }

    assert!(found_account, "Expected account state event on connect");
    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_exec_client_get_account_returns_cached() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, cache) = create_test_execution_client(addr);

    assert!(client.get_account().is_none());

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    let account = client.get_account();
    assert!(account.is_some());
}

#[rstest]
#[tokio::test]
async fn test_exec_config_url_overrides() {
    let config = AxExecutionClientConfig {
        base_url_http: Some("http://custom:1234".to_string()),
        base_url_orders: Some("http://custom:5678".to_string()),
        base_url_ws_private: Some("ws://custom:9012/ws".to_string()),
        ..Default::default()
    };

    assert_eq!(config.http_base_url(), "http://custom:1234");
    assert_eq!(config.orders_base_url(), "http://custom:5678");
    assert_eq!(config.ws_private_url(), "ws://custom:9012/ws");
}

#[rstest]
#[tokio::test]
async fn test_exec_config_sandbox_defaults() {
    let config = AxExecutionClientConfig {
        environment: AxEnvironment::Sandbox,
        ..Default::default()
    };

    assert!(config.http_base_url().contains("sandbox"));
    assert!(config.orders_base_url().contains("sandbox"));
    assert!(config.ws_private_url().contains("sandbox"));
}

#[rstest]
#[tokio::test]
async fn test_exec_config_production_defaults() {
    let config = AxExecutionClientConfig {
        environment: AxEnvironment::Production,
        ..Default::default()
    };

    assert!(!config.http_base_url().contains("sandbox"));
    assert!(!config.orders_base_url().contains("sandbox"));
    assert!(!config.ws_private_url().contains("sandbox"));
}

#[rstest]
#[tokio::test]
async fn test_query_account_does_not_block_within_runtime() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");

    // Drain any events from connect
    while rx.try_recv().is_ok() {}

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        AccountId::from("AX-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client
        .query_account(cmd)
        .expect("query_account should not panic with nested runtime");

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("Timed out waiting for account event")
        .expect("Channel closed without event");

    assert!(
        matches!(event, ExecutionEvent::Account(_)),
        "Expected Account event, was {event:?}"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

fn add_open_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: &str,
    venue_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    strategy_id: &str,
) {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from(strategy_id);
    let coid = ClientOrderId::from(client_order_id);

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        coid,
        side,
        Quantity::from("1"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,  // expire_time
        false, // post_only
        false, // reduce_only
        false, // quote_quantity
        None,  // display_qty
        None,  // emulation_trigger
        None,  // trigger_instrument_id
        None,  // contingency_type
        None,  // order_list_id
        None,  // linked_order_ids
        None,  // parent_order_id
        None,  // exec_algorithm_id
        None,  // exec_algorithm_params
        None,  // exec_spawn_id
        None,  // tags
        UUID4::new(),
        UnixNanos::default(),
    );

    let order_any: OrderAny = order.into();

    let accepted = OrderAccepted::new(
        trader_id,
        strategy_id,
        instrument_id,
        coid,
        VenueOrderId::new(venue_order_id),
        AccountId::from("AX-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );

    cache
        .borrow_mut()
        .add_order(order_any, None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Accepted(accepted))
        .unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_uses_http_endpoint() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    add_open_order_to_cache(
        &cache,
        "O-001",
        "VOI-001",
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );
    add_open_order_to_cache(
        &cache,
        "O-002",
        "VOI-002",
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );

    let cmd = CancelAllOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(*AX_CLIENT_ID),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        order_side: None,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    client
        .cancel_all_orders(cmd)
        .expect("cancel_all_orders should not error");

    // Allow spawned task to complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    assert_eq!(
        state
            .cancel_all_count
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "Expected HTTP cancel_all_orders endpoint to be called once"
    );

    let messages = state.get_messages().await;
    let ws_cancel_count = messages
        .iter()
        .filter(|m| m.get("t").and_then(|v| v.as_str()) == Some("c"))
        .count();
    assert_eq!(ws_cancel_count, 0);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[case::buy(OrderSide::Buy, vec!["VOI-BUY-1", "VOI-BUY-2"])]
#[case::sell(OrderSide::Sell, vec!["VOI-SELL-1"])]
#[tokio::test]
async fn test_cancel_all_orders_with_side_cancels_matching_side_via_batch(
    #[case] order_side: OrderSide,
    #[case] expected_venue_order_ids: Vec<&str>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let other_instrument_id = InstrumentId::from("BTC-PERP.AX");

    add_open_order_to_cache(
        &cache,
        "O-BUY-1",
        "VOI-BUY-1",
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );
    add_open_order_to_cache(
        &cache,
        "O-BUY-2",
        "VOI-BUY-2",
        instrument_id,
        OrderSide::Buy,
        "S-002",
    );
    add_open_order_to_cache(
        &cache,
        "O-SELL-1",
        "VOI-SELL-1",
        instrument_id,
        OrderSide::Sell,
        "S-001",
    );
    add_open_order_to_cache(
        &cache,
        "O-OTHER",
        "VOI-OTHER",
        other_instrument_id,
        OrderSide::Buy,
        "S-001",
    );

    let cmd = CancelAllOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(*AX_CLIENT_ID),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        order_side: Some(order_side),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    client
        .cancel_all_orders(cmd)
        .expect("cancel_all_orders should not error");

    // Allow spawned fan-out tasks to complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    assert_eq!(
        state
            .cancel_all_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "Sided cancel-all must not hit the venue-wide HTTP cancel-all endpoint"
    );

    let messages = state.get_messages().await;
    let mut canceled_oids: Vec<String> = messages
        .iter()
        .filter(|m| m.get("t").and_then(|v| v.as_str()) == Some("x"))
        .filter_map(|m| m.get("oid").and_then(|v| v.as_str()))
        .map(str::to_string)
        .collect();

    canceled_oids.sort();
    let mut expected_oids: Vec<String> = expected_venue_order_ids
        .into_iter()
        .map(str::to_string)
        .collect();
    expected_oids.sort();

    assert_eq!(
        canceled_oids, expected_oids,
        "Only matching-side open orders on the instrument should be canceled"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_with_side_and_empty_cache_sends_nothing() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    // Open order on another instrument only; the sided cancel-all for
    // `instrument_id` must not touch it.
    let other_instrument_id = InstrumentId::from("BTC-PERP.AX");
    add_open_order_to_cache(
        &cache,
        "O-OTHER",
        "VOI-OTHER",
        other_instrument_id,
        OrderSide::Buy,
        "S-001",
    );

    let cmd = CancelAllOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(*AX_CLIENT_ID),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        order_side: Some(OrderSide::Buy),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    client
        .cancel_all_orders(cmd)
        .expect("cancel_all_orders should not error");

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    assert_eq!(
        state
            .cancel_all_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "Empty sided cancel-all must not hit the HTTP cancel-all endpoint"
    );

    let messages = state.get_messages().await;
    let ws_cancel_count = messages
        .iter()
        .filter(|m| m.get("t").and_then(|v| v.as_str()) == Some("x"))
        .count();
    assert_eq!(ws_cancel_count, 0, "No WS cancels should be sent");

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[case::short(Some(60), 60 * 60 * 1_000_000_000)]
#[case::long(Some(60 * 24 * 30), 7 * 24 * 60 * 60 * 1_000_000_000)]
#[case::default(None, 7 * 24 * 60 * 60 * 1_000_000_000)]
#[case::zero(Some(0), 1)]
#[tokio::test]
async fn test_generate_mass_status_declares_bounded_lookback_window(
    #[case] lookback_mins: Option<u64>,
    #[case] expected_span_ns: u64,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({ "orders": [] }));
    *state.orders_payload.lock().await = Some(serde_json::json!({ "orders": [] }));
    *state.fills_payload.lock().await = Some(serde_json::json!({ "fills": [] }));
    *state.positions_payload.lock().await = Some(serde_json::json!({ "positions": [] }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let mass_status = client
        .generate_mass_status(lookback_mins)
        .await
        .unwrap()
        .expect("mass status");
    let expected_start = mass_status.ts_init.as_u64() - expected_span_ns;

    assert_eq!(
        mass_status.lookback_start(),
        Some(UnixNanos::from(expected_start))
    );
    assert!(mass_status.reports_complete());
    let queries = state.fills_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].start_timestamp_ns, expected_start as i64);
    assert_eq!(queries[0].end_timestamp_ns, mass_status.ts_init.as_i64());

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[case::long(OrderSide::Buy, OrderSide::Sell)]
#[case::short(OrderSide::Sell, OrderSide::Buy)]
#[tokio::test]
async fn test_generate_mass_status_recovers_cached_position_closed_offline(
    #[case] side_open: OrderSide,
    #[case] side_close: OrderSide,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let instruments: AxInstrumentsResponse =
        serde_json::from_value(load_test_data("http_get_instruments.json")).unwrap();
    let instrument = parse_instrument(
        &instruments.instruments[0],
        UnixNanos::default(),
        UnixNanos::default(),
    )
    .unwrap();
    let instrument_id = instrument.id();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("AX-001");
    let strategy_id = StrategyId::from("S-001");
    let harness = ExecutionHarness::new(trader_id, *AX_CLIENT_ID, account_id, instrument.clone());
    add_test_account_to_cache(harness.cache(), account_id);
    let position = add_retained_position(
        harness.cache(),
        &instrument,
        account_id,
        strategy_id,
        side_open,
    );
    let position_id = position.id;
    let closing = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-CLOSE"))
        .side(side_close)
        .quantity(Quantity::from("100"))
        .price(Price::from("1.0845"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    let venue_order_id = VenueOrderId::from("O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let accepted = TestOrderEventStubs::accepted(&closing, account_id, venue_order_id);
    harness
        .cache()
        .borrow_mut()
        .add_order(closing, Some(position_id), Some(*AX_CLIENT_ID), false)
        .unwrap();
    harness
        .cache()
        .borrow_mut()
        .update_order(&accepted)
        .unwrap();
    harness
        .exec_engine()
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);
    assert_eq!(
        harness
            .cache()
            .borrow()
            .position(&position_id)
            .unwrap()
            .quantity,
        Quantity::from("100")
    );

    let now = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .as_u64()
        - 1_000_000_000;

    let direction = if side_close == OrderSide::Buy {
        "B"
    } else {
        "S"
    };

    let mut orders = load_test_data("http_get_orders.json");
    orders["orders"].as_array_mut().unwrap().truncate(1);
    let order = &mut orders["orders"][0];
    order["ts"] = serde_json::json!(now / 1_000_000_000);
    order["tn"] = serde_json::json!(now % 1_000_000_000);
    order["o"] = serde_json::json!("FILLED");
    order["d"] = serde_json::json!(direction);
    order["p"] = serde_json::json!("1.0845");
    order["q"] = serde_json::json!(100);
    order["xq"] = serde_json::json!(100);
    order["rq"] = serde_json::json!(0);
    order["cid"] = serde_json::Value::Null;
    let mut fills = load_test_data("http_get_fills.json");
    fills["fills"].as_array_mut().unwrap().truncate(1);
    fills["fills"][0]["quantity"] = serde_json::json!(100);
    fills["fills"][0]["side"] = serde_json::json!(direction);
    fills["fills"][0]["timestamp"] = serde_json::json!(
        jiff::Timestamp::from_nanosecond(i128::from(now))
            .unwrap()
            .to_string()
    );
    *state.open_orders_payload.lock().await = Some(serde_json::json!({"orders": []}));
    *state.orders_payload.lock().await = Some(orders);
    *state.fills_payload.lock().await = Some(fills);
    *state.positions_payload.lock().await = Some(serde_json::json!({"positions": []}));

    let core = ExecutionClientCore::new(
        trader_id,
        *AX_CLIENT_ID,
        *AX_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        harness.cache().clone(),
    );
    let mut client = AxExecutionClient::new(core, create_test_exec_config(addr)).unwrap();
    client.connect().await.unwrap();
    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    client.disconnect().await.unwrap();
    harness.register_client(Box::new(client)).unwrap();
    let mut manager = ExecutionManager::new(
        harness.clock().clone(),
        harness.cache().clone(),
        ExecutionManagerConfig::default(),
    )
    .unwrap();
    let result = manager.reconcile_execution_mass_status(&mass_status, harness.exec_engine());
    let cache = harness.cache().borrow();
    let recovered = cache.position(&position_id).unwrap();

    assert!(result.unresolved_positions.is_empty());
    assert!(recovered.is_closed());
    assert_eq!(recovered.quantity, Quantity::zero(0));
    assert_eq!(recovered.strategy_id, strategy_id);
    assert_eq!(
        cache
            .order(&ClientOrderId::from("O-CLOSE"))
            .unwrap()
            .status(),
        OrderStatus::Filled
    );
    assert_eq!(
        cache
            .positions_open(None, None, None, Some(&account_id), None)
            .len(),
        0
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_scopes_cached_flat_reports() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({"orders": []}));
    *state.orders_payload.lock().await = Some(serde_json::json!({"orders": []}));
    *state.fills_payload.lock().await = Some(serde_json::json!({"fills": []}));
    *state.positions_payload.lock().await = Some(serde_json::json!({"positions": []}));
    let instruments: AxInstrumentsResponse =
        serde_json::from_value(load_test_data("http_get_instruments.json")).unwrap();
    let instrument = parse_instrument(
        &instruments.instruments[0],
        UnixNanos::default(),
        UnixNanos::default(),
    )
    .unwrap();
    let instrument_other_account = parse_instrument(
        &instruments.instruments[1],
        UnixNanos::default(),
        UnixNanos::default(),
    )
    .unwrap();
    let instrument_second = parse_instrument(
        &instruments.instruments[2],
        UnixNanos::default(),
        UnixNanos::default(),
    )
    .unwrap();
    let foreign_instrument: InstrumentAny = crypto_perpetual_ethusdt().into();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    let account_id = AccountId::from("AX-001");
    add_test_account_to_cache(&cache, account_id);
    let retained_second = add_retained_position(
        &cache,
        &instrument_second,
        account_id,
        StrategyId::from("S-004"),
        OrderSide::Sell,
    );
    let retained = add_retained_position(
        &cache,
        &instrument,
        account_id,
        StrategyId::from("S-001"),
        OrderSide::Buy,
    );
    let other_account = add_retained_position(
        &cache,
        &instrument_other_account,
        AccountId::from("AX-002"),
        StrategyId::from("S-002"),
        OrderSide::Sell,
    );
    let other_venue = add_retained_position(
        &cache,
        &foreign_instrument,
        account_id,
        StrategyId::from("S-003"),
        OrderSide::Buy,
    );
    client.connect().await.unwrap();

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    client.disconnect().await.unwrap();
    let positions = mass_status.position_reports();
    assert_eq!(
        positions.keys().copied().collect::<Vec<_>>(),
        vec![instrument.id(), instrument_second.id()]
    );

    for retained in [&retained, &retained_second] {
        let reports = &positions[&retained.instrument_id];
        let report = &reports[0];

        let expected = PositionStatusReport::new(
            account_id,
            retained.instrument_id,
            PositionSide::Flat,
            Quantity::zero(retained.quantity.precision),
            mass_status.ts_init,
            mass_status.ts_init,
            Some(report.report_id),
            None,
            None,
        );

        assert_eq!(reports.len(), 1);
        assert_eq!(*report, expected);
    }

    assert_eq!(
        cache.borrow().position(&other_account.id).unwrap().quantity,
        other_account.quantity
    );
    assert_eq!(
        cache.borrow().position(&other_venue.id).unwrap().quantity,
        other_venue.quantity
    );
}

fn add_retained_position(
    cache: &Rc<RefCell<Cache>>,
    instrument: &InstrumentAny,
    account_id: AccountId,
    strategy_id: StrategyId,
    side: OrderSide,
) -> Position {
    let position_id = PositionId::from(format!("{}-{strategy_id}", instrument.id()));
    let quantity = Quantity::from_decimal_dp(dec!(100), instrument.size_precision()).unwrap();
    let price = Price::from_decimal_dp(dec!(1), instrument.price_precision()).unwrap();
    let opening = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from(format!("OPEN-{strategy_id}")))
        .side(side)
        .quantity(quantity)
        .build();
    let fill = TestOrderEventStubs::filled(
        &opening,
        instrument,
        Some(TradeId::from(format!("T-OPEN-{strategy_id}"))),
        Some(position_id),
        Some(price),
        Some(quantity),
        None,
        Some(Money::from("0.02 USD")),
        Some(UnixNanos::from(1_000_000_000)),
        Some(account_id),
    );
    let position = Position::new(instrument, fill.clone().into());
    let accepted = TestOrderEventStubs::accepted(
        &opening,
        account_id,
        VenueOrderId::from(format!("V-OPEN-{strategy_id}")),
    );
    let mut cache = cache.borrow_mut();
    cache.add_position(&position, OmsType::Netting).unwrap();
    cache
        .add_order(opening, None, Some(*AX_CLIENT_ID), false)
        .unwrap();
    cache.update_order(&accepted).unwrap();
    cache.update_order(&fill).unwrap();
    position
}

#[rstest]
#[case::invalid_qty("quantity")]
#[case::malformed_row("malformed")]
#[tokio::test]
async fn test_generate_mass_status_rejects_invalid_position_snapshot(#[case] invalid: &str) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.connect().await.unwrap();
    drain_rx(&mut rx);
    let mut positions = load_test_data("http_get_positions.json");
    positions["positions"][0]["symbol"] = serde_json::json!("EURUSD-PERP");
    positions["positions"][1]["symbol"] = serde_json::json!("XAU-PERP");
    if invalid == "quantity" {
        positions["positions"][1]["signed_quantity"] = serde_json::json!(i64::MAX);
    } else {
        positions["positions"][1]["signed_quantity"] = serde_json::json!("invalid");
    }

    *state.positions_payload.lock().await = Some(positions);

    let result = client.generate_mass_status(None).await;
    client.disconnect().await.unwrap();
    let error = format!("{:#}", result.unwrap_err());

    if invalid == "quantity" {
        assert!(
            error.contains("failed to parse AX position for XAU-PERP"),
            "{error}"
        );
    } else {
        assert!(error.contains("invalid type"), "{error}");
    }
}

#[rstest]
#[case::block_trade(false)]
#[case::final_settlement(true)]
#[tokio::test]
async fn test_generate_mass_status_gives_special_fills_order_evidence(
    #[case] is_final_settlement: bool,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({ "orders": [] }));
    *state.orders_payload.lock().await = Some(serde_json::json!({ "orders": [] }));
    *state.positions_payload.lock().await = Some(serde_json::json!({ "positions": [] }));
    let mut fills = load_test_data("http_get_fills.json");
    fills["fills"].as_array_mut().unwrap().truncate(1);
    fills["total_count"] = serde_json::json!(1);
    let fill = &mut fills["fills"][0];
    fill["order_id"] = serde_json::Value::Null;
    fill["is_block_trade"] = serde_json::json!(true);
    fill["is_final_settlement"] = serde_json::json!(is_final_settlement);
    *state.fills_payload.lock().await = Some(fills);

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    drain_rx(&mut rx);

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let repeated = client.generate_mass_status(None).await.unwrap().unwrap();
    let order_reports = mass_status.order_reports();
    let fill_reports = mass_status.fill_reports();
    let venue_order_id = VenueOrderId::from("AX-FILL-T-01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let order = order_reports
        .get(&venue_order_id)
        .expect("special-fill order");
    let fills = fill_reports.get(&venue_order_id).expect("special fill");
    let fill = &fills[0];
    let expected = OrderStatusReport::new(
        AccountId::from("AX-001"),
        InstrumentId::from("EURUSD-PERP.AX"),
        None,
        venue_order_id,
        Some(OrderSide::Buy),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("500"),
        Quantity::from("500"),
        UnixNanos::from(1_705_314_645_000_000_000_u64),
        UnixNanos::from(1_705_314_645_000_000_000_u64),
        fill.ts_init,
        Some(order.report_id),
    )
    .with_avg_px(dec!(1.08450));

    assert_eq!(order_reports.len(), 1);
    assert_eq!(fill_reports.len(), 1);
    assert_eq!(fills.len(), 1);
    assert_eq!(
        repeated.order_reports().keys().collect::<HashSet<_>>(),
        order_reports.keys().collect::<HashSet<_>>()
    );
    assert_eq!(
        repeated.fill_reports()[&venue_order_id][0].trade_id,
        fill.trade_id
    );
    assert_eq!(*order, expected);
    assert_eq!(fill.trade_id, TradeId::from("T-01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    assert_eq!(fill.last_qty, Quantity::from("500"));
    assert_eq!(fill.last_px, Price::from("1.08450"));
    assert_eq!(
        mass_status.lookback_start(),
        Some(UnixNanos::from(
            mass_status.ts_init.as_u64() - 7 * 24 * 60 * 60 * 1_000_000_000,
        ))
    );
    assert!(mass_status.reports_complete());
    assert!(mass_status.position_reports().is_empty());

    client.disconnect().await.unwrap();
}

#[rstest]
#[case::default(None, 7 * 24 * 60 * 60 * 1_000_000_000)]
#[case::zero(Some(0), 1)]
#[tokio::test]
#[ignore = "requires AX sandbox credentials; reads live account reports"]
async fn test_live_mass_status_declares_fill_window_sandbox(
    #[case] lookback_mins: Option<u64>,
    #[case] expected_span_ns: u64,
) {
    let credential = Credential::resolve(None, None).expect("AX sandbox credentials");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let account_id = AccountId::from("AX-001");
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        TraderId::from("TESTER-001"),
        *AX_CLIENT_ID,
        *AX_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );

    let config = AxExecutionClientConfig {
        api_key: Some(credential.api_key().to_owned().into()),
        api_secret: Some(credential.api_secret().to_owned().into()),
        environment: AxEnvironment::Sandbox,
        ..Default::default()
    };

    let _rx = setup_exec_channel();
    let mut client = AxExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    let active_command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(now)
        .open_only(true)
        .build()
        .unwrap();
    let active = client
        .generate_order_status_reports(&active_command)
        .await
        .unwrap();
    let history_command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(now)
        .open_only(false)
        .start(Some(UnixNanos::from(now.as_u64() - 60_000_000_000)))
        .end(Some(now))
        .build()
        .unwrap();
    let bounded = client
        .generate_order_status_reports(&history_command)
        .await
        .unwrap();

    for report in active {
        assert_eq!(
            bounded
                .iter()
                .filter(|row| row.venue_order_id == report.venue_order_id)
                .count(),
            1
        );
    }

    let result = client.generate_mass_status(lookback_mins).await;
    client.disconnect().await.unwrap();
    let mass_status = result.unwrap().unwrap();

    assert_eq!(
        mass_status.lookback_start(),
        Some(UnixNanos::from(
            mass_status.ts_init.as_u64() - expected_span_ns,
        ))
    );
    assert!(mass_status.reports_complete());
    let orders = mass_status.order_reports();
    for (venue_order_id, fills) in mass_status.fill_reports() {
        let order = orders
            .get(&venue_order_id)
            .expect("live fills have order evidence");
        assert_eq!(order.account_id, account_id);

        for fill in fills {
            assert_eq!(fill.account_id, account_id);
            assert_eq!(fill.instrument_id, order.instrument_id);
            assert_eq!(fill.venue_order_id, order.venue_order_id);
        }
    }
}

#[rstest]
#[tokio::test]
#[ignore = "requires AX sandbox credentials and a flat EURUSD position; reads live reports"]
async fn test_live_mass_status_recovers_cached_flat_position_sandbox() {
    let credential = Credential::resolve(None, None).expect("AX sandbox credentials");
    let instruments: AxInstrumentsResponse =
        serde_json::from_value(load_test_data("http_get_instruments.json")).unwrap();
    let instrument = parse_instrument(
        &instruments.instruments[0],
        UnixNanos::default(),
        UnixNanos::default(),
    )
    .unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("AX-001");
    let strategy_id = StrategyId::external();
    let harness = ExecutionHarness::new(trader_id, *AX_CLIENT_ID, account_id, instrument.clone());
    add_test_account_to_cache(harness.cache(), account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        *AX_CLIENT_ID,
        *AX_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        harness.cache().clone(),
    );

    let config = AxExecutionClientConfig {
        api_key: Some(credential.api_key().to_owned().into()),
        api_secret: Some(credential.api_secret().to_owned().into()),
        environment: AxEnvironment::Sandbox,
        ..Default::default()
    };

    let mut client = AxExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();

    let cmd = GeneratePositionStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(instrument.id()),
        None,
        None,
        None,
        None,
    );
    let live_positions = client.generate_position_status_reports(&cmd).await.unwrap();
    assert!(
        live_positions
            .iter()
            .all(|report| report.quantity.is_zero()),
        "Sandbox EURUSD must be flat for this read-only recovery check"
    );
    let instrument = harness
        .cache()
        .borrow()
        .instrument(&instrument.id())
        .unwrap()
        .clone();
    let retained = add_retained_position(
        harness.cache(),
        &instrument,
        account_id,
        strategy_id,
        OrderSide::Buy,
    );
    let result = client.generate_mass_status(Some(0)).await;
    client.disconnect().await.unwrap();
    let mass_status = result.unwrap().unwrap();
    let positions = mass_status.position_reports();
    let flat = &positions[&instrument.id()][0];
    assert_eq!(flat.position_side, PositionSide::Flat);
    assert_eq!(flat.quantity, Quantity::zero(instrument.size_precision()));
    harness.register_client(Box::new(client)).unwrap();
    let mut manager = ExecutionManager::new(
        harness.clock().clone(),
        harness.cache().clone(),
        ExecutionManagerConfig::default(),
    )
    .unwrap();
    let result = manager.reconcile_execution_mass_status(&mass_status, harness.exec_engine());
    let cache = harness.cache().borrow();
    let recovered = cache.position(&retained.id).unwrap();

    assert!(result.unresolved_positions.is_empty());
    assert!(recovered.is_closed());
    assert_eq!(recovered.strategy_id, strategy_id);
    assert_eq!(
        recovered.quantity,
        Quantity::zero(instrument.size_precision())
    );
    assert_eq!(
        cache
            .positions_open(None, None, None, Some(&account_id), None)
            .len(),
        0
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_restores_historical_terminal_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({ "orders": [] }));
    *state.orders_payload.lock().await = Some(serde_json::json!({
        "orders": [
            {
                "ts": 1_704_067_200,
                "tn": 500_000_000,
                "oid": "OID-FILLED",
                "aid": "account-1",
                "u": "u",
                "s": "EURUSD-PERP",
                "p": "1.08400",
                "q": 100,
                "xq": 100,
                "rq": 0,
                "o": "FILLED",
                "d": "B",
                "tif": "IOC",
                "cid": 12345,
                "r": null,
                "tag": null,
                "txt": null,
                "po": false
            },
            {
                "ts": 1_704_067_201,
                "tn": 600_000_000,
                "oid": "OID-CANCELED",
                "aid": "account-1",
                "u": "u",
                "s": "EURUSD-PERP",
                "p": "1.08390",
                "q": 200,
                "xq": 50,
                "rq": 150,
                "o": "CANCELED",
                "d": "S",
                "tif": "GTC",
                "cid": null,
                "r": null,
                "tag": null,
                "txt": null,
                "po": true
            }
        ]
    }));
    *state.fills_payload.lock().await = Some(serde_json::json!({
        "fills": [{
            "trade_id": "T-FILLED",
            "order_id": "OID-FILLED",
            "fee": "0.10",
            "is_taker": true,
            "is_block_trade": false,
            "is_final_settlement": false,
            "price": "1.08410",
            "quantity": 100,
            "side": "B",
            "symbol": "EURUSD-PERP",
            "timestamp": "2024-01-01T00:00:00.508000123Z",
            "account_id": "account-1"
        }]
    }));
    let mut canceled_fill = state.fills_payload.lock().await.as_ref().unwrap()["fills"][0].clone();
    canceled_fill["trade_id"] = serde_json::json!("T-CANCELED");
    canceled_fill["order_id"] = serde_json::json!("OID-CANCELED");
    canceled_fill["quantity"] = serde_json::json!(50);
    canceled_fill["side"] = serde_json::json!("S");
    canceled_fill["timestamp"] = serde_json::json!("2024-01-01T00:00:01.608000456Z");
    state.fills_payload.lock().await.as_mut().unwrap()["fills"]
        .as_array_mut()
        .unwrap()
        .push(canceled_fill);
    *state.positions_payload.lock().await = Some(serde_json::json!({ "positions": [] }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status");
    let order_reports = mass_status.order_reports();
    let filled = order_reports
        .get(&VenueOrderId::from("OID-FILLED"))
        .expect("filled historical order");
    let canceled = order_reports
        .get(&VenueOrderId::from("OID-CANCELED"))
        .expect("canceled historical order");
    let fill_reports = mass_status.fill_reports();

    assert_eq!(order_reports.len(), 2);
    assert_eq!(
        filled.client_order_id,
        Some(ClientOrderId::from("CID-12345"))
    );
    assert_eq!(filled.order_status, OrderStatus::Filled);
    assert_eq!(filled.quantity, Quantity::from("100"));
    assert_eq!(filled.filled_qty, Quantity::from("100"));
    assert_eq!(filled.price, Some(Price::from("1.08400")));
    assert_eq!(filled.time_in_force, TimeInForce::Ioc);
    assert_eq!(canceled.client_order_id, None);
    assert_eq!(canceled.order_status, OrderStatus::Canceled);
    assert_eq!(canceled.quantity, Quantity::from("200"));
    assert_eq!(canceled.filled_qty, Quantity::from("50"));
    assert_eq!(canceled.price, Some(Price::from("1.08390")));
    assert_eq!(canceled.order_side, Some(OrderSide::Sell));
    assert_eq!(
        filled.ts_accepted,
        UnixNanos::from(1_704_067_200_500_000_000_u64)
    );
    assert_eq!(
        filled.ts_last,
        UnixNanos::from(1_704_067_200_508_000_123_u64)
    );
    assert_eq!(
        canceled.ts_accepted,
        UnixNanos::from(1_704_067_201_600_000_000_u64)
    );
    assert_eq!(
        canceled.ts_last,
        UnixNanos::from(1_704_067_201_608_000_456_u64)
    );
    assert_eq!(fill_reports.len(), 2);
    assert_eq!(
        fill_reports
            .get(&VenueOrderId::from("OID-FILLED"))
            .expect("companion fill")
            .first()
            .expect("first companion fill")
            .last_px,
        Price::from("1.08410")
    );
    let queries = state.orders_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].start_timestamp_ns, None);
    assert_eq!(queries[0].end_timestamp_ns, None);
    drop(queries);
    assert_eq!(state.fills_queries.lock().await.len(), 1);
    assert_eq!(state.open_orders_queries.lock().await.len(), 1);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[case::trade_backed(true, false, 1)]
#[case::verified_zero(true, true, 1)]
#[case::missing_history(false, false, 32)]
#[tokio::test]
async fn test_historical_position_flip_commission_replay(
    #[case] with_fills: bool,
    #[case] zero_fee: bool,
    #[case] days_after_fill: u64,
) {
    let instruments: AxInstrumentsResponse =
        serde_json::from_value(load_test_data("captured/instruments.json")).unwrap();
    let wire_instrument = instruments
        .instruments
        .iter()
        .find(|instrument| instrument.symbol == "XAG-PERP")
        .unwrap();
    let instrument =
        parse_instrument(wire_instrument, UnixNanos::default(), UnixNanos::default()).unwrap();
    let account_id = AccountId::from("AX-001");

    let harness = ExecutionHarness::new(
        TraderId::from("TESTER-001"),
        *AX_CLIENT_ID,
        account_id,
        instrument.clone(),
    );
    add_test_account_to_cache(harness.cache(), account_id);

    let core = ExecutionClientCore::new(
        harness.trader_id(),
        *AX_CLIENT_ID,
        *AX_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        harness.cache().clone(),
    );
    let client = AxExecutionClient::new(
        core,
        create_test_exec_config("127.0.0.1:9".parse().unwrap()),
    )
    .unwrap();
    harness.register_client(Box::new(client)).unwrap();
    let mut manager = ExecutionManager::new(
        harness.clock().clone(),
        harness.cache().clone(),
        ExecutionManagerConfig::default(),
    )
    .unwrap();
    let strategy_id = StrategyId::from("HISTORY-001");
    manager
        .claim_external_orders(instrument.id(), strategy_id)
        .unwrap();
    harness
        .exec_engine()
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let orders: AxOrdersResponse =
        serde_json::from_value(load_test_data("captured/orders-live.json")).unwrap();
    let fills: AxFillsResponse =
        serde_json::from_value(load_test_data("captured/fills.json")).unwrap();
    let day_ns = 24 * 60 * 60 * 1_000_000_000;
    let ts_fill = u64::try_from(fills.fills[0].timestamp.as_nanosecond()).unwrap();
    let ts_init = UnixNanos::from(ts_fill + days_after_fill * day_ns);
    let mut mass = ExecutionMassStatus::new(*AX_CLIENT_ID, account_id, *AX_VENUE, ts_init, None);
    mass.set_report_window(Some(UnixNanos::from(ts_init.as_u64() - 7 * day_ns)), true);
    let mut reports = Vec::new();
    let mut fill_reports = Vec::new();

    for fill in fills.fills.iter().rev() {
        let wire_order = orders
            .orders
            .iter()
            .find(|order| Some(order.oid.as_str()) == fill.order_id.as_deref())
            .unwrap();
        let report = parse_order_detail_status_report(
            wire_order,
            account_id,
            &instrument,
            ts_init,
            None::<&fn(u64) -> Option<ClientOrderId>>,
        )
        .unwrap();
        assert_eq!(report.ts_last < mass.lookback_start().unwrap(), !with_fills);
        reports.push(report);
        let mut fill = fill.clone();
        if zero_fee {
            fill.fee = dec!(0);
        }

        let fill_report = parse_fill_report(&fill, account_id, &instrument, ts_init).unwrap();
        assert_eq!(
            fill_report.ts_event < mass.lookback_start().unwrap(),
            !with_fills
        );
        fill_reports.push(fill_report);
    }

    assert_eq!(
        fills
            .fills
            .iter()
            .map(|fill| fill.fee)
            .sum::<rust_decimal::Decimal>(),
        dec!(0.052372)
    );

    let expected_fee = if with_fills && !zero_fee {
        dec!(0.05)
    } else {
        dec!(0)
    };

    mass.add_order_reports(reports.clone());
    if with_fills {
        mass.add_fill_reports(fill_reports);
    }

    mass.add_position_reports(vec![PositionStatusReport::new(
        account_id,
        instrument.id(),
        PositionSide::Flat,
        Quantity::from("0"),
        ts_init,
        ts_init,
        None,
        None,
        None,
    )]);

    for replay in 0..2 {
        let result = manager.reconcile_execution_mass_status(&mass, harness.exec_engine());
        assert!(result.unresolved_positions.is_empty());

        let replay_fills: Vec<_> = result
            .events
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill),
                _ => None,
            })
            .collect();

        assert_eq!(replay_fills.len(), if replay == 0 { 3 } else { 0 });

        for (index, fill) in replay_fills.iter().enumerate() {
            assert_eq!(fill.instrument_id, instrument.id());
            assert_eq!(fill.account_id, account_id);
            assert_eq!(fill.strategy_id, strategy_id);
            assert_eq!(
                fill.client_order_id,
                reports[index].client_order_id.unwrap()
            );
            assert_eq!(fill.venue_order_id, reports[index].venue_order_id);
            assert_eq!(fill.order_type, OrderType::Limit);
            assert_eq!(fill.currency, Currency::USD());
            assert!(fill.reconciliation);
            assert_eq!(
                fill.trade_id
                    == TradeId::from(["fixture-0006", "fixture-0004", "fixture-0002"][index]),
                with_fills
            );
            assert_eq!(fill.last_qty, Quantity::from(["1", "2", "1"][index]));
            assert_eq!(
                fill.last_px,
                Price::from(["65.48", "65.45", "65.48"][index])
            );
            assert_eq!(
                fill.order_side,
                [OrderSide::Buy, OrderSide::Sell, OrderSide::Buy][index]
            );
            assert_eq!(
                fill.liquidity_side,
                if with_fills {
                    LiquiditySide::Taker
                } else {
                    LiquiditySide::NoLiquiditySide
                }
            );
            assert_eq!(
                fill.commission,
                with_fills.then(|| Money::from(if zero_fee {
                    "0 USD"
                } else {
                    ["0.01 USD", "0.03 USD", "0.01 USD"][index]
                }))
            );

            assert_eq!(fill.to_string().contains("commission=None,"), !with_fills);
        }

        let cache = harness.cache().borrow();
        let positions =
            cache.positions(None, Some(&instrument.id()), None, Some(&account_id), None);
        assert_eq!(positions.len(), 1);
        let position = &positions[0];
        let snapshots = cache.position_snapshots(Some(&position.id), Some(&account_id));
        assert_eq!(snapshots.len(), 1);
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.quantity, Quantity::from("0"));
        assert_eq!(position.avg_px_open, dec!(65.45));
        assert_eq!(position.avg_px_close, Some(dec!(65.48)));
        assert_eq!(snapshots[0].side, PositionSide::Flat);
        assert_eq!(snapshots[0].quantity, Quantity::from("0"));
        assert_eq!(snapshots[0].avg_px_open, dec!(65.48));
        assert_eq!(snapshots[0].avg_px_close, Some(dec!(65.45)));
        assert_eq!(
            position
                .commissions()
                .iter()
                .map(|fee| fee.as_decimal())
                .sum::<rust_decimal::Decimal>(),
            expected_fee / dec!(2)
        );
        assert_eq!(
            position.realized_pnl.unwrap().as_decimal(),
            dec!(-0.03) - expected_fee / dec!(2)
        );
        assert_eq!(
            snapshots[0].realized_pnl.unwrap().as_decimal(),
            dec!(-0.03) - expected_fee / dec!(2)
        );
        let total_pnl = position.realized_pnl.unwrap().as_decimal()
            + snapshots[0].realized_pnl.unwrap().as_decimal();
        assert_eq!(total_pnl, dec!(-0.06) - expected_fee);
        let total_fee = position
            .commissions()
            .iter()
            .chain(snapshots[0].commissions().iter())
            .map(|fee| fee.as_decimal())
            .sum::<rust_decimal::Decimal>();
        assert_eq!(total_fee, expected_fee);

        for report in &reports {
            let order = cache.order(&report.client_order_id.unwrap()).unwrap();
            assert_eq!(order.status(), OrderStatus::Filled);
            assert_eq!(order.filled_qty(), report.filled_qty);
            assert_eq!(order.trade_ids().len(), 1);
        }
    }
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_fetches_uncached_report_instrument() {
    let (addr, state) = start_test_server().await.unwrap();
    set_uncached_mass_status_payload(&state).await;

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);
    assert!(state.instrument_queries.lock().await.is_empty());

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status");
    let order = mass_status
        .order_reports()
        .get(&VenueOrderId::from("OID-UNCACHED"))
        .cloned()
        .expect("historical order");
    let fill = mass_status
        .fill_reports()
        .get(&VenueOrderId::from("OID-UNCACHED"))
        .and_then(|reports| reports.first())
        .cloned()
        .expect("fill");
    let position = mass_status
        .position_reports()
        .get(&InstrumentId::from("GBPUSD-PERP.AX"))
        .and_then(|reports| reports.first())
        .cloned()
        .expect("position");
    let instrument_queries = state.instrument_queries.lock().await;

    assert_eq!(order.instrument_id, InstrumentId::from("GBPUSD-PERP.AX"));
    assert_eq!(order.order_status, OrderStatus::Filled);
    assert_eq!(order.filled_qty, Quantity::from("100"));
    assert_eq!(fill.instrument_id, InstrumentId::from("GBPUSD-PERP.AX"));
    assert_eq!(fill.trade_id, TradeId::from("T-UNCACHED"));
    assert_eq!(fill.last_qty, Quantity::from("100"));
    assert_eq!(fill.last_px, Price::from("1.08410"));
    assert_eq!(position.instrument_id, InstrumentId::from("GBPUSD-PERP.AX"));
    assert!(position.is_long());
    assert_eq!(position.quantity, Quantity::from("100"));
    assert_eq!(position.avg_px_open, Some(dec!(1.084)));
    assert_eq!(
        mass_status.position_reports()[&position.instrument_id].len(),
        1
    );
    assert!(!instrument_queries.is_empty());
    assert!(
        instrument_queries
            .iter()
            .all(|query| query.symbol.as_str() == "GBPUSD-PERP")
    );
    drop(instrument_queries);

    client.disconnect().await.expect("Failed to disconnect");
}

#[derive(Debug, Clone, Copy)]
enum InstrumentResolutionFailure {
    Request,
    Parse,
}

#[rstest]
#[case::request(
    InstrumentResolutionFailure::Request,
    "Failed to resolve AX instrument GBPUSD-PERP via GET /instrument: Unexpected HTTP status code 400: {\"error\":\"instrument unavailable\"}"
)]
#[case::parse(
    InstrumentResolutionFailure::Parse,
    "Failed to resolve AX instrument GBPUSD-PERP via GET /instrument: AX minimum_order_size must be a positive whole number, was 0"
)]
#[tokio::test]
async fn test_generate_mass_status_aborts_on_uncached_instrument_failure(
    #[case] failure: InstrumentResolutionFailure,
    #[case] expected: &str,
) {
    let (addr, state) = start_test_server().await.unwrap();
    set_uncached_mass_status_payload(&state).await;

    match failure {
        InstrumentResolutionFailure::Request => {
            state
                .instrument_fail
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        InstrumentResolutionFailure::Parse => {
            let mut instrument =
                load_test_data("http_get_instruments.json")["instruments"][0].clone();
            instrument["minimum_order_size"] = serde_json::json!("0");
            *state.instrument_payload.lock().await = Some(instrument);
        }
    }

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);
    assert!(state.instrument_queries.lock().await.is_empty());

    let error = client.generate_mass_status(None).await.unwrap_err();

    assert_eq!(error.to_string(), expected);
    assert!(
        state
            .instrument_queries
            .lock()
            .await
            .iter()
            .all(|query| query.symbol.as_str() == "GBPUSD-PERP")
    );

    client.disconnect().await.expect("Failed to disconnect");
}

async fn set_uncached_mass_status_payload(state: &crate::common::server::TestServerState) {
    *state.orders_payload.lock().await = Some(serde_json::json!({
        "orders": [{
            "ts": 1_704_067_200,
            "tn": 500_000_000,
            "oid": "OID-UNCACHED",
            "aid": "account-1",
            "u": "u",
            "s": "GBPUSD-PERP",
            "p": "1.08400",
            "q": 100,
            "xq": 100,
            "rq": 0,
            "o": "FILLED",
            "d": "B",
            "tif": "IOC",
            "cid": null,
            "r": null,
            "tag": null,
            "txt": null,
            "po": false
        }]
    }));
    *state.fills_payload.lock().await = Some(serde_json::json!({
        "fills": [{
            "trade_id": "T-UNCACHED",
            "order_id": "OID-UNCACHED",
            "fee": "0.10",
            "is_taker": true,
            "is_block_trade": false,
            "is_final_settlement": false,
            "price": "1.08410",
            "quantity": 100,
            "side": "B",
            "symbol": "GBPUSD-PERP",
            "timestamp": "2024-01-01T00:00:00Z",
            "account_id": "account-1"
        }]
    }));
    *state.positions_payload.lock().await = Some(serde_json::json!({
        "positions": [{
            "account_id": "account-1",
            "symbol": "GBPUSD-PERP",
            "signed_quantity": 100,
            "signed_notional": "108.40",
            "timestamp": "2024-01-01T00:00:00Z",
            "realized_pnl": "1.25"
        }]
    }));
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_open_only_uses_open_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({
        "orders": [{
            "tn": 500_000_000,
            "ts": 1_704_067_200,
            "d": "B",
            "o": "ACCEPTED",
            "oid": "OID-OPEN",
            "p": "1.08400",
            "q": 100,
            "rq": 100,
            "s": "EURUSD-PERP",
            "tif": "GTC",
            "u": "u",
            "xq": 0,
            "cid": null,
            "tag": null,
            "po": true
        }]
    }));
    *state.orders_payload.lock().await = Some(serde_json::json!({
        "orders": [create_historical_order(
            "OID-HISTORICAL",
            "FILLED",
            1_704_067_201,
        )]
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("OID-OPEN"));
    assert_eq!(reports[0].order_status, OrderStatus::Accepted);
    assert_eq!(state.open_orders_queries.lock().await.len(), 1);
    assert_eq!(state.orders_queries.lock().await.len(), 0);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_filters() {
    let (addr, state) = start_test_server().await.unwrap();

    // Replace default fixture with EURUSD + XAU orders across different states
    let payload = serde_json::json!({
        "orders": [
            {
                "tn": 0,
                "ts": 1704067200,
                "d": "B",
                "o": "ACCEPTED",
                "oid": "OID-ACCEPTED",
                "p": "1.08400",
                "q": 100,
                "rq": 100,
                "s": "EURUSD-PERP",
                "tif": "GTC",
                "u": "u",
                "xq": 0,
                "cid": null,
                "tag": null
            },
            {
                "tn": 0,
                "ts": 1704067201,
                "d": "S",
                "o": "FILLED",
                "oid": "OID-FILLED",
                "p": "2000.00",
                "q": 1,
                "rq": 0,
                "s": "XAU-PERP",
                "tif": "GTC",
                "u": "u",
                "xq": 1,
                "cid": null,
                "tag": null
            },
            {
                "tn": 0,
                "ts": 1704067203,
                "d": "B",
                "o": "PENDING",
                "oid": "OID-PENDING",
                "p": "1.08420",
                "q": 100,
                "rq": 100,
                "s": "EURUSD-PERP",
                "tif": "GTC",
                "u": "u",
                "xq": 0,
                "cid": null,
                "tag": null
            },
            {
                "tn": 0,
                "ts": 1704067202,
                "d": "B",
                "o": "ACCEPTED",
                "oid": "OID-ACCEPTED-2",
                "p": "1.08410",
                "q": 100,
                "rq": 100,
                "s": "EURUSD-PERP",
                "tif": "GTC",
                "u": "u",
                "xq": 0,
                "cid": null,
                "tag": null
            }
        ],
        "total_count": 4,
        "limit": 100,
        "offset": 0
    });
    *state.orders_payload.lock().await = Some(payload.clone());
    let open_orders = payload["orders"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|order| order["o"] == "ACCEPTED" || order["o"] == "PENDING")
        .cloned()
        .collect::<Vec<_>>();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({ "orders": open_orders }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    // No filter -> all 4 reports
    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client
        .generate_order_status_reports(&cmd)
        .await
        .expect("generate_order_status_reports");
    assert_eq!(reports.len(), 4);

    // Filter by instrument -> only XAU
    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        Some(InstrumentId::from("XAU-PERP.AX")),
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].instrument_id, InstrumentId::from("XAU-PERP.AX"),);

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();
    assert_eq!(reports.len(), 3);
    assert!(
        reports
            .iter()
            .all(|r| r.order_status.is_open() || r.order_status.is_inflight()),
    );
    // A resting `PENDING` order maps to `Submitted`, which is in-flight rather than open, and must
    // still be reported under `open_only` or reconciliation treats it as missing at the venue.
    assert_eq!(
        reports
            .iter()
            .filter(|r| r.order_status == OrderStatus::Submitted)
            .count(),
        1,
    );

    // A start cutoff at ts=1704067201 excludes closed history before it, but an order still
    // working at the venue is authoritative regardless of how long it has rested. The only
    // closed order in the fixture (OID-FILLED) sits on the cutoff, so nothing is dropped.
    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        Some(UnixNanos::from(1_704_067_201_000_000_000u64)),
        None,
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();
    assert_eq!(reports.len(), 4);
    // OID-ACCEPTED predates the cutoff and is retained because it is not closed.
    assert!(
        reports
            .iter()
            .any(|r| r.venue_order_id.as_str() == "OID-ACCEPTED"
                && r.ts_last.as_u64() < 1_704_067_201_000_000_000),
    );

    // A cutoff past the closed order drops it, while the open orders survive.
    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        Some(UnixNanos::from(1_704_067_202_000_000_000u64)),
        None,
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();
    assert_eq!(reports.len(), 3);
    assert!(
        !reports
            .iter()
            .any(|r| r.order_status == OrderStatus::Filled),
        "closed history must respect the report window",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_reads_all_partial_pages() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({"orders": []}));

    let orders = (0..101)
        .map(|index| {
            serde_json::json!({
                "tn": index,
                "ts": 1_704_067_200 + index,
                "d": "B",
                "o": "ACCEPTED",
                "oid": format!("OID-{index:03}"),
                "p": "1.08400",
                "q": 100,
                "rq": 100,
                "s": "EURUSD-PERP",
                "tif": "GTC",
                "u": "u",
                "xq": 0,
                "cid": null,
                "tag": null,
            })
        })
        .collect::<Vec<_>>();
    *state.orders_payload.lock().await = Some(serde_json::json!({ "orders": orders }));
    *state.orders_page_size.lock().await = Some(25);

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();
    let venue_order_ids = reports
        .iter()
        .map(|report| report.venue_order_id)
        .collect::<HashSet<_>>();

    assert_eq!(reports.len(), 101);
    assert_eq!(venue_order_ids.len(), 101);
    assert!(venue_order_ids.contains(&VenueOrderId::from("OID-000")));
    assert!(venue_order_ids.contains(&VenueOrderId::from("OID-100")));
    assert_eq!(state.orders_queries.lock().await.len(), 5);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_rejects_duplicate_order_ids() {
    let (addr, state) = start_test_server().await.unwrap();
    let order = serde_json::json!({
        "tn": 0,
        "ts": 1_704_067_200,
        "d": "B",
        "o": "ACCEPTED",
        "oid": "OID-DUPLICATE",
        "p": "1.08400",
        "q": 100,
        "rq": 100,
        "s": "EURUSD-PERP",
        "tif": "GTC",
        "u": "u",
        "xq": 0,
        "cid": null,
        "tag": null,
    });
    *state.orders_payload.lock().await = Some(serde_json::json!({
        "orders": [order.clone(), order],
    }));
    *state.orders_page_size.lock().await = Some(1);

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        None,
        None,
        None,
        None,
    );
    let error = client
        .generate_order_status_reports(&cmd)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "AX orders pagination returned duplicate order ID OID-DUPLICATE"
    );
    assert_eq!(state.orders_queries.lock().await.len(), 2);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_rejects_repeated_cursor() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.orders_payload.lock().await = Some(serde_json::json!({
        "orders": [
            create_historical_order("OID-001", "ACCEPTED", 1_704_067_200),
            create_historical_order("OID-002", "ACCEPTED", 1_704_067_201),
            create_historical_order("OID-003", "ACCEPTED", 1_704_067_202),
        ]
    }));
    *state.orders_page_size.lock().await = Some(1);
    *state.orders_repeated_cursor.lock().await = Some("same".to_string());

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        None,
        None,
        None,
        None,
    );
    let error = client
        .generate_order_status_reports(&cmd)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "AX orders pagination repeated cursor \"same\""
    );
    assert_eq!(state.orders_queries.lock().await.len(), 2);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_forwards_historical_bounds() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.open_orders_payload.lock().await = Some(serde_json::json!({ "orders": [] }));
    *state.orders_payload.lock().await = Some(serde_json::json!({ "orders": [] }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let start = UnixNanos::from(1_704_067_200_123_456_789);
    let end = UnixNanos::from(1_704_067_299_987_654_321);
    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        Some(start),
        Some(end),
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();
    let queries = state.orders_queries.lock().await;

    assert!(reports.is_empty());
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].start_timestamp_ns, Some(start.as_i64()));
    assert_eq!(queries[0].end_timestamp_ns, Some(end.as_i64()));
    assert_eq!(queries[0].limit, Some(100));
    assert_eq!(queries[0].cursor, None);
    drop(queries);

    client.disconnect().await.expect("Failed to disconnect");
}

fn create_historical_order(oid: &str, status: &str, ts: i64) -> serde_json::Value {
    serde_json::json!({
        "ts": ts,
        "tn": 500_000_000,
        "oid": oid,
        "aid": "account-1",
        "u": "u",
        "s": "EURUSD-PERP",
        "p": "1.08400",
        "q": 100,
        "xq": 0,
        "rq": 100,
        "o": status,
        "d": "B",
        "tif": "GTC",
        "cid": null,
        "r": null,
        "tag": null,
        "txt": null,
        "po": true
    })
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_filters() {
    let (addr, state) = start_test_server().await.unwrap();

    *state.fills_payload.lock().await = Some(serde_json::json!({
        "fills": [
            {
                "trade_id": "T-1",
                "order_id": "OID-A",
                "fee": "0.10",
                "is_taker": true,
                "price": "1.08450",
                "quantity": 100,
                "side": "B",
                "symbol": "EURUSD-PERP",
                "timestamp": "2024-01-15T10:30:45Z",
                "account_id": "u"
            },
            {
                "trade_id": "T-2",
                "order_id": "OID-B",
                "fee": "0.20",
                "is_taker": false,
                "is_block_trade": false,
                "is_final_settlement": false,
                "price": "2000.50",
                "quantity": 2,
                "side": "S",
                "symbol": "XAU-PERP",
                "timestamp": "2024-01-15T10:31:12Z",
                "account_id": "u"
            },
            {
                "trade_id": "T-3",
                "order_id": "OID-A",
                "fee": "0.15",
                "is_taker": true,
                "is_block_trade": false,
                "is_final_settlement": false,
                "price": "1.08455",
                "quantity": 50,
                "side": "B",
                "symbol": "EURUSD-PERP",
                "timestamp": "2024-01-15T10:32:00Z",
                "account_id": "u"
            }
        ]
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    // No filter -> all 3
    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client
        .generate_fill_reports(cmd)
        .await
        .expect("generate_fill_reports");
    assert_eq!(reports.len(), 3);

    // By instrument -> only XAU
    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("XAU-PERP.AX")),
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd).await.unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].trade_id.to_string(), "T-2");

    // By venue order -> only OID-A (2 fills)
    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(VenueOrderId::new("OID-A")),
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd).await.unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.venue_order_id.as_str() == "OID-A"));

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_reads_all_cursor_pages() {
    let (addr, state) = start_test_server().await.unwrap();
    let fills = (0..101)
        .map(|index| {
            serde_json::json!({
                "trade_id": format!("T-{index:03}"),
                "order_id": format!("OID-{index:03}"),
                "fee": "0.10",
                "is_taker": true,
                "is_block_trade": false,
                "is_final_settlement": false,
                "price": "1.08450",
                "quantity": 100,
                "side": "B",
                "symbol": "EURUSD-PERP",
                "timestamp": "2024-01-15T10:30:45Z",
                "account_id": "u"
            })
        })
        .collect::<Vec<_>>();
    *state.fills_payload.lock().await = Some(serde_json::json!({ "fills": fills }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd).await.unwrap();
    let trade_ids = reports
        .iter()
        .map(|report| report.trade_id)
        .collect::<HashSet<_>>();

    assert_eq!(reports.len(), 101);
    assert_eq!(trade_ids.len(), 101);
    assert!(trade_ids.contains(&TradeId::new("T-000")));
    assert!(trade_ids.contains(&TradeId::new("T-100")));

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_rejects_duplicate_trade_ids() {
    let (addr, state) = start_test_server().await.unwrap();
    let fill = serde_json::json!({
        "trade_id": "T-DUPLICATE",
        "order_id": "OID-DUPLICATE",
        "fee": "0.10",
        "is_taker": true,
        "is_block_trade": false,
        "is_final_settlement": false,
        "price": "1.08450",
        "quantity": 100,
        "side": "B",
        "symbol": "EURUSD-PERP",
        "timestamp": "2024-01-15T10:30:45Z",
        "account_id": "u",
    });
    *state.fills_payload.lock().await = Some(serde_json::json!({
        "fills": [fill.clone(), fill],
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let error = client.generate_fill_reports(cmd).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "AX fills pagination returned duplicate trade ID T-DUPLICATE"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_rejects_ambiguous_fill_classification() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.fills_payload.lock().await = Some(serde_json::json!({
        "fills": [{
            "trade_id": "T-AMBIGUOUS",
            "order_id": null,
            "fee": "0.10",
            "is_taker": true,
            "price": "1.08450",
            "quantity": 100,
            "side": "B",
            "symbol": "EURUSD-PERP",
            "timestamp": "2024-01-15T10:30:45Z",
            "account_id": "u"
        }]
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let result = client.generate_fill_reports(cmd).await;

    let error = result.unwrap_err();

    assert!(
        format!("{error:#}").contains("missing order_id and explicit special-fill classification"),
        "error was: {error:#}"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_rounds_sub_cent_fee_to_usd_precision() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.fills_payload.lock().await = Some(serde_json::json!({
        "fills": [{
            "trade_id": "T-SUBCENT",
            "order_id": "OID-SUBCENT",
            "fee": "0.012188",
            "is_taker": true,
            "is_block_trade": false,
            "is_final_settlement": false,
            "price": "1.08450",
            "quantity": 100,
            "side": "B",
            "symbol": "EURUSD-PERP",
            "timestamp": "2024-01-15T10:30:45Z",
            "account_id": "u"
        }]
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let reports = client
        .generate_fill_reports(GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("generate_fill_reports");

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].commission, Money::from("0.01 USD"));

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_rejects_unrepresentable_fee() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.fills_payload.lock().await = Some(serde_json::json!({
        "fills": [{
            "trade_id": "T-OVERFLOW",
            "order_id": "OID-OVERFLOW",
            "fee": "9999999999999999999999999999",
            "is_taker": true,
            "is_block_trade": false,
            "is_final_settlement": false,
            "price": "1.08450",
            "quantity": 100,
            "side": "B",
            "symbol": "EURUSD-PERP",
            "timestamp": "2024-01-15T10:30:45Z",
            "account_id": "u"
        }]
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let error = client
        .generate_fill_reports(GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("unrepresentable fee must fail the fill request");
    assert!(
        format!("{error:#}").contains("Failed to convert fill.fee Decimal to Money"),
        "error was: {error:#}"
    );

    let mass_error = client
        .generate_mass_status(Some(60))
        .await
        .expect_err("unrepresentable fee must fail mass status");
    assert!(
        format!("{mass_error:#}").contains("Failed to convert fill.fee Decimal to Money"),
        "mass status error was: {mass_error:#}"
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
    assert!(
        event.is_err() || event.as_ref().is_ok_and(|msg| msg.is_none()),
        "unrepresentable fee must not emit a fallback fill, was {event:?}"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[case::fills("fills")]
#[case::fills_task("fills_task")]
#[case::targeted("targeted")]
#[case::targeted_task("targeted_task")]
#[case::history("history")]
#[case::history_task("history_task")]
#[case::mass_status("mass_status")]
#[tokio::test]
async fn test_commission_failure_defers_reports_until_corrected(#[case] route: &str) {
    let (addr, state) = start_test_server().await.unwrap();
    let instruments: AxInstrumentsResponse =
        serde_json::from_value(load_test_data("http_get_instruments.json")).unwrap();
    let instrument = parse_instrument(
        &instruments.instruments[0],
        UnixNanos::default(),
        UnixNanos::default(),
    )
    .unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("AX-001");
    let client_order_id = ClientOrderId::from("O-FEE-RECOVERY");
    let venue_order_id = VenueOrderId::from("O-PARTIAL-CANCELED");
    let harness = ExecutionHarness::new(trader_id, *AX_CLIENT_ID, account_id, instrument.clone());
    add_test_account_to_cache(harness.cache(), account_id);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("200"))
        .price(Price::from("1.0840"))
        .time_in_force(TimeInForce::Ioc)
        .build();
    let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
    harness
        .cache()
        .borrow_mut()
        .add_order(order, None, Some(*AX_CLIENT_ID), false)
        .unwrap();
    harness
        .cache()
        .borrow_mut()
        .update_order(&accepted)
        .unwrap();

    let core = ExecutionClientCore::new(
        trader_id,
        *AX_CLIENT_ID,
        *AX_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        harness.cache().clone(),
    );
    let mut client = AxExecutionClient::new(core, create_test_exec_config(addr)).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(tx);
    client.start().unwrap();
    client.connect().await.unwrap();
    drain_rx(&mut rx);
    let now = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .as_u64()
        - 2_000_000_000;
    let mut fills = load_test_data("http_get_fills_partial_canceled.json");
    fills["fills"].as_array_mut().unwrap().truncate(2);
    fills["fills"].as_array_mut().unwrap().reverse();
    for (index, fill) in fills["fills"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        fill["timestamp"] = jiff::Timestamp::from_nanosecond(i128::from(now + index as u64))
            .unwrap()
            .to_string()
            .into();
    }

    let fill_cmd = GenerateFillReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .instrument_id(Some(instrument.id()))
        .venue_order_id(Some(venue_order_id))
        .build()
        .unwrap();
    let mut prior = fills.clone();
    prior["fills"].as_array_mut().unwrap().truncate(1);
    *state.fills_payload.lock().await = Some(prior);
    let prior_reports = client
        .generate_fill_reports(fill_cmd.clone())
        .await
        .unwrap();
    assert_eq!(prior_reports.len(), 1);
    harness
        .exec_engine()
        .borrow_mut()
        .reconcile_fill_report(&prior_reports[0]);
    let before = harness
        .cache()
        .borrow()
        .order(&client_order_id)
        .unwrap()
        .clone();
    assert_eq!(before.status(), OrderStatus::PartiallyFilled);
    assert_eq!(before.filled_qty(), Quantity::from("40"));
    assert_eq!(
        before.commissions().values().copied().collect::<Vec<_>>(),
        vec![Money::from("0.10 USD")]
    );
    let mut orders = load_test_data("http_get_orders_partial_canceled.json");
    orders["orders"][0]["cid"] = serde_json::Value::Null;
    orders["orders"][0]["ts"] = (now / 1_000_000_000).into();
    orders["orders"][0]["tn"] = (now % 1_000_000_000).into();
    *state.orders_payload.lock().await = Some(orders);
    *state.open_orders_payload.lock().await = Some(serde_json::json!({"orders": []}));
    *state.positions_payload.lock().await = Some(serde_json::json!({"positions": []}));
    let mut status = load_test_data("http_get_order_status.json");
    status["status"]["order_id"] = venue_order_id.to_string().into();
    status["status"]["state"] = "CANCELED".into();
    *state.order_status_response.lock().await = Some((200, status));
    let order_cmd = GenerateOrderStatusReportBuilder::default()
        .ts_init(UnixNanos::default())
        .client_order_id(Some(client_order_id))
        .venue_order_id(Some(venue_order_id))
        .build()
        .unwrap();
    let history_cmd = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(false)
        .build()
        .unwrap();

    for valid in [false, true] {
        fills["fills"][1]["fee"] = if valid {
            "0.23"
        } else {
            "9999999999999999999999999999"
        }
        .into();

        *state.fills_payload.lock().await = Some(fills.clone());

        let result = match route {
            "fills" => client
                .generate_fill_reports(fill_cmd.clone())
                .await
                .map(|reports| assert_eq!(reports.len(), 2)),
            "fills_task" => {
                collect_report_task(client.generate_fill_reports_task(&fill_cmd).unwrap())
                    .await
                    .map(|reports| assert_eq!(reports.len(), 2))
            }
            "targeted" => client
                .generate_order_status_report(&order_cmd)
                .await
                .map(|report| {
                    assert_commission_recovery_order(
                        &report.unwrap(),
                        venue_order_id,
                        Some(client_order_id),
                        UnixNanos::from(now + 1),
                    );
                }),
            "targeted_task" => collect_report_task(
                client
                    .generate_order_status_report_task(&order_cmd)
                    .unwrap(),
            )
            .await
            .map(|report| {
                assert_commission_recovery_order(
                    &report.unwrap(),
                    venue_order_id,
                    Some(client_order_id),
                    UnixNanos::from(now + 1),
                );
            }),
            "history" => client
                .generate_order_status_reports(&history_cmd)
                .await
                .map(|reports| {
                    assert_eq!(reports.len(), 1);
                    assert_commission_recovery_order(
                        &reports[0],
                        venue_order_id,
                        None,
                        UnixNanos::from(now + 1),
                    );
                }),
            "history_task" => collect_report_task(
                client
                    .generate_order_status_reports_task(&history_cmd)
                    .unwrap(),
            )
            .await
            .map(|reports| {
                assert_eq!(reports.len(), 1);
                assert_commission_recovery_order(
                    &reports[0],
                    venue_order_id,
                    None,
                    UnixNanos::from(now + 1),
                );
            }),
            "mass_status" => client
                .generate_mass_status(None)
                .await
                .map(|mass| assert_eq!(mass.unwrap().order_reports().len(), 1)),
            _ => unreachable!(),
        };

        if valid {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert!(
                format!("{error:#}").contains("Failed to convert fill.fee Decimal to Money"),
                "{error:#}"
            );
        }

        let cache = harness.cache().borrow();
        let cached = cache.order(&client_order_id).unwrap();
        assert_eq!(cached.events(), before.events());
        assert_eq!(cached.status(), OrderStatus::PartiallyFilled);
        assert_eq!(cached.filled_qty(), Quantity::from("40"));
        assert_eq!(cached.trade_ids(), vec![&TradeId::from("T-FIRST")]);
        assert_eq!(
            cached.commissions().values().copied().collect::<Vec<_>>(),
            vec![Money::from("0.10 USD")]
        );
        assert!(rx.try_recv().is_err());
    }

    let reports = client.generate_fill_reports(fill_cmd).await.unwrap();
    assert_eq!(reports.len(), 2);

    for (report, trade, qty, fee, timestamp) in [
        (&reports[0], "T-FIRST", "40", "0.10 USD", now),
        (&reports[1], "T-LAST", "60", "0.23 USD", now + 1),
    ] {
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(report.venue_order_id, venue_order_id);
        assert_eq!(report.trade_id, TradeId::from(trade));
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_qty, Quantity::from(qty));
        assert_eq!(report.last_px, Price::from("1.0841"));
        assert_eq!(report.commission, Money::from(fee));
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.ts_event, UnixNanos::from(timestamp));
        assert_eq!(report.client_order_id, None);
        assert_eq!(report.venue_position_id, None);
        assert_eq!(report.avg_px, None);
    }

    let mass = client.generate_mass_status(None).await.unwrap().unwrap();
    client.disconnect().await.unwrap();
    harness.register_client(Box::new(client)).unwrap();
    let mut manager = ExecutionManager::new(
        harness.clock().clone(),
        harness.cache().clone(),
        ExecutionManagerConfig::default(),
    )
    .unwrap();

    for _ in 0..2 {
        let result = manager.reconcile_execution_mass_status(&mass, harness.exec_engine());
        assert!(result.unresolved_positions.is_empty());
        let cache = harness.cache().borrow();
        let cached = cache.order(&client_order_id).unwrap();
        assert_eq!(cached.status(), OrderStatus::Canceled);
        assert_eq!(cached.filled_qty(), Quantity::from("100"));
        assert_eq!(cached.leaves_qty(), Quantity::from("100"));
        assert_eq!(
            cached.trade_ids(),
            vec![&TradeId::from("T-FIRST"), &TradeId::from("T-LAST")]
        );
        assert_eq!(
            cached.commissions().values().copied().collect::<Vec<_>>(),
            vec![Money::from("0.33 USD")]
        );
        assert_eq!(
            cached
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                .count(),
            2
        );
    }
}

fn assert_commission_recovery_order(
    report: &OrderStatusReport,
    venue_order_id: VenueOrderId,
    client_order_id: Option<ClientOrderId>,
    ts_last: UnixNanos,
) {
    assert_eq!(report.account_id, AccountId::from("AX-001"));
    assert_eq!(report.instrument_id, InstrumentId::from("EURUSD-PERP.AX"));
    assert_eq!(report.client_order_id, client_order_id);
    assert_eq!(report.venue_order_id, venue_order_id);
    assert_eq!(report.order_side, Some(OrderSide::Buy));
    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.time_in_force, TimeInForce::Ioc);
    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.quantity, Quantity::from("200"));
    assert_eq!(report.filled_qty, Quantity::from("100"));
    assert_eq!(report.price, Some(Price::from("1.0840")));
    assert_eq!(report.ts_last, ts_last);
}

#[rstest]
#[tokio::test]
async fn test_submit_transport_failure_emits_no_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    state
        .disconnect_trigger
        .store(true, std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let places_before = state
        .get_messages()
        .await
        .iter()
        .filter(|message| message.get("t").and_then(|value| value.as_str()) == Some("p"))
        .count();

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-SUBMIT-TRANSPORT");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .price(Price::from("1.00"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    let mut saw_submitted = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(800);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Submitted(submitted)))) => {
                assert_eq!(submitted.client_order_id, client_order_id);
                saw_submitted = true;
            }
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Rejected(rejected)))) => {
                panic!(
                    "transport failure must not emit OrderRejected, was {}",
                    rejected.reason
                );
            }
            Ok(Some(_) | None) | Err(_) => {}
        }
    }

    assert!(
        saw_submitted,
        "expected OrderSubmitted before the failed send"
    );
    let places_after = state
        .get_messages()
        .await
        .iter()
        .filter(|message| message.get("t").and_then(|value| value.as_str()) == Some("p"))
        .count();
    assert_eq!(
        places_after, places_before,
        "place must not reach the dropped socket, before {places_before}, after {places_after}"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_already_terminal_order_is_forwarded() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-CANCEL-TWICE");
    let venue_order_id = VenueOrderId::new("OID-CANCEL-TWICE");
    client.register_external_order(
        client_order_id,
        venue_order_id,
        instrument_id,
        StrategyId::from("S-001"),
        UnixNanos::default(),
    );
    add_open_order_to_cache(
        &cache,
        client_order_id.as_str(),
        venue_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Canceled(OrderCanceled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(venue_order_id),
            Some(AccountId::from("AX-001")),
            None,
        )))
        .expect("mark order canceled");

    let cancel = || {
        CancelOrder::new(
            TraderId::from("TESTER-001"),
            Some(*AX_CLIENT_ID),
            StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    };

    client
        .cancel_order(cancel())
        .expect("first cancel should not error");
    client
        .cancel_order(cancel())
        .expect("second cancel should not error");

    wait_until_async(
        || async {
            state
                .get_messages()
                .await
                .iter()
                .filter(|message| message.get("t").and_then(|value| value.as_str()) == Some("x"))
                .count()
                >= 2
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    let result = tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv()).await;
    assert!(
        !matches!(
            result,
            Ok(Some(ExecutionEvent::Order(OrderEventAny::CancelRejected(
                _
            ))))
        ),
        "a forwarded cancel must not invent OrderCancelRejected, was {result:?}"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_position_status_reports_filters() {
    let (addr, state) = start_test_server().await.unwrap();

    *state.positions_payload.lock().await = Some(serde_json::json!({
        "positions": [
            {
                "account_id": "u",
                "symbol": "EURUSD-PERP",
                "signed_quantity": 100,
                "signed_notional": "108400.00",
                "timestamp": "2024-01-15T10:30:45Z",
                "realized_pnl": "0"
            },
            {
                "account_id": "u",
                "symbol": "XAU-PERP",
                "signed_quantity": -5,
                "signed_notional": "-10000.00",
                "timestamp": "2024-01-15T10:31:00Z",
                "realized_pnl": "0"
            },
            {
                "account_id": "u",
                "symbol": "NVDA-PERP",
                "signed_quantity": 0,
                "signed_notional": "0",
                "timestamp": "2024-01-15T10:32:00Z",
                "realized_pnl": "0"
            }
        ]
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let cmd = GeneratePositionStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client
        .generate_position_status_reports(&cmd)
        .await
        .expect("generate_position_status_reports");
    assert_eq!(reports.len(), 3);
    let flat = reports
        .iter()
        .find(|report| report.instrument_id == InstrumentId::from("NVDA-PERP.AX"))
        .unwrap();
    assert_eq!(flat.position_side, PositionSide::Flat);
    assert_eq!(flat.quantity, Quantity::zero(0));
    assert_eq!(flat.signed_decimal_qty, dec!(0));
    assert_eq!(flat.avg_px_open, None);

    // Filter by instrument -> only XAU short position
    let cmd = GeneratePositionStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("XAU-PERP.AX")),
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_position_status_reports(&cmd).await.unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].instrument_id, InstrumentId::from("XAU-PERP.AX"),);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_without_venue_order_id_emits_rejection() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MOD-NO-VOI");

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        None,
        Some(Quantity::from("5")),
        Some(Price::from("50001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("timed out waiting for modify rejection")
        .expect("execution event channel closed");
    let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event else {
        panic!("expected OrderModifyRejected, was {event:?}");
    };

    assert_eq!(rejected.client_order_id, client_order_id);
    assert_eq!(rejected.instrument_id, instrument_id);
    assert_eq!(rejected.venue_order_id, None);
    assert_eq!(rejected.reason, "missing venue_order_id");

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_with_trigger_price_is_rejected_before_http() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MOD-TRIGGER");
    let venue_order_id = VenueOrderId::new("OID-TRIGGER");
    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("5")),
        Some(Price::from("50001.00")),
        Some(Price::from("50000.00")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    assert_eq!(
        state
            .replace_order_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    let event = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("timed out waiting for modify rejection")
        .expect("execution event channel closed");
    let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event else {
        panic!("expected OrderModifyRejected, was {event:?}");
    };

    assert_eq!(rejected.client_order_id, client_order_id);
    assert_eq!(rejected.instrument_id, instrument_id);
    assert_eq!(rejected.venue_order_id, Some(venue_order_id));
    assert_eq!(
        rejected.reason,
        "AX does not support venue-native trigger prices"
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv())
            .await
            .is_err(),
        "expected exactly one modify rejection",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_with_fractional_quantity_is_rejected_before_http() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MOD-FRACTIONAL");
    let venue_order_id = VenueOrderId::new("OID-FRACTIONAL");
    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("1.5")),
        Some(Price::from("50001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("timed out waiting for modify rejection")
        .expect("execution event channel closed");
    let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event else {
        panic!("expected OrderModifyRejected, was {event:?}");
    };

    assert_eq!(
        state
            .replace_order_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    assert_eq!(rejected.client_order_id, client_order_id);
    assert_eq!(rejected.venue_order_id, Some(venue_order_id));
    assert!(
        rejected
            .reason
            .contains("AX requires whole contract quantities")
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_while_disconnected_emits_local_rejection() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MOD-NO-SESSION");
    let venue_order_id = VenueOrderId::new("OID-NO-SESSION");
    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("5")),
        Some(Price::from("50001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("timed out waiting for modify rejection")
        .expect("execution event channel closed");
    let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event else {
        panic!("expected OrderModifyRejected, was {event:?}");
    };

    assert_eq!(
        state
            .replace_order_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    assert_eq!(rejected.client_order_id, client_order_id);
    assert_eq!(rejected.venue_order_id, Some(venue_order_id));
    assert_eq!(rejected.reason, "AX execution client is not connected");
}

#[rstest]
#[case::current(false)]
#[case::stale_caller(true)]
#[tokio::test]
async fn test_modify_order_success_emits_no_rejection(#[case] stale: bool) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MOD-OK");
    let old_venue_order_id = VenueOrderId::new("OLD-OID");

    // Register the order metadata in the WS orders cache so the modify
    // success path finds it when updating venue_to_client_id and metadata.
    client.register_external_order(
        client_order_id,
        old_venue_order_id,
        instrument_id,
        StrategyId::from("S-001"),
        UnixNanos::default(),
    );

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(if stale {
            VenueOrderId::from("STALE-CALLER-ID")
        } else {
            old_venue_order_id
        }),
        Some(Quantity::from("5")),
        Some(Price::from("50001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    wait_until_async(
        || async {
            state
                .replace_order_count
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    let requests = state.replace_order_requests.lock().await.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["oid"],
        serde_json::json!(old_venue_order_id.as_str())
    );
    assert_eq!(requests[0]["p"], serde_json::json!("50001.00"));
    assert_eq!(requests[0]["q"], serde_json::json!(5));

    // No rejection event expected on success path
    let result = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
    assert!(
        !matches!(
            result,
            Ok(Some(ExecutionEvent::Order(OrderEventAny::ModifyRejected(
                _
            ))))
        ),
        "unexpected ModifyRejected on success path: {result:?}",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_http_error_emits_no_rejection() {
    let (addr, state) = start_test_server().await.unwrap();
    state
        .replace_order_fail
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MOD-ERR");
    let venue_order_id = VenueOrderId::new("OLD-OID-ERR");
    client.register_external_order(
        client_order_id,
        venue_order_id,
        instrument_id,
        StrategyId::from("S-001"),
        UnixNanos::default(),
    );

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("5")),
        Some(Price::from("50001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    wait_until_async(
        || async {
            state
                .replace_order_count
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    // Ambiguous HTTP failure: no rejection event, outcome left to reconciliation
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await;
    assert!(
        !matches!(
            result,
            Ok(Some(ExecutionEvent::Order(OrderEventAny::ModifyRejected(
                _
            ))))
        ),
        "expected no ModifyRejected for ambiguous HTTP failure, was {result:?}",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_invalid_replacement_id_does_not_panic_or_reject() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.replace_order_oid.lock().await = Some(" \n".to_string());

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MOD-BAD-OID");
    let venue_order_id = VenueOrderId::new("OLD-OID-BAD-OID");
    client.register_external_order(
        client_order_id,
        venue_order_id,
        instrument_id,
        StrategyId::from("S-001"),
        UnixNanos::default(),
    );
    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("5")),
        Some(Price::from("50001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");
    wait_until_async(
        || async {
            state
                .replace_order_count
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    let result = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;

    assert!(
        !matches!(
            result,
            Ok(Some(ExecutionEvent::Order(OrderEventAny::ModifyRejected(
                _
            ))))
        ),
        "invalid replacement ID is ambiguous and must not reject, was {result:?}",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_http_failure_emits_no_cancel_rejected() {
    let (addr, state) = start_test_server().await.unwrap();
    state
        .cancel_all_fail
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    add_open_order_to_cache(
        &cache,
        "O-CA-1",
        "VOI-CA-1",
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );
    add_open_order_to_cache(
        &cache,
        "O-CA-2",
        "VOI-CA-2",
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );

    let cmd = CancelAllOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(*AX_CLIENT_ID),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        order_side: None,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    client
        .cancel_all_orders(cmd)
        .expect("cancel_all_orders should not return an error");

    wait_until_async(
        || async {
            state
                .cancel_all_count
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    // A whole-request failure must not become one rejection per order
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await;
    assert!(
        !matches!(
            result,
            Ok(Some(ExecutionEvent::Order(OrderEventAny::CancelRejected(
                _
            ))))
        ),
        "expected no CancelRejected for whole-request failure, was {result:?}",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_emits_one_ws_cancel_per_entry() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    add_open_order_to_cache(
        &cache,
        "O-BC-1",
        "VOI-BC-1",
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );
    add_open_order_to_cache(
        &cache,
        "O-BC-2",
        "VOI-BC-2",
        instrument_id,
        OrderSide::Buy,
        "S-001",
    );

    let cancels = vec![
        CancelOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: Some(*AX_CLIENT_ID),
            strategy_id: StrategyId::from("S-001"),
            instrument_id,
            client_order_id: ClientOrderId::from("O-BC-1"),
            venue_order_id: Some(VenueOrderId::new("VOI-BC-1")),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
            correlation_id: None,
            causation_id: None,
        },
        CancelOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: Some(*AX_CLIENT_ID),
            strategy_id: StrategyId::from("S-001"),
            instrument_id,
            client_order_id: ClientOrderId::from("O-BC-2"),
            venue_order_id: Some(VenueOrderId::new("VOI-BC-2")),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
            correlation_id: None,
            causation_id: None,
        },
    ];

    let cmd = BatchCancelOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(*AX_CLIENT_ID),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        cancels,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    client
        .batch_cancel_orders(cmd)
        .expect("batch_cancel_orders should not error");

    // AxWsCancelOrder serializes `t` as "x" (CancelOrder request type).
    wait_until_async(
        || async {
            let messages = state.get_messages().await;
            messages
                .iter()
                .filter(|m| m.get("t").and_then(|v| v.as_str()) == Some("x"))
                .count()
                >= 2
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    {
        let messages = state.get_messages().await;
        let cancels: Vec<String> = messages
            .iter()
            .filter(|m| m.get("t").and_then(|v| v.as_str()) == Some("x"))
            .filter_map(|m| m.get("oid").and_then(|v| v.as_str()).map(str::to_string))
            .collect();

        assert!(
            cancels.contains(&"VOI-BC-1".to_string()),
            "cancels={cancels:?}"
        );
        assert!(
            cancels.contains(&"VOI-BC-2".to_string()),
            "cancels={cancels:?}"
        );
    }

    client.disconnect().await.expect("Failed to disconnect");
}

fn make_submit_order_cmd(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        order,
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

fn drain_rx(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>) {
    while rx.try_recv().is_ok() {}
}

#[rstest]
#[case(OrderType::StopMarket)]
#[case(OrderType::StopLimit)]
#[tokio::test]
async fn test_submit_order_denies_unsupported_order_type(#[case] order_type: OrderType) {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-UNSUPP");
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10"))
        .trigger_price(Price::from("50000.00"))
        .time_in_force(TimeInForce::Gtc);

    if order_type == OrderType::StopLimit {
        builder.price(Price::from("50001.00"));
    }
    let order = builder.build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout waiting for denial")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::Denied(denied)) => {
            assert_eq!(denied.client_order_id, client_order_id);
            assert!(
                denied.reason.contains("Unsupported order type"),
                "reason was: {}",
                denied.reason
            );
        }
        other => panic!("expected OrderDenied, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denies_gtd_time_in_force() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-GTD");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .price(Price::from("50000.00"))
        .quantity(Quantity::from("10"))
        .time_in_force(TimeInForce::Gtd)
        .expire_time(UnixNanos::from(2_000_000_000_000_000_000u64))
        .build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout waiting for denial")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::Denied(denied)) => {
            assert_eq!(denied.client_order_id, client_order_id);
            assert!(
                denied.reason.contains("Unsupported time in force"),
                "reason was: {}",
                denied.reason
            );
        }
        other => panic!("expected OrderDenied, was {other:?}"),
    }
}

#[rstest]
#[case(OrderType::Market, TimeInForce::Ioc)]
#[case(OrderType::Limit, TimeInForce::Gtc)]
#[tokio::test]
async fn test_submit_order_denies_reduce_only(
    #[case] order_type: OrderType,
    #[case] time_in_force: TimeInForce,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from(format!("O-REDUCE-{order_type}"));
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .time_in_force(time_in_force)
        .reduce_only(true);

    if order_type == OrderType::Limit {
        builder.price(Price::from("1.0000"));
    }
    let order = builder.build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout waiting for denial")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
        panic!("expected OrderDenied, was {event:?}");
    };

    assert_eq!(denied.client_order_id, client_order_id);
    assert_eq!(denied.reason, "AX does not support reduce-only orders");
    assert_eq!(
        state
            .preview_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
    );
    assert!(state.get_messages().await.is_empty());

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[case(
    "quote_quantity",
    OrderType::Market,
    TimeInForce::Ioc,
    "UNSUPPORTED_QUOTE_QUANTITY"
)]
#[case(
    "display_qty",
    OrderType::Limit,
    TimeInForce::Gtc,
    "Architect AX adapter cannot encode display_qty iceberg instructions"
)]
#[tokio::test]
async fn test_submit_order_denies_unsupported_instruction(
    #[case] instruction: &str,
    #[case] order_type: OrderType,
    #[case] time_in_force: TimeInForce,
    #[case] expected_reason: &str,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from(format!("O-UNSUPPORTED-{instruction}"));
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .time_in_force(time_in_force);

    match instruction {
        "quote_quantity" => {
            builder.quote_quantity(true);
        }
        "display_qty" => {
            builder
                .price(Price::from("1.0000"))
                .display_qty(Quantity::from("50"));
        }
        _ => unreachable!(),
    }
    let order = builder.build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout waiting for denial")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
        panic!("expected OrderDenied, was {event:?}");
    };

    assert_eq!(denied.client_order_id, client_order_id);
    assert_eq!(denied.reason.as_str(), expected_reason);
    assert_eq!(
        state
            .preview_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
    );
    assert!(state.get_messages().await.is_empty());

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_emulated_market_denies_original_display_quantity() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-EMULATED-DISPLAY");
    let original_order = OrderTestBuilder::new(OrderType::StopMarket)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .time_in_force(TimeInForce::Ioc)
        .trigger_price(Price::from("1.10"))
        .trigger_type(TriggerType::LastPrice)
        .display_qty(Quantity::from("50"))
        .emulation_trigger(TriggerType::LastPrice)
        .build();
    let submit_command = make_submit_order_cmd(&original_order);
    let transformed_order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .time_in_force(TimeInForce::Ioc)
        .build();
    cache
        .borrow_mut()
        .add_order(transformed_order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(submit_command)
        .expect("submit_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout waiting for denial")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
        panic!("expected OrderDenied, was {event:?}");
    };

    assert_eq!(denied.client_order_id, client_order_id);
    assert!(denied.reason.contains("display_qty"));
    assert_eq!(
        state
            .preview_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
    );
    assert!(state.get_messages().await.is_empty());

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_post_only_order_uses_default_rejection_policy() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    let client_order_id = ClientOrderId::from("O-POST-ONLY");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .price(Price::from("1.1247"))
        .time_in_force(TimeInForce::Gtc)
        .post_only(true)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();
    client.submit_order(make_submit_order_cmd(&order)).unwrap();
    wait_until_async(
        || async { state.get_messages().await.iter().any(|m| m["t"] == "p") },
        Duration::from_secs(5),
    )
    .await;
    let messages = state.get_messages().await;
    let place = messages.iter().find(|m| m["t"] == "p").unwrap();
    let cid = nautilus_architect_ax::common::parse::client_order_id_to_cid(&client_order_id);

    assert_eq!(
        place,
        &serde_json::json!({
            "rid": 1,
            "t": "p",
            "s": "EURUSD-PERP",
            "d": "B",
            "q": 100,
            "p": "1.1247",
            "tif": "GTC",
            "po": true,
            "tag": "Nautilus",
            "cid": cid,
        }),
    );
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_uses_preview_price() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MKT-OK");
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .time_in_force(TimeInForce::Ioc)
        .build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    wait_until_async(
        || async {
            let messages = state.get_messages().await;
            messages
                .iter()
                .any(|m| m.get("t").and_then(|v| v.as_str()) == Some("p"))
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    {
        let messages = state.get_messages().await;
        let place = messages
            .into_iter()
            .find(|m| m.get("t").and_then(|v| v.as_str()) == Some("p"))
            .unwrap();

        assert_eq!(place.get("s").and_then(|v| v.as_str()), Some("EURUSD-PERP"));
        assert_eq!(place.get("q").and_then(|v| v.as_i64()), Some(100));
        // The preview returns "50001.00" but EURUSD-PERP has a 0.0001 tick, so the take-through
        // price carries the instrument's precision rather than the venue string's
        assert_eq!(place.get("p").and_then(|v| v.as_str()), Some("50001.0000"));
        // Market orders route as IOC
        assert_eq!(place.get("tif").and_then(|v| v.as_str()), Some("IOC"));
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_query_order_uses_venue_id_from_place_response() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-QUERY-CACHED-OID");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .price(Price::from("50000.00"))
        .quantity(Quantity::from("100"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();
    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    wait_until_async(
        || async {
            state
                .get_messages()
                .await
                .iter()
                .any(|message| message.get("t").and_then(|value| value.as_str()) == Some("p"))
        },
        std::time::Duration::from_secs(5),
    )
    .await;

    let request_id = state
        .get_messages()
        .await
        .into_iter()
        .find(|message| message.get("t").and_then(|value| value.as_str()) == Some("p"))
        .and_then(|message| message.get("rid").and_then(|value| value.as_i64()))
        .expect("place request should contain rid");
    let expected_venue_order_id = format!("order-{request_id}");

    let mut venue_query = None;

    for attempt in 0..10 {
        client
            .query_order(QueryOrder::new(
                TraderId::from("TESTER-001"),
                Some(*AX_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                client_order_id,
                Some(VenueOrderId::from("STALE-CALLER-ID")),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("query_order should not error");

        wait_until_async(
            || async { state.order_status_queries.lock().await.len() > attempt },
            std::time::Duration::from_secs(5),
        )
        .await;

        venue_query = state
            .order_status_queries
            .lock()
            .await
            .get(attempt)
            .filter(|query| query.order_id.is_some())
            .cloned();

        if venue_query.is_some() {
            break;
        }

        tokio::task::yield_now().await;
    }

    let venue_query = venue_query.expect("query should use venue order ID after place response");
    assert_eq!(
        venue_query.order_id.as_deref(),
        Some(expected_venue_order_id.as_str())
    );
    assert_eq!(venue_query.client_order_id, None);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[case::empty_liquidity(true, false, "No liquidity available")]
#[case::preview_request_failure(false, true, "Failed to preview aggressive limit order")]
#[tokio::test]
async fn test_submit_market_order_rejects_before_placement(
    #[case] preview_empty: bool,
    #[case] preview_fail: bool,
    #[case] expected_reason: &str,
) {
    let (addr, state) = start_test_server().await.unwrap();
    state
        .preview_empty
        .store(preview_empty, std::sync::atomic::Ordering::Relaxed);
    state
        .preview_fail
        .store(preview_fail, std::sync::atomic::Ordering::Relaxed);
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-MKT-EMPTY");
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .time_in_force(TimeInForce::Ioc)
        .build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    // First event: OrderSubmitted emitted synchronously
    let submitted = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for submitted")
        .expect("channel closed");
    assert!(
        matches!(
            submitted,
            ExecutionEvent::Order(OrderEventAny::Submitted(_))
        ),
        "expected OrderSubmitted, was {submitted:?}",
    );

    let rejected = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for rejected")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = rejected else {
        panic!("expected OrderRejected, was {rejected:?}");
    };

    let messages = state.get_messages().await;

    assert_eq!(rejected.client_order_id, client_order_id);
    assert!(rejected.reason.contains(expected_reason));
    assert!(
        !messages
            .iter()
            .any(|m| m.get("t").and_then(|v| v.as_str()) == Some("p")),
        "expected no WS place_order message, was {messages:?}",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_skips_closed_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-CLOSED");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .price(Price::from("50000.00"))
        .quantity(Quantity::from("10"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    let rejected = OrderRejected::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        AccountId::from("AX-001"),
        ustr::Ustr::from("pre-rejected by test"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        false,
    );
    order
        .apply(OrderEventAny::Rejected(rejected))
        .expect("apply rejected");

    assert!(order.is_closed());

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    // No event should be emitted for a closed order
    let result = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
    assert!(
        result.is_err(),
        "expected no event for closed order, found {result:?}",
    );
}

#[rstest]
#[case::all(None, None)]
#[case::instrument(Some("EURUSD-PERP.AX"), None)]
#[case::venue(None, Some("O-01ARZ3NDEKTSV4RRFFQ69G5FAV"))]
#[case::missing(Some("UNKNOWN.AX"), None)]
#[tokio::test]
async fn test_owned_reports_match_inline(
    #[case] instrument: Option<&str>,
    #[case] venue_order: Option<&str>,
) {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    let instrument_id = instrument.map(InstrumentId::from);
    let venue_order_id = venue_order.map(VenueOrderId::from);
    let ts_init = UnixNanos::from(123);
    let single = GenerateOrderStatusReportBuilder::default()
        .ts_init(ts_init)
        .instrument_id(instrument_id)
        .venue_order_id(Some(
            venue_order_id.unwrap_or_else(|| VenueOrderId::from("O-01ARZ3NDEKTSV4RRFFQ69G5FAV")),
        ))
        .build()
        .unwrap();
    let orders = GenerateOrderStatusReportsBuilder::default()
        .ts_init(ts_init)
        .open_only(false)
        .instrument_id(instrument_id)
        .build()
        .unwrap();
    // The fixture fills fall outside the default seven-day lookback
    let fills = GenerateFillReportsBuilder::default()
        .ts_init(ts_init)
        .instrument_id(instrument_id)
        .venue_order_id(venue_order_id)
        .start(Some(UnixNanos::from(1_705_276_800_000_000_000)))
        .end(Some(UnixNanos::from(1_705_363_200_000_000_000)))
        .build()
        .unwrap();
    let positions = GeneratePositionStatusReportsBuilder::default()
        .ts_init(ts_init)
        .instrument_id(instrument_id)
        .build()
        .unwrap();

    let before = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

    if instrument == Some("UNKNOWN.AX") {
        let inline = client
            .generate_order_status_report(&single)
            .await
            .unwrap_err();
        let owned = collect_report_task(client.generate_order_status_report_task(&single).unwrap())
            .await
            .unwrap_err();
        assert_eq!(
            inline.to_string(),
            "AX order report instrument does not match requested instrument"
        );
        assert_eq!(owned.to_string(), inline.to_string());
    } else {
        let inline = client.generate_order_status_report(&single).await.unwrap();
        let task = collect_report_task(client.generate_order_status_report_task(&single).unwrap())
            .await
            .unwrap();
        assert!(inline.is_some());
        assert_reports_equal(
            inline.into_iter().collect(),
            task.into_iter().collect(),
            before,
        );
    }

    let inline = client.generate_order_status_reports(&orders).await.unwrap();
    let task = collect_report_task(client.generate_order_status_reports_task(&orders).unwrap())
        .await
        .unwrap();
    assert_eq!(
        inline.len(),
        if instrument == Some("UNKNOWN.AX") {
            0
        } else if instrument.is_some() {
            1
        } else {
            2
        }
    );
    assert_reports_equal(inline, task, before);
    let inline = client.generate_fill_reports(fills.clone()).await.unwrap();
    let task = collect_report_task(client.generate_fill_reports_task(&fills).unwrap())
        .await
        .unwrap();
    assert_eq!(
        inline.len(),
        if instrument == Some("UNKNOWN.AX") {
            0
        } else if instrument.is_some() || venue_order.is_some() {
            1
        } else {
            2
        }
    );
    assert_reports_equal(inline, task, before);
    let inline = client
        .generate_position_status_reports(&positions)
        .await
        .unwrap();
    let task = collect_report_task(
        client
            .generate_position_status_reports_task(&positions)
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(inline.len(), if instrument.is_some() { 0 } else { 2 });
    assert_reports_equal(inline, task, before);
    client.disconnect().await.unwrap();
}

async fn collect_report_task<T>(task: ExecutionReportTask<T>) -> anyhow::Result<T> {
    tokio::spawn(task.collection).await?;
    (task.finish)()
}

fn assert_reports_equal<T: serde::Serialize>(inline: Vec<T>, owned: Vec<T>, before: UnixNanos) {
    let after = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    let mut inline = serde_json::to_value(inline).unwrap();
    let mut owned = serde_json::to_value(owned).unwrap();
    // Only generated metadata differs between requests; compare every other field exactly
    for reports in [&mut inline, &mut owned] {
        for report in reports.as_array_mut().unwrap() {
            let fields = report.as_object_mut().unwrap();
            let ts_init: UnixNanos =
                serde_json::from_value(fields.remove("ts_init").unwrap()).unwrap();
            assert!((before..=after).contains(&ts_init));
            let _: UUID4 = serde_json::from_value(fields.remove("report_id").unwrap()).unwrap();
        }
    }

    assert_eq!(owned, inline);
}

#[rstest]
#[case::default("default", "1.1247", OrderStatus::Rejected, "1.1247", 0)]
#[case::reject("reject", "1.1248", OrderStatus::Rejected, "1.1248", 0)]
#[case::back_off("back_off", "1.1247", OrderStatus::Filled, "1.1237", 100)]
#[case::top_of_book("top_of_book", "1.1247", OrderStatus::Accepted, "1.1222", 0)]
#[tokio::test]
async fn test_post_only_crossing_reports(
    #[case] case: &str,
    #[case] submitted_price: &str,
    #[case] status: OrderStatus,
    #[case] reported_price: &str,
    #[case] filled_qty: u64,
    #[values(false, true)] open_only: bool,
) {
    let capture = load_test_data("captured/post-only-crossing.json");
    let capture = &capture[case];
    let detail = &capture["history"]["orders"][0];
    let (addr, state) = start_test_server().await.unwrap();
    *state.orders_payload.lock().await = Some(capture["history"].clone());
    *state.open_orders_payload.lock().await = Some(capture["open_orders"].clone());
    *state.fills_payload.lock().await = Some(serde_json::json!({"fills": []}));
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    let account_id = AccountId::from("AX-001");
    add_test_account_to_cache(&cache, account_id);
    client.start().unwrap();
    client.connect().await.unwrap();
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(open_only)
        .build()
        .unwrap();
    let before = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let ts_event = UnixNanos::from(
        detail["ts"].as_u64().unwrap() * 1_000_000_000 + detail["tn"].as_u64().unwrap(),
    );
    let price = Price::from(reported_price);
    let quantity = Quantity::from(100);
    let filled_qty = Quantity::from(filled_qty);
    let mut expected = OrderStatusReport::new(
        account_id,
        InstrumentId::from("EURUSD-PERP.AX"),
        None,
        VenueOrderId::from(detail["oid"].as_str().unwrap()),
        Some(OrderSide::Buy),
        OrderType::Limit,
        TimeInForce::Gtc,
        status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        before,
        None,
    )
    .with_price(price)
    .with_post_only(true);

    if status == OrderStatus::Rejected {
        expected = expected.with_cancel_reason("post-only order would cross the book".to_string());
    }

    assert_eq!(capture["request"]["p"], submitted_price);

    let expected = if open_only && status != OrderStatus::Accepted {
        vec![]
    } else {
        vec![expected]
    };

    assert_reports_equal(reports, expected, before);
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::complete(false)]
#[case::cancel(true)]
#[tokio::test(flavor = "current_thread")]
async fn test_owned_report_delay_allows_core_progress(#[case] cancel: bool) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    state
        .order_report_response_delay
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let client = Rc::new(RefCell::new(client));
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::from(321))
        .open_only(false)
        .build()
        .unwrap();
    let task = client
        .borrow()
        .generate_order_status_reports_task(&command)
        .unwrap();

    let worker = tokio::spawn(task.collection);
    tokio::time::timeout(
        Duration::from_secs(5),
        state.order_report_response_entered.notified(),
    )
    .await
    .unwrap();
    // An owned worker leaves the non-Send client and live cache available to the core
    client.borrow_mut().register_external_order(
        ClientOrderId::from("CORE-PROGRESS"),
        VenueOrderId::from("CORE-VENUE"),
        InstrumentId::from("EURUSD-PERP.AX"),
        StrategyId::from("CORE-001"),
        UnixNanos::from(456),
    );
    cache
        .borrow_mut()
        .add_instrument(crate::common::server::create_test_instrument("CORE-PERP"))
        .unwrap();

    if cancel {
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        let error = (task.finish)().unwrap_err();
        assert_eq!(
            error.to_string(),
            "report collection stopped without a result"
        );
    } else {
        state.order_report_response_release.notify_one();
        worker.await.unwrap();
        let reports = (task.finish)().unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].account_id, AccountId::from("AX-001"));
    }

    assert!(
        cache
            .borrow()
            .instrument(&InstrumentId::from("CORE-PERP.AX"))
            .is_some()
    );
    let mut client = Rc::try_unwrap(client).unwrap().into_inner();
    client.disconnect().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_mass_status_timestamp_precedes_delayed_collection() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    state
        .order_report_response_delay
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let clock = nautilus_core::time::get_atomic_clock_realtime();
    let before = clock.get_time_ns();
    let mut mass = Box::pin(client.generate_mass_status(Some(60)));
    tokio::select! {
        result = &mut mass => panic!("mass status finished before release: {result:?}"),
        () = state.order_report_response_entered.notified() => {},
        () = tokio::time::sleep(Duration::from_secs(5)) => panic!("report request did not start"),
    }
    let entered = clock.get_time_ns();
    state.order_report_response_release.notify_one();
    let report = mass.await.unwrap().unwrap();
    assert!((before..=entered).contains(&report.ts_init));
    assert_eq!(
        report.lookback_start(),
        Some(UnixNanos::from(report.ts_init.as_u64() - 3_600_000_000_000))
    );
    assert_eq!(report.client_id, *AX_CLIENT_ID);
    assert_eq!(report.account_id, AccountId::from("AX-001"));
    assert_eq!(report.venue, *AX_VENUE);
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::single(0)]
#[case::bulk(1)]
#[case::fills(2)]
#[case::positions(3)]
#[tokio::test]
async fn test_owned_report_failure_matches_inline(#[case] kind: u8) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    set_uncached_mass_status_payload(&state).await;
    let mut status = load_test_data("http_get_order_status.json");
    status["status"]["order_id"] = "OID-UNCACHED".into();
    *state.order_status_response.lock().await = Some((200, status));
    *state.open_orders_payload.lock().await = state.orders_payload.lock().await.clone();
    state
        .instrument_fail
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let now = UnixNanos::from(123);

    let (inline, owned) = match kind {
        0 => {
            let command = GenerateOrderStatusReportBuilder::default()
                .ts_init(now)
                .venue_order_id(Some(VenueOrderId::from("OID-UNCACHED")))
                .build()
                .unwrap();
            (
                client
                    .generate_order_status_report(&command)
                    .await
                    .map(|_| ()),
                collect_report_task(client.generate_order_status_report_task(&command).unwrap())
                    .await
                    .map(|_| ()),
            )
        }
        1 => {
            let command = GenerateOrderStatusReportsBuilder::default()
                .ts_init(now)
                .open_only(false)
                .build()
                .unwrap();
            (
                client
                    .generate_order_status_reports(&command)
                    .await
                    .map(|_| ()),
                collect_report_task(client.generate_order_status_reports_task(&command).unwrap())
                    .await
                    .map(|_| ()),
            )
        }
        2 => {
            let command = GenerateFillReportsBuilder::default()
                .ts_init(now)
                .start(Some(UnixNanos::from(1_704_067_200_000_000_000)))
                .end(Some(UnixNanos::from(1_704_153_600_000_000_000)))
                .build()
                .unwrap();
            (
                client
                    .generate_fill_reports(command.clone())
                    .await
                    .map(|_| ()),
                collect_report_task(client.generate_fill_reports_task(&command).unwrap())
                    .await
                    .map(|_| ()),
            )
        }
        3 => {
            let command = GeneratePositionStatusReportsBuilder::default()
                .ts_init(now)
                .build()
                .unwrap();
            (
                client
                    .generate_position_status_reports(&command)
                    .await
                    .map(|_| ()),
                collect_report_task(
                    client
                        .generate_position_status_reports_task(&command)
                        .unwrap(),
                )
                .await
                .map(|_| ()),
            )
        }
        _ => unreachable!(),
    };

    let inline = format!("{:#}", inline.unwrap_err());
    let owned = format!("{:#}", owned.unwrap_err());
    assert_eq!(owned, inline);
    assert!(owned.contains("instrument unavailable"), "{owned}");
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::venue(true)]
#[case::client(false)]
#[tokio::test]
async fn test_single_closed_order_lookup(#[case] by_venue: bool) {
    let (addr, state) = start_test_server().await.unwrap();
    let order_id = "O-01ARZ3NDEKTSV4RRFFQ69G5FAV";
    *state.open_orders_payload.lock().await = Some(serde_json::json!({"orders": []}));
    *state.orders_payload.lock().await = Some(serde_json::json!({
        "orders": [create_historical_order(order_id, "CANCELED", 1_704_067_200)]
    }));
    let mut status = load_test_data("http_get_order_status.json");
    status["status"]["state"] = "CANCELED".into();
    *state.order_status_response.lock().await = Some((200, status));
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    let coid = ClientOrderId::from("O-STRATEGY-123");
    let command = GenerateOrderStatusReportBuilder::default()
        .ts_init(UnixNanos::default())
        .client_order_id(Some(coid))
        .venue_order_id(by_venue.then(|| VenueOrderId::from(order_id)))
        .build()
        .unwrap();
    let report = client
        .generate_order_status_report(&command)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.venue_order_id, VenueOrderId::from(order_id));
    assert_eq!(report.client_order_id, Some(coid));
    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.quantity.as_decimal(), dec!(100));
    assert_eq!(report.filled_qty.as_decimal(), dec!(0));
    let queries = state.order_status_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].order_id.as_deref(), by_venue.then_some(order_id));
    assert_eq!(
        queries[0].client_order_id,
        (!by_venue).then(|| nautilus_architect_ax::common::parse::client_order_id_to_cid(&coid))
    );
    drop(queries);
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::absent(404, serde_json::json!({"error": "no matching orders"}), true)]
#[case::other_not_found(404, serde_json::json!({"error": "unknown route"}), false)]
#[case::failed(400, serde_json::json!({"error": "lookup failed"}), false)]
#[case::decode(200, serde_json::json!({"status": {}}), false)]
#[tokio::test]
async fn test_single_order_absence_requires_authoritative_response(
    #[case] status: u16,
    #[case] body: serde_json::Value,
    #[case] absent: bool,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.order_status_response.lock().await = Some((status, body));
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    let command = GenerateOrderStatusReportBuilder::default()
        .ts_init(UnixNanos::default())
        .client_order_id(Some(ClientOrderId::from("O-MISSING")))
        .build()
        .unwrap();
    let result = client.generate_order_status_report(&command).await;

    if absent {
        assert!(result.unwrap().is_none());
    } else {
        assert!(result.is_err());
    }

    assert_eq!(state.order_status_queries.lock().await.len(), 1);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_bounded_history_keeps_old_active_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    let active = create_historical_order("OID-OLD", "ACCEPTED", 1_704_067_100);
    let mut pending = active.clone();
    pending["oid"] = "OID-PENDING-OLD".into();
    pending["o"] = "PENDING".into();
    let closed = create_historical_order("OID-CLOSED", "CANCELED", 1_704_067_201);
    *state.orders_payload.lock().await =
        Some(serde_json::json!({"orders": [active.clone(), pending.clone(), closed]}));
    *state.open_orders_payload.lock().await = Some(serde_json::json!({"orders": [active,pending]}));
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(false)
        .start(Some(UnixNanos::from(1_704_067_200_000_000_000)))
        .end(Some(UnixNanos::from(1_704_067_202_000_000_000)))
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let actual = reports
        .iter()
        .map(|r| (r.venue_order_id.as_str(), r.order_status))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            ("OID-CLOSED", OrderStatus::Canceled),
            ("OID-OLD", OrderStatus::Accepted),
            ("OID-PENDING-OLD", OrderStatus::Submitted)
        ]
    );
    let queries = state.orders_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(
        queries[0].start_timestamp_ns,
        Some(1_704_067_200_000_000_000)
    );
    drop(queries);
    assert_eq!(state.open_orders_queries.lock().await.len(), 1);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_single_order_missing_history_is_failure() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.orders_payload.lock().await = Some(serde_json::json!({"orders": []}));
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    let command = GenerateOrderStatusReportBuilder::default()
        .ts_init(UnixNanos::default())
        .venue_order_id(Some(VenueOrderId::from("O-01ARZ3NDEKTSV4RRFFQ69G5FAV")))
        .build()
        .unwrap();
    let error = client
        .generate_order_status_report(&command)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "AX order status exists but its complete history record is unavailable"
    );
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_single_closed_order_uses_latest_fill_timestamp() {
    let (addr, state) = start_test_server().await.unwrap();
    let clock = nautilus_core::time::get_atomic_clock_realtime();
    let now = clock.get_time_ns().as_u64();
    let created = (now / 1_000_000_000 - 10) * 1_000_000_000;
    let latest_fill = now - 2_000_000_000;
    let mut orders = load_test_data("http_get_orders_partial_canceled.json");
    orders["orders"][0]["ts"] = (created / 1_000_000_000).into();
    orders["orders"][0]["tn"] = 0.into();
    *state.orders_payload.lock().await = Some(orders);
    let mut fills = load_test_data("http_get_fills_partial_canceled.json");
    for (index, timestamp) in [latest_fill, now - 3_000_000_000, now - 1_000_000_000]
        .into_iter()
        .enumerate()
    {
        fills["fills"][index]["timestamp"] =
            jiff::Timestamp::from_nanosecond(i128::from(timestamp))
                .unwrap()
                .to_string()
                .into();
    }

    *state.fills_payload.lock().await = Some(fills);
    let mut status = load_test_data("http_get_order_status.json");
    status["status"]["order_id"] = "O-PARTIAL-CANCELED".into();
    status["status"]["state"] = "CANCELED".into();
    *state.order_status_response.lock().await = Some((200, status));
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    let account_id = AccountId::from("AX-001");
    add_test_account_to_cache(&cache, account_id);
    client.start().unwrap();
    client.connect().await.unwrap();
    let venue_order_id = VenueOrderId::from("O-PARTIAL-CANCELED");
    let command = GenerateOrderStatusReportBuilder::default()
        .ts_init(UnixNanos::default())
        .venue_order_id(Some(venue_order_id))
        .build()
        .unwrap();
    let before = clock.get_time_ns();
    let report = client
        .generate_order_status_report(&command)
        .await
        .unwrap()
        .unwrap();
    let price = Price::from("1.0840");
    let quantity = Quantity::from("200");
    let filled_qty = Quantity::from("100");
    let expected = OrderStatusReport::new(
        account_id,
        InstrumentId::from("EURUSD-PERP.AX"),
        Some(ClientOrderId::from("CID-42")),
        venue_order_id,
        Some(OrderSide::Buy),
        OrderType::Limit,
        TimeInForce::Ioc,
        OrderStatus::Canceled,
        quantity,
        filled_qty,
        UnixNanos::from(created),
        UnixNanos::from(latest_fill),
        before,
        None,
    )
    .with_price(price)
    .with_post_only(false);
    assert_reports_equal(vec![report], vec![expected], before);
    assert_eq!(state.fills_queries.lock().await.len(), 1);
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::http_first(false, 1)]
#[case::stream_first(true, 1)]
#[case::overlapping_modifies(true, 2)]
#[tokio::test]
async fn test_post_only_replace_rejection_preserves_parent(
    #[case] stream_first: bool,
    #[case] attempts: usize,
) {
    let (addr, state) = start_test_server().await.unwrap();
    state
        .replace_order_response_delay
        .store(stream_first, std::sync::atomic::Ordering::Relaxed);
    *state.replace_order_oid.lock().await = Some("PROPOSED-OID".to_string());
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    drain_rx(&mut rx);
    let client_order_id = ClientOrderId::from("O-POST-ONLY-MOD");
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let strategy_id = StrategyId::from("S-001");
    let parent = VenueOrderId::from("PARENT-OID");
    client.register_external_order(
        client_order_id,
        parent,
        instrument_id,
        strategy_id,
        UnixNanos::default(),
    );
    let mut local_order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .price(Price::from("1.1230"))
        .time_in_force(TimeInForce::Gtc)
        .post_only(true)
        .build();
    let submitted = TestOrderEventStubs::submitted(&local_order, AccountId::from("AX-001"));
    local_order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&local_order, AccountId::from("AX-001"), parent);
    local_order.apply(accepted).unwrap();
    local_order
        .apply(OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
            TraderId::from("TESTER-001"),
            strategy_id,
            instrument_id,
            client_order_id,
            Some(AccountId::from("AX-001")),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(parent),
        )))
        .unwrap();
    let capture = load_test_data("captured/post-only-replace-rejected.json");

    for _ in 0..attempts {
        client
            .modify_order(ModifyOrder::new(
                TraderId::from("TESTER-001"),
                Some(*AX_CLIENT_ID),
                strategy_id,
                instrument_id,
                client_order_id,
                Some(parent),
                None,
                Some(Price::from("1.1252")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    }

    wait_until_async(
        || async {
            state
                .replace_order_count
                .load(std::sync::atomic::Ordering::Relaxed)
                == attempts
        },
        Duration::from_secs(5),
    )
    .await;

    if !stream_first {
        state.replace_order_response_sent.notified().await;
    }

    for _ in 0..attempts {
        state
            .order_injected_events
            .lock()
            .await
            .push(capture["events"][1].clone());
    }

    for _ in 0..attempts {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event else {
            panic!("expected modify rejection, was {event:?}");
        };

        assert_eq!(rejected.trader_id, TraderId::from("TESTER-001"));
        assert_eq!(rejected.strategy_id, strategy_id);
        assert_eq!(rejected.instrument_id, instrument_id);
        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(rejected.venue_order_id, Some(parent));
        assert_eq!(rejected.account_id, Some(AccountId::from("AX-001")));
        assert_eq!(rejected.reason, "post-only order would cross the book");
        assert!(!rejected.reconciliation);
        local_order
            .apply(OrderEventAny::ModifyRejected(rejected))
            .unwrap();
        assert_eq!(local_order.status(), OrderStatus::Accepted);
        assert_eq!(local_order.venue_order_id(), Some(parent));
        assert_eq!(local_order.price(), Some(Price::from("1.1230")));
    }

    if stream_first {
        for _ in 0..attempts {
            state.replace_order_response_release.notify_one();
            state.replace_order_response_sent.notified().await;
        }
    }

    // A late HTTP receipt must not redirect the next query or cancel
    assert!(
        tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err()
    );
    client
        .query_order(QueryOrder::new(
            TraderId::from("TESTER-001"),
            Some(*AX_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            Some(parent),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .cancel_order(CancelOrder::new(
            TraderId::from("TESTER-001"),
            Some(*AX_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            Some(parent),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until_async(
        || async {
            !state.order_status_queries.lock().await.is_empty()
                && state
                    .get_messages()
                    .await
                    .iter()
                    .any(|message| message["t"] == "x")
        },
        Duration::from_secs(5),
    )
    .await;

    let queries = state.order_status_queries.lock().await.clone();
    let messages = state.get_messages().await;
    let cancels: Vec<_> = messages
        .iter()
        .filter(|message| message["t"] == "x")
        .collect();
    client.disconnect().await.unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].order_id.as_deref(), Some("PARENT-OID"));
    assert_eq!(cancels.len(), 1);
    assert_eq!(cancels[0]["oid"], "PARENT-OID");
}

#[rstest]
#[case::missing_stream(true, false, false, "ACCEPTED", Some(OrderStatus::Accepted), false)]
#[case::late_stream(true, true, false, "ACCEPTED", Some(OrderStatus::Accepted), false)]
#[case::missing_child(false, true, false, "ACCEPTED", None, false)]
#[case::parent_changes_during_query(
    true,
    false,
    true,
    "ACCEPTED",
    Some(OrderStatus::Accepted),
    false
)]
#[case::filled(true, false, false, "FILLED", Some(OrderStatus::Filled), false)]
#[case::canceled(true, false, false, "CANCELED", Some(OrderStatus::Canceled), false)]
#[case::expired(true, false, false, "EXPIRED", Some(OrderStatus::Expired), false)]
#[case::rejected_proposal(true, true, false, "REJECTED", None, false)]
#[case::retired_proposal(true, true, false, "REPLACED", None, false)]
#[case::report_then_reconnect(true, false, false, "ACCEPTED", Some(OrderStatus::Accepted), true)]
#[tokio::test]
async fn test_query_replacement_preserves_child(
    #[case] child_found: bool,
    #[case] late_stream: bool,
    #[case] changing_parent: bool,
    #[case] candidate_state: &str,
    #[case] expected: Option<OrderStatus>,
    #[case] reconnect: bool,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let mut replacement = load_test_data("ws_order_replaced_live.json");
    let parent = VenueOrderId::from(replacement["ro"]["oid"].as_str().unwrap());
    let proposed = VenueOrderId::from(replacement["noid"].as_str().unwrap());
    let mut candidate_order = replacement["no"].clone();

    let filled = if expected == Some(OrderStatus::Filled) {
        100
    } else {
        0
    };

    let remaining = if candidate_state == "ACCEPTED" {
        100
    } else {
        0
    };

    candidate_order["o"] = candidate_state.into();
    candidate_order["xq"] = filled.into();
    candidate_order["rq"] = remaining.into();

    if child_found && expected.is_none() {
        replacement["noid"] = "OID-ACTUAL-CHILD".into();
        replacement["no"]["oid"] = "OID-ACTUAL-CHILD".into();
        replacement["no"]["p"] = "1.0927".into();
    }

    let child = VenueOrderId::from(replacement["noid"].as_str().unwrap());
    let expected_price = Price::from(replacement["no"]["p"].as_str().unwrap());
    *state.replace_order_oid.lock().await = Some(proposed.to_string());
    let mut parent_status = load_test_data("captured/order-status-replaced.json");
    parent_status["status"]["order_id"] = parent.to_string().into();
    let mut child_status = load_test_data("http_get_order_status.json");
    child_status["status"]["order_id"] = proposed.to_string().into();
    child_status["status"]["state"] = candidate_state.into();
    child_status["status"]["filled_quantity"] = filled.into();
    child_status["status"]["remaining_quantity"] = remaining.into();
    if changing_parent {
        let mut accepted = child_status.clone();
        accepted["status"]["order_id"] = parent.to_string().into();
        state
            .order_status_responses
            .lock()
            .await
            .push((parent.to_string(), 200, accepted));
    }

    state.order_status_responses.lock().await.extend(vec![
        (parent.to_string(), 200, parent_status),
        (
            proposed.to_string(),
            if child_found { 200 } else { 404 },
            if child_found {
                child_status
            } else {
                serde_json::json!({"error":"no matching orders"})
            },
        ),
    ]);
    *state.orders_payload.lock().await =
        Some(serde_json::json!({"orders":[replacement["ro"].clone(), candidate_order]}));
    *state.fills_payload.lock().await = Some(serde_json::json!({"fills":[]}));
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    drain_rx(&mut rx);
    let client_order_id = ClientOrderId::from("O-QUERY-REPLACE");
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let strategy_id = StrategyId::from("S-001");
    client.register_external_order(
        client_order_id,
        parent,
        instrument_id,
        strategy_id,
        UnixNanos::default(),
    );
    client
        .modify_order(ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*AX_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            Some(parent),
            None,
            Some(Price::from("1.0926")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    state.replace_order_response_sent.notified().await;

    if reconnect {
        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(instrument_id),
            Some(client_order_id),
            Some(parent),
            None,
            None,
        );
        let report = client
            .generate_order_status_report(&command)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(report.venue_order_id, parent);
        assert_eq!(report.order_status, OrderStatus::Canceled);
        client.disconnect().await.unwrap();
        client.connect().await.unwrap();
        drain_rx(&mut rx);
    }

    wait_until_async(
        || async {
            if state
                .order_status_queries
                .lock()
                .await
                .iter()
                .any(|query| query.order_id.as_deref() == Some(proposed.as_str()))
            {
                return true;
            }

            client
                .query_order(QueryOrder::new(
                    TraderId::from("TESTER-001"),
                    Some(*AX_CLIENT_ID),
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    Some(parent),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ))
                .unwrap();
            false
        },
        Duration::from_secs(5),
    )
    .await;

    if let Some(expected) = expected {
        let report = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.unwrap();

                let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
                    panic!("expected replacement report, was {event:?}");
                };

                if report.venue_order_id == child {
                    break report;
                }

                assert!(changing_parent);
                assert_eq!(report.venue_order_id, parent);
                assert_eq!(report.order_status, OrderStatus::Accepted);
            }
        })
        .await
        .unwrap();

        assert_eq!(report.account_id, AccountId::from("AX-001"));
        assert_eq!(report.client_order_id, Some(client_order_id));
        assert_eq!(report.venue_order_id, child);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.order_status, expected);
        assert_eq!(report.price, Some(expected_price));
        assert_eq!(report.quantity, Quantity::from("100"));
        assert_eq!(
            report.filled_qty,
            Quantity::from(filled.to_string().as_str())
        );
    }

    if late_stream {
        state.order_injected_events.lock().await.push(replacement);

        let update = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.unwrap();
                match event {
                    ExecutionEvent::Order(OrderEventAny::Updated(update)) => break update,
                    ExecutionEvent::Report(ExecutionReport::Order(report)) => {
                        assert_eq!(report.order_status, OrderStatus::Accepted);
                    }
                    other => panic!("unexpected event {other:?}"),
                }
            }
        })
        .await
        .unwrap();

        assert_eq!(update.client_order_id, client_order_id);
        assert_eq!(update.venue_order_id, Some(child));
        assert_eq!(update.price, Some(expected_price));
        assert_eq!(update.quantity, Quantity::from("100"));
    }

    let closed = expected.is_some_and(OrderStatus::is_closed);
    if !closed {
        client
            .cancel_order(CancelOrder::new(
                TraderId::from("TESTER-001"),
                Some(*AX_CLIENT_ID),
                strategy_id,
                instrument_id,
                client_order_id,
                Some(parent),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_until_async(
            || async {
                state
                    .get_messages()
                    .await
                    .iter()
                    .any(|message| message["t"] == "x")
            },
            Duration::from_secs(5),
        )
        .await;
    }

    let messages = state.get_messages().await;
    client.disconnect().await.unwrap();
    let cancels: Vec<_> = messages
        .iter()
        .filter(|message| message["t"] == "x")
        .collect();
    assert_eq!(cancels.len(), usize::from(!closed));

    if !closed {
        assert_eq!(cancels[0]["oid"], child.as_str());
    }

    while let Ok(event) = rx.try_recv() {
        if let ExecutionEvent::Report(ExecutionReport::Order(report)) = event {
            if report.venue_order_id == child {
                assert_eq!(
                    report.order_status,
                    expected.unwrap_or(OrderStatus::Accepted)
                );
            } else {
                assert!(closed);
                assert_eq!(report.venue_order_id, parent);
                assert_eq!(report.order_status, OrderStatus::Canceled);
            }
        } else {
            panic!("unexpected event {event:?}");
        }
    }
}

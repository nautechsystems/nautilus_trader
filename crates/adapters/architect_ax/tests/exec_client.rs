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

mod common;

use std::{cell::RefCell, net::SocketAddr, rc::Rc};

use nautilus_architect_ax::{
    common::enums::AxEnvironment, config::AxExecClientConfig, execution::AxExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReports, GeneratePositionStatusReports, ModifyOrder, QueryAccount,
            SubmitOrder,
        },
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::{AccountState, OrderAccepted, OrderEventAny, OrderRejected},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    orders::{LimitOrder, Order, OrderAny, builder::OrderTestBuilder},
    types::{AccountBalance, Money, Price, Quantity},
};
use rstest::rstest;

use crate::common::server::start_test_server;

fn setup_exec_channel() -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(sender);
    receiver
}

fn create_test_exec_config(addr: SocketAddr) -> AxExecClientConfig {
    AxExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
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
    let client_id = ClientId::from("AX");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("AX"),
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
    let config = AxExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        environment: AxEnvironment::Sandbox,
        ..Default::default()
    };

    assert_eq!(config.api_key, Some("test_api_key".to_string()));
    assert_eq!(config.environment, AxEnvironment::Sandbox);
    assert_eq!(config.trader_id, TraderId::from("TRADER-001"));
    assert_eq!(config.account_id, AccountId::from("AX-001"));
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), ClientId::from("AX"));
    assert_eq!(client.venue(), Venue::from("AX"));
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert_eq!(client.account_id(), AccountId::from("AX-001"));
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    // Pre-register account so await_account_registered succeeds
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    assert!(!client.is_connected());

    client.connect().await.expect("Failed to connect");
    assert!(client.is_connected());

    client.disconnect().await.expect("Failed to disconnect");
    assert!(!client.is_connected());
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
    let config = AxExecClientConfig {
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
    let config = AxExecClientConfig {
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
    let config = AxExecClientConfig {
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
        Some(ClientId::from("AX")),
        AccountId::from("AX-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
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
) {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let coid = ClientOrderId::from(client_order_id);

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        coid,
        OrderSide::Buy,
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

    let mut order_any: OrderAny = order.into();

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

    order_any
        .apply(OrderEventAny::Accepted(accepted))
        .expect("Failed to apply accepted");

    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();
    cache.borrow_mut().update_order(&order_any).unwrap();
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
    add_open_order_to_cache(&cache, "O-001", "VOI-001", instrument_id);
    add_open_order_to_cache(&cache, "O-002", "VOI-002", instrument_id);

    let cmd = CancelAllOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(ClientId::from("AX")),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        order_side: OrderSide::NoOrderSide,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
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
#[tokio::test]
async fn test_generate_order_status_reports_filters() {
    let (addr, state) = start_test_server().await.unwrap();

    // Replace default fixture with EURUSD + XAU orders across different states
    *state.open_orders_payload.lock().await = Some(serde_json::json!({
        "orders": [
            {
                "tn": 1704067200,
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
                "tn": 1704067201,
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
                "tn": 1704067202,
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
        ]
    }));

    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");
    drain_rx(&mut rx);

    // No filter -> all 3 reports
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
    assert_eq!(reports.len(), 3);

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

    // open_only -> drops the FILLED one
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
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.order_status.is_open()));

    // start cutoff that excludes the first order (ts=1704067200)
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
    assert_eq!(reports.len(), 2);

    client.disconnect().await.expect("Failed to disconnect");
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
                "user_id": "u"
            },
            {
                "trade_id": "T-2",
                "order_id": "OID-B",
                "fee": "0.20",
                "is_taker": false,
                "price": "2000.50",
                "quantity": 2,
                "side": "S",
                "symbol": "XAU-PERP",
                "timestamp": "2024-01-15T10:31:12Z",
                "user_id": "u"
            },
            {
                "trade_id": "T-3",
                "order_id": "OID-A",
                "fee": "0.15",
                "is_taker": true,
                "price": "1.08455",
                "quantity": 50,
                "side": "B",
                "symbol": "EURUSD-PERP",
                "timestamp": "2024-01-15T10:32:00Z",
                "user_id": "u"
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
async fn test_generate_position_status_reports_filters() {
    let (addr, state) = start_test_server().await.unwrap();

    *state.positions_payload.lock().await = Some(serde_json::json!({
        "positions": [
            {
                "user_id": "u",
                "symbol": "EURUSD-PERP",
                "signed_quantity": 100,
                "signed_notional": "108400.00",
                "timestamp": "2024-01-15T10:30:45Z",
                "realized_pnl": "0"
            },
            {
                "user_id": "u",
                "symbol": "XAU-PERP",
                "signed_quantity": -5,
                "signed_notional": "-10000.00",
                "timestamp": "2024-01-15T10:31:00Z",
                "realized_pnl": "0"
            },
            {
                "user_id": "u",
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

    // No filter -> flat position is skipped, yields 2
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
    assert_eq!(reports.len(), 2);

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
async fn test_modify_order_without_venue_order_id_emits_rejected() {
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
        Some(ClientId::from("AX")),
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
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout waiting for ModifyRejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(r)) => {
            assert_eq!(r.client_order_id, client_order_id);
            assert!(
                r.reason.as_str().contains("venue_order_id"),
                "reason was: {}",
                r.reason,
            );
        }
        other => panic!("expected OrderModifyRejected, was {other:?}"),
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_success_updates_caches() {
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
        Some(ClientId::from("AX")),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(old_venue_order_id),
        Some(Quantity::from("5")),
        Some(Price::from("50001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    // Wait for the mock to record the replace_order call
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

    while state
        .replace_order_count
        .load(std::sync::atomic::Ordering::Relaxed)
        == 0
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timeout waiting for /replace_order",
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

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
async fn test_modify_order_http_error_emits_rejected() {
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
        Some(ClientId::from("AX")),
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
    );

    client
        .modify_order(cmd)
        .expect("modify_order should not error");

    // Find the ModifyRejected among emitted events (may follow account state events)
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timeout waiting for ModifyRejected",
        );
        let Ok(Some(event)) =
            tokio::time::timeout(std::time::Duration::from_millis(250), rx.recv()).await
        else {
            continue;
        };

        if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(r)) = event {
            assert_eq!(r.client_order_id, client_order_id);
            assert!(
                r.reason.as_str().contains("modify-order-error"),
                "reason was: {}",
                r.reason,
            );
            break;
        }
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_http_failure_emits_cancel_rejected() {
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
    add_open_order_to_cache(&cache, "O-CA-1", "VOI-CA-1", instrument_id);
    add_open_order_to_cache(&cache, "O-CA-2", "VOI-CA-2", instrument_id);

    let cmd = CancelAllOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(ClientId::from("AX")),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        order_side: OrderSide::NoOrderSide,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    };

    client
        .cancel_all_orders(cmd)
        .expect("cancel_all_orders should not return an error");

    // Collect cancel-rejected events for both open orders
    let mut rejected: Vec<ClientOrderId> = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while rejected.len() < 2 && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(250), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::CancelRejected(r)))) => {
                rejected.push(r.client_order_id);
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {}
        }
    }

    rejected.sort_by_key(|cid| cid.to_string());
    let expected = vec![ClientOrderId::from("O-CA-1"), ClientOrderId::from("O-CA-2")];
    assert_eq!(rejected, expected);

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
    add_open_order_to_cache(&cache, "O-BC-1", "VOI-BC-1", instrument_id);
    add_open_order_to_cache(&cache, "O-BC-2", "VOI-BC-2", instrument_id);

    let cancels = vec![
        CancelOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: Some(ClientId::from("AX")),
            strategy_id: StrategyId::from("S-001"),
            instrument_id,
            client_order_id: ClientOrderId::from("O-BC-1"),
            venue_order_id: Some(VenueOrderId::new("VOI-BC-1")),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
        CancelOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: Some(ClientId::from("AX")),
            strategy_id: StrategyId::from("S-001"),
            instrument_id,
            client_order_id: ClientOrderId::from("O-BC-2"),
            venue_order_id: Some(VenueOrderId::new("VOI-BC-2")),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
    ];

    let cmd = BatchCancelOrders {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(ClientId::from("AX")),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        cancels,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    };

    client
        .batch_cancel_orders(cmd)
        .expect("batch_cancel_orders should not error");

    // Wait for both per-order WS cancel messages. AxWsCancelOrder serializes
    // `t` as "x" (CancelOrder request type).
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
        if tokio::time::Instant::now() >= deadline {
            let messages = state.get_messages().await;
            panic!("timeout waiting for WS cancels, messages so far: {messages:?}");
        }

        let messages = state.get_messages().await;
        let cancels: Vec<String> = messages
            .iter()
            .filter(|m| m.get("t").and_then(|v| v.as_str()) == Some("x"))
            .filter_map(|m| m.get("oid").and_then(|v| v.as_str()).map(str::to_string))
            .collect();

        if cancels.len() >= 2 {
            assert!(
                cancels.contains(&"VOI-BC-1".to_string()),
                "cancels={cancels:?}"
            );
            assert!(
                cancels.contains(&"VOI-BC-2".to_string()),
                "cancels={cancels:?}"
            );
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    client.disconnect().await.expect("Failed to disconnect");
}

fn make_submit_order_cmd(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        order,
        TraderId::from("TESTER-001"),
        Some(ClientId::from("AX")),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

fn drain_rx(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>) {
    while rx.try_recv().is_ok() {}
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denies_unsupported_order_type() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().expect("Failed to start");
    drain_rx(&mut rx);

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let client_order_id = ClientOrderId::from("O-UNSUPP");
    let order = OrderTestBuilder::new(OrderType::StopMarket)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10"))
        .trigger_price(Price::from("50000.00"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(ClientId::from("AX")), false)
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
                denied.reason.as_str().contains("Unsupported order type"),
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
        .add_order(order.clone(), None, Some(ClientId::from("AX")), false)
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
                denied.reason.as_str().contains("GTD"),
                "reason was: {}",
                denied.reason
            );
        }
        other => panic!("expected OrderDenied, was {other:?}"),
    }
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
        .add_order(order.clone(), None, Some(ClientId::from("AX")), false)
        .unwrap();

    client
        .submit_order(make_submit_order_cmd(&order))
        .expect("submit_order should not error");

    // Wait for the place-order message to arrive on the mock orders WS
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timeout waiting for WS place_order",
        );

        let messages = state.get_messages().await;

        if messages
            .iter()
            .any(|m| m.get("t").and_then(|v| v.as_str()) == Some("p"))
        {
            let place = messages
                .into_iter()
                .find(|m| m.get("t").and_then(|v| v.as_str()) == Some("p"))
                .unwrap();
            assert_eq!(place.get("s").and_then(|v| v.as_str()), Some("EURUSD-PERP"));
            assert_eq!(place.get("q").and_then(|v| v.as_i64()), Some(100));
            assert_eq!(place.get("p").and_then(|v| v.as_str()), Some("50001.00"));
            // Market orders route as IOC
            assert_eq!(place.get("tif").and_then(|v| v.as_str()), Some("IOC"));
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_rejects_on_empty_liquidity() {
    let (addr, state) = start_test_server().await.unwrap();
    state
        .preview_empty
        .store(true, std::sync::atomic::Ordering::Relaxed);
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
        .add_order(order.clone(), None, Some(ClientId::from("AX")), false)
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

    // Next: OrderRejected after preview returns null limit_price
    let rejected = loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout waiting for rejected")
            .expect("channel closed");

        if let ExecutionEvent::Order(OrderEventAny::Rejected(r)) = event {
            break r;
        }
    };
    assert_eq!(rejected.client_order_id, client_order_id);
    assert!(
        rejected.reason.as_str().contains("No liquidity"),
        "reason was: {}",
        rejected.reason,
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
        .add_order(order.clone(), None, Some(ClientId::from("AX")), false)
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

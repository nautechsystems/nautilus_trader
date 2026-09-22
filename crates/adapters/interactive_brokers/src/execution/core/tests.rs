// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Tests for Interactive Brokers execution client behavior.

use std::{cell::RefCell, rc::Rc, str::FromStr};

use ibapi::{
    contracts::{
        Contract, Currency as IBCurrency, Exchange, LegAction, OptionRight, SecurityType,
        Symbol as IBSymbol,
    },
    orders::{
        CommissionReport, Execution, ExecutionData, ExecutionSide, Liquidity, Order as IBOrder,
        OrderData as IBOrderData, OrderState, OrderStatus as IBOrderStatus, OrderStatusKind,
        OrderUpdate,
    },
};
use nautilus_common::{
    cache::Cache,
    live::{runner::replace_exec_event_sender, sender::EventSender},
};
use nautilus_core::Params;
use nautilus_live::{ExecutionClientCore, execution::failure::CommandFailure};
use nautilus_model::{
    enums::{AccountType, AssetClass, OmsType, OrderSide, OrderType},
    events::OrderInitialized,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol, TradeId, TraderId,
        Venue, VenueOrderId,
    },
    instruments::{InstrumentAny, OptionSpread, stubs::equity_aapl},
    orders::{OrderList, builder::OrderTestBuilder},
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use ustr::Ustr;

use super::*;
use crate::{
    common::consts::{IB_CLIENT_ID, IB_VENUE},
    config::InteractiveBrokersInstrumentProviderConfig,
    execution::account::check_external_position_change,
    stubs::ChannelSubscription,
};

const NATIVE_CLIENT_ID: i32 = 1;

fn create_test_instrument_provider() -> Arc<InteractiveBrokersInstrumentProvider> {
    let config = InteractiveBrokersInstrumentProviderConfig::default();
    Arc::new(InteractiveBrokersInstrumentProvider::new(config))
}

fn create_test_execution_client() -> (
    InteractiveBrokersExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("IB-001");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        *IB_CLIENT_ID,
        *IB_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );
    let instrument_provider = create_test_instrument_provider();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(tx);
    let client = InteractiveBrokersExecutionClient::new(
        core,
        InteractiveBrokersExecutionClientConfig::default(),
        instrument_provider,
    )
    .unwrap();

    (client, rx, cache)
}

fn create_test_spread_instrument() -> InstrumentId {
    InstrumentId::new(
        Symbol::from("(1)SPY C400___((1))SPY C410"),
        Venue::from("SMART"),
    )
}

fn create_test_leg_instrument() -> InstrumentId {
    InstrumentId::new(Symbol::from("SPY C400"), Venue::from("SMART"))
}

fn create_test_stock_instrument() -> InstrumentId {
    InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"))
}

fn create_test_limit_order(client_order_id: ClientOrderId) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(create_test_stock_instrument())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .price(Price::from("100.00"))
        .quantity(Quantity::from(1))
        .submit(true)
        .build()
}

fn create_tracked_order_context(
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
) -> TrackedOrder {
    TrackedOrder {
        client_order_id,
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("STRATEGY-001"),
        instrument_id,
        order_side: OrderSide::Buy,
        order_type: OrderType::Limit,
        accepted: false,
        avg_px: None,
        pending_cancel: false,
        pending_modify: None,
        perm_id: 0,
        spread_fill_ids: ahash::AHashSet::new(),
        last_update: None,
    }
}

fn insert_tracked_order(orders: &OrderTracker, order_id: i32, order: TrackedOrder) {
    let mut state = orders.lock().unwrap();
    state.order_id_map.insert(order.client_order_id, order_id);
    state
        .venue_order_id_map
        .insert(order_id, order.client_order_id);
    state.active_orders.insert(order_id, order);
}

struct SubmitTrackingState(OrderTracker);

impl SubmitTrackingState {
    fn new() -> Self {
        Self(OrderTracker::new(NATIVE_CLIENT_ID))
    }

    fn cache(
        &self,
        order_id: i32,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    ) {
        InteractiveBrokersExecutionClient::cache_order_tracking(
            order_id,
            client_order_id,
            instrument_id,
            trader_id,
            strategy_id,
            OrderSide::Buy,
            OrderType::Limit,
            &self.0,
        )
        .unwrap();
    }

    fn assert_active(
        &self,
        order_id: i32,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        accepted: bool,
    ) {
        let state = self.0.lock().unwrap();
        assert_eq!(state.order_id_map.get(&client_order_id), Some(&order_id));
        assert_eq!(
            state.venue_order_id_map.get(&order_id),
            Some(&client_order_id)
        );
        let context = state.active_orders.get(&order_id).unwrap();
        assert_eq!(context.client_order_id, client_order_id);
        assert_eq!(context.instrument_id, instrument_id);
        assert_eq!(context.trader_id, trader_id);
        assert_eq!(context.strategy_id, strategy_id);
        assert_eq!(context.order_side, OrderSide::Buy);
        assert_eq!(context.order_type, OrderType::Limit);
        assert_eq!(context.accepted, accepted);
        assert_eq!(context.avg_px, None);
        assert!(state.terminal_orders.get(&order_id).is_none());
    }

    fn emit_accepted(
        &self,
        order_id: i32,
        account_id: AccountId,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> bool {
        InteractiveBrokersExecutionClient::emit_order_accepted_if_needed(
            order_id,
            VenueOrderId::from(order_id.to_string()),
            account_id,
            UnixNanos::new(29),
            &self.0,
            exec_sender,
        )
        .unwrap()
    }

    fn assert_absent(&self, order_id: i32, client_order_id: ClientOrderId) {
        let state = self.0.lock().unwrap();
        assert_eq!(state.order_id_map.get(&client_order_id), None);
        assert_eq!(state.venue_order_id_map.get(&order_id), None);
        assert!(state.active_orders.get(&order_id).is_none());
        assert!(state.terminal_orders.get(&order_id).is_none());
    }
}

async fn process_submitted_status(
    order_id: i32,
    state: &SubmitTrackingState,
    exec_sender: &EventSender<ExecutionEvent>,
) {
    InteractiveBrokersExecutionClient::handle_order_status(
        &create_test_order_status(order_id, "Submitted"),
        &state.0,
        &create_test_instrument_provider(),
        exec_sender,
        UnixNanos::new(27),
        AccountId::from("IB-001"),
    )
    .await
    .unwrap();
}

fn next_order_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> OrderEventAny {
    match rx.try_recv().unwrap() {
        ExecutionEvent::Order(event) => event,
        event => panic!("Expected order event, was {event:?}"),
    }
}

#[rstest]
#[case(1, 0, 1)]
#[case(1, 310, 310_000_001)]
#[case(42, 1_402, 402_000_042)]
#[case(450_000_123, 402, 402_000_123)]
#[case(1, -12, 12_000_001)]
fn apply_client_order_id_floor(
    #[case] next_id: i32,
    #[case] client_id: i32,
    #[case] expected: i32,
) {
    assert_eq!(
        InteractiveBrokersExecutionClient::apply_client_order_id_floor(next_id, client_id),
        expected
    );
}

#[rstest]
fn stop_cancels_execution_lifecycle_and_aborts_tracked_tasks() {
    let (mut client, _receiver, _cache) = create_test_execution_client();
    client.is_connected.store(true, Ordering::Relaxed);
    client.core.set_connected();
    let cancellation = client.pending_tasks.cancellation_token();
    client
        .pending_tasks
        .spawn(async { std::future::pending::<()>().await })
        .unwrap();

    ExecutionClient::stop(&mut client).unwrap();

    assert!(cancellation.is_cancelled());
    assert!(!client.pending_tasks.is_open());
    assert!(!client.is_connected.load(Ordering::Relaxed));
    assert!(client.core.is_disconnected());
}

#[rstest]
fn order_id_partition_rejects_foreign_open_order() {
    assert!(InteractiveBrokersExecutionClient::is_order_id_in_client_partition(402_000_123, 402));
    assert!(!InteractiveBrokersExecutionClient::is_order_id_in_client_partition(450_000_123, 402));
}

#[rstest]
fn new_derives_ib_account_from_configured_code_for_non_default_client_name() {
    let core = ExecutionClientCore::new(
        TraderId::from("TESTER-001"),
        ClientId::from("IB-TEST"),
        *IB_VENUE,
        OmsType::Netting,
        AccountId::from("IB-TEST-U7654321"),
        AccountType::Margin,
        None,
        Rc::new(RefCell::new(Cache::default())),
    );
    let config = InteractiveBrokersExecutionClientConfig {
        account_id: Some(String::from("U7654321")),
        ..Default::default()
    };

    let client =
        InteractiveBrokersExecutionClient::new(core, config, create_test_instrument_provider())
            .unwrap();

    assert_eq!(client.core.account_id, AccountId::from("IB-TEST-U7654321"));
    assert_eq!(client.ib_account, Ustr::from("U7654321"));
}

#[rstest]
fn execution_filter_uses_raw_ib_account_code() {
    let filter = InteractiveBrokersExecutionClient::execution_filter(Ustr::from("DU123456"), None);

    assert_eq!(filter.client_id, None);
    assert_eq!(filter.account_code, "DU123456");
    assert_eq!(filter.time, "");
    assert_eq!(filter.symbol, "");
    assert_eq!(filter.security_type, "");
    assert_eq!(filter.exchange, "");
    assert_eq!(filter.side, None);
    assert_eq!(filter.last_n_days, 0);
    assert!(filter.specific_dates.is_empty());
}

#[rstest]
fn ib_order_selector_parses_numeric_venue_order_id() {
    let selector = IbOrderSelector::from_venue_order_id(&VenueOrderId::from("123")).unwrap();

    assert_eq!(selector, IbOrderSelector::OrderId(123));
    assert!(selector.matches(123, 456));
    assert!(!selector.matches(124, 456));
    assert_eq!(selector.venue_order_id(), VenueOrderId::from("123"));
}

#[rstest]
fn ib_order_selector_parses_perm_venue_order_id() {
    let selector = IbOrderSelector::from_venue_order_id(&VenueOrderId::from("PERM-456")).unwrap();

    assert_eq!(selector, IbOrderSelector::PermId(456));
    assert!(selector.matches(0, 456));
    assert!(selector.matches(123, 456));
    assert!(!selector.matches(123, 457));
    assert_eq!(selector.venue_order_id(), VenueOrderId::from("PERM-456"));
}

#[rstest]
#[case(
    "PERM-invalid",
    "Failed to parse venue_order_id \"PERM-invalid\" as IB perm_id"
)]
#[case("invalid", "Failed to parse venue_order_id \"invalid\" as IB order_id")]
fn ib_order_selector_rejects_invalid_venue_order_id(
    #[case] venue_order_id: &str,
    #[case] expected: &str,
) {
    let result = IbOrderSelector::from_venue_order_id(&VenueOrderId::from(venue_order_id));

    assert_eq!(result.unwrap_err().to_string(), expected);
}

#[rstest]
#[case::tracked(false, IbOrderSelector::OrderId(123))]
#[case::duplicate_member(true, IbOrderSelector::PermId(789))]
fn cancel_selector_prefers_tracked_order_id(
    #[case] duplicate_member: bool,
    #[case] expected: IbOrderSelector,
) {
    let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let mut tracked = create_tracked_order_context(client_order_id, instrument_id);
    tracked.perm_id = 789;

    if duplicate_member {
        orders
            .lock()
            .unwrap()
            .auxiliary_orders
            .insert(client_order_id, tracked);
    } else {
        insert_tracked_order(&orders, 123, tracked);
    }

    let selector = orders
        .lock()
        .unwrap()
        .cancel_selector(client_order_id, Some(&VenueOrderId::from("PERM-456")))
        .unwrap();

    assert_eq!(selector, Some(expected));
}

#[rstest]
fn cancel_selector_falls_back_to_venue_order_id() {
    let client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);

    let selector = orders
        .lock()
        .unwrap()
        .cancel_selector(client_order_id, Some(&VenueOrderId::from("PERM-456")))
        .unwrap();

    assert_eq!(selector, Some(IbOrderSelector::PermId(456)));
}

#[rstest]
#[case(false, true)]
#[case(true, false)]
fn active_open_order_excludes_deactivated_records(
    #[case] deactivate: bool,
    #[case] expected: bool,
) {
    let order = IBOrder {
        deactivate,
        ..Default::default()
    };

    assert_eq!(
        InteractiveBrokersExecutionClient::is_active_open_order(&order),
        expected
    );
}

#[rstest]
fn order_submit_error_classifies_by_delivery_evidence() {
    let invalid = ibapi::Error::InvalidArgument("invalid quantity".to_string());
    let unsupported = ibapi::Error::ServerVersion(100, 99, "feature".to_string());
    let rejection = ibapi::Error::Notice(ibapi::Notice {
        request_id: None,
        code: 201,
        message: "Order rejected".to_string(),
        error_time: None,
        advanced_order_reject_json: String::new(),
    });
    let cancellation = ibapi::Error::Notice(ibapi::Notice {
        request_id: None,
        code: 202,
        message: "Order cancelled".to_string(),
        error_time: None,
        advanced_order_reject_json: String::new(),
    });

    assert_eq!(
        InteractiveBrokersExecutionClient::classify_order_submit_error(&invalid),
        CommandFailure::not_sent(invalid.to_string())
    );
    assert_eq!(
        InteractiveBrokersExecutionClient::classify_order_submit_error(&unsupported),
        CommandFailure::not_sent(unsupported.to_string())
    );
    assert_eq!(
        InteractiveBrokersExecutionClient::classify_order_submit_error(&rejection),
        CommandFailure::venue_rejected(rejection.to_string())
    );
    assert_eq!(
        InteractiveBrokersExecutionClient::classify_order_submit_error(&cancellation),
        CommandFailure::ambiguous(cancellation.to_string())
    );

    for ambiguous in [
        ibapi::Error::ConnectionReset,
        ibapi::Error::EndOfStream,
        ibapi::Error::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "partial write",
        )),
        ibapi::Error::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "write timed out",
        )),
    ] {
        assert_eq!(
            InteractiveBrokersExecutionClient::classify_order_submit_error(&ambiguous),
            CommandFailure::ambiguous(ambiguous.to_string())
        );
    }

    assert!(InteractiveBrokersExecutionClient::is_definitive_order_submit_error(&invalid));
    assert!(InteractiveBrokersExecutionClient::is_definitive_order_submit_error(&unsupported));
    assert!(
        !InteractiveBrokersExecutionClient::is_definitive_order_submit_error(
            &ibapi::Error::ConnectionReset
        )
    );
}

#[rstest]
fn single_submit_definitive_failure_rejects_and_removes_tracking() {
    let state = SubmitTrackingState::new();
    let order_id = 7101;
    let client_order_id = ClientOrderId::from("O-SINGLE-NOT-SENT");
    let instrument_id = InstrumentId::new(Symbol::from("MSFT"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-SINGLE-001");
    let strategy_id = StrategyId::from("STRATEGY-SINGLE-001");
    let account_id = AccountId::from("IB-SINGLE-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );

    let result = InteractiveBrokersExecutionClient::handle_order_submit_failure(
        &ibapi::Error::InvalidArgument("quantity must be positive".to_string()),
        "Failed to submit order",
        order_id,
        account_id,
        UnixNanos::new(19),
        &state.0,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
    );

    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to submit order: InvalidArgument: quantity must be positive"
    );
    state.assert_absent(order_id, client_order_id);

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.trader_id, trader_id);
            assert_eq!(event.strategy_id, strategy_id);
            assert_eq!(event.instrument_id, instrument_id);
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.account_id, account_id);
            assert_eq!(
                event.reason,
                "Failed to submit order: InvalidArgument: quantity must be positive"
            );
            assert_eq!(event.ts_event, UnixNanos::new(19));
            assert!(!event.reconciliation);
            assert!(!event.due_post_only);
        }
        event => panic!("Expected rejected order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());
}

#[tokio::test]
async fn single_submit_ambiguous_failure_retains_tracking_for_status_resolution() {
    let state = SubmitTrackingState::new();
    let order_id = 7102;
    let client_order_id = ClientOrderId::from("O-SINGLE-AMBIGUOUS");
    let instrument_id = InstrumentId::new(Symbol::from("NVDA"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-SINGLE-002");
    let strategy_id = StrategyId::from("STRATEGY-SINGLE-002");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );

    let result = InteractiveBrokersExecutionClient::handle_order_submit_failure(
        &ibapi::Error::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "socket write timed out",
        )),
        "Failed to submit order",
        order_id,
        AccountId::from("IB-001"),
        UnixNanos::new(23),
        &state.0,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
    );

    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to submit order; outcome is unknown after possible transmission: socket write timed out"
    );
    assert!(exec_receiver.try_recv().is_err());
    state.assert_active(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
        false,
    );

    process_submitted_status(order_id, &state, &exec_sender.clone().into()).await;

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Accepted(event)) => {
            assert_eq!(event.trader_id, trader_id);
            assert_eq!(event.strategy_id, strategy_id);
            assert_eq!(event.instrument_id, instrument_id);
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(
                event.venue_order_id,
                VenueOrderId::from(order_id.to_string())
            );
            assert_eq!(event.account_id, AccountId::from("IB-001"));
            assert_eq!(event.ts_event, UnixNanos::new(27));
        }
        event => panic!("Expected accepted order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());
    state.assert_active(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
        true,
    );
}

#[tokio::test]
async fn inactive_order_status_emits_rejected_before_terminal_eviction() {
    let state = SubmitTrackingState::new();
    let order_id = 7103;
    let client_order_id = ClientOrderId::from("O-SINGLE-INACTIVE");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-SINGLE-003");
    let strategy_id = StrategyId::from("STRATEGY-SINGLE-003");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );
    let mut status = create_test_order_status(order_id, "Inactive");
    status.why_held = "Order rejected by venue".to_string();

    InteractiveBrokersExecutionClient::handle_order_status(
        &status,
        &state.0,
        &create_test_instrument_provider(),
        &exec_sender.clone().into(),
        UnixNanos::new(41),
        account_id,
    )
    .await
    .unwrap();

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.trader_id, trader_id);
            assert_eq!(event.strategy_id, strategy_id);
            assert_eq!(event.instrument_id, instrument_id);
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.account_id, account_id);
            assert_eq!(event.reason.as_str(), "Order rejected by venue");
            assert_eq!(event.ts_event, UnixNanos::new(41));
            assert_eq!(event.ts_init, UnixNanos::new(41));
            assert!(!event.reconciliation);
            assert!(!event.due_post_only);
        }
        event => panic!("Expected rejected order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());
    let tracker = state.0.lock().unwrap();
    assert_eq!(tracker.order_id_map.get(&client_order_id), None);
    assert_eq!(tracker.venue_order_id_map.get(&order_id), None);
    assert!(tracker.active_orders.get(&order_id).is_none());
    assert_eq!(
        tracker
            .terminal_orders
            .get(&order_id)
            .map(|context| context.client_order_id),
        Some(client_order_id)
    );
}

#[rstest]
#[tokio::test]
async fn inactive_partially_filled_order_emits_canceled() {
    let state = SubmitTrackingState::new();
    let order_id = 7104;
    let client_order_id = ClientOrderId::from("O-PARTIAL-INACTIVE");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-SINGLE-004");
    let strategy_id = StrategyId::from("STRATEGY-SINGLE-004");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );

    let mut status = create_test_order_status(order_id, "Inactive");
    status.filled = 1.0;
    status.remaining = 1.0;
    status.why_held = "Order inactive after partial fill".to_string();

    InteractiveBrokersExecutionClient::handle_order_status(
        &status,
        &state.0,
        &create_test_instrument_provider(),
        &exec_sender.clone().into(),
        UnixNanos::new(42),
        account_id,
    )
    .await
    .unwrap();

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
            assert_eq!(event.trader_id, trader_id);
            assert_eq!(event.strategy_id, strategy_id);
            assert_eq!(event.instrument_id, instrument_id);
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.account_id, Some(account_id));
            assert_eq!(event.ts_event, UnixNanos::new(42));
        }
        event => panic!("Expected canceled order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());
}

#[rstest]
#[case::pending_modify(true, 0.0)]
#[case::no_pending_modify(false, 0.0)]
#[case::partially_filled_pending_modify(true, 1.0)]
#[tokio::test]
async fn inactive_accepted_order_stays_working(#[case] pending_modify: bool, #[case] filled: f64) {
    let state = SubmitTrackingState::new();
    let order_id = 7105;
    let client_order_id = ClientOrderId::from("O-ACCEPTED-INACTIVE");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-SINGLE-005");
    let strategy_id = StrategyId::from("STRATEGY-SINGLE-005");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );
    {
        let mut tracker = state.0.lock().unwrap();
        let order = tracker.active_orders.get_mut(&order_id).unwrap();
        order.accepted = true;
        order.pending_modify = pending_modify.then_some(PendingModifyValues {
            total_quantity: 1.0,
            limit_price: Some(5000.0),
            aux_price: None,
            trail_stop_price: None,
        });
    }

    let mut status = create_test_order_status(order_id, "Inactive");
    status.why_held = "Modify refused by venue".to_string();
    status.filled = filled;

    InteractiveBrokersExecutionClient::handle_order_status(
        &status,
        &state.0,
        &create_test_instrument_provider(),
        &exec_sender.clone().into(),
        UnixNanos::new(46),
        account_id,
    )
    .await
    .unwrap();

    if pending_modify {
        match exec_receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
                assert_eq!(event.trader_id, trader_id);
                assert_eq!(event.strategy_id, strategy_id);
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.account_id, Some(account_id));
                assert_eq!(event.reason.as_str(), "Modify refused by venue");
                assert_eq!(event.ts_event, UnixNanos::new(46));
            }
            event => panic!("Expected modify rejected event, was {event:?}"),
        }
    }
    assert!(exec_receiver.try_recv().is_err());
    state.assert_active(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
        true,
    );
    let tracker = state.0.lock().unwrap();
    assert!(tracker.active_orders[&order_id].pending_modify.is_none());
}

#[rstest]
fn order_notice_rejection_emits_terminal_event_after_informational_notices() {
    let state = SubmitTrackingState::new();
    let order_id = 7104;
    let client_order_id = ClientOrderId::from("O-SINGLE-NOTICE");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-SINGLE-004");
    let strategy_id = StrategyId::from("STRATEGY-SINGLE-004");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );

    for notice in [
        Notice {
            request_id: Some(order_id),
            code: 2109,
            message: "Order event attribute ignored".to_string(),
            error_time: None,
            advanced_order_reject_json: String::new(),
        },
        Notice {
            request_id: Some(order_id),
            code: 399,
            message: "Order Message:\nSELL 1 ES DEC'26\nWarning: Your order will not be placed at the exchange until 2026-08-17 08:30:00 US/Central.".to_string(),
            error_time: None,
            advanced_order_reject_json: String::new(),
        },
        Notice {
            request_id: Some(order_id),
            code: 202,
            message: "Order cancelled".to_string(),
            error_time: None,
            advanced_order_reject_json: String::new(),
        },
        Notice {
            request_id: None,
            code: 201,
            message: "Request-less rejection".to_string(),
            error_time: None,
            advanced_order_reject_json: String::new(),
        },
    ] {
        InteractiveBrokersExecutionClient::handle_order_notice(
            &notice,
            &state.0,
            &exec_sender.clone().into(),
            UnixNanos::new(42),
            account_id,
            None,
        )
        .unwrap();
    }

    assert!(exec_receiver.try_recv().is_err());
    state.assert_active(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
        false,
    );

    let rejection = Notice {
        request_id: Some(order_id),
        code: 201,
        message: "Order rejected: insufficient margin".to_string(),
        error_time: None,
        advanced_order_reject_json: r#"{"errorCode":"IBDBUYTX"}"#.to_string(),
    };
    InteractiveBrokersExecutionClient::handle_order_notice(
        &rejection,
        &state.0,
        &exec_sender.clone().into(),
        UnixNanos::new(43),
        account_id,
        None,
    )
    .unwrap();

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.trader_id, trader_id);
            assert_eq!(event.strategy_id, strategy_id);
            assert_eq!(event.instrument_id, instrument_id);
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.account_id, account_id);
            assert_eq!(event.reason.as_str(), "Order rejected: insufficient margin");
            assert_eq!(event.ts_event, UnixNanos::new(43));
            assert_eq!(event.ts_init, UnixNanos::new(43));
            assert!(!event.reconciliation);
            assert!(!event.due_post_only);
        }
        event => panic!("Expected rejected order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());
    let tracker = state.0.lock().unwrap();
    assert_eq!(tracker.order_id_map.get(&client_order_id), None);
    assert_eq!(tracker.venue_order_id_map.get(&order_id), None);
    assert!(tracker.active_orders.get(&order_id).is_none());
    assert_eq!(
        tracker
            .terminal_orders
            .get(&order_id)
            .map(|context| context.client_order_id),
        Some(client_order_id)
    );
}

#[rstest]
fn order_notice_sub_200_error_rejects_pre_acceptance_order() {
    let state = SubmitTrackingState::new();
    let order_id = 7105;
    let client_order_id = ClientOrderId::from("O-SUB-200");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-SUB-200");
    let strategy_id = StrategyId::from("STRATEGY-SUB-200");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );

    let notice = Notice {
        request_id: Some(order_id),
        code: 110,
        message: "The price does not conform to the minimum price variation".to_string(),
        error_time: None,
        advanced_order_reject_json: String::new(),
    };
    InteractiveBrokersExecutionClient::handle_order_notice(
        &notice,
        &state.0,
        &exec_sender.clone().into(),
        UnixNanos::new(44),
        account_id,
        None,
    )
    .unwrap();

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.account_id, account_id);
            assert_eq!(
                event.reason.as_str(),
                "The price does not conform to the minimum price variation"
            );
            assert_eq!(event.ts_event, UnixNanos::new(44));
        }
        event => panic!("Expected rejected order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());

    let tracker = state.0.lock().unwrap();
    assert!(tracker.active_orders.get(&order_id).is_none());
    assert_eq!(
        tracker
            .terminal_orders
            .get(&order_id)
            .map(|context| context.client_order_id),
        Some(client_order_id)
    );
}

#[rstest]
fn order_notice_for_accepted_order_with_pending_modify_emits_modify_rejected() {
    let state = SubmitTrackingState::new();
    let order_id = 7106;
    let client_order_id = ClientOrderId::from("O-MODIFY-REJECT");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-MODIFY");
    let strategy_id = StrategyId::from("STRATEGY-MODIFY");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );
    {
        let mut tracker = state.0.lock().unwrap();
        let order = tracker.active_orders.get_mut(&order_id).unwrap();
        order.accepted = true;
        order.pending_modify = Some(PendingModifyValues {
            total_quantity: 1.0,
            limit_price: Some(5000.0),
            aux_price: None,
            trail_stop_price: None,
        });
    }

    let notice = Notice {
        request_id: Some(order_id),
        code: 201,
        message: "Order rejected - reason: modify would violate margin".to_string(),
        error_time: None,
        advanced_order_reject_json: String::new(),
    };
    InteractiveBrokersExecutionClient::handle_order_notice(
        &notice,
        &state.0,
        &exec_sender.clone().into(),
        UnixNanos::new(45),
        account_id,
        None,
    )
    .unwrap();

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.account_id, Some(account_id));
            assert_eq!(
                event.reason.as_str(),
                "Order rejected - reason: modify would violate margin"
            );
            assert_eq!(event.ts_event, UnixNanos::new(45));
        }
        event => panic!("Expected modify rejected event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());

    let tracker = state.0.lock().unwrap();
    let order = tracker.active_orders.get(&order_id).unwrap();
    assert!(order.accepted);
    assert!(order.pending_modify.is_none());
}

#[rstest]
fn order_notice_for_accepted_order_with_pending_cancel_emits_cancel_rejected() {
    let state = SubmitTrackingState::new();
    let order_id = 7108;
    let client_order_id = ClientOrderId::from("O-CANCEL-REJECT");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-CANCEL");
    let strategy_id = StrategyId::from("STRATEGY-CANCEL");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );
    {
        let mut tracker = state.0.lock().unwrap();
        let order = tracker.active_orders.get_mut(&order_id).unwrap();
        order.accepted = true;
        order.pending_cancel = true;
        order.perm_id = 9_108;
    }

    let notice = Notice {
        request_id: Some(order_id),
        code: 161,
        message: "Cancel attempted when order is not in a cancellable state".to_string(),
        error_time: None,
        advanced_order_reject_json: String::new(),
    };
    InteractiveBrokersExecutionClient::handle_order_notice(
        &notice,
        &state.0,
        &exec_sender.clone().into(),
        UnixNanos::new(47),
        account_id,
        None,
    )
    .unwrap();

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.trader_id, trader_id);
            assert_eq!(event.strategy_id, strategy_id);
            assert_eq!(event.instrument_id, instrument_id);
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.venue_order_id, Some(VenueOrderId::new("PERM-9108")));
            assert_eq!(event.account_id, Some(account_id));
            assert_eq!(
                event.reason.as_str(),
                "Cancel attempted when order is not in a cancellable state"
            );
            assert_eq!(event.ts_event, UnixNanos::new(47));
        }
        event => panic!("Expected cancel rejected event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());

    let tracker = state.0.lock().unwrap();
    let order = tracker.active_orders.get(&order_id).unwrap();
    assert!(order.accepted);
    assert!(!order.pending_cancel);
}

#[rstest]
fn order_notice_for_accepted_order_without_pending_modify_stays_non_terminal() {
    let state = SubmitTrackingState::new();
    let order_id = 7107;
    let client_order_id = ClientOrderId::from("O-ACCEPTED-NOTICE");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-ACCEPTED");
    let strategy_id = StrategyId::from("STRATEGY-ACCEPTED");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
    );
    {
        let mut tracker = state.0.lock().unwrap();
        tracker.active_orders.get_mut(&order_id).unwrap().accepted = true;
    }

    for code in [110, 201] {
        let notice = Notice {
            request_id: Some(order_id),
            code,
            message: format!("Venue notice {code}"),
            error_time: None,
            advanced_order_reject_json: String::new(),
        };
        InteractiveBrokersExecutionClient::handle_order_notice(
            &notice,
            &state.0,
            &exec_sender.clone().into(),
            UnixNanos::new(46),
            account_id,
            None,
        )
        .unwrap();
    }

    assert!(exec_receiver.try_recv().is_err());
    state.assert_active(
        order_id,
        client_order_id,
        instrument_id,
        trader_id,
        strategy_id,
        true,
    );
}

#[rstest]
fn resolve_failed_order_list_predecessors_cancels_and_clears_tracking() {
    let state = SubmitTrackingState::new();
    let first_id = 7301;
    let second_id = 7302;
    let first_client_id = ClientOrderId::from("O-PRED-FIRST");
    let second_client_id = ClientOrderId::from("O-PRED-SECOND");
    let strategy_id = StrategyId::from("STRATEGY-PRED-001");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    let first_order = create_test_limit_order(first_client_id);
    let second_order = create_test_limit_order(second_client_id);
    let mut ib_order_ids = AHashMap::new();
    ib_order_ids.insert(first_client_id, first_id);
    ib_order_ids.insert(second_client_id, second_id);
    for (order, order_id) in [(&first_order, first_id), (&second_order, second_id)] {
        state.cache(
            order_id,
            order.client_order_id(),
            order.instrument_id(),
            order.trader_id(),
            strategy_id,
        );
    }

    InteractiveBrokersExecutionClient::resolve_failed_order_list_predecessors(
        &[first_order, second_order],
        &ib_order_ids,
        &AHashSet::new(),
        &state.0,
        strategy_id,
        account_id,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
    )
    .unwrap();

    for expected_client_id in [first_client_id, second_client_id] {
        match exec_receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
                assert_eq!(event.client_order_id, expected_client_id);
                assert_eq!(event.strategy_id, strategy_id);
                assert_eq!(event.account_id, Some(account_id));
            }
            event => panic!("Expected canceled order event, was {event:?}"),
        }
    }
    assert!(exec_receiver.try_recv().is_err());

    let tracker = state.0.lock().unwrap();
    assert!(tracker.active_orders.is_empty());
    assert!(tracker.order_id_map.is_empty());
    assert!(tracker.venue_order_id_map.is_empty());
}

#[rstest]
fn resolve_failed_order_list_predecessors_skips_failed_cancels() {
    let state = SubmitTrackingState::new();
    let first_id = 7311;
    let second_id = 7312;
    let first_client_id = ClientOrderId::from("O-PRED-PARKED");
    let second_client_id = ClientOrderId::from("O-PRED-CANCELLED");
    let strategy_id = StrategyId::from("STRATEGY-PRED-002");
    let account_id = AccountId::from("IB-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    let first_order = create_test_limit_order(first_client_id);
    let second_order = create_test_limit_order(second_client_id);
    let mut ib_order_ids = AHashMap::new();
    ib_order_ids.insert(first_client_id, first_id);
    ib_order_ids.insert(second_client_id, second_id);
    for (order, order_id) in [(&first_order, first_id), (&second_order, second_id)] {
        state.cache(
            order_id,
            order.client_order_id(),
            order.instrument_id(),
            order.trader_id(),
            strategy_id,
        );
    }

    let mut failed_cancels = AHashSet::new();
    failed_cancels.insert(first_client_id);

    InteractiveBrokersExecutionClient::resolve_failed_order_list_predecessors(
        &[first_order, second_order],
        &ib_order_ids,
        &failed_cancels,
        &state.0,
        strategy_id,
        account_id,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
    )
    .unwrap();

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
            assert_eq!(event.client_order_id, second_client_id);
        }
        event => panic!("Expected canceled order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());

    let tracker = state.0.lock().unwrap();
    assert!(tracker.active_orders.contains_key(&first_id));
    assert_eq!(tracker.order_id_map.get(&first_client_id), Some(&first_id));
    assert!(!tracker.active_orders.contains_key(&second_id));
}

#[rstest]
fn absent_order_terminal_event_maps_completed_status() {
    let client_order_id = ClientOrderId::from("O-ABSENT");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    let tracked = create_tracked_order_context(client_order_id, instrument_id);
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(50);

    let canceled = InteractiveBrokersExecutionClient::absent_order_terminal_event(
        Some((OrderStatusKind::Cancelled, 9_900, 0.0)),
        None,
        &tracked,
        7401,
        account_id,
        ts_init,
    );

    match canceled {
        Some(OrderEventAny::Canceled(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.venue_order_id, Some(VenueOrderId::new("PERM-9900")));
            assert_eq!(event.account_id, Some(account_id));
            assert_eq!(event.ts_event, ts_init);
        }
        event => panic!("Expected canceled event, was {event:?}"),
    }

    let rejected = InteractiveBrokersExecutionClient::absent_order_terminal_event(
        Some((OrderStatusKind::Inactive, 0, 0.0)),
        None,
        &tracked,
        7401,
        account_id,
        ts_init,
    );

    match rejected {
        Some(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(
                event.reason.as_str(),
                "IB reports Inactive after a venue notice"
            );
        }
        event => panic!("Expected rejected event, was {event:?}"),
    }

    let filled = InteractiveBrokersExecutionClient::absent_order_terminal_event(
        Some((OrderStatusKind::Filled, 9_900, 1.0)),
        None,
        &tracked,
        7401,
        account_id,
        ts_init,
    );
    assert!(filled.is_none());

    let partially_filled_inactive = InteractiveBrokersExecutionClient::absent_order_terminal_event(
        Some((OrderStatusKind::Inactive, 9_900, 1.0)),
        None,
        &tracked,
        7401,
        account_id,
        ts_init,
    );

    match partially_filled_inactive {
        Some(OrderEventAny::Canceled(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.venue_order_id, Some(VenueOrderId::new("PERM-9900")));
            assert_eq!(event.account_id, Some(account_id));
            assert_eq!(event.ts_event, ts_init);
        }
        event => panic!("Expected canceled event, was {event:?}"),
    }

    let unknown = InteractiveBrokersExecutionClient::absent_order_terminal_event(
        None, None, &tracked, 7401, account_id, ts_init,
    );
    assert!(unknown.is_none());

    let filled_before_acceptance = InteractiveBrokersExecutionClient::absent_order_terminal_event(
        Some((OrderStatusKind::Filled, 9_900, 1.0)),
        Some(Ustr::from("Order preset notice")),
        &tracked,
        7401,
        account_id,
        ts_init,
    );
    assert!(filled_before_acceptance.is_none());

    let never_listed = InteractiveBrokersExecutionClient::absent_order_terminal_event(
        None,
        Some(Ustr::from("Price does not conform to the minimum tick")),
        &tracked,
        7401,
        account_id,
        ts_init,
    );

    match never_listed {
        Some(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.account_id, account_id);
            assert_eq!(
                event.reason.as_str(),
                "Price does not conform to the minimum tick"
            );
            assert_eq!(event.ts_event, ts_init);
        }
        event => panic!("Expected rejected event, was {event:?}"),
    }
}

#[rstest]
#[case(1.0, None, true)]
#[case(2.0, Some(101.0), false)]
#[tokio::test]
async fn open_order_refresh_clears_pending_modify_only_on_match(
    #[case] refresh_quantity: f64,
    #[case] refresh_limit: Option<f64>,
    #[case] expect_pending_after: bool,
) {
    let order_id = 7501;
    let client_order_id = ClientOrderId::from("O-PENDING-MODIFY");
    let equity = equity_aapl();
    let instrument_id = equity.id();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let position_tracker = create_position_tracker();

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    let mut tracked = create_tracked_order_context(client_order_id, instrument_id);
    tracked.accepted = true;
    tracked.pending_modify = Some(PendingModifyValues {
        total_quantity: 2.0,
        limit_price: Some(101.0),
        aux_price: None,
        trail_stop_price: None,
    });
    insert_tracked_order(&orders, order_id, tracked);

    let mut open_order = create_test_open_order(order_id, "Submitted", client_order_id.as_str());
    open_order.order.total_quantity = refresh_quantity;
    open_order.order.limit_price = refresh_limit;
    open_order.order.order_type = "LMT".to_string();

    InteractiveBrokersExecutionClient::handle_order_update(
        &OrderUpdate::OpenOrder(open_order),
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &position_tracker,
    )
    .await
    .unwrap();

    while exec_receiver.try_recv().is_ok() {}
    let state = orders.lock().unwrap();
    let order = state.active_orders.get(&order_id).unwrap();
    assert_eq!(order.pending_modify.is_some(), expect_pending_after);
}

#[tokio::test]
async fn trailing_stop_modify_is_acknowledged_by_its_trail_stop_price() {
    let order_id = 7503;
    let client_order_id = ClientOrderId::from("O-TRAIL-MODIFY");
    let equity = equity_aapl();
    let instrument_id = equity.id();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let position_tracker = create_position_tracker();

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    let mut tracked = create_tracked_order_context(client_order_id, instrument_id);
    tracked.order_type = OrderType::TrailingStopMarket;
    tracked.accepted = true;
    tracked.last_update = Some((Quantity::from(2), None, Some(Price::from("96.00"))));
    tracked.pending_modify = Some(PendingModifyValues {
        total_quantity: 2.0,
        limit_price: None,
        aux_price: Some(1.5),
        trail_stop_price: Some(95.0),
    });
    insert_tracked_order(&orders, order_id, tracked);

    let mut open_order = create_test_open_order(order_id, "Submitted", client_order_id.as_str());
    open_order.order.order_type = "TRAIL".to_string();
    open_order.order.total_quantity = 2.0;
    open_order.order.limit_price = None;
    open_order.order.aux_price = Some(1.5);
    open_order.order.trail_stop_price = Some(95.0);

    InteractiveBrokersExecutionClient::handle_order_update(
        &OrderUpdate::OpenOrder(open_order),
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &position_tracker,
    )
    .await
    .unwrap();

    let mut updates = Vec::new();

    while let Ok(event) = exec_receiver.try_recv() {
        if let ExecutionEvent::Order(OrderEventAny::Updated(event)) = event {
            updates.push((event.quantity, event.price, event.trigger_price));
        }
    }

    assert_eq!(
        updates,
        vec![(Quantity::from(2), None, Some(Price::from("95.00")))]
    );
    let state = orders.lock().unwrap();
    assert!(state.active_orders[&order_id].pending_modify.is_none());
}

#[rstest]
#[case::tracked(false, vec![Price::from("100.00"), Price::from("101.00")])]
#[case::recovered(true, vec![Price::from("101.00")])]
#[tokio::test]
async fn open_order_refresh_emits_order_updated_only_on_change(
    #[case] recovered: bool,
    #[case] expected_prices: Vec<Price>,
) {
    let order_id = 7502;
    let client_order_id = ClientOrderId::from("O-OPEN-REFRESH");
    let equity = equity_aapl();
    let instrument_id = equity.id();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let position_tracker = create_position_tracker();

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);

    if recovered {
        let cached = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from(2))
            .submit(true)
            .build();
        InteractiveBrokersExecutionClient::cache_recovered_order_tracking(
            order_id, &cached, &orders,
        )
        .unwrap();
    } else {
        let mut tracked = create_tracked_order_context(client_order_id, instrument_id);
        tracked.accepted = true;
        insert_tracked_order(&orders, order_id, tracked);
    }

    let mut updates = Vec::new();

    for limit_price in [100.0, 100.0, 101.0] {
        let mut open_order =
            create_test_open_order(order_id, "PreSubmitted", client_order_id.as_str());
        open_order.order.total_quantity = 2.0;
        open_order.order.limit_price = Some(limit_price);
        open_order.order.order_type = "LMT".to_string();

        InteractiveBrokersExecutionClient::handle_order_update(
            &OrderUpdate::OpenOrder(open_order),
            &orders,
            &instrument_provider,
            &exec_sender.clone().into(),
            nautilus_core::time::get_atomic_clock_realtime(),
            AccountId::from("IB-001"),
            Ustr::from("001"),
            &commission_cache,
            &pending_execution_cache,
            &position_tracker,
        )
        .await
        .unwrap();

        while let Ok(event) = exec_receiver.try_recv() {
            if let ExecutionEvent::Order(OrderEventAny::Updated(event)) = event {
                updates.push((event.client_order_id, event.quantity, event.price));
            }
        }
    }

    let expected: Vec<_> = expected_prices
        .into_iter()
        .map(|price| (client_order_id, Quantity::from(2), Some(price)))
        .collect();
    assert_eq!(updates, expected);
}

#[rstest]
#[case::limit("LMT", Some(Price::from("101.00")), None, Some(101.0), Some(1.5), None)]
#[case::stop("STP", None, Some(Price::from("95.00")), None, Some(95.0), None)]
#[case::trailing("TRAIL", None, Some(Price::from("95.00")), None, Some(2.5), Some(95.0))]
fn modify_changes_only_requested_fields_of_the_open_order(
    #[case] order_type: &str,
    #[case] price: Option<Price>,
    #[case] trigger_price: Option<Price>,
    #[case] expected_limit: Option<f64>,
    #[case] expected_aux: Option<f64>,
    #[case] expected_trail_stop: Option<f64>,
) {
    let equity = equity_aapl();
    let instrument_id = equity.id();
    let instrument_provider = create_test_instrument_provider();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    let original = IBOrder {
        order_type: order_type.to_string(),
        total_quantity: 1.0,
        limit_price: None,
        aux_price: Some(1.5),
        tif: ibapi::orders::TimeInForce::GoodTillCanceled,
        good_after_time: "20260924 12:31:13 UTC".to_string(),
        oca_group: "PROTECT-1".to_string(),
        oca_type: ibapi::orders::OcaType::ReduceWithBlock,
        outside_rth: true,
        order_ref: "O-RESTORED-1".to_string(),
        account: "DU123".to_string(),
        ..Default::default()
    };
    let mut params = Params::new();
    params.insert(
        MODIFY_TRAILING_OFFSET_PARAM.to_string(),
        serde_json::json!(2.5),
    );
    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("CLIENT-001")),
        StrategyId::from("STRATEGY-001"),
        instrument_id,
        ClientOrderId::from("O-RESTORED-1"),
        None,
        Some(Quantity::from(2)),
        price,
        trigger_price,
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
        None,
    );
    let mut modified = original.clone();

    InteractiveBrokersExecutionClient::apply_modify_fields_to_ib_order(
        &cmd,
        &mut modified,
        &instrument_provider,
    )
    .unwrap();

    let expected = IBOrder {
        total_quantity: 2.0,
        limit_price: expected_limit,
        aux_price: expected_aux,
        trail_stop_price: expected_trail_stop,
        ..original
    };
    assert_eq!(modified, expected);
}

#[rstest]
#[case::quantity_modify_ignores_moved_trigger(None, Some(94.0), true)]
#[case::requested_trigger_matches(Some(95.0), Some(95.0), true)]
#[case::requested_trigger_differs(Some(95.0), Some(94.0), false)]
#[case::requested_trigger_missing(Some(95.0), None, false)]
fn pending_modify_checks_trailing_trigger_only_when_requested(
    #[case] requested: Option<f64>,
    #[case] reported: Option<f64>,
    #[case] expected: bool,
) {
    let pending = PendingModifyValues {
        total_quantity: 2.0,
        limit_price: None,
        aux_price: Some(1.5),
        trail_stop_price: requested,
    };
    let order = IBOrder {
        order_type: "TRAIL".to_string(),
        total_quantity: 2.0,
        limit_price: None,
        aux_price: Some(1.5),
        trail_stop_price: reported,
        ..Default::default()
    };

    assert_eq!(pending.matches(&order), expected);
}

#[rstest]
fn pending_modify_is_marked_once_for_matching_tracked_order() {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let order_id = 7120;
    let client_order_id = ClientOrderId::from("O-MODIFY-ATOMIC");
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );
    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("CLIENT-001")),
        StrategyId::from("STRATEGY-001"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from(order_id.to_string())),
        Some(Quantity::from(2)),
        Some(Price::from("101.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let ib_order = IBOrder {
        total_quantity: 2.0,
        limit_price: Some(101.0),
        ..Default::default()
    };

    InteractiveBrokersExecutionClient::mark_pending_modify(&cmd, order_id, &orders, &ib_order)
        .unwrap();

    assert_eq!(
        orders
            .lock()
            .unwrap()
            .order(order_id)
            .unwrap()
            .pending_modify,
        Some(PendingModifyValues {
            total_quantity: 2.0,
            limit_price: Some(101.0),
            aux_price: None,
            trail_stop_price: None,
        }),
    );
    assert_eq!(
        InteractiveBrokersExecutionClient::mark_pending_modify(&cmd, order_id, &orders, &ib_order,)
            .unwrap_err()
            .to_string(),
        "IB order 7120 already has a pending modify",
    );
}

// Replies to `reqExecutions` reach the update stream with the request's positive ID
#[rstest]
#[case(-1, None)]
#[case(0, None)]
#[case(9001, Some((false, Decimal::ZERO)))]
#[tokio::test]
async fn execution_data_records_own_fill_before_commission_report(
    #[case] request_id: i32,
    #[case] expected_external_change: Option<(bool, Decimal)>,
) {
    let order_id = 7401;
    let client_order_id = ClientOrderId::from("O-OWN-FILL-EAGER");
    let equity = equity_aapl();
    let instrument_id = equity.id();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let position_tracker = create_position_tracker();

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let mut exec_data = create_test_execution_data(order_id, "EXEC-EAGER-1", 2.0, 100.0, "BOT");
    exec_data.request_id = request_id;
    let contract_id = exec_data.contract.contract_id;
    InteractiveBrokersExecutionClient::handle_order_update(
        &OrderUpdate::ExecutionData(exec_data),
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &position_tracker,
    )
    .await
    .unwrap();

    // No commission yet: the fill is buffered and no event is emitted, but the
    // position stream must already observe the post-fill quantity as its own.
    assert!(exec_receiver.try_recv().is_err());
    assert_eq!(
        check_external_position_change(&position_tracker, contract_id, Decimal::from(2)).await,
        expected_external_change
    );
}

#[rstest]
fn list_submit_definitive_partial_failure_preserves_prefix_and_omits_tail() {
    let state = SubmitTrackingState::new();
    let prior_id = 7201;
    let current_id = 7202;
    let tail_id = 7203;
    let prior_client_id = ClientOrderId::from("O-LIST-PRIOR");
    let current_client_id = ClientOrderId::from("O-LIST-NOT-SENT");
    let tail_client_id = ClientOrderId::from("O-LIST-TAIL");
    let prior_instrument_id = InstrumentId::new(Symbol::from("AMD"), Venue::from("SMART"));
    let current_instrument_id = InstrumentId::new(Symbol::from("INTC"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-LIST-001");
    let strategy_id = StrategyId::from("STRATEGY-LIST-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        prior_id,
        prior_client_id,
        prior_instrument_id,
        trader_id,
        strategy_id,
    );
    state.cache(
        current_id,
        current_client_id,
        current_instrument_id,
        trader_id,
        strategy_id,
    );
    assert!(state.emit_accepted(
        prior_id,
        AccountId::from("IB-LIST-001"),
        &exec_sender.clone().into()
    ));
    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.client_order_id == prior_client_id
                && event.venue_order_id == VenueOrderId::from(prior_id.to_string())
    ));

    let error = ibapi::Error::ServerVersion(170, 169, "order feature".to_string());
    let result = InteractiveBrokersExecutionClient::handle_order_submit_failure(
        &error,
        "Failed to submit order from list",
        current_id,
        AccountId::from("IB-LIST-001"),
        UnixNanos::new(31),
        &state.0,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
    );

    assert_eq!(
        result.unwrap_err().to_string(),
        format!("Failed to submit order from list: {error}")
    );
    state.assert_active(
        prior_id,
        prior_client_id,
        prior_instrument_id,
        trader_id,
        strategy_id,
        true,
    );
    state.assert_absent(current_id, current_client_id);
    state.assert_absent(tail_id, tail_client_id);

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, current_client_id);
            assert_eq!(event.instrument_id, current_instrument_id);
            assert_eq!(event.ts_event, UnixNanos::new(31));
        }
        event => panic!("Expected rejected order event, was {event:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());
}

#[rstest]
fn list_submit_failure_denies_every_unsubmitted_tail_order() {
    let first_id = ClientOrderId::from("O-LIST-TAIL-001");
    let second_id = ClientOrderId::from("O-LIST-TAIL-002");
    let orders = vec![
        create_test_limit_order(first_id),
        create_test_limit_order(second_id),
    ];
    let strategy_id = StrategyId::from("STRATEGY-LIST-001");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    InteractiveBrokersExecutionClient::deny_unsubmitted_order_list(
        &orders,
        DENIAL_ORDER_LIST_SIBLING_SUBMIT_FAILED,
        strategy_id,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
    )
    .unwrap();

    for expected_id in [first_id, second_id] {
        match exec_receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Denied(event)) => {
                assert_eq!(event.client_order_id, expected_id);
                assert_eq!(event.strategy_id, strategy_id);
                assert_eq!(event.reason.as_str(), "ORDER_LIST_SIBLING_SUBMIT_FAILED");
            }
            event => panic!("Expected denied order event, was {event:?}"),
        }
    }
    assert!(exec_receiver.try_recv().is_err());
}

#[rstest]
fn prepare_order_list_fails_before_submission_on_an_untransformable_child() {
    let equity = equity_aapl();
    let instrument_id = equity.id();
    let instrument_provider = create_test_instrument_provider();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    let entry = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-LIST-ENTRY"))
        .side(OrderSide::Buy)
        .price(Price::from("100.00"))
        .quantity(Quantity::from(1))
        .build();
    let stop = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-LIST-STOP"))
        .parent_order_id(entry.client_order_id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(1))
        .trigger_price(Price::from("95.00"))
        .trailing_offset(Decimal::from(5))
        .trailing_offset_type(TrailingOffsetType::Ticks)
        .build();
    let orders = vec![entry.clone(), stop.clone()];
    let cmd = SubmitOrderList::new(
        TraderId::from("TRADER-001"),
        Some(*IB_CLIENT_ID),
        entry.strategy_id(),
        OrderList::new(
            OrderListId::from("OL-IB-UNTRANSFORMABLE"),
            instrument_id,
            entry.strategy_id(),
            vec![entry.client_order_id(), stop.client_order_id()],
            UnixNanos::default(),
        ),
        vec![
            OrderInitialized::from(&entry),
            OrderInitialized::from(&stop),
        ],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let ib_order_ids = AHashMap::from([
        (entry.client_order_id(), 7401),
        (stop.client_order_id(), 7402),
    ]);

    let error = InteractiveBrokersExecutionClient::prepare_order_list(
        &cmd,
        &orders,
        &ib_order_ids,
        &OrderTracker::new(NATIVE_CLIENT_ID),
        &instrument_provider,
        Ustr::from("DU001"),
    )
    .expect_err("an untransformable child fails preparation");

    assert_eq!(
        format!("{error:#}"),
        "Failed to transform order: `TrailingOffsetType` Ticks is not supported"
    );
}

#[tokio::test]
async fn list_submit_ambiguous_partial_failure_retains_attempted_children_only() {
    let state = SubmitTrackingState::new();
    let prior_id = 7301;
    let current_id = 7302;
    let tail_id = 7303;
    let prior_client_id = ClientOrderId::from("O-LIST-AMBIGUOUS-PRIOR");
    let current_client_id = ClientOrderId::from("O-LIST-AMBIGUOUS-CURRENT");
    let tail_client_id = ClientOrderId::from("O-LIST-AMBIGUOUS-TAIL");
    let prior_instrument_id = InstrumentId::new(Symbol::from("META"), Venue::from("SMART"));
    let current_instrument_id = InstrumentId::new(Symbol::from("GOOG"), Venue::from("SMART"));
    let trader_id = TraderId::from("TRADER-LIST-002");
    let strategy_id = StrategyId::from("STRATEGY-LIST-002");
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    state.cache(
        prior_id,
        prior_client_id,
        prior_instrument_id,
        trader_id,
        strategy_id,
    );
    state.cache(
        current_id,
        current_client_id,
        current_instrument_id,
        trader_id,
        strategy_id,
    );
    assert!(state.emit_accepted(
        prior_id,
        AccountId::from("IB-001"),
        &exec_sender.clone().into()
    ));
    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.client_order_id == prior_client_id
                && event.venue_order_id == VenueOrderId::from(prior_id.to_string())
    ));

    let result = InteractiveBrokersExecutionClient::handle_order_submit_failure(
        &ibapi::Error::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "socket closed after partial write",
        )),
        "Failed to submit order from list",
        current_id,
        AccountId::from("IB-001"),
        UnixNanos::new(37),
        &state.0,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
    );

    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to submit order from list; outcome is unknown after possible transmission: socket closed after partial write"
    );
    assert!(exec_receiver.try_recv().is_err());
    state.assert_active(
        prior_id,
        prior_client_id,
        prior_instrument_id,
        trader_id,
        strategy_id,
        true,
    );
    state.assert_active(
        current_id,
        current_client_id,
        current_instrument_id,
        trader_id,
        strategy_id,
        false,
    );
    state.assert_absent(tail_id, tail_client_id);

    process_submitted_status(current_id, &state, &exec_sender.clone().into()).await;

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.client_order_id == current_client_id
                && event.venue_order_id == VenueOrderId::from(current_id.to_string())
    ));
    assert!(exec_receiver.try_recv().is_err());
    state.assert_active(
        prior_id,
        prior_client_id,
        prior_instrument_id,
        trader_id,
        strategy_id,
        true,
    );
    state.assert_active(
        current_id,
        current_client_id,
        current_instrument_id,
        trader_id,
        strategy_id,
        true,
    );
    state.assert_absent(tail_id, tail_client_id);
}

#[rstest]
#[case::price(OrderType::TrailingStopMarket, Some(TrailingOffsetType::Price), None)]
#[case::basis_points(
    OrderType::TrailingStopLimit,
    Some(TrailingOffsetType::BasisPoints),
    None
)]
#[case::unset(OrderType::TrailingStopMarket, None, None)]
#[case::not_trailing(OrderType::Limit, Some(TrailingOffsetType::Ticks), None)]
#[case::ticks(
    OrderType::TrailingStopMarket,
    Some(TrailingOffsetType::Ticks),
    Some(
        "UNSUPPORTED_TRAILING_OFFSET_TYPE: `TrailingOffsetType` Ticks is not supported (only PRICE and BASIS_POINTS are supported)"
    )
)]
fn unsupported_trailing_offset_reason_allows_price_and_basis_points(
    #[case] order_type: OrderType,
    #[case] trailing_offset_type: Option<TrailingOffsetType>,
    #[case] expected: Option<&str>,
) {
    let reason = InteractiveBrokersExecutionClient::unsupported_trailing_offset_reason(
        order_type,
        trailing_offset_type,
    );

    assert_eq!(reason.as_deref(), expected);
}

#[rstest]
fn submit_order_denies_reduce_only() {
    let (client, mut rx, cache) = create_test_execution_client();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(create_test_stock_instrument())
        .client_order_id(ClientOrderId::from("O-IB-REDUCE-ONLY"))
        .side(OrderSide::Sell)
        .price(Price::from("100.00"))
        .quantity(Quantity::from(1))
        .reduce_only(true)
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*IB_CLIENT_ID), false)
        .unwrap();
    let cmd = SubmitOrder::from_order(
        &order,
        client.core.trader_id,
        Some(client.core.client_id),
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(cmd).unwrap();

    match next_order_event(&mut rx) {
        OrderEventAny::Denied(event) => {
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(event.reason, "UNSUPPORTED_REDUCE_ONLY");
        }
        event => panic!("Expected OrderDenied, was {event:?}"),
    }
    assert!(rx.try_recv().is_err());
}

#[rstest]
fn submit_order_denies_when_client_not_ready() {
    let (client, mut rx, cache) = create_test_execution_client();
    let order = create_test_limit_order(ClientOrderId::from("O-IB-001"));
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*IB_CLIENT_ID), false)
        .unwrap();
    let cmd = SubmitOrder::from_order(
        &order,
        client.core.trader_id,
        Some(client.core.client_id),
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(cmd).unwrap();

    match next_order_event(&mut rx) {
        OrderEventAny::Denied(event) => {
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(
                event.reason.to_string(),
                "IB_CLIENT_NOT_READY: Interactive Brokers client is not ready; refusing to submit order"
            );
        }
        event => panic!("Expected OrderDenied, was {event:?}"),
    }
}

#[rstest]
fn submit_order_list_denies_all_orders_when_reduce_only_is_present() {
    let (client, mut rx, cache) = create_test_execution_client();
    let reduce_only = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(create_test_stock_instrument())
        .client_order_id(ClientOrderId::from("O-IB-REDUCE-ONLY"))
        .side(OrderSide::Sell)
        .price(Price::from("100.00"))
        .quantity(Quantity::from(1))
        .reduce_only(true)
        .submit(true)
        .build();
    let regular = create_test_limit_order(ClientOrderId::from("O-IB-REGULAR"));
    for order in [&reduce_only, &regular] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*IB_CLIENT_ID), false)
            .unwrap();
    }
    let order_list = OrderList::new(
        OrderListId::from("OL-IB-REDUCE-ONLY"),
        reduce_only.instrument_id(),
        reduce_only.strategy_id(),
        vec![reduce_only.client_order_id(), regular.client_order_id()],
        UnixNanos::default(),
    );
    let cmd = SubmitOrderList::new(
        client.core.trader_id,
        Some(client.core.client_id),
        reduce_only.strategy_id(),
        order_list,
        vec![
            OrderInitialized::from(&reduce_only),
            OrderInitialized::from(&regular),
        ],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.submit_order_list(cmd).unwrap();

    for client_order_id in [reduce_only.client_order_id(), regular.client_order_id()] {
        match next_order_event(&mut rx) {
            OrderEventAny::Denied(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.reason, "UNSUPPORTED_REDUCE_ONLY");
            }
            event => panic!("Expected OrderDenied, was {event:?}"),
        }
    }
    assert!(rx.try_recv().is_err());
}

#[rstest]
fn submit_order_list_denies_all_orders_when_client_not_ready() {
    let (client, mut rx, cache) = create_test_execution_client();
    let order1 = create_test_limit_order(ClientOrderId::from("O-IB-001"));
    let order2 = create_test_limit_order(ClientOrderId::from("O-IB-002"));
    for order in [&order1, &order2] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*IB_CLIENT_ID), false)
            .unwrap();
    }
    let order_list = OrderList::new(
        OrderListId::from("OL-IB-001"),
        order1.instrument_id(),
        order1.strategy_id(),
        vec![order1.client_order_id(), order2.client_order_id()],
        UnixNanos::default(),
    );
    let cmd = SubmitOrderList::new(
        client.core.trader_id,
        Some(client.core.client_id),
        order1.strategy_id(),
        order_list,
        vec![
            OrderInitialized::from(&order1),
            OrderInitialized::from(&order2),
        ],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.submit_order_list(cmd).unwrap();

    for expected_client_order_id in [order1.client_order_id(), order2.client_order_id()] {
        match next_order_event(&mut rx) {
            OrderEventAny::Denied(event) => {
                assert_eq!(event.client_order_id, expected_client_order_id);
                assert_eq!(
                    event.reason.to_string(),
                    "IB_CLIENT_NOT_READY: Interactive Brokers client is not ready; refusing to submit order list"
                );
            }
            event => panic!("Expected OrderDenied, was {event:?}"),
        }
    }
}

#[rstest]
fn modify_order_rejects_when_client_not_ready() {
    let (client, mut rx, _) = create_test_execution_client();
    let order = create_test_limit_order(ClientOrderId::from("O-IB-001"));
    let cmd = ModifyOrder::new(
        client.core.trader_id,
        Some(client.core.client_id),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(VenueOrderId::from("1001")),
        Some(Quantity::from(2)),
        Some(Price::from("101.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.modify_order(cmd).unwrap();

    match next_order_event(&mut rx) {
        OrderEventAny::ModifyRejected(event) => {
            assert_eq!(event.trader_id, client.core.trader_id);
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(event.instrument_id, order.instrument_id());
            assert_eq!(event.strategy_id, order.strategy_id());
            assert_eq!(event.venue_order_id, Some(VenueOrderId::from("1001")));
            assert_eq!(event.account_id, Some(client.core.account_id));
            assert_eq!(event.ts_init, event.ts_event);
            assert!(!event.reconciliation);
            assert_eq!(event.causation_id, None);
            assert_eq!(
                event.reason.to_string(),
                "Interactive Brokers client is not ready; refusing to modify order"
            );
        }
        event => panic!("Expected OrderModifyRejected, was {event:?}"),
    }
    assert!(rx.try_recv().is_err());
}

#[rstest]
fn cancel_order_rejects_when_client_not_ready() {
    let (client, mut rx, cache) = create_test_execution_client();
    let order = create_test_limit_order(ClientOrderId::from("O-IB-001"));
    let accepted = OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from("1001"),
        client.core.account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    ));
    {
        let mut cache = cache.borrow_mut();
        cache
            .add_order(order.clone(), None, Some(client.core.client_id), false)
            .unwrap();
        cache.update_order(&accepted).unwrap();
    }
    let cmd = CancelOrder::new(
        client.core.trader_id,
        Some(client.core.client_id),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(VenueOrderId::from("1001")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(cmd).unwrap();

    match next_order_event(&mut rx) {
        OrderEventAny::CancelRejected(event) => {
            assert_eq!(event.trader_id, order.trader_id());
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(event.instrument_id, order.instrument_id());
            assert_eq!(event.strategy_id, order.strategy_id());
            assert_eq!(event.venue_order_id, Some(VenueOrderId::from("1001")));
            assert_eq!(event.account_id, Some(client.core.account_id));
            assert_eq!(event.ts_init, event.ts_event);
            assert!(!event.reconciliation);
            assert_eq!(event.causation_id, None);
            assert_eq!(
                event.reason.to_string(),
                "Interactive Brokers client is not ready; refusing to cancel order"
            );
        }
        event => panic!("Expected OrderCancelRejected, was {event:?}"),
    }
    assert!(rx.try_recv().is_err());
}

#[rstest]
fn cancel_all_orders_emits_no_events_when_client_not_ready() {
    let (client, mut rx, cache) = create_test_execution_client();
    let order = create_test_limit_order(ClientOrderId::from("O-IB-001"));
    let accepted = OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from("1001"),
        client.core.account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    ));
    {
        let mut cache = cache.borrow_mut();
        cache
            .add_order(order.clone(), None, Some(client.core.client_id), false)
            .unwrap();
        cache.update_order(&accepted).unwrap();
    }
    let cmd = CancelAllOrders::new(
        client.core.trader_id,
        Some(client.core.client_id),
        order.strategy_id(),
        order.instrument_id(),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_all_orders(cmd).unwrap();

    // A whole-request local failure must not become one rejection per order
    assert!(rx.try_recv().is_err(), "expected no events");
}

fn add_accepted_cache_order(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    strategy_id: &str,
    venue_order_id: &str,
    account_id: AccountId,
) {
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .strategy_id(StrategyId::from(strategy_id))
        .side(side)
        .price(Price::from("100.00"))
        .quantity(Quantity::from(1))
        .submit(true)
        .build();
    let accepted = OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from(venue_order_id),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    ));
    let mut cache = cache.borrow_mut();
    cache
        .add_order(order, None, Some(*IB_CLIENT_ID), false)
        .unwrap();
    cache.update_order(&accepted).unwrap();
}

fn insert_side_tracked_order(
    client: &InteractiveBrokersExecutionClient,
    order_id: i32,
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    strategy_id: &str,
    perm_id: i64,
) {
    let mut order =
        create_tracked_order_context(ClientOrderId::from(client_order_id), instrument_id);
    order.order_side = side;
    order.strategy_id = StrategyId::from(strategy_id);
    order.accepted = true;
    order.perm_id = perm_id;
    insert_tracked_order(&client.orders, order_id, order);
}

fn record_side_group(
    client: &InteractiveBrokersExecutionClient,
    client_order_id: &str,
    order_id: i32,
    perm_id: i64,
    side: OrderSide,
    account_id: AccountId,
) {
    let mut parent = create_tracked_order_context(
        ClientOrderId::from(client_order_id),
        create_test_stock_instrument(),
    );
    parent.order_side = side;
    parent.perm_id = perm_id;
    let mut sibling = create_test_open_order(order_id + 1, "Submitted", client_order_id);
    sibling.order.perm_id = perm_id + 1;
    client.orders.lock().unwrap().record_incarnation(
        order_id,
        &parent,
        account_id,
        perm_id + 1,
        Some(&sibling),
        false,
    );
}

#[rstest]
#[case::unsided(
    None,
    vec![
        ("O-CACHE-BUY", Some("1001")),
        ("O-CACHE-SELL", Some("1002")),
        ("O-GROUP-SELL", None),
        ("O-TRACK-BUY", Some("21")),
        ("O-TRACK-SELL", Some("PERM-222")),
    ],
)]
#[case::buy(
    Some(OrderSide::Buy),
    vec![("O-CACHE-BUY", Some("1001")), ("O-TRACK-BUY", Some("21"))],
)]
#[case::sell(
    Some(OrderSide::Sell),
    vec![
        ("O-CACHE-SELL", Some("1002")),
        ("O-GROUP-SELL", None),
        ("O-TRACK-SELL", Some("PERM-222")),
    ],
)]
fn cancel_all_targets_select_requested_side_across_sources(
    #[case] order_side: Option<OrderSide>,
    #[case] expected: Vec<(&str, Option<&str>)>,
) {
    let (client, _rx, cache) = create_test_execution_client();
    let account_id = client.core.account_id;
    let aapl = create_test_stock_instrument();
    let msft = InstrumentId::from("MSFT.SMART");
    add_accepted_cache_order(
        &cache,
        "O-CACHE-BUY",
        aapl,
        OrderSide::Buy,
        "S-001",
        "1001",
        account_id,
    );
    add_accepted_cache_order(
        &cache,
        "O-CACHE-SELL",
        aapl,
        OrderSide::Sell,
        "S-002",
        "1002",
        account_id,
    );
    add_accepted_cache_order(
        &cache,
        "O-CACHE-MSFT",
        msft,
        OrderSide::Sell,
        "S-002",
        "1003",
        account_id,
    );
    add_accepted_cache_order(
        &cache,
        "O-CACHE-OTHER-ACCOUNT",
        aapl,
        OrderSide::Sell,
        "S-002",
        "1004",
        AccountId::from("IB-002"),
    );
    // Tracked but not cached: the tracker is the fallback selection path
    insert_side_tracked_order(&client, 11, "O-CACHE-BUY", aapl, OrderSide::Buy, "S-001", 0);
    insert_side_tracked_order(&client, 21, "O-TRACK-BUY", aapl, OrderSide::Buy, "S-003", 0);
    insert_side_tracked_order(
        &client,
        22,
        "O-TRACK-SELL",
        aapl,
        OrderSide::Sell,
        "S-004",
        222,
    );
    insert_side_tracked_order(
        &client,
        23,
        "O-TRACK-MSFT",
        msft,
        OrderSide::Buy,
        "S-003",
        0,
    );
    record_side_group(
        &client,
        "O-GROUP-SELL",
        31,
        301,
        OrderSide::Sell,
        account_id,
    );
    record_side_group(
        &client,
        "O-GROUP-OTHER-ACCOUNT",
        41,
        401,
        OrderSide::Sell,
        AccountId::from("IB-002"),
    );
    let cmd = CancelAllOrders::new(
        client.core.trader_id,
        Some(client.core.client_id),
        StrategyId::from("S-001"),
        aapl,
        order_side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    let targets = client.cancel_all_targets(&cmd).unwrap();

    let expected: Vec<_> = expected
        .into_iter()
        .map(|(id, venue_id)| (ClientOrderId::from(id), venue_id.map(VenueOrderId::from)))
        .collect();
    assert_eq!(targets, expected);
}

#[rstest]
fn cancel_all_targets_sided_request_without_matches_selects_nothing() {
    let (client, _rx, cache) = create_test_execution_client();
    let account_id = client.core.account_id;
    let aapl = create_test_stock_instrument();
    add_accepted_cache_order(
        &cache,
        "O-CACHE-BUY",
        aapl,
        OrderSide::Buy,
        "S-001",
        "1001",
        account_id,
    );
    insert_side_tracked_order(&client, 21, "O-TRACK-BUY", aapl, OrderSide::Buy, "S-002", 0);
    record_side_group(&client, "O-GROUP-BUY", 31, 301, OrderSide::Buy, account_id);
    let cmd = CancelAllOrders::new(
        client.core.trader_id,
        Some(client.core.client_id),
        StrategyId::from("S-001"),
        aapl,
        Some(OrderSide::Sell),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    let targets = client.cancel_all_targets(&cmd).unwrap();

    assert_eq!(targets, vec![]);
}

fn create_test_execution_data(
    order_id: i32,
    execution_id: &str,
    shares: f64,
    price: f64,
    side: &str,
) -> ExecutionData {
    let contract = Contract {
        contract_id: 12345,
        symbol: IBSymbol::from("SPY"),
        security_type: SecurityType::Option,
        last_trade_date_or_contract_month: String::from("20250101"),
        strike: 400.0,
        right: Some(OptionRight::Call),
        multiplier: String::from("100"),
        exchange: Exchange::from("SMART"),
        currency: IBCurrency::from("USD"),
        local_symbol: String::from("SPY C400"),
        trading_class: String::new(),
        combo_legs: vec![],
        ..Default::default()
    };

    let execution = Execution {
        execution_id: execution_id.to_string(),
        order_id,
        time: String::from("20250101 08:00:00"),
        side: if side == "BOT" {
            ExecutionSide::Bought
        } else {
            ExecutionSide::Sold
        },
        shares,
        price,
        perm_id: 0,
        client_id: NATIVE_CLIENT_ID,
        liquidation: 0,
        account_number: String::from("001"),
        exchange: String::new(),
        cumulative_quantity: shares,
        average_price: price,
        order_reference: String::new(),
        ev_rule: String::new(),
        ev_multiplier: None,
        model_code: String::new(),
        last_liquidity: Liquidity::None,
        pending_price_revision: false,
        submitter: String::new(),
    };

    ExecutionData {
        request_id: 0,
        contract,
        execution,
    }
}

fn create_test_stock_execution_data(
    contract_id: i32,
    order_id: i32,
    execution_id: &str,
) -> ExecutionData {
    let contract = Contract {
        contract_id,
        symbol: IBSymbol::from("AAPL"),
        security_type: SecurityType::Stock,
        exchange: Exchange::from("SMART"),
        currency: IBCurrency::from("USD"),
        ..Default::default()
    };

    let execution = Execution {
        execution_id: execution_id.to_string(),
        order_id,
        time: String::from("20250101 08:00:00"),
        side: ExecutionSide::Bought,
        shares: 10.0,
        price: 150.25,
        perm_id: 0,
        client_id: NATIVE_CLIENT_ID,
        liquidation: 0,
        account_number: String::from("001"),
        exchange: String::new(),
        cumulative_quantity: 10.0,
        average_price: 150.25,
        order_reference: String::from("O-IB-001"),
        ev_rule: String::new(),
        ev_multiplier: None,
        model_code: String::new(),
        last_liquidity: Liquidity::None,
        pending_price_revision: false,
        submitter: String::new(),
    };

    ExecutionData {
        request_id: 0,
        contract,
        execution,
    }
}

fn create_test_bag_execution_data(order_id: i32, execution_id: &str) -> ExecutionData {
    let contract = Contract {
        symbol: IBSymbol::from("SPY"),
        security_type: SecurityType::Spread,
        exchange: Exchange::from("SMART"),
        currency: IBCurrency::from("USD"),
        combo_legs: vec![
            ibapi::contracts::ComboLeg {
                contract_id: 12345,
                ratio: 1,
                action: LegAction::Buy,
                exchange: String::from("SMART"),
                open_close: ibapi::contracts::ComboLegOpenClose::Same,
                short_sale_slot: 0,
                designated_location: String::new(),
                exempt_code: 0,
            },
            ibapi::contracts::ComboLeg {
                contract_id: 67890,
                ratio: 1,
                action: LegAction::Sell,
                exchange: String::from("SMART"),
                open_close: ibapi::contracts::ComboLegOpenClose::Same,
                short_sale_slot: 0,
                designated_location: String::new(),
                exempt_code: 0,
            },
        ],
        ..Default::default()
    };

    let execution = Execution {
        execution_id: execution_id.to_string(),
        order_id,
        time: String::from("20250101 08:00:00"),
        side: ExecutionSide::Bought,
        shares: 1.0,
        price: 1.25,
        perm_id: 0,
        client_id: NATIVE_CLIENT_ID,
        liquidation: 0,
        account_number: String::from("001"),
        exchange: String::new(),
        cumulative_quantity: 1.0,
        average_price: 1.25,
        order_reference: String::from("O-IB-SPREAD"),
        ev_rule: String::new(),
        ev_multiplier: None,
        model_code: String::new(),
        last_liquidity: Liquidity::None,
        pending_price_revision: false,
        submitter: String::new(),
    };

    ExecutionData {
        request_id: 0,
        contract,
        execution,
    }
}

fn create_test_option_spread() -> OptionSpread {
    OptionSpread::builder()
        .instrument_id(create_test_spread_instrument())
        .raw_symbol(Symbol::from("(1)SPY C400_((1))SPY C410"))
        .asset_class(AssetClass::Equity)
        .exchange(Ustr::from("SMART"))
        .underlying(Ustr::from("SPY"))
        .strategy_type(Ustr::from("VERTICAL"))
        .activation_ns(UnixNanos::new(0))
        .expiration_ns(UnixNanos::new(0))
        .currency(Currency::USD())
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(100))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::new(0))
        .ts_init(UnixNanos::new(0))
        .build()
        .unwrap()
}

fn create_test_order_status(order_id: i32, status: &str) -> IBOrderStatus {
    IBOrderStatus {
        order_id,
        status: OrderStatusKind::from_str(status).unwrap(),
        filled: 0.0,
        remaining: 0.0,
        average_fill_price: Some(0.0),
        perm_id: 0,
        parent_id: 0,
        last_fill_price: Some(0.0),
        client_id: NATIVE_CLIENT_ID,
        why_held: String::new(),
        market_cap_price: Some(0.0),
    }
}

fn create_test_open_order(order_id: i32, status: &str, order_ref: &str) -> IBOrderData {
    IBOrderData {
        order_id,
        contract: Contract {
            contract_id: 12345,
            symbol: IBSymbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("SMART"),
            currency: IBCurrency::from("USD"),
            ..Default::default()
        },
        order: IBOrder {
            account: String::from("001"),
            client_id: NATIVE_CLIENT_ID,
            order_ref: order_ref.to_string(),
            ..Default::default()
        },
        order_state: OrderState {
            status: OrderStatusKind::from_str(status).unwrap(),
            ..Default::default()
        },
    }
}

#[rstest]
#[case(false, "Submitted")]
#[case(true, "PreSubmitted")]
#[tokio::test]
async fn handle_order_update_ignores_deactivated_open_order(
    #[case] what_if: bool,
    #[case] status: &str,
) {
    let order_id = 7009;
    let client_order_id = ClientOrderId::from("O-DEACTIVATED");
    let equity = equity_aapl();
    let instrument_id = equity.id();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let position_tracker = create_position_tracker();
    let mut open_order = create_test_open_order(order_id, status, client_order_id.as_str());
    open_order.order.what_if = what_if;
    open_order.order.deactivate = true;
    open_order.order.total_quantity = 1.0;

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let result = InteractiveBrokersExecutionClient::handle_order_update(
        &OrderUpdate::OpenOrder(open_order),
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &position_tracker,
    )
    .await;

    result.unwrap();
    assert!(exec_receiver.try_recv().is_err());
    let state = orders.lock().unwrap();
    assert_eq!(state.order_id_map.get(&client_order_id), Some(&order_id));
    assert_eq!(
        state.venue_order_id_map.get(&order_id),
        Some(&client_order_id)
    );
    let order = state.active_orders.get(&order_id).unwrap();
    assert_eq!(order.instrument_id, instrument_id);
    assert!(!order.accepted);
}

#[rstest]
fn test_remove_order_tracking_clears_submit_identity() {
    let order_id = 7008;
    let client_order_id = ClientOrderId::from("O-SUBMIT-FAIL");
    let instrument_id = create_test_stock_instrument();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);

    InteractiveBrokersExecutionClient::cache_order_tracking(
        order_id,
        client_order_id,
        instrument_id,
        TraderId::from("TRADER-001"),
        StrategyId::from("STRATEGY-001"),
        OrderSide::Buy,
        OrderType::Limit,
        &orders,
    )
    .unwrap();

    InteractiveBrokersExecutionClient::remove_order_tracking(order_id, client_order_id, &orders)
        .unwrap();

    let state = orders.lock().unwrap();
    assert!(state.order_id_map.is_empty());
    assert!(state.venue_order_id_map.is_empty());
    assert!(state.active_orders.is_empty());
    assert!(state.terminal_orders.is_empty());
}

#[tokio::test]
async fn test_get_leg_position_standard_spread() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)SPY C400___((1))SPY C410"),
        Venue::from("SMART"),
    );
    let leg_id = InstrumentId::new(Symbol::from("SPY C400"), Venue::from("SMART"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 0); // First leg is at position 0
}

#[tokio::test]
async fn test_get_leg_position_second_leg() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)SPY C400___((1))SPY C410"),
        Venue::from("SMART"),
    );
    let leg_id = InstrumentId::new(Symbol::from("SPY C410"), Venue::from("SMART"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 1); // Second leg is at position 1
}

#[tokio::test]
async fn test_get_leg_position_ratio_spread() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)E4DN5 P6350___((2))E4DN5 P6355"),
        Venue::from("XCME"),
    );
    let leg_id = InstrumentId::new(Symbol::from("E4DN5 P6350"), Venue::from("XCME"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 0);
}

#[tokio::test]
async fn test_get_leg_position_not_found() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)SPY C400___((1))SPY C410"),
        Venue::from("SMART"),
    );
    let leg_id = InstrumentId::new(Symbol::from("SPY C420"), Venue::from("SMART"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    // Should fallback to position 0
    assert_eq!(result, 0);
}

#[rstest]
fn test_cached_instrument_ids_for_preload_deduplicates_spread_orders() {
    let instrument_provider = create_test_instrument_provider();
    let mut cache = Cache::default();
    let spread_instrument_id = create_test_spread_instrument();

    let order_one = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(spread_instrument_id)
        .side(OrderSide::Buy)
        .price(Price::from("1.00"))
        .quantity(Quantity::from(1))
        .build();
    let order_two = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(spread_instrument_id)
        .client_order_id(ClientOrderId::from("O-SPREAD-002"))
        .side(OrderSide::Buy)
        .price(Price::from("1.00"))
        .quantity(Quantity::from(2))
        .build();

    cache.add_order(order_one, None, None, false).unwrap();
    cache.add_order(order_two, None, None, false).unwrap();

    let spread_ids = InteractiveBrokersExecutionClient::cached_instrument_ids_for_preload(
        &cache,
        &instrument_provider,
        *IB_CLIENT_ID,
        AccountId::from("IB-DU123456"),
    );

    assert_eq!(spread_ids, vec![spread_instrument_id]);
}

#[rstest]
fn test_cached_instrument_ids_for_preload_includes_non_spread_orders() {
    let instrument_provider = create_test_instrument_provider();
    let mut cache = Cache::default();
    let instrument_id = InstrumentId::new(Symbol::from("AAPL"), Venue::from("SMART"));

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .price(Price::from("1.00"))
        .quantity(Quantity::from(1))
        .build();

    cache.add_order(order, None, None, false).unwrap();

    let spread_ids = InteractiveBrokersExecutionClient::cached_instrument_ids_for_preload(
        &cache,
        &instrument_provider,
        *IB_CLIENT_ID,
        AccountId::from("IB-DU123456"),
    );

    assert_eq!(spread_ids, vec![instrument_id]);
}

#[rstest]
fn test_parse_historical_fill_report_uses_provider_resolved_stock_venue() {
    let (client, _, _) = create_test_execution_client();
    let equity = equity_aapl();
    let instrument_id = equity.id();
    client
        .instrument_provider
        .insert_test_instrument(InstrumentAny::from(equity), 265598, 1);
    let exec_data = create_test_stock_execution_data(0, 123, "exec-aapl-001");
    let cmd = GenerateFillReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();

    let report = client
        .parse_historical_fill_report(&cmd, &exec_data, 1.25, "USD", UnixNanos::default())
        .unwrap();

    assert_eq!(report.instrument_id, instrument_id);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("O-IB-001"))
    );
    assert_eq!(report.trade_id, TradeId::from("exec-aapl-001"));
    assert_eq!(report.venue_order_id, VenueOrderId::from("123"));
    assert_eq!(report.last_qty, Quantity::from(10));
    assert_eq!(report.last_px, Price::from("150.25"));
}

#[rstest]
fn test_report_contract_resolution_preserves_canonical_opra_id() {
    let (client, _, _) = create_test_execution_client();
    let instrument_id = InstrumentId::from("SPY   250101C00400000.OPRA");
    client
        .instrument_provider
        .insert_test_contract_id_mapping(12_345, instrument_id);
    let exec_data = create_test_execution_data(123, "exec-opra-001", 1.0, 1.25, "BOT");

    let resolved = client
        .resolve_report_contract_instrument_id(&exec_data.contract)
        .unwrap();

    assert_eq!(resolved, instrument_id);
}

#[rstest]
fn test_parse_historical_fill_report_uses_cached_bag_spread_id() {
    let (client, _, _) = create_test_execution_client();
    let spread = create_test_option_spread();
    let instrument_id = spread.id;
    client
        .instrument_provider
        .insert_test_instrument(InstrumentAny::from(spread), 54321, 1);
    client
        .instrument_provider
        .insert_test_contract_id_mapping(12345, create_test_leg_instrument());
    client.instrument_provider.insert_test_contract_id_mapping(
        67890,
        InstrumentId::new(Symbol::from("SPY C410"), Venue::from("SMART")),
    );
    let exec_data = create_test_bag_execution_data(7001, "exec-spread-001");
    let cmd = GenerateFillReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();

    let report = client
        .parse_historical_fill_report(&cmd, &exec_data, 2.00, "USD", UnixNanos::default())
        .unwrap();

    assert_eq!(report.instrument_id, instrument_id);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("O-IB-SPREAD"))
    );
    assert_eq!(report.trade_id, TradeId::from("exec-spread-001"));
    assert_eq!(report.venue_order_id, VenueOrderId::from("7001"));
    assert_eq!(report.last_qty, Quantity::from(1));
    assert_eq!(report.last_px, Price::from("1.25"));
}

#[tokio::test]
async fn test_handle_spread_execution_emits_only_leg_fill_event() {
    let instrument_provider = create_test_instrument_provider();
    let mut leg = equity_aapl();
    leg.id = create_test_leg_instrument();
    let spread = create_test_option_spread();
    let spread_instrument_id = spread.id;
    let instrument_id = leg.id;
    instrument_provider.insert_test_instrument(InstrumentAny::from(leg), 12345, 1);
    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);

    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let context = create_tracked_order_context(client_order_id, spread_instrument_id);
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(0);

    insert_tracked_order(&orders, exec_data.execution.order_id, context.clone());

    let fill = SpreadFillContext {
        client_order_id,
        spread_instrument_id,
        commission: 1.0,
        commission_currency: "USD",
        ts_init,
        account_id,
    };
    InteractiveBrokersExecutionClient::handle_spread_execution(
        &exec_data,
        &fill,
        &instrument_provider,
        &exec_sender.clone().into(),
        &orders,
        &context,
    )
    .await
    .unwrap();

    let leg_event = exec_receiver.try_recv().unwrap();
    let leg_position =
        InteractiveBrokersExecutionClient::get_leg_position(&spread_instrument_id, &instrument_id);

    match leg_event {
        ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
            assert_eq!(fill.trader_id, context.trader_id);
            assert_eq!(fill.strategy_id, context.strategy_id);
            assert_eq!(fill.instrument_id, instrument_id);
            assert_eq!(
                fill.client_order_id,
                ClientOrderId::from("O-001-LEG-SPY C400")
            );
            assert_eq!(
                fill.venue_order_id,
                VenueOrderId::new(format!(
                    "{}-LEG-{leg_position}",
                    exec_data.execution.order_id
                ))
            );
            assert_eq!(fill.account_id, account_id);
            assert_eq!(
                fill.trade_id,
                TradeId::new(format!("exec-001-{leg_position}"))
            );
            assert_eq!(fill.order_side, OrderSide::Buy);
            assert_eq!(fill.last_qty, Quantity::from(3));
            assert_eq!(fill.last_px, Price::from("5.25"));
            assert_eq!(fill.commission, Some(Money::from("1.00 USD")));
            assert_eq!(fill.position_id, None);
        }
        other => panic!("unexpected leg event: {other:?}"),
    }
    assert!(exec_receiver.try_recv().is_err());
    let state = orders.lock().unwrap();
    let order = state.order(exec_data.execution.order_id).unwrap();
    assert_eq!(order.spread_fill_ids.len(), 1);
}

#[tokio::test]
async fn test_handle_spread_execution_rejects_non_leg_execution() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let spread = create_test_option_spread();
    let spread_instrument_id = spread.id;
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let context = create_tracked_order_context(client_order_id, spread_instrument_id);
    insert_tracked_order(&orders, exec_data.execution.order_id, context.clone());
    let fill = SpreadFillContext {
        client_order_id,
        spread_instrument_id,
        commission: 1.0,
        commission_currency: "USD",
        ts_init: UnixNanos::new(0),
        account_id: AccountId::from("IB-001"),
    };

    let error = InteractiveBrokersExecutionClient::handle_spread_execution(
        &exec_data,
        &fill,
        &instrument_provider,
        &exec_sender.clone().into(),
        &orders,
        &context,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("Execution instrument AAPL.XNAS is not a leg of spread {spread_instrument_id}")
    );
    assert!(exec_receiver.try_recv().is_err());
}

#[tokio::test]
async fn test_handle_spread_execution_duplicate_detection() {
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, _exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);

    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let spread_instrument_id = create_test_spread_instrument();
    let context = create_tracked_order_context(client_order_id, spread_instrument_id);
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(0);

    insert_tracked_order(&orders, exec_data.execution.order_id, context.clone());
    // Pre-populate tracking with the fill ID to simulate duplicate
    {
        let mut state = orders.lock().unwrap();
        state
            .order_mut(exec_data.execution.order_id)
            .unwrap()
            .spread_fill_ids
            .insert("exec-001".to_string());
    }

    let fill = SpreadFillContext {
        client_order_id,
        spread_instrument_id,
        commission: 1.0,
        commission_currency: "USD",
        ts_init,
        account_id,
    };
    let result = InteractiveBrokersExecutionClient::handle_spread_execution(
        &exec_data,
        &fill,
        &instrument_provider,
        &exec_sender.clone().into(),
        &orders,
        &context,
    )
    .await;

    // Should return Ok(()) immediately without processing duplicate
    assert!(result.is_ok());
}

#[rstest]
fn test_update_order_avg_price_allows_negative_spread_avg_fill_price() {
    let order_id = 7002;
    let instrument_provider = create_test_instrument_provider();
    let spread = create_test_option_spread();
    let spread_instrument_id = spread.id;
    let client_order_id = ClientOrderId::from("O-COMBO-NEG-001");
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);

    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, spread_instrument_id),
    );

    InteractiveBrokersExecutionClient::update_order_avg_price(
        order_id,
        &spread_instrument_id,
        -2.25,
        3.0,
        &instrument_provider,
        &orders,
    )
    .unwrap();

    let state = orders.lock().unwrap();
    let order = state.order(order_id).unwrap();
    let avg_px = order.avg_px.unwrap();
    assert_eq!(avg_px, Price::from("-2.25"));
}

#[rstest]
fn test_track_pending_cancel_marks_once_without_emitting() {
    let order_id = 7002;
    let client_order_id = ClientOrderId::from("O-CANCEL-002");
    let instrument_id = create_test_spread_instrument();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let tracked = create_tracked_order_context(client_order_id, instrument_id);
    let (trader_id, strategy_id) = (tracked.trader_id, tracked.strategy_id);
    insert_tracked_order(&orders, order_id, tracked);

    let first =
        InteractiveBrokersExecutionClient::track_pending_cancel(client_order_id, &orders).unwrap();
    let second =
        InteractiveBrokersExecutionClient::track_pending_cancel(client_order_id, &orders).unwrap();

    assert_eq!(first, Some((trader_id, strategy_id, instrument_id)));
    assert_eq!(second, None);
    assert!(orders.lock().unwrap().active_orders[&order_id].pending_cancel);
}

#[rstest]
fn test_emit_order_pending_cancel_is_idempotent() {
    let order_id = 7001;
    let client_order_id = ClientOrderId::from("O-CANCEL-001");
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, create_test_spread_instrument()),
    );

    InteractiveBrokersExecutionClient::emit_order_pending_cancel(
        order_id,
        client_order_id,
        VenueOrderId::from(order_id.to_string()),
        &orders,
        &exec_sender.clone().into(),
        UnixNanos::new(1),
        AccountId::from("IB-001"),
    )
    .unwrap();
    InteractiveBrokersExecutionClient::emit_order_pending_cancel(
        order_id,
        client_order_id,
        VenueOrderId::from(order_id.to_string()),
        &orders,
        &exec_sender.clone().into(),
        UnixNanos::new(1),
        AccountId::from("IB-001"),
    )
    .unwrap();

    let first = exec_receiver.try_recv().unwrap();
    assert!(matches!(
        first,
        ExecutionEvent::Order(OrderEventAny::PendingCancel(_))
    ));
    assert!(exec_receiver.try_recv().is_err());
    assert!(
        orders
            .lock()
            .unwrap()
            .order(order_id)
            .unwrap()
            .pending_cancel
    );
}

#[tokio::test]
async fn test_handle_order_status_canceled_emits_canceled_event() {
    let instrument_provider = create_test_instrument_provider();
    let spread = create_test_option_spread();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let order_id = 7001;
    let client_order_id = ClientOrderId::from("O-CANCEL-002");
    let instrument_id = spread.id;

    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);

    let mut order = create_tracked_order_context(client_order_id, instrument_id);
    order.pending_cancel = true;
    insert_tracked_order(&orders, order_id, order);
    let mut status = create_test_order_status(order_id, "Cancelled");
    status.filled = 1.0;
    status.average_fill_price = Some(2.25);

    InteractiveBrokersExecutionClient::handle_order_status(
        &status,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        UnixNanos::new(1),
        AccountId::from("IB-001"),
    )
    .await
    .unwrap();

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.client_order_id == client_order_id
    ));

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.instrument_id, instrument_id);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    let state = orders.lock().unwrap();
    assert!(state.order_id_map.is_empty());
    assert!(state.venue_order_id_map.is_empty());
    assert!(state.active_orders.is_empty());
    let order = state.terminal_orders.get(&order_id).unwrap();
    assert!(!order.pending_cancel);
}

#[tokio::test]
async fn test_opra_cancel_status_preserves_canonical_instrument_identity() {
    let state = SubmitTrackingState::new();
    let order_id = 7_002;
    let client_order_id = ClientOrderId::from("O-OPRA-CANCEL-001");
    let instrument_id = InstrumentId::from("SPY   250101C00400000.OPRA");
    state.cache(
        order_id,
        client_order_id,
        instrument_id,
        TraderId::from("TRADER-001"),
        StrategyId::from("STRATEGY-001"),
    );

    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    InteractiveBrokersExecutionClient::handle_order_status(
        &create_test_order_status(order_id, "Cancelled"),
        &state.0,
        &instrument_provider,
        &exec_sender.clone().into(),
        UnixNanos::new(1),
        AccountId::from("IB-001"),
    )
    .await
    .unwrap();

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.instrument_id == instrument_id
    ));
    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Canceled(event))
            if event.instrument_id == instrument_id
                && event.client_order_id == client_order_id
    ));
    assert!(exec_receiver.try_recv().is_err());
    let state = state.0.lock().unwrap();
    assert_eq!(
        state.terminal_orders.get(&order_id).unwrap().instrument_id,
        instrument_id
    );
}

#[tokio::test]
async fn test_process_order_update_stream_emits_accepted_then_canceled() {
    let instrument_provider = create_test_instrument_provider();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let position_tracker = create_position_tracker();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);
    let order_id = 7002;
    let client_order_id = ClientOrderId::from("O-STREAM-001");
    let instrument_id = create_test_spread_instrument();

    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    update_sender
        .send(Ok(OrderUpdate::OpenOrder(create_test_open_order(
            order_id,
            "Submitted",
            "",
        ))))
        .unwrap();
    update_sender
        .send(Ok(OrderUpdate::OrderStatus(create_test_order_status(
            order_id,
            "Cancelled",
        ))))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &position_tracker,
        None,
    )
    .await;

    let accepted_event = exec_receiver.try_recv().unwrap();
    assert!(matches!(
        accepted_event,
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));

    let canceled_event = exec_receiver.try_recv().unwrap();
    assert!(matches!(
        canceled_event,
        ExecutionEvent::Order(OrderEventAny::Canceled(_))
    ));
}

#[tokio::test]
async fn test_process_order_update_stream_clears_market_order_update_prices() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let order_id = 7005;
    let contract_id = 12347;
    let client_order_id = ClientOrderId::from("O-STREAM-MKT-UPDATE");
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);

    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let mut open_order = create_test_open_order(order_id, "Submitted", "");
    open_order.contract.contract_id = contract_id;
    open_order.order.total_quantity = 10.0;
    open_order.order.order_type = "MKT".to_string();
    open_order.order.limit_price = Some(150.25);
    open_order.order.aux_price = Some(149.75);

    update_sender
        .send(Ok(OrderUpdate::OpenOrder(open_order)))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &create_position_tracker(),
        None,
    )
    .await;

    let accepted_event = exec_receiver.try_recv().unwrap();
    assert!(matches!(
        accepted_event,
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));

    let updated_event = exec_receiver.try_recv().unwrap();
    match updated_event {
        ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.quantity, Quantity::from(10));
            assert_eq!(event.price, None);
            assert_eq!(event.trigger_price, None);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn execution_without_commission_flushes_with_zero_marker() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let order_id = 7008;
    let contract_id = 12349;
    let client_order_id = ClientOrderId::from("O-STREAM-NO-COMMISSION");
    let position_tracker = create_position_tracker();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let mut exec_data =
        create_test_execution_data(order_id, "exec-no-commission", 1.0, 50.0, "BOT");
    exec_data.contract.contract_id = contract_id;
    exec_data.contract.security_type = SecurityType::Stock;
    exec_data.contract.symbol = IBSymbol::from("AAPL");
    exec_data.contract.exchange = Exchange::from("SMART");
    exec_data.contract.currency = IBCurrency::from("USD");
    pending_execution_cache.lock().insert(
        exec_data.execution.execution_id.clone(),
        (
            tokio::time::Instant::now() - Duration::from_secs(5),
            exec_data,
        ),
    );

    InteractiveBrokersExecutionClient::flush_executions_without_commission(
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &position_tracker,
    )
    .await
    .unwrap();

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.client_order_id == client_order_id
    ));
    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Filled(event))
            if event.client_order_id == client_order_id
                && event.commission == Some(Money::new(0.0, Currency::USD()))
    ));
    assert!(exec_receiver.try_recv().is_err());
    assert!(pending_execution_cache.lock().is_empty());
    assert!(commission_cache.lock().is_empty());
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_process_order_update_stream_emits_fill_after_commission_report(
    #[case] already_accepted: bool,
) {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let order_id = 7003;
    let contract_id = 12345;
    let client_order_id = ClientOrderId::from("O-STREAM-002");
    let position_tracker = create_position_tracker();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);

    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    let mut context = create_tracked_order_context(client_order_id, instrument_id);
    context.accepted = already_accepted;
    insert_tracked_order(&orders, order_id, context);

    let mut exec_data = create_test_execution_data(order_id, "exec-stream-001", 100.0, 50.0, "BOT");
    exec_data.contract.contract_id = contract_id;
    exec_data.contract.security_type = SecurityType::Stock;
    exec_data.contract.symbol = IBSymbol::from("AAPL");
    exec_data.contract.exchange = Exchange::from("SMART");
    exec_data.contract.currency = IBCurrency::from("USD");
    exec_data.execution.order_reference.clear();

    update_sender
        .send(Ok(OrderUpdate::ExecutionData(exec_data)))
        .unwrap();
    update_sender
        .send(Ok(OrderUpdate::CommissionReport(CommissionReport {
            execution_id: String::from("exec-stream-001"),
            commission: 1.25,
            currency: String::from("USD"),
            realized_pnl: None,
            yields: None,
            yield_redemption_date: String::new(),
        })))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &position_tracker,
        None,
    )
    .await;

    if !already_accepted {
        let accepted_event = exec_receiver.try_recv().unwrap();
        assert!(matches!(
            accepted_event,
            ExecutionEvent::Order(OrderEventAny::Accepted(event))
                if event.client_order_id == client_order_id
        ));
    }

    let fill_event = exec_receiver.try_recv().unwrap();
    match fill_event {
        ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
            assert_eq!(fill.trader_id, TraderId::from("TRADER-001"));
            assert_eq!(fill.strategy_id, StrategyId::from("STRATEGY-001"));
            assert_eq!(fill.client_order_id, client_order_id);
            assert_eq!(fill.instrument_id, instrument_id);
            assert_eq!(fill.trade_id, TradeId::from("exec-stream-001"));
            assert_eq!(fill.order_side, OrderSide::Buy);
            assert_eq!(fill.order_type, OrderType::Limit);
            assert_eq!(fill.last_qty, Quantity::from(100));
            assert_eq!(fill.last_px, Price::from("50"));
            assert_eq!(fill.currency, Currency::USD());
            assert_eq!(fill.commission, Some(Money::new(1.25, Currency::USD())));
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(commission_cache.lock().is_empty());
    assert_eq!(
        check_external_position_change(&position_tracker, contract_id, Decimal::from(100),).await,
        None,
    );
    assert!(exec_receiver.try_recv().is_err());
}

#[tokio::test]
async fn test_process_order_update_stream_retains_terminal_identity_for_late_fill() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let order_id = 7006;
    let contract_id = 12348;
    let client_order_id = ClientOrderId::from("O-STREAM-LATE-FILL");
    let instrument_id = equity.id();
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let mut exec_data =
        create_test_execution_data(order_id, "exec-stream-late", 100.0, 50.0, "BOT");
    exec_data.contract.contract_id = contract_id;
    exec_data.contract.security_type = SecurityType::Stock;
    exec_data.contract.symbol = IBSymbol::from("AAPL");
    exec_data.contract.exchange = Exchange::from("SMART");
    exec_data.contract.currency = IBCurrency::from("USD");
    exec_data.execution.order_reference.clear();

    update_sender
        .send(Ok(OrderUpdate::OrderStatus(create_test_order_status(
            order_id, "Filled",
        ))))
        .unwrap();
    update_sender
        .send(Ok(OrderUpdate::ExecutionData(exec_data)))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &create_position_tracker(),
        None,
    )
    .await;

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.client_order_id == client_order_id
    ));
    assert!(
        pending_execution_cache
            .lock()
            .contains_key(&String::from("exec-stream-late"))
    );
    assert!(exec_receiver.try_recv().is_err());

    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);
    update_sender
        .send(Ok(OrderUpdate::CommissionReport(CommissionReport {
            execution_id: String::from("exec-stream-late"),
            commission: 1.25,
            currency: String::from("USD"),
            realized_pnl: None,
            yields: None,
            yield_redemption_date: String::new(),
        })))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &create_position_tracker(),
        None,
    )
    .await;

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Filled(event))
            if event.client_order_id == client_order_id
                && event.trade_id == TradeId::from("exec-stream-late")
    ));
    assert!(exec_receiver.try_recv().is_err());
    assert!(pending_execution_cache.lock().is_empty());
    let state = orders.lock().unwrap();
    assert!(state.active_orders.is_empty());
    let terminal_context = state.terminal_orders.get(&order_id).unwrap();
    assert!(terminal_context.accepted);
    assert_eq!(terminal_context.client_order_id, client_order_id);
}

#[tokio::test]
async fn test_process_order_update_stream_retains_terminal_combo_routing() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let spread = create_test_option_spread();
    let order_id = 7007;
    let contract_id = 12345;
    let client_order_id = ClientOrderId::from("O-STREAM-LATE-COMBO");
    let instrument_id = spread.id;
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);
    insert_tracked_order(
        &orders,
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let mut status = create_test_order_status(order_id, "Filled");
    status.filled = 1.0;
    status.average_fill_price = Some(2.25);
    let mut exec_data =
        create_test_execution_data(order_id, "exec-stream-late-combo", 1.0, 5.25, "BOT");
    exec_data.contract.contract_id = 54321;
    exec_data.contract.security_type = SecurityType::Spread;
    exec_data.contract.combo_legs = create_test_bag_execution_data(order_id, "unused")
        .contract
        .combo_legs;
    exec_data.execution.order_reference.clear();
    let replay_exec_data = exec_data.clone();

    update_sender
        .send(Ok(OrderUpdate::OrderStatus(status)))
        .unwrap();
    update_sender
        .send(Ok(OrderUpdate::ExecutionData(exec_data)))
        .unwrap();
    update_sender
        .send(Ok(OrderUpdate::CommissionReport(CommissionReport {
            execution_id: String::from("exec-stream-late-combo"),
            commission: 1.25,
            currency: String::from("USD"),
            realized_pnl: None,
            yields: None,
            yield_redemption_date: String::new(),
        })))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &create_position_tracker(),
        None,
    )
    .await;

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Accepted(event))
            if event.client_order_id == client_order_id
    ));
    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Filled(event))
            if event.client_order_id == client_order_id
                && event.instrument_id == instrument_id
                && event.last_px == Price::from("5.25")
    ));
    assert!(exec_receiver.try_recv().is_err());
    {
        let state = orders.lock().unwrap();
        assert!(state.active_orders.is_empty());
        assert!(state.terminal_orders.contains_key(&order_id));
    }

    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);
    update_sender
        .send(Ok(OrderUpdate::ExecutionData(replay_exec_data)))
        .unwrap();
    update_sender
        .send(Ok(OrderUpdate::CommissionReport(CommissionReport {
            execution_id: String::from("exec-stream-late-combo"),
            commission: 1.25,
            currency: String::from("USD"),
            realized_pnl: None,
            yields: None,
            yield_redemption_date: String::new(),
        })))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &create_position_tracker(),
        None,
    )
    .await;

    assert!(exec_receiver.try_recv().is_err());
}

#[tokio::test]
async fn test_process_order_update_stream_preserves_execution_reference_without_changing_routes() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let order_id = 7004;
    let contract_id = 12346;
    let client_order_id = ClientOrderId::from("O-STREAM-EXEC-REF");
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = ChannelSubscription::new(update_receiver);

    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);

    let mut exec_data = create_test_execution_data(order_id, "exec-stream-002", 100.0, 50.0, "BOT");
    exec_data.contract.contract_id = contract_id;
    exec_data.contract.security_type = SecurityType::Stock;
    exec_data.contract.symbol = IBSymbol::from("AAPL");
    exec_data.contract.exchange = Exchange::from("SMART");
    exec_data.contract.currency = IBCurrency::from("USD");
    exec_data.execution.order_reference = client_order_id.to_string();
    exec_data.execution.perm_id = 90210;

    update_sender
        .send(Ok(OrderUpdate::ExecutionData(exec_data)))
        .unwrap();
    update_sender
        .send(Ok(OrderUpdate::CommissionReport(CommissionReport {
            execution_id: String::from("exec-stream-002"),
            commission: 1.25,
            currency: String::from("USD"),
            realized_pnl: None,
            yields: None,
            yield_redemption_date: String::new(),
        })))
        .unwrap();
    drop(update_sender);

    InteractiveBrokersExecutionClient::process_order_update_stream(
        &mut subscription,
        &orders,
        &instrument_provider,
        &exec_sender.clone().into(),
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        Ustr::from("001"),
        &commission_cache,
        &pending_execution_cache,
        &create_position_tracker(),
        None,
    )
    .await;

    let fill_event = exec_receiver.try_recv().unwrap();
    match fill_event {
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
            assert_eq!(fill.client_order_id, Some(client_order_id));
            assert_eq!(fill.instrument_id, instrument_id);
            assert_eq!(fill.account_id, AccountId::from("IB-001"));
            assert_eq!(fill.venue_order_id, VenueOrderId::from("PERM-90210"));
            assert_eq!(fill.trade_id, TradeId::from("exec-stream-002"));
            assert_eq!(fill.order_side, OrderSide::Buy);
            assert_eq!(fill.last_qty, Quantity::from(100));
            assert_eq!(fill.last_px, Price::from("50.00"));
            assert_eq!(fill.commission, Money::from("1.25 USD"));
        }
        other => panic!("unexpected event: {other:?}"),
    }
    let state = orders.lock().unwrap();
    assert_eq!(state.venue_order_id_map.get(&order_id), None);
    assert_eq!(state.order_id_map.get(&client_order_id), None);
}

#[rstest]
fn test_get_leg_position_edge_cases() {
    // Test with single component (no spread)
    let spread_id = InstrumentId::new(Symbol::from("SPY C400"), Venue::from("SMART"));
    let leg_id = InstrumentId::new(Symbol::from("SPY C400"), Venue::from("SMART"));
    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 0); // Should fallback to 0

    // Test with invalid format
    let spread_id = InstrumentId::new(Symbol::from("INVALID_FORMAT"), Venue::from("SMART"));
    let leg_id = InstrumentId::new(Symbol::from("SPY C400"), Venue::from("SMART"));
    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 0); // Should fallback to 0
}

#[rstest]
fn test_get_leg_position_three_leg_spread() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)LEG1___((1))LEG2___((2))LEG3"),
        Venue::from("SMART"),
    );

    // Test first leg
    let leg_id1 = InstrumentId::new(Symbol::from("LEG1"), Venue::from("SMART"));
    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id1);
    assert_eq!(result, 0);

    // Test second leg
    let leg_id2 = InstrumentId::new(Symbol::from("LEG2"), Venue::from("SMART"));
    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id2);
    assert_eq!(result, 1);

    // Test third leg
    let leg_id3 = InstrumentId::new(Symbol::from("LEG3"), Venue::from("SMART"));
    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id3);
    assert_eq!(result, 2);
}

#[rstest]
#[case(1, 22, vec![(101, 11), (202, 22)], vec![])]
#[case(1, 11, vec![], vec![101, 202])]
#[case(9, 22, vec![(101, 11)], vec![202])]
fn duplicate_orders_cancel_only_unique_owned_routes(
    #[case] sibling_client: i32,
    #[case] sibling_raw_id: i32,
    #[case] expected_routes: Vec<(i64, i32)>,
    #[case] expected_unresolved: Vec<i64>,
) {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let parent_id = ClientOrderId::from("O-PRIMARY");
    let mut parent = create_tracked_order_context(parent_id, create_test_stock_instrument());
    parent.perm_id = 101;
    insert_tracked_order(&orders, 11, parent.clone());
    let mut primary = create_test_open_order(11, "Submitted", parent_id.as_str());
    primary.order.perm_id = 101;
    let mut sibling = create_test_open_order(sibling_raw_id, "Submitted", parent_id.as_str());
    sibling.order.client_id = sibling_client;
    sibling.order.perm_id = 202;
    let mut state = orders.lock().unwrap();
    state.record_incarnation(
        11,
        &parent,
        AccountId::from("IB-001"),
        101,
        Some(&primary),
        false,
    );
    state.record_incarnation(
        11,
        &parent,
        AccountId::from("IB-001"),
        202,
        Some(&sibling),
        false,
    );
    assert_eq!(
        state.group_cancel_routes(parent_id, None),
        (expected_routes, expected_unresolved)
    );
}

#[rstest]
fn binding_notification_resolves_a_duplicate_order_route() {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let parent_id = ClientOrderId::from("O-PRIMARY");
    let mut parent = create_tracked_order_context(parent_id, create_test_stock_instrument());
    parent.perm_id = 101;
    let mut data = create_test_open_order(11, "Submitted", parent_id.as_str());
    data.order.perm_id = 101;
    let mut state = orders.lock().unwrap();
    state.record_incarnation(
        11,
        &parent,
        AccountId::from("IB-001"),
        101,
        Some(&data),
        false,
    );
    data.order.perm_id = 202;
    state.record_incarnation(
        11,
        &parent,
        AccountId::from("IB-001"),
        202,
        Some(&data),
        false,
    );
    assert_eq!(
        state.group_cancel_routes(parent_id, None),
        (vec![], vec![101, 202])
    );
    state.observe_order_binding(&ibapi::orders::OrderBound {
        perm_id: 202,
        client_id: NATIVE_CLIENT_ID,
        order_id: 22,
    });
    assert_eq!(
        state.group_cancel_routes(parent_id, None),
        (vec![(101, 11), (202, 22)], vec![])
    );
}

#[rstest]
#[case::unsided(None, vec![(101, 11), (202, 22), (303, 33)])]
#[case::buy(Some(OrderSide::Buy), vec![(101, 11)])]
#[case::sell(Some(OrderSide::Sell), vec![(303, 33)])]
fn group_cancel_routes_skip_members_off_the_requested_side(
    #[case] order_side: Option<OrderSide>,
    #[case] expected_routes: Vec<(i64, i32)>,
) {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let parent_id = ClientOrderId::from("O-PRIMARY");
    let mut parent = create_tracked_order_context(parent_id, create_test_stock_instrument());
    parent.perm_id = 101;
    let mut primary = create_test_open_order(11, "Submitted", parent_id.as_str());
    primary.order.perm_id = 101;
    primary.order.action = ibapi::orders::Action::Buy;
    let mut sell = create_test_open_order(33, "Submitted", parent_id.as_str());
    sell.order.perm_id = 303;
    sell.order.action = ibapi::orders::Action::Sell;
    let account_id = AccountId::from("IB-001");
    let mut state = orders.lock().unwrap();
    state.record_incarnation(11, &parent, account_id, 101, Some(&primary), false);
    // Bound without order data, so its broker side is unknown
    state.record_incarnation(11, &parent, account_id, 202, None, false);
    state.observe_order_binding(&ibapi::orders::OrderBound {
        perm_id: 202,
        client_id: NATIVE_CLIENT_ID,
        order_id: 22,
    });
    state.record_incarnation(11, &parent, account_id, 303, Some(&sell), false);

    assert_eq!(
        state.group_cancel_routes(parent_id, order_side),
        (expected_routes, vec![])
    );
}

#[rstest]
fn conflicting_routes_for_one_permanent_id_remain_unresolved() {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let parent_id = ClientOrderId::from("O-PRIMARY");
    let mut parent = create_tracked_order_context(parent_id, create_test_stock_instrument());
    parent.perm_id = 101;
    let mut data = create_test_open_order(11, "Submitted", parent_id.as_str());
    data.order.perm_id = 101;
    let mut state = orders.lock().unwrap();
    state.record_incarnation(
        11,
        &parent,
        AccountId::from("IB-001"),
        101,
        Some(&data),
        false,
    );
    data.order_id = 22;
    state.record_incarnation(
        11,
        &parent,
        AccountId::from("IB-001"),
        101,
        Some(&data),
        false,
    );
    assert_eq!(
        state.group_cancel_routes(parent_id, None),
        (vec![], vec![101])
    );
}

#[rstest]
fn callback_identity_does_not_alias_another_api_clients_raw_id() {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let id = ClientOrderId::from("O-PRIMARY");
    let mut parent = create_tracked_order_context(id, create_test_stock_instrument());
    parent.perm_id = 101;
    insert_tracked_order(&orders, 11, parent);
    let state = orders.lock().unwrap();
    assert!(matches!(
        state.correlate(9, 11, 202, "").unwrap(),
        OrderCorrelation::Untracked {
            client_order_id: None
        }
    ));
    assert!(state.correlate(1, 11, 101, "ANOTHER-ORDER").is_err());
    let OrderCorrelation::Tracked { order_id, context } =
        state.correlate(9, 77, 101, id.as_str()).unwrap()
    else {
        panic!("known permanent ID must retain its original context");
    };
    assert_eq!(
        (order_id, context.client_order_id, context.perm_id),
        (11, id, 101)
    );
    assert_eq!(state.order_id_map.get(&id), Some(&11));
}

#[rstest]
#[case(OrderStatusKind::Submitted)]
#[case(OrderStatusKind::Cancelled)]
fn late_sibling_status_does_not_reopen_a_filled_order(#[case] late: OrderStatusKind) {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let id = ClientOrderId::from("O-PRIMARY");
    let mut parent = create_tracked_order_context(id, create_test_stock_instrument());
    parent.perm_id = 101;
    let mut data = create_test_open_order(22, "Filled", id.as_str());
    data.order.perm_id = 202;
    data.order.filled_quantity = 2.0;
    let mut state = orders.lock().unwrap();
    state.record_incarnation(
        11,
        &parent,
        AccountId::from("IB-001"),
        202,
        Some(&data),
        true,
    );
    let status = IBOrderStatus {
        order_id: 22,
        perm_id: 202,
        client_id: NATIVE_CLIENT_ID,
        status: late,
        filled: 0.0,
        ..Default::default()
    };
    let (_, snapshot) = state.observe_incarnation_status(&status).unwrap();
    assert_eq!(snapshot.order_state.status, OrderStatusKind::Filled);
    assert_eq!(snapshot.order.filled_quantity, 2.0);
}

#[rstest]
fn cancel_all_includes_working_siblings_after_the_parent_closes() {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let id = ClientOrderId::from("O-PRIMARY");
    let instrument_id = create_test_stock_instrument();
    let account_id = AccountId::from("IB-001");
    let mut parent = create_tracked_order_context(id, instrument_id);
    parent.perm_id = 101;
    let mut original = create_test_open_order(11, "Cancelled", id.as_str());
    original.order.perm_id = 101;
    let mut sibling = create_test_open_order(22, "Submitted", id.as_str());
    sibling.order.perm_id = 202;
    let mut state = orders.lock().unwrap();
    state.record_incarnation(11, &parent, account_id, 101, Some(&original), true);
    state.record_incarnation(11, &parent, account_id, 202, Some(&sibling), false);
    assert!(state.active_orders.is_empty());
    assert_eq!(
        state
            .group_cancel_candidates(instrument_id, account_id, None)
            .collect::<Vec<_>>(),
        vec![id]
    );
    assert_eq!(
        state
            .group_cancel_candidates(InstrumentId::from("MSFT.SMART"), account_id, None)
            .count(),
        0
    );
    assert_eq!(
        state
            .group_cancel_candidates(instrument_id, AccountId::from("IB-002"), None)
            .count(),
        0
    );
    state.observe_incarnation_status(&IBOrderStatus {
        order_id: 22,
        perm_id: 202,
        client_id: NATIVE_CLIENT_ID,
        status: OrderStatusKind::Cancelled,
        ..Default::default()
    });
    assert_eq!(
        state
            .group_cancel_candidates(instrument_id, account_id, None)
            .count(),
        0
    );
}

#[rstest]
fn completed_duplicate_group_removes_only_its_child_routes() {
    let orders = OrderTracker::new(NATIVE_CLIENT_ID);
    let parent_id = ClientOrderId::from("O-PRIMARY");
    let account_id = AccountId::from("IB-001");
    let child_id =
        ClientOrderId::for_duplicate_order(account_id, VenueOrderId::from("PERM-202")).unwrap();
    let mut parent = create_tracked_order_context(parent_id, create_test_stock_instrument());
    parent.perm_id = 101;
    insert_tracked_order(&orders, 11, parent.clone());
    let mut child = parent.clone();
    child.client_order_id = child_id;
    child.perm_id = 202;
    insert_tracked_order(&orders, 22, child);
    let mut original = create_test_open_order(11, "Cancelled", parent_id.as_str());
    original.order.perm_id = 101;
    let mut sibling = create_test_open_order(22, "Cancelled", parent_id.as_str());
    sibling.order.perm_id = 202;
    let mut state = orders.lock().unwrap();
    state.record_incarnation(11, &parent, account_id, 101, Some(&original), true);
    state.record_incarnation(11, &parent, account_id, 202, Some(&sibling), true);
    state.archive_finished_groups();
    assert_eq!(state.order_id_map.get(&parent_id), Some(&11));
    assert_eq!(state.venue_order_id_map.get(&11), Some(&parent_id));
    assert_eq!(state.order_id_map.get(&child_id), None);
    assert_eq!(state.venue_order_id_map.get(&22), None);
    assert!(!state.active_orders.contains_key(&22));
    assert_eq!(
        state.terminal_orders.get(&22).unwrap().client_order_id,
        child_id
    );
    assert!(state.groups.is_empty());
    assert!(state.group_history.contains_key(&parent_id));
}

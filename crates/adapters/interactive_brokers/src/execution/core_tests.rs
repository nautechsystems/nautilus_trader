// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

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
    subscriptions::Subscription,
};
use nautilus_common::{cache::Cache, live::runner::replace_exec_event_sender};
use nautilus_live::{ExecutionClientCore, execution::failure::CommandFailure};
use nautilus_model::{
    enums::{AccountType, AssetClass, LiquiditySide, OmsType, OrderSide, OrderType},
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
use crate::common::consts::{IB_CLIENT_ID, IB_VENUE};

fn create_test_instrument_provider() -> Arc<InteractiveBrokersInstrumentProvider> {
    let config = crate::config::InteractiveBrokersInstrumentProviderConfig::default();
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
        InteractiveBrokersExecClientConfig::default(),
        instrument_provider,
    )
    .unwrap();

    (client, rx, cache)
}

fn create_test_spread_instrument() -> InstrumentId {
    InstrumentId::new(
        Symbol::from("(1)SPY C400_((1))SPY C410"),
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
) -> TrackedOrderContext {
    TrackedOrderContext {
        client_order_id,
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("STRATEGY-001"),
        instrument_id,
        order_side: OrderSide::Buy,
        order_type: OrderType::Limit,
        accepted: false,
        avg_px: None,
    }
}

struct SubmitTrackingState {
    order_id_map: Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
    venue_order_id_map: Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
    instrument_id_map: Arc<Mutex<AHashMap<i32, InstrumentId>>>,
    trader_id_map: Arc<Mutex<AHashMap<i32, TraderId>>>,
    strategy_id_map: Arc<Mutex<AHashMap<i32, StrategyId>>>,
    active_order_contexts: Arc<Mutex<AHashMap<i32, TrackedOrderContext>>>,
    terminal_order_contexts: Arc<Mutex<FifoCacheMap<i32, TrackedOrderContext, 10_000>>>,
}

impl SubmitTrackingState {
    fn new() -> Self {
        Self {
            order_id_map: Arc::new(Mutex::new(AHashMap::new())),
            venue_order_id_map: Arc::new(Mutex::new(AHashMap::new())),
            instrument_id_map: Arc::new(Mutex::new(AHashMap::new())),
            trader_id_map: Arc::new(Mutex::new(AHashMap::new())),
            strategy_id_map: Arc::new(Mutex::new(AHashMap::new())),
            active_order_contexts: Arc::new(Mutex::new(AHashMap::new())),
            terminal_order_contexts: Arc::new(Mutex::new(FifoCacheMap::new())),
        }
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
            &self.order_id_map,
            &self.venue_order_id_map,
            &self.instrument_id_map,
            &self.trader_id_map,
            &self.strategy_id_map,
            &self.active_order_contexts,
            &self.terminal_order_contexts,
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
        assert_eq!(
            self.order_id_map.lock().unwrap().get(&client_order_id),
            Some(&order_id)
        );
        assert_eq!(
            self.venue_order_id_map.lock().unwrap().get(&order_id),
            Some(&client_order_id)
        );
        assert_eq!(
            self.instrument_id_map.lock().unwrap().get(&order_id),
            Some(&instrument_id)
        );
        assert_eq!(
            self.trader_id_map.lock().unwrap().get(&order_id),
            Some(&trader_id)
        );
        assert_eq!(
            self.strategy_id_map.lock().unwrap().get(&order_id),
            Some(&strategy_id)
        );

        let contexts = self.active_order_contexts.lock().unwrap();
        let context = contexts.get(&order_id).unwrap();
        assert_eq!(context.client_order_id, client_order_id);
        assert_eq!(context.instrument_id, instrument_id);
        assert_eq!(context.trader_id, trader_id);
        assert_eq!(context.strategy_id, strategy_id);
        assert_eq!(context.order_side, OrderSide::Buy);
        assert_eq!(context.order_type, OrderType::Limit);
        assert_eq!(context.accepted, accepted);
        assert_eq!(context.avg_px, None);
        assert!(
            self.terminal_order_contexts
                .lock()
                .unwrap()
                .get(&order_id)
                .is_none()
        );
    }

    fn emit_accepted(
        &self,
        order_id: i32,
        account_id: AccountId,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    ) -> bool {
        InteractiveBrokersExecutionClient::emit_order_accepted_if_needed(
            order_id,
            VenueOrderId::from(order_id.to_string()),
            account_id,
            UnixNanos::new(29),
            &self.active_order_contexts,
            exec_sender,
        )
        .unwrap()
    }

    fn assert_absent(&self, order_id: i32, client_order_id: ClientOrderId) {
        assert_eq!(
            self.order_id_map.lock().unwrap().get(&client_order_id),
            None
        );
        assert_eq!(self.venue_order_id_map.lock().unwrap().get(&order_id), None);
        assert_eq!(self.instrument_id_map.lock().unwrap().get(&order_id), None);
        assert_eq!(self.trader_id_map.lock().unwrap().get(&order_id), None);
        assert_eq!(self.strategy_id_map.lock().unwrap().get(&order_id), None);
        assert!(
            self.active_order_contexts
                .lock()
                .unwrap()
                .get(&order_id)
                .is_none()
        );
        assert!(
            self.terminal_order_contexts
                .lock()
                .unwrap()
                .get(&order_id)
                .is_none()
        );
    }
}

async fn process_submitted_status(
    order_id: i32,
    state: &SubmitTrackingState,
    exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    InteractiveBrokersExecutionClient::handle_order_status(
        &create_test_order_status(order_id, "Submitted"),
        &state.order_id_map,
        &state.venue_order_id_map,
        &create_test_instrument_provider(),
        exec_sender,
        UnixNanos::new(27),
        AccountId::from("IB-001"),
        &state.instrument_id_map,
        &state.trader_id_map,
        &state.strategy_id_map,
        &state.active_order_contexts,
        &state.terminal_order_contexts,
        &Arc::new(Mutex::new(AHashMap::new())),
        &Arc::new(Mutex::new(AHashMap::new())),
        &Arc::new(Mutex::new(AHashMap::new())),
        &Arc::new(Mutex::new(AHashMap::new())),
        &Arc::new(Mutex::new(ahash::AHashSet::new())),
        &Arc::new(Mutex::new(AHashMap::new())),
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
#[case(450_000_123, 402, 450_000_123)]
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
        code: 201,
        message: "Order rejected".to_string(),
        error_time: None,
        advanced_order_reject_json: String::new(),
    });
    let cancellation = ibapi::Error::Notice(ibapi::Notice {
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
        &state.order_id_map,
        &state.venue_order_id_map,
        &state.instrument_id_map,
        &state.trader_id_map,
        &state.strategy_id_map,
        &state.active_order_contexts,
        &state.terminal_order_contexts,
        &exec_sender,
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
                event.reason.as_str(),
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
        &state.order_id_map,
        &state.venue_order_id_map,
        &state.instrument_id_map,
        &state.trader_id_map,
        &state.strategy_id_map,
        &state.active_order_contexts,
        &state.terminal_order_contexts,
        &exec_sender,
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

    process_submitted_status(order_id, &state, &exec_sender).await;

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
    assert!(state.emit_accepted(prior_id, AccountId::from("IB-LIST-001"), &exec_sender));
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
        &state.order_id_map,
        &state.venue_order_id_map,
        &state.instrument_id_map,
        &state.trader_id_map,
        &state.strategy_id_map,
        &state.active_order_contexts,
        &state.terminal_order_contexts,
        &exec_sender,
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
    assert!(state.emit_accepted(prior_id, AccountId::from("IB-001"), &exec_sender));
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
        &state.order_id_map,
        &state.venue_order_id_map,
        &state.instrument_id_map,
        &state.trader_id_map,
        &state.strategy_id_map,
        &state.active_order_contexts,
        &state.terminal_order_contexts,
        &exec_sender,
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

    process_submitted_status(current_id, &state, &exec_sender).await;

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
fn submit_order_denies_when_client_not_ready() {
    let (client, mut rx, _) = create_test_execution_client();
    let order = create_test_limit_order(ClientOrderId::from("O-IB-001"));
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
                "Interactive Brokers client is not ready; refusing to submit order"
            );
        }
        event => panic!("Expected OrderDenied, was {event:?}"),
    }
}

#[rstest]
fn submit_order_list_denies_all_orders_when_client_not_ready() {
    let (client, mut rx, _) = create_test_execution_client();
    let order1 = create_test_limit_order(ClientOrderId::from("O-IB-001"));
    let order2 = create_test_limit_order(ClientOrderId::from("O-IB-002"));
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
                    "Interactive Brokers client is not ready; refusing to submit order list"
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
    let (client, mut rx, _) = create_test_execution_client();
    let order = create_test_limit_order(ClientOrderId::from("O-IB-001"));
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
        OrderSide::NoOrderSide,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_all_orders(cmd).unwrap();

    // A whole-request local failure must not become one rejection per order
    assert!(rx.try_recv().is_err(), "expected no events");
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
        client_id: 0,
        liquidation: 0,
        account_number: String::new(),
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
        client_id: 0,
        liquidation: 0,
        account_number: String::new(),
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
        contract_id: 0,
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
        client_id: 0,
        liquidation: 0,
        account_number: String::new(),
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

fn create_pending_combo_fill(
    client_order_id: ClientOrderId,
    quantity: Quantity,
) -> PendingComboFill {
    PendingComboFill {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("STRATEGY-001"),
        account_id: AccountId::from("IB-001"),
        instrument_id: create_test_spread_instrument(),
        venue_order_id: VenueOrderId::from("7001"),
        trade_id: TradeId::from("T-001"),
        order_side: OrderSide::Buy,
        order_type: OrderType::Limit,
        last_qty: quantity,
        commission: Money::new(1.0, Currency::USD()),
        liquidity_side: LiquiditySide::NoLiquiditySide,
        quote_currency: Currency::USD(),
        client_order_id,
        ts_event: UnixNanos::new(1),
        ts_init: UnixNanos::new(1),
    }
}

fn create_test_option_spread() -> OptionSpread {
    OptionSpread::new(
        create_test_spread_instrument(),
        Symbol::from("(1)SPY C400_((1))SPY C410"),
        AssetClass::Equity,
        Some(Ustr::from("SMART")),
        Ustr::from("SPY"),
        Ustr::from("VERTICAL"),
        UnixNanos::new(0),
        UnixNanos::new(0),
        Currency::USD(),
        2,
        Price::from("0.01"),
        Quantity::from(100),
        Quantity::from(1),
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
        UnixNanos::new(0),
        UnixNanos::new(0),
    )
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
        client_id: 0,
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
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let mut open_order = create_test_open_order(order_id, status, client_order_id.as_str());
    open_order.order.what_if = what_if;
    open_order.order.deactivate = true;
    open_order.order.total_quantity = 1.0;

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    order_id_map
        .lock()
        .unwrap()
        .insert(client_order_id, order_id);
    venue_order_id_map
        .lock()
        .unwrap()
        .insert(order_id, client_order_id);
    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, instrument_id);
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));
    active_order_contexts.lock().unwrap().insert(
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let result = InteractiveBrokersExecutionClient::handle_order_update(
        &OrderUpdate::OpenOrder(open_order),
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
        &pending_execution_cache,
    )
    .await;

    result.unwrap();
    assert!(exec_receiver.try_recv().is_err());
    assert_eq!(
        order_id_map.lock().unwrap().get(&client_order_id),
        Some(&order_id)
    );
    assert_eq!(
        venue_order_id_map.lock().unwrap().get(&order_id),
        Some(&client_order_id)
    );
    assert_eq!(
        instrument_id_map.lock().unwrap().get(&order_id),
        Some(&instrument_id)
    );
    assert!(
        !active_order_contexts
            .lock()
            .unwrap()
            .get(&order_id)
            .unwrap()
            .accepted
    );
}

#[rstest]
fn test_remove_order_tracking_clears_submit_identity() {
    let order_id = 7008;
    let client_order_id = ClientOrderId::from("O-SUBMIT-FAIL");
    let instrument_id = create_test_stock_instrument();
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));

    InteractiveBrokersExecutionClient::cache_order_tracking(
        order_id,
        client_order_id,
        instrument_id,
        TraderId::from("TRADER-001"),
        StrategyId::from("STRATEGY-001"),
        OrderSide::Buy,
        OrderType::Limit,
        &order_id_map,
        &venue_order_id_map,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
    )
    .unwrap();

    InteractiveBrokersExecutionClient::remove_order_tracking(
        order_id,
        client_order_id,
        &order_id_map,
        &venue_order_id_map,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
    )
    .unwrap();

    assert!(order_id_map.lock().unwrap().is_empty());
    assert!(venue_order_id_map.lock().unwrap().is_empty());
    assert!(instrument_id_map.lock().unwrap().is_empty());
    assert!(trader_id_map.lock().unwrap().is_empty());
    assert!(strategy_id_map.lock().unwrap().is_empty());
    assert!(active_order_contexts.lock().unwrap().is_empty());
    assert!(terminal_order_contexts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_get_leg_position_standard_spread() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)SPY C400_((1))SPY C410"),
        Venue::from("SMART"),
    );
    let leg_id = InstrumentId::new(Symbol::from("SPY C400"), Venue::from("SMART"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 0); // First leg is at position 0
}

#[tokio::test]
async fn test_get_leg_position_second_leg() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)SPY C400_((1))SPY C410"),
        Venue::from("SMART"),
    );
    let leg_id = InstrumentId::new(Symbol::from("SPY C410"), Venue::from("SMART"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 1); // Second leg is at position 1
}

#[tokio::test]
async fn test_get_leg_position_ratio_spread() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)E4DN5 P6350_((2))E4DN5 P6355"),
        Venue::from("XCME"),
    );
    let leg_id = InstrumentId::new(Symbol::from("E4DN5 P6350"), Venue::from("XCME"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    assert_eq!(result, 0);
}

#[tokio::test]
async fn test_get_leg_position_not_found() {
    let spread_id = InstrumentId::new(
        Symbol::from("(1)SPY C400_((1))SPY C410"),
        Venue::from("SMART"),
    );
    let leg_id = InstrumentId::new(Symbol::from("SPY C420"), Venue::from("SMART"));

    let result = InteractiveBrokersExecutionClient::get_leg_position(&spread_id, &leg_id);
    // Should fallback to position 0
    assert_eq!(result, 0);
}

#[tokio::test]
async fn test_get_leg_instrument_id_and_ratio() {
    let instrument_provider = create_test_instrument_provider();
    let leg_id = create_test_leg_instrument();

    // Create a contract with combo legs
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
        combo_legs: vec![ibapi::contracts::ComboLeg {
            contract_id: 12345,
            ratio: 1,
            action: LegAction::Buy,
            exchange: String::from("SMART"),
            open_close: ibapi::contracts::ComboLegOpenClose::Same,
            short_sale_slot: 0,
            designated_location: String::new(),
            exempt_code: 0,
        }],
        ..Default::default()
    };

    let result = InteractiveBrokersExecutionClient::get_leg_instrument_id_and_ratio(
        &contract,
        &leg_id,
        &instrument_provider,
    );
    let (returned_leg_id, ratio) = result;
    // Since we can't easily mock the contract ID mapping, it should fallback to the provided leg_id
    assert_eq!(returned_leg_id, leg_id);
    // Fallback ratio is 1
    assert_eq!(ratio, 1);
}

#[tokio::test]
async fn test_get_leg_instrument_id_and_ratio_with_sell_action() {
    let instrument_provider = create_test_instrument_provider();
    let leg_id = create_test_leg_instrument();

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
        combo_legs: vec![ibapi::contracts::ComboLeg {
            contract_id: 12345,
            ratio: 2,
            action: LegAction::Sell,
            exchange: String::from("SMART"),
            open_close: ibapi::contracts::ComboLegOpenClose::Same,
            short_sale_slot: 0,
            designated_location: String::new(),
            exempt_code: 0,
        }],
        ..Default::default()
    };

    let result = InteractiveBrokersExecutionClient::get_leg_instrument_id_and_ratio(
        &contract,
        &leg_id,
        &instrument_provider,
    );
    let (_, ratio) = result;
    // Should fallback to ratio 1
    assert_eq!(ratio, 1);
}

#[rstest]
fn test_cached_spread_instrument_ids_for_preload_deduplicates_spread_orders() {
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

    let spread_ids = InteractiveBrokersExecutionClient::cached_spread_instrument_ids_for_preload(
        &cache,
        &instrument_provider,
    );

    assert_eq!(spread_ids, vec![spread_instrument_id]);
}

#[rstest]
fn test_cached_spread_instrument_ids_for_preload_ignores_non_spread_orders() {
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

    let spread_ids = InteractiveBrokersExecutionClient::cached_spread_instrument_ids_for_preload(
        &cache,
        &instrument_provider,
    );

    assert!(spread_ids.is_empty());
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
async fn test_handle_spread_execution_first_fill() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let spread = create_test_option_spread();
    let spread_instrument_id = spread.id;
    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), 12345, 1);
    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));

    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let context = create_tracked_order_context(client_order_id, spread_instrument_id);
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(0);

    pending_combo_fill_avgs.lock().unwrap().insert(
        client_order_id,
        std::collections::VecDeque::from([(Decimal::from(3), Price::from("2.25"))]),
    );
    order_fill_progress.lock().unwrap().insert(
        client_order_id,
        (Decimal::from(3), Decimal::from_str("6.75").unwrap()),
    );

    InteractiveBrokersExecutionClient::handle_spread_execution(
        &exec_data,
        client_order_id,
        spread_instrument_id,
        &instrument_id,
        1.0,
        "USD",
        &instrument_provider,
        &exec_sender,
        ts_init,
        account_id,
        &spread_fill_tracking,
        &context,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        None, // avg_px
    )
    .await
    .unwrap();

    let combo_event = exec_receiver.try_recv().unwrap();
    match combo_event {
        ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
            assert_eq!(fill.instrument_id, spread_instrument_id);
            assert_eq!(fill.last_qty, Quantity::from(3));
            assert_eq!(fill.last_px, Price::from("2.25"));
            assert_eq!(fill.client_order_id, client_order_id);
        }
        other => panic!("unexpected combo event: {other:?}"),
    }

    let leg_event = exec_receiver.try_recv().unwrap();
    match leg_event {
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
            assert_eq!(fill.instrument_id, instrument_id);
            assert_eq!(fill.last_qty, Quantity::from(3));
            assert_eq!(fill.last_px, Price::from("5.25"));
        }
        other => panic!("unexpected leg event: {other:?}"),
    }
    assert!(pending_combo_fills.lock().unwrap().is_empty());
    assert!(
        spread_fill_tracking
            .lock()
            .unwrap()
            .contains_key(&client_order_id)
    );
}

#[tokio::test]
async fn test_handle_spread_execution_duplicate_detection() {
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, _exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));

    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let spread_instrument_id = create_test_spread_instrument();
    let context = create_tracked_order_context(client_order_id, spread_instrument_id);
    let leg_instrument_id = create_test_leg_instrument();
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(0);

    // Pre-populate tracking with the fill ID to simulate duplicate
    {
        let mut tracking = spread_fill_tracking.lock().unwrap();
        let fill_set = tracking
            .entry(client_order_id)
            .or_insert_with(ahash::AHashSet::new);
        fill_set.insert("exec-001".to_string());
    }

    let result = InteractiveBrokersExecutionClient::handle_spread_execution(
        &exec_data,
        client_order_id,
        spread_instrument_id,
        &leg_instrument_id,
        1.0,
        "USD",
        &instrument_provider,
        &exec_sender,
        ts_init,
        account_id,
        &spread_fill_tracking,
        &context,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        None, // avg_px
    )
    .await;

    // Should return Ok(()) immediately without processing duplicate
    assert!(result.is_ok());
}

#[rstest]
fn test_flush_pending_combo_fills_emits_tracked_order_fill() {
    let client_order_id = ClientOrderId::from("O-COMBO-001");
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    pending_combo_fills.lock().unwrap().insert(
        client_order_id,
        std::collections::VecDeque::from([create_pending_combo_fill(
            client_order_id,
            Quantity::from(2),
        )]),
    );
    pending_combo_fill_avgs.lock().unwrap().insert(
        client_order_id,
        std::collections::VecDeque::from([(Decimal::from(2), Price::from("2.75"))]),
    );
    order_fill_progress.lock().unwrap().insert(
        client_order_id,
        (Decimal::from(2), Decimal::from_str("5.50").unwrap()),
    );

    InteractiveBrokersExecutionClient::flush_pending_combo_fills(
        client_order_id,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &exec_sender,
    )
    .unwrap();

    let event = exec_receiver.try_recv().unwrap();
    match event {
        ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
            assert_eq!(fill.client_order_id, client_order_id);
            assert_eq!(fill.last_qty, Quantity::from(2));
            assert_eq!(fill.last_px, Price::from("2.75"));
            assert_eq!(fill.order_type, OrderType::Limit);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(pending_combo_fills.lock().unwrap().is_empty());
    assert!(pending_combo_fill_avgs.lock().unwrap().is_empty());
    assert!(order_fill_progress.lock().unwrap().is_empty());
}

#[rstest]
fn test_update_order_avg_price_allows_negative_spread_avg_fill_price() {
    let instrument_provider = create_test_instrument_provider();
    let spread = create_test_option_spread();
    let spread_instrument_id = spread.id;
    let client_order_id = ClientOrderId::from("O-COMBO-NEG-001");
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));

    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);

    InteractiveBrokersExecutionClient::update_order_avg_price(
        client_order_id,
        &spread_instrument_id,
        -2.25,
        3.0,
        &instrument_provider,
        &order_avg_prices,
        &pending_combo_fill_avgs,
        &order_fill_progress,
    )
    .unwrap();

    let avg_px = order_avg_prices
        .lock()
        .unwrap()
        .get(&client_order_id)
        .copied()
        .unwrap();
    assert_eq!(avg_px, Price::from("-2.25"));

    let avg_chunks = pending_combo_fill_avgs.lock().unwrap();
    let (fill_delta, partial_avg_px) = avg_chunks
        .get(&client_order_id)
        .unwrap()
        .front()
        .copied()
        .unwrap();
    assert_eq!(fill_delta, Decimal::from(3));
    assert_eq!(partial_avg_px, Price::from("-2.25"));

    let fill_progress = order_fill_progress.lock().unwrap();
    let (filled, total_notional) = fill_progress.get(&client_order_id).copied().unwrap();
    assert_eq!(filled, Decimal::from(3));
    assert_eq!(total_notional, Decimal::from_str("-6.75").unwrap());
}

#[rstest]
fn test_flush_pending_combo_fills_retains_partial_avg_chunk_remainder() {
    let client_order_id = ClientOrderId::from("O-COMBO-002");
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    pending_combo_fills.lock().unwrap().insert(
        client_order_id,
        std::collections::VecDeque::from([create_pending_combo_fill(
            client_order_id,
            Quantity::from(1),
        )]),
    );
    pending_combo_fill_avgs.lock().unwrap().insert(
        client_order_id,
        std::collections::VecDeque::from([(Decimal::from(3), Price::from("2.10"))]),
    );
    order_fill_progress.lock().unwrap().insert(
        client_order_id,
        (Decimal::from(3), Decimal::from_str("6.30").unwrap()),
    );

    InteractiveBrokersExecutionClient::flush_pending_combo_fills(
        client_order_id,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &exec_sender,
    )
    .unwrap();

    let event = exec_receiver.try_recv().unwrap();
    match event {
        ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
            assert_eq!(fill.client_order_id, client_order_id);
            assert_eq!(fill.last_qty, Quantity::from(1));
            assert_eq!(fill.last_px, Price::from("2.10"));
        }
        other => panic!("unexpected event: {other:?}"),
    }

    let avg_chunks = pending_combo_fill_avgs.lock().unwrap();
    let remainder = avg_chunks.get(&client_order_id).unwrap().front().unwrap();
    assert_eq!(remainder.0, Decimal::from(2));
    assert_eq!(remainder.1, Price::from("2.10"));
    assert!(pending_combo_fills.lock().unwrap().is_empty());
    assert!(order_fill_progress.lock().unwrap().is_empty());
}

#[rstest]
fn test_emit_order_pending_cancel_is_idempotent() {
    let order_id = 7001;
    let client_order_id = ClientOrderId::from("O-CANCEL-001");
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, create_test_spread_instrument());
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));

    InteractiveBrokersExecutionClient::emit_order_pending_cancel(
        order_id,
        client_order_id,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &pending_cancel_orders,
        &exec_sender,
        UnixNanos::new(1),
        AccountId::from("IB-001"),
    )
    .unwrap();
    InteractiveBrokersExecutionClient::emit_order_pending_cancel(
        order_id,
        client_order_id,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &pending_cancel_orders,
        &exec_sender,
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
        pending_cancel_orders
            .lock()
            .unwrap()
            .contains(&client_order_id)
    );
}

#[tokio::test]
async fn test_handle_order_status_canceled_emits_canceled_event() {
    let instrument_provider = create_test_instrument_provider();
    let spread = create_test_option_spread();
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let order_id = 7001;
    let client_order_id = ClientOrderId::from("O-CANCEL-002");
    let instrument_id = spread.id;

    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);

    venue_order_id_map
        .lock()
        .unwrap()
        .insert(order_id, client_order_id);
    order_id_map
        .lock()
        .unwrap()
        .insert(client_order_id, order_id);
    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, instrument_id);
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));
    active_order_contexts.lock().unwrap().insert(
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );
    pending_cancel_orders
        .lock()
        .unwrap()
        .insert(client_order_id);
    pending_combo_fills.lock().unwrap().insert(
        client_order_id,
        VecDeque::from([create_pending_combo_fill(
            client_order_id,
            Quantity::from(1),
        )]),
    );
    let mut status = create_test_order_status(order_id, "Cancelled");
    status.filled = 1.0;
    status.average_fill_price = Some(2.25);

    InteractiveBrokersExecutionClient::handle_order_status(
        &status,
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        UnixNanos::new(1),
        AccountId::from("IB-001"),
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
        &spread_fill_tracking,
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
                && event.last_px == Price::from("2.25")
    ));

    match exec_receiver.try_recv().unwrap() {
        ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.instrument_id, instrument_id);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(
        !pending_cancel_orders
            .lock()
            .unwrap()
            .contains(&client_order_id)
    );
    assert!(order_id_map.lock().unwrap().is_empty());
    assert!(venue_order_id_map.lock().unwrap().is_empty());
    assert!(instrument_id_map.lock().unwrap().is_empty());
    assert!(trader_id_map.lock().unwrap().is_empty());
    assert!(strategy_id_map.lock().unwrap().is_empty());
    assert!(active_order_contexts.lock().unwrap().is_empty());
    assert!(
        terminal_order_contexts
            .lock()
            .unwrap()
            .contains_key(&order_id)
    );
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
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();

    InteractiveBrokersExecutionClient::handle_order_status(
        &create_test_order_status(order_id, "Cancelled"),
        &state.order_id_map,
        &state.venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        UnixNanos::new(1),
        AccountId::from("IB-001"),
        &state.instrument_id_map,
        &state.trader_id_map,
        &state.strategy_id_map,
        &state.active_order_contexts,
        &state.terminal_order_contexts,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
        &spread_fill_tracking,
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
    assert_eq!(
        state
            .terminal_order_contexts
            .lock()
            .unwrap()
            .get(&order_id)
            .unwrap()
            .instrument_id,
        instrument_id
    );
}

#[tokio::test]
async fn test_process_order_update_stream_emits_accepted_then_canceled() {
    let instrument_provider = create_test_instrument_provider();
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = Subscription::new(update_receiver);
    let order_id = 7002;
    let client_order_id = ClientOrderId::from("O-STREAM-001");
    let instrument_id = create_test_spread_instrument();

    venue_order_id_map
        .lock()
        .unwrap()
        .insert(order_id, client_order_id);
    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, instrument_id);
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));
    active_order_contexts.lock().unwrap().insert(
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
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &pending_execution_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
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
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = Subscription::new(update_receiver);

    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    venue_order_id_map
        .lock()
        .unwrap()
        .insert(order_id, client_order_id);
    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, instrument_id);
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));
    active_order_contexts.lock().unwrap().insert(
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
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &pending_execution_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
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
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = Subscription::new(update_receiver);

    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    venue_order_id_map
        .lock()
        .unwrap()
        .insert(order_id, client_order_id);
    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, instrument_id);
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));
    let mut context = create_tracked_order_context(client_order_id, instrument_id);
    context.accepted = already_accepted;
    active_order_contexts
        .lock()
        .unwrap()
        .insert(order_id, context);

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
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &pending_execution_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
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
    assert!(commission_cache.lock().unwrap().is_empty());
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
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = Subscription::new(update_receiver);

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    order_id_map
        .lock()
        .unwrap()
        .insert(client_order_id, order_id);
    venue_order_id_map
        .lock()
        .unwrap()
        .insert(order_id, client_order_id);
    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, instrument_id);
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));
    active_order_contexts.lock().unwrap().insert(
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
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &pending_execution_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
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
            .unwrap()
            .contains_key(&String::from("exec-stream-late"))
    );
    assert!(exec_receiver.try_recv().is_err());

    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = Subscription::new(update_receiver);
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
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &pending_execution_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
    )
    .await;

    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Order(OrderEventAny::Filled(event))
            if event.client_order_id == client_order_id
                && event.trade_id == TradeId::from("exec-stream-late")
    ));
    assert!(exec_receiver.try_recv().is_err());
    assert!(pending_execution_cache.lock().unwrap().is_empty());
    assert!(active_order_contexts.lock().unwrap().is_empty());
    let terminal_contexts = terminal_order_contexts.lock().unwrap();
    let terminal_context = terminal_contexts.get(&order_id).unwrap();
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
    let leg_instrument_id = equity.id();
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = Subscription::new(update_receiver);

    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);
    instrument_provider.insert_test_instrument(InstrumentAny::from(spread), 54321, 1);
    order_id_map
        .lock()
        .unwrap()
        .insert(client_order_id, order_id);
    venue_order_id_map
        .lock()
        .unwrap()
        .insert(order_id, client_order_id);
    instrument_id_map
        .lock()
        .unwrap()
        .insert(order_id, instrument_id);
    trader_id_map
        .lock()
        .unwrap()
        .insert(order_id, TraderId::from("TRADER-001"));
    strategy_id_map
        .lock()
        .unwrap()
        .insert(order_id, StrategyId::from("STRATEGY-001"));
    active_order_contexts.lock().unwrap().insert(
        order_id,
        create_tracked_order_context(client_order_id, instrument_id),
    );

    let mut status = create_test_order_status(order_id, "Filled");
    status.filled = 1.0;
    status.average_fill_price = Some(2.25);
    let mut exec_data =
        create_test_execution_data(order_id, "exec-stream-late-combo", 1.0, 5.25, "BOT");
    exec_data.contract.combo_legs = create_test_bag_execution_data(order_id, "unused")
        .contract
        .combo_legs;
    exec_data.execution.order_reference.clear();

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
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &pending_execution_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
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
                && event.last_px == Price::from("2.25")
    ));
    assert!(matches!(
        exec_receiver.try_recv().unwrap(),
        ExecutionEvent::Report(ExecutionReport::Fill(event))
            if event.instrument_id == leg_instrument_id
    ));
    assert!(exec_receiver.try_recv().is_err());
    assert!(active_order_contexts.lock().unwrap().is_empty());
    assert!(
        terminal_order_contexts
            .lock()
            .unwrap()
            .contains_key(&order_id)
    );
}

#[tokio::test]
async fn test_process_order_update_stream_learns_order_ref_from_execution() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let order_id = 7004;
    let contract_id = 12346;
    let client_order_id = ClientOrderId::from("O-STREAM-EXEC-REF");
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let active_order_contexts = Arc::new(Mutex::new(AHashMap::new()));
    let terminal_order_contexts = Arc::new(Mutex::new(FifoCacheMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(CommissionCache::new()));
    let pending_execution_cache = Arc::new(Mutex::new(PendingExecutionCache::new()));
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (update_sender, update_receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut subscription = Subscription::new(update_receiver);

    let instrument_id = equity.id();
    instrument_provider.insert_test_instrument(InstrumentAny::from(equity), contract_id, 1);

    let mut exec_data = create_test_execution_data(order_id, "exec-stream-002", 100.0, 50.0, "BOT");
    exec_data.contract.contract_id = contract_id;
    exec_data.contract.security_type = SecurityType::Stock;
    exec_data.contract.symbol = IBSymbol::from("AAPL");
    exec_data.contract.exchange = Exchange::from("SMART");
    exec_data.contract.currency = IBCurrency::from("USD");
    exec_data.execution.order_reference = client_order_id.to_string();

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
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        nautilus_core::time::get_atomic_clock_realtime(),
        AccountId::from("IB-001"),
        &commission_cache,
        &pending_execution_cache,
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &active_order_contexts,
        &terminal_order_contexts,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &pending_cancel_orders,
    )
    .await;

    let fill_event = exec_receiver.try_recv().unwrap();
    match fill_event {
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
            assert_eq!(fill.client_order_id, Some(client_order_id));
            assert_eq!(fill.instrument_id, instrument_id);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert_eq!(
        venue_order_id_map.lock().unwrap().get(&order_id),
        Some(&client_order_id)
    );
    assert_eq!(
        order_id_map.lock().unwrap().get(&client_order_id),
        Some(&order_id)
    );
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
        Symbol::from("(1)LEG1_((1))LEG2_((2))LEG3"),
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

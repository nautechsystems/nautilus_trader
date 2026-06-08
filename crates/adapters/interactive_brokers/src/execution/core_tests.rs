// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use std::{cell::RefCell, rc::Rc};

use ibapi::{
    contracts::{Contract, Currency as IBCurrency, Exchange, SecurityType, Symbol as IBSymbol},
    orders::{
        CommissionReport, Execution, ExecutionData, Liquidity, Order as IBOrder,
        OrderData as IBOrderData, OrderState, OrderStatus as IBOrderStatus, OrderUpdate,
    },
    subscriptions::Subscription,
};
use nautilus_common::{cache::Cache, live::runner::replace_exec_event_sender};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, AssetClass, LiquiditySide, OmsType, OrderSide, OrderType},
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
fn submit_order_rejects_when_client_not_ready() {
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
        OrderEventAny::Rejected(event) => {
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(
                event.reason.to_string(),
                "Interactive Brokers client is not ready; refusing to submit order"
            );
        }
        event => panic!("Expected OrderRejected, was {event:?}"),
    }
}

#[rstest]
fn submit_order_list_rejects_all_orders_when_client_not_ready() {
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
            OrderEventAny::Rejected(event) => {
                assert_eq!(event.client_order_id, expected_client_order_id);
                assert_eq!(
                    event.reason.to_string(),
                    "Interactive Brokers client is not ready; refusing to submit order list"
                );
            }
            event => panic!("Expected OrderRejected, was {event:?}"),
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
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(event.venue_order_id, Some(VenueOrderId::from("1001")));
            assert_eq!(
                event.reason.to_string(),
                "Interactive Brokers client is not ready; refusing to modify order"
            );
        }
        event => panic!("Expected OrderModifyRejected, was {event:?}"),
    }
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
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(event.venue_order_id, Some(VenueOrderId::from("1001")));
            assert_eq!(
                event.reason.to_string(),
                "Interactive Brokers client is not ready; refusing to cancel order"
            );
        }
        event => panic!("Expected OrderCancelRejected, was {event:?}"),
    }
}

#[rstest]
fn cancel_all_orders_rejects_open_orders_when_client_not_ready() {
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

    match next_order_event(&mut rx) {
        OrderEventAny::CancelRejected(event) => {
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(
                event.reason.to_string(),
                "Interactive Brokers client is not ready; refusing to cancel orders"
            );
        }
        event => panic!("Expected OrderCancelRejected, was {event:?}"),
    }
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
        right: String::from("C"),
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
        side: side.to_string(),
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
        side: String::from("BOT"),
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
                action: String::from("BUY"),
                exchange: String::from("SMART"),
                open_close: ibapi::contracts::ComboLegOpenClose::Same,
                short_sale_slot: 0,
                designated_location: String::new(),
                exempt_code: 0,
            },
            ibapi::contracts::ComboLeg {
                contract_id: 67890,
                ratio: 1,
                action: String::from("SELL"),
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
        side: String::from("BOT"),
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
    price: Price,
) -> PendingComboFill {
    PendingComboFill {
        account_id: AccountId::from("IB-001"),
        instrument_id: create_test_spread_instrument(),
        venue_order_id: VenueOrderId::from("7001"),
        trade_id: TradeId::from("T-001"),
        order_side: OrderSide::Buy,
        last_qty: quantity,
        last_px: price,
        commission: Money::new(1.0, Currency::USD()),
        liquidity_side: LiquiditySide::NoLiquiditySide,
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
        UnixNanos::new(0),
        UnixNanos::new(0),
    )
}

fn create_test_order_status(order_id: i32, status: &str) -> IBOrderStatus {
    IBOrderStatus {
        order_id,
        status: status.to_string(),
        filled: 0.0,
        remaining: 0.0,
        average_fill_price: 0.0,
        perm_id: 0,
        parent_id: 0,
        last_fill_price: 0.0,
        client_id: 0,
        why_held: String::new(),
        market_cap_price: 0.0,
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
            status: status.to_string(),
            ..Default::default()
        },
    }
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
        right: String::from("C"),
        multiplier: String::from("100"),
        exchange: Exchange::from("SMART"),
        currency: IBCurrency::from("USD"),
        local_symbol: String::from("SPY C400"),
        trading_class: String::new(),
        combo_legs: vec![ibapi::contracts::ComboLeg {
            contract_id: 12345,
            ratio: 1,
            action: String::from("BUY"),
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
        right: String::from("C"),
        multiplier: String::from("100"),
        exchange: Exchange::from("SMART"),
        currency: IBCurrency::from("USD"),
        local_symbol: String::from("SPY C400"),
        trading_class: String::new(),
        combo_legs: vec![ibapi::contracts::ComboLeg {
            contract_id: 12345,
            ratio: 2,
            action: String::from("SELL"),
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
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));

    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(0);

    // Add trader and strategy mappings
    {
        let mut trader_map = trader_id_map.lock().unwrap();
        trader_map.insert(213, TraderId::from("TRADER-001"));
        let mut strategy_map = strategy_id_map.lock().unwrap();
        strategy_map.insert(213, StrategyId::from("STRATEGY-001"));
    }
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
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        None, // avg_px
    )
    .await
    .unwrap();

    let combo_event = exec_receiver.try_recv().unwrap();
    match combo_event {
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
            assert_eq!(fill.instrument_id, spread_instrument_id);
            assert_eq!(fill.last_qty, Quantity::from(3));
            assert_eq!(fill.avg_px, Some(Decimal::from_str("2.25").unwrap()));
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
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));

    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let spread_instrument_id = create_test_spread_instrument();
    let leg_instrument_id = create_test_leg_instrument();
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(0);

    // Add trader and strategy mappings
    {
        let mut trader_map = trader_id_map.lock().unwrap();
        trader_map.insert(213, TraderId::from("TRADER-001"));
        let mut strategy_map = strategy_id_map.lock().unwrap();
        strategy_map.insert(213, StrategyId::from("STRATEGY-001"));
    }

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
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        None, // avg_px
    )
    .await;

    // Should return Ok(()) immediately without processing duplicate
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_handle_spread_execution_missing_trader_id() {
    let instrument_provider = create_test_instrument_provider();
    let (exec_sender, _exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));

    let exec_data = create_test_execution_data(213, "exec-001", 3.0, 5.25, "BOT");
    let client_order_id = ClientOrderId::from("O-001");
    let spread_instrument_id = create_test_spread_instrument();
    let leg_instrument_id = create_test_leg_instrument();
    let account_id = AccountId::from("IB-001");
    let ts_init = UnixNanos::new(0);

    // Don't add trader mapping - should fail

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
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        None, // avg_px
    )
    .await;

    // Should fail with missing trader ID
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Trader ID") || error_msg.contains("trader"));
}

#[rstest]
fn test_flush_pending_combo_fills_emits_report_with_exact_avg_px() {
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
            Price::from("3.30"),
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
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
            assert_eq!(fill.client_order_id, Some(client_order_id));
            assert_eq!(fill.last_qty, Quantity::from(2));
            assert_eq!(fill.avg_px, Some(Decimal::from_str("2.75").unwrap()));
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
            Price::from("3.30"),
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
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
            assert_eq!(fill.client_order_id, Some(client_order_id));
            assert_eq!(fill.last_qty, Quantity::from(1));
            assert_eq!(fill.avg_px, Some(Decimal::from_str("2.10").unwrap()));
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
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let accepted_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let pending_live_exec_data = Arc::new(Mutex::new(AHashMap::new()));
    let pending_terminal_orders = Arc::new(Mutex::new(AHashMap::new()));
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let order_id = 7001;
    let client_order_id = ClientOrderId::from("O-CANCEL-002");
    let instrument_id = create_test_spread_instrument();

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
    pending_cancel_orders
        .lock()
        .unwrap()
        .insert(client_order_id);

    InteractiveBrokersExecutionClient::handle_order_status(
        &create_test_order_status(order_id, "Cancelled"),
        &order_id_map,
        &venue_order_id_map,
        &instrument_provider,
        &exec_sender,
        UnixNanos::new(1),
        AccountId::from("IB-001"),
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &accepted_orders,
        &pending_cancel_orders,
        &spread_fill_tracking,
        &pending_live_exec_data,
        &pending_terminal_orders,
    )
    .await
    .unwrap();

    let event = exec_receiver.try_recv().unwrap();
    match event {
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
}

#[tokio::test]
async fn test_process_order_update_stream_emits_accepted_then_canceled() {
    let instrument_provider = create_test_instrument_provider();
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let accepted_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(AHashMap::new()));
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
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &accepted_orders,
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
async fn test_process_order_update_stream_emits_fill_after_commission_report() {
    let instrument_provider = create_test_instrument_provider();
    let equity = equity_aapl();
    let order_id = 7003;
    let contract_id = 12345;
    let client_order_id = ClientOrderId::from("O-STREAM-002");
    let venue_order_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let instrument_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let trader_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let strategy_id_map = Arc::new(Mutex::new(AHashMap::new()));
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let accepted_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(AHashMap::new()));
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

    let mut exec_data = create_test_execution_data(order_id, "exec-stream-001", 100.0, 50.0, "BOT");
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
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &accepted_orders,
        &pending_cancel_orders,
    )
    .await;

    let fill_event = exec_receiver.try_recv().unwrap();
    match fill_event {
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
            assert_eq!(fill.client_order_id, Some(client_order_id));
            assert_eq!(fill.instrument_id, instrument_id);
            assert_eq!(fill.last_qty, Quantity::from(100));
            assert_eq!(fill.last_px, Price::from("50"));
            assert_eq!(fill.commission, Money::new(1.25, Currency::USD()));
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(commission_cache.lock().unwrap().is_empty());
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
    let order_avg_prices = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fills = Arc::new(Mutex::new(AHashMap::new()));
    let pending_combo_fill_avgs = Arc::new(Mutex::new(AHashMap::new()));
    let order_fill_progress = Arc::new(Mutex::new(AHashMap::new()));
    let accepted_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let pending_cancel_orders = Arc::new(Mutex::new(ahash::AHashSet::new()));
    let spread_fill_tracking = Arc::new(Mutex::new(AHashMap::new()));
    let commission_cache = Arc::new(Mutex::new(AHashMap::new()));
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
        &instrument_id_map,
        &trader_id_map,
        &strategy_id_map,
        &spread_fill_tracking,
        &order_avg_prices,
        &pending_combo_fills,
        &pending_combo_fill_avgs,
        &order_fill_progress,
        &accepted_orders,
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

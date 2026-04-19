// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use ibapi::{
    contracts::{Contract, Currency as IBCurrency, Exchange, SecurityType, Symbol as IBSymbol},
    orders::{
        CommissionReport, Execution, ExecutionData, Liquidity, Order as IBOrder,
        OrderData as IBOrderData, OrderState, OrderStatus as IBOrderStatus, OrderUpdate,
    },
    subscriptions::Subscription,
};
use nautilus_common::cache::Cache;
use nautilus_model::{
    enums::{AssetClass, LiquiditySide, OrderSide, OrderType},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{InstrumentAny, OptionSpread, stubs::equity_aapl},
    orders::builder::OrderTestBuilder,
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use ustr::Ustr;

use super::*;

fn create_test_instrument_provider() -> Arc<InteractiveBrokersInstrumentProvider> {
    let config = crate::config::InteractiveBrokersInstrumentProviderConfig::default();
    Arc::new(InteractiveBrokersInstrumentProvider::new(config))
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
    let (exec_sender, mut exec_receiver) = tokio::sync::mpsc::unbounded_channel();
    let order_id = 7001;
    let client_order_id = ClientOrderId::from("O-CANCEL-002");
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
    pending_cancel_orders
        .lock()
        .unwrap()
        .insert(client_order_id);

    InteractiveBrokersExecutionClient::handle_order_status(
        &create_test_order_status(order_id, "Cancelled"),
        &Arc::new(Mutex::new(AHashMap::new())),
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
        .send(Ok(OrderUpdate::CommissionReport(CommissionReport {
            execution_id: String::from("exec-stream-001"),
            commission: 1.25,
            currency: String::from("USD"),
            realized_pnl: None,
            yields: None,
            yield_redemption_date: String::new(),
        })))
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

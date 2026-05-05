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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, TestClock},
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{
        IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick,
        stubs::{OrderBookDeltaTestBuilder, stub_bar},
    },
    enums::{
        AggressorSide, BookType, ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::{OrderAccepted, OrderEventAny},
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orderbook::OrderBook,
    orders::{LimitOrder, Order, OrderAny},
    stubs::TestDefault,
    types::{Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::*;
use rust_decimal::Decimal;

use super::*;

/// Register an ExecTester with all required components.
/// This gives the tester access to OrderFactory for actual order creation.
fn register_exec_tester(tester: &mut ExecTester, cache: Rc<RefCell<Cache>>) {
    let trader_id = TraderId::from("TRADER-001");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        cache.clone(),
        clock.clone(),
        None,
    )));

    tester
        .core
        .register(trader_id, clock, cache, portfolio)
        .unwrap();
}

/// Create a cache with the test instrument pre-loaded.
fn create_cache_with_instrument(instrument: &InstrumentAny) -> Rc<RefCell<Cache>> {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let _ = cache.borrow_mut().add_instrument(instrument.clone());
    cache
}

#[fixture]
fn config() -> ExecTesterConfig {
    ExecTesterConfig::new(
        StrategyId::from("EXEC_TESTER-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        ClientId::new("BINANCE"),
        Quantity::from("0.001"),
    )
}

#[fixture]
fn instrument() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt())
}

fn create_initialized_limit_order() -> OrderAny {
    OrderAny::Limit(LimitOrder::test_default())
}

#[rstest]
fn test_config_creation(config: ExecTesterConfig) {
    assert_eq!(
        config.base.strategy_id,
        Some(StrategyId::from("EXEC_TESTER-001"))
    );
    assert_eq!(
        config.instrument_id,
        InstrumentId::from("ETHUSDT-PERP.BINANCE")
    );
    assert_eq!(config.client_id, Some(ClientId::new("BINANCE")));
    assert_eq!(config.order_qty, Quantity::from("0.001"));
    assert!(config.subscribe_quotes);
    assert!(config.subscribe_trades);
    assert!(!config.subscribe_book);
    assert!(config.enable_limit_buys);
    assert!(config.enable_limit_sells);
    assert!(!config.enable_stop_buys);
    assert!(!config.enable_stop_sells);
    assert_eq!(config.tob_offset_ticks, 500);
}

#[rstest]
fn test_config_default() {
    let config = ExecTesterConfig::default();

    assert!(config.base.strategy_id.is_none());
    assert!(config.subscribe_quotes);
    assert!(config.subscribe_trades);
    assert!(config.enable_limit_buys);
    assert!(config.enable_limit_sells);
    assert!(config.cancel_orders_on_stop);
    assert!(config.close_positions_on_stop);
    assert!(config.close_positions_time_in_force.is_none());
    assert!(!config.use_batch_cancel_on_stop);
}

#[rstest]
fn test_config_with_stop_orders(mut config: ExecTesterConfig) {
    config.enable_stop_buys = true;
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::StopLimit;
    config.stop_offset_ticks = 200;
    config.stop_limit_offset_ticks = Some(50);

    let tester = ExecTester::new(config);

    assert!(tester.config.enable_stop_buys);
    assert!(tester.config.enable_stop_sells);
    assert_eq!(tester.config.stop_order_type, OrderType::StopLimit);
    assert_eq!(tester.config.stop_offset_ticks, 200);
    assert_eq!(tester.config.stop_limit_offset_ticks, Some(50));
}

#[rstest]
fn test_config_with_batch_cancel() {
    let config = ExecTesterConfig::builder()
        .use_batch_cancel_on_stop(true)
        .build();
    assert!(config.use_batch_cancel_on_stop);
}

#[rstest]
fn test_config_with_order_maintenance(mut config: ExecTesterConfig) {
    config.modify_orders_to_maintain_tob_offset = true;
    config.cancel_replace_orders_to_maintain_tob_offset = false;

    let tester = ExecTester::new(config);

    assert!(tester.config.modify_orders_to_maintain_tob_offset);
    assert!(!tester.config.cancel_replace_orders_to_maintain_tob_offset);
}

#[rstest]
fn test_config_with_dry_run(mut config: ExecTesterConfig) {
    config.dry_run = true;

    let tester = ExecTester::new(config);

    assert!(tester.config.dry_run);
}

#[rstest]
fn test_config_with_position_opening(mut config: ExecTesterConfig) {
    config.open_position_on_start_qty = Some(Decimal::from(1));
    config.open_position_time_in_force = TimeInForce::Ioc;

    let tester = ExecTester::new(config);

    assert_eq!(
        tester.config.open_position_on_start_qty,
        Some(Decimal::from(1))
    );
    assert_eq!(tester.config.open_position_time_in_force, TimeInForce::Ioc);
}

#[rstest]
fn test_config_with_close_positions_time_in_force_builder() {
    let config = ExecTesterConfig::builder()
        .close_positions_time_in_force(TimeInForce::Ioc)
        .build();

    assert_eq!(config.close_positions_time_in_force, Some(TimeInForce::Ioc));
}

#[rstest]
fn test_config_with_all_stop_order_types(mut config: ExecTesterConfig) {
    // Test STOP_MARKET
    config.stop_order_type = OrderType::StopMarket;
    assert_eq!(config.stop_order_type, OrderType::StopMarket);

    // Test STOP_LIMIT
    config.stop_order_type = OrderType::StopLimit;
    assert_eq!(config.stop_order_type, OrderType::StopLimit);

    // Test MARKET_IF_TOUCHED
    config.stop_order_type = OrderType::MarketIfTouched;
    assert_eq!(config.stop_order_type, OrderType::MarketIfTouched);

    // Test LIMIT_IF_TOUCHED
    config.stop_order_type = OrderType::LimitIfTouched;
    assert_eq!(config.stop_order_type, OrderType::LimitIfTouched);
}

#[rstest]
fn test_exec_tester_creation(config: ExecTesterConfig) {
    let tester = ExecTester::new(config);

    assert!(tester.instrument.is_none());
    assert!(tester.price_offset.is_none());
    assert!(tester.buy_order.is_none());
    assert!(tester.sell_order.is_none());
    assert!(tester.buy_stop_order.is_none());
    assert!(tester.sell_stop_order.is_none());
}

#[rstest]
fn test_get_price_offset(config: ExecTesterConfig, instrument: InstrumentAny) {
    let tester = ExecTester::new(config);

    let offset_ticks = tester.get_price_offset(&instrument);

    assert_eq!(offset_ticks, 500);
}

#[rstest]
fn test_get_price_offset_different_ticks(instrument: InstrumentAny) {
    let config = ExecTesterConfig {
        tob_offset_ticks: 100,
        ..Default::default()
    };

    let tester = ExecTester::new(config);

    let offset_ticks = tester.get_price_offset(&instrument);

    assert_eq!(offset_ticks, 100);
}

#[rstest]
fn test_get_price_offset_single_tick(instrument: InstrumentAny) {
    let config = ExecTesterConfig {
        tob_offset_ticks: 1,
        ..Default::default()
    };

    let tester = ExecTester::new(config);

    let offset_ticks = tester.get_price_offset(&instrument);

    assert_eq!(offset_ticks, 1);
}

#[rstest]
fn test_is_order_active_initialized(config: ExecTesterConfig) {
    let tester = ExecTester::new(config);
    let order = create_initialized_limit_order();

    assert!(tester.is_order_active(&order));
    assert_eq!(order.status(), OrderStatus::Initialized);
}

#[rstest]
fn test_get_order_trigger_price_limit_order_returns_none(config: ExecTesterConfig) {
    let tester = ExecTester::new(config);
    let order = create_initialized_limit_order();

    assert!(tester.get_order_trigger_price(&order).is_none());
}

#[rstest]
fn test_on_quote_with_logging(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let quote = QuoteTick::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50000.0"),
        Price::from("50001.0"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_quote(&quote);
    assert!(result.is_ok());
}

#[rstest]
fn test_on_quote_without_logging(mut config: ExecTesterConfig) {
    config.log_data = false;
    let mut tester = ExecTester::new(config);

    let quote = QuoteTick::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50000.0"),
        Price::from("50001.0"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_quote(&quote);
    assert!(result.is_ok());
}

#[rstest]
fn test_on_trade_with_logging(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let trade = TradeTick::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50000.0"),
        Quantity::from("0.1"),
        AggressorSide::Buyer,
        TradeId::new("12345"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_trade(&trade);
    assert!(result.is_ok());
}

#[rstest]
fn test_on_trade_without_logging(mut config: ExecTesterConfig) {
    config.log_data = false;
    let mut tester = ExecTester::new(config);

    let trade = TradeTick::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50000.0"),
        Quantity::from("0.1"),
        AggressorSide::Buyer,
        TradeId::new("12345"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_trade(&trade);
    assert!(result.is_ok());
}

#[rstest]
fn test_on_book_without_bids_or_asks(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let book = OrderBook::new(InstrumentId::from("BTCUSDT-PERP.BINANCE"), BookType::L2_MBP);

    let result = tester.on_book(&book);
    assert!(result.is_ok());
}

#[rstest]
fn test_on_book_deltas_with_logging(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
    let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

    let result = tester.on_book_deltas(&deltas);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_book_deltas_without_logging(mut config: ExecTesterConfig) {
    config.log_data = false;
    let mut tester = ExecTester::new(config);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
    let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

    let result = tester.on_book_deltas(&deltas);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_bar_with_logging(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);
    let bar = stub_bar();

    let result = tester.on_bar(&bar);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_bar_without_logging(mut config: ExecTesterConfig) {
    config.log_data = false;
    let mut tester = ExecTester::new(config);
    let bar = stub_bar();

    let result = tester.on_bar(&bar);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_mark_price_with_logging(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let mark_price = MarkPriceUpdate::new(
        instrument_id,
        Price::from("50000.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_mark_price(&mark_price);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_mark_price_without_logging(mut config: ExecTesterConfig) {
    config.log_data = false;
    let mut tester = ExecTester::new(config);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let mark_price = MarkPriceUpdate::new(
        instrument_id,
        Price::from("50000.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_mark_price(&mark_price);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_index_price_with_logging(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let index_price = IndexPriceUpdate::new(
        instrument_id,
        Price::from("49999.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_index_price(&index_price);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_index_price_without_logging(mut config: ExecTesterConfig) {
    config.log_data = false;
    let mut tester = ExecTester::new(config);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let index_price = IndexPriceUpdate::new(
        instrument_id,
        Price::from("49999.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_index_price(&index_price);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_stop_dry_run(mut config: ExecTesterConfig) {
    config.dry_run = true;
    let mut tester = ExecTester::new(config);

    let result = tester.on_stop();

    assert!(result.is_ok());
}

#[rstest]
fn test_maintain_orders_dry_run_does_nothing(mut config: ExecTesterConfig) {
    config.dry_run = true;
    config.enable_limit_buys = true;
    config.enable_limit_sells = true;
    let mut tester = ExecTester::new(config);

    let best_bid = Price::from("50000.0");
    let best_ask = Price::from("50001.0");

    tester.maintain_orders(best_bid, best_ask);

    assert!(tester.buy_order.is_none());
    assert!(tester.sell_order.is_none());
}

#[rstest]
fn test_maintain_orders_no_instrument_does_nothing(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let best_bid = Price::from("50000.0");
    let best_ask = Price::from("50001.0");

    tester.maintain_orders(best_bid, best_ask);

    assert!(tester.buy_order.is_none());
    assert!(tester.sell_order.is_none());
}

#[rstest]
fn test_submit_limit_order_no_instrument_returns_error(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("50000.0"));

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No instrument"));
}

#[rstest]
fn test_submit_limit_order_dry_run_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.dry_run = true;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("50000.0"));

    assert!(result.is_ok());
    assert!(tester.buy_order.is_none());
}

#[rstest]
fn test_submit_limit_order_buys_disabled_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("50000.0"));

    assert!(result.is_ok());
    assert!(tester.buy_order.is_none());
}

#[rstest]
fn test_submit_limit_order_sells_disabled_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_sells = false;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Sell, Price::from("50000.0"));

    assert!(result.is_ok());
    assert!(tester.sell_order.is_none());
}

#[rstest]
fn test_submit_stop_order_no_instrument_returns_error(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("51000.0"), None);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No instrument"));
}

#[rstest]
fn test_submit_stop_order_dry_run_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.dry_run = true;
    config.enable_stop_buys = true;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("51000.0"), None);

    assert!(result.is_ok());
    assert!(tester.buy_stop_order.is_none());
}

#[rstest]
fn test_submit_stop_order_buys_disabled_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = false;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("51000.0"), None);

    assert!(result.is_ok());
    assert!(tester.buy_stop_order.is_none());
}

#[rstest]
fn test_submit_stop_limit_without_limit_price_returns_error(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::StopLimit;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    // Cannot actually submit without a registered OrderFactory
}

#[rstest]
fn test_open_position_no_instrument_returns_error(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let result = tester.open_position(Decimal::from(1));

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No instrument"));
}

#[rstest]
fn test_open_position_zero_quantity_returns_ok(
    config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.open_position(Decimal::ZERO);

    assert!(result.is_ok());
}

#[rstest]
fn test_config_with_enable_brackets() {
    let config = ExecTesterConfig::builder().enable_brackets(true).build();
    assert!(config.enable_brackets);
}

#[rstest]
fn test_config_with_bracket_offset_ticks() {
    let config = ExecTesterConfig::builder()
        .bracket_offset_ticks(1000)
        .build();
    assert_eq!(config.bracket_offset_ticks, 1000);
}

#[rstest]
fn test_config_with_test_reject_post_only() {
    let config = ExecTesterConfig::builder()
        .test_reject_post_only(true)
        .build();
    assert!(config.test_reject_post_only);
}

#[rstest]
fn test_config_with_test_reject_reduce_only() {
    let config = ExecTesterConfig::builder()
        .test_reject_reduce_only(true)
        .build();
    assert!(config.test_reject_reduce_only);
}

#[rstest]
fn test_config_with_emulation_trigger() {
    let config = ExecTesterConfig::builder()
        .emulation_trigger(TriggerType::LastPrice)
        .build();
    assert_eq!(config.emulation_trigger, Some(TriggerType::LastPrice));
}

#[rstest]
fn test_config_with_use_quote_quantity() {
    let config = ExecTesterConfig::builder().use_quote_quantity(true).build();
    assert!(config.use_quote_quantity);
}

#[rstest]
fn test_config_with_order_params() {
    use serde_json::Value;
    let mut params = Params::new();
    params.insert("key".to_string(), Value::String("value".to_string()));
    let config = ExecTesterConfig::builder()
        .order_params(params.clone())
        .build();
    assert_eq!(config.order_params, Some(params));
}

#[rstest]
fn test_submit_bracket_order_no_instrument_returns_error(config: ExecTesterConfig) {
    let mut tester = ExecTester::new(config);

    let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No instrument"));
}

#[rstest]
fn test_submit_bracket_order_dry_run_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.dry_run = true;
    config.enable_brackets = true;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

    assert!(result.is_ok());
    assert!(tester.buy_order.is_none());
}

#[rstest]
fn test_submit_bracket_order_unsupported_entry_type_returns_error(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.bracket_entry_order_type = OrderType::Market;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Only Limit entry orders are supported")
    );
}

#[rstest]
fn test_submit_bracket_order_buys_disabled_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.enable_limit_buys = false;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

    assert!(result.is_ok());
    assert!(tester.buy_order.is_none());
}

#[rstest]
fn test_submit_bracket_order_sells_disabled_returns_ok(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.enable_limit_sells = false;
    let mut tester = ExecTester::new(config);
    tester.instrument = Some(instrument);

    let result = tester.submit_bracket_order(OrderSide::Sell, Price::from("50000.0"));

    assert!(result.is_ok());
    assert!(tester.sell_order.is_none());
}

#[rstest]
fn test_submit_limit_order_creates_buy_order(config: ExecTesterConfig, instrument: InstrumentAny) {
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    assert!(tester.buy_order.is_some());
    let order = tester.buy_order.unwrap();
    assert_eq!(order.order_side(), OrderSide::Buy);
    assert_eq!(order.order_type(), OrderType::Limit);
}

#[rstest]
fn test_submit_limit_order_creates_sell_order(config: ExecTesterConfig, instrument: InstrumentAny) {
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Sell, Price::from("3000.0"));

    assert!(result.is_ok());
    assert!(tester.sell_order.is_some());
    let order = tester.sell_order.unwrap();
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert_eq!(order.order_type(), OrderType::Limit);
}

#[rstest]
fn test_submit_limit_order_with_post_only(mut config: ExecTesterConfig, instrument: InstrumentAny) {
    config.use_post_only = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    let order = tester.buy_order.unwrap();
    assert!(order.is_post_only());
}

#[rstest]
fn test_submit_limit_order_with_test_reject_post_only_implies_post_only(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.test_reject_post_only = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    let order = tester.buy_order.unwrap();
    assert!(order.is_post_only());
}

#[rstest]
fn test_submit_limit_order_with_expire_time(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.order_expire_time_delta_mins = Some(30);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    let order = tester.buy_order.unwrap();
    assert_eq!(order.time_in_force(), TimeInForce::Gtd);
    assert!(order.expire_time().is_some());
}

#[rstest]
fn test_submit_limit_order_with_order_params(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    use serde_json::Value;
    let mut params = Params::new();
    params.insert("tdMode".to_string(), Value::String("cross".to_string()));
    config.order_params = Some(params);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    assert!(tester.buy_order.is_some());
}

#[rstest]
fn test_submit_stop_market_order_creates_order(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::StopMarket;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3500.0"), None);

    assert!(result.is_ok());
    assert!(tester.buy_stop_order.is_some());
    let order = tester.buy_stop_order.unwrap();
    assert_eq!(order.order_type(), OrderType::StopMarket);
    assert_eq!(order.trigger_price(), Some(Price::from("3500.0")));
}

#[rstest]
fn test_submit_stop_limit_order_creates_order(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::StopLimit;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(
        OrderSide::Sell,
        Price::from("2500.0"),
        Some(Price::from("2490.0")),
    );

    assert!(result.is_ok());
    assert!(tester.sell_stop_order.is_some());
    let order = tester.sell_stop_order.unwrap();
    assert_eq!(order.order_type(), OrderType::StopLimit);
    assert_eq!(order.trigger_price(), Some(Price::from("2500.0")));
    assert_eq!(order.price(), Some(Price::from("2490.0")));
}

#[rstest]
fn test_submit_market_if_touched_order_creates_order(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::MarketIfTouched;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("2800.0"), None);

    assert!(result.is_ok());
    assert!(tester.buy_stop_order.is_some());
    let order = tester.buy_stop_order.unwrap();
    assert_eq!(order.order_type(), OrderType::MarketIfTouched);
}

#[rstest]
fn test_submit_limit_if_touched_order_creates_order(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::LimitIfTouched;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(
        OrderSide::Sell,
        Price::from("3200.0"),
        Some(Price::from("3190.0")),
    );

    assert!(result.is_ok());
    assert!(tester.sell_stop_order.is_some());
    let order = tester.sell_stop_order.unwrap();
    assert_eq!(order.order_type(), OrderType::LimitIfTouched);
}

#[rstest]
fn test_submit_trailing_stop_market_order_creates_order_with_activation_price(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::TrailingStopMarket;
    config.trailing_offset = Some(Decimal::from(25));
    config.trailing_offset_type = TrailingOffsetType::BasisPoints;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Sell, Price::from("3200.0"), None);

    assert!(result.is_ok());
    assert!(tester.sell_stop_order.is_some());
    let order = tester.sell_stop_order.unwrap();
    assert_eq!(order.order_type(), OrderType::TrailingStopMarket);
    assert_eq!(order.trigger_price(), Some(Price::from("3200.0")));
    assert_eq!(order.activation_price(), Some(Price::from("3200.0")));
}

#[rstest]
fn test_maintain_stop_buy_orders_trailing_stop_places_activation_below_market(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::TrailingStopMarket;
    config.trailing_offset = Some(Decimal::from(25));
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3000.5"));

    let order = tester.buy_stop_order.unwrap();
    assert_eq!(order.order_type(), OrderType::TrailingStopMarket);
    assert_eq!(order.trigger_price(), Some(Price::from("2999.0")));
    assert_eq!(order.activation_price(), Some(Price::from("2999.0")));
}

#[rstest]
fn test_maintain_stop_sell_orders_trailing_stop_places_activation_above_market(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::TrailingStopMarket;
    config.trailing_offset = Some(Decimal::from(25));
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3000.5"));

    let order = tester.sell_stop_order.unwrap();
    assert_eq!(order.order_type(), OrderType::TrailingStopMarket);
    assert_eq!(order.trigger_price(), Some(Price::from("3001.5")));
    assert_eq!(order.activation_price(), Some(Price::from("3001.5")));
}

#[rstest]
fn test_submit_stop_order_with_emulation_trigger(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::StopMarket;
    config.emulation_trigger = Some(TriggerType::LastPrice);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3500.0"), None);

    assert!(result.is_ok());
    let order = tester.buy_stop_order.unwrap();
    assert_eq!(order.emulation_trigger(), Some(TriggerType::LastPrice));
}

#[rstest]
fn test_submit_bracket_order_creates_order_list(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.bracket_offset_ticks = 100;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    assert!(tester.buy_order.is_some());
    let order = tester.buy_order.unwrap();
    assert_eq!(order.order_side(), OrderSide::Buy);
    assert_eq!(order.order_type(), OrderType::Limit);
    assert_eq!(order.contingency_type(), Some(ContingencyType::Oto));
}

#[rstest]
fn test_submit_bracket_order_sell_creates_order_list(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.bracket_offset_ticks = 100;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_bracket_order(OrderSide::Sell, Price::from("3000.0"));

    assert!(result.is_ok());
    assert!(tester.sell_order.is_some());
    let order = tester.sell_order.unwrap();
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert_eq!(order.contingency_type(), Some(ContingencyType::Oto));
}

#[rstest]
fn test_open_position_creates_market_order(config: ExecTesterConfig, instrument: InstrumentAny) {
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.open_position(Decimal::from(1));

    assert!(result.is_ok());
}

#[rstest]
fn test_open_position_with_reduce_only_rejection(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.test_reject_reduce_only = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    // Should succeed in creating order (rejection happens at exchange)
    let result = tester.open_position(Decimal::from(1));

    assert!(result.is_ok());
}

#[rstest]
fn test_submit_stop_limit_without_limit_price_fails(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::StopLimit;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3500.0"), None);

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("requires limit_price")
    );
}

#[rstest]
fn test_submit_limit_if_touched_without_limit_price_fails(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::LimitIfTouched;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Sell, Price::from("3200.0"), None);

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("requires limit_price")
    );
}

#[rstest]
fn test_config_new_fields_default_values(config: ExecTesterConfig) {
    assert!(config.limit_time_in_force.is_none());
    assert!(config.stop_time_in_force.is_none());
}

#[rstest]
fn test_config_with_limit_time_in_force_builder() {
    let config = ExecTesterConfig::builder()
        .limit_time_in_force(TimeInForce::Ioc)
        .build();
    assert_eq!(config.limit_time_in_force, Some(TimeInForce::Ioc));
}

#[rstest]
fn test_config_with_stop_time_in_force_builder() {
    let config = ExecTesterConfig::builder()
        .stop_time_in_force(TimeInForce::Day)
        .build();
    assert_eq!(config.stop_time_in_force, Some(TimeInForce::Day));
}

#[rstest]
fn test_submit_limit_order_with_limit_time_in_force(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.limit_time_in_force = Some(TimeInForce::Ioc);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    let order = tester.buy_order.unwrap();
    assert_eq!(order.time_in_force(), TimeInForce::Ioc);
    assert!(order.expire_time().is_none());
}

#[rstest]
fn test_submit_limit_order_limit_time_in_force_overrides_expire(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    // limit_time_in_force takes priority over order_expire_time_delta_mins
    config.limit_time_in_force = Some(TimeInForce::Day);
    config.order_expire_time_delta_mins = Some(30);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

    assert!(result.is_ok());
    let order = tester.buy_order.unwrap();
    assert_eq!(order.time_in_force(), TimeInForce::Day);
    assert!(order.expire_time().is_none());
}

#[rstest]
fn test_submit_stop_order_with_stop_time_in_force(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = true;
    config.stop_time_in_force = Some(TimeInForce::Day);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3200.0"), None);

    assert!(result.is_ok());
    let order = tester.buy_stop_order.unwrap();
    assert_eq!(order.time_in_force(), TimeInForce::Day);
    assert!(order.expire_time().is_none());
}

#[rstest]
fn test_submit_stop_order_stop_time_in_force_overrides_expire(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_stop_buys = true;
    config.stop_time_in_force = Some(TimeInForce::Ioc);
    config.order_expire_time_delta_mins = Some(30);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3200.0"), None);

    assert!(result.is_ok());
    let order = tester.buy_stop_order.unwrap();
    assert_eq!(order.time_in_force(), TimeInForce::Ioc);
    assert!(order.expire_time().is_none());
}

#[rstest]
fn test_config_limit_aggressive_default() {
    let config = ExecTesterConfig::default();
    assert!(!config.limit_aggressive);
}

#[rstest]
fn test_config_limit_aggressive_builder() {
    let config = ExecTesterConfig::builder().limit_aggressive(true).build();
    assert!(config.limit_aggressive);
}

#[rstest]
fn test_config_test_modify_rejected_default() {
    let config = ExecTesterConfig::default();
    assert!(!config.test_modify_rejected);
}

#[rstest]
fn test_config_test_modify_rejected_builder() {
    let config = ExecTesterConfig::builder()
        .test_modify_rejected(true)
        .build();
    assert!(config.test_modify_rejected);
}

#[rstest]
fn test_exec_tester_modify_rejected_attempted_starts_false(config: ExecTesterConfig) {
    let tester = ExecTester::new(config);
    assert!(!tester.modify_rejected_attempted);
}

// `limit_aggressive` flips BUY pricing to cross the spread (place at/above ask).
#[rstest]
fn test_maintain_buy_orders_limit_aggressive_crosses_ask(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.limit_aggressive = true;
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    let best_bid = Price::from("3000.0");
    let best_ask = Price::from("3001.0");
    tester.maintain_orders(best_bid, best_ask);

    let order = tester.buy_order.expect("buy order should be submitted");
    let price = order.price().expect("limit order has price");
    // Aggressive BUY: ask + 5 ticks at 0.01 increment.
    assert_eq!(price, Price::from("3001.05"));
    assert!(price >= best_ask, "expected {price} >= {best_ask}");
}

// `limit_aggressive` flips SELL pricing to cross the spread (place at/below bid).
#[rstest]
fn test_maintain_sell_orders_limit_aggressive_crosses_bid(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.limit_aggressive = true;
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    let best_bid = Price::from("3000.0");
    let best_ask = Price::from("3001.0");
    // Disable the buy side so only the sell order maintenance runs.
    tester.config.enable_limit_buys = false;
    tester.maintain_orders(best_bid, best_ask);

    let order = tester.sell_order.expect("sell order should be submitted");
    let price = order.price().expect("limit order has price");
    // Aggressive SELL: bid - 5 ticks at 0.01 increment.
    assert_eq!(price, Price::from("2999.95"));
    assert!(price <= best_bid, "expected {price} <= {best_bid}");
}

// Default (passive) BUY pricing places below the bid, never crossing.
#[rstest]
fn test_maintain_buy_orders_passive_below_bid(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    let best_bid = Price::from("3000.0");
    let best_ask = Price::from("3001.0");
    tester.maintain_orders(best_bid, best_ask);

    let order = tester.buy_order.expect("buy order should be submitted");
    let price = order.price().expect("limit order has price");
    // Passive BUY: bid - 5 ticks at 0.01 increment.
    assert_eq!(price, Price::from("2999.95"));
    assert!(price < best_bid, "expected {price} < {best_bid}");
}

// `limit_aggressive` combined with `limit_time_in_force=Ioc` produces an aggressive
// IOC limit order: the configuration that exercises TC-E13 (immediate-fill) and
// TC-E14 (when paired with a non-aggressive price, no-fill cancel).
#[rstest]
fn test_submit_limit_order_aggressive_ioc_carries_ioc_tif(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.limit_aggressive = true;
    config.limit_time_in_force = Some(TimeInForce::Ioc);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester
        .submit_limit_order(OrderSide::Buy, Price::from("3001.0"))
        .unwrap();

    let order = tester.buy_order.unwrap();
    assert_eq!(order.time_in_force(), TimeInForce::Ioc);
}

// FOK passthrough on the limit path (TC-E15 / TC-E16).
#[rstest]
fn test_submit_limit_order_fok_carries_fok_tif(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.limit_time_in_force = Some(TimeInForce::Fok);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester
        .submit_limit_order(OrderSide::Buy, Price::from("3000.0"))
        .unwrap();

    assert_eq!(tester.buy_order.unwrap().time_in_force(), TimeInForce::Fok,);
}

// DAY TIF passthrough: TC-E73 relies on the adapter denying DAY before the
// venue ever sees it. The tester's job is just to forward the configured TIF.
#[rstest]
fn test_submit_limit_order_day_tif_carries_day_tif(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.limit_time_in_force = Some(TimeInForce::Day);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester
        .submit_limit_order(OrderSide::Buy, Price::from("3000.0"))
        .unwrap();

    assert_eq!(tester.buy_order.unwrap().time_in_force(), TimeInForce::Day,);
}

// `test_modify_rejected` flips the one-shot guard the first time the maintain loop
// finds an accepted resting order. The order itself doesn't have to transition to
// ACCEPTED status in unit tests; we only verify the guard is consumed exactly once
// when there is an in-flight buy order.
#[rstest]
fn test_modify_rejected_one_shot_guard_consumed(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.test_modify_rejected = true;
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    let best_bid = Price::from("3000.0");
    let best_ask = Price::from("3001.0");

    // First call: submits the order; guard remains unset because the order is not
    // yet venue-acknowledged.
    tester.maintain_orders(best_bid, best_ask);
    assert!(tester.buy_order.is_some());
    assert!(!tester.modify_rejected_attempted);

    // Subsequent calls cannot flip the guard either, since the order has no
    // venue_order_id yet. This documents that the guard waits for venue acceptance.
    tester.maintain_orders(best_bid, best_ask);
    assert!(!tester.modify_rejected_attempted);
}

// Apply an `OrderAccepted` event to the cache copy so subsequent cache reads
// observe a venue-acknowledged order. Mirrors what the OrderManager does when
// a real `OrderAccepted` event flows through the engine.
fn ack_buy_order_in_cache(tester: &ExecTester, cache: &Rc<RefCell<Cache>>) {
    let order = tester
        .buy_order
        .as_ref()
        .cloned()
        .expect("buy order should be tracked locally");
    let cid = order.client_order_id();
    let strategy_id = order.strategy_id();
    let instrument_id = order.instrument_id();

    let accepted = OrderAccepted::new(
        TraderId::from("TRADER-001"),
        strategy_id,
        instrument_id,
        cid,
        VenueOrderId::from("V-1"),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );

    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Accepted(accepted))
        .unwrap();
}

// Once a real `OrderAccepted` event has been applied to the cache, the next
// `maintain_orders` call should refresh the locally tracked clone, observe a
// non-empty `venue_order_id`, and consume the one-shot modify-rejected guard.
#[rstest]
fn test_modify_rejected_fires_after_cache_acceptance(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.test_modify_rejected = true;
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    let best_bid = Price::from("3000.0");
    let best_ask = Price::from("3001.0");

    tester.maintain_orders(best_bid, best_ask);
    assert!(tester.buy_order.is_some());
    assert!(!tester.modify_rejected_attempted);

    // Simulate the venue acknowledging the order. This puts the canonical
    // accepted state in the cache; the tester's stored `buy_order` is still
    // the pre-submit clone.
    ack_buy_order_in_cache(&tester, &cache);

    // Next maintain tick refreshes from cache and trips the guard.
    tester.maintain_orders(best_bid, best_ask);
    assert!(
        tester.modify_rejected_attempted,
        "expected guard to flip once the cache shows venue acceptance",
    );

    // And only once.
    tester.maintain_orders(best_bid, best_ask);
    assert!(tester.modify_rejected_attempted);
}

// Batch-mode submission must also honor `limit_aggressive`, otherwise IOC/FOK
// fill scenarios silently submit passive prices in batch mode.
#[rstest]
fn test_maintain_batch_limit_pair_limit_aggressive_crosses_spread(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.batch_submit_limit_pair = true;
    config.limit_aggressive = true;
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    let best_bid = Price::from("3000.0");
    let best_ask = Price::from("3001.0");
    tester.maintain_orders(best_bid, best_ask);

    let buy = tester.buy_order.expect("batch buy should be submitted");
    let sell = tester.sell_order.expect("batch sell should be submitted");
    let buy_price = buy.price().expect("limit order has price");
    let sell_price = sell.price().expect("limit order has price");

    // Aggressive batch: BUY at ask + 5 ticks, SELL at bid - 5 ticks (0.01 increment).
    assert_eq!(buy_price, Price::from("3001.05"));
    assert_eq!(sell_price, Price::from("2999.95"));
    assert!(buy_price >= best_ask, "expected {buy_price} >= {best_ask}");
    assert!(
        sell_price <= best_bid,
        "expected {sell_price} <= {best_bid}"
    );
}

// BUY StopLimit through the maintain path: trigger above the ask, limit above
// the trigger so `trigger_price <= price` invariant holds.
#[rstest]
fn test_maintain_stop_buy_orders_stop_limit_prices(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::StopLimit;
    config.stop_offset_ticks = 100;
    config.stop_limit_offset_ticks = Some(50);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let order = tester.buy_stop_order.expect("buy stop order submitted");
    assert_eq!(order.order_type(), OrderType::StopLimit);
    // Trigger: ask + 100 ticks at 0.01 = 3002.0; limit: trigger + 50 ticks = 3002.5.
    assert_eq!(order.trigger_price(), Some(Price::from("3002.0")));
    assert_eq!(order.price(), Some(Price::from("3002.5")));
}

// BUY LimitIfTouched through the maintain path: trigger below the bid, limit
// above the trigger (upstream fix; previously LIT used `trigger - offset`).
#[rstest]
fn test_maintain_stop_buy_orders_lit_prices(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::LimitIfTouched;
    config.stop_offset_ticks = 100;
    config.stop_limit_offset_ticks = Some(50);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let order = tester.buy_stop_order.expect("buy stop order submitted");
    assert_eq!(order.order_type(), OrderType::LimitIfTouched);
    // Trigger: bid - 100 ticks at 0.01 = 2999.0; limit: trigger + 50 ticks = 2999.5.
    assert_eq!(order.trigger_price(), Some(Price::from("2999.0")));
    assert_eq!(order.price(), Some(Price::from("2999.5")));
}

// SELL StopLimit through the maintain path: trigger below the bid, limit below
// the trigger so `trigger_price >= price` invariant holds.
#[rstest]
fn test_maintain_stop_sell_orders_stop_limit_prices(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::StopLimit;
    config.stop_offset_ticks = 100;
    config.stop_limit_offset_ticks = Some(50);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let order = tester.sell_stop_order.expect("sell stop order submitted");
    assert_eq!(order.order_type(), OrderType::StopLimit);
    // Trigger: bid - 100 ticks at 0.01 = 2999.0; limit: trigger - 50 ticks = 2998.5.
    assert_eq!(order.trigger_price(), Some(Price::from("2999.0")));
    assert_eq!(order.price(), Some(Price::from("2998.5")));
}

// SELL LimitIfTouched through the maintain path: trigger above the ask, limit
// below the trigger (upstream fix; previously LIT used `trigger + offset`).
#[rstest]
fn test_maintain_stop_sell_orders_lit_prices(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::LimitIfTouched;
    config.stop_offset_ticks = 100;
    config.stop_limit_offset_ticks = Some(50);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let order = tester.sell_stop_order.expect("sell stop order submitted");
    assert_eq!(order.order_type(), OrderType::LimitIfTouched);
    // Trigger: ask + 100 ticks at 0.01 = 3002.0; limit: trigger - 50 ticks = 3001.5.
    assert_eq!(order.trigger_price(), Some(Price::from("3002.0")));
    assert_eq!(order.price(), Some(Price::from("3001.5")));
}

// BUY bracket: TP above entry, SL below entry, both at `bracket_offset_ticks`.
// The entry order is tracked locally; SL/TP land in the cache via `submit_order_list`.
#[rstest]
fn test_submit_bracket_order_buy_tp_sl_prices(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.bracket_offset_ticks = 100;
    let instrument_id = instrument.id();
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.instrument = Some(instrument);

    tester
        .submit_bracket_order(OrderSide::Buy, Price::from("3000.0"))
        .expect("bracket submission");

    let cache_ref = cache.borrow();
    let orders = cache_ref.orders(None, Some(&instrument_id), None, None, None);

    let sl = orders
        .iter()
        .find(|o| o.order_side() == OrderSide::Sell && o.order_type() == OrderType::StopMarket)
        .expect("SL stop-market present");
    let tp = orders
        .iter()
        .find(|o| o.order_side() == OrderSide::Sell && o.order_type() == OrderType::Limit)
        .expect("TP limit present");

    // Entry 3000.0 +/- 100 ticks at 0.01 increment: TP = 3001.0, SL trigger = 2999.0.
    assert_eq!(tp.price(), Some(Price::from("3001.0")));
    assert_eq!(sl.trigger_price(), Some(Price::from("2999.0")));
}

// SELL bracket: TP below entry, SL above entry. Mirror of the BUY case.
#[rstest]
fn test_submit_bracket_order_sell_tp_sl_prices(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.bracket_offset_ticks = 100;
    let instrument_id = instrument.id();
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.instrument = Some(instrument);

    tester
        .submit_bracket_order(OrderSide::Sell, Price::from("3000.0"))
        .expect("bracket submission");

    let cache_ref = cache.borrow();
    let orders = cache_ref.orders(None, Some(&instrument_id), None, None, None);

    let sl = orders
        .iter()
        .find(|o| o.order_side() == OrderSide::Buy && o.order_type() == OrderType::StopMarket)
        .expect("SL stop-market present");
    let tp = orders
        .iter()
        .find(|o| o.order_side() == OrderSide::Buy && o.order_type() == OrderType::Limit)
        .expect("TP limit present");

    // Entry 3000.0 +/- 100 ticks at 0.01 increment: TP = 2999.0, SL trigger = 3001.0.
    assert_eq!(tp.price(), Some(Price::from("2999.0")));
    assert_eq!(sl.trigger_price(), Some(Price::from("3001.0")));
}

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

use ahash::AHashSet;
use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, TestClock},
    messages::execution::{SubmitOrder, TradingCommand},
    msgbus::{
        self, MessagingSwitchboard,
        stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
    },
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
    events::{
        OrderEventAny, OrderRejected,
        order::spec::{OrderAcceptedSpec, OrderPendingCancelSpec, OrderRejectedSpec},
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orderbook::OrderBook,
    orders::{LimitOrder, Order, OrderAny},
    stubs::TestDefault,
    types::{Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use nautilus_trading::StrategyNative;
use rstest::*;
use rust_decimal::Decimal;

use super::*;

/// Register an `ExecTester` with all required components.
/// This gives the tester access to `OrderFactory` for actual order creation.
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

fn quote_for(instrument_id: InstrumentId) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from("3000.0"),
        Price::from("3000.5"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
    assert!(!tester.config.open_position_on_first_quote);
    assert_eq!(tester.pending_open_position_qty, Some(Decimal::from(1)));
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
    assert!(tester.pending_open_position_qty.is_none());
    assert!(!tester.modify_rejected_attempted);
    assert!(!tester.buy_cancel_replace_attempted);
    assert!(!tester.sell_cancel_replace_attempted);
    assert!(!tester.buy_stop_cancel_replace_attempted);
    assert!(!tester.sell_stop_cancel_replace_attempted);
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
fn test_is_order_active_initialized() {
    let order = create_initialized_limit_order();

    assert!(ExecTester::is_order_active(&order));
    assert_eq!(order.status(), OrderStatus::Initialized);
}

#[rstest]
fn test_get_order_trigger_price_limit_order_returns_none() {
    let order = create_initialized_limit_order();

    assert!(ExecTester::get_order_trigger_price(&order).is_none());
}

#[rstest]
fn test_open_position_on_start_waits_for_first_quote(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.instrument_id = instrument.id();
    config.open_position_on_start_qty = Some(Decimal::from(1));
    config.open_position_on_first_quote = true;
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.subscribe_quotes = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);

    tester.on_instrument(&instrument).unwrap();
    assert_eq!(tester.pending_open_position_qty, Some(Decimal::from(1)));

    let quote = quote_for(tester.config.instrument_id);
    tester.on_quote(&quote).unwrap();
    assert!(tester.pending_open_position_qty.is_none());
}

#[rstest]
fn test_open_position_on_start_submits_market_order_after_first_quote(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.instrument_id = instrument.id();
    config.open_position_on_start_qty = Some(Decimal::from(1));
    config.open_position_on_first_quote = true;
    config.open_position_time_in_force = TimeInForce::Ioc;
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.subscribe_quotes = true;
    let expected_qty = instrument.make_qty(1.0, None);
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    let risk_saver = capture_risk_commands();

    tester.on_instrument(&instrument).unwrap();
    assert!(submit_orders(&risk_saver).is_empty());

    let quote = quote_for(tester.config.instrument_id);
    tester.on_quote(&quote).unwrap();

    let submits = submit_orders(&risk_saver);
    assert_eq!(submits.len(), 1, "expected one SubmitOrder");
    let init = &submits[0].order_init;
    assert!(tester.pending_open_position_qty.is_none());
    assert_eq!(init.order_type, OrderType::Market);
    assert_eq!(init.order_side, OrderSide::Buy);
    assert_eq!(init.quantity, expected_qty);
    assert_eq!(init.time_in_force, TimeInForce::Ioc);
}

#[rstest]
fn test_open_position_on_start_ignores_quote_before_instrument(mut config: ExecTesterConfig) {
    config.open_position_on_start_qty = Some(Decimal::from(1));
    config.open_position_on_first_quote = true;
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.subscribe_quotes = true;
    let mut tester = ExecTester::new(config);
    let quote = quote_for(tester.config.instrument_id);

    tester.on_quote(&quote).unwrap();
    assert_eq!(tester.pending_open_position_qty, Some(Decimal::from(1)));
}

#[rstest]
fn test_open_position_on_start_opens_without_quote_subscription(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.instrument_id = instrument.id();
    config.open_position_on_start_qty = Some(Decimal::from(1));
    config.open_position_on_first_quote = true;
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.subscribe_quotes = false;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);

    tester.on_instrument(&instrument).unwrap();
    assert!(tester.pending_open_position_qty.is_none());
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
fn test_on_quote_opens_start_position_when_configured(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.log_data = false;
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.open_position_on_start_qty = Some(Decimal::from(1));
    config.open_position_on_first_quote = true;
    let instrument_id = instrument.id();
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let quote = QuoteTick::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Price::from("50000.0"),
        Price::from("50001.0"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_quote(&quote);
    assert!(result.is_ok());
    assert!(tester.open_position_submitted);

    let first_count = tester
        .cache()
        .client_order_ids(None, Some(&instrument_id), None, None)
        .len();

    let result = tester.on_quote(&quote);
    let second_count = tester
        .cache()
        .client_order_ids(None, Some(&instrument_id), None, None)
        .len();

    assert!(result.is_ok());
    assert_eq!(first_count, 1);
    assert_eq!(second_count, first_count);
}

#[rstest]
fn test_on_quote_does_not_open_start_position_without_first_quote_flag(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.log_data = false;
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.open_position_on_start_qty = Some(Decimal::from(1));
    let instrument_id = instrument.id();
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let quote = QuoteTick::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Price::from("50000.0"),
        Price::from("50001.0"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_quote(&quote);
    let order_count = tester
        .cache()
        .client_order_ids(None, Some(&instrument_id), None, None)
        .len();

    assert!(result.is_ok());
    assert!(!tester.open_position_submitted);
    assert_eq!(order_count, 0);
}

#[rstest]
fn test_on_quote_waits_for_instrument_before_opening_start_position(mut config: ExecTesterConfig) {
    config.log_data = false;
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.open_position_on_start_qty = Some(Decimal::from(1));
    config.open_position_on_first_quote = true;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);

    let quote = QuoteTick::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Price::from("50000.0"),
        Price::from("50001.0"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = tester.on_quote(&quote);

    assert!(result.is_ok());
    assert!(!tester.open_position_submitted);
}

#[rstest]
fn test_on_instrument_opens_start_position_immediately_by_default(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.open_position_on_start_qty = Some(Decimal::from(1));
    let instrument_id = instrument.id();
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);

    let result = tester.on_instrument(&instrument);

    let cache_ref = tester.cache();
    let orders = cache_ref
        .client_order_ids(None, Some(&instrument_id), None, None)
        .into_iter()
        .filter_map(|client_order_id| cache_ref.order(&client_order_id))
        .collect::<Vec<_>>();
    assert!(result.is_ok());
    assert!(tester.open_position_submitted);
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].order_type(), OrderType::Market);
    assert_eq!(orders[0].order_side(), OrderSide::Buy);
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
    let instrument_id = instrument.id();
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument);

    let result = tester.open_position(Decimal::from(1));

    let cache_ref = tester.cache();
    let orders = cache_ref
        .client_order_ids(None, Some(&instrument_id), None, None)
        .into_iter()
        .filter_map(|client_order_id| cache_ref.order(&client_order_id))
        .collect::<Vec<_>>();
    assert!(result.is_ok());
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].order_type(), OrderType::Market);
    assert_eq!(orders[0].order_side(), OrderSide::Buy);
    assert_eq!(orders[0].quantity(), Quantity::from("1.00000000"));
    assert_eq!(orders[0].time_in_force(), TimeInForce::Gtc);
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

#[rstest]
fn test_limit_order_is_one_shot_default_false(config: ExecTesterConfig) {
    let tester = ExecTester::new(config);
    assert!(!tester.limit_order_is_one_shot());
}

#[rstest]
#[case::ioc(TimeInForce::Ioc)]
#[case::fok(TimeInForce::Fok)]
fn test_limit_order_is_one_shot_for_terminal_tifs(
    mut config: ExecTesterConfig,
    #[case] time_in_force: TimeInForce,
) {
    config.limit_time_in_force = Some(time_in_force);
    let tester = ExecTester::new(config);
    assert!(tester.limit_order_is_one_shot());
}

#[rstest]
fn test_limit_order_is_one_shot_for_reject_post_only(mut config: ExecTesterConfig) {
    config.test_reject_post_only = true;
    let tester = ExecTester::new(config);
    assert!(tester.limit_order_is_one_shot());
}

#[rstest]
fn test_limit_order_is_one_shot_for_aggressive_limit(mut config: ExecTesterConfig) {
    config.limit_aggressive = true;
    let tester = ExecTester::new(config);
    assert!(tester.limit_order_is_one_shot());
}

#[rstest]
fn test_limit_order_is_one_shot_for_gtd_delta(mut config: ExecTesterConfig) {
    config.order_expire_time_delta_mins = Some(30);
    let tester = ExecTester::new(config);
    assert!(tester.limit_order_is_one_shot());
}

#[rstest]
fn test_stop_order_is_one_shot_default_false(config: ExecTesterConfig) {
    let tester = ExecTester::new(config);
    assert!(!tester.stop_order_is_one_shot());
}

#[rstest]
#[case::ioc(TimeInForce::Ioc)]
#[case::fok(TimeInForce::Fok)]
fn test_stop_order_is_one_shot_for_terminal_tifs(
    mut config: ExecTesterConfig,
    #[case] time_in_force: TimeInForce,
) {
    config.stop_time_in_force = Some(time_in_force);
    let tester = ExecTester::new(config);
    assert!(tester.stop_order_is_one_shot());
}

#[rstest]
fn test_stop_order_is_one_shot_for_gtd_delta(mut config: ExecTesterConfig) {
    config.order_expire_time_delta_mins = Some(30);
    let tester = ExecTester::new(config);
    assert!(tester.stop_order_is_one_shot());
}

#[rstest]
fn test_stop_order_is_one_shot_for_trailing_stop(mut config: ExecTesterConfig) {
    config.stop_order_type = OrderType::TrailingStopMarket;
    let tester = ExecTester::new(config);
    assert!(tester.stop_order_is_one_shot());
}

#[rstest]
fn test_maintain_limit_ioc_does_not_resubmit_after_rejection(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = true;
    config.enable_limit_sells = false;
    config.limit_time_in_force = Some(TimeInForce::Ioc);
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);
    let risk_saver = capture_risk_commands();

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));
    let buy_id = tester.buy_order.as_ref().unwrap().client_order_id();
    apply_rejected_in_cache(&cache, buy_id);
    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let submits = submit_orders(&risk_saver);
    assert_eq!(submits.len(), 1, "IOC limit should submit once");
    assert_eq!(
        tester.buy_order.as_ref().unwrap().status(),
        OrderStatus::Rejected,
    );
}

#[rstest]
fn test_maintain_stop_ioc_does_not_resubmit_after_rejection(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::StopMarket;
    config.stop_time_in_force = Some(TimeInForce::Ioc);
    config.stop_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.instrument = Some(instrument);
    let risk_saver = capture_risk_commands();

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));
    let buy_id = tester.buy_stop_order.as_ref().unwrap().client_order_id();
    apply_rejected_in_cache(&cache, buy_id);
    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let submits = submit_orders(&risk_saver);
    assert_eq!(submits.len(), 1, "IOC stop should submit once");
    assert_eq!(
        tester.buy_stop_order.as_ref().unwrap().status(),
        OrderStatus::Rejected,
    );
}

#[rstest]
#[case::buy(OrderSide::Buy, "3001.0", "3002.0", "3002.0", "3003.0")]
#[case::sell(OrderSide::Sell, "2999.0", "3000.0", "2998.0", "2999.0")]
fn test_limit_cancel_replace_guard_fires_once(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
    #[case] side: OrderSide,
    #[case] second_bid: &str,
    #[case] second_ask: &str,
    #[case] third_bid: &str,
    #[case] third_ask: &str,
) {
    config.enable_limit_buys = side == OrderSide::Buy;
    config.enable_limit_sells = side == OrderSide::Sell;
    config.cancel_replace_orders_to_maintain_tob_offset = true;
    config.tob_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));
    let first_id = tracked_limit_order(&tester, side).client_order_id();
    ack_order_in_cache(&cache, first_id, "V-FIRST");
    let exec_saver = capture_exec_commands();
    let risk_saver = capture_risk_commands();

    tester.maintain_orders(Price::from(second_bid), Price::from(second_ask));
    let replacement_id = tracked_limit_order(&tester, side).client_order_id();
    ack_order_in_cache(&cache, replacement_id, "V-REPLACEMENT");
    tester.maintain_orders(Price::from(third_bid), Price::from(third_ask));

    let submits = submit_orders(&risk_saver);
    assert_ne!(replacement_id, first_id);
    assert_eq!(cancel_order_ids(&exec_saver), vec![first_id]);
    assert_eq!(submits.len(), 1, "expected one replacement SubmitOrder");
    assert_eq!(submits[0].client_order_id, replacement_id);
    assert!(
        cancel_replace_attempted(&tester, side),
        "limit cancel-replace guard should be consumed",
    );
}

#[rstest]
#[case::buy(OrderSide::Buy, "3001.0", "3002.0", "3002.0", "3003.0")]
#[case::sell(OrderSide::Sell, "2999.0", "3000.0", "2998.0", "2999.0")]
fn test_stop_cancel_replace_guard_fires_once(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
    #[case] side: OrderSide,
    #[case] second_bid: &str,
    #[case] second_ask: &str,
    #[case] third_bid: &str,
    #[case] third_ask: &str,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_buys = side == OrderSide::Buy;
    config.enable_stop_sells = side == OrderSide::Sell;
    config.cancel_replace_stop_orders_to_maintain_offset = true;
    config.stop_order_type = OrderType::StopMarket;
    config.stop_offset_ticks = 5;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));
    let first_id = tracked_stop_order(&tester, side).client_order_id();
    ack_order_in_cache(&cache, first_id, "V-FIRST");
    let exec_saver = capture_exec_commands();
    let risk_saver = capture_risk_commands();

    tester.maintain_orders(Price::from(second_bid), Price::from(second_ask));
    let replacement_id = tracked_stop_order(&tester, side).client_order_id();
    ack_order_in_cache(&cache, replacement_id, "V-REPLACEMENT");
    tester.maintain_orders(Price::from(third_bid), Price::from(third_ask));

    let submits = submit_orders(&risk_saver);
    assert_ne!(replacement_id, first_id);
    assert_eq!(cancel_order_ids(&exec_saver), vec![first_id]);
    assert_eq!(submits.len(), 1, "expected one replacement SubmitOrder");
    assert_eq!(submits[0].client_order_id, replacement_id);
    assert_eq!(submits[0].order_init.order_type, OrderType::StopMarket);
    assert!(
        stop_cancel_replace_attempted(&tester, side),
        "stop cancel-replace guard should be consumed",
    );
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
        .clone()
        .expect("buy order should be tracked locally");
    let cid = order.client_order_id();
    let strategy_id = order.strategy_id();
    let instrument_id = order.instrument_id();

    let accepted = OrderAcceptedSpec::builder()
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(cid)
        .venue_order_id(VenueOrderId::from("V-1"))
        .build();

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

fn capture_risk_commands() -> TypedIntoMessageSavingHandler<TradingCommand> {
    let (handler, saver): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
        get_typed_into_message_saving_handler(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        handler,
    );
    saver
}

fn capture_exec_commands() -> TypedIntoMessageSavingHandler<TradingCommand> {
    let (handler, saver): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
        get_typed_into_message_saving_handler(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        handler,
    );
    saver
}

fn capture_exec_cancels() -> TypedIntoMessageSavingHandler<TradingCommand> {
    capture_exec_commands()
}

fn submit_orders(saver: &TypedIntoMessageSavingHandler<TradingCommand>) -> Vec<SubmitOrder> {
    saver
        .get_messages()
        .into_iter()
        .filter_map(|cmd| match cmd {
            TradingCommand::SubmitOrder(cmd) => Some(cmd),
            _ => None,
        })
        .collect()
}

fn cancel_order_ids(saver: &TypedIntoMessageSavingHandler<TradingCommand>) -> Vec<ClientOrderId> {
    saver
        .get_messages()
        .into_iter()
        .filter_map(|cmd| match cmd {
            TradingCommand::CancelOrder(cmd) => Some(cmd.client_order_id),
            _ => None,
        })
        .collect()
}

fn tracked_limit_order(tester: &ExecTester, side: OrderSide) -> &OrderAny {
    match side {
        OrderSide::Buy => tester.buy_order.as_ref().expect("buy order should exist"),
        OrderSide::Sell => tester.sell_order.as_ref().expect("sell order should exist"),
        OrderSide::NoOrderSide => panic!("Unsupported order side {side:?}"),
    }
}

fn tracked_stop_order(tester: &ExecTester, side: OrderSide) -> &OrderAny {
    match side {
        OrderSide::Buy => tester
            .buy_stop_order
            .as_ref()
            .expect("buy stop order should exist"),
        OrderSide::Sell => tester
            .sell_stop_order
            .as_ref()
            .expect("sell stop order should exist"),
        OrderSide::NoOrderSide => panic!("Unsupported order side {side:?}"),
    }
}

fn cancel_replace_attempted(tester: &ExecTester, side: OrderSide) -> bool {
    match side {
        OrderSide::Buy => tester.buy_cancel_replace_attempted,
        OrderSide::Sell => tester.sell_cancel_replace_attempted,
        OrderSide::NoOrderSide => panic!("Unsupported order side {side:?}"),
    }
}

fn stop_cancel_replace_attempted(tester: &ExecTester, side: OrderSide) -> bool {
    match side {
        OrderSide::Buy => tester.buy_stop_cancel_replace_attempted,
        OrderSide::Sell => tester.sell_stop_cancel_replace_attempted,
        OrderSide::NoOrderSide => panic!("Unsupported order side {side:?}"),
    }
}

fn rejected_event_for(order: &OrderAny) -> OrderRejected {
    OrderRejectedSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .account_id(AccountId::from("SIM-001"))
        .event_id(UUID4::new())
        .build()
}

// Bracket TP/SL legs stay INITIALIZED until the entry fills; on_stop must reach
// them via cache.order_lists() since no live index covers them.
#[rstest]
fn test_on_stop_cancels_initialized_bracket_legs(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.bracket_offset_ticks = 100;
    config.close_positions_on_stop = false;
    config.can_unsubscribe = false;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.instrument = Some(instrument);

    tester
        .submit_bracket_order(OrderSide::Buy, Price::from("3000.0"))
        .unwrap();

    let entry = tester.buy_order.as_ref().expect("entry must exist").clone();
    let order_list_id = entry.order_list_id().expect("bracket order list id");
    let order_list = {
        let cache_ref = cache.borrow();
        cache_ref.order_list(&order_list_id).cloned()
    }
    .expect("order list cached");

    let expected_ids: AHashSet<ClientOrderId> =
        order_list.client_order_ids.iter().copied().collect();

    let saver = capture_exec_cancels();

    tester.on_stop().unwrap();

    let canceled_list = cancel_order_ids(&saver);
    let canceled: AHashSet<ClientOrderId> = canceled_list.iter().copied().collect();
    assert!(
        expected_ids.is_subset(&canceled),
        "expected all bracket legs to be canceled; expected {expected_ids:?}, canceled {canceled:?}",
    );
    // Each leg cancelled exactly once: guards the contingency filter on the
    // bracket sweep + active-local supplement against regression.
    assert_eq!(
        canceled_list.len(),
        canceled.len(),
        "expected no duplicate cancels, saw {canceled_list:?}",
    );
}

// PENDING_CANCEL sits in both open and inflight indexes; the candidate sweep
// must skip it and return each order exactly once.
#[rstest]
fn test_collect_cancellable_orders_dedupes_and_skips_pending_cancel(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = true;
    config.enable_limit_sells = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let buy_id = tester.buy_order.as_ref().unwrap().client_order_id();
    let sell_id = tester.sell_order.as_ref().unwrap().client_order_id();

    ack_order_in_cache(&cache, buy_id, "V-OPEN");
    ack_order_in_cache(&cache, sell_id, "V-PENDING");
    apply_pending_cancel_in_cache(&cache, sell_id);

    let strategy_id = StrategyId::from(tester.core.actor_id.inner().as_str());
    let candidates = tester.collect_cancellable_orders(tester.config.instrument_id, strategy_id);
    let candidate_ids: Vec<ClientOrderId> = candidates.iter().map(Order::client_order_id).collect();

    assert!(candidate_ids.contains(&buy_id));
    assert!(!candidate_ids.contains(&sell_id));
    let unique: AHashSet<ClientOrderId> = candidate_ids.iter().copied().collect();
    assert_eq!(unique.len(), candidate_ids.len());
}

fn ack_order_in_cache(cache: &Rc<RefCell<Cache>>, cid: ClientOrderId, venue_order_id: &str) {
    let order = cache
        .borrow()
        .order(&cid)
        .map(|o| o.cloned())
        .expect("order present");
    let accepted = OrderAcceptedSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(cid)
        .venue_order_id(VenueOrderId::from(venue_order_id))
        .build();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Accepted(accepted))
        .unwrap();
}

fn apply_rejected_in_cache(cache: &Rc<RefCell<Cache>>, cid: ClientOrderId) {
    let order = cache
        .borrow()
        .order(&cid)
        .map(|o| o.cloned())
        .expect("order present");
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Rejected(rejected_event_for(&order)))
        .unwrap();
}

// Clamp on: a BUY offset larger than the bid pulls up to min_price instead of
// underflowing to the adapter signer.
#[rstest]
fn test_maintain_buy_orders_clamps_price_to_instrument_min(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = true;
    config.enable_limit_sells = false;
    config.tob_offset_ticks = 10_000_000; // Drives sub_price_ticks below min_price
    config.clamp_to_instrument_price_range = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument.clone());

    tester.maintain_orders(Price::from("0.50"), Price::from("0.51"));

    let buy = tester.buy_order.as_ref().expect("buy order submitted");
    assert_eq!(buy.price(), instrument.min_price());
}

// Clamp off: legacy behavior preserved, underflow passes through.
#[rstest]
fn test_maintain_buy_orders_without_clamp_passes_underflowed_price(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = true;
    config.enable_limit_sells = false;
    config.tob_offset_ticks = 10_000_000;
    config.clamp_to_instrument_price_range = false;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument.clone());

    tester.maintain_orders(Price::from("0.50"), Price::from("0.51"));

    let buy = tester.buy_order.as_ref().expect("buy order submitted");
    let min_price = instrument.min_price().expect("instrument has min_price");
    assert!(
        buy.price().is_some_and(|p| p < min_price),
        "expected price below min when clamp disabled, was {:?}",
        buy.price(),
    );
}

// Clamp must reach the bracket SL leg, not only the entry price passed in by
// the caller.
#[rstest]
fn test_submit_bracket_order_clamps_sl_trigger_to_instrument_min(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.bracket_offset_ticks = 10_000_000;
    config.clamp_to_instrument_price_range = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.instrument = Some(instrument.clone());

    tester
        .submit_bracket_order(OrderSide::Buy, Price::from("0.50"))
        .unwrap();

    let entry = tester.buy_order.as_ref().expect("entry submitted");
    let order_list_id = entry.order_list_id().expect("bracket order list id");
    let order_list = {
        let cache_ref = cache.borrow();
        cache_ref.order_list(&order_list_id).cloned()
    }
    .expect("order list cached");

    let cache_ref = cache.borrow();
    let sl = order_list
        .client_order_ids
        .iter()
        .filter_map(|cid| cache_ref.order(cid).map(|o| o.cloned()))
        .find(|o| o.order_type() == OrderType::StopMarket)
        .expect("SL leg present");
    assert_eq!(sl.trigger_price(), instrument.min_price());
}

// An INITIALIZED non-bracket order lives only in `orders_active_local`; the
// default cancel-all branch must still reach it.
#[rstest]
fn test_on_stop_cancels_initialized_non_bracket_via_default_branch(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = true;
    config.enable_limit_sells = false;
    config.use_individual_cancels_on_stop = false;
    config.use_batch_cancel_on_stop = false;
    config.close_positions_on_stop = false;
    config.can_unsubscribe = false;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));
    let buy_id = tester
        .buy_order
        .as_ref()
        .expect("buy order created")
        .client_order_id();

    let saver = capture_exec_cancels();

    tester.on_stop().unwrap();

    let canceled = cancel_order_ids(&saver);
    assert!(
        canceled.contains(&buy_id),
        "expected CancelOrder for INITIALIZED non-bracket order, saw {canceled:?}",
    );
}

fn apply_pending_cancel_in_cache(cache: &Rc<RefCell<Cache>>, cid: ClientOrderId) {
    let order = cache
        .borrow()
        .order(&cid)
        .map(|o| o.cloned())
        .expect("order present");
    let event = OrderPendingCancelSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(cid)
        .account_id(order.account_id().expect("account id"))
        .maybe_venue_order_id(order.venue_order_id())
        .build();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::PendingCancel(event))
        .unwrap();
}

// Bracket sweep must not hijack a `batch_submit_limit_pair` OrderList, otherwise
// the configured batch mode is starved. Guards `is_in_contingency_group` against
// the `NoContingency` false-positive.
#[rstest]
fn test_batch_submit_limit_pair_flows_through_batch_cancel(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.batch_submit_limit_pair = true;
    config.use_batch_cancel_on_stop = true;
    config.close_positions_on_stop = false;
    config.can_unsubscribe = false;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));

    let buy_id = tester.buy_order.as_ref().unwrap().client_order_id();
    let sell_id = tester.sell_order.as_ref().unwrap().client_order_id();
    // ACCEPTED so they are eligible for batch (active-local is filtered out).
    ack_order_in_cache(&cache, buy_id, "V-BATCH-BUY");
    ack_order_in_cache(&cache, sell_id, "V-BATCH-SELL");

    let saver = capture_exec_cancels();

    tester.on_stop().unwrap();

    let messages = saver.get_messages();
    let batch_count = messages
        .iter()
        .filter(|c| matches!(c, TradingCommand::CancelOrders(_)))
        .count();
    let cancel_count = messages
        .iter()
        .filter(|c| matches!(c, TradingCommand::CancelOrder(_)))
        .count();
    assert_eq!(batch_count, 1, "expected exactly one BatchCancelOrders");
    assert_eq!(
        cancel_count, 0,
        "expected no per-order CancelOrders, saw {messages:?}",
    );
}

// Individual cancel mode must also reach INITIALIZED orders via
// `orders_active_local` in `collect_cancellable_orders`.
#[rstest]
fn test_on_stop_individual_mode_cancels_initialized_order(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = true;
    config.enable_limit_sells = false;
    config.use_individual_cancels_on_stop = true;
    config.use_batch_cancel_on_stop = false;
    config.close_positions_on_stop = false;
    config.can_unsubscribe = false;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("3000.0"), Price::from("3001.0"));
    let buy_id = tester.buy_order.as_ref().unwrap().client_order_id();

    let saver = capture_exec_cancels();

    tester.on_stop().unwrap();

    let canceled = cancel_order_ids(&saver);
    assert!(
        canceled.contains(&buy_id),
        "individual mode must cancel INITIALIZED non-bracket order, saw {canceled:?}",
    );
}

// Bracket legs must be excluded from `collect_cancellable_orders` so the bracket
// sweep + configured branch don't double-cancel them.
#[rstest]
fn test_collect_cancellable_orders_excludes_contingency_group(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_brackets = true;
    config.enable_limit_buys = true;
    config.enable_limit_sells = false;
    config.bracket_offset_ticks = 100;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache.clone());
    tester.instrument = Some(instrument.clone());

    tester
        .submit_bracket_order(OrderSide::Buy, Price::from("3000.0"))
        .unwrap();
    let entry_id = tester.buy_order.as_ref().unwrap().client_order_id();
    let order_list_id = tester
        .buy_order
        .as_ref()
        .unwrap()
        .order_list_id()
        .expect("bracket order_list_id");

    // Plain limit added directly so it lands in cache without an OrderList.
    let plain = tester.order_factory().limit(
        instrument.id(),
        OrderSide::Sell,
        Quantity::from("0.01"),
        Price::from("3500.0"),
        Some(TimeInForce::Gtc),
        None,
        Some(false),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let plain_id = plain.client_order_id();
    cache
        .borrow_mut()
        .add_order(plain, None, None, true)
        .unwrap();

    let strategy_id = StrategyId::from(tester.core.actor_id.inner().as_str());
    let candidates = tester.collect_cancellable_orders(tester.config.instrument_id, strategy_id);
    let candidate_ids: Vec<ClientOrderId> = candidates.iter().map(Order::client_order_id).collect();

    assert!(
        candidate_ids.contains(&plain_id),
        "plain limit must be in candidates, saw {candidate_ids:?}",
    );
    assert!(
        !candidate_ids.contains(&entry_id),
        "bracket entry must NOT be in candidates (handled by bracket sweep)",
    );
    let bracket_ids: AHashSet<ClientOrderId> = cache
        .borrow()
        .order_list(&order_list_id)
        .unwrap()
        .client_order_ids
        .iter()
        .copied()
        .collect();

    for id in &candidate_ids {
        assert!(
            !bracket_ids.contains(id),
            "candidate {id} is a bracket leg; should have been excluded",
        );
    }
}

// SELL overflow mirror: a huge offset above the ask must clamp down to max_price.
#[rstest]
fn test_maintain_sell_orders_clamps_price_to_instrument_max(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = true;
    config.tob_offset_ticks = 10_000_000; // Drives add_price_ticks above max_price
    config.clamp_to_instrument_price_range = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument.clone());

    tester.maintain_orders(Price::from("100.0"), Price::from("101.0"));

    let sell = tester.sell_order.as_ref().expect("sell order submitted");
    assert_eq!(sell.price(), instrument.max_price());
}

// BUY stop trigger overflow: clamp down to max_price.
#[rstest]
fn test_maintain_stop_buy_orders_clamps_trigger_to_instrument_max(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_buys = true;
    config.stop_order_type = OrderType::StopMarket;
    config.stop_offset_ticks = 10_000_000;
    config.clamp_to_instrument_price_range = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument.clone());

    tester.maintain_orders(Price::from("100.0"), Price::from("101.0"));

    let stop = tester.buy_stop_order.as_ref().expect("buy stop submitted");
    assert_eq!(stop.trigger_price(), instrument.max_price());
}

// SELL stop trigger underflow: clamp up to min_price.
#[rstest]
fn test_maintain_stop_sell_orders_clamps_trigger_to_instrument_min(
    mut config: ExecTesterConfig,
    instrument: InstrumentAny,
) {
    config.enable_limit_buys = false;
    config.enable_limit_sells = false;
    config.enable_stop_sells = true;
    config.stop_order_type = OrderType::StopMarket;
    config.stop_offset_ticks = 10_000_000;
    config.clamp_to_instrument_price_range = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.instrument = Some(instrument.clone());

    tester.maintain_orders(Price::from("5.0"), Price::from("6.0"));

    let stop = tester
        .sell_stop_order
        .as_ref()
        .expect("sell stop submitted");
    assert_eq!(stop.trigger_price(), instrument.min_price());
}

// Clamp is a no-op when both bounds are None: underflow passes through.
#[rstest]
fn test_clamp_passes_through_when_instrument_unbounded(mut config: ExecTesterConfig) {
    let mut perp = crypto_perpetual_ethusdt();
    perp.min_price = None;
    perp.max_price = None;
    let instrument = InstrumentAny::CryptoPerpetual(perp);

    config.enable_limit_buys = true;
    config.enable_limit_sells = false;
    config.tob_offset_ticks = 10_000_000;
    config.clamp_to_instrument_price_range = true;
    let cache = create_cache_with_instrument(&instrument);
    let mut tester = ExecTester::new(config);
    register_exec_tester(&mut tester, cache);
    tester.price_offset = Some(tester.get_price_offset(&instrument));
    tester.instrument = Some(instrument);

    tester.maintain_orders(Price::from("100.0"), Price::from("101.0"));

    let buy = tester.buy_order.as_ref().expect("buy submitted");
    let buy_price = buy.price().expect("limit has price");
    assert!(
        buy_price < Price::from("0.0"),
        "expected negative pass-through price, was {buy_price}",
    );
}

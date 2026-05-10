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

#![expect(clippy::too_many_arguments)] // Test functions with many fixtures

use std::{cell::RefCell, rc::Rc, str::FromStr};

use ahash::AHashMap;
use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    messages::{
        execution::{ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand},
        system::trading::TradingStateChanged,
    },
    msgbus::{
        self, MessagingSwitchboard,
        stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
    },
    throttler::RateLimit,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
use nautilus_model::{
    accounts::{AccountAny, BettingAccount, CashAccount, MarginAccount, stubs::cash_account},
    data::{QuoteTick, stubs::quote_audusd},
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderType, PositionSide, TimeInForce,
        TradingState, TrailingOffsetType, TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderEventAny, OrderEventType, OrderFilled, OrderSubmitted,
        account::stubs::cash_account_state_million_usd,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
        Symbol, TradeId, TraderId, VenueOrderId,
        stubs::{
            account_id, client_id_binance, client_order_id, strategy_id_ema_cross, trader_id,
            uuid4, venue_order_id,
        },
    },
    instruments::{
        CryptoPerpetual, CurrencyPair, FuturesSpread, Instrument, InstrumentAny, OptionSpread,
        stubs::{
            audusd_sim, betting, crypto_perpetual_ethusdt, futures_spread_es, option_spread,
            xbtusd_bitmex,
        },
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity, fixed::FIXED_PRECISION},
};
use nautilus_portfolio::Portfolio;
use rstest::{fixture, rstest};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use rust_decimal_macros::dec;
use ustr::Ustr;

// Helper that registers message collectors for ExecEngine.process events and
// returns the shared handler so callers can later retrieve the collected
// OrderEventAny messages via `get_process_order_event_handler_messages`.
fn register_process_handler() -> TypedIntoMessageSavingHandler<OrderEventAny> {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(Some(
        Ustr::from("ExecEngine.process"),
    ));
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
    saving_handler
}

#[rstest]
fn test_deny_order_on_price_precision_exceeded(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
) {
    // Register collector for denied events
    let process_handler = register_process_handler();

    // Build a RiskEngine with default (non-bypassed) settings and an account with ample balance
    let mut cache = Cache::default();
    cache.add_instrument(instrument_audusd.clone()).unwrap();
    // Add large cash account so balance checks pass (focus is price precision)
    cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    // Add a last quote so notional calculation can proceed if needed
    cache.add_quote(quote_audusd()).unwrap();

    let mut risk_engine = get_risk_engine(Some(Rc::new(RefCell::new(cache))), None, None, false);

    // AUD/USD price precision is 5 – create a Limit order with 6-dp price (invalid)
    let bad_price = Price::from("1.000001"); // precision 6
    assert!(bad_price.precision > instrument_audusd.price_precision());

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(bad_price)
        .quantity(Quantity::from("1000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // Expect an OrderDenied to be emitted
    let saved_events = get_process_order_event_handler_messages(&process_handler);
    assert_eq!(saved_events.len(), 1);
    matches!(saved_events[0], OrderEventAny::Denied(_));
}

#[rstest]
fn test_deny_order_exceeding_max_notional(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
) {
    let process_handler = register_process_handler();

    // Prepare small max_notional setting (1 USD)
    let mut max_notional_map = AHashMap::new();
    max_notional_map.insert(instrument_audusd.id(), Decimal::from_i64(1).unwrap());

    let mut cache = Cache::default();
    cache.add_instrument(instrument_audusd.clone()).unwrap();
    cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();
    cache.add_quote(quote_audusd()).unwrap();

    let risk_config = RiskEngineConfig {
        debug: true,
        bypass: false,
        max_order_submit: RateLimit::new(10, 1000),
        max_order_modify: RateLimit::new(5, 1000),
        max_notional_per_order: AHashMap::new(),
    };

    let mut risk_engine = get_risk_engine(
        Some(Rc::new(RefCell::new(cache))),
        Some(risk_config),
        None,
        false,
    );

    risk_engine.set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(1).unwrap());

    // Build an order with notional ~100 USD (price 1, qty 100) > max 1 USD
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::from("1"))
        .quantity(Quantity::from("100"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_events = get_process_order_event_handler_messages(&process_handler);
    assert_eq!(saved_events.len(), 1);
    matches!(saved_events[0], OrderEventAny::Denied(_));
}

use nautilus_risk::engine::{RiskEngine, config::RiskEngineConfig};

#[fixture]
fn process_order_event_handler() -> TypedIntoMessageSavingHandler<OrderEventAny> {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(Some(
        Ustr::from("ExecEngine.process"),
    ));
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
    saving_handler
}

#[fixture]
fn execute_order_event_handler() -> TypedIntoMessageSavingHandler<TradingCommand> {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<TradingCommand>(Some(
        Ustr::from("ExecEngine.queue_execute"),
    ));
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        handler,
    );
    saving_handler
}

#[fixture]
fn simple_cache() -> Cache {
    Cache::new(None, None)
}

#[fixture]
fn clock() -> TestClock {
    TestClock::new()
}

#[fixture]
fn max_order_submit() -> RateLimit {
    RateLimit::new(10, 1)
}

#[fixture]
fn max_order_modify() -> RateLimit {
    RateLimit::new(5, 1)
}

#[fixture]
fn max_notional_per_order() -> AHashMap<InstrumentId, Decimal> {
    AHashMap::new()
}

// Market buy order with corresponding fill
#[fixture]
fn market_order_buy(instrument_eth_usdt: InstrumentAny) -> OrderAny {
    OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .build()
}

// Market sell order
#[fixture]
fn market_order_sell(instrument_eth_usdt: InstrumentAny) -> OrderAny {
    OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1"))
        .build()
}

#[fixture]
#[allow(dead_code)]
fn get_stub_submit_order(
    trader_id: TraderId,
    client_id_binance: ClientId,
    strategy_id_ema_cross: StrategyId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    instrument_eth_usdt: InstrumentAny,
) -> (OrderAny, SubmitOrder) {
    let order = market_order_buy(instrument_eth_usdt.clone());
    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        UnixNanos::from(10),
    );
    (order, submit_order)
}

#[fixture]
fn config_fixture(
    max_order_submit: RateLimit,
    max_order_modify: RateLimit,
    max_notional_per_order: AHashMap<InstrumentId, Decimal>,
) -> RiskEngineConfig {
    RiskEngineConfig {
        debug: true,
        bypass: false,
        max_order_submit,
        max_order_modify,
        max_notional_per_order,
    }
}

#[fixture]
pub fn bitmex_cash_account_state_multi() -> AccountState {
    let btc_account_balance = AccountBalance::new(
        Money::from("10 BTC"),
        Money::from("0 BTC"),
        Money::from("10 BTC"),
    );
    let eth_account_balance = AccountBalance::new(
        Money::from("20 ETH"),
        Money::from("0 ETH"),
        Money::from("20 ETH"),
    );
    AccountState::new(
        AccountId::from("BITMEX-001"),
        AccountType::Cash,
        vec![btc_account_balance, eth_account_balance],
        vec![],
        true,
        uuid4(),
        0.into(),
        0.into(),
        None, // multi cash account
    )
}

fn get_process_order_event_handler_messages(
    event_handler: &TypedIntoMessageSavingHandler<OrderEventAny>,
) -> Vec<OrderEventAny> {
    event_handler.get_messages()
}

fn get_execute_order_event_handler_messages(
    event_handler: &TypedIntoMessageSavingHandler<TradingCommand>,
) -> Vec<TradingCommand> {
    event_handler.get_messages()
}

#[fixture]
fn instrument_eth_usdt(crypto_perpetual_ethusdt: CryptoPerpetual) -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt)
}

#[fixture]
fn instrument_xbtusd_bitmex(xbtusd_bitmex: CryptoPerpetual) -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(xbtusd_bitmex)
}

#[fixture]
fn instrument_audusd(audusd_sim: CurrencyPair) -> InstrumentAny {
    InstrumentAny::CurrencyPair(audusd_sim)
}

#[fixture]
fn instrument_futures_spread(futures_spread_es: FuturesSpread) -> InstrumentAny {
    InstrumentAny::FuturesSpread(futures_spread_es)
}

#[fixture]
fn instrument_option_spread(option_spread: OptionSpread) -> InstrumentAny {
    InstrumentAny::OptionSpread(option_spread)
}

#[fixture]
pub fn instrument_xbtusd_with_high_size_precision() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        InstrumentId::from("BTCUSDT.BITMEX"),
        Symbol::from("XBTUSD"),
        Currency::BTC(),
        Currency::USD(),
        Currency::BTC(),
        true,
        1,
        2,
        Price::from("0.5"),
        Quantity::from("0.01"),
        None,
        None,
        None,
        None,
        Some(Money::from("10000000 USD")),
        Some(Money::from("1 USD")),
        Some(Price::from("10000000")),
        Some(Price::from("0.01")),
        Some(dec!(0.01)),
        Some(dec!(0.0035)),
        Some(dec!(-0.00025)),
        Some(dec!(0.00075)),
        None, // info
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

// Helpers
fn get_risk_engine(
    cache: Option<Rc<RefCell<Cache>>>,
    config: Option<RiskEngineConfig>,
    clock: Option<Rc<RefCell<TestClock>>>,
    bypass: bool,
) -> RiskEngine {
    let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
    let config = config.unwrap_or(RiskEngineConfig {
        debug: true,
        bypass,
        max_order_submit: RateLimit::new(10, 1000),
        max_order_modify: RateLimit::new(5, 1000),
        max_notional_per_order: AHashMap::new(),
    });
    let clock = clock.unwrap_or(Rc::new(RefCell::new(TestClock::new())));
    let portfolio = Portfolio::new(cache.clone(), clock.clone(), None);
    RiskEngine::new(config, portfolio, clock, cache)
}

fn get_exec_engine(
    cache: Option<Rc<RefCell<Cache>>>,
    clock: Option<Rc<RefCell<TestClock>>>,
    config: Option<ExecutionEngineConfig>,
) -> ExecutionEngine {
    let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
    let clock = clock.unwrap_or(Rc::new(RefCell::new(TestClock::new())));
    ExecutionEngine::new(clock, cache, config)
}

fn order_submitted(order: &OrderAny) -> OrderSubmitted {
    OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.account_id().unwrap_or(account_id()),
        UUID4::new(),
        0.into(),
        0.into(),
    )
}

fn order_accepted(
    order: &OrderAny,
    venue_order_id: Option<VenueOrderId>,
    account_id: Option<AccountId>,
) -> OrderAccepted {
    OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id.expect("venue_order_id required for order_accepted"),
        account_id.unwrap_or_else(|| AccountId::new("SIM-001")),
        UUID4::new(),
        0.into(),
        0.into(),
        false,
    )
}

fn order_filled(
    order: &OrderAny,
    instrument: &InstrumentAny,
    strategy_id: Option<StrategyId>,
    account_id: Option<AccountId>,
    venue_order_id: Option<VenueOrderId>,
    trade_id: Option<TradeId>,
    last_qty: Option<Quantity>,
    last_px: Option<Price>,
    liquidity_side: Option<LiquiditySide>,
    account: Option<AccountAny>,
    ts_filled_ns: Option<UnixNanos>,
) -> OrderFilled {
    let strategy_id = strategy_id.unwrap_or(order.strategy_id());
    let account_id = account_id
        .or_else(|| order.account_id())
        .expect("account_id required for order_filled");
    let venue_order_id = venue_order_id
        .or_else(|| order.venue_order_id())
        .expect("venue_order_id required for order_filled");
    let trade_id = trade_id.unwrap_or(order.client_order_id().as_str().replace('O', "E").into());
    let last_qty = last_qty.unwrap_or(order.quantity());
    let last_px = last_px.unwrap_or(order.price().unwrap_or_default());
    let liquidity_side = liquidity_side.unwrap_or(LiquiditySide::Taker);
    let ts_filled_ns = ts_filled_ns.unwrap_or(0.into());
    let account = account.unwrap_or(AccountAny::Cash(cash_account(
        cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
    )));

    let commission = account
        .calculate_commission(instrument, order.quantity(), last_px, liquidity_side, None)
        .unwrap();

    OrderFilled::new(
        trader_id(),
        strategy_id,
        instrument.id(),
        order.client_order_id(),
        venue_order_id,
        account_id,
        trade_id,
        order.order_side(),
        order.order_type(),
        last_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        ts_filled_ns,
        0.into(),
        false,
        None,
        Some(commission),
    )
}

#[rstest]
fn test_bypass_config_risk_engine() {
    let risk_engine = get_risk_engine(
        None, None, None, true, // <-- Bypassing pre-trade risk checks for backtest
    );

    assert!(risk_engine.config().bypass);
}

#[rstest]
fn test_trading_state_after_instantiation_returns_active() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.trading_state(), TradingState::Active);
}

#[rstest]
fn test_set_trading_state_when_no_change_logs_warning() {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine.set_trading_state(TradingState::Active);

    assert_eq!(risk_engine.trading_state(), TradingState::Active);
}

#[rstest]
fn test_set_trading_state_changes_value_and_publishes_event() {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine.set_trading_state(TradingState::Halted);

    assert_eq!(risk_engine.trading_state(), TradingState::Halted);
}

#[rstest]
fn test_max_order_submit_rate_when_no_risk_config_returns_10_per_second() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.config().max_order_submit.limit, 10);
    assert_eq!(risk_engine.config().max_order_submit.interval_ns, 1000);
}

#[rstest]
fn test_max_order_modify_rate_when_no_risk_config_returns_5_per_second() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.config().max_order_modify.limit, 5);
    assert_eq!(risk_engine.config().max_order_modify.interval_ns, 1000);
}

#[rstest]
fn test_max_notionals_per_order_when_no_risk_config_returns_empty_hashmap() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(*risk_engine.max_notional_per_order(), AHashMap::new());
}

#[rstest]
fn test_set_max_notional_per_order_changes_setting(instrument_audusd: InstrumentAny) {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());

    let mut expected = AHashMap::new();
    expected.insert(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());
    assert_eq!(*risk_engine.max_notional_per_order(), expected);
}

#[rstest]
fn test_given_random_command_then_logs_and_continues(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
) {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::new(100.0, 0))
        .quantity(Quantity::from("1000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let random_command = TradingCommand::SubmitOrder(submit_order);

    risk_engine.execute(random_command);
}

// SUBMIT ORDER TESTS
#[rstest]
fn test_submit_order_with_default_settings_then_sends_to_client(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::new(100.0, 0))
        .quantity(Quantity::from("1000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_audusd.id()
    );
}

#[rstest]
fn test_submit_order_when_risk_bypassed_sends_to_execution_engine(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
) {
    let mut risk_engine = get_risk_engine(None, None, None, true);

    // TODO: Limit -> Market
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::new(100.0, 0))
        .quantity(Quantity::from("1000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_audusd.id()
    );
}

#[rstest]
fn test_submit_reduce_only_order_when_position_already_closed_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    clock: TestClock,
    simple_cache: Cache,
) {
    let clock = Rc::new(RefCell::new(clock));
    let simple_cache = Rc::new(RefCell::new(simple_cache));

    let mut risk_engine =
        get_risk_engine(Some(simple_cache.clone()), None, Some(clock.clone()), true);
    let mut exec_engine = get_exec_engine(Some(simple_cache), Some(clock), None);

    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1000"))
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1000"))
        .reduce_only(true)
        .build();

    let order3 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1000"))
        .reduce_only(true)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order1.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order1 = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order1.client_order_id(),
        order1.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let account_id = AccountId::new("SIM-001");
    let submitted = OrderEventAny::Submitted(order_submitted(&order1));
    let accepted = OrderEventAny::Accepted(order_accepted(
        &order1,
        Some(venue_order_id),
        Some(account_id),
    ));
    let filled = OrderEventAny::Filled(order_filled(
        &order1,
        &instrument_audusd,
        None,
        Some(account_id),
        Some(venue_order_id),
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
    exec_engine.process(&submitted);
    exec_engine.process(&accepted);
    exec_engine.process(&filled);

    let submit_order2 = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order2.client_order_id(),
        order2.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let venue_order_id2 = VenueOrderId::new("002");
    risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
    exec_engine.process(&OrderEventAny::Submitted(order_submitted(&order2)));
    exec_engine.process(&OrderEventAny::Filled(order_filled(
        &order2,
        &instrument_audusd,
        None,
        Some(account_id),
        Some(venue_order_id2),
        None,
        None,
        None,
        None,
        None,
        None,
    )));

    let submit_order3 = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order3.client_order_id(),
        order3.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order3));

    // TODO
    // assert_eq!(order1.status(), OrderStatus::Filled);
    // assert_eq!(order2.status(), OrderStatus::Filled);
    // assert_eq!(order3.status(), OrderStatus::Denied);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 3);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_audusd.id()
    );
}

#[rstest]
fn test_submit_reduce_only_order_when_position_would_be_increased_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    clock: TestClock,
    simple_cache: Cache,
) {
    let clock = Rc::new(RefCell::new(clock));
    let simple_cache = Rc::new(RefCell::new(simple_cache));

    let mut risk_engine =
        get_risk_engine(Some(simple_cache.clone()), None, Some(clock.clone()), true);
    let mut exec_engine = get_exec_engine(Some(simple_cache), Some(clock), None);

    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1000"))
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("2000"))
        .reduce_only(true)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order1.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order1 = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order1.client_order_id(),
        order1.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let account_id = AccountId::new("SIM-001");
    let submitted = OrderEventAny::Submitted(order_submitted(&order1));
    let accepted = OrderEventAny::Accepted(order_accepted(
        &order1,
        Some(venue_order_id),
        Some(account_id),
    ));
    let filled = OrderEventAny::Filled(order_filled(
        &order1,
        &instrument_audusd,
        None,
        Some(account_id),
        Some(venue_order_id),
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
    exec_engine.process(&submitted);
    exec_engine.process(&accepted);
    exec_engine.process(&filled);

    let submit_order2 = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order2.client_order_id(),
        order2.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let venue_order_id2 = VenueOrderId::new("002");
    risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
    exec_engine.process(&OrderEventAny::Submitted(order_submitted(&order2)));
    exec_engine.process(&OrderEventAny::Accepted(order_accepted(
        &order2,
        Some(venue_order_id2),
        Some(account_id),
    )));
    exec_engine.process(&OrderEventAny::Filled(order_filled(
        &order2,
        &instrument_audusd,
        None,
        Some(account_id),
        Some(venue_order_id2),
        None,
        None,
        None,
        None,
        None,
        None,
    )));

    // TODO
    // assert_eq!(order1.status(), OrderStatus::Filled);
    // assert_eq!(order2.status(), OrderStatus::Denied);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 2);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_audusd.id()
    );
}

#[rstest]
fn test_submit_order_reduce_only_order_with_custom_position_id_not_open_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::new(100.0, 0))
        .quantity(Quantity::from("1000"))
        .reduce_only(true)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        Some(PositionId::new("CUSTOM-001")), // <-- Custom position ID
        None,                                // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from("Position CUSTOM-001 not found for reduce-only order")
    );
}

#[rstest]
fn test_submit_order_when_instrument_not_in_cache_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::new(100.0, 0))
        .quantity(Quantity::from("1000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from("Instrument for AUD/USD.SIM not found")
    );
}

#[rstest]
fn test_submit_order_when_invalid_price_precision_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::from_raw(1_000_000_000_000, FIXED_PRECISION)) // <- Invalid price
        .quantity(Quantity::from("1000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert!(
        saved_process_messages
            .first()
            .unwrap()
            .message()
            .unwrap()
            .contains(&format!("invalid (precision {FIXED_PRECISION} > 5)"))
    );
}

#[rstest]
fn test_submit_order_when_invalid_negative_price_and_not_option_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::new(-0.1, 1)) // <- Invalid price (negative)
        .quantity(Quantity::from("1000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from("price -0.1 invalid (<= 0)")
    );
}

#[rstest]
fn test_submit_order_when_negative_price_for_futures_spread_then_allows(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_futures_spread: InstrumentAny,
    _venue_order_id: VenueOrderId,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_futures_spread.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_futures_spread.id())
        .side(OrderSide::Buy)
        .price(Price::new(-17.0, 2)) // Negative price is valid for spreads
        .quantity(Quantity::from("1"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_futures_spread.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_futures_spread.id()
    );
}

#[rstest]
fn test_submit_order_when_negative_price_for_option_spread_then_allows(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_option_spread: InstrumentAny,
    _venue_order_id: VenueOrderId,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_option_spread.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_option_spread.id())
        .side(OrderSide::Buy)
        .price(Price::new(-2.50, 2)) // Negative price -2.50 is valid for spreads
        .quantity(Quantity::from("1"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_option_spread.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_option_spread.id()
    );
}

#[rstest]
fn test_submit_order_when_invalid_trigger_price_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::StopLimit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("1000").unwrap())
        .price(Price::new(0.1, 1))
        .trigger_price(Price::from_raw(1_000_000_000_000_000, FIXED_PRECISION)) // <- Invalid price
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    // assert!(saved_process_messages
    //     .first()
    //     .unwrap()
    //     .message()
    //     .unwrap()
    //     .contains(&format!("invalid (precision {PRECISION})")));
}

#[rstest]
fn test_submit_order_when_invalid_quantity_precision_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("0.1").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from("quantity 0.1 invalid (precision 1 > 0)")
    );
}

#[rstest]
fn test_submit_order_when_invalid_quantity_exceeds_maximum_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100000000").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from("quantity 100000000 invalid (> maximum trade size of 1000000)")
    );
}

#[rstest]
fn test_submit_order_when_invalid_quantity_less_than_minimum_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("1").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from("quantity 1 invalid (< minimum trade size of 100)")
    );
}

#[rstest]
fn test_submit_order_when_market_order_and_no_market_then_logs_warning(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i32(10000000).unwrap());

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_audusd.id()
    );
}

#[rstest]
fn test_submit_order_when_less_than_min_notional_for_instrument_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_xbtusd_with_high_size_precision: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_xbtusd_with_high_size_precision.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            bitmex_cash_account_state_multi,
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_xbtusd_with_high_size_precision.id(),
        Price::from("0.075000"),
        Price::from("0.075005"),
        Quantity::from("50000"),
        Quantity::from("50000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_with_high_size_precision.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("0.9").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_xbtusd_with_high_size_precision.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from(
            "NOTIONAL_LESS_THAN_MIN_FOR_INSTRUMENT: min_notional=Money(1.00, USD), notional=Money(0.90, USD)"
        )
    );
}

#[rstest]
fn test_submit_order_when_greater_than_max_notional_for_instrument_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_xbtusd_bitmex.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            bitmex_cash_account_state_multi,
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_xbtusd_bitmex.id(),
        Price::from("7.5000"),
        Price::from("7.5005"),
        Quantity::from("50000"),
        Quantity::from("50000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine.set_max_notional_per_order(
        instrument_xbtusd_bitmex.id(),
        Decimal::from_i64(100000000).unwrap(),
    );

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("10000001").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from(
            "NOTIONAL_GREATER_THAN_MAX_FOR_INSTRUMENT: max_notional=Money(10000000.00, USD), notional=Money(10000001.00, USD)"
        )
    );
}

#[rstest]
fn test_submit_order_when_buy_market_order_and_over_max_notional_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_audusd.id(),
        Price::from("0.75000"),
        Price::from("0.75005"),
        Quantity::from("500000"),
        Quantity::from("500000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("1000000").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from(
            "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional=Money(100000.00, USD), notional=Money(750050.00, USD)"
        )
    );
}

#[rstest]
fn test_submit_order_when_sell_market_order_and_over_max_notional_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_audusd.id(),
        Price::from("0.75000"),
        Price::from("0.75005"),
        Quantity::from("500000"),
        Quantity::from("500000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("1000000").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from(
            "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional=Money(100000.00, USD), notional=Money(750000.00, USD)"
        )
    );
}

#[rstest]
fn test_submit_order_when_market_order_and_over_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100000").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from(
            "NOTIONAL_EXCEEDS_FREE_BALANCE: free=Money(1000000.00, USD), notional=Money(10100000.00, USD)"
        )
    );
}

#[rstest]
fn test_submit_order_when_market_order_over_free_balance_with_borrowing_enabled_then_accepts(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    // Test that orders exceeding free balance are accepted when borrowing is enabled
    // (e.g. spot margin trading on Bybit)

    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    let cash_account_with_borrowing = CashAccount::new(cash_account_state_million_usd, true, true);
    simple_cache
        .add_account(AccountAny::Cash(cash_account_with_borrowing))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Create order that would exceed free balance (same as denied test above)
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100000").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // Should NOT be denied because borrowing is enabled
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert!(
        saved_process_messages.is_empty(),
        "Order should not be denied when borrowing is enabled, but got: {saved_process_messages:?}"
    );
}

#[rstest]
fn test_submit_order_list_buys_when_over_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("4920").unwrap())
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("5653").unwrap()) // <--- over free balance
        .build();

    simple_cache
        .add_order(order1.clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(order2.clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let orders = [order1, order2];
    let order_list = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![orders[0].client_order_id(), orders[1].client_order_id()],
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_order = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);

    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
    }

    // The actual reason is in the first denial; the rest will show `OrderListID` as Denied.
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from(
            "CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, cum_notional=1067873.00 USD"
        )
    );
}

#[rstest]
fn test_submit_order_list_sells_when_over_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-001"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("4920").unwrap())
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-002"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("5653").unwrap()) // <--- over free balance
        .build();

    let orders = [order1, order2];

    simple_cache
        .add_order(orders[0].clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(orders[1].clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order_list = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![orders[0].client_order_id(), orders[1].client_order_id()],
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_order = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);

    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
    }

    // Correct reason is in First deny, rest will show OrderList`ID` Denied.
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from(
            "CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, cum_notional=1057300.00 USD"
        )
    );
}

#[rstest]
fn test_submit_order_when_trading_halted_then_denies_order(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order.instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.set_trading_state(TradingState::Halted);

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // Get messages and test
    let saved_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Denied);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("TradingState::HALTED")
    );
}

#[rstest]
fn test_submit_order_beyond_rate_limit_then_denies_order(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    for i in 0..11 {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .client_order_id(ClientOrderId::new(format!("O-{i}")))
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        risk_engine
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id_binance), false)
            .unwrap();

        let submit_order = SubmitOrder::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            None, // params
            UUID4::new(),
            risk_engine.clock().borrow().timestamp_ns(),
        );

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    }

    assert_eq!(risk_engine.throttled_submit.used(), 1.0);

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    let first_message = saved_process_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Denied);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("REJECTED BY THROTTLER")
    );
}

#[rstest]
fn test_submit_order_list_when_trading_halted_then_denies_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(0.1, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-003"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::new(0.11, 2))
        .build();

    let orders = [entry, stop_loss, take_profit];

    simple_cache
        .add_order(orders[0].clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(orders[1].clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(orders[2].clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![
            orders[0].client_order_id(),
            orders[1].client_order_id(),
            orders[2].client_order_id(),
        ],
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_bracket = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        bracket,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(event.message().unwrap(), Ustr::from("TradingState::HALTED"));
    }
}

// Test that order lists with BUY orders are denied when in REDUCING state and already LONG.
//
// This test verifies the risk engine correctly prevents adding to existing positions
// when the trading state is set to REDUCING (position reduction mode only).
//
// TODO: Complete implementation - similar to single order reducing tests but for order lists.
// The test logic needs to properly track portfolio position state through message bus updates.
#[ignore = "Under development - requires portfolio state tracking integration"]
#[rstest]
fn test_submit_order_list_buys_when_trading_reducing_then_denies_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_xbtusd_bitmex.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            bitmex_cash_account_state_multi,
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_xbtusd_bitmex.id(),
        Price::from("0.075000"),
        Price::from("0.075005"),
        Quantity::from("50000"),
        Quantity::from("50000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    risk_engine.set_max_notional_per_order(instrument_xbtusd_bitmex.id(), dec!(10000));

    let long = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(long.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        long.client_order_id(),
        long.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    risk_engine.set_trading_state(TradingState::Reducing);

    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(1.1, 1))
        .build();

    // TODO: attempt to add with overflow
    // let take_profit = OrderTestBuilder::new(OrderType::Limit)
    //     .instrument_id(instrument_xbtusd_bitmex.id())
    //     .side(OrderSide::Buy)
    //     .quantity(Quantity::from_str("100").unwrap())
    //     .price(Price::new(1.2, 1))
    //     .build();

    let orders = [entry, stop_loss];

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(orders[0].clone(), None, Some(client_id_binance), true)
        .unwrap();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(orders[1].clone(), None, Some(client_id_binance), true)
        .unwrap();

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_xbtusd_bitmex.id(),
        StrategyId::new("S-001"),
        vec![orders[0].client_order_id(), orders[1].client_order_id()],
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_order_list = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        bracket,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

// Test that order lists with SELL orders are denied when in REDUCING state and already SHORT.
//
// This test verifies the risk engine correctly prevents adding to existing short positions
// when the trading state is set to REDUCING (position reduction mode only).
//
// TODO: Re-enable after high-precision decimal work is merged and stable.
// The test may have precision-related issues with position calculations.
#[ignore = "Waiting on high-precision decimal merge"]
#[rstest]
fn test_submit_order_list_sells_when_trading_reducing_then_denies_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_xbtusd_bitmex.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            bitmex_cash_account_state_multi,
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_xbtusd_bitmex.id(),
        Price::from("0.075000"),
        Price::from("0.075005"),
        Quantity::from("50000"),
        Quantity::from("50000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    risk_engine.set_max_notional_per_order(instrument_xbtusd_bitmex.id(), dec!(10000));

    let short = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(short.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        short.client_order_id(),
        short.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    risk_engine.set_trading_state(TradingState::Reducing);

    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(1.1, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::new(1.2, 1))
        .build();

    let orders = [entry, stop_loss, take_profit];

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(orders[0].clone(), None, Some(client_id_binance), true)
        .unwrap();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(orders[1].clone(), None, Some(client_id_binance), true)
        .unwrap();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(orders[2].clone(), None, Some(client_id_binance), true)
        .unwrap();

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_xbtusd_bitmex.id(),
        StrategyId::new("S-001"),
        vec![
            orders[0].client_order_id(),
            orders[1].client_order_id(),
            orders[2].client_order_id(),
        ],
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_order_list = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        bracket,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

// SUBMIT BRACKET ORDER TESTS

// Verify bracket orders with emulated orders are sent to emulator.
//
// This test requires the order emulator component to be implemented. The emulator
// handles client-side order management for conditional orders (stop-loss, take-profit, etc.)
// that need to be triggered locally before being sent to the venue.
//
// TODO: Re-enable once the emulator component is integrated with the risk engine.
// Dependencies: Order emulation infrastructure in execution engine
#[ignore = "Waiting on emulator implementation"]
#[rstest]
fn test_submit_bracket_with_emulated_orders_sends_to_emulator() {}

#[rstest]
fn test_submit_bracket_order_when_instrument_not_in_cache_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(0.1, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-003"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::new(0.1001, 4))
        .build();

    let orders = [entry, stop_loss, take_profit];

    // Add orders to cache (but NOT the instrument - testing instrument not found case)
    simple_cache
        .add_order(orders[0].clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(orders[1].clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(orders[2].clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![
            orders[0].client_order_id(),
            orders[1].client_order_id(),
            orders[2].client_order_id(),
        ],
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_bracket = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        bracket,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(
            event.message().unwrap(),
            Ustr::from("no instrument found for AUD/USD.SIM")
        );
    }
}

// Verify that orders marked for emulation are correctly routed to the emulator.
//
// This test should verify that when an order is submitted with emulation flags,
// the risk engine routes it to the order emulator rather than directly to execution.
//
// TODO: Re-enable once the emulator component is integrated with the risk engine.
// Dependencies: Order emulation infrastructure in execution engine
#[ignore = "Waiting on emulator implementation"]
#[rstest]
fn test_submit_order_for_emulation_sends_command_to_emulator() {}

// MODIFY ORDER TESTS
#[rstest]
fn test_modify_order_when_no_order_found_logs_error(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let modify_order = ModifyOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        Some(venue_order_id),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);
}

#[rstest]
fn test_modify_order_beyond_rate_limit_then_rejects(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(1.0001, 4))
        .build();

    simple_cache
        .add_order(order, None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    for i in 0..11 {
        let modify_order = ModifyOrder::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            Some(venue_order_id),
            Some(Quantity::from_str("100").unwrap()),
            Some(Price::new(1.00011 + (i as f64) * 0.00001, 5)),
            None,
            UUID4::new(),
            risk_engine.clock().borrow().timestamp_ns(),
            None,
        );

        risk_engine.execute(TradingCommand::ModifyOrder(modify_order));
    }

    assert_eq!(risk_engine.throttled_modify_order.used(), 1.0);

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 6);
    let first_message = saved_process_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::ModifyRejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Exceeded MAX_ORDER_MODIFY_RATE")
    );
}

#[rstest]
fn test_modify_order_with_default_settings_then_sends_to_client(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(1.0001, 4))
        .build();

    simple_cache
        .add_order(order.clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let modify_order = ModifyOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from_str("100").unwrap()),
        Some(Price::new(1.00011, 5)),
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 2);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_audusd.id()
    );
}

// Verify that modifications to emulated orders are routed to the emulator.
//
// This test should verify that when modifying an order that's being managed by
// the emulator, the modification command is sent to the emulator rather than
// directly to the venue.
//
// TODO: Re-enable once the emulator component is integrated with the risk engine.
// Dependencies: Order emulation infrastructure in execution engine
#[ignore = "Waiting on emulator implementation"]
#[rstest]
fn test_modify_order_for_emulated_order_then_sends_to_emulator() {}

#[rstest]
fn test_submit_order_when_betting_back_order_liability_within_free_balance_then_accepts(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    let gbp = Currency::GBP();
    let instrument = InstrumentAny::Betting(betting());
    let account_state = AccountState::new(
        AccountId::new("BETFAIR-001"),
        AccountType::Betting,
        vec![AccountBalance::new(
            Money::new(1_000.0, gbp),
            Money::new(0.0, gbp),
            Money::new(1_000.0, gbp),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(gbp),
    );

    simple_cache.add_instrument(instrument.clone()).unwrap();

    simple_cache
        .add_account(AccountAny::Betting(BettingAccount::new(
            account_state,
            true,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("1.25"))
        .quantity(Quantity::from_str("1000").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert!(saved_process_messages.is_empty());
}

#[rstest]
fn test_submit_order_when_betting_back_order_liability_exceeds_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    let gbp = Currency::GBP();
    let instrument = InstrumentAny::Betting(betting());
    let account_state = AccountState::new(
        AccountId::new("BETFAIR-002"),
        AccountType::Betting,
        vec![AccountBalance::new(
            Money::new(999.0, gbp),
            Money::new(0.0, gbp),
            Money::new(999.0, gbp),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(gbp),
    );

    simple_cache.add_instrument(instrument.clone()).unwrap();
    simple_cache
        .add_account(AccountAny::Betting(BettingAccount::new(
            account_state,
            true,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("10.0"))
        .quantity(Quantity::from_str("1000").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert!(
        saved_process_messages
            .first()
            .unwrap()
            .message()
            .unwrap()
            .as_str()
            .contains("NOTIONAL_EXCEEDS_FREE_BALANCE")
    );
}

#[rstest]
fn test_submit_order_when_betting_sell_reduces_long_position_then_accepts(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    let gbp = Currency::GBP();
    let instrument = InstrumentAny::Betting(betting());

    // Account with only 10 GBP free (not enough for a new bet)
    let account_state = AccountState::new(
        AccountId::new("BETFAIR-001"),
        AccountType::Betting,
        vec![AccountBalance::new(
            Money::new(10.0, gbp),
            Money::new(0.0, gbp),
            Money::new(10.0, gbp),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(gbp),
    );

    simple_cache.add_instrument(instrument.clone()).unwrap();
    let betting_account = BettingAccount::new(account_state, true);
    simple_cache
        .add_account(AccountAny::Betting(betting_account.clone()))
        .unwrap();

    // Create a long position via a filled Buy order
    let entry_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();

    let mut fill = order_filled(
        &entry_order,
        &instrument,
        None,
        Some(AccountId::new("BETFAIR-001")),
        Some(VenueOrderId::from("V-001")),
        None,
        None,
        Some(Price::from("2.0")),
        None,
        Some(AccountAny::Betting(betting_account)),
        None,
    );
    fill.position_id = Some(PositionId::from("P-001"));
    let position = Position::new(&instrument, fill);
    assert_eq!(position.side, PositionSide::Long);

    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Sell 50 to reduce the 100-qty long position (position-reducing, skips balance check)
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Sell)
        .price(Price::from("2.5"))
        .quantity(Quantity::from("50"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    // Position-reducing sell should NOT be denied despite low free balance
    assert!(saved_process_messages.is_empty());
}

#[rstest]
fn test_submit_order_for_less_than_max_cum_transaction_value_adausdt_with_crypto_cash_account(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    let quote = QuoteTick::new(
        instrument_xbtusd_bitmex.id(),
        Price::from("0.6109"),
        Price::from("0.6110"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache
        .add_instrument(instrument_xbtusd_bitmex.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            bitmex_cash_account_state_multi,
        )))
        .unwrap();

    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("440").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_xbtusd_bitmex.id()
    );
}

// Verify that account balances are correctly updated with partial and full order fills.
//
// This test should verify that when orders are partially or fully filled, the
// account balance tracking reflects the correct values including:
// - Reserved margin/capital being released
// - Commission being deducted
// - Realized P&L being applied to account balance
//
// TODO: Re-enable once real-time account balance tracking is implemented.
// Dependencies: Account balance tracking in portfolio/risk engine integration
// Related: Real-time position valuation and margin calculations
#[ignore = "Waiting on account balance tracking implementation"]
#[rstest]
fn test_partial_fill_and_full_fill_account_balance_correct() {}

#[rstest]
fn test_submit_order_with_gtd_expire_time_already_passed(
    clock: TestClock,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    let quote = QuoteTick::new(
        instrument_xbtusd_bitmex.id(),
        Price::from("0.6109"),
        Price::from("0.6110"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    simple_cache
        .add_instrument(instrument_xbtusd_bitmex.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            bitmex_cash_account_state_multi,
        )))
        .unwrap();

    simple_cache.add_quote(quote).unwrap();

    let cache = Rc::new(RefCell::new(simple_cache));

    let mut risk_engine = get_risk_engine(Some(cache), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .price(Price::from("100_000.0"))
        .quantity(Quantity::from_str("440").unwrap())
        .time_in_force(TimeInForce::Gtd)
        .expire_time(UnixNanos::from(1_000)) // <-- Set expire time in the past
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        clock.timestamp_ns(),
    );

    clock.set_time(UnixNanos::from(2_000)); // <-- Set time to 2,000 nanos past epoch

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // TODO: Change command messages to not require owned orders
}

#[rstest]
fn test_submit_order_with_quote_quantity_skips_min_max_quantity_check(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    // Create a BTCUSDT spot instrument with max_quantity = 83 BTC
    let btc_usdt = InstrumentAny::CurrencyPair(CurrencyPair::new(
        InstrumentId::from("BTCUSDT-SPOT.BYBIT"),
        Symbol::from("BTCUSDT"),
        Currency::BTC(),
        Currency::USDT(),
        1,
        6,
        Price::from("0.1"),
        Quantity::from("0.000001"),
        Some(Quantity::from("1")),         // multiplier
        Some(Quantity::from("0.000001")),  // lot_size
        Some(Quantity::from("83")),        // max_quantity = 83 BTC
        Some(Quantity::from("0.000011")),  // min_quantity
        Some(Money::from("8000000 USDT")), // max_notional
        Some(Money::from("5 USDT")),       // min_notional
        None,
        None,
        Some(dec!(0.1)),      // margin_init
        Some(dec!(0.1)),      // margin_maint
        Some(dec!(-0.00005)), // maker_fee
        Some(dec!(0.00015)),  // taker_fee
        None,                 // info
        UnixNanos::default(),
        UnixNanos::default(),
    ));

    simple_cache.add_instrument(btc_usdt.clone()).unwrap();

    // Create a cash account with USDT balance (not USD) to match the instrument
    let usdt_account_state = AccountState::new(
        AccountId::from("BYBIT-001"), // Match the venue from the instrument
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000000 USDT"),
            Money::from("0 USDT"),
            Money::from("1000000 USDT"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        Some(Currency::USDT()),
    );

    simple_cache
        .add_account(AccountAny::Cash(cash_account(usdt_account_state)))
        .unwrap();

    // Add a quote tick at $100,000 per BTC
    // This means 100 USDT quote quantity = 0.001 BTC base quantity
    let quote = QuoteTick::new(
        btc_usdt.id(),
        Price::from("100000.0"), // ask
        Price::from("99999.9"),  // bid
        Quantity::from("1.0"),   // ask_size
        Quantity::from("1.0"),   // bid_size
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Create a market order with quote_quantity = 100 USDT
    // This should convert to 0.001 BTC which is well below max_quantity of 83 BTC
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(btc_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100")) // 100 USDT
        .quote_quantity(true)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        btc_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // The order should be accepted (not denied)
    // If the bug exists, it would compare 100 > 83 and deny the order
    // With the fix, it converts 100 USDT -> 0.001 BTC, then checks 0.001 < 83 (passes)
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);

    // Should have 1 event (submitted to exec engine, not denied)
    assert_eq!(
        saved_process_messages.len(),
        0,
        "Order should not be denied"
    );
}

#[rstest]
fn test_submit_order_with_quote_quantity_does_not_deny_on_base_max_quantity(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    // Base-quantity bounds do not apply to quote-denominated orders, so a
    // converted base quantity that would exceed `max_quantity` must still pass.
    let btc_usdt = InstrumentAny::CurrencyPair(CurrencyPair::new(
        InstrumentId::from("BTCUSDT-SPOT.BYBIT"),
        Symbol::from("BTCUSDT"),
        Currency::BTC(),
        Currency::USDT(),
        1,
        6,
        Price::from("0.1"),
        Quantity::from("0.000001"),
        Some(Quantity::from("1")),        // multiplier
        Some(Quantity::from("0.000001")), // lot_size
        Some(Quantity::from("0.5")),      // max_quantity = 0.5 BTC
        Some(Quantity::from("0.000011")), // min_quantity
        Some(Money::from("8000000 USDT")),
        Some(Money::from("5 USDT")),
        None,
        None,
        Some(dec!(0.1)),
        Some(dec!(0.1)),
        Some(dec!(-0.00005)),
        Some(dec!(0.00015)),
        None, // info
        UnixNanos::default(),
        UnixNanos::default(),
    ));

    simple_cache.add_instrument(btc_usdt.clone()).unwrap();

    let usdt_account_state = AccountState::new(
        AccountId::from("BYBIT-001"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000000 USDT"),
            Money::from("0 USDT"),
            Money::from("1000000 USDT"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        Some(Currency::USDT()),
    );

    simple_cache
        .add_account(AccountAny::Cash(cash_account(usdt_account_state)))
        .unwrap();

    // Quote at $100k/BTC: 100,000 USDT would convert to 1 BTC > max 0.5 BTC.
    let quote = QuoteTick::new(
        btc_usdt.id(),
        Price::from("100000.0"),
        Price::from("99999.9"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(btc_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .quote_quantity(true)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        btc_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(
        saved_process_messages.len(),
        0,
        "Order should not be denied for quote-quantity base bounds"
    );
}

#[rstest]
fn test_submit_order_with_quote_quantity_does_not_deny_on_base_min_quantity(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    // Mirrors the Polymarket scenario from #3874: a quote-denominated order whose
    // converted base quantity falls below a large `min_quantity` must still pass.
    let btc_usdt = InstrumentAny::CurrencyPair(CurrencyPair::new(
        InstrumentId::from("BTCUSDT-SPOT.BYBIT"),
        Symbol::from("BTCUSDT"),
        Currency::BTC(),
        Currency::USDT(),
        1,
        6,
        Price::from("0.1"),
        Quantity::from("0.000001"),
        Some(Quantity::from("1")),
        Some(Quantity::from("0.000001")),
        None,                      // max_quantity
        Some(Quantity::from("5")), // min_quantity = 5 base units
        None,                      // max_notional
        Some(Money::from("1 USDT")),
        None,
        None,
        Some(dec!(0.1)),
        Some(dec!(0.1)),
        Some(dec!(-0.00005)),
        Some(dec!(0.00015)),
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ));

    simple_cache.add_instrument(btc_usdt.clone()).unwrap();

    let usdt_account_state = AccountState::new(
        AccountId::from("BYBIT-001"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000000 USDT"),
            Money::from("0 USDT"),
            Money::from("1000000 USDT"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        Some(Currency::USDT()),
    );

    simple_cache
        .add_account(AccountAny::Cash(cash_account(usdt_account_state)))
        .unwrap();

    // Quote at $100k/BTC: 10 USDT -> 0.0001 BTC, well below min_quantity of 5.
    let quote = QuoteTick::new(
        btc_usdt.id(),
        Price::from("100000.0"),
        Price::from("99999.9"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(btc_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10"))
        .quote_quantity(true)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        btc_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(
        saved_process_messages.len(),
        0,
        "Order should not be denied for quote-quantity base bounds"
    );
}

#[rstest]
fn test_submit_order_with_quote_quantity_still_enforces_min_notional(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    // Base-quantity bounds are skipped for quote-denominated orders, but
    // `min_notional` still applies and must deny sub-minimum notionals.
    let btc_usdt = InstrumentAny::CurrencyPair(CurrencyPair::new(
        InstrumentId::from("BTCUSDT-SPOT.BYBIT"),
        Symbol::from("BTCUSDT"),
        Currency::BTC(),
        Currency::USDT(),
        1,
        6,
        Price::from("0.1"),
        Quantity::from("0.000001"),
        Some(Quantity::from("1")),
        Some(Quantity::from("0.000001")),
        None, // max_quantity
        None, // min_quantity
        None, // max_notional
        Some(Money::from("10 USDT")),
        None,
        None,
        Some(dec!(0.1)),
        Some(dec!(0.1)),
        Some(dec!(-0.00005)),
        Some(dec!(0.00015)),
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ));

    simple_cache.add_instrument(btc_usdt.clone()).unwrap();

    let usdt_account_state = AccountState::new(
        AccountId::from("BYBIT-001"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000000 USDT"),
            Money::from("0 USDT"),
            Money::from("1000000 USDT"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        Some(Currency::USDT()),
    );

    simple_cache
        .add_account(AccountAny::Cash(cash_account(usdt_account_state)))
        .unwrap();

    let quote = QuoteTick::new(
        btc_usdt.id(),
        Price::from("100000.0"),
        Price::from("99999.9"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // 1 USDT quote quantity, below the 10 USDT minimum notional.
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(btc_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .quote_quantity(true)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        btc_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert!(
        saved_process_messages
            .first()
            .unwrap()
            .message()
            .unwrap()
            .contains("NOTIONAL_LESS_THAN_MIN_FOR_INSTRUMENT")
    );
}

#[rstest]
fn test_submit_order_list_beyond_rate_limit_then_denies_all_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    // Rate limit of 10 submissions per interval
    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Submit 10 order lists to fill the rate limit
    for i in 0..10 {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .client_order_id(ClientOrderId::new(format!("O-{i}")))
            .side(OrderSide::Buy)
            .price(Price::new(1.0, 0))
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        risk_engine
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id_binance), false)
            .unwrap();

        let order_list = OrderList::new(
            OrderListId::new(format!("OL-{i}")),
            instrument_audusd.id(),
            strategy_id_ema_cross,
            vec![order.client_order_id()],
            risk_engine.clock().borrow().timestamp_ns(),
        );

        let submit_order_list = SubmitOrderList::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            order_list,
            vec![order.init_event().clone()],
            None,
            None,
            None,
            UUID4::new(),
            risk_engine.clock().borrow().timestamp_ns(),
        );

        risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));
    }

    // The 11th order list should be throttled
    let throttled_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::new("O-THROTTLED"))
        .side(OrderSide::Buy)
        .price(Price::new(1.0, 0))
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(
            throttled_order.clone(),
            None,
            Some(client_id_binance),
            false,
        )
        .unwrap();

    let throttled_list = OrderList::new(
        OrderListId::new("OL-THROTTLED"),
        instrument_audusd.id(),
        strategy_id_ema_cross,
        vec![throttled_order.client_order_id()],
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_throttled = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        throttled_list,
        vec![throttled_order.init_event().clone()],
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_throttled));

    assert_eq!(risk_engine.throttled_submit.used(), 1.0);

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    let first_message = saved_process_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Denied);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("REJECTED BY THROTTLER")
    );
}

#[rstest]
fn test_submit_order_list_beyond_rate_limit_denies_all_orders_in_list(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Fill rate limit with 10 single-order lists
    for i in 0..10 {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .client_order_id(ClientOrderId::new(format!("O-{i}")))
            .side(OrderSide::Buy)
            .price(Price::new(1.0, 0))
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        risk_engine
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id_binance), false)
            .unwrap();

        let order_list = OrderList::new(
            OrderListId::new(format!("OL-{i}")),
            instrument_audusd.id(),
            strategy_id_ema_cross,
            vec![order.client_order_id()],
            risk_engine.clock().borrow().timestamp_ns(),
        );

        let submit = SubmitOrderList::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            order_list,
            vec![order.init_event().clone()],
            None,
            None,
            None,
            UUID4::new(),
            risk_engine.clock().borrow().timestamp_ns(),
        );

        risk_engine.execute(TradingCommand::SubmitOrderList(submit));
    }

    // Submit a bracket (3 orders) beyond the limit
    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-ENTRY"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-SL"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(0.9, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-TP"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::new(1.1, 1))
        .build();

    let orders = [entry, stop_loss, take_profit];
    for order in &orders {
        risk_engine
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let bracket = OrderList::new(
        OrderListId::new("OL-BRACKET"),
        instrument_audusd.id(),
        strategy_id_ema_cross,
        orders.iter().map(|o| o.client_order_id()).collect(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit_bracket = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        bracket,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // All 3 orders in the bracket should be denied
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(
            event.message().unwrap(),
            Ustr::from("REJECTED BY THROTTLER")
        );
    }
}

#[rstest]
fn test_set_trading_state_publishes_trading_state_changed_event() {
    let config = RiskEngineConfig {
        debug: true,
        bypass: false,
        max_order_submit: RateLimit::new(100, 1_000_000_000),
        max_order_modify: RateLimit::new(50, 1_000_000_000),
        max_notional_per_order: AHashMap::new(),
    };

    let mut risk_engine = get_risk_engine(None, Some(config), None, false);
    risk_engine.set_max_notional_per_order(
        InstrumentId::from("AUD/USD.SIM"),
        Decimal::from_i64(500000).unwrap(),
    );

    let handler = msgbus::stubs::get_message_saving_handler::<TradingStateChanged>(None);
    msgbus::subscribe_any("events.risk".into(), handler.clone(), None);

    risk_engine.set_trading_state(TradingState::Halted);

    let events = msgbus::stubs::get_saved_messages::<TradingStateChanged>(&handler);
    assert_eq!(events.len(), 1);

    let event = &events[0];
    assert_eq!(event.state, TradingState::Halted);
    assert_eq!(event.config["bypass"], "false");
    assert_eq!(event.config["max_order_submit_rate"], "100/00:00:01");
    assert_eq!(event.config["max_order_modify_rate"], "50/00:00:01");
    assert_eq!(event.config["debug"], "true");
    assert_eq!(event.config["max_notional_per_order.AUD/USD.SIM"], "500000");
}

#[rstest]
fn test_set_trading_state_from_halted_to_reducing() {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine.set_trading_state(TradingState::Halted);
    assert_eq!(risk_engine.trading_state(), TradingState::Halted);

    risk_engine.set_trading_state(TradingState::Reducing);
    assert_eq!(risk_engine.trading_state(), TradingState::Reducing);
}

#[rstest]
fn test_set_trading_state_from_reducing_to_active() {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine.set_trading_state(TradingState::Reducing);
    assert_eq!(risk_engine.trading_state(), TradingState::Reducing);

    risk_engine.set_trading_state(TradingState::Active);
    assert_eq!(risk_engine.trading_state(), TradingState::Active);
}

#[rstest]
fn test_reset_restores_trading_state_and_config_notionals() {
    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let config_notional = Decimal::from_i64(50000).unwrap();

    let mut config_notionals = AHashMap::new();
    config_notionals.insert(instrument_id, config_notional);

    let config = RiskEngineConfig {
        debug: true,
        bypass: false,
        max_order_submit: RateLimit::new(10, 1000),
        max_order_modify: RateLimit::new(5, 1000),
        max_notional_per_order: config_notionals,
    };

    let mut risk_engine = get_risk_engine(None, Some(config), None, false);

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.set_max_notional_per_order(instrument_id, Decimal::from_i64(100000).unwrap());

    risk_engine.reset();

    assert_eq!(risk_engine.trading_state(), TradingState::Active);
    assert_eq!(
        risk_engine.max_notional_per_order().get(&instrument_id),
        Some(&config_notional),
    );
}

#[rstest]
fn test_submit_order_list_within_rate_limit_passes_through(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let entry = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-001"))
        .side(OrderSide::Buy)
        .price(Price::new(1.0, 0))
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-002"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(0.9, 1))
        .build();

    let orders = [entry, stop_loss];
    for order in &orders {
        risk_engine
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let order_list = OrderList::new(
        OrderListId::new("OL-001"),
        instrument_audusd.id(),
        strategy_id_ema_cross,
        orders.iter().map(|o| o.client_order_id()).collect(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    // No orders should be denied
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);

    // Order list should pass through to execution
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

fn margin_account_with_usdt_balance(total: &str, locked: &str, free: &str) -> MarginAccount {
    let state = AccountState::new(
        AccountId::from("BINANCE-001"),
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from(total),
            Money::from(locked),
            Money::from(free),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        Some(Currency::USDT()),
    );
    MarginAccount::new(state, true)
}

#[rstest]
fn test_submit_order_margin_account_buy_within_free_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // ETHUSDT margin_init=1.0, 10x leverage: margin = notional / 10
    // Buy 1 ETH @ $3000 -> notional = $3000 -> margin = $300
    let mut margin_acct = margin_account_with_usdt_balance("100000 USDT", "0 USDT", "100000 USDT");
    margin_acct.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_acct))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0); // No denial

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1); // Passed through
}

#[rstest]
fn test_submit_order_margin_account_buy_exceeds_free_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // ETHUSDT margin_init=1.0, 10x leverage: margin = notional / 10
    // Buy 100 ETH @ $3000 -> notional = $300,000 -> margin = $30,000
    // Free balance = $20,000 -> denied
    let mut margin_acct = margin_account_with_usdt_balance("20000 USDT", "0 USDT", "20000 USDT");
    margin_acct.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_acct))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert!(matches!(
        saved_process_messages[0].event_type(),
        OrderEventType::Denied
    ));
}

#[rstest]
fn test_submit_order_margin_account_sell_short_exceeds_free_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // Sell 100 ETH @ $3000 -> notional = $300,000 -> margin = $30,000
    // Free balance = $20,000 -> denied
    let mut margin_acct = margin_account_with_usdt_balance("20000 USDT", "0 USDT", "20000 USDT");
    margin_acct.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_acct))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("100.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert!(matches!(
        saved_process_messages[0].event_type(),
        OrderEventType::Denied
    ));
}

#[rstest]
fn test_submit_order_margin_account_position_reducing_sell_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // Account with minimal free balance (can't afford new margin)
    let mut margin_acct = margin_account_with_usdt_balance("100 USDT", "0 USDT", "100 USDT");
    margin_acct.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_acct))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    // Create long position of 10 ETH via a fill
    let entry_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.000"))
        .build();

    let mut fill = order_filled(
        &entry_order,
        &instrument_eth_usdt,
        None,
        Some(AccountId::from("BINANCE-001")),
        Some(VenueOrderId::from("V-001")),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
    );
    fill.position_id = Some(PositionId::from("P-001"));
    let position = Position::new(&instrument_eth_usdt, fill);

    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Sell 5 ETH to reduce position (within 10 ETH long)
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // Position-reducing sell passes despite insufficient free balance for new margin
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[rstest]
fn test_submit_order_margin_account_position_reducing_buy_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // Account with minimal free balance
    let mut margin_acct = margin_account_with_usdt_balance("100 USDT", "0 USDT", "100 USDT");
    margin_acct.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_acct))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    // Create short position of 10 ETH via a sell fill
    let entry_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("10.000"))
        .build();

    let mut fill = order_filled(
        &entry_order,
        &instrument_eth_usdt,
        None,
        Some(AccountId::from("BINANCE-001")),
        Some(VenueOrderId::from("V-001")),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
    );
    fill.position_id = Some(PositionId::from("P-002"));
    let position = Position::new(&instrument_eth_usdt, fill);
    assert_eq!(position.side, PositionSide::Short);

    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Buy 5 ETH to reduce short position (within 10 ETH short)
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("5.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // Position-reducing buy passes despite insufficient free balance for new margin
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[rstest]
fn test_submit_order_list_margin_account_cum_margin_exceeds_free_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // Free = $500 USDT, 10x leverage
    // Each 1 ETH @ $3000 -> margin = $300
    // First order (1 ETH): cum_margin = $300 < $500 -> passes
    // Second order (1 ETH): cum_margin = $600 > $500 -> denied
    let mut margin_acct = margin_account_with_usdt_balance("500 USDT", "0 USDT", "500 USDT");
    margin_acct.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_acct))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .client_order_id(ClientOrderId::from("O-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .client_order_id(ClientOrderId::from("O-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();

    let orders = [order1, order2];
    for order in &orders {
        risk_engine
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let order_list = OrderList::new(
        OrderListId::new("OL-001"),
        instrument_eth_usdt.id(),
        strategy_id_ema_cross,
        orders.iter().map(|o| o.client_order_id()).collect(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    let submit = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        orders.iter().map(|o| o.init_event().clone()).collect(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    // 1 denial from check_orders_risk (2nd order) + 2 from deny_order_list (both orders)
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);
    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
    }
}

#[rstest]
fn test_submit_order_margin_account_limit_order_within_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // Limit buy 1 ETH @ $2500 -> notional = $2500 -> margin = $250 at 10x
    let mut margin_acct = margin_account_with_usdt_balance("1000 USDT", "0 USDT", "1000 USDT");
    margin_acct.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_acct))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("2500.00"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[rstest]
fn test_submit_buy_when_reducing_and_net_long_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    // Create a long position via a filled buy order
    let fill_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();

    let mut fill = order_filled(
        &fill_order,
        &instrument_eth_usdt,
        None,
        Some(AccountId::from("SIM-001")),
        Some(VenueOrderId::from("V-001")),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
    );
    fill.position_id = Some(PositionId::from("P-001"));
    let position = Position::new(&instrument_eth_usdt, fill);
    assert_eq!(position.side, PositionSide::Long);

    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let cache = Rc::new(RefCell::new(simple_cache));
    let mut risk_engine = get_risk_engine(Some(cache), None, None, false);

    risk_engine.portfolio_mut().initialize_positions();
    risk_engine.set_trading_state(TradingState::Reducing);

    // Submit a buy order (increases long exposure) - should be denied
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::Denied
    );
    assert!(
        saved_process_messages[0]
            .message()
            .unwrap()
            .contains("REDUCING")
    );
}

#[rstest]
fn test_submit_sell_when_reducing_and_net_short_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    // Create a short position via a filled sell order
    let fill_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .build();

    let mut fill = order_filled(
        &fill_order,
        &instrument_eth_usdt,
        None,
        Some(AccountId::from("SIM-001")),
        Some(VenueOrderId::from("V-001")),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
    );
    fill.position_id = Some(PositionId::from("P-002"));
    let position = Position::new(&instrument_eth_usdt, fill);
    assert_eq!(position.side, PositionSide::Short);

    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let cache = Rc::new(RefCell::new(simple_cache));
    let mut risk_engine = get_risk_engine(Some(cache), None, None, false);

    risk_engine.portfolio_mut().initialize_positions();
    risk_engine.set_trading_state(TradingState::Reducing);

    // Submit a sell order (increases short exposure) - should be denied
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::Denied
    );
    assert!(
        saved_process_messages[0]
            .message()
            .unwrap()
            .contains("REDUCING")
    );
}

#[rstest]
fn test_submit_sell_when_reducing_and_net_long_then_allows(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    // Create a long position
    let fill_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();

    let mut fill = order_filled(
        &fill_order,
        &instrument_eth_usdt,
        None,
        Some(AccountId::from("SIM-001")),
        Some(VenueOrderId::from("V-001")),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
    );
    fill.position_id = Some(PositionId::from("P-003"));
    let position = Position::new(&instrument_eth_usdt, fill);

    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let cache = Rc::new(RefCell::new(simple_cache));
    let mut risk_engine = get_risk_engine(Some(cache), None, None, false);

    risk_engine.portfolio_mut().initialize_positions();
    risk_engine.set_trading_state(TradingState::Reducing);

    // Submit a sell order (reduces long exposure) - should pass
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[rstest]
fn test_submit_trailing_stop_market_buy_with_trigger_price_then_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Trailing stop buy with trigger_price and BidAsk trigger
    let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .trigger_price(Price::from("3100.00"))
        .trailing_offset(dec!(100))
        .trailing_offset_type(TrailingOffsetType::Price)
        .trigger_type(TriggerType::BidAsk)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[rstest]
fn test_submit_trailing_stop_with_trigger_price_set_then_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Trailing stop with trigger_price already set - skips calculation
    let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_price(Price::from("2900.00"))
        .trailing_offset(dec!(100))
        .trailing_offset_type(TrailingOffsetType::Price)
        .trigger_type(TriggerType::BidAsk)
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[rstest]
fn test_submit_order_with_zero_price_on_non_spread_instrument_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    _venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    _execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    simple_cache.add_quote(quote_audusd()).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Limit order with price = 0 on a CurrencyPair (non-spread) - should be denied
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::from("0.00000"))
        .quantity(Quantity::from("100"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::Denied
    );
    assert!(
        saved_process_messages[0]
            .message()
            .unwrap()
            .contains("<= 0")
    );
}

#[rstest]
fn test_modify_order_when_trading_halted_then_rejects(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    // Create and accept a limit order so it has Accepted status
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .price(Price::from("1.00010"))
        .build();

    order
        .apply(OrderEventAny::Submitted(order_submitted(&order)))
        .unwrap();
    order
        .apply(OrderEventAny::Accepted(order_accepted(
            &order,
            Some(venue_order_id),
            None,
        )))
        .unwrap();

    simple_cache
        .add_order(order, None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    risk_engine.set_trading_state(TradingState::Halted);

    let modify_order = ModifyOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("200")),
        Some(Price::from("1.00020")),
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::ModifyRejected
    );
    assert!(
        saved_process_messages[0]
            .message()
            .unwrap()
            .contains("HALTED")
    );

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 0);
}

#[rstest]
fn test_modify_order_with_invalid_price_precision_then_rejects(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .price(Price::from("1.00010"))
        .build();

    order
        .apply(OrderEventAny::Submitted(order_submitted(&order)))
        .unwrap();
    order
        .apply(OrderEventAny::Accepted(order_accepted(
            &order,
            Some(venue_order_id),
            None,
        )))
        .unwrap();

    simple_cache
        .add_order(order, None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Modify with 6-dp price on a 5-dp instrument
    let modify_order = ModifyOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        Some(venue_order_id),
        None,
        Some(Price::from("1.000001")),
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::ModifyRejected
    );
    assert!(
        saved_process_messages[0]
            .message()
            .unwrap()
            .contains("precision")
    );
}

#[rstest]
fn test_modify_order_with_invalid_quantity_precision_then_rejects(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .price(Price::from("1.00010"))
        .build();

    order
        .apply(OrderEventAny::Submitted(order_submitted(&order)))
        .unwrap();
    order
        .apply(OrderEventAny::Accepted(order_accepted(
            &order,
            Some(venue_order_id),
            None,
        )))
        .unwrap();

    simple_cache
        .add_order(order, None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    // Modify with too-high quantity precision
    let modify_order = ModifyOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("100.1")),
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::ModifyRejected
    );
    assert!(
        saved_process_messages[0]
            .message()
            .unwrap()
            .contains("precision")
    );
}

#[rstest]
fn test_submit_sell_cash_account_with_long_position_reduces_then_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    _client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    _venue_order_id: VenueOrderId,
    _process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // Cash account with small free balance (not enough for a new buy)
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("100 USD", "0 USD", "100 USD"),
        )))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("3000.00"),
        Price::from("3000.01"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    // Create a long position
    let fill_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();

    let mut fill = order_filled(
        &fill_order,
        &instrument_eth_usdt,
        None,
        Some(AccountId::from("SIM-001")),
        Some(VenueOrderId::from("V-001")),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
    );
    fill.position_id = Some(PositionId::from("P-004"));
    let position = Position::new(&instrument_eth_usdt, fill);
    assert_eq!(position.side, PositionSide::Long);

    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let cache = Rc::new(RefCell::new(simple_cache));
    let mut risk_engine = get_risk_engine(Some(cache), None, None, false);

    risk_engine.portfolio_mut().initialize_positions();

    // Sell 1 ETH (reduces long position) - should pass even with small balance
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .build();

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

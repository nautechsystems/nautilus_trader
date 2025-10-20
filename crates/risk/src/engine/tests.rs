// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

#![allow(clippy::too_many_arguments)] // Test functions with many fixtures

use std::{cell::RefCell, collections::HashMap, rc::Rc, str::FromStr};

use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    messages::execution::{ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand},
    msgbus::{
        self,
        handler::ShareableMessageHandler,
        stubs::{get_message_saving_handler, get_saved_messages},
        switchboard::MessagingSwitchboard,
    },
    throttler::RateLimit,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
use nautilus_model::{
    accounts::{
        AccountAny,
        stubs::{cash_account, margin_account},
    },
    data::{QuoteTick, stubs::quote_audusd},
    enums::{AccountType, LiquiditySide, OrderSide, OrderType, TimeInForce, TradingState},
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
        CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny,
        stubs::{audusd_sim, crypto_perpetual_ethusdt, xbtusd_bitmex},
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder},
    types::{AccountBalance, Currency, Money, Price, Quantity, fixed::FIXED_PRECISION},
};
use nautilus_portfolio::Portfolio;
use rstest::{fixture, rstest};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use ustr::Ustr;

// Helper that registers message collectors for ExecEngine.process events and
// returns the shared handler so callers can later retrieve the collected
// OrderEventAny messages via `get_process_order_event_handler_messages`.
fn register_process_handler() -> ShareableMessageHandler {
    let handler =
        get_message_saving_handler::<OrderEventAny>(Some(Ustr::from("ExecEngine.process")));
    msgbus::register(MessagingSwitchboard::exec_engine_process(), handler.clone());
    handler
}

#[rstest]
fn test_deny_order_on_price_precision_exceeded(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // Expect an OrderDenied to be emitted
    let saved_events = get_process_order_event_handler_messages(process_handler);
    assert_eq!(saved_events.len(), 1);
    matches!(saved_events[0], OrderEventAny::Denied(_));
}

#[rstest]
fn test_deny_order_exceeding_max_notional(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
) {
    let process_handler = register_process_handler();

    // Prepare small max_notional setting (1 USD)
    let mut max_notional_map = HashMap::new();
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
        max_notional_per_order: HashMap::new(),
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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_events = get_process_order_event_handler_messages(process_handler);
    assert_eq!(saved_events.len(), 1);
    matches!(saved_events[0], OrderEventAny::Denied(_));
}

use super::{RiskEngine, config::RiskEngineConfig};

#[fixture]
fn process_order_event_handler() -> ShareableMessageHandler {
    get_message_saving_handler::<OrderEventAny>(Some(Ustr::from("ExecEngine.process")))
}

#[fixture]
fn execute_order_event_handler() -> ShareableMessageHandler {
    get_message_saving_handler::<TradingCommand>(Some(Ustr::from("ExecEngine.execute")))
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
fn max_notional_per_order() -> HashMap<InstrumentId, Decimal> {
    HashMap::new()
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
fn get_stub_submit_order(
    trader_id: TraderId,
    client_id_binance: ClientId,
    strategy_id_ema_cross: StrategyId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    instrument_eth_usdt: InstrumentAny,
) -> SubmitOrder {
    SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        client_order_id,
        venue_order_id,
        market_order_buy(instrument_eth_usdt),
        None,
        None,
        UUID4::new(),
        UnixNanos::from(10),
    )
    .unwrap()
}

#[fixture]
fn config_fixture(
    max_order_submit: RateLimit,
    max_order_modify: RateLimit,
    max_notional_per_order: HashMap<InstrumentId, Decimal>,
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
    event_handler: ShareableMessageHandler,
) -> Vec<OrderEventAny> {
    get_saved_messages::<OrderEventAny>(event_handler)
}

fn get_execute_order_event_handler_messages(
    event_handler: ShareableMessageHandler,
) -> Vec<TradingCommand> {
    get_saved_messages::<TradingCommand>(event_handler)
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
        Some(Decimal::from_str("0.01").unwrap()),
        Some(Decimal::from_str("0.0035").unwrap()),
        Some(Decimal::from_str("-0.00025").unwrap()),
        Some(Decimal::from_str("0.00075").unwrap()),
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
        max_notional_per_order: HashMap::new(),
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

fn order_accepted(order: &OrderAny, venue_order_id: Option<VenueOrderId>) -> OrderAccepted {
    OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id.unwrap_or_default(),
        order.account_id().unwrap_or_default(),
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
    let account_id = account_id.unwrap_or(order.account_id().unwrap_or_default());
    let venue_order_id = venue_order_id.unwrap_or(order.venue_order_id().unwrap_or_default());
    let trade_id = trade_id.unwrap_or(order.client_order_id().as_str().replace('O', "E").into());
    let last_qty = last_qty.unwrap_or(order.quantity());
    let last_px = last_px.unwrap_or(order.price().unwrap_or_default());
    let liquidity_side = liquidity_side.unwrap_or(LiquiditySide::Taker);
    let ts_filled_ns = ts_filled_ns.unwrap_or(0.into());
    let account = account.unwrap_or(AccountAny::Cash(cash_account(
        cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
    )));

    let commission = account
        .calculate_commission(
            instrument.clone(),
            order.quantity(),
            last_px,
            liquidity_side,
            None,
        )
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

    assert!(risk_engine.config.bypass);
}

#[rstest]
fn test_trading_state_after_instantiation_returns_active() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.trading_state, TradingState::Active);
}

#[rstest]
fn test_set_trading_state_when_no_change_logs_warning() {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine.set_trading_state(TradingState::Active);

    assert_eq!(risk_engine.trading_state, TradingState::Active);
}

#[rstest]
fn test_set_trading_state_changes_value_and_publishes_event() {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine.set_trading_state(TradingState::Halted);

    assert_eq!(risk_engine.trading_state, TradingState::Halted);
}

#[rstest]
fn test_max_order_submit_rate_when_no_risk_config_returns_10_per_second() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.config.max_order_submit.limit, 10);
    assert_eq!(risk_engine.config.max_order_submit.interval_ns, 1000);
}

#[rstest]
fn test_max_order_modify_rate_when_no_risk_config_returns_5_per_second() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.config.max_order_modify.limit, 5);
    assert_eq!(risk_engine.config.max_order_modify.interval_ns, 1000);
}

#[rstest]
fn test_max_notionals_per_order_when_no_risk_config_returns_empty_hashmap() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.max_notional_per_order, HashMap::new());
}

#[rstest]
fn test_set_max_notional_per_order_changes_setting(instrument_audusd: InstrumentAny) {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());

    let mut expected = HashMap::new();
    expected.insert(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());
    assert_eq!(risk_engine.max_notional_per_order, expected);
}

#[rstest]
fn test_given_random_command_then_logs_and_continues(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
) {
    let mut risk_engine = get_risk_engine(None, None, None, false);

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::from_raw(100, 0))
        .quantity(Quantity::from("1000"))
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    let random_command = TradingCommand::SubmitOrder(submit_order);

    risk_engine.execute(random_command);
}

// SUBMIT ORDER TESTS
#[rstest]
fn test_submit_order_with_default_settings_then_sends_to_client(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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
        .price(Price::from_raw(100, 0))
        .quantity(Quantity::from("1000"))
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );
    let mut risk_engine = get_risk_engine(None, None, None, true);

    // TODO: Limit -> Market
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .price(Price::from_raw(100, 0))
        .quantity(Quantity::from("1000"))
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    clock: TestClock,
    simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );
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

    let submit_order1 = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order1.clone(),
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    let submitted = OrderEventAny::Submitted(order_submitted(&order1));
    let accepted = OrderEventAny::Accepted(order_accepted(&order1, None));
    let filled = OrderEventAny::Filled(order_filled(
        &order1,
        &instrument_audusd,
        None,
        None,
        None,
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
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order2.clone(),
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
    exec_engine.process(&OrderEventAny::Submitted(order_submitted(&order2)));
    exec_engine.process(&OrderEventAny::Filled(order_filled(
        &order2,
        &instrument_audusd,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )));

    let submit_order3 = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order3,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    // Act
    risk_engine.execute(TradingCommand::SubmitOrder(submit_order3));

    // Assert: TODO
    // assert_eq!(order1.status(), OrderStatus::Filled);
    // assert_eq!(order2.status(), OrderStatus::Filled);
    // assert_eq!(order3.status(), OrderStatus::Denied);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    clock: TestClock,
    simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );
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

    let submit_order1 = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order1.clone(),
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    let submitted = OrderEventAny::Submitted(order_submitted(&order1));
    let accepted = OrderEventAny::Accepted(order_accepted(&order1, None));
    let filled = OrderEventAny::Filled(order_filled(
        &order1,
        &instrument_audusd,
        None,
        None,
        None,
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
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order2.clone(),
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    // Act
    risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
    exec_engine.process(&OrderEventAny::Submitted(order_submitted(&order2)));
    exec_engine.process(&OrderEventAny::Accepted(order_accepted(&order2, None)));
    exec_engine.process(&OrderEventAny::Filled(order_filled(
        &order2,
        &instrument_audusd,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )));

    // Assert: TODO
    // assert_eq!(order1.status(), OrderStatus::Filled);
    // assert_eq!(order2.status(), OrderStatus::Denied);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
        .price(Price::from_raw(100, 0))
        .quantity(Quantity::from("1000"))
        .reduce_only(true)
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        Some(PositionId::new("CUSTOM-001")), // <-- Custom position ID
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
fn test_check_orders_risk_allows_reduce_only_sell_with_cash_base_currency(
    instrument_audusd: InstrumentAny,
    process_order_event_handler: ShareableMessageHandler,
    quote_audusd: QuoteTick,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

    let mut cache = Cache::new(None, None);
    cache.add_instrument(instrument_audusd.clone()).unwrap();
    cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("100002 USD", "100000 USD", "2 USD"),
        )))
        .unwrap();
    cache.add_quote(quote_audusd).unwrap();

    let risk_engine = get_risk_engine(Some(Rc::new(RefCell::new(cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("100000"))
        .reduce_only(true)
        .build();

    let allowed = risk_engine.check_orders_risk(instrument_audusd, vec![order]);
    assert!(allowed);

    let messages = get_process_order_event_handler_messages(process_order_event_handler);
    assert!(messages.is_empty());
}

#[rstest]
fn test_check_orders_risk_allows_reduce_only_sell_with_multi_currency_cash_account(
    instrument_audusd: InstrumentAny,
    process_order_event_handler: ShareableMessageHandler,
    quote_audusd: QuoteTick,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

    let mut cache = Cache::new(None, None);
    cache.add_instrument(instrument_audusd.clone()).unwrap();

    let multi_account_state = AccountState::new(
        account_id(),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("100002 USD"),
            Money::from("100000 USD"),
            Money::from("2 USD"),
        )],
        vec![],
        true,
        uuid4(),
        0.into(),
        0.into(),
        None,
    );

    cache
        .add_account(AccountAny::Cash(cash_account(multi_account_state)))
        .unwrap();
    cache.add_quote(quote_audusd).unwrap();

    let risk_engine = get_risk_engine(Some(Rc::new(RefCell::new(cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("100000"))
        .reduce_only(true)
        .build();

    let allowed = risk_engine.check_orders_risk(instrument_audusd, vec![order]);
    assert!(allowed);

    let messages = get_process_order_event_handler_messages(process_order_event_handler);
    assert!(messages.is_empty());
}

#[rstest]
fn test_check_orders_risk_non_reduce_sell_denies_on_free_balance(
    instrument_audusd: InstrumentAny,
    process_order_event_handler: ShareableMessageHandler,
    quote_audusd: QuoteTick,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

    let mut cache = Cache::new(None, None);
    cache.add_instrument(instrument_audusd.clone()).unwrap();
    cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("100002 USD", "100000 USD", "2 USD"),
        )))
        .unwrap();
    cache.add_quote(quote_audusd).unwrap();

    let risk_engine = get_risk_engine(Some(Rc::new(RefCell::new(cache))), None, None, false);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("100000"))
        .build();

    let allowed = risk_engine.check_orders_risk(instrument_audusd, vec![order]);
    assert!(!allowed);

    let messages = get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].event_type(), OrderEventType::Denied);
    let reason = messages[0].message().unwrap();
    assert!(
        reason
            .as_str()
            .contains("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE")
    );
}

#[rstest]
fn test_submit_order_when_instrument_not_in_cache_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
        .price(Price::from_raw(100, 0))
        .quantity(Quantity::from("1000"))
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
        .price(Price::from_raw(-1, 1)) // <- Invalid price
        .quantity(Quantity::from("1000"))
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);

    assert_eq!(
        saved_process_messages.first().unwrap().event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages.first().unwrap().message().unwrap(),
        Ustr::from("price 0.0 invalid (<= 0)")
    );
}

#[rstest]
fn test_submit_order_when_invalid_trigger_price_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
        .price(Price::from_raw(1, 1))
        .trigger_price(Price::from_raw(1_000_000_000_000_000, FIXED_PRECISION)) // <- Invalid price
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    execute_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_xbtusd_with_high_size_precision: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler,
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_with_high_size_precision.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
fn test_submit_order_list_buys_when_over_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("4920").unwrap())
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("5653").unwrap()) // <--- over free balance
        .build();

    let order_list = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![order1, order2],
        risk_engine.clock.borrow().timestamp_ns(),
    );

    let submit_order = SubmitOrderList::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order_list,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);

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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("4920").unwrap())
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("5653").unwrap()) // <--- over free balance
        .build();

    let order_list = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![order1, order2],
        risk_engine.clock.borrow().timestamp_ns(),
    );

    let submit_order = SubmitOrderList::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order_list,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);

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

// TODO: Implement test for multi-currency cash account over cumulative notional
#[ignore = "TODO: Requires ExecutionClient implementation"]
#[rstest]
fn test_submit_order_list_sells_when_multi_currency_cash_account_over_cumulative_notional() {}

#[ignore = "Message bus related changes re-investigate"]
#[rstest]
fn test_submit_order_when_reducing_and_buy_order_adds_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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

    risk_engine.set_max_notional_per_order(
        instrument_xbtusd_bitmex.id(),
        Decimal::from_str("10000").unwrap(),
    );

    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let submit_order1 = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        order1,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
    risk_engine.set_trading_state(TradingState::Reducing);

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let submit_order2 = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        order2,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);

    // TODO: currently, portfolio.is_net_long() is false, because portfolio.net_position() is not updated
    // assert!(risk_engine.portfolio.is_net_long(&instrument_xbtusd_bitmex.id()));
    // let saved_process_messages =
    //     get_process_order_event_handler_messages(process_order_event_handler);
    // assert_eq!(saved_process_messages.len(), 1);

    // assert_eq!(
    //     saved_process_messages.first().unwrap().event_type(),
    //     OrderEventType::Denied
    // );
    // assert_eq!(
    //     saved_process_messages.first().unwrap().message().unwrap(),
    //     "BUY when TradingState.REDUCING and LONG"
    // );
}

#[ignore = "Message bus related changes re-investigate"]
#[rstest]
fn test_submit_order_when_reducing_and_sell_order_adds_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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

    risk_engine.set_max_notional_per_order(
        instrument_xbtusd_bitmex.id(),
        Decimal::from_str("10000").unwrap(),
    );

    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let submit_order1 = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        order1,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
    risk_engine.set_trading_state(TradingState::Reducing);

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let submit_order2 = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        order2,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);

    // TODO: currently, portfolio.is_net_short() is false, because portfolio.net_position() is not updated
    // assert!(risk_engine.portfolio.is_net_short(&instrument_xbtusd_bitmex.id()));
    // let saved_process_messages =
    //     get_process_order_event_handler_messages(process_order_event_handler);
    // assert_eq!(saved_process_messages.len(), 1);

    // assert_eq!(
    //     saved_process_messages.first().unwrap().event_type(),
    //     OrderEventType::Denied
    // );
    // assert_eq!(
    //     saved_process_messages.first().unwrap().message().unwrap(),
    //     "SELL when TradingState.REDUCING and SHORT"
    // );
}

#[rstest]
fn test_submit_order_when_trading_halted_then_denies_order(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_eth_usdt: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        order.instrument_id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.set_trading_state(TradingState::Halted);

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // Get messages and test
    let saved_messages = get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Denied);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("TradingState::HALTED")
    );
}

#[ignore = "Message bus related changes re-investigate"]
#[rstest]
fn test_submit_order_beyond_rate_limit_then_denies_order(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
    for _i in 0..11 {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            order.instrument_id(),
            client_order_id,
            venue_order_id,
            order.clone(),
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    }

    assert_eq!(risk_engine.throttled_submit_order.used(), 1.0);

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::from_raw(1, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::from_raw(11, 2))
        .build();

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![entry, stop_loss, take_profit],
        risk_engine.clock.borrow().timestamp_ns(),
    );

    let submit_bracket = SubmitOrderList::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        bracket.instrument_id,
        client_order_id,
        venue_order_id,
        bracket,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(event.message().unwrap(), Ustr::from("TradingState::HALTED"));
    }
}

#[ignore = "Under development"]
#[rstest]
fn test_submit_order_list_buys_when_trading_reducing_then_denies_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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

    risk_engine.set_max_notional_per_order(
        instrument_xbtusd_bitmex.id(),
        Decimal::from_str("10000").unwrap(),
    );

    let long = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        long,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

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
        .trigger_price(Price::from_raw(11, 1))
        .build();

    // TODO: attempt to add with overflow
    // let take_profit = OrderTestBuilder::new(OrderType::Limit)
    //     .instrument_id(instrument_xbtusd_bitmex.id())
    //     .side(OrderSide::Buy)
    //     .quantity(Quantity::from_str("100").unwrap())
    //     .price(Price::from_raw(12, 1))
    //     .build();

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_xbtusd_bitmex.id(),
        StrategyId::new("S-001"),
        vec![entry, stop_loss],
        risk_engine.clock.borrow().timestamp_ns(),
    );

    let submit_order_list = SubmitOrderList::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        bracket,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[ignore = "Revisit after high-precision merged"]
#[rstest]
fn test_submit_order_list_sells_when_trading_reducing_then_denies_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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

    risk_engine.set_max_notional_per_order(
        instrument_xbtusd_bitmex.id(),
        Decimal::from_str("10000").unwrap(),
    );

    let short = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        short,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

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
        .trigger_price(Price::from_raw(11, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_xbtusd_bitmex.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::from_raw(12, 1))
        .build();

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_xbtusd_bitmex.id(),
        StrategyId::new("S-001"),
        vec![entry, stop_loss, take_profit],
        risk_engine.clock.borrow().timestamp_ns(),
    );

    let submit_order_list = SubmitOrderList::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        bracket,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

// SUBMIT BRACKET ORDER TESTS
#[ignore = "Message bus related changes re-investigate"]
#[rstest]
fn test_submit_bracket_with_default_settings_sends_to_client(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );

    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let risk_engine = get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::from_raw(1, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::from_raw(1001, 4))
        .build();

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![entry, stop_loss, take_profit],
        risk_engine.clock.borrow().timestamp_ns(),
    );

    let _submit_bracket = SubmitOrderList::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        bracket.instrument_id,
        client_order_id,
        venue_order_id,
        bracket,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    // risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // Get messages and test
    // TODO: complete fn execution_gateway
    // let saved_process_messages =
    //     get_process_order_event_handler_messages(process_order_event_handler);
    // assert_eq!(saved_process_messages.len(), 0);
}

// TODO: Verify bracket orders with emulated orders are sent to emulator
#[ignore = "TODO: Requires emulator implementation"]
#[rstest]
fn test_submit_bracket_with_emulated_orders_sends_to_emulator() {}

#[rstest]
fn test_submit_bracket_order_when_instrument_not_in_cache_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let entry = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::from_raw(1, 1))
        .build();

    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::from_raw(1001, 4))
        .build();

    let bracket = OrderList::new(
        OrderListId::new("1"),
        instrument_audusd.id(),
        StrategyId::new("S-001"),
        vec![entry, stop_loss, take_profit],
        risk_engine.clock.borrow().timestamp_ns(),
    );

    let submit_bracket = SubmitOrderList::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        bracket.instrument_id,
        client_order_id,
        venue_order_id,
        bracket,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(
            event.message().unwrap(),
            Ustr::from("no instrument found for AUD/USD.SIM")
        );
    }
}

// TODO: Verify emulated orders are sent to emulator
#[ignore = "TODO: Requires emulator implementation"]
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
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);
}

#[ignore = "Message bus related changes re-investigate"]
#[rstest]
fn test_modify_order_beyond_rate_limit_then_rejects(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
        .trigger_price(Price::from_raw(10001, 4))
        .build();

    simple_cache
        .add_order(order, None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    for i in 0..11 {
        let modify_order = ModifyOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            Some(Quantity::from_str("100").unwrap()),
            Some(Price::from_raw(100011 + i, 5)),
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::ModifyOrder(modify_order));
    }

    assert_eq!(risk_engine.throttled_modify_order.used(), 1.0);

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 6);
    let first_message = saved_process_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::ModifyRejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Exceeded MAX_ORDER_MODIFY_RATE")
    );
}

#[ignore = "Message bus related changes re-investigate"]
#[rstest]
fn test_modify_order_with_default_settings_then_sends_to_client(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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
        .trigger_price(Price::from_raw(10001, 4))
        .build();

    simple_cache
        .add_order(order.clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    let modify_order = ModifyOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        Some(Quantity::from_str("100").unwrap()),
        Some(Price::from_raw(100011, 5)),
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 2);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_audusd.id()
    );
}

// TODO: Verify modify order for emulated orders sends to emulator
#[ignore = "TODO: Requires emulator implementation"]
#[rstest]
fn test_modify_order_for_emulated_order_then_sends_to_emulator() {}

#[rstest]
fn test_submit_order_when_market_order_and_over_free_balance_then_denies_with_betting_account(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Margin(margin_account(
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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0); // Currently, it executes because check_orders_risk returns true for margin_account
}

#[ignore = "Message bus related changes re-investigate"]
#[rstest]
fn test_submit_order_for_less_than_max_cum_transaction_value_adausdt_with_crypto_cash_account(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler.clone(),
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);

    let saved_execute_messages =
        get_execute_order_event_handler_messages(execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_xbtusd_bitmex.id()
    );
}

// TODO: Verify account balance updates correctly with partial and full fills
#[ignore = "TODO: Requires account balance tracking implementation"]
#[rstest]
fn test_partial_fill_and_full_fill_account_balance_correct() {}

#[rstest]
fn test_submit_order_with_gtd_expire_time_already_passed(
    clock: TestClock,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_xbtusd_bitmex: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    execute_order_event_handler: ShareableMessageHandler,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler,
    );
    msgbus::register(
        MessagingSwitchboard::exec_engine_execute(),
        execute_order_event_handler,
    );

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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        instrument_xbtusd_bitmex.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        clock.timestamp_ns(),
    )
    .unwrap();

    clock.set_time(UnixNanos::from(2_000)); // <-- Set time to 2,000 nanos past epoch

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // TODO: Change command messages to not require owned orders
}

#[rstest]
fn test_submit_order_with_quote_quantity_validates_correctly(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    _cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

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
        Some(Decimal::from_str("0.1").unwrap()), // margin_init
        Some(Decimal::from_str("0.1").unwrap()), // margin_maint
        Some(Decimal::from_str("-0.00005").unwrap()), // maker_fee
        Some(Decimal::from_str("0.00015").unwrap()), // taker_fee
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

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        btc_usdt.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // The order should be accepted (not denied)
    // If the bug exists, it would compare 100 > 83 and deny the order
    // With the fix, it converts 100 USDT -> 0.001 BTC, then checks 0.001 < 83 (passes)
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);

    // Should have 1 event (submitted to exec engine, not denied)
    assert_eq!(
        saved_process_messages.len(),
        0,
        "Order should not be denied"
    );
}

#[rstest]
fn test_submit_order_with_quote_quantity_exceeds_max_after_conversion(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    process_order_event_handler: ShareableMessageHandler,
    _cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    msgbus::register(
        MessagingSwitchboard::exec_engine_process(),
        process_order_event_handler.clone(),
    );

    // Create a BTCUSDT spot instrument with max_quantity = 0.5 BTC
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
        Some(Decimal::from_str("0.1").unwrap()),
        Some(Decimal::from_str("0.1").unwrap()),
        Some(Decimal::from_str("-0.00005").unwrap()),
        Some(Decimal::from_str("0.00015").unwrap()),
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
    // 100,000 USDT quote quantity = 1 BTC base quantity
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

    // Create a market order with quote_quantity = 100,000 USDT
    // This converts to 1 BTC which exceeds max_quantity of 0.5 BTC
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(btc_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000")) // 100,000 USDT
        .quote_quantity(true)
        .build();

    let submit_order = SubmitOrder::new(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        btc_usdt.id(),
        client_order_id,
        venue_order_id,
        order,
        None,
        None,
        UUID4::new(),
        risk_engine.clock.borrow().timestamp_ns(),
    )
    .unwrap();

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    // The order should be denied because effective_quantity (1 BTC) > max_quantity (0.5 BTC)
    let saved_process_messages =
        get_process_order_event_handler_messages(process_order_event_handler);
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
            .contains("QUANTITY_EXCEEDS_MAXIMUM")
    );
}

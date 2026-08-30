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

#![expect(
    clippy::too_many_arguments,
    reason = "rstest fixtures define broad test setup signatures"
)]

use std::{cell::RefCell, rc::Rc, str::FromStr, sync::Arc};

use ahash::{AHashMap, AHashSet};
use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    messages::{
        execution::{
            BatchModifyOrders, CancelOrder, ModifyOrder, PARAMS_CLOSE_POSITION,
            PARAMS_EMERGENCY_EXIT, SubmitOrder, SubmitOrderList, TradingCommand,
        },
        system::trading::TradingStateChanged,
    },
    msgbus::{
        self, MessagingSwitchboard, TypedHandler,
        stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
    },
    runner::{
        SyncTradingCommandSender, drain_trading_cmd_queue, replace_exec_cmd_sender,
        trading_cmd_queue_is_empty,
    },
    throttler::RateLimit,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
use nautilus_model::{
    accounts::{
        AccountAny, BettingAccount, CashAccount, MarginAccount, WalletAccount, stubs::cash_account,
    },
    data::{
        Bar, BarSpecification, BarType, QuoteTick, TradeTick,
        stubs::{quote_audusd, quote_ethusdt_binance},
    },
    enums::{
        AccountType, AggregationSource, AggressorSide, BarAggregation, LiquiditySide, OmsType,
        OrderSide, OrderStatus, OrderType, PositionSide, PriceType, TimeInForce, TradingState,
        TrailingOffsetType, TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderDeniedReason, OrderEventAny, OrderEventType, OrderFilled,
        OrderPriceField, OrderSubmitted, PositionEvent, PositionOpened,
        account::stubs::cash_account_state_million_usd,
        order::spec::{OrderAcceptedSpec, OrderFilledSpec, OrderSubmittedSpec},
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
        Symbol, TradeId, TraderId, Venue, VenueOrderId,
        stubs::{
            account_id, client_id_binance, client_order_id, strategy_id_ema_cross, trader_id,
            uuid4, venue_order_id,
        },
    },
    instruments::{
        Commodity, CryptoPerpetual, CurrencyPair, FuturesSpread, Instrument, InstrumentAny,
        OptionSpread, PerpetualContract,
        stubs::{
            audusd_sim, betting, commodity_gold, crypto_perpetual_ethusdt, currency_pair_btcusdt,
            futures_spread_es, gbpusd_sim, option_spread, perpetual_contract_eurusd, xbtusd_bitmex,
        },
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder},
    position::Position,
    types::{AccountBalance, Currency, MONEY_MAX, Money, Price, Quantity, fixed::FIXED_PRECISION},
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

fn consume_fixture<T>(_: T) {}

#[rstest]
fn test_deny_order_on_price_precision_exceeded(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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

    // AUD/USD price precision is 5 - create a Limit order with 6-dp price (invalid)
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
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
        full_position_exit_venues: AHashSet::new(),
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
        None, // correlation_id
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
        None, // correlation_id
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
        full_position_exit_venues: AHashSet::new(),
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
fn instrument_commodity(commodity_gold: Commodity) -> InstrumentAny {
    InstrumentAny::Commodity(commodity_gold)
}

#[fixture]
pub fn instrument_xbtusd_with_high_size_precision() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(
        CryptoPerpetual::builder()
            .instrument_id(InstrumentId::from("BTCUSDT.BITMEX"))
            .raw_symbol(Symbol::from("XBTUSD"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USD())
            .settlement_currency(Currency::BTC())
            .is_inverse(true)
            .price_precision(1)
            .size_precision(2)
            .price_increment(Price::from("0.5"))
            .size_increment(Quantity::from("0.01"))
            .max_notional(Money::from("10000000 USD"))
            .min_notional(Money::from("1 USD"))
            .max_price(Price::from("10000000"))
            .min_price(Price::from("0.01"))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.0035))
            .maker_fee(dec!(-0.00025))
            .taker_fee(dec!(0.00075))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
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
        full_position_exit_venues: AHashSet::new(),
    });
    let clock = clock.unwrap_or(Rc::new(RefCell::new(TestClock::new())));
    let portfolio = Portfolio::new(clock.clone(), cache.clone(), None);
    RiskEngine::new(config, portfolio, clock, cache)
}

fn get_risk_engine_for_full_position_exit(
    cache: Option<Rc<RefCell<Cache>>>,
    venue: Venue,
) -> RiskEngine {
    let config = RiskEngineConfig {
        debug: true,
        bypass: false,
        max_order_submit: RateLimit::new(10, 1000),
        max_order_modify: RateLimit::new(5, 1000),
        max_notional_per_order: AHashMap::new(),
        full_position_exit_venues: [venue].into_iter().collect(),
    };
    get_risk_engine(cache, Some(config), None, false)
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

#[rstest]
fn test_counters_increment_and_reset(get_stub_submit_order: (OrderAny, SubmitOrder)) {
    let (order, submit_order) = get_stub_submit_order;
    let mut risk_engine = get_risk_engine(None, None, None, true);

    assert_eq!(risk_engine.command_count(), 0);
    assert_eq!(risk_engine.event_count(), 0);

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    risk_engine.process(OrderEventAny::Submitted(order_submitted(&order)));

    assert_eq!(risk_engine.command_count(), 1);
    assert_eq!(risk_engine.event_count(), 1);

    risk_engine.reset();

    assert_eq!(risk_engine.command_count(), 0);
    assert_eq!(risk_engine.event_count(), 0);
}

#[rstest]
fn test_register_msgbus_handlers_registers_process_and_event_subscriptions(
    get_stub_submit_order: (OrderAny, SubmitOrder),
) {
    msgbus::get_message_bus().borrow_mut().dispose();

    let (order, _) = get_stub_submit_order;
    let risk_engine = Rc::new(RefCell::new(get_risk_engine(None, None, None, true)));
    RiskEngine::register_msgbus_handlers(&risk_engine);

    let submitted = OrderEventAny::Submitted(order_submitted(&order));
    msgbus::send_order_event(
        MessagingSwitchboard::risk_engine_process(),
        submitted.clone(),
    );

    let order_topic = format!("events.order.{}", order.strategy_id());
    msgbus::publish_order_event(order_topic.into(), &submitted);

    let position_event = PositionEvent::PositionOpened(position_opened(&order));
    let position_topic = format!("events.position.{}", order.strategy_id());
    msgbus::publish_position_event(position_topic.into(), &position_event);

    assert_eq!(risk_engine.borrow().event_count(), 3);
}

#[rstest]
fn test_deferred_risk_command_is_checked_before_execution(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
    cash_account_state_million_usd: AccountState,
) {
    std::thread::spawn(move || {
        msgbus::get_message_bus().borrow_mut().dispose();
        replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));

        let process_handler = register_process_handler();
        let (exec_handler, exec_saving_handler) = get_typed_into_message_saving_handler::<
            TradingCommand,
        >(Some(Ustr::from("ExecEngine.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            exec_handler,
        );

        let mut cache = Cache::default();
        cache.add_instrument(instrument_audusd.clone()).unwrap();
        cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();
        let risk_engine = Rc::new(RefCell::new(get_risk_engine(
            Some(Rc::new(RefCell::new(cache))),
            None,
            None,
            false,
        )));
        RiskEngine::register_msgbus_handlers(&risk_engine);

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from("2000000"))
            .build();
        risk_engine
            .borrow()
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
            risk_engine.borrow().clock().borrow().timestamp_ns(),
            None,
        );

        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_queue_execute(),
            TradingCommand::SubmitOrder(submit_order),
        );
        assert_eq!(risk_engine.borrow().command_count(), 0);

        drain_trading_cmd_queue();

        let denied = get_process_order_event_handler_messages(&process_handler);
        assert_eq!(risk_engine.borrow().command_count(), 1);
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].event_type(), OrderEventType::Denied);
        assert_eq!(
            denied[0].message().unwrap(),
            Ustr::from("QUANTITY_EXCEEDS_MAXIMUM: effective=2000000, max=1000000")
        );
        assert_eq!(exec_saving_handler.get_messages(), Vec::new());
    })
    .join()
    .unwrap();
}

#[rstest]
fn test_deferred_risk_denial_does_not_reenter_engine(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
    cash_account_state_million_usd: AccountState,
) {
    std::thread::spawn(move || {
        msgbus::get_message_bus().borrow_mut().dispose();
        replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(instrument_audusd.clone()).unwrap();
            cache
                .add_account(AccountAny::Cash(cash_account(
                    cash_account_state_million_usd,
                )))
                .unwrap();
        }

        let exec_engine = Rc::new(RefCell::new(get_exec_engine(
            Some(cache.clone()),
            Some(clock.clone()),
            None,
        )));
        ExecutionEngine::register_msgbus_handlers(&exec_engine);
        let risk_engine = Rc::new(RefCell::new(get_risk_engine(
            Some(cache.clone()),
            None,
            Some(clock),
            false,
        )));
        RiskEngine::register_msgbus_handlers(&risk_engine);

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from("2000000"))
            .build();
        cache
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
            risk_engine.borrow().clock().borrow().timestamp_ns(),
            None,
        );

        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_queue_execute(),
            TradingCommand::SubmitOrder(submit_order),
        );
        drain_trading_cmd_queue();

        assert!(trading_cmd_queue_is_empty());
        assert_eq!(risk_engine.borrow().command_count(), 1);
        assert_eq!(exec_engine.borrow().command_count(), 0);
        assert_eq!(
            cache
                .borrow()
                .order(&order.client_order_id())
                .unwrap()
                .status(),
            OrderStatus::Denied
        );
    })
    .join()
    .unwrap();
}

#[rstest]
fn test_deferred_risk_approval_preserves_command_order(
    get_stub_submit_order: (OrderAny, SubmitOrder),
) {
    std::thread::spawn(move || {
        msgbus::get_message_bus().borrow_mut().dispose();
        replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let exec_engine = Rc::new(RefCell::new(get_exec_engine(
            Some(cache.clone()),
            Some(clock.clone()),
            None,
        )));
        ExecutionEngine::register_msgbus_handlers(&exec_engine);

        let risk_engine = Rc::new(RefCell::new(get_risk_engine(
            Some(cache),
            None,
            Some(clock),
            true,
        )));
        RiskEngine::register_msgbus_handlers(&risk_engine);

        let (exec_handler, exec_saving_handler) = get_typed_into_message_saving_handler::<
            TradingCommand,
        >(Some(Ustr::from("ExecEngine.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            exec_handler,
        );

        let (order, submit_order) = get_stub_submit_order;
        let cancel_order = CancelOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            None,
            UUID4::new(),
            UnixNanos::from(11),
            None,
            None,
        );

        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_queue_execute(),
            TradingCommand::SubmitOrder(submit_order),
        );
        msgbus::send_trading_command(
            MessagingSwitchboard::exec_engine_queue_execute(),
            TradingCommand::CancelOrder(cancel_order),
        );

        drain_trading_cmd_queue();

        let commands = exec_saving_handler.get_messages();
        assert!(trading_cmd_queue_is_empty());
        assert_eq!(commands.len(), 2);
        assert!(matches!(commands[0], TradingCommand::SubmitOrder(_)));
        assert!(matches!(commands[1], TradingCommand::CancelOrder(_)));
    })
    .join()
    .unwrap();
}

#[rstest]
fn test_register_msgbus_handlers_subscribes_event_topics_at_priority_10(
    get_stub_submit_order: (OrderAny, SubmitOrder),
) {
    msgbus::get_message_bus().borrow_mut().dispose();

    let (order, _) = get_stub_submit_order;
    let risk_engine = Rc::new(RefCell::new(get_risk_engine(None, None, None, true)));
    let order_observations = Rc::new(RefCell::new(Vec::new()));
    let position_observations = Rc::new(RefCell::new(Vec::new()));

    let high_order_observations = order_observations.clone();
    let high_order_engine = risk_engine.clone();
    msgbus::subscribe_order_events(
        "events.order.*".into(),
        TypedHandler::from_with_id("order-high", move |_: &OrderEventAny| {
            high_order_observations
                .borrow_mut()
                .push(("high", high_order_engine.borrow().event_count()));
        }),
        Some(11),
    );

    let high_position_observations = position_observations.clone();
    let high_position_engine = risk_engine.clone();
    msgbus::subscribe_position_events(
        "events.position.*".into(),
        TypedHandler::from_with_id("position-high", move |_: &PositionEvent| {
            high_position_observations
                .borrow_mut()
                .push(("high", high_position_engine.borrow().event_count()));
        }),
        Some(11),
    );

    RiskEngine::register_msgbus_handlers(&risk_engine);

    let low_order_observations = order_observations.clone();
    let low_order_engine = risk_engine.clone();
    msgbus::subscribe_order_events(
        "events.order.*".into(),
        TypedHandler::from_with_id("order-low", move |_: &OrderEventAny| {
            low_order_observations
                .borrow_mut()
                .push(("low", low_order_engine.borrow().event_count()));
        }),
        Some(9),
    );

    let low_position_observations = position_observations.clone();
    let low_position_engine = risk_engine.clone();
    msgbus::subscribe_position_events(
        "events.position.*".into(),
        TypedHandler::from_with_id("position-low", move |_: &PositionEvent| {
            low_position_observations
                .borrow_mut()
                .push(("low", low_position_engine.borrow().event_count()));
        }),
        Some(9),
    );

    let submitted = OrderEventAny::Submitted(order_submitted(&order));
    let order_topic = format!("events.order.{}", order.strategy_id());
    msgbus::publish_order_event(order_topic.into(), &submitted);

    let position_event = PositionEvent::PositionOpened(position_opened(&order));
    let position_topic = format!("events.position.{}", order.strategy_id());
    msgbus::publish_position_event(position_topic.into(), &position_event);

    assert_eq!(
        order_observations.borrow().as_slice(),
        &[("high", 0), ("low", 1)]
    );
    assert_eq!(
        position_observations.borrow().as_slice(),
        &[("high", 1), ("low", 2)]
    );
    assert_eq!(risk_engine.borrow().event_count(), 2);
}

fn order_submitted(order: &OrderAny) -> OrderSubmitted {
    OrderSubmittedSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .account_id(order.account_id().unwrap_or(account_id()))
        .build()
}

fn position_opened(order: &OrderAny) -> PositionOpened {
    PositionOpened {
        trader_id: order.trader_id(),
        strategy_id: order.strategy_id(),
        instrument_id: order.instrument_id(),
        position_id: PositionId::new("P-001"),
        account_id: account_id(),
        opening_order_id: order.client_order_id(),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 1.0,
        quantity: order.quantity(),
        last_qty: order.quantity(),
        last_px: Price::from("1.0"),
        currency: Currency::USD(),
        avg_px_open: 1.0,
        realized_pnl: None,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1),
        ts_init: UnixNanos::from(1),
    }
}

fn order_accepted(
    order: &OrderAny,
    venue_order_id: Option<VenueOrderId>,
    account_id: Option<AccountId>,
) -> OrderAccepted {
    OrderAcceptedSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .venue_order_id(venue_order_id.expect("venue_order_id required for order_accepted"))
        .account_id(account_id.unwrap_or_else(|| AccountId::new("SIM-001")))
        .build()
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

    OrderFilledSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .client_order_id(order.client_order_id())
        .venue_order_id(venue_order_id)
        .account_id(account_id)
        .trade_id(trade_id)
        .order_side(order.order_side())
        .order_type(order.order_type())
        .last_qty(last_qty)
        .last_px(last_px)
        .currency(instrument.quote_currency())
        .liquidity_side(liquidity_side)
        .ts_event(ts_filled_ns)
        .commission(commission)
        .build()
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

    assert_eq!(risk_engine.config().max_order_submit.limit(), 10);
    assert_eq!(risk_engine.config().max_order_submit.interval_ns(), 1000);
}

#[rstest]
fn test_max_order_modify_rate_when_no_risk_config_returns_5_per_second() {
    let risk_engine = get_risk_engine(None, None, None, false);

    assert_eq!(risk_engine.config().max_order_modify.limit(), 5);
    assert_eq!(risk_engine.config().max_order_modify.interval_ns(), 1000);
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
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100_000).unwrap());

    let mut expected = AHashMap::new();
    expected.insert(instrument_audusd.id(), Decimal::from_i64(100_000).unwrap());
    assert_eq!(*risk_engine.max_notional_per_order(), expected);
}

#[rstest]
fn test_given_random_command_then_logs_and_continues(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    quote_audusd: QuoteTick,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    clock: TestClock,
    simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
        None, // correlation_id
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    clock: TestClock,
    simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
        Ustr::from("POSITION_NOT_FOUND: CUSTOM-001")
    );
}

#[rstest]
fn test_submit_order_when_instrument_not_in_cache_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
        Ustr::from("INSTRUMENT_NOT_FOUND: AUD/USD.SIM")
    );
}

#[rstest]
fn test_submit_order_when_invalid_price_precision_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
            &OrderDeniedReason::PricePrecisionExceedsMaximum {
                field: OrderPriceField::Price,
                price: order.price().unwrap(),
                price_precision: order.price().unwrap().precision,
                max_precision: instrument_audusd.price_precision(),
            }
            .to_string()
        )
    );
}

#[rstest]
fn test_submit_order_when_invalid_negative_price_and_not_option_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
        Ustr::from("PRICE_NOT_POSITIVE: field=PRICE, price=-0.1")
    );
}

#[rstest]
fn test_submit_order_when_negative_price_for_futures_spread_then_allows(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_futures_spread: InstrumentAny,
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
        None, // correlation_id
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
    instrument_option_spread: InstrumentAny,
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
        None, // correlation_id
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
fn test_submit_order_when_negative_price_for_commodity_then_allows(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_commodity: InstrumentAny,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_commodity.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_commodity.id())
        .side(OrderSide::Buy)
        .price(Price::new(-5.0, 2)) // Negative price is valid for spot commodities
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
        instrument_commodity.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_commodity.id()
    );
}

#[rstest]
fn test_submit_order_when_zero_price_for_commodity_then_allows(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_commodity: InstrumentAny,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_commodity.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_commodity.id())
        .side(OrderSide::Buy)
        .price(Price::new(0.0, 2)) // Zero price shares the negative price gate
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
        instrument_commodity.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_commodity.id()
    );
}

#[rstest]
fn test_submit_order_when_invalid_trigger_price_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
            &OrderDeniedReason::PricePrecisionExceedsMaximum {
                field: OrderPriceField::TriggerPrice,
                price: order.trigger_price().unwrap(),
                price_precision: order.trigger_price().unwrap().precision,
                max_precision: instrument_audusd.price_precision(),
            }
            .to_string()
        )
    );
}

#[rstest]
fn test_submit_order_when_invalid_quantity_precision_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
            "QUANTITY_PRECISION_EXCEEDS_MAXIMUM: quantity=0.1, precision=1, max_precision=0",
        )
    );
}

#[rstest]
fn test_submit_order_when_invalid_quantity_exceeds_maximum_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
        Ustr::from("QUANTITY_EXCEEDS_MAXIMUM: effective=100000000, max=1000000")
    );
}

#[rstest]
fn test_submit_order_when_invalid_quantity_less_than_minimum_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
        Ustr::from("QUANTITY_BELOW_MINIMUM: effective=1, min=100")
    );
}

#[rstest]
#[case::market(
    OrderType::Market,
    "MARKET_PRICE_UNAVAILABLE: order_type=MARKET, instrument_id=AUD/USD.SIM"
)]
#[case::market_to_limit(
    OrderType::MarketToLimit,
    "MARKET_PRICE_UNAVAILABLE: order_type=MARKET_TO_LIMIT, instrument_id=AUD/USD.SIM"
)]
fn test_submit_market_order_without_price_then_denies(
    #[case] order_type: OrderType,
    #[case] expected_reason: &str,
    #[values(true, false)] with_account: bool,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();

    if with_account {
        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();
    }

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    submit_market_order(
        &mut risk_engine,
        instrument_audusd.id(),
        order_type,
        OrderSide::Buy,
        "100",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(expected_reason)
    );
    assert_eq!(execute_messages.len(), 0);
}

#[rstest]
#[case::quote(true, "750050.00 USD")]
#[case::trade(false, "500000.00 USD")]
fn test_submit_market_order_preserves_price_precedence(
    #[case] with_quote: bool,
    #[case] expected_notional: &str,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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

    if with_quote {
        simple_cache
            .add_quote(QuoteTick::new(
                instrument_audusd.id(),
                Price::from("0.75000"),
                Price::from("0.75005"),
                Quantity::from("1"),
                Quantity::from("1"),
                UnixNanos::from(1),
                UnixNanos::from(1),
            ))
            .unwrap();
    }
    simple_cache
        .add_trade(TradeTick::new(
            instrument_audusd.id(),
            Price::from("0.50000"),
            Quantity::from("1"),
            AggressorSide::Buy,
            TradeId::new("T-001"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();
    simple_cache
        .add_bar(market_bar(
            instrument_audusd.id(),
            PriceType::Ask,
            1,
            "0.25000",
            3,
        ))
        .unwrap();
    simple_cache
        .add_bar(market_bar(
            instrument_audusd.id(),
            PriceType::Last,
            1,
            "0.12500",
            4,
        ))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100_000).unwrap());
    submit_market_order(
        &mut risk_engine,
        instrument_audusd.id(),
        OrderType::Market,
        OrderSide::Buy,
        "1000000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    assert_max_notional_denied(&process_order_event_handler, expected_notional);
}

#[rstest]
#[case::buy(OrderSide::Buy, "750050.00 USD")]
#[case::sell(OrderSide::Sell, "750000.00 USD")]
fn test_submit_market_order_uses_side_bar_close(
    #[case] order_side: OrderSide,
    #[case] expected_notional: &str,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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

    for (price_type, selected_close) in [(PriceType::Bid, "0.75000"), (PriceType::Ask, "0.75005")] {
        let other = market_bar(instrument_audusd.id(), price_type, 1, "0.10000", 2);
        let selected = market_bar(instrument_audusd.id(), price_type, 5, selected_close, 2);
        assert!(selected.bar_type > other.bar_type);
        simple_cache.add_bar(selected).unwrap();
        simple_cache.add_bar(other).unwrap();
    }
    simple_cache
        .add_bar(market_bar(
            instrument_audusd.id(),
            PriceType::Last,
            1,
            "0.90000",
            3,
        ))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100_000).unwrap());
    submit_market_order(
        &mut risk_engine,
        instrument_audusd.id(),
        OrderType::Market,
        order_side,
        "1000000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    assert_max_notional_denied(&process_order_event_handler, expected_notional);
}

#[rstest]
fn test_submit_market_order_uses_last_bar_fallback(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
    simple_cache
        .add_bar(market_bar(
            instrument_audusd.id(),
            PriceType::Last,
            1,
            "0.75001",
            1,
        ))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100_000).unwrap());
    submit_market_order(
        &mut risk_engine,
        instrument_audusd.id(),
        OrderType::Market,
        OrderSide::Buy,
        "1000000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    assert_max_notional_denied(&process_order_event_handler, "750010.00 USD");
}

#[rstest]
fn test_submit_market_order_without_price_checks_cash_asset_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    let account_state = AccountState::new(
        AccountId::from("BINANCE-001"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("10000 USDT"),
            Money::from("0 USDT"),
            Money::from("10000 USDT"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        None,
    );
    simple_cache
        .add_account(AccountAny::Cash(CashAccount::new(
            account_state,
            true,
            false,
        )))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    submit_market_order(
        &mut risk_engine,
        instrument_eth_usdt.id(),
        OrderType::Market,
        OrderSide::Sell,
        "1.000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                free_balance: Money::from("0 ETH"),
                cumulative_notional: Money::from("1 ETH"),
            }
            .to_string()
        )
    );
    assert_eq!(execute_messages.len(), 0);
}

fn add_wallet_account(cache: &mut Cache, eth_total: &str) {
    let eth = Currency::ETH();
    let usdc = Currency::USDC();
    let account_state = AccountState::new(
        AccountId::from("BINANCE-001"),
        AccountType::Wallet,
        vec![
            AccountBalance::new(
                Money::from(eth_total),
                Money::zero(eth),
                Money::from(eth_total),
            ),
            AccountBalance::new(
                Money::from("25000 USDC"),
                Money::zero(usdc),
                Money::from("25000 USDC"),
            ),
        ],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        None,
    );
    cache
        .add_account(AccountAny::Wallet(WalletAccount::new(account_state, true)))
        .unwrap();
}

#[rstest]
fn test_submit_market_order_wallet_sell_exceeds_free_balance_without_price_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    add_wallet_account(&mut simple_cache, "10 ETH");

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    submit_market_order(
        &mut risk_engine,
        instrument_eth_usdt.id(),
        OrderType::Market,
        OrderSide::Sell,
        "11.000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                free_balance: Money::from("10 ETH"),
                cumulative_notional: Money::from("11 ETH"),
            }
            .to_string()
        )
    );
    assert_eq!(execute_messages.len(), 0);
}

#[rstest]
fn test_submit_market_order_wallet_sell_within_balance_without_price_denies_no_market_price(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    add_wallet_account(&mut simple_cache, "10 ETH");

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    submit_market_order(
        &mut risk_engine,
        instrument_eth_usdt.id(),
        OrderType::Market,
        OrderSide::Sell,
        "1.000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(&format!(
            "MARKET_PRICE_UNAVAILABLE: order_type=MARKET, instrument_id={}",
            instrument_eth_usdt.id()
        ))
    );
    assert_eq!(execute_messages.len(), 0);
}

#[rstest]
fn test_submit_market_order_wallet_sell_exceeds_free_balance_with_price_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from("3000.00"),
            Price::from("3001.00"),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ))
        .unwrap();
    add_wallet_account(&mut simple_cache, "10 ETH");

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    submit_market_order(
        &mut risk_engine,
        instrument_eth_usdt.id(),
        OrderType::Market,
        OrderSide::Sell,
        "11.000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                free_balance: Money::from("10 ETH"),
                cumulative_notional: Money::from("11 ETH"),
            }
            .to_string()
        )
    );
    assert_eq!(execute_messages.len(), 0);
}

#[rstest]
fn test_submit_market_order_wallet_sell_within_balance_with_price_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from("3000.00"),
            Price::from("3001.00"),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ))
        .unwrap();
    add_wallet_account(&mut simple_cache, "10 ETH");

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    submit_market_order(
        &mut risk_engine,
        instrument_eth_usdt.id(),
        OrderType::Market,
        OrderSide::Sell,
        "1.000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 0);
    assert_eq!(execute_messages.len(), 1);
}

#[rstest]
fn test_submit_market_order_wallet_buy_missing_quote_balance_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from("3000.00"),
            Price::from("3001.00"),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ))
        .unwrap();
    add_wallet_account(&mut simple_cache, "10 ETH");

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    submit_market_order(
        &mut risk_engine,
        instrument_eth_usdt.id(),
        OrderType::Market,
        OrderSide::Buy,
        "1.000",
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
    );

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::NotionalExceedsFreeBalance {
                free_balance: Money::from("0 USDT"),
                notional: Money::from("3001 USDT"),
            }
            .to_string()
        )
    );
    assert_eq!(execute_messages.len(), 0);
}

#[rstest]
fn test_submit_reduce_only_wallet_sell_still_checks_asset_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from("3000.00"),
            Price::from("3001.00"),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ))
        .unwrap();
    add_wallet_account(&mut simple_cache, "10 ETH");

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("11.000"))
        .reduce_only(true)
        .build();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();
    let command = SubmitOrder::new(
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
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(command));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                free_balance: Money::from("10 ETH"),
                cumulative_notional: Money::from("11 ETH"),
            }
            .to_string()
        )
    );
    assert_eq!(execute_messages.len(), 0);
}

#[rstest]
fn test_submit_market_order_list_resolves_price_per_order(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_audusd.id(),
            Price::from("0.10000"),
            Price::from("0.75005"),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ))
        .unwrap();

    let orders = [
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .client_order_id(ClientOrderId::from("O-SELL"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1000000"))
            .build(),
        OrderTestBuilder::new(OrderType::MarketToLimit)
            .instrument_id(instrument_audusd.id())
            .client_order_id(ClientOrderId::from("O-BUY"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000000"))
            .build(),
    ];

    for order in &orders {
        simple_cache
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(500_000).unwrap());
    let order_list = OrderList::new(
        OrderListId::new("OL-MIXED-SIDE"),
        instrument_audusd.id(),
        strategy_id_ema_cross,
        orders.iter().map(Order::client_order_id).collect(),
        risk_engine.clock().borrow().timestamp_ns(),
    );
    let submit_order_list = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        orders
            .iter()
            .map(|order| order.init_event().clone())
            .collect(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

    let messages = get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::NotionalExceedsMaxPerOrder {
                max_notional: Money::from("500000.00 USD"),
                notional: Money::from("750050.00 USD"),
            }
            .to_string()
        )
    );
    assert_eq!(messages[1].event_type(), OrderEventType::Denied);
    assert_eq!(messages[2].event_type(), OrderEventType::Denied);
}

fn assert_max_notional_denied(
    event_handler: &TypedIntoMessageSavingHandler<OrderEventAny>,
    expected_notional: &str,
) {
    let messages = get_process_order_event_handler_messages(event_handler);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::NotionalExceedsMaxPerOrder {
                max_notional: Money::from("100000.00 USD"),
                notional: Money::from(expected_notional),
            }
            .to_string()
        )
    );
}

fn submit_market_order(
    risk_engine: &mut RiskEngine,
    instrument_id: InstrumentId,
    order_type: OrderType,
    order_side: OrderSide,
    quantity: &str,
    trader_id: TraderId,
    client_id: ClientId,
    strategy_id: StrategyId,
) {
    let order = OrderTestBuilder::new(order_type)
        .instrument_id(instrument_id)
        .side(order_side)
        .quantity(Quantity::from(quantity))
        .build();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id), false)
        .unwrap();
    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );
    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
}

fn market_bar(
    instrument_id: InstrumentId,
    price_type: PriceType,
    step: usize,
    close: &str,
    ts_init: u64,
) -> Bar {
    let price = Price::from(close);
    Bar::new(
        BarType::new(
            instrument_id,
            BarSpecification::new(step, BarAggregation::Minute, price_type),
            AggregationSource::External,
        ),
        price,
        price,
        price,
        price,
        Quantity::from("1"),
        UnixNanos::from(ts_init),
        UnixNanos::from(ts_init),
    )
}

#[rstest]
fn test_submit_order_when_less_than_min_notional_for_instrument_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_xbtusd_with_high_size_precision: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    consume_fixture(execute_order_event_handler);
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
        None, // correlation_id
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
        Ustr::from("NOTIONAL_BELOW_MINIMUM: min=1.00 USD, notional=0.90 USD")
    );
}

#[rstest]
#[case::not_reduce_only(false, true, true)]
#[case::reduce_only_with_position(true, true, false)]
#[case::reduce_only_without_position(true, false, false)]
fn test_submit_order_below_min_notional_respects_reduce_only(
    #[case] reduce_only: bool,
    #[case] include_position_id: bool,
    #[case] expect_denied: bool,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    let mut margin_account = margin_account_with_usdt_balance("100 USDT", "0 USDT", "100 USDT");
    margin_account.set_default_leverage(dec!(10));
    simple_cache
        .add_account(AccountAny::Margin(margin_account))
        .unwrap();

    let quote = QuoteTick::new(
        instrument_eth_usdt.id(),
        Price::from("5000.00"),
        Price::from("5000.00"),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    simple_cache.add_quote(quote).unwrap();

    let entry_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("0.002"))
        .build();
    let position_id = PositionId::from("P-MIN-NOTIONAL");
    let mut fill = order_filled(
        &entry_order,
        &instrument_eth_usdt,
        None,
        Some(AccountId::from("BINANCE-001")),
        Some(VenueOrderId::from("V-MIN-NOTIONAL")),
        None,
        None,
        Some(Price::from("5000.00")),
        None,
        None,
        None,
    );
    fill.position_id = Some(position_id);
    let position = Position::new(&instrument_eth_usdt, fill);
    assert_eq!(position.side, PositionSide::Short);
    assert_eq!(position.quantity, Quantity::from("0.002"));
    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.001"))
        .reduce_only(reduce_only)
        .build();
    let notional = instrument_eth_usdt
        .try_calculate_notional_value(order.quantity(), quote.ask_price, Some(true))
        .unwrap();

    assert_eq!(
        instrument_eth_usdt.min_quantity(),
        Some(Quantity::from("0.001"))
    );
    assert_eq!(
        instrument_eth_usdt.min_notional(),
        Some(Money::from("10.00 USDT"))
    );
    assert_eq!(notional, Money::from("5.00 USDT"));
    assert!(order.would_reduce_only(position.side, position.quantity));

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();

    let command_position_id = include_position_id.then_some(position_id);
    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        command_position_id,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);

    if expect_denied {
        assert_eq!(saved_process_messages.len(), 1);
        assert_eq!(
            saved_process_messages[0].client_order_id(),
            order.client_order_id()
        );
        assert_eq!(
            saved_process_messages[0].message().unwrap(),
            Ustr::from(
                "NOTIONAL_BELOW_MINIMUM: min=10.00000000 USDT, \
                 notional=5.00000000 USDT"
            )
        );
        assert!(saved_execute_messages.is_empty());
    } else {
        assert!(saved_process_messages.is_empty());
        assert_eq!(saved_execute_messages.len(), 1);
        let TradingCommand::SubmitOrder(forwarded) = &saved_execute_messages[0] else {
            panic!("Expected SubmitOrder command");
        };
        assert_eq!(forwarded.client_order_id, order.client_order_id());
        assert_eq!(forwarded.position_id, command_position_id);
        assert!(forwarded.order_init.reduce_only);
    }
}

#[derive(Debug, Clone, Copy)]
enum ClosePositionBound {
    MinQuantity,
    MaxQuantity,
    MaxQuantityCoinM,
    MinNotional,
    MaxNotionalConfigured,
    MaxNotionalInstrument,
}

#[rstest]
#[case::min_quantity(ClosePositionBound::MinQuantity)]
#[case::max_quantity(ClosePositionBound::MaxQuantity)]
#[case::max_quantity_coinm(ClosePositionBound::MaxQuantityCoinM)]
#[case::min_notional(ClosePositionBound::MinNotional)]
#[case::max_notional_configured(ClosePositionBound::MaxNotionalConfigured)]
#[case::max_notional_instrument(ClosePositionBound::MaxNotionalInstrument)]
fn test_submit_close_position_exempts_placeholder_bound(
    #[case] bound: ClosePositionBound,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    mut xbtusd_bitmex: CryptoPerpetual,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    let mut instrument = if matches!(bound, ClosePositionBound::MaxQuantityCoinM) {
        xbtusd_bitmex.id = InstrumentId::from("BTCUSD_PERP.BINANCE");
        xbtusd_bitmex.raw_symbol = Symbol::from("BTCUSD_PERP");
        InstrumentAny::CryptoPerpetual(xbtusd_bitmex)
    } else {
        instrument_eth_usdt
    };
    let InstrumentAny::CryptoPerpetual(crypto_perpetual) = &mut instrument else {
        unreachable!();
    };
    crypto_perpetual.min_quantity = None;
    crypto_perpetual.max_quantity = None;
    crypto_perpetual.min_notional = None;
    crypto_perpetual.max_notional = None;

    let quantity = match bound {
        ClosePositionBound::MinQuantity => {
            crypto_perpetual.min_quantity = Some(Quantity::from("2.000"));
            Quantity::from("1.000")
        }
        ClosePositionBound::MaxQuantity => {
            crypto_perpetual.max_quantity = Some(Quantity::from("1.000"));
            Quantity::from("2.000")
        }
        ClosePositionBound::MaxQuantityCoinM => {
            crypto_perpetual.max_quantity = Some(Quantity::from("1"));
            Quantity::from("2")
        }
        ClosePositionBound::MinNotional => {
            crypto_perpetual.min_notional = Some(Money::from("20.00 USDT"));
            Quantity::from("1.000")
        }
        ClosePositionBound::MaxNotionalConfigured => Quantity::from("2.000"),
        ClosePositionBound::MaxNotionalInstrument => {
            crypto_perpetual.max_notional = Some(Money::from("10.00 USDT"));
            Quantity::from("2.000")
        }
    };

    simple_cache.add_instrument(instrument.clone()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-CLOSE-POSITION");
    let position_side = if matches!(bound, ClosePositionBound::MaxNotionalConfigured) {
        PositionSide::Short
    } else {
        PositionSide::Long
    };
    add_position_for_close_position(
        &mut simple_cache,
        &instrument,
        quantity,
        position_id,
        position_side,
    );

    let mut risk_engine = get_risk_engine_for_full_position_exit(
        Some(Rc::new(RefCell::new(simple_cache))),
        instrument.id().venue,
    );

    if matches!(bound, ClosePositionBound::MaxNotionalConfigured) {
        risk_engine.set_max_notional_per_order(instrument.id(), dec!(10));
    }

    let order_type = if matches!(bound, ClosePositionBound::MaxNotionalConfigured) {
        OrderType::MarketIfTouched
    } else {
        OrderType::StopMarket
    };
    let order = OrderTestBuilder::new(order_type)
        .instrument_id(instrument.id())
        .side(match position_side {
            PositionSide::Long => OrderSide::Sell,
            PositionSide::Short => OrderSide::Buy,
            _ => unreachable!(),
        })
        .quantity(quantity)
        .trigger_price(Price::from("10"))
        .build();
    assert!(order.would_reduce_only(position_side, quantity));

    risk_engine
        .cache()
        .borrow_mut()
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        Some(position_id),
        Some(close_position_params(true)),
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert!(process_messages.is_empty());
    assert_eq!(execute_messages.len(), 1);
    let TradingCommand::SubmitOrder(forwarded) = &execute_messages[0] else {
        panic!("Expected SubmitOrder command");
    };
    assert_eq!(forwarded.client_order_id, order.client_order_id());
    assert_eq!(forwarded.position_id, Some(position_id));
    assert_eq!(
        forwarded
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAMS_CLOSE_POSITION)),
        Some(true)
    );
}

#[derive(Debug, Clone, Copy)]
enum InvalidClosePositionShape {
    ClosePositionFalse,
    VenueNotAllowlisted,
    OtherVenue,
    SpotInstrument,
    InversePerpetualContract,
    OrderInstrumentMismatch,
    UnsupportedOrderType,
    ReduceOnly,
    MissingPositionId,
    PositionNotFound,
    OrderPositionMismatch,
    PositionClosed,
    PositionInstrumentMismatch,
    WrongSide,
    QuantityExceedsPosition,
}

#[derive(Debug, Clone, Copy)]
enum InvalidClosePositionValue {
    QuantityPrecision,
    TriggerPrecision,
    TriggerNonPositive,
}

#[rstest]
#[case::close_position_false(InvalidClosePositionShape::ClosePositionFalse)]
#[case::venue_not_allowlisted(InvalidClosePositionShape::VenueNotAllowlisted)]
#[case::other_venue(InvalidClosePositionShape::OtherVenue)]
#[case::spot_instrument(InvalidClosePositionShape::SpotInstrument)]
#[case::inverse_perpetual_contract(InvalidClosePositionShape::InversePerpetualContract)]
#[case::order_instrument_mismatch(InvalidClosePositionShape::OrderInstrumentMismatch)]
#[case::unsupported_order_type(InvalidClosePositionShape::UnsupportedOrderType)]
#[case::reduce_only(InvalidClosePositionShape::ReduceOnly)]
#[case::missing_position_id(InvalidClosePositionShape::MissingPositionId)]
#[case::position_not_found(InvalidClosePositionShape::PositionNotFound)]
#[case::order_position_mismatch(InvalidClosePositionShape::OrderPositionMismatch)]
#[case::position_closed(InvalidClosePositionShape::PositionClosed)]
#[case::position_instrument_mismatch(InvalidClosePositionShape::PositionInstrumentMismatch)]
#[case::wrong_side(InvalidClosePositionShape::WrongSide)]
#[case::quantity_exceeds_position(InvalidClosePositionShape::QuantityExceedsPosition)]
fn test_submit_invalid_close_position_shape_does_not_bypass_max_quantity(
    #[case] shape: InvalidClosePositionShape,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    currency_pair_btcusdt: CurrencyPair,
    mut perpetual_contract_eurusd: PerpetualContract,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    let mut instrument = match shape {
        InvalidClosePositionShape::SpotInstrument => {
            InstrumentAny::CurrencyPair(currency_pair_btcusdt.clone())
        }
        InvalidClosePositionShape::InversePerpetualContract => {
            perpetual_contract_eurusd.id = InstrumentId::from("EURUSD-PERP.BINANCE");
            perpetual_contract_eurusd.is_inverse = true;
            InstrumentAny::PerpetualContract(perpetual_contract_eurusd)
        }
        _ => instrument_eth_usdt,
    };

    match &mut instrument {
        InstrumentAny::CryptoPerpetual(instrument) => {
            if matches!(shape, InvalidClosePositionShape::OtherVenue) {
                instrument.id = InstrumentId::from("ETHUSDT-PERP.BITMEX");
            }
            instrument.min_quantity = None;
            instrument.max_quantity = Some(Quantity::from("1"));
            instrument.min_notional = None;
            instrument.max_notional = None;
        }
        InstrumentAny::CurrencyPair(instrument) => {
            instrument.min_quantity = None;
            instrument.max_quantity = Some(Quantity::from("1"));
            instrument.min_notional = None;
            instrument.max_notional = None;
        }
        InstrumentAny::PerpetualContract(instrument) => {
            instrument.min_quantity = None;
            instrument.max_quantity = Some(Quantity::from("1"));
            instrument.min_notional = None;
            instrument.max_notional = None;
        }
        _ => unreachable!(),
    }

    simple_cache.add_instrument(instrument.clone()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-INVALID-CLOSE-POSITION");
    let position_quantity = if matches!(shape, InvalidClosePositionShape::QuantityExceedsPosition) {
        Quantity::from("1")
    } else {
        Quantity::from("2")
    };
    let position_instrument =
        if matches!(shape, InvalidClosePositionShape::PositionInstrumentMismatch) {
            InstrumentAny::CurrencyPair(currency_pair_btcusdt)
        } else {
            instrument.clone()
        };
    add_position_for_close_position(
        &mut simple_cache,
        &position_instrument,
        position_quantity,
        position_id,
        PositionSide::Long,
    );

    if matches!(shape, InvalidClosePositionShape::PositionClosed) {
        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(position_quantity)
            .build();
        let mut close_fill = order_filled(
            &close_order,
            &instrument,
            None,
            Some(AccountId::from("BINANCE-001")),
            Some(VenueOrderId::from("V-CLOSE-POSITION-CLOSED")),
            None,
            None,
            Some(Price::from("10.00")),
            None,
            None,
            None,
        );
        close_fill.position_id = Some(position_id);
        close_fill.trade_id = TradeId::from("E-CLOSE-POSITION-CLOSED");
        let mut position = simple_cache.position_mut(&position_id).unwrap();
        position.apply(&close_fill);
        assert!(position.is_closed());
    }

    let command_instrument_id =
        if matches!(shape, InvalidClosePositionShape::OrderInstrumentMismatch) {
            let mut command_instrument = instrument.clone();
            let InstrumentAny::CryptoPerpetual(command_instrument) = &mut command_instrument else {
                unreachable!();
            };
            command_instrument.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
            let command_instrument_id = command_instrument.id;
            simple_cache
                .add_instrument(InstrumentAny::CryptoPerpetual(command_instrument.clone()))
                .unwrap();
            command_instrument_id
        } else {
            instrument.id()
        };

    let cache = Some(Rc::new(RefCell::new(simple_cache)));
    let mut risk_engine = if matches!(shape, InvalidClosePositionShape::VenueNotAllowlisted) {
        get_risk_engine(cache, None, None, false)
    } else {
        get_risk_engine_for_full_position_exit(cache, Venue::from("BINANCE"))
    };
    let order_side = if matches!(shape, InvalidClosePositionShape::WrongSide) {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };
    let order = if matches!(shape, InvalidClosePositionShape::UnsupportedOrderType) {
        OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(instrument.id())
            .side(order_side)
            .quantity(Quantity::from("2"))
            .price(Price::from("9.00"))
            .trigger_price(Price::from("10.00"))
            .build()
    } else {
        OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .side(order_side)
            .quantity(Quantity::from("2"))
            .trigger_price(Price::from("10.00"))
            .reduce_only(matches!(shape, InvalidClosePositionShape::ReduceOnly))
            .build()
    };
    let order_position_id = if matches!(shape, InvalidClosePositionShape::OrderPositionMismatch) {
        PositionId::from("P-OTHER")
    } else {
        position_id
    };
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(
            order.clone(),
            Some(order_position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();

    let command_position_id = match shape {
        InvalidClosePositionShape::MissingPositionId => None,
        InvalidClosePositionShape::PositionNotFound => Some(PositionId::from("P-NOT-FOUND")),
        _ => Some(position_id),
    };
    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        command_instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        command_position_id,
        Some(close_position_params(!matches!(
            shape,
            InvalidClosePositionShape::ClosePositionFalse
        ))),
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::QuantityExceedsMaximum {
                effective_quantity: Quantity::from("2"),
                max_quantity: Quantity::from("1"),
            }
            .to_string()
        )
    );
    assert!(execute_messages.is_empty());
}

#[rstest]
fn test_submit_close_position_order_list_does_not_bypass_max_quantity(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    mut instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    let InstrumentAny::CryptoPerpetual(instrument) = &mut instrument_eth_usdt else {
        unreachable!();
    };
    instrument.min_quantity = None;
    instrument.max_quantity = Some(Quantity::from("1.000"));
    instrument.min_notional = None;
    instrument.max_notional = None;

    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-CLOSE-POSITION-LIST");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );

    let mut risk_engine = get_risk_engine_for_full_position_exit(
        Some(Rc::new(RefCell::new(simple_cache))),
        instrument_eth_usdt.id().venue,
    );
    let order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("2.000"))
        .trigger_price(Price::from("10.00"))
        .build();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("L-CLOSE-POSITION"),
        instrument_eth_usdt.id(),
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
        Some(position_id),
        Some(close_position_params(true)),
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(
        process_messages[0].client_order_id(),
        order.client_order_id()
    );
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::QuantityExceedsMaximum {
                effective_quantity: Quantity::from("2.000"),
                max_quantity: Quantity::from("1.000"),
            }
            .to_string()
        )
    );
    assert!(execute_messages.is_empty());
}

#[rstest]
#[case::quantity_precision("2.0000", "10.00", InvalidClosePositionValue::QuantityPrecision)]
#[case::trigger_precision("2.000", "10.000", InvalidClosePositionValue::TriggerPrecision)]
#[case::trigger_non_positive("2.000", "-1.00", InvalidClosePositionValue::TriggerNonPositive)]
fn test_submit_close_position_preserves_quantity_and_trigger_checks(
    #[case] quantity: &str,
    #[case] trigger_price: &str,
    #[case] invalid_value: InvalidClosePositionValue,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    mut instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    let InstrumentAny::CryptoPerpetual(instrument) = &mut instrument_eth_usdt else {
        unreachable!();
    };
    instrument.min_quantity = None;
    instrument.max_quantity = None;
    instrument.min_notional = None;
    instrument.max_notional = None;

    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-CLOSE-POSITION-PRECISION");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );

    let mut risk_engine = get_risk_engine_for_full_position_exit(
        Some(Rc::new(RefCell::new(simple_cache))),
        instrument_eth_usdt.id().venue,
    );
    let order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(quantity))
        .trigger_price(Price::from(trigger_price))
        .build();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();

    let submit_order = SubmitOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_eth_usdt.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        Some(position_id),
        Some(close_position_params(true)),
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    let expected_reason = match invalid_value {
        InvalidClosePositionValue::QuantityPrecision => {
            OrderDeniedReason::QuantityPrecisionExceedsMaximum {
                quantity: order.quantity(),
                quantity_precision: order.quantity().precision,
                max_precision: instrument_eth_usdt.size_precision(),
            }
        }
        InvalidClosePositionValue::TriggerPrecision => {
            OrderDeniedReason::PricePrecisionExceedsMaximum {
                field: OrderPriceField::TriggerPrice,
                price: order.trigger_price().unwrap(),
                price_precision: order.trigger_price().unwrap().precision,
                max_precision: instrument_eth_usdt.price_precision(),
            }
        }
        InvalidClosePositionValue::TriggerNonPositive => OrderDeniedReason::PriceNotPositive {
            field: OrderPriceField::TriggerPrice,
            price: order.trigger_price().unwrap(),
        },
    };
    assert_eq!(
        process_messages[0].message().unwrap(),
        Ustr::from(&expected_reason.to_string())
    );
    assert!(execute_messages.is_empty());
}

fn add_margin_account_for_close_position(cache: &mut Cache) {
    let mut account = margin_account_with_usdt_balance("1000000 USDT", "0 USDT", "1000000 USDT");
    account.set_default_leverage(dec!(10));
    cache.add_account(AccountAny::Margin(account)).unwrap();
}

fn add_position_for_close_position(
    cache: &mut Cache,
    instrument: &InstrumentAny,
    quantity: Quantity,
    position_id: PositionId,
    position_side: PositionSide,
) {
    let entry_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(match position_side {
            PositionSide::Long => OrderSide::Buy,
            PositionSide::Short => OrderSide::Sell,
            _ => unreachable!(),
        })
        .quantity(quantity)
        .build();
    let mut fill = order_filled(
        &entry_order,
        instrument,
        None,
        Some(AccountId::from("BINANCE-001")),
        Some(VenueOrderId::from("V-CLOSE-POSITION")),
        None,
        None,
        Some(Price::from("10")),
        None,
        None,
        None,
    );
    fill.position_id = Some(position_id);
    let position = Position::new(instrument, fill);
    assert_eq!(position.side, position_side);
    assert_eq!(position.quantity, quantity);
    cache.add_position(&position, OmsType::Hedging).unwrap();
}

fn close_position_params(close_position: bool) -> Params {
    let mut params = Params::new();
    params.insert(PARAMS_CLOSE_POSITION.to_string(), close_position.into());
    params
}

fn emergency_exit_params(emergency_exit: bool) -> Params {
    let mut params = Params::new();
    params.insert(PARAMS_EMERGENCY_EXIT.to_string(), emergency_exit.into());
    params
}

fn emergency_exit_quote(instrument_id: InstrumentId) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from("10.00"),
        Price::from("10.01"),
        Quantity::from("100.000"),
        Quantity::from("100.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn emergency_exit_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    quantity: Quantity,
    reduce_only: bool,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(side)
        .quantity(quantity)
        .reduce_only(reduce_only)
        .build()
}

fn emergency_exit_command(
    trader_id: TraderId,
    client_id: ClientId,
    strategy_id: StrategyId,
    order: &OrderAny,
    position_id: Option<PositionId>,
    emergency_exit: bool,
    ts_init: UnixNanos,
) -> SubmitOrder {
    SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order.instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        position_id,
        Some(emergency_exit_params(emergency_exit)),
        UUID4::new(),
        ts_init,
        None,
    )
}

#[rstest]
fn test_submit_emergency_exit_when_trading_halted_forwards_to_execution(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-EMERGENCY-EXIT");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );
    let order = emergency_exit_order(
        instrument_eth_usdt.id(),
        ClientOrderId::from("O-EMERGENCY-EXIT"),
        OrderSide::Sell,
        Quantity::from("1.000"),
        true,
    );
    simple_cache
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();
    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let command = emergency_exit_command(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        &order,
        Some(position_id),
        true,
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrder(command));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert!(process_messages.is_empty());
    assert_eq!(execute_messages.len(), 1);
    let TradingCommand::SubmitOrder(forwarded) = &execute_messages[0] else {
        panic!("Expected SubmitOrder command");
    };
    assert_eq!(forwarded.client_order_id, order.client_order_id());
    assert_eq!(forwarded.position_id, Some(position_id));
    assert!(forwarded.order_init.reduce_only);
    assert_eq!(
        forwarded
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAMS_EMERGENCY_EXIT)),
        Some(true)
    );
}

#[rstest]
#[case::active(false)]
#[case::bypass(true)]
fn test_submit_order_strips_caller_emergency_exit_param(
    #[case] bypass: bool,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-EMERGENCY-EXIT-PROVENANCE");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );
    let order = emergency_exit_order(
        instrument_eth_usdt.id(),
        ClientOrderId::from("O-EMERGENCY-EXIT-PROVENANCE"),
        OrderSide::Sell,
        Quantity::from("1.000"),
        true,
    );
    simple_cache
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();
    let mut risk_engine = get_risk_engine(
        Some(Rc::new(RefCell::new(simple_cache))),
        Some(RiskEngineConfig {
            bypass,
            ..RiskEngineConfig::default()
        }),
        None,
        false,
    );
    let command = emergency_exit_command(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        &order,
        Some(position_id),
        true,
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.execute(TradingCommand::SubmitOrder(command));

    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(execute_messages.len(), 1);
    let TradingCommand::SubmitOrder(forwarded) = &execute_messages[0] else {
        panic!("Expected SubmitOrder command");
    };
    assert!(
        forwarded
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAMS_EMERGENCY_EXIT))
            .is_none()
    );
}

#[rstest]
#[case::submitted_quantity_exceeds_position("10.000", "6.000", "4.000", false)]
#[case::submitted_quantity_equals_position("4.000", "0.000", "4.000", true)]
fn test_submit_emergency_exit_uses_submitted_quantity_bound(
    #[case] submitted_quantity: &str,
    #[case] filled_quantity: &str,
    #[case] position_quantity: &str,
    #[case] should_forward: bool,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-EMERGENCY-EXIT-QUANTITY");
    let position_quantity = Quantity::from(position_quantity);
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        position_quantity,
        position_id,
        PositionSide::Long,
    );
    let mut order = emergency_exit_order(
        instrument_eth_usdt.id(),
        ClientOrderId::from("O-EMERGENCY-EXIT-QUANTITY"),
        OrderSide::Sell,
        Quantity::from(submitted_quantity),
        true,
    );
    let filled_quantity = Quantity::from(filled_quantity);
    if filled_quantity.is_positive() {
        order
            .apply(OrderEventAny::Accepted(order_accepted(
                &order,
                Some(VenueOrderId::from("V-EMERGENCY-EXIT-QUANTITY")),
                Some(AccountId::from("BINANCE-001")),
            )))
            .unwrap();
        let fill = order_filled(
            &order,
            &instrument_eth_usdt,
            None,
            Some(AccountId::from("BINANCE-001")),
            Some(VenueOrderId::from("V-EMERGENCY-EXIT-QUANTITY")),
            None,
            Some(filled_quantity),
            Some(Price::from("10")),
            None,
            None,
            None,
        );
        order.apply(OrderEventAny::Filled(fill)).unwrap();
    }
    assert_eq!(order.quantity(), Quantity::from(submitted_quantity));
    assert_eq!(order.filled_qty(), filled_quantity);
    assert_eq!(order.leaves_qty(), position_quantity);
    assert!(order.would_reduce_only(PositionSide::Long, position_quantity));
    simple_cache
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();
    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let command = emergency_exit_command(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        &order,
        Some(position_id),
        true,
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrder(command));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);

    if should_forward {
        assert!(process_messages.is_empty());
        assert_eq!(execute_messages.len(), 1);
    } else {
        // `deny_order` returns early for an order past `Initialized`, so a partially filled
        // order is blocked without an `OrderDenied` event; not reaching execution is the assertion.
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert!(process_messages.is_empty());
        assert!(execute_messages.is_empty());
    }
}

#[derive(Debug, Clone, Copy)]
enum InvalidEmergencyExit {
    WrongSide,
    QuantityExceedsPosition,
    MissingPositionId,
    MismatchedCachedPosition,
    MissingIntent,
    NotReduceOnly,
}

#[rstest]
#[case::wrong_side(InvalidEmergencyExit::WrongSide)]
#[case::quantity_exceeds_position(InvalidEmergencyExit::QuantityExceedsPosition)]
#[case::missing_position_id(InvalidEmergencyExit::MissingPositionId)]
#[case::mismatched_cached_position(InvalidEmergencyExit::MismatchedCachedPosition)]
#[case::missing_intent(InvalidEmergencyExit::MissingIntent)]
#[case::not_reduce_only(InvalidEmergencyExit::NotReduceOnly)]
fn test_submit_invalid_emergency_exit_when_trading_halted_denies(
    #[case] invalid: InvalidEmergencyExit,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-INVALID-EMERGENCY-EXIT");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );
    let order = emergency_exit_order(
        instrument_eth_usdt.id(),
        ClientOrderId::from("O-INVALID-EMERGENCY-EXIT"),
        if matches!(invalid, InvalidEmergencyExit::WrongSide) {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        },
        if matches!(invalid, InvalidEmergencyExit::QuantityExceedsPosition) {
            Quantity::from("3.000")
        } else {
            Quantity::from("1.000")
        },
        !matches!(invalid, InvalidEmergencyExit::NotReduceOnly),
    );
    simple_cache
        .add_order(
            order.clone(),
            Some(
                if matches!(invalid, InvalidEmergencyExit::MismatchedCachedPosition) {
                    PositionId::from("P-OTHER-EMERGENCY-EXIT")
                } else {
                    position_id
                },
            ),
            Some(client_id_binance),
            false,
        )
        .unwrap();
    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let command = emergency_exit_command(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        &order,
        (!matches!(invalid, InvalidEmergencyExit::MissingPositionId)).then_some(position_id),
        !matches!(invalid, InvalidEmergencyExit::MissingIntent),
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrder(command));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
    // Wrong side and excess quantity are caught by the reduce-only pre-check
    // before classification; the remaining cases reach the gateway and keep
    // the ordinary halted denial.
    let expected_reason = match invalid {
        InvalidEmergencyExit::WrongSide | InvalidEmergencyExit::QuantityExceedsPosition => {
            "REDUCE_ONLY_WOULD_INCREASE_POSITION"
        }
        InvalidEmergencyExit::MissingPositionId
        | InvalidEmergencyExit::MismatchedCachedPosition
        | InvalidEmergencyExit::MissingIntent
        | InvalidEmergencyExit::NotReduceOnly => "TRADING_HALTED",
    };
    assert!(
        process_messages[0]
            .message()
            .unwrap()
            .starts_with(expected_reason)
    );
    assert!(execute_messages.is_empty());
}

#[rstest]
fn test_submit_order_list_strips_caller_emergency_exit_param(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-EMERGENCY-EXIT-LIST-PROVENANCE");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );
    let order = emergency_exit_order(
        instrument_eth_usdt.id(),
        ClientOrderId::from("O-EMERGENCY-EXIT-LIST-PROVENANCE"),
        OrderSide::Sell,
        Quantity::from("1.000"),
        true,
    );
    simple_cache
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            true,
        )
        .unwrap();
    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order_list = OrderList::new(
        OrderListId::new("EMERGENCY-EXIT-LIST-PROVENANCE"),
        instrument_eth_usdt.id(),
        strategy_id_ema_cross,
        vec![order.client_order_id()],
        risk_engine.clock().borrow().timestamp_ns(),
    );
    let command = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        vec![order.init_event().clone()],
        None,
        Some(position_id),
        Some(emergency_exit_params(true)),
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(command));

    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(execute_messages.len(), 1);
    let TradingCommand::SubmitOrderList(forwarded) = &execute_messages[0] else {
        panic!("Expected SubmitOrderList command");
    };
    assert!(
        forwarded
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAMS_EMERGENCY_EXIT))
            .is_none()
    );
}

#[rstest]
fn test_submit_emergency_exit_order_list_when_trading_halted_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance()).unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-EMERGENCY-EXIT-LIST");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );
    let order = emergency_exit_order(
        instrument_eth_usdt.id(),
        ClientOrderId::from("O-EMERGENCY-EXIT-LIST"),
        OrderSide::Sell,
        Quantity::from("1.000"),
        true,
    );
    simple_cache
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            true,
        )
        .unwrap();
    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order_list = OrderList::new(
        OrderListId::new("EMERGENCY-EXIT-LIST"),
        instrument_eth_usdt.id(),
        strategy_id_ema_cross,
        vec![order.client_order_id()],
        risk_engine.clock().borrow().timestamp_ns(),
    );
    let command = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        vec![order.init_event().clone()],
        None,
        Some(position_id),
        Some(emergency_exit_params(true)),
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrderList(command));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(
        process_messages[0].message(),
        Some(Ustr::from("TRADING_HALTED"))
    );
    assert!(execute_messages.is_empty());
}

#[rstest]
fn test_submit_emergency_exit_when_over_max_notional_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    mut instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    let InstrumentAny::CryptoPerpetual(instrument) = &mut instrument_eth_usdt else {
        unreachable!();
    };
    instrument.max_notional = None;
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache
        .add_quote(emergency_exit_quote(instrument_eth_usdt.id()))
        .unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-EMERGENCY-EXIT-NOTIONAL");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );
    let order = emergency_exit_order(
        instrument_eth_usdt.id(),
        ClientOrderId::from("O-EMERGENCY-EXIT-NOTIONAL"),
        OrderSide::Sell,
        Quantity::from("1.000"),
        true,
    );
    simple_cache
        .add_order(
            order.clone(),
            Some(position_id),
            Some(client_id_binance),
            false,
        )
        .unwrap();
    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    risk_engine.set_max_notional_per_order(instrument_eth_usdt.id(), dec!(1));
    let command = emergency_exit_command(
        trader_id,
        client_id_binance,
        strategy_id_ema_cross,
        &order,
        Some(position_id),
        true,
        risk_engine.clock().borrow().timestamp_ns(),
    );

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrder(command));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(process_messages.len(), 1);
    assert!(
        process_messages[0]
            .message()
            .unwrap()
            .starts_with("NOTIONAL_EXCEEDS_MAX_PER_ORDER")
    );
    assert!(execute_messages.is_empty());
}

#[rstest]
fn test_submit_emergency_exit_when_over_rate_limit_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache
        .add_quote(emergency_exit_quote(instrument_eth_usdt.id()))
        .unwrap();
    add_margin_account_for_close_position(&mut simple_cache);
    let position_id = PositionId::from("P-EMERGENCY-EXIT-RATE");
    add_position_for_close_position(
        &mut simple_cache,
        &instrument_eth_usdt,
        Quantity::from("2.000"),
        position_id,
        PositionSide::Long,
    );
    let orders = (0..11)
        .map(|index| {
            emergency_exit_order(
                instrument_eth_usdt.id(),
                ClientOrderId::new(format!("O-EMERGENCY-EXIT-RATE-{index}")),
                OrderSide::Sell,
                Quantity::from("1.000"),
                true,
            )
        })
        .collect::<Vec<_>>();

    for order in &orders {
        simple_cache
            .add_order(
                order.clone(),
                Some(position_id),
                Some(client_id_binance),
                false,
            )
            .unwrap();
    }
    let config = RiskEngineConfig {
        debug: true,
        bypass: false,
        max_order_submit: RateLimit::new(10, 1000),
        max_order_modify: RateLimit::new(5, 1000),
        max_notional_per_order: AHashMap::new(),
        full_position_exit_venues: AHashSet::new(),
    };
    let mut risk_engine = get_risk_engine(
        Some(Rc::new(RefCell::new(simple_cache))),
        Some(config),
        None,
        false,
    );
    risk_engine.set_trading_state(TradingState::Halted);

    for order in &orders {
        let command = emergency_exit_command(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            order,
            Some(position_id),
            true,
            risk_engine.clock().borrow().timestamp_ns(),
        );
        risk_engine.execute(TradingCommand::SubmitOrder(command));
    }

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(execute_messages.len(), 10);
    assert_eq!(process_messages.len(), 1);
    assert_eq!(
        process_messages[0].message(),
        Some(Ustr::from("RATE_LIMIT_EXCEEDED"))
    );
}

#[rstest]
fn test_submit_order_when_greater_than_max_notional_for_instrument_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_xbtusd_bitmex: InstrumentAny,
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
        Decimal::from_i64(100_000_000).unwrap(),
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
        None, // correlation_id
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
            "NOTIONAL_EXCEEDS_MAXIMUM: max=10000000.00 USD, \
             notional=10000001.00 USD"
        )
    );
}

#[rstest]
fn test_submit_order_when_buy_market_order_and_over_max_notional_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100_000).unwrap());

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
        None, // correlation_id
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
        Ustr::from("NOTIONAL_EXCEEDS_MAX_PER_ORDER: max=100000.00 USD, notional=750050.00 USD")
    );
}

#[rstest]
fn test_submit_order_when_notional_is_unrepresentable_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_audusd.id(),
            Price::from("1000000000.00000"),
            Price::from("1000000000.00000"),
            Quantity::from("1000000"),
            Quantity::from("1000000"),
            UnixNanos::default(),
            UnixNanos::default(),
        ))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1000000"))
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
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved = get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].event_type(), OrderEventType::Denied);
    assert!(
        saved[0]
            .message()
            .unwrap()
            .as_str()
            .starts_with("NOTIONAL_CALCULATION_FAILED:")
    );
}

#[rstest]
fn test_submit_order_when_sell_market_order_and_over_max_notional_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100_000).unwrap());

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
        None, // correlation_id
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
        Ustr::from("NOTIONAL_EXCEEDS_MAX_PER_ORDER: max=100000.00 USD, notional=750000.00 USD")
    );
}

#[rstest]
fn test_submit_order_when_market_order_and_over_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
            "NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, \
             notional=10100000.00 USD"
        )
    );
}

#[rstest]
fn test_submit_order_reduce_only_buy_over_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None, // correlation_id
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
            "NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, \
             notional=10100000.00 USD"
        )
    );
}

#[rstest]
fn test_submit_order_reduce_only_buy_within_free_balance_then_sends_to_execution(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("1000").unwrap())
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
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None, // correlation_id
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
fn test_submit_order_when_market_order_over_free_balance_with_borrowing_enabled_then_accepts(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
            "CUMULATIVE_NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, \
             notional=1067873.00 USD"
        )
    );
}

#[rstest]
fn test_submit_order_list_sells_when_over_free_balance_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
            "CUMULATIVE_NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, \
             notional=1057300.00 USD"
        )
    );
}

#[rstest]
fn test_submit_order_when_trading_halted_then_denies_order(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance()).unwrap();

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
        None, // correlation_id
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
        Ustr::from("TRADING_HALTED")
    );
}

// `allow` not `expect`: nightly clippy does not fire `float_cmp` inside `assert_eq!`
#[allow(
    clippy::float_cmp,
    reason = "throttler usage is an integer counter represented as f64"
)]
#[rstest]
fn test_submit_order_beyond_rate_limit_then_denies_order(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();
    simple_cache.add_quote(quote_audusd()).unwrap();

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
            None, // correlation_id
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
        Ustr::from("RATE_LIMIT_EXCEEDED")
    );
}

#[rstest]
fn test_submit_order_list_when_trading_halted_then_denies_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_audusd.clone())
        .unwrap();
    simple_cache.add_quote(quote_audusd()).unwrap();

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
        None, // correlation_id
    );

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // Get messages and test
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(event.message().unwrap(), Ustr::from("TRADING_HALTED"));
    }
}

#[rstest]
fn test_submit_order_list_denies_when_non_representative_instrument_missing(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    let instrument_a: InstrumentAny = audusd_sim.into();
    let instrument_b: InstrumentAny = gbpusd_sim.into();

    // Only register the representative instrument; non-representative is missing.
    simple_cache.add_instrument(instrument_a.clone()).unwrap();
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let order_a = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_a.id())
        .client_order_id(ClientOrderId::from("O-MISS-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();
    let order_b = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_b.id())
        .client_order_id(ClientOrderId::from("O-MISS-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let orders = [order_a.clone(), order_b.clone()];
    for order in &orders {
        simple_cache
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order_list = OrderList::new(
        OrderListId::new("L-MISS-001"),
        instrument_a.id(),
        StrategyId::new("S-001"),
        vec![order_a.client_order_id(), order_b.client_order_id()],
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
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    let saved = get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved.len(), orders.len());
    for event in &saved {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        let msg = event.message().unwrap();
        assert!(
            msg.as_str().contains("INSTRUMENT_NOT_FOUND")
                && msg.as_str().contains(&instrument_b.id().to_string()),
            "unexpected denial reason: {msg}",
        );
    }
}

#[rstest]
fn test_submit_order_list_denies_when_representative_instrument_not_in_list(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    let instrument_a: InstrumentAny = audusd_sim.into();
    let instrument_b: InstrumentAny = gbpusd_sim.into();
    let representative_id = InstrumentId::from("USD/JPY.SIM");

    simple_cache.add_instrument(instrument_a.clone()).unwrap();
    simple_cache.add_instrument(instrument_b.clone()).unwrap();

    let order_a = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_a.id())
        .client_order_id(ClientOrderId::from("O-REP-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();
    let order_b = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_b.id())
        .client_order_id(ClientOrderId::from("O-REP-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let orders = [order_a.clone(), order_b.clone()];
    for order in &orders {
        simple_cache
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order_list = OrderList::new(
        OrderListId::new("L-REP-001"),
        representative_id,
        StrategyId::new("S-001"),
        vec![order_a.client_order_id(), order_b.client_order_id()],
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
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    let saved = get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved.len(), orders.len());
    for event in &saved {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(
            event.message().unwrap(),
            Ustr::from("INSTRUMENT_NOT_FOUND: USD/JPY.SIM")
        );
    }
}

#[rstest]
fn test_submit_order_list_check_order_uses_each_orders_own_instrument(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    let instrument_a: InstrumentAny = audusd_sim.into();
    let instrument_b: InstrumentAny = gbpusd_sim.into();

    simple_cache.add_instrument(instrument_a.clone()).unwrap();
    simple_cache.add_instrument(instrument_b.clone()).unwrap();
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    // AUD/USD price precision is 5. A 6-decimal-place limit price violates instrument_a's
    // precision regardless of which instrument the risk engine looks up. The first order
    // uses a valid 5-dp price; the second order's price is 6-dp and must be denied because
    // its own instrument's precision is checked, not the representative's.
    let order_a = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_a.id())
        .client_order_id(ClientOrderId::from("O-PREC-001"))
        .side(OrderSide::Buy)
        .price(Price::from("0.50000"))
        .quantity(Quantity::from_str("100").unwrap())
        .build();
    let bad_price = Price::from("1.000001"); // 6-dp, exceeds GBP/USD 5-dp precision
    assert!(bad_price.precision > instrument_b.price_precision());
    let order_b = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_b.id())
        .client_order_id(ClientOrderId::from("O-PREC-002"))
        .side(OrderSide::Buy)
        .price(bad_price)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let orders = [order_a.clone(), order_b.clone()];
    for order in &orders {
        simple_cache
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

    let order_list = OrderList::new(
        OrderListId::new("L-PREC-001"),
        instrument_a.id(),
        StrategyId::new("S-001"),
        vec![order_a.client_order_id(), order_b.client_order_id()],
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
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    let saved = get_process_order_event_handler_messages(&process_order_event_handler);
    // The second order is denied for price precision; the first order is not denied
    // (no event emitted for the still-pending entry).
    assert!(saved.iter().any(|event| {
        event.event_type() == OrderEventType::Denied
            && event.client_order_id() == order_b.client_order_id()
    }));
    assert!(
        !saved.iter().any(|event| {
            event.event_type() == OrderEventType::Denied
                && event.client_order_id() == order_a.client_order_id()
        }),
        "first order should not be denied; check_order is per-order: {saved:?}",
    );
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
    instrument_xbtusd_bitmex: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
        None, // correlation_id
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
    instrument_xbtusd_bitmex: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
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
        None, // correlation_id
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
            Ustr::from("INSTRUMENT_NOT_FOUND: AUD/USD.SIM")
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 0);
}

// `allow` not `expect`: nightly clippy does not fire `float_cmp` inside `assert_eq!`
#[allow(
    clippy::float_cmp,
    reason = "throttler usage is an integer counter represented as f64"
)]
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

    let mut order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .trigger_price(Price::new(1.0001, 4))
        .build();

    order
        .apply(OrderEventAny::Submitted(order_submitted(&order)))
        .unwrap();

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
            Some(Price::new(1.00011 + f64::from(i) * 0.00001, 5)),
            None,
            UUID4::new(),
            risk_engine.clock().borrow().timestamp_ns(),
            None,
            None, // correlation_id
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
    assert_eq!(first_message.account_id(), Some(account_id()));
}

#[rstest]
fn test_modify_order_with_default_settings_then_sends_to_client(
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
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
        None, // correlation_id
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

// `allow` not `expect`: nightly clippy does not fire `float_cmp` inside `assert_eq!`
#[allow(
    clippy::float_cmp,
    reason = "throttler usage is an integer counter represented as f64"
)]
#[rstest]
fn test_batch_modify_orders_counts_each_child_against_rate_limit(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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

    let order1 = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-BATCH-MODIFY-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::new(1.0001, 4))
        .build();
    let order2 = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-BATCH-MODIFY-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("200").unwrap())
        .price(Price::new(1.0002, 4))
        .build();

    simple_cache
        .add_order(order1.clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(order2.clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let ts_init = risk_engine.clock().borrow().timestamp_ns();
    let modifies = vec![
        ModifyOrder::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            instrument_audusd.id(),
            order1.client_order_id(),
            order1.venue_order_id(),
            Some(Quantity::from_str("101").unwrap()),
            Some(Price::new(1.0003, 4)),
            None,
            UUID4::new(),
            ts_init,
            None,
            None, // correlation_id
        ),
        ModifyOrder::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            instrument_audusd.id(),
            order2.client_order_id(),
            order2.venue_order_id(),
            Some(Quantity::from_str("201").unwrap()),
            Some(Price::new(1.0004, 4)),
            None,
            UUID4::new(),
            ts_init,
            None,
            None, // correlation_id
        ),
    ];
    let command = BatchModifyOrders::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        modifies,
        UUID4::new(),
        ts_init,
        None,
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::ModifyOrders(command));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
    assert!(matches!(
        saved_execute_messages.first(),
        Some(TradingCommand::ModifyOrders(command)) if command.modifies.len() == 2
    ));
    assert_eq!(risk_engine.throttled_modify_order.recv_count(), 2);
    assert_eq!(risk_engine.throttled_modify_order.sent_count(), 2);
    assert_eq!(risk_engine.throttled_modify_order.used(), 0.4);
}

#[rstest]
fn test_batch_modify_orders_rejects_all_children_when_one_child_fails_validation(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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

    let order1 = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-BATCH-MODIFY-PARTIAL-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .price(Price::from("1.00010"))
        .build();
    let order2 = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .client_order_id(ClientOrderId::from("O-BATCH-MODIFY-PARTIAL-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("200").unwrap())
        .price(Price::from("1.00020"))
        .build();

    simple_cache
        .add_order(order1.clone(), None, Some(client_id_binance), true)
        .unwrap();
    simple_cache
        .add_order(order2.clone(), None, Some(client_id_binance), true)
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let ts_init = risk_engine.clock().borrow().timestamp_ns();
    let modifies = vec![
        ModifyOrder::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            instrument_audusd.id(),
            order1.client_order_id(),
            order1.venue_order_id(),
            Some(Quantity::from_str("101").unwrap()),
            Some(Price::from("1.00030")),
            None,
            UUID4::new(),
            ts_init,
            None,
            None, // correlation_id
        ),
        ModifyOrder::new(
            trader_id,
            Some(client_id_binance),
            strategy_id_ema_cross,
            instrument_audusd.id(),
            order2.client_order_id(),
            order2.venue_order_id(),
            Some(Quantity::from_str("201").unwrap()),
            Some(Price::from("1.000001")),
            None,
            UUID4::new(),
            ts_init,
            None,
            None, // correlation_id
        ),
    ];
    let command = BatchModifyOrders::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        modifies,
        UUID4::new(),
        ts_init,
        None,
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::ModifyOrders(command));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_process_messages.len(), 2);
    assert!(
        saved_process_messages
            .iter()
            .all(|event| { event.event_type() == OrderEventType::ModifyRejected })
    );
    assert!(saved_process_messages.iter().any(|event| {
        event.client_order_id() == order1.client_order_id()
            && event
                .message()
                .unwrap()
                .contains("one or more child modifications failed validation")
    }));
    assert!(
        saved_process_messages
            .iter()
            .any(|event| { event.client_order_id() == order2.client_order_id() })
    );
    assert_eq!(saved_execute_messages.len(), 0);
}

#[rstest]
fn test_modify_order_when_negative_price_for_commodity_then_allows(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_commodity: InstrumentAny,
    venue_order_id: VenueOrderId,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    cash_account_state_million_usd: AccountState,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
    simple_cache
        .add_instrument(instrument_commodity.clone())
        .unwrap();

    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd,
        )))
        .unwrap();

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_commodity.id())
        .side(OrderSide::Buy)
        .price(Price::new(5.0, 2))
        .quantity(Quantity::from("1"))
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
        instrument_commodity.id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None, // correlation_id
    );

    let modify_order = ModifyOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_commodity.id(),
        order.client_order_id(),
        Some(venue_order_id),
        Some(Quantity::from("1")),
        Some(Price::new(-5.0, 2)), // Negative price is valid for spot commodities
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 2);
    assert_eq!(
        saved_execute_messages.first().unwrap().instrument_id(),
        instrument_commodity.id()
    );
}

#[rstest]
fn test_modify_order_when_risk_bypassed_sends_to_execution_engine(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    client_order_id: ClientOrderId,
    instrument_audusd: InstrumentAny,
    venue_order_id: VenueOrderId,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
) {
    let mut risk_engine = get_risk_engine(None, None, None, true);

    // Order intentionally not in the cache: bypass skips all validation
    let modify_order = ModifyOrder::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        instrument_audusd.id(),
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("1000")),
        Some(Price::new(-100.0, 0)),
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
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
            Money::zero(gbp),
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
        None, // correlation_id
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
            Money::zero(gbp),
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
        None, // correlation_id
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
            Money::zero(gbp),
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
        None, // correlation_id
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
    instrument_xbtusd_bitmex: InstrumentAny,
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
        None, // correlation_id
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
    instrument_xbtusd_bitmex: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    bitmex_cash_account_state_multi: AccountState,
    mut simple_cache: Cache,
) {
    consume_fixture((process_order_event_handler, execute_order_event_handler));
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
        None, // correlation_id
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
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    // Create a BTCUSDT spot instrument with max_quantity = 83 BTC
    let btc_usdt = InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("BTCUSDT-SPOT.BYBIT"))
            .raw_symbol(Symbol::from("BTCUSDT"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USDT())
            .price_precision(1)
            .size_precision(6)
            .price_increment(Price::from("0.1"))
            .size_increment(Quantity::from("0.000001"))
            .multiplier(Quantity::from("1"))
            .lot_size(Quantity::from("0.000001"))
            // max_quantity = 83 BTC
            .max_quantity(Quantity::from("83"))
            .min_quantity(Quantity::from("0.000011"))
            .max_notional(Money::from("8000000 USDT"))
            .min_notional(Money::from("5 USDT"))
            .margin_init(dec!(0.1))
            .margin_maint(dec!(0.1))
            .maker_fee(dec!(-0.00005))
            .taker_fee(dec!(0.00015))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    );

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
        None, // correlation_id
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
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    // Base-quantity bounds do not apply to quote-denominated orders, so a
    // converted base quantity that would exceed `max_quantity` must still pass.
    let btc_usdt = InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("BTCUSDT-SPOT.BYBIT"))
            .raw_symbol(Symbol::from("BTCUSDT"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USDT())
            .price_precision(1)
            .size_precision(6)
            .price_increment(Price::from("0.1"))
            .size_increment(Quantity::from("0.000001"))
            .multiplier(Quantity::from("1"))
            .lot_size(Quantity::from("0.000001"))
            // max_quantity = 0.5 BTC
            .max_quantity(Quantity::from("0.5"))
            .min_quantity(Quantity::from("0.000011"))
            .max_notional(Money::from("8000000 USDT"))
            .min_notional(Money::from("5 USDT"))
            .margin_init(dec!(0.1))
            .margin_maint(dec!(0.1))
            .maker_fee(dec!(-0.00005))
            .taker_fee(dec!(0.00015))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    );

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
        None, // correlation_id
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
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    // Mirrors the Polymarket scenario from #3874: a quote-denominated order whose
    // converted base quantity falls below a large `min_quantity` must still pass.
    let btc_usdt = InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("BTCUSDT-SPOT.BYBIT"))
            .raw_symbol(Symbol::from("BTCUSDT"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USDT())
            .price_precision(1)
            .size_precision(6)
            .price_increment(Price::from("0.1"))
            .size_increment(Quantity::from("0.000001"))
            .multiplier(Quantity::from("1"))
            .lot_size(Quantity::from("0.000001"))
            // min_quantity = 5 base units
            .min_quantity(Quantity::from("5"))
            .min_notional(Money::from("1 USDT"))
            .margin_init(dec!(0.1))
            .margin_maint(dec!(0.1))
            .maker_fee(dec!(-0.00005))
            .taker_fee(dec!(0.00015))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    );

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
        None, // correlation_id
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
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    // Base-quantity bounds are skipped for quote-denominated orders, but
    // `min_notional` still applies and must deny sub-minimum notionals.
    let btc_usdt = InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("BTCUSDT-SPOT.BYBIT"))
            .raw_symbol(Symbol::from("BTCUSDT"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USDT())
            .price_precision(1)
            .size_precision(6)
            .price_increment(Price::from("0.1"))
            .size_increment(Quantity::from("0.000001"))
            .multiplier(Quantity::from("1"))
            .lot_size(Quantity::from("0.000001"))
            .min_notional(Money::from("10 USDT"))
            .margin_init(dec!(0.1))
            .margin_maint(dec!(0.1))
            .maker_fee(dec!(-0.00005))
            .taker_fee(dec!(0.00015))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    );

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
        None, // correlation_id
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
            .contains("NOTIONAL_BELOW_MINIMUM")
    );
}

// `allow` not `expect`: nightly clippy does not fire `float_cmp` inside `assert_eq!`
#[allow(
    clippy::float_cmp,
    reason = "throttler usage is an integer counter represented as f64"
)]
#[rstest]
fn test_submit_order_list_beyond_rate_limit_then_denies_all_orders(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
            None, // correlation_id
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
        None, // correlation_id
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
        Ustr::from("RATE_LIMIT_EXCEEDED")
    );
}

#[rstest]
fn test_submit_order_list_beyond_rate_limit_denies_all_orders_in_list(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_audusd: InstrumentAny,
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
            None, // correlation_id
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
        orders.iter().map(Order::client_order_id).collect(),
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

    // All 3 orders in the bracket should be denied
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
        assert_eq!(event.message().unwrap(), Ustr::from("RATE_LIMIT_EXCEEDED"));
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
        full_position_exit_venues: [Venue::from("BINANCE")].into_iter().collect(),
    };

    let mut risk_engine = get_risk_engine(None, Some(config), None, false);
    risk_engine.set_max_notional_per_order(
        InstrumentId::from("AUD/USD.SIM"),
        Decimal::from_i64(500_000).unwrap(),
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
    assert_eq!(event.config["full_position_exit_venues"], "BINANCE");
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
        full_position_exit_venues: AHashSet::new(),
    };

    let mut risk_engine = get_risk_engine(None, Some(config), None, false);

    risk_engine.set_trading_state(TradingState::Halted);
    risk_engine.set_max_notional_per_order(instrument_id, Decimal::from_i64(100_000).unwrap());

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
    instrument_audusd: InstrumentAny,
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
        orders.iter().map(Order::client_order_id).collect(),
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
        None, // correlation_id
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
#[case::unheld(None, "1.000", false, false, Some("0 ETH"), Some("1 ETH"))]
#[case::held_within_balance(Some("2 ETH"), "1.000", false, false, None, None)]
#[case::held_exceeding_balance(Some("2 ETH"), "3.000", false, false, Some("2 ETH"), Some("3 ETH"))]
#[case::borrowing_unheld(None, "1.000", true, false, None, None)]
#[case::reduce_only_unheld(None, "1.000", false, true, None, None)]
fn test_submit_order_cash_account_sell_checks_asset_balance(
    #[case] asset_balance: Option<&str>,
    #[case] quantity: &str,
    #[case] allow_borrowing: bool,
    #[case] reduce_only: bool,
    #[case] expected_free: Option<&str>,
    #[case] expected_cum_notional: Option<&str>,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    quote_ethusdt_binance: QuoteTick,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    let mut balances = vec![AccountBalance::new(
        Money::from("10000 USDT"),
        Money::from("0 USDT"),
        Money::from("10000 USDT"),
    )];

    if let Some(asset_balance) = asset_balance {
        let balance = Money::from(asset_balance);
        balances.push(AccountBalance::new(
            balance,
            Money::zero(balance.currency),
            balance,
        ));
    }
    let account_state = AccountState::new(
        AccountId::from("BINANCE-001"),
        AccountType::Cash,
        balances,
        vec![],
        true,
        UUID4::new(),
        UnixNanos::from(0),
        UnixNanos::from(0),
        None,
    );
    simple_cache
        .add_account(AccountAny::Cash(CashAccount::new(
            account_state,
            true,
            allow_borrowing,
        )))
        .unwrap();
    simple_cache.add_quote(quote_ethusdt_binance).unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(quantity))
        .reduce_only(reduce_only)
        .build();
    risk_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id_binance), false)
        .unwrap();
    let ts_init = risk_engine.clock().borrow().timestamp_ns();
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
        ts_init,
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let process_messages = get_process_order_event_handler_messages(&process_order_event_handler);
    let execute_messages = get_execute_order_event_handler_messages(&execute_order_event_handler);

    match (expected_free, expected_cum_notional) {
        (Some(expected_free), Some(expected_cum_notional)) => {
            assert_eq!(process_messages.len(), 1);
            assert_eq!(process_messages[0].event_type(), OrderEventType::Denied);
            assert_eq!(
                process_messages[0].message().unwrap(),
                Ustr::from(
                    &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                        free_balance: Money::from(expected_free),
                        cumulative_notional: Money::from(expected_cum_notional),
                    }
                    .to_string()
                )
            );
            assert_eq!(execute_messages.len(), 0);
        }
        (None, None) => {
            assert_eq!(process_messages.len(), 0);
            assert_eq!(execute_messages.len(), 1);
            assert_eq!(
                execute_messages[0].instrument_id(),
                instrument_eth_usdt.id()
            );
        }
        _ => unreachable!(),
    }
}

#[rstest]
#[case::buy(OrderSide::Buy)]
#[case::sell(OrderSide::Sell)]
fn test_submit_order_margin_account_within_free_balance(
    #[case] order_side: OrderSide,
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // ETHUSDT margin_init=1.0, 10x leverage: 1 ETH @ $3000 requires $300 margin
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
        .side(order_side)
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
        None, // correlation_id
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
    instrument_eth_usdt: InstrumentAny,
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
        None, // correlation_id
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
fn test_submit_order_when_initial_margin_is_unrepresentable_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    mut instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    let InstrumentAny::CryptoPerpetual(instrument) = &mut instrument_eth_usdt else {
        unreachable!();
    };
    instrument.margin_init = Decimal::MAX;

    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    simple_cache
        .add_account(AccountAny::Margin(margin_account_with_usdt_balance(
            "100000 USDT",
            "0 USDT",
            "100000 USDT",
        )))
        .unwrap();
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from("3000.00"),
            Price::from("3000.01"),
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::default(),
            UnixNanos::default(),
        ))
        .unwrap();

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
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved = get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].event_type(), OrderEventType::Denied);
    assert!(
        saved[0]
            .message()
            .unwrap()
            .as_str()
            .starts_with("INITIAL_MARGIN_CALCULATION_FAILED:")
    );
}

#[rstest]
fn test_submit_order_margin_account_sell_short_exceeds_free_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
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
        None, // correlation_id
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
    instrument_eth_usdt: InstrumentAny,
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
        None, // correlation_id
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
    instrument_eth_usdt: InstrumentAny,
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
        None, // correlation_id
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
fn test_submit_order_list_when_cumulative_initial_margin_exceeds_free_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();

    // Free = $500 USDT, 10x leverage
    // Each 1 ETH @ $3000.01 ask -> margin = $300.001
    // First order (1 ETH): cumulative initial margin = $300.001 < $500 -> passes
    // Second order (1 ETH): cumulative initial margin = $600.002 > $500 -> denied
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
        orders.iter().map(Order::client_order_id).collect(),
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    // 1 denial from check_orders_risk (2nd order) + 2 from deny_order_list (both orders)
    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 3);
    assert_eq!(
        saved_process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::CumulativeInitialMarginExceedsFreeBalance {
                free_balance: Money::from("500 USDT"),
                cumulative_initial_margin: Money::from("600.002 USDT"),
            }
            .to_string()
        )
    );

    for event in &saved_process_messages {
        assert_eq!(event.event_type(), OrderEventType::Denied);
    }
}

#[rstest]
fn test_submit_order_list_when_cumulative_initial_margin_is_unrepresentable_then_denies(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    mut instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    let InstrumentAny::CryptoPerpetual(instrument) = &mut instrument_eth_usdt else {
        unreachable!();
    };
    instrument.margin_init = Decimal::ONE;
    instrument.max_quantity = None;
    instrument.max_notional = None;

    simple_cache
        .add_instrument(instrument_eth_usdt.clone())
        .unwrap();
    let max_balance = format!("{MONEY_MAX:.0} USDT");
    simple_cache
        .add_account(AccountAny::Margin(margin_account_with_usdt_balance(
            &max_balance,
            "0 USDT",
            &max_balance,
        )))
        .unwrap();
    simple_cache
        .add_quote(QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from("1.00"),
            Price::from("1.00"),
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::default(),
            UnixNanos::default(),
        ))
        .unwrap();

    let mut risk_engine =
        get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
    let quantity = Quantity::new(MONEY_MAX * 0.75, 3);
    let orders = [
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .client_order_id(ClientOrderId::from("O-001"))
            .side(OrderSide::Buy)
            .quantity(quantity)
            .build(),
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .client_order_id(ClientOrderId::from("O-002"))
            .side(OrderSide::Buy)
            .quantity(quantity)
            .build(),
    ];

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
        orders.iter().map(Order::client_order_id).collect(),
        risk_engine.clock().borrow().timestamp_ns(),
    );
    let submit = SubmitOrderList::new(
        trader_id,
        Some(client_id_binance),
        strategy_id_ema_cross,
        order_list,
        orders
            .iter()
            .map(|order| order.init_event().clone())
            .collect(),
        None,
        None,
        None,
        UUID4::new(),
        risk_engine.clock().borrow().timestamp_ns(),
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    let saved = get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[0].event_type(), OrderEventType::Denied);
    assert_eq!(
        saved[0].message().unwrap(),
        Ustr::from("CUMULATIVE_INITIAL_MARGIN_CALCULATION_FAILED: total exceeds Money bounds")
    );
}

#[rstest]
fn test_submit_order_margin_account_limit_order_within_balance(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
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
        None, // correlation_id
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
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    consume_fixture(execute_order_event_handler);
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
        None, // correlation_id
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
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    consume_fixture(execute_order_event_handler);
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
        None, // correlation_id
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
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

#[rstest]
fn test_submit_order_list_reducing_uses_each_orders_own_instrument(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    mut simple_cache: Cache,
) {
    let instrument_a: InstrumentAny = audusd_sim.into();
    let instrument_b: InstrumentAny = gbpusd_sim.into();

    simple_cache.add_instrument(instrument_a.clone()).unwrap();
    simple_cache.add_instrument(instrument_b.clone()).unwrap();
    for instrument_id in [instrument_a.id(), instrument_b.id()] {
        simple_cache
            .add_quote(QuoteTick::new(
                instrument_id,
                Price::from("1.00000"),
                Price::from("1.00001"),
                Quantity::from("1"),
                Quantity::from("1"),
                UnixNanos::from(1),
                UnixNanos::from(1),
            ))
            .unwrap();
    }
    simple_cache
        .add_account(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )))
        .unwrap();

    // Open a LONG position only on instrument_b.
    let fill_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_b.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();
    let mut fill = order_filled(
        &fill_order,
        &instrument_b,
        None,
        Some(AccountId::from("SIM-001")),
        Some(VenueOrderId::from("V-REDUCE-001")),
        None,
        None,
        Some(Price::from("1.20000")),
        None,
        None,
        None,
    );
    fill.position_id = Some(PositionId::from("P-REDUCE-B"));
    let position = Position::new(&instrument_b, fill);
    simple_cache
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let cache = Rc::new(RefCell::new(simple_cache));
    let mut risk_engine = get_risk_engine(Some(cache), None, None, false);
    risk_engine.portfolio_mut().initialize_positions();
    risk_engine.set_trading_state(TradingState::Reducing);

    // Order on instrument_a should pass (no position on A). Order on instrument_b
    // is a BUY that would extend the existing LONG -> denied with B's instrument_id
    // in the reason. Representative is instrument_a; reverting to the representative
    // would let order_b through.
    let order_a = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_a.id())
        .client_order_id(ClientOrderId::from("O-REDUCE-001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();
    let order_b = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_b.id())
        .client_order_id(ClientOrderId::from("O-REDUCE-002"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from_str("100").unwrap())
        .build();

    let orders = [order_a.clone(), order_b.clone()];
    for order in &orders {
        risk_engine
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();
    }

    let order_list = OrderList::new(
        OrderListId::new("L-REDUCE-001"),
        instrument_a.id(),
        StrategyId::new("S-001"),
        vec![order_a.client_order_id(), order_b.client_order_id()],
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
        None,
    );

    risk_engine.execute(TradingCommand::SubmitOrderList(submit));

    let saved = get_process_order_event_handler_messages(&process_order_event_handler);
    assert!(
        !saved.is_empty(),
        "REDUCING should have produced denial events",
    );
    let denial_messages: Vec<String> = saved
        .iter()
        .filter(|e| e.event_type() == OrderEventType::Denied)
        .filter_map(|e| e.message().map(|m| m.as_str().to_string()))
        .collect();
    assert!(
        denial_messages
            .iter()
            .any(|m| m.contains(&instrument_b.id().to_string())),
        "expected denial reason to name instrument {}, found: {denial_messages:?}",
        instrument_b.id(),
    );
    assert!(
        !denial_messages
            .iter()
            .any(|m| m.contains(&instrument_a.id().to_string()) && m.contains("REDUCING")),
        "instrument_a should not appear in a REDUCING denial reason: {denial_messages:?}",
    );
}

#[rstest]
fn test_submit_trailing_stop_market_buy_with_trigger_price_then_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
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
    instrument_audusd: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    consume_fixture(execute_order_event_handler);
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::Denied
    );
    assert_eq!(
        saved_process_messages[0].message().unwrap(),
        Ustr::from(
            &OrderDeniedReason::PriceNotPositive {
                field: OrderPriceField::Price,
                price: order.price().unwrap(),
            }
            .to_string()
        )
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
        None, // correlation_id
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::ModifyRejected
    );
    assert_eq!(
        saved_process_messages[0].message().unwrap(),
        Ustr::from(
            "PRICE_PRECISION_EXCEEDS_MAXIMUM: field=PRICE, price=1.000001, precision=6, max_precision=5"
        )
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

    let saved_process_messages =
        get_process_order_event_handler_messages(&process_order_event_handler);
    assert_eq!(saved_process_messages.len(), 1);
    assert_eq!(
        saved_process_messages[0].event_type(),
        OrderEventType::ModifyRejected
    );
    assert_eq!(
        saved_process_messages[0].message().unwrap(),
        Ustr::from(
            "QUANTITY_PRECISION_EXCEEDS_MAXIMUM: quantity=100.1, precision=1, max_precision=0",
        )
    );
}

#[rstest]
fn test_submit_sell_cash_account_with_long_position_reduces_then_passes(
    strategy_id_ema_cross: StrategyId,
    client_id_binance: ClientId,
    trader_id: TraderId,
    instrument_eth_usdt: InstrumentAny,
    process_order_event_handler: TypedIntoMessageSavingHandler<OrderEventAny>,
    execute_order_event_handler: TypedIntoMessageSavingHandler<TradingCommand>,
    mut simple_cache: Cache,
) {
    consume_fixture(process_order_event_handler);
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
        None, // correlation_id
    );

    risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    let saved_execute_messages =
        get_execute_order_event_handler_messages(&execute_order_event_handler);
    assert_eq!(saved_execute_messages.len(), 1);
}

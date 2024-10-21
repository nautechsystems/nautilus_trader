// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, rc::Rc, sync::LazyLock};

use chrono::{TimeZone, Utc};
use nautilus_common::{
    cache::Cache,
    msgbus::{
        handler::ShareableMessageHandler,
        stubs::{get_message_saving_handler, get_saved_messages},
        MessageBus,
    },
};
use nautilus_core::{nanos::UnixNanos, time::AtomicTime, uuid::UUID4};
use nautilus_model::{
    data::{delta::OrderBookDelta, order::BookOrder},
    enums::{
        AccountType, BookAction, BookType, ContingencyType, LiquiditySide, OmsType, OrderSide,
        OrderType,
    },
    events::order::{
        rejected::OrderRejectedBuilder, OrderEventAny, OrderEventType, OrderFilled, OrderRejected,
    },
    identifiers::{AccountId, ClientOrderId, PositionId, TradeId, VenueOrderId},
    instruments::{
        any::InstrumentAny,
        crypto_perpetual::CryptoPerpetual,
        equity::Equity,
        stubs::{crypto_perpetual_ethusdt, equity_aapl, futures_contract_es},
    },
    orders::{any::OrderAny, builder::OrderTestBuilder, stubs::TestOrderStubs},
    position::Position,
    types::{price::Price, quantity::Quantity},
};
use rstest::{fixture, rstest};
use ustr::Ustr;

use crate::{
    matching_engine::{config::OrderMatchingEngineConfig, OrderMatchingEngine},
    models::fill::FillModel,
};

static ATOMIC_TIME: LazyLock<AtomicTime> =
    LazyLock::new(|| AtomicTime::new(true, UnixNanos::default()));

#[fixture]
fn msgbus() -> MessageBus {
    MessageBus::default()
}

#[fixture]
fn account_id() -> AccountId {
    AccountId::from("SIM-001")
}

#[fixture]
fn time() -> AtomicTime {
    AtomicTime::new(true, UnixNanos::default())
}

#[fixture]
fn order_event_handler() -> ShareableMessageHandler {
    get_message_saving_handler::<OrderEventAny>(Some(Ustr::from("ExecEngine.process")))
}

#[fixture]
fn instrument_eth_usdt(crypto_perpetual_ethusdt: CryptoPerpetual) -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt)
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

#[fixture]
fn market_order_fill(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    market_order_buy: OrderAny,
) -> OrderFilled {
    OrderFilled::new(
        market_order_buy.trader_id(),
        market_order_buy.strategy_id(),
        market_order_buy.instrument_id(),
        market_order_buy.client_order_id(),
        VenueOrderId::new("BINANCE-1"),
        account_id,
        TradeId::new("1"),
        market_order_buy.order_side(),
        market_order_buy.order_type(),
        Quantity::from("1"),
        Price::from("1000.000"),
        instrument_eth_usdt.quote_currency(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("P-1")),
        None,
    )
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

// For valid ES futures contract currently active
#[fixture]
fn instrument_es() -> InstrumentAny {
    let activation = UnixNanos::from(
        Utc.with_ymd_and_hms(2022, 4, 8, 0, 0, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    );
    let expiration = UnixNanos::from(
        Utc.with_ymd_and_hms(2100, 7, 8, 0, 0, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    );
    InstrumentAny::FuturesContract(futures_contract_es(Some(activation), Some(expiration)))
}

#[fixture]
fn engine_config() -> OrderMatchingEngineConfig {
    OrderMatchingEngineConfig {
        bar_execution: false,
        reject_stop_orders: false,
        support_gtd_orders: false,
        support_contingent_orders: true,
        use_position_ids: false,
        use_random_ids: false,
        use_reduce_only: true,
    }
}
// -- HELPERS ---------------------------------------------------------------------------

fn get_order_matching_engine(
    instrument: InstrumentAny,
    msgbus: Rc<RefCell<MessageBus>>,
    cache: Option<Rc<RefCell<Cache>>>,
    account_type: Option<AccountType>,
    config: Option<OrderMatchingEngineConfig>,
) -> OrderMatchingEngine {
    let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
    let config = config.unwrap_or_default();
    OrderMatchingEngine::new(
        instrument,
        1,
        FillModel::default(),
        BookType::L1_MBP,
        OmsType::Netting,
        account_type.unwrap_or(AccountType::Cash),
        &ATOMIC_TIME,
        msgbus,
        cache,
        config,
    )
}

fn get_order_matching_engine_l2(
    instrument: InstrumentAny,
    msgbus: Rc<RefCell<MessageBus>>,
    cache: Option<Rc<RefCell<Cache>>>,
    account_type: Option<AccountType>,
    config: Option<OrderMatchingEngineConfig>,
) -> OrderMatchingEngine {
    let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
    let config = config.unwrap_or_default();
    OrderMatchingEngine::new(
        instrument,
        1,
        FillModel::default(),
        BookType::L2_MBP,
        OmsType::Netting,
        account_type.unwrap_or(AccountType::Cash),
        &ATOMIC_TIME,
        msgbus,
        cache,
        config,
    )
}

fn get_order_event_handler_messages(event_handler: ShareableMessageHandler) -> Vec<OrderEventAny> {
    get_saved_messages::<OrderEventAny>(event_handler)
}

// -- TESTS -----------------------------------------------------------------------------------

#[rstest]
fn test_process_order_when_instrument_already_expired(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    market_order_buy: OrderAny,
) {
    let instrument = InstrumentAny::FuturesContract(futures_contract_es(None, None));

    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    // Create engine and process order
    let mut engine =
        get_order_matching_engine(instrument, Rc::new(RefCell::new(msgbus)), None, None, None);

    engine.process_order(&market_order_buy, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Contract ESZ1.GLBX has expired, expiration 1625702400000000000")
    );
}

#[rstest]
fn test_process_order_when_instrument_not_active(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    market_order_buy: OrderAny,
) {
    let activation = UnixNanos::from(
        Utc.with_ymd_and_hms(2222, 4, 8, 0, 0, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    );
    let expiration = UnixNanos::from(
        Utc.with_ymd_and_hms(2223, 7, 8, 0, 0, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    );
    let instrument =
        InstrumentAny::FuturesContract(futures_contract_es(Some(activation), Some(expiration)));

    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    // Create engine and process order
    let mut engine =
        get_order_matching_engine(instrument, Rc::new(RefCell::new(msgbus)), None, None, None);

    engine.process_order(&market_order_buy, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Contract ESZ1.GLBX is not yet active, activation 7960723200000000000")
    );
}

#[rstest]
fn test_process_order_when_invalid_quantity_precision(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_eth_usdt: InstrumentAny,
    market_order_buy: OrderAny,
) {
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    // Create engine and process order
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        Rc::new(RefCell::new(msgbus)),
        None,
        None,
        None,
    );

    engine.process_order(&market_order_buy, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Invalid order quantity precision for order O-19700101-000000-001-001-1, was 0 when ETHUSDT-PERP.BINANCE size precision is 3")
    );
}

#[rstest]
fn test_process_order_when_invalid_price_precision(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_es: InstrumentAny,
) {
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    // Create engine and process order
    let mut engine = get_order_matching_engine(
        instrument_es.clone(),
        Rc::new(RefCell::new(msgbus)),
        None,
        None,
        None,
    );

    let limit_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_es.id())
        .side(OrderSide::Sell)
        .price(Price::from("100.12333")) // <- wrong price precision for es futures contract (which is 2)
        .quantity(Quantity::from("1"))
        .build();

    engine.process_order(&limit_order, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Invalid order price precision for order O-19700101-000000-001-001-1, was 5 when ESZ1.GLBX price precision is 2")
    );
}

#[rstest]
fn test_process_order_when_invalid_trigger_price_precision(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_es: InstrumentAny,
) {
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    // Create engine and process order
    let mut engine = get_order_matching_engine(
        instrument_es.clone(),
        Rc::new(RefCell::new(msgbus)),
        None,
        None,
        None,
    );
    let stop_order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_es.id())
        .side(OrderSide::Sell)
        .trigger_price(Price::from("100.12333")) // <- wrong trigger price precision for es futures contract (which is 2)
        .quantity(Quantity::from("1"))
        .build();

    engine.process_order(&stop_order, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Invalid order trigger price precision for order O-19700101-000000-001-001-1, was 5 when ESZ1.GLBX price precision is 2")
    );
}

#[rstest]
fn test_process_order_when_shorting_equity_without_margin_account(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    equity_aapl: Equity,
    market_order_sell: OrderAny,
) {
    let instrument = InstrumentAny::Equity(equity_aapl);
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    // Create engine and process order
    let mut engine =
        get_order_matching_engine(instrument, Rc::new(RefCell::new(msgbus)), None, None, None);

    engine.process_order(&market_order_sell, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from(
            "Short selling not permitted on a CASH account with position None and \
            order MarketOrder(SELL 1 ETHUSDT-PERP.BINANCE @ MARKET GTC, status=INITIALIZED, \
            client_order_id=O-19700101-000000-001-001-1, venue_order_id=None, position_id=None, \
            exec_algorithm_id=None, exec_spawn_id=None, tags=None)"
        )
    );
}

#[rstest]
fn test_process_order_when_invalid_reduce_only(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_eth_usdt: InstrumentAny,
    engine_config: OrderMatchingEngineConfig,
) {
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    let mut engine = get_order_matching_engine(
        instrument_eth_usdt.clone(),
        Rc::new(RefCell::new(msgbus)),
        None,
        None,
        Some(engine_config),
    );
    let market_order_reduce = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .reduce_only(true)
        .build();

    engine.process_order(&market_order_reduce, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from("Reduce-only order O-19700101-000000-001-001-1 (MARKET-BUY) would have increased position")
    );
}

#[rstest]
fn test_process_order_when_invalid_contingent_orders(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_es: InstrumentAny,
    engine_config: OrderMatchingEngineConfig,
) {
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut engine = get_order_matching_engine(
        instrument_es.clone(),
        Rc::new(RefCell::new(msgbus)),
        Some(cache.clone()),
        None,
        Some(engine_config),
    );

    let entry_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
    let stop_loss_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-2");

    // Create entry market order
    let mut entry_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_es.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .contingency_type(ContingencyType::Oto)
        .client_order_id(entry_client_order_id)
        .build();
    // Set entry order status to Rejected with proper event
    let rejected_event = OrderRejected::default();
    entry_order
        .apply(OrderEventAny::Rejected(rejected_event))
        .unwrap();

    // Create stop loss order
    let stop_order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_es.id())
        .side(OrderSide::Sell)
        .trigger_price(Price::from("0.95"))
        .quantity(Quantity::from(1))
        .contingency_type(ContingencyType::Oto)
        .client_order_id(stop_loss_client_order_id)
        .parent_order_id(entry_client_order_id)
        .build();
    // Make it Accepted
    let accepted_stop_order = TestOrderStubs::make_accepted_order(&stop_order);

    // 1. Save entry order in the cache as it will be loaded by the matching engine
    // 2. Send the stop loss order which has parent of entry order
    cache
        .as_ref()
        .borrow_mut()
        .add_order(entry_order.clone(), None, None, false)
        .unwrap();
    engine.process_order(&accepted_stop_order, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from(format!("Rejected OTO order from {entry_client_order_id}").as_str())
    );
}

#[rstest]
fn test_process_order_when_closed_linked_order(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_es: InstrumentAny,
    engine_config: OrderMatchingEngineConfig,
) {
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut engine = get_order_matching_engine(
        instrument_es.clone(),
        Rc::new(RefCell::new(msgbus)),
        Some(cache.clone()),
        None,
        Some(engine_config),
    );

    let stop_loss_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-2");
    let take_profit_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-3");
    // Create two linked orders: stop loss and take profit
    let mut stop_loss_order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_es.id())
        .side(OrderSide::Sell)
        .trigger_price(Price::from("0.95"))
        .quantity(Quantity::from(1))
        .contingency_type(ContingencyType::Oco)
        .client_order_id(stop_loss_client_order_id)
        .linked_order_ids(vec![take_profit_client_order_id])
        .build();
    let take_profit_order = OrderTestBuilder::new(OrderType::MarketIfTouched)
        .instrument_id(instrument_es.id())
        .side(OrderSide::Sell)
        .trigger_price(Price::from("1.1"))
        .quantity(Quantity::from(1))
        .contingency_type(ContingencyType::Oco)
        .client_order_id(take_profit_client_order_id)
        .linked_order_ids(vec![stop_loss_client_order_id])
        .build();
    // Set stop loss order status to Rejected with proper event
    let rejected_event: OrderRejected = OrderRejectedBuilder::default()
        .client_order_id(stop_loss_client_order_id)
        .reason(Ustr::from("Rejected"))
        .build()
        .unwrap();
    stop_loss_order
        .apply(OrderEventAny::Rejected(rejected_event))
        .unwrap();

    // Make take profit order Accepted
    let accepted_take_profit = TestOrderStubs::make_accepted_order(&take_profit_order);

    // 1. Save stop loss order in cache which is rejected and closed is set to true
    // 2. Send the take profit order which has linked the stop loss order
    cache
        .as_ref()
        .borrow_mut()
        .add_order(stop_loss_order.clone(), None, None, false)
        .unwrap();
    let stop_loss_closed_after = stop_loss_order.is_closed();
    engine.process_order(&accepted_take_profit, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 1);
    let first_message = saved_messages.first().unwrap();
    assert_eq!(first_message.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first_message.message().unwrap(),
        Ustr::from(format!("Contingent order {stop_loss_client_order_id} already closed").as_str())
    );
}

#[rstest]
fn test_process_market_order_no_market_rejected(
    mut msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_es: InstrumentAny,
    market_order_buy: OrderAny,
    market_order_sell: OrderAny,
) {
    // Register saving message handler to exec engine endpoint
    msgbus.register(
        msgbus.switchboard.exec_engine_process,
        order_event_handler.clone(),
    );

    // Create engine and process order
    let mut engine = get_order_matching_engine(
        instrument_es,
        Rc::new(RefCell::new(msgbus)),
        None,
        None,
        None,
    );

    engine.process_order(&market_order_buy, account_id);
    engine.process_order(&market_order_sell, account_id);

    // Get messages and test
    let saved_messages = get_order_event_handler_messages(order_event_handler);
    assert_eq!(saved_messages.len(), 2);
    let first = saved_messages.first().unwrap();
    let second = saved_messages.get(1).unwrap();
    assert_eq!(first.event_type(), OrderEventType::Rejected);
    assert_eq!(second.event_type(), OrderEventType::Rejected);
    assert_eq!(
        first.message().unwrap(),
        Ustr::from("No market for ETHUSDT-PERP.BINANCE")
    );
    assert_eq!(
        second.message().unwrap(),
        Ustr::from("No market for ETHUSDT-PERP.BINANCE")
    );
}

#[rstest]
fn test_matching_core_bid_ask_initialized(
    msgbus: MessageBus,
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_es: InstrumentAny,
) {
    let mut engine_l2 = get_order_matching_engine_l2(
        instrument_es.clone(),
        Rc::new(RefCell::new(msgbus)),
        None,
        None,
        None,
    );
    // Create bid and ask orderbook delta and check if
    // bid and ask are initialized in order matching core
    let orderbook_delta_buy = OrderBookDelta::new(
        instrument_es.id(),
        BookAction::Add,
        BookOrder::new(OrderSide::Buy, Price::from("100"), Quantity::from("1"), 0),
        0,
        0,
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    let orderbook_delta_sell = OrderBookDelta::new(
        instrument_es.id(),
        BookAction::Add,
        BookOrder::new(OrderSide::Sell, Price::from("101"), Quantity::from("1"), 1),
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(1),
    );

    engine_l2.process_order_book_delta(&orderbook_delta_buy);
    assert_eq!(engine_l2.core.bid, Some(Price::from("100")));
    assert!(engine_l2.core.is_bid_initialized);
    assert_eq!(engine_l2.core.ask, None);
    assert!(!engine_l2.core.is_ask_initialized);

    engine_l2.process_order_book_delta(&orderbook_delta_sell);
    assert_eq!(engine_l2.core.bid, Some(Price::from("100")));
    assert!(engine_l2.core.is_bid_initialized);
    assert_eq!(engine_l2.core.ask, Some(Price::from("101")));
    assert!(engine_l2.core.is_ask_initialized);
}

#[rstest]
fn test_generate_venue_position_id(
    order_event_handler: ShareableMessageHandler,
    account_id: AccountId,
    time: AtomicTime,
    instrument_eth_usdt: InstrumentAny,
) {
    // Create two order matching engines with different configs
    // one with and other without position ids
    let config_no_position_id = OrderMatchingEngineConfig {
        use_position_ids: false,
        ..OrderMatchingEngineConfig::default()
    };
    let mut engine_no_position_id = get_order_matching_engine_l2(
        instrument_eth_usdt.clone(),
        Rc::new(RefCell::new(MessageBus::default())),
        None,
        None,
        Some(config_no_position_id),
    );

    let config_with_position_id = OrderMatchingEngineConfig {
        use_position_ids: true,
        ..OrderMatchingEngineConfig::default()
    };
    let mut engine_with_position_id = get_order_matching_engine_l2(
        instrument_eth_usdt,
        Rc::new(RefCell::new(MessageBus::default())),
        None,
        None,
        Some(config_with_position_id),
    );

    // Engine which doesnt have position id should return None
    assert_eq!(engine_no_position_id.generate_venue_position_id(), None);

    // Engine which has position id should return position id in incrementing order
    let position_id_1 = engine_with_position_id.generate_venue_position_id();
    let position_id_2 = engine_with_position_id.generate_venue_position_id();
    assert_eq!(position_id_1, Some(PositionId::new("BINANCE-1-1")));
    assert_eq!(position_id_2, Some(PositionId::new("BINANCE-1-2")));
}

#[rstest]
fn test_get_position_id_hedging_with_existing_position(
    account_id: AccountId,
    time: AtomicTime,
    instrument_eth_usdt: InstrumentAny,
    market_order_buy: OrderAny,
    market_order_fill: OrderFilled,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));

    // Create oms type hedging engine
    let mut engine = OrderMatchingEngine::new(
        instrument_eth_usdt.clone(),
        1,
        FillModel::default(),
        BookType::L1_MBP,
        OmsType::Hedging,
        AccountType::Cash,
        &ATOMIC_TIME,
        Rc::new(RefCell::new(MessageBus::default())),
        cache,
        OrderMatchingEngineConfig::default(),
    );

    let position = Position::new(&instrument_eth_usdt, market_order_fill);

    // Add position to cache
    engine
        .cache
        .borrow_mut()
        .add_position(position.clone(), engine.oms_type)
        .unwrap();

    let position_id = engine.get_position_id(&market_order_buy, None);
    assert_eq!(position_id, Some(position.id));
}

#[rstest]
fn test_get_position_id_hedging_with_generated_position(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    market_order_buy: OrderAny,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));

    // Use order matching config with position ids
    let config_with_position_id = OrderMatchingEngineConfig {
        use_position_ids: true,
        ..OrderMatchingEngineConfig::default()
    };
    // Create oms type hedging engine
    let mut engine = OrderMatchingEngine::new(
        instrument_eth_usdt,
        1,
        FillModel::default(),
        BookType::L1_MBP,
        OmsType::Hedging,
        AccountType::Cash,
        &ATOMIC_TIME,
        Rc::new(RefCell::new(MessageBus::default())),
        cache,
        config_with_position_id,
    );

    let position_id = engine.get_position_id(&market_order_buy, None);
    assert_eq!(position_id, Some(PositionId::new("BINANCE-1-1")));
}

#[rstest]
fn test_get_position_id_netting(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    market_order_buy: OrderAny,
    market_order_fill: OrderFilled,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));

    // create engine with Netting OMS type
    let mut engine = OrderMatchingEngine::new(
        instrument_eth_usdt.clone(),
        1,
        FillModel::default(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Cash,
        &ATOMIC_TIME,
        Rc::new(RefCell::new(MessageBus::default())),
        cache,
        OrderMatchingEngineConfig::default(),
    );

    // position id should be none in non-initialized position id for this instrument
    let position_id = engine.get_position_id(&market_order_buy, None);
    assert_eq!(position_id, None);

    // create and add position in cache
    let position = Position::new(&instrument_eth_usdt, market_order_fill);
    engine
        .cache
        .as_ref()
        .borrow_mut()
        .add_position(position.clone(), engine.oms_type)
        .unwrap();

    // position id should be returned for the existing position
    let position_id = engine.get_position_id(&market_order_buy, None);
    assert_eq!(position_id, Some(position.id));
}

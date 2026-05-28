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

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use nautilus_backtest::{
    config::SimulatedVenueConfig,
    exchange::SimulatedExchange,
    execution_client::BacktestExecutionClient,
    modules::{ExchangeContext, SimulationModule},
};
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    messages::execution::{ModifyOrder, SubmitOrder, TradingCommand},
    msgbus::{
        self, MessagingSwitchboard,
        stubs::{get_typed_into_message_saving_handler, get_typed_message_saving_handler},
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::models::{
    fee::{FeeModelAny, MakerTakerFeeModel},
    latency::StaticLatencyModel,
};
use nautilus_model::{
    accounts::{AccountAny, CashAccount, MarginAccount},
    data::{
        Bar, BarType, BookOrder, Data, InstrumentStatus, OrderBookDelta, OrderBookDeltas,
        QuoteTick, TradeTick,
    },
    enums::{
        AccountType, AggressorSide, BookAction, BookType, MarketStatus, MarketStatusAction,
        OmsType, OrderSide, OrderStatus, OrderType,
    },
    events::{AccountState, OrderEventAny, order::spec::OrderPendingUpdateSpec},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    stubs::TestDefault,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;

fn get_exchange(
    venue: Venue,
    account_type: AccountType,
    book_type: BookType,
    cache: Option<Rc<RefCell<Cache>>>,
) -> Rc<RefCell<SimulatedExchange>> {
    let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(account_type)
        .book_type(book_type)
        .starting_balances(vec![Money::new(1000.0, Currency::USD())])
        .default_leverage(Decimal::ONE)
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel))
        .build();
    let exchange = Rc::new(RefCell::new(
        SimulatedExchange::new(config, cache.clone(), clock).unwrap(),
    ));

    let clock = TestClock::new();
    let execution_client = BacktestExecutionClient::new(
        TraderId::test_default(),
        AccountId::test_default(),
        &exchange,
        cache,
        Rc::new(RefCell::new(clock)),
        None,
        None,
    );
    exchange
        .borrow_mut()
        .register_client(Rc::new(execution_client));

    exchange
}

fn create_submit_order_command(
    ts_init: UnixNanos,
    client_order_id: &str,
) -> (OrderAny, TradingCommand) {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::new(client_order_id))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1000.00"))
        .build();
    let command = TradingCommand::SubmitOrder(SubmitOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None, // params
        UUID4::default(),
        ts_init,
        None, // correlation_id
    ));
    (order, command)
}

#[rstest]
#[should_panic(
    expected = "Condition failed: 'Venue of instrument id' value of BINANCE was not equal to 'Venue of simulated exchange' value of SIM"
)]
fn test_venue_mismatch_between_exchange_and_instrument(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();
}

#[rstest]
#[should_panic(expected = "Cash account cannot trade futures or perpetuals")]
fn test_cash_account_trading_futures_or_perpetuals(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Cash,
        BookType::L1_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();
}

#[rstest]
fn test_exchange_process_quote_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

    // register instrument
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // process tick
    let quote_tick = QuoteTick::new(
        crypto_perpetual_ethusdt.id,
        Price::from("1000.00"),
        Price::from("1001.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    exchange.borrow_mut().process_quote_tick(&quote_tick);

    let best_bid_price = exchange
        .borrow()
        .best_bid_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_bid_price, Some(Price::from("1000.00")));
    let best_ask_price = exchange
        .borrow()
        .best_ask_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_ask_price, Some(Price::from("1001.00")));
}

#[rstest]
fn test_exchange_process_trade_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

    // register instrument
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // process tick
    let trade_tick = TradeTick::new(
        crypto_perpetual_ethusdt.id,
        Price::from("1000.00"),
        Quantity::from("1.000"),
        AggressorSide::Buyer,
        TradeId::from("1"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    exchange.borrow_mut().process_trade_tick(&trade_tick);

    let best_bid_price = exchange
        .borrow()
        .best_bid_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_bid_price, Some(Price::from("1000.00")));
    let best_ask = exchange
        .borrow()
        .best_ask_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_ask, Some(Price::from("1000.00")));
}

#[rstest]
fn test_exchange_process_bar_last_bar_spec(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

    // register instrument
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // process bar
    let bar = Bar::new(
        BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL"),
        Price::from("1500.00"),
        Price::from("1505.00"),
        Price::from("1490.00"),
        Price::from("1502.00"),
        Quantity::from("100.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    exchange.borrow_mut().process_bar(bar);

    // this will be processed as ticks so both bid and ask will be the same as close of the bar
    let best_bid_price = exchange
        .borrow()
        .best_bid_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_bid_price, Some(Price::from("1502.00")));
    let best_ask_price = exchange
        .borrow()
        .best_ask_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_ask_price, Some(Price::from("1502.00")));
}

#[rstest]
fn test_exchange_process_bar_bid_ask_bar_spec(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

    // register instrument
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // create both bid and ask based bars
    // add +1 on ask to make sure it is different from bid
    let bar_bid = Bar::new(
        BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-BID-EXTERNAL"),
        Price::from("1500.00"),
        Price::from("1505.00"),
        Price::from("1490.00"),
        Price::from("1502.00"),
        Quantity::from("100.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let bar_ask = Bar::new(
        BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-ASK-EXTERNAL"),
        Price::from("1501.00"),
        Price::from("1506.00"),
        Price::from("1491.00"),
        Price::from("1503.00"),
        Quantity::from("100.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );

    // process them
    exchange.borrow_mut().process_bar(bar_bid);
    exchange.borrow_mut().process_bar(bar_ask);

    // current bid and ask prices will be the corresponding close of the ask and bid bar
    let best_bid_price = exchange
        .borrow()
        .best_bid_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_bid_price, Some(Price::from("1502.00")));
    let best_ask_price = exchange
        .borrow()
        .best_ask_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_ask_price, Some(Price::from("1503.00")));
}

#[rstest]
fn test_exchange_process_orderbook_delta(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

    // register instrument
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // create order book delta at both bid and ask with incremented ts init and sequence
    let delta_buy = OrderBookDelta::new(
        crypto_perpetual_ethusdt.id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Buy,
            Price::from("1000.00"),
            Quantity::from("1.000"),
            1,
        ),
        0,
        0,
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let delta_sell = OrderBookDelta::new(
        crypto_perpetual_ethusdt.id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("1001.00"),
            Quantity::from("1.000"),
            1,
        ),
        0,
        1,
        UnixNanos::from(2),
        UnixNanos::from(2),
    );

    // process both deltas
    exchange.borrow_mut().process_order_book_delta(delta_buy);
    exchange.borrow_mut().process_order_book_delta(delta_sell);

    let book = exchange
        .borrow()
        .get_book(crypto_perpetual_ethusdt.id)
        .unwrap()
        .clone();
    assert_eq!(book.update_count, 2);
    assert_eq!(book.sequence, 1);
    assert_eq!(book.ts_last, UnixNanos::from(2));
    let best_bid_price = exchange
        .borrow()
        .best_bid_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_bid_price, Some(Price::from("1000.00")));
    let best_ask_price = exchange
        .borrow()
        .best_ask_price(crypto_perpetual_ethusdt.id);
    assert_eq!(best_ask_price, Some(Price::from("1001.00")));
}

#[rstest]
fn test_exchange_process_orderbook_deltas(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

    // register instrument
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // create two sell order book deltas with same timestamps and higher sequence
    let delta_sell_1 = OrderBookDelta::new(
        crypto_perpetual_ethusdt.id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("1000.00"),
            Quantity::from("3.000"),
            1,
        ),
        0,
        0,
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let delta_sell_2 = OrderBookDelta::new(
        crypto_perpetual_ethusdt.id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("1001.00"),
            Quantity::from("1.000"),
            1,
        ),
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let orderbook_deltas = OrderBookDeltas::new(
        crypto_perpetual_ethusdt.id,
        vec![delta_sell_1, delta_sell_2],
    );

    // process both deltas
    exchange
        .borrow_mut()
        .process_order_book_deltas(&orderbook_deltas);

    let book = exchange
        .borrow()
        .get_book(crypto_perpetual_ethusdt.id)
        .unwrap()
        .clone();
    assert_eq!(book.update_count, 2);
    assert_eq!(book.sequence, 1);
    assert_eq!(book.ts_last, UnixNanos::from(1));
    let best_bid_price = exchange
        .borrow()
        .best_bid_price(crypto_perpetual_ethusdt.id);
    // no bid orders in orderbook deltas
    assert_eq!(best_bid_price, None);
    let best_ask_price = exchange
        .borrow()
        .best_ask_price(crypto_perpetual_ethusdt.id);
    // best ask price is the first order in orderbook deltas
    assert_eq!(best_ask_price, Some(Price::from("1000.00")));
}

#[rstest]
fn test_exchange_process_instrument_status(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

    // register instrument
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let instrument_status = InstrumentStatus::new(
        crypto_perpetual_ethusdt.id,
        MarketStatusAction::Close, // close the market
        UnixNanos::from(1),
        UnixNanos::from(1),
        None,
        None,
        None,
        None,
        None,
    );

    exchange
        .borrow_mut()
        .process_instrument_status(instrument_status);

    let market_status = exchange
        .borrow()
        .get_matching_engine(&crypto_perpetual_ethusdt.id)
        .unwrap()
        .market_status;
    assert_eq!(market_status, MarketStatus::Closed);
}

#[rstest]
fn test_accounting() {
    let account_type = AccountType::Margin;
    let mut cache = Cache::default();
    let (handler, saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);
    let margin_account = MarginAccount::new(
        AccountState::new(
            AccountId::from("SIM-001"),
            account_type,
            vec![AccountBalance::new(
                Money::from("1000 USD"),
                Money::from("0 USD"),
                Money::from("1000 USD"),
            )],
            vec![],
            false,
            UUID4::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        ),
        false,
    );
    let () = cache
        .add_account(AccountAny::Margin(margin_account))
        .unwrap();
    // build indexes
    cache.build_index();

    let exchange = get_exchange(
        Venue::new("SIM"),
        account_type,
        BookType::L2_MBP,
        Some(Rc::new(RefCell::new(cache))),
    );
    exchange.borrow_mut().initialize_account();

    // Test adjust account, increase balance by 500 USD
    exchange.borrow_mut().adjust_account(Money::from("500 USD"));

    // Check if we received two messages, one for initial account state and one for adjusted account state
    let messages = saving_handler.get_messages();
    assert_eq!(messages.len(), 2);
    let account_state_first = messages.first().unwrap();
    let account_state_second = messages.last().unwrap();

    assert_eq!(account_state_first.balances.len(), 1);
    let current_balance = account_state_first.balances[0];
    assert_eq!(current_balance.free, Money::new(1000.0, Currency::USD()));
    assert_eq!(current_balance.locked, Money::new(0.0, Currency::USD()));
    assert_eq!(current_balance.total, Money::new(1000.0, Currency::USD()));

    assert_eq!(account_state_second.balances.len(), 1);
    let current_balance = account_state_second.balances[0];
    assert_eq!(current_balance.free, Money::new(1500.0, Currency::USD()));
    assert_eq!(current_balance.locked, Money::new(0.0, Currency::USD()));
    assert_eq!(current_balance.total, Money::new(1500.0, Currency::USD()));
}

fn build_exchange_with_frozen_account(
    venue: Venue,
    account_type: AccountType,
    frozen_account: bool,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<SimulatedExchange>> {
    build_exchange_with_options(venue, account_type, frozen_account, false, cache)
}

fn build_exchange_with_options(
    venue: Venue,
    account_type: AccountType,
    frozen_account: bool,
    allow_cash_borrowing: bool,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<SimulatedExchange>> {
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(account_type)
        .book_type(BookType::L2_MBP)
        .starting_balances(vec![Money::new(1000.0, Currency::USD())])
        .default_leverage(Decimal::ONE)
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel))
        .frozen_account(frozen_account)
        .allow_cash_borrowing(allow_cash_borrowing)
        .build();
    let exchange = Rc::new(RefCell::new(
        SimulatedExchange::new(config, cache.clone(), clock.clone()).unwrap(),
    ));
    let exec_client = BacktestExecutionClient::new(
        TraderId::test_default(),
        AccountId::from(format!("{venue}-001").as_str()),
        &exchange,
        cache,
        clock,
        None,
        Some(frozen_account),
    );
    exchange.borrow_mut().register_client(Rc::new(exec_client));
    exchange
}

fn pre_populate_margin_account(cache: &mut Cache, account_id: &str) {
    let margin_account = MarginAccount::new(
        AccountState::new(
            AccountId::from(account_id),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1000 USD"),
                Money::from("0 USD"),
                Money::from("1000 USD"),
            )],
            vec![],
            false,
            UUID4::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        ),
        false,
    );
    cache
        .add_account(AccountAny::Margin(margin_account))
        .unwrap();
    cache.build_index();
}

#[rstest]
fn test_initialize_account_enables_calculate_account_state() {
    let mut cache = Cache::default();
    let (handler, _saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);
    pre_populate_margin_account(&mut cache, "SIM-001");

    let cache = Rc::new(RefCell::new(cache));
    let exchange = build_exchange_with_frozen_account(
        Venue::new("SIM"),
        AccountType::Margin,
        false,
        cache.clone(),
    );
    exchange.borrow_mut().initialize_account();

    let cache_ref = cache.borrow();
    let account = cache_ref.account(&AccountId::from("SIM-001")).unwrap();
    match &*account {
        AccountAny::Margin(margin) => {
            assert!(margin.base.calculate_account_state);
        }
        _ => panic!("expected margin account"),
    }
}

fn pre_populate_cash_account(cache: &mut Cache, account_id: &str) {
    let cash_account = CashAccount::new(
        AccountState::new(
            AccountId::from(account_id),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::from("1000 USD"),
                Money::from("0 USD"),
                Money::from("1000 USD"),
            )],
            vec![],
            false,
            UUID4::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        ),
        false,
        false,
    );
    cache.add_account(AccountAny::Cash(cash_account)).unwrap();
    cache.build_index();
}

#[rstest]
fn test_initialize_account_applies_allow_cash_borrowing() {
    let mut cache = Cache::default();
    let (handler, _saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);
    pre_populate_cash_account(&mut cache, "SIM-001");

    let cache = Rc::new(RefCell::new(cache));
    let exchange = build_exchange_with_options(
        Venue::new("SIM"),
        AccountType::Cash,
        false,
        true,
        cache.clone(),
    );
    exchange.borrow_mut().initialize_account();

    let cache_ref = cache.borrow();
    let account = cache_ref.account(&AccountId::from("SIM-001")).unwrap();
    match &*account {
        AccountAny::Cash(cash) => {
            assert!(cash.base.calculate_account_state);
            assert!(cash.allow_borrowing);
        }
        _ => panic!("expected cash account"),
    }
}

#[rstest]
fn test_initialize_account_frozen_disables_calculate_account_state() {
    let mut cache = Cache::default();
    let (handler, _saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);
    pre_populate_margin_account(&mut cache, "SIM-001");

    let cache = Rc::new(RefCell::new(cache));
    let exchange = build_exchange_with_frozen_account(
        Venue::new("SIM"),
        AccountType::Margin,
        true,
        cache.clone(),
    );
    exchange.borrow_mut().initialize_account();

    let cache_ref = cache.borrow();
    let account = cache_ref.account(&AccountId::from("SIM-001")).unwrap();
    match &*account {
        AccountAny::Margin(margin) => {
            assert!(!margin.base.calculate_account_state);
        }
        _ => panic!("expected margin account"),
    }
}

#[rstest]
fn test_inflight_commands_process_fifo_for_same_timestamp(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let latency_model = StaticLatencyModel::new(
        UnixNanos::from(0),
        UnixNanos::from(0),
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    exchange
        .borrow_mut()
        .set_latency_model(Box::new(latency_model));
    exchange
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt))
        .unwrap();

    let account_id = AccountId::test_default();
    let (order1, cmd1) = create_submit_order_command(UnixNanos::from(100), "O-1");
    let (order2, cmd2) = create_submit_order_command(UnixNanos::from(100), "O-2");
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order1, account_id))
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order2, account_id))
        .unwrap();

    exchange.borrow_mut().send(cmd1);
    exchange.borrow_mut().send(cmd2);
    exchange.borrow_mut().process(UnixNanos::from(100));

    let accepted_order_ids = saving_handler
        .get_messages()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        accepted_order_ids,
        vec![ClientOrderId::new("O-1"), ClientOrderId::new("O-2")]
    );
}

#[rstest]
fn test_max_inflight_command_ts_empty() {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    assert_eq!(exchange.borrow().max_inflight_command_ts(), None);
}

#[rstest]
fn test_max_inflight_command_ts_single_entry() {
    let latency_model = StaticLatencyModel::new(
        UnixNanos::from(0),
        UnixNanos::from(50),
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    exchange
        .borrow_mut()
        .set_latency_model(Box::new(latency_model));
    let (_, cmd) = create_submit_order_command(UnixNanos::from(100), "O-1");
    exchange.borrow_mut().send(cmd);

    assert_eq!(
        exchange.borrow().max_inflight_command_ts(),
        Some(UnixNanos::from(150))
    );
}

#[rstest]
fn test_max_inflight_command_ts_returns_global_max_across_entries() {
    let latency_model = StaticLatencyModel::new(
        UnixNanos::from(0),
        UnixNanos::from(0),
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    exchange
        .borrow_mut()
        .set_latency_model(Box::new(latency_model));
    let (_, cmd1) = create_submit_order_command(UnixNanos::from(50), "O-1");
    let (_, cmd2) = create_submit_order_command(UnixNanos::from(200), "O-2");
    let (_, cmd3) = create_submit_order_command(UnixNanos::from(100), "O-3");

    exchange.borrow_mut().send(cmd1);
    exchange.borrow_mut().send(cmd2);
    exchange.borrow_mut().send(cmd3);

    assert_eq!(
        exchange.borrow().max_inflight_command_ts(),
        Some(UnixNanos::from(200))
    );
}

#[rstest]
fn test_max_inflight_command_ts_ignores_counter_for_same_timestamp() {
    let latency_model = StaticLatencyModel::new(
        UnixNanos::from(0),
        UnixNanos::from(0),
        UnixNanos::from(0),
        UnixNanos::from(0),
    );
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    exchange
        .borrow_mut()
        .set_latency_model(Box::new(latency_model));
    let (_, cmd1) = create_submit_order_command(UnixNanos::from(100), "O-1");
    let (_, cmd2) = create_submit_order_command(UnixNanos::from(100), "O-2");

    exchange.borrow_mut().send(cmd1);
    exchange.borrow_mut().send(cmd2);

    assert_eq!(
        exchange.borrow().max_inflight_command_ts(),
        Some(UnixNanos::from(100))
    );
}

#[rstest]
fn test_process_without_latency_model(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let (order1, command1) = create_submit_order_command(UnixNanos::from(100), "O-1");
    let (order2, command2) = create_submit_order_command(UnixNanos::from(200), "O-2");

    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(order1, None, None, false)
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(order2, None, None, false)
        .unwrap();

    exchange.borrow_mut().send(command1);
    exchange.borrow_mut().send(command2);

    assert!(exchange.borrow().has_pending_commands(UnixNanos::from(0)));

    exchange.borrow_mut().process(UnixNanos::from(300));
    assert!(!exchange.borrow().has_pending_commands(UnixNanos::from(300)));
}

#[rstest]
fn test_modify_submitted_order_generates_updated_event(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(
            crypto_perpetual_ethusdt.clone(),
        ))
        .unwrap();

    let account_id = AccountId::test_default();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .client_order_id(ClientOrderId::from("O-SUBMITTED-MODIFY"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1000.00"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order, account_id))
        .unwrap();
    order = cache
        .borrow()
        .order(&order.client_order_id())
        .map(|order| order.clone())
        .unwrap();

    let command = ModifyOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        crypto_perpetual_ethusdt.id,
        order.client_order_id(),
        None,
        Some(Quantity::from("2.000")),
        None,
        None,
        UUID4::new(),
        UnixNanos::from(1),
        None,
        None,
    );
    exchange
        .borrow_mut()
        .send(TradingCommand::ModifyOrder(command));
    exchange.borrow_mut().process(UnixNanos::from(1));

    let messages = saving_handler.get_messages();
    assert_eq!(messages.len(), 1);
    let updated = match &messages[0] {
        OrderEventAny::Updated(updated) => updated,
        event => panic!("Expected OrderUpdated event, received {event:?}"),
    };
    assert_eq!(updated.client_order_id, order.client_order_id());
    assert_eq!(updated.quantity, Quantity::from("2.000"));
    assert_eq!(updated.price, Some(Price::from("1000.00")));
    assert_eq!(updated.trigger_price, None);
}

#[rstest]
fn test_modify_pending_update_from_submitted_order_generates_updated_event(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(
            crypto_perpetual_ethusdt.clone(),
        ))
        .unwrap();

    let account_id = AccountId::test_default();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .client_order_id(ClientOrderId::from("O-PENDING-SUBMITTED-MODIFY"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1000.00"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order, account_id))
        .unwrap();

    let pending_update = OrderEventAny::PendingUpdate(
        OrderPendingUpdateSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(account_id)
            .build(),
    );
    cache.borrow_mut().update_order(&pending_update).unwrap();
    order = cache
        .borrow()
        .order(&order.client_order_id())
        .map(|order| order.clone())
        .unwrap();
    assert_eq!(order.status(), OrderStatus::PendingUpdate);
    assert_eq!(order.previous_status(), Some(OrderStatus::Submitted));

    let command = ModifyOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        crypto_perpetual_ethusdt.id,
        order.client_order_id(),
        None,
        Some(Quantity::from("3.000")),
        Some(Price::from("998.00")),
        None,
        UUID4::new(),
        UnixNanos::from(1),
        None,
        None,
    );
    exchange
        .borrow_mut()
        .send(TradingCommand::ModifyOrder(command));
    exchange.borrow_mut().process(UnixNanos::from(1));

    let messages = saving_handler.get_messages();
    assert_eq!(messages.len(), 1);
    let updated = match &messages[0] {
        OrderEventAny::Updated(updated) => updated,
        event => panic!("Expected OrderUpdated event, received {event:?}"),
    };
    assert_eq!(updated.client_order_id, order.client_order_id());
    assert_eq!(updated.quantity, Quantity::from("3.000"));
    assert_eq!(updated.price, Some(Price::from("998.00")));
    assert_eq!(updated.trigger_price, None);
}

#[rstest]
fn test_modify_accepted_order_routes_to_matching_engine(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(
            crypto_perpetual_ethusdt.clone(),
        ))
        .unwrap();

    let account_id = AccountId::test_default();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .client_order_id(ClientOrderId::from("O-ACCEPTED-MODIFY"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1000.00"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order, account_id))
        .unwrap();

    let submit = SubmitOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        crypto_perpetual_ethusdt.id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::from(1),
        None,
    );
    exchange
        .borrow_mut()
        .send(TradingCommand::SubmitOrder(submit));
    exchange.borrow_mut().process(UnixNanos::from(1));

    let accepted = saving_handler
        .get_messages()
        .into_iter()
        .find_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(OrderEventAny::Accepted(accepted)),
            _ => None,
        })
        .unwrap();
    cache.borrow_mut().update_order(&accepted).unwrap();
    saving_handler.clear();

    order = cache
        .borrow()
        .order(&order.client_order_id())
        .map(|order| order.clone())
        .unwrap();
    assert_eq!(order.status(), OrderStatus::Accepted);

    let command = ModifyOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        crypto_perpetual_ethusdt.id,
        order.client_order_id(),
        order.venue_order_id(),
        Some(Quantity::from("2.000")),
        Some(Price::from("999.00")),
        None,
        UUID4::new(),
        UnixNanos::from(2),
        None,
        None,
    );
    exchange
        .borrow_mut()
        .send(TradingCommand::ModifyOrder(command));
    exchange.borrow_mut().process(UnixNanos::from(2));

    let messages = saving_handler.get_messages();
    assert_eq!(messages.len(), 1);
    let updated = match &messages[0] {
        OrderEventAny::Updated(updated) => updated,
        event => panic!("Expected OrderUpdated event, received {event:?}"),
    };
    assert_eq!(updated.client_order_id, order.client_order_id());
    assert_eq!(updated.quantity, Quantity::from("2.000"));
    assert_eq!(updated.price, Some(Price::from("999.00")));
    assert_eq!(updated.trigger_price, None);
}

#[rstest]
fn test_modify_pending_update_from_accepted_order_routes_to_matching_engine(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(
            crypto_perpetual_ethusdt.clone(),
        ))
        .unwrap();

    let account_id = AccountId::test_default();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .client_order_id(ClientOrderId::from("O-PENDING-ACCEPTED-MODIFY"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1000.00"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order, account_id))
        .unwrap();

    let submit = SubmitOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        crypto_perpetual_ethusdt.id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::from(1),
        None,
    );
    exchange
        .borrow_mut()
        .send(TradingCommand::SubmitOrder(submit));
    exchange.borrow_mut().process(UnixNanos::from(1));

    let accepted = saving_handler
        .get_messages()
        .into_iter()
        .find_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(OrderEventAny::Accepted(accepted)),
            _ => None,
        })
        .unwrap();
    cache.borrow_mut().update_order(&accepted).unwrap();
    saving_handler.clear();
    order = cache
        .borrow()
        .order(&order.client_order_id())
        .map(|order| order.clone())
        .unwrap();

    let pending_update = OrderEventAny::PendingUpdate(
        OrderPendingUpdateSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(account_id)
            .maybe_venue_order_id(order.venue_order_id())
            .build(),
    );
    cache.borrow_mut().update_order(&pending_update).unwrap();
    order = cache
        .borrow()
        .order(&order.client_order_id())
        .map(|order| order.clone())
        .unwrap();
    assert_eq!(order.status(), OrderStatus::PendingUpdate);
    assert_eq!(order.previous_status(), Some(OrderStatus::Accepted));

    let command = ModifyOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        crypto_perpetual_ethusdt.id,
        order.client_order_id(),
        order.venue_order_id(),
        Some(Quantity::from("2.000")),
        Some(Price::from("999.00")),
        None,
        UUID4::new(),
        UnixNanos::from(2),
        None,
        None,
    );
    exchange
        .borrow_mut()
        .send(TradingCommand::ModifyOrder(command));
    exchange.borrow_mut().process(UnixNanos::from(2));

    let messages = saving_handler.get_messages();
    assert_eq!(messages.len(), 1);
    let updated = match &messages[0] {
        OrderEventAny::Updated(updated) => updated,
        event => panic!("Expected OrderUpdated event, received {event:?}"),
    };
    assert_eq!(updated.client_order_id, order.client_order_id());
    assert_eq!(updated.quantity, Quantity::from("2.000"));
    assert_eq!(updated.price, Some(Price::from("999.00")));
    assert_eq!(updated.trigger_price, None);
}

#[rstest]
fn test_process_with_latency_model(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    // StaticLatencyModel adds base_latency to each operation latency
    // base=100, insert=200 -> effective insert latency = 300
    let latency_model = StaticLatencyModel::new(
        UnixNanos::from(100),
        UnixNanos::from(200),
        UnixNanos::from(300),
        UnixNanos::from(100),
    );
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    exchange
        .borrow_mut()
        .set_latency_model(Box::new(latency_model));

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let account_id = AccountId::test_default();
    let (order1, command1) = create_submit_order_command(UnixNanos::from(100), "O-1");
    let (order2, command2) = create_submit_order_command(UnixNanos::from(150), "O-2");

    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order1, account_id))
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order2, account_id))
        .unwrap();

    exchange.borrow_mut().send(command1);
    exchange.borrow_mut().send(command2);

    assert!(!exchange.borrow().has_pending_commands(UnixNanos::from(399)));
    assert!(exchange.borrow().has_pending_commands(UnixNanos::from(400)));
    assert_eq!(
        exchange.borrow().max_inflight_command_ts(),
        Some(UnixNanos::from(450))
    );

    exchange.borrow_mut().process(UnixNanos::from(420));
    let accepted_order_ids = saving_handler
        .get_messages()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(accepted_order_ids, vec![ClientOrderId::new("O-1")]);
    assert!(!exchange.borrow().has_pending_commands(UnixNanos::from(420)));
    assert!(exchange.borrow().has_pending_commands(UnixNanos::from(450)));
    assert_eq!(
        exchange.borrow().max_inflight_command_ts(),
        Some(UnixNanos::from(450))
    );
}

#[rstest]
fn test_process_iterates_matching_engines_after_commands(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let quote = QuoteTick::new(
        instrument_id,
        Price::from("1000.00"),
        Price::from("1001.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    exchange.borrow_mut().process_quote_tick(&quote);

    // Create a passive buy limit below the ask (should NOT fill)
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::new("O-LIMIT-1"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("999.00"))
        .build();

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let command = TradingCommand::SubmitOrder(SubmitOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::default(),
        UnixNanos::from(1),
        None, // correlation_id
    ));
    exchange.borrow_mut().send(command);

    exchange.borrow_mut().process(UnixNanos::from(1));

    let open_orders = exchange.borrow().get_open_orders(Some(instrument_id));
    assert_eq!(open_orders.len(), 1);
    assert_eq!(
        open_orders[0].client_order_id,
        ClientOrderId::new("O-LIMIT-1")
    );
}

#[derive(Clone)]
struct MockModuleCounts {
    pre_process: Rc<Cell<u32>>,
    process: Rc<Cell<u32>>,
    reset: Rc<Cell<u32>>,
    log_diagnostics: Rc<Cell<u32>>,
}

impl MockModuleCounts {
    fn new() -> Self {
        Self {
            pre_process: Rc::new(Cell::new(0)),
            process: Rc::new(Cell::new(0)),
            reset: Rc::new(Cell::new(0)),
            log_diagnostics: Rc::new(Cell::new(0)),
        }
    }
}

struct MockSimulationModule {
    counts: MockModuleCounts,
}

impl MockSimulationModule {
    fn new(counts: MockModuleCounts) -> Self {
        Self { counts }
    }
}

impl SimulationModule for MockSimulationModule {
    fn pre_process(&self, _data: &Data) {
        self.counts
            .pre_process
            .set(self.counts.pre_process.get() + 1);
    }

    fn process(&self, _ts_now: UnixNanos, _ctx: &ExchangeContext) -> Vec<Money> {
        self.counts.process.set(self.counts.process.get() + 1);
        Vec::new()
    }

    fn log_diagnostics(&self) {
        self.counts
            .log_diagnostics
            .set(self.counts.log_diagnostics.get() + 1);
    }

    fn reset(&self) {
        self.counts.reset.set(self.counts.reset.get() + 1);
    }
}

fn get_exchange_with_module(
    venue: Venue,
    counts: MockModuleCounts,
) -> Rc<RefCell<SimulatedExchange>> {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));

    // Register msgbus handler so generate_account_state works during reset
    let (handler, _saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);

    let modules: Vec<Box<dyn SimulationModule>> = vec![Box::new(MockSimulationModule::new(counts))];

    let config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::new(1000.0, Currency::USD())])
        .default_leverage(Decimal::ONE)
        .modules(modules)
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel))
        .build();
    let exchange = Rc::new(RefCell::new(
        SimulatedExchange::new(config, cache.clone(), clock).unwrap(),
    ));

    let exec_clock = TestClock::new();
    let execution_client = BacktestExecutionClient::new(
        TraderId::test_default(),
        AccountId::test_default(),
        &exchange,
        cache,
        Rc::new(RefCell::new(exec_clock)),
        None,
        None,
    );
    exchange
        .borrow_mut()
        .register_client(Rc::new(execution_client));

    exchange
}

#[rstest]
fn test_module_pre_process_called_on_quote(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let quote = QuoteTick::new(
        crypto_perpetual_ethusdt.id,
        Price::from("1000.00"),
        Price::from("1001.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    exchange.borrow_mut().process_quote_tick(&quote);

    assert_eq!(counts.pre_process.get(), 1);
    assert_eq!(counts.process.get(), 0);
}

#[rstest]
fn test_module_pre_process_called_on_instrument_status(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let status = InstrumentStatus::new(
        crypto_perpetual_ethusdt.id,
        MarketStatusAction::Close,
        UnixNanos::from(1),
        UnixNanos::from(1),
        None,
        None,
        None,
        None,
        None,
    );
    exchange.borrow_mut().process_instrument_status(status);

    assert_eq!(counts.pre_process.get(), 1);
    assert_eq!(counts.process.get(), 0);
}

#[rstest]
fn test_module_process_not_called_by_process(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // process() drains commands but does not run modules
    exchange.borrow_mut().process(UnixNanos::from(100));

    assert_eq!(counts.process.get(), 0);
}

#[rstest]
fn test_module_process_called_by_process_modules(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    exchange.borrow_mut().process_modules(UnixNanos::from(100));

    assert_eq!(counts.process.get(), 1);
}

#[rstest]
fn test_module_reset_called_on_reset(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // Pre-populate account in cache so generate_fresh_account_state succeeds
    let margin_account = MarginAccount::new(
        AccountState::new(
            AccountId::test_default(),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1000 USD"),
                Money::from("0 USD"),
                Money::from("1000 USD"),
            )],
            vec![],
            false,
            UUID4::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        ),
        false,
    );
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_account(AccountAny::Margin(margin_account))
        .unwrap();

    exchange.borrow_mut().reset();

    assert_eq!(counts.reset.get(), 1);
}

#[rstest]
fn test_module_log_diagnostics(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    exchange.borrow().log_diagnostics();

    assert_eq!(counts.log_diagnostics.get(), 1);
}

#[rstest]
fn test_module_pre_process_and_process_call_order(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    // pre_process called per data item, process_modules called separately
    let quote = QuoteTick::new(
        crypto_perpetual_ethusdt.id,
        Price::from("1000.00"),
        Price::from("1001.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    exchange.borrow_mut().process_quote_tick(&quote);
    exchange.borrow_mut().process_quote_tick(&quote);
    exchange.borrow_mut().process(UnixNanos::from(100));
    exchange.borrow_mut().process_modules(UnixNanos::from(100));

    assert_eq!(counts.pre_process.get(), 2);
    assert_eq!(counts.process.get(), 1);
}

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
    str::FromStr,
};

use ahash::AHashMap;
use jiff::{
    civil::{Date, Time, Weekday},
    tz::TimeZone,
};
use log::{Level, LevelFilter, Log, Metadata, Record};
use nautilus_backtest::{
    config::SimulatedVenueConfig,
    exchange::SimulatedExchange,
    execution_client::BacktestExecutionClient,
    modules::{
        AccountAdjustmentError, AccountAdjustmentOutcome, CfdSwapModule, CfdSwapRate,
        ExchangeContext, FXRolloverInterestModule, SimulationModule, SimulationModuleAny,
        SimulationModuleHandle, SimulationModuleResult, fx_rollover::InterestRateRecord,
    },
};
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    messages::execution::{ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand},
    msgbus::{
        self, MessagingSwitchboard,
        stubs::{
            TypedIntoMessageSavingHandler, get_any_saving_handler,
            get_typed_into_message_saving_handler, get_typed_message_saving_handler,
        },
        typed_handler::TypedHandler,
    },
};
use nautilus_core::{UUID4, UnixNanos, datetime::get_timezone};
use nautilus_execution::models::{
    fee::{FeeModelAny, MakerTakerFeeModel},
    latency::{LatencyModelHandle, StaticLatencyModel},
};
use nautilus_model::{
    accounts::{Account, AccountAny, CashAccount, MarginAccount},
    data::{
        Bar, BarType, BookOrder, Data, FundingRateUpdate, InstrumentStatus, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{
        AccountType, AggressorSide, AssetClass, BookAction, BookType, LiquiditySide, MarketStatus,
        MarketStatusAction, OmsType, OptionKind, OrderSide, OrderStatus, OrderType,
        PositionAdjustmentType,
    },
    events::{
        AccountState, FundingSettlement, OrderEventAny, OrderFilled, PositionEvent,
        order::spec::OrderPendingUpdateSpec,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId, Symbol,
        TradeId, TraderId, Venue,
    },
    instruments::{
        CryptoOption, CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny, OptionContract,
        stubs::{audusd_sim, cfd_gold, crypto_perpetual_ethusdt, gbpusd_sim, xbtusd_bitmex},
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    stubs::TestDefault,
    types::{
        AccountBalance, Currency, MarginBalance, Money, Price, Quantity, money::MONEY_RAW_MAX,
    },
};
use nautilus_testkit::cache::TestCacheDatabaseControl;
use parking_lot::Mutex;
use rstest::rstest;
use rust_decimal::Decimal;
use ustr::Ustr;

fn get_exchange(
    venue: Venue,
    account_type: AccountType,
    book_type: BookType,
    cache: Option<Rc<RefCell<Cache>>>,
) -> Rc<RefCell<SimulatedExchange>> {
    get_exchange_with_oms(venue, OmsType::Netting, account_type, book_type, cache)
}

fn get_exchange_with_oms(
    venue: Venue,
    oms_type: OmsType,
    account_type: AccountType,
    book_type: BookType,
    cache: Option<Rc<RefCell<Cache>>>,
) -> Rc<RefCell<SimulatedExchange>> {
    let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(oms_type)
        .account_type(account_type)
        .book_type(book_type)
        .starting_balances(vec![Money::new(1000.0, Currency::USD())])
        .default_leverage(Decimal::ONE)
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel).into())
        .build()
        .unwrap();
    let exchange = Rc::new(RefCell::new(
        SimulatedExchange::new(config, cache.clone(), clock).unwrap(),
    ));
    SimulatedExchange::register_spread_quote_endpoint(&exchange);

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
fn test_matching_engine_iteration_order_is_stable_across_rebuilds(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let first_instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut second = crypto_perpetual_ethusdt;
    second.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    second.raw_symbol = Symbol::from("BTCUSDT");
    second.base_currency = Currency::from("BTC");
    let second_instrument = InstrumentAny::CryptoPerpetual(second);
    let expected = vec![first_instrument.id(), second_instrument.id()];

    for _ in 0..32 {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        exchange
            .borrow_mut()
            .add_instrument(first_instrument.clone())
            .unwrap();
        exchange
            .borrow_mut()
            .add_instrument(second_instrument.clone())
            .unwrap();

        let actual = exchange
            .borrow()
            .get_matching_engines()
            .keys()
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn test_liquidation_closes_all_breached_currencies_in_one_pass(
    audusd_sim: CurrencyPair,
    #[case] reverse_balances: bool,
) {
    let usd = Currency::USD();
    let jpy = Currency::JPY();
    let audusd = InstrumentAny::CurrencyPair(audusd_sim.clone());
    let mut usdjpy = audusd_sim;
    usdjpy.id = InstrumentId::from("USD/JPY.SIM");
    usdjpy.raw_symbol = Symbol::from("USD/JPY");
    usdjpy.base_currency = usd;
    usdjpy.quote_currency = jpy;
    let usdjpy = InstrumentAny::CurrencyPair(usdjpy);
    let mut balances = vec![
        AccountBalance::new(Money::new(1.0, usd), Money::zero(usd), Money::new(1.0, usd)),
        AccountBalance::new(Money::new(1.0, jpy), Money::zero(jpy), Money::new(1.0, jpy)),
    ];

    if reverse_balances {
        balances.reverse();
    }

    let account = MarginAccount::new(
        AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Margin,
            balances.clone(),
            vec![
                MarginBalance::new(
                    Money::new(10.0, usd),
                    Money::new(10.0, usd),
                    Some(audusd.id()),
                ),
                MarginBalance::new(
                    Money::new(10.0, jpy),
                    Money::new(10.0, jpy),
                    Some(usdjpy.id()),
                ),
            ],
            false,
            UUID4::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        ),
        false,
    );
    let mut raw_cache = Cache::default();
    raw_cache.add_account(AccountAny::Margin(account)).unwrap();
    add_fx_position(
        &mut raw_cache,
        &audusd,
        "T-LIQUIDATION-USD",
        "100000",
        "2.00000",
    );
    add_fx_position(
        &mut raw_cache,
        &usdjpy,
        "T-LIQUIDATION-JPY",
        "100000",
        "200.00000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let config = SimulatedVenueConfig::builder()
        .venue(Venue::new("SIM"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(
            balances
                .iter()
                .map(|balance| balance.total)
                .collect::<Vec<_>>(),
        )
        .default_leverage(Decimal::ONE)
        .liquidation_enabled(true)
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel).into())
        .build()
        .unwrap();
    let exchange = Rc::new(RefCell::new(
        SimulatedExchange::new(config, cache.clone(), clock.clone()).unwrap(),
    ));
    let client = BacktestExecutionClient::new(
        TraderId::test_default(),
        AccountId::from("SIM-001"),
        &exchange,
        cache.clone(),
        clock,
        None,
        None,
    );
    exchange.borrow_mut().register_client(Rc::new(client));
    exchange
        .borrow_mut()
        .add_instrument(audusd.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .add_instrument(usdjpy.clone())
        .unwrap();
    add_fx_quote(&exchange, &cache, audusd.id(), "1.00000", "1.00010");
    add_fx_quote(&exchange, &cache, usdjpy.id(), "100.00000", "100.00010");
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    {
        let cache = cache.borrow();
        let account = cache.account_for_venue_owned(&Venue::new("SIM")).unwrap();
        let AccountAny::Margin(account) = account else {
            panic!("expected margin account");
        };
        let currencies = account.currencies();
        assert_eq!(currencies.len(), 2, "{currencies:?}");
        assert_eq!(account.total_maintenance_margin(usd), Money::new(10.0, usd));
        assert_eq!(account.total_maintenance_margin(jpy), Money::new(10.0, jpy));
        assert_eq!(
            cache
                .positions_open(Some(&Venue::new("SIM")), None, None, None, None)
                .len(),
            2
        );

        for position in cache.positions_open(Some(&Venue::new("SIM")), None, None, None, None) {
            assert!(cache.calculate_unrealized_pnl(&position).unwrap().as_f64() < 0.0);
        }
    }

    exchange
        .borrow_mut()
        .process_liquidations(UnixNanos::from(3));

    let mut liquidated = saving_handler
        .get_messages()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill)
                if fill.client_order_id.as_str().starts_with("LIQUIDATION-") =>
            {
                Some(fill.instrument_id)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    liquidated.sort_unstable();
    let mut expected = vec![audusd.id(), usdjpy.id()];
    expected.sort_unstable();
    assert_eq!(liquidated, expected);
}

#[rstest]
fn test_append_only_matching_engine_raw_ids_start_at_one_and_increment(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let first_instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut second = crypto_perpetual_ethusdt;
    second.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    second.raw_symbol = Symbol::from("BTCUSDT");
    second.base_currency = Currency::from("BTC");
    let second_instrument = InstrumentAny::CryptoPerpetual(second);
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );

    exchange
        .borrow_mut()
        .add_instrument(first_instrument.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .add_instrument(second_instrument.clone())
        .unwrap();

    let exchange = exchange.borrow();
    assert_eq!(
        exchange
            .get_matching_engine(&first_instrument.id())
            .unwrap()
            .raw_id,
        1
    );
    assert_eq!(
        exchange
            .get_matching_engine(&second_instrument.id())
            .unwrap()
            .raw_id,
        2
    );
}

#[rstest]
fn test_readded_instrument_does_not_collide_generated_fill_ids(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let saving_handler = register_order_event_saving_handler();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange_with_oms(
        Venue::new("BINANCE"),
        OmsType::Hedging,
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    let first_instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut second = crypto_perpetual_ethusdt;
    second.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    second.raw_symbol = Symbol::from("BTCUSDT");
    second.base_currency = Currency::from("BTC");
    let second_instrument = InstrumentAny::CryptoPerpetual(second);

    exchange
        .borrow_mut()
        .add_instrument(first_instrument.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .add_instrument(second_instrument.clone())
        .unwrap();

    let fill_timestamp = UnixNanos::from(2);
    exchange
        .borrow_mut()
        .process_quote_tick(&QuoteTick::new(
            first_instrument.id(),
            Price::from("1000.00"),
            Price::from("1001.00"),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ))
        .unwrap();
    let pre_readd_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(first_instrument.id())
        .client_order_id(ClientOrderId::from("O-READD-PRE"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1001.00"))
        .build();
    submit_matching_option_limit(&exchange, &cache, &pre_readd_order, fill_timestamp);

    exchange
        .borrow_mut()
        .add_instrument(first_instrument.clone())
        .unwrap();

    for instrument in [&first_instrument, &second_instrument] {
        exchange
            .borrow_mut()
            .process_quote_tick(&QuoteTick::new(
                instrument.id(),
                Price::from("1000.00"),
                Price::from("1001.00"),
                Quantity::from("10.000"),
                Quantity::from("10.000"),
                UnixNanos::from(1),
                UnixNanos::from(1),
            ))
            .unwrap();
    }

    let first_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(first_instrument.id())
        .client_order_id(ClientOrderId::from("O-READD-FIRST"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1001.00"))
        .build();
    let second_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(second_instrument.id())
        .client_order_id(ClientOrderId::from("O-READD-SECOND"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1001.00"))
        .build();

    submit_matching_option_limit(&exchange, &cache, &first_order, fill_timestamp);
    submit_matching_option_limit(&exchange, &cache, &second_order, fill_timestamp);

    let messages = saving_handler.get_messages();
    let pre_readd_fill = matching_option_fill(&messages, pre_readd_order.client_order_id());
    let first_fill = matching_option_fill(&messages, first_order.client_order_id());
    let second_fill = matching_option_fill(&messages, second_order.client_order_id());
    assert_eq!(pre_readd_fill.ts_event, first_fill.ts_event);
    assert_ne!(pre_readd_fill.venue_order_id, first_fill.venue_order_id);
    assert_ne!(pre_readd_fill.trade_id, first_fill.trade_id);
    assert_eq!(first_fill.ts_event, second_fill.ts_event);
    assert_ne!(first_fill.venue_order_id, second_fill.venue_order_id);
    assert_ne!(first_fill.position_id, second_fill.position_id);
    assert_ne!(first_fill.trade_id, second_fill.trade_id);
}

#[rstest]
fn test_same_timestamp_fills_follow_matching_engine_registration_order(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let saving_handler = register_order_event_saving_handler();
    let first_instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut second = crypto_perpetual_ethusdt;
    second.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    second.raw_symbol = Symbol::from("BTCUSDT");
    second.base_currency = Currency::from("BTC");
    let second_instrument = InstrumentAny::CryptoPerpetual(second);
    let instruments = [&first_instrument, &second_instrument];
    let expected = vec![first_instrument.id(), second_instrument.id()];

    for rebuild in 0..32 {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            Some(cache.clone()),
        );

        for (index, instrument) in instruments.iter().enumerate() {
            exchange
                .borrow_mut()
                .add_instrument((*instrument).clone())
                .unwrap();

            let initial_quote = QuoteTick::new(
                instrument.id(),
                Price::from("1000.00"),
                Price::from("1001.00"),
                Quantity::from("10.000"),
                Quantity::from("10.000"),
                UnixNanos::from(1),
                UnixNanos::from(1),
            );
            exchange
                .borrow_mut()
                .process_quote_tick(&initial_quote)
                .unwrap();

            let client_order_id = ClientOrderId::from(format!("O-{rebuild}-{index}").as_str());
            let order = OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(instrument.id())
                .client_order_id(client_order_id)
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1.000"))
                .price(Price::from("1000.00"))
                .build();
            submit_matching_option_limit(&exchange, &cache, &order, UnixNanos::from(2));

            let closed = InstrumentStatus::new(
                instrument.id(),
                MarketStatusAction::Close,
                UnixNanos::from(3),
                UnixNanos::from(3),
                None,
                None,
                None,
                None,
                None,
            );
            exchange
                .borrow_mut()
                .process_instrument_status(closed)
                .unwrap();

            let crossing_quote = QuoteTick::new(
                instrument.id(),
                Price::from("998.00"),
                Price::from("999.00"),
                Quantity::from("10.000"),
                Quantity::from("10.000"),
                UnixNanos::from(100),
                UnixNanos::from(100),
            );
            exchange
                .borrow_mut()
                .process_quote_tick(&crossing_quote)
                .unwrap();

            let reopened = InstrumentStatus::new(
                instrument.id(),
                MarketStatusAction::Trading,
                UnixNanos::from(100),
                UnixNanos::from(100),
                None,
                None,
                None,
                None,
                None,
            );
            exchange
                .borrow_mut()
                .process_instrument_status(reopened)
                .unwrap();
        }

        saving_handler.clear();
        exchange
            .borrow_mut()
            .iterate_matching_engines(UnixNanos::from(100));

        let actual = saving_handler
            .get_messages()
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill.instrument_id),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
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
    exchange
        .borrow_mut()
        .process_quote_tick(&quote_tick)
        .unwrap();

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
fn test_exchange_process_quote_tick_endpoint(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let quote_tick = QuoteTick::new(
        crypto_perpetual_ethusdt.id,
        Price::from("1000.00"),
        Price::from("1001.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    msgbus::send_quote(
        "SimulatedExchange.process_new_quote.BINANCE".into(),
        &quote_tick,
    );

    assert_eq!(
        exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id),
        Some(Price::from("1000.00"))
    );
    assert_eq!(
        exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id),
        Some(Price::from("1001.00"))
    );
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
        AggressorSide::Buy,
        TradeId::from("1"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    exchange
        .borrow_mut()
        .process_trade_tick(&trade_tick)
        .unwrap();

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
#[case::option_contract_call(
    matching_option_contract(OptionKind::Call),
    OrderSide::Buy,
    Price::from("102.00"),
    Price::from("101.00")
)]
#[case::option_contract_put(
    matching_option_contract(OptionKind::Put),
    OrderSide::Sell,
    Price::from("99.00"),
    Price::from("100.00")
)]
#[case::crypto_option_call(
    matching_crypto_option(OptionKind::Call),
    OrderSide::Buy,
    Price::from("102.00"),
    Price::from("101.00")
)]
#[case::crypto_option_put(
    matching_crypto_option(OptionKind::Put),
    OrderSide::Sell,
    Price::from("99.00"),
    Price::from("100.00")
)]
fn test_option_limit_order_crossing_bbo_fills_as_taker(
    #[case] instrument: InstrumentAny,
    #[case] side: OrderSide,
    #[case] limit_price: Price,
    #[case] expected_fill_price: Price,
) {
    let saving_handler = register_order_event_saving_handler();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        instrument.id().venue,
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let quote = matching_option_quote(&instrument, "100.00", "101.00", UnixNanos::from(1));
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();
    let order = matching_option_limit_order(
        instrument.id(),
        ClientOrderId::from("O-OPT-TAKER"),
        side,
        matching_option_quantity(&instrument),
        limit_price,
    );
    submit_matching_option_limit(&exchange, &cache, &order, UnixNanos::from(2));

    let messages = saving_handler.get_messages();
    let fill = matching_option_fill(&messages, order.client_order_id());
    assert_eq!(fill.instrument_id, instrument.id());
    assert_eq!(fill.order_side, side);
    assert_eq!(fill.last_px, expected_fill_price);
    assert_eq!(fill.last_qty, matching_option_quantity(&instrument));
    assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    assert!(
        exchange
            .borrow()
            .get_open_orders(Some(instrument.id()))
            .is_empty()
    );
}

#[rstest]
fn test_submit_order_list_routes_mixed_instrument_legs_to_own_matching_engine(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let saving_handler = register_order_event_saving_handler();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    let eth_instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut btcusdt = crypto_perpetual_ethusdt;
    btcusdt.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    btcusdt.raw_symbol = Symbol::from("BTCUSDT");
    btcusdt.base_currency = Currency::from("BTC");
    let btc_instrument = InstrumentAny::CryptoPerpetual(btcusdt);

    exchange
        .borrow_mut()
        .add_instrument(eth_instrument.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .add_instrument(btc_instrument.clone())
        .unwrap();

    let eth_quote = QuoteTick::new(
        eth_instrument.id(),
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let btc_quote = QuoteTick::new(
        btc_instrument.id(),
        Price::from("200.00"),
        Price::from("201.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    exchange
        .borrow_mut()
        .process_quote_tick(&eth_quote)
        .unwrap();
    exchange
        .borrow_mut()
        .process_quote_tick(&btc_quote)
        .unwrap();

    let eth_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(eth_instrument.id())
        .client_order_id(ClientOrderId::from("O-MIXED-ETH"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();
    let btc_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(btc_instrument.id())
        .client_order_id(ClientOrderId::from("O-MIXED-BTC"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();
    let orders = vec![eth_order.clone(), btc_order.clone()];
    let account_id = AccountId::test_default();

    for order in &orders {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .update_order(&TestOrderEventStubs::submitted(order, account_id))
            .unwrap();
    }

    let ts_init = UnixNanos::from(2);
    let order_list = OrderList::new(
        OrderListId::from("OL-MIXED-001"),
        eth_order.instrument_id(),
        StrategyId::test_default(),
        orders.iter().map(OrderAny::client_order_id).collect(),
        ts_init,
    );
    let command = SubmitOrderList::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        order_list,
        orders
            .iter()
            .map(|order| order.init_event().clone())
            .collect(),
        None,
        None,
        None,
        UUID4::default(),
        ts_init,
        None,
    );

    exchange
        .borrow_mut()
        .send(TradingCommand::SubmitOrderList(command));
    exchange.borrow_mut().process(ts_init);

    let messages = saving_handler.get_messages();
    let fill_price = |client_order_id: ClientOrderId| -> Price {
        messages
            .iter()
            .find_map(|event| match event {
                OrderEventAny::Filled(fill) if fill.client_order_id == client_order_id => {
                    Some(fill.last_px)
                }
                _ => None,
            })
            .expect("expected mixed instrument order-list leg fill")
    };

    assert_eq!(
        fill_price(eth_order.client_order_id()),
        Price::from("101.00")
    );
    assert_eq!(
        fill_price(btc_order.client_order_id()),
        Price::from("201.00")
    );
}

#[rstest]
fn test_open_order_accessors_filter_by_instrument_id(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let _saving_handler = register_order_event_saving_handler();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    let eth_instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut btcusdt = crypto_perpetual_ethusdt;
    btcusdt.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    btcusdt.raw_symbol = Symbol::from("BTCUSDT");
    btcusdt.base_currency = Currency::from("BTC");
    let btc_instrument = InstrumentAny::CryptoPerpetual(btcusdt);
    let unknown_instrument_id = InstrumentId::from("SOLUSDT-PERP.BINANCE");

    exchange
        .borrow_mut()
        .add_instrument(eth_instrument.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .add_instrument(btc_instrument.clone())
        .unwrap();

    let eth_quote = QuoteTick::new(
        eth_instrument.id(),
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let btc_quote = QuoteTick::new(
        btc_instrument.id(),
        Price::from("200.00"),
        Price::from("201.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    exchange
        .borrow_mut()
        .process_quote_tick(&eth_quote)
        .unwrap();
    exchange
        .borrow_mut()
        .process_quote_tick(&btc_quote)
        .unwrap();

    // Both prices are away from their market, so each order rests rather than fills.
    let eth_bid = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(eth_instrument.id())
        .client_order_id(ClientOrderId::from("O-RESTING-ETH-BID"))
        .side(OrderSide::Buy)
        .price(Price::from("90.00"))
        .quantity(Quantity::from("1.000"))
        .build();
    let btc_ask = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(btc_instrument.id())
        .client_order_id(ClientOrderId::from("O-RESTING-BTC-ASK"))
        .side(OrderSide::Sell)
        .price(Price::from("210.00"))
        .quantity(Quantity::from("1.000"))
        .build();
    submit_matching_option_limit(&exchange, &cache, &eth_bid, UnixNanos::from(2));
    submit_matching_option_limit(&exchange, &cache, &btc_ask, UnixNanos::from(3));

    let exchange = exchange.borrow();

    // No filter aggregates every matching engine.
    assert_eq!(exchange.get_open_orders(None).len(), 2);
    assert_eq!(exchange.get_open_bid_orders(None).len(), 1);
    assert_eq!(exchange.get_open_ask_orders(None).len(), 1);

    // A known instrument returns only its own orders.
    assert_eq!(exchange.get_open_orders(Some(eth_instrument.id())).len(), 1);
    assert_eq!(
        exchange
            .get_open_bid_orders(Some(eth_instrument.id()))
            .len(),
        1
    );
    assert!(
        exchange
            .get_open_ask_orders(Some(eth_instrument.id()))
            .is_empty()
    );
    assert_eq!(exchange.get_open_orders(Some(btc_instrument.id())).len(), 1);
    assert!(
        exchange
            .get_open_bid_orders(Some(btc_instrument.id()))
            .is_empty()
    );
    assert_eq!(
        exchange
            .get_open_ask_orders(Some(btc_instrument.id()))
            .len(),
        1
    );

    // An instrument with no matching engine returns nothing, rather than falling
    // through to the unfiltered branch.
    assert!(
        exchange
            .get_open_orders(Some(unknown_instrument_id))
            .is_empty()
    );
    assert!(
        exchange
            .get_open_bid_orders(Some(unknown_instrument_id))
            .is_empty()
    );
    assert!(
        exchange
            .get_open_ask_orders(Some(unknown_instrument_id))
            .is_empty()
    );
}

#[rstest]
#[case::option_contract_call(
    matching_option_contract(OptionKind::Call),
    OrderSide::Buy,
    Price::from("100.00")
)]
#[case::option_contract_put(
    matching_option_contract(OptionKind::Put),
    OrderSide::Sell,
    Price::from("101.00")
)]
#[case::crypto_option_call(
    matching_crypto_option(OptionKind::Call),
    OrderSide::Buy,
    Price::from("100.00")
)]
#[case::crypto_option_put(
    matching_crypto_option(OptionKind::Put),
    OrderSide::Sell,
    Price::from("101.00")
)]
fn test_option_resting_limit_order_fills_as_maker_when_bbo_trades_through(
    #[case] instrument: InstrumentAny,
    #[case] side: OrderSide,
    #[case] limit_price: Price,
) {
    let saving_handler = register_order_event_saving_handler();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let exchange = get_exchange(
        instrument.id().venue,
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let quote = matching_option_quote(&instrument, "100.00", "101.00", UnixNanos::from(1));
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();
    let order = matching_option_limit_order(
        instrument.id(),
        ClientOrderId::from("O-OPT-MAKER"),
        side,
        matching_option_quantity(&instrument),
        limit_price,
    );
    submit_matching_option_limit(&exchange, &cache, &order, UnixNanos::from(2));

    assert!(
        saving_handler
            .get_messages()
            .iter()
            .all(|event| !matches!(event, OrderEventAny::Filled(_)))
    );
    assert_eq!(
        exchange
            .borrow()
            .get_open_orders(Some(instrument.id()))
            .len(),
        1
    );

    let trade_through_quote = matching_option_trade_through_quote(&instrument, side);
    exchange
        .borrow_mut()
        .process_quote_tick(&trade_through_quote)
        .unwrap();

    let messages = saving_handler.get_messages();
    let fill = matching_option_fill(&messages, order.client_order_id());
    assert_eq!(fill.instrument_id, instrument.id());
    assert_eq!(fill.order_side, side);
    assert_eq!(fill.last_px, limit_price);
    assert_eq!(fill.last_qty, matching_option_quantity(&instrument));
    assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
    assert!(
        exchange
            .borrow()
            .get_open_orders(Some(instrument.id()))
            .is_empty()
    );
}

fn register_order_event_saving_handler() -> TypedIntoMessageSavingHandler<OrderEventAny> {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
    saving_handler
}

fn matching_option_limit_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    quantity: Quantity,
    price: Price,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(side)
        .quantity(quantity)
        .price(price)
        .build()
}

fn submit_matching_option_limit(
    exchange: &Rc<RefCell<SimulatedExchange>>,
    cache: &Rc<RefCell<Cache>>,
    order: &OrderAny,
    ts_init: UnixNanos,
) {
    let account_id = AccountId::test_default();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(order, account_id))
        .unwrap();

    let command = TradingCommand::SubmitOrder(SubmitOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        order.instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::default(),
        ts_init,
        None,
    ));
    exchange.borrow_mut().send(command);
    exchange.borrow_mut().process(ts_init);
}

fn matching_option_fill(
    messages: &[OrderEventAny],
    client_order_id: ClientOrderId,
) -> &OrderFilled {
    messages
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) if fill.client_order_id == client_order_id => Some(fill),
            _ => None,
        })
        .expect("Expected option order fill")
}

fn matching_option_contract(kind: OptionKind) -> InstrumentAny {
    let venue = Venue::new("OPRA");
    let symbol = match kind {
        OptionKind::Call => "AAPL240315C00150000",
        OptionKind::Put => "AAPL240315P00150000",
    };
    InstrumentAny::OptionContract(
        OptionContract::builder()
            .instrument_id(InstrumentId::from(format!("{symbol}.{venue}").as_str()))
            .raw_symbol(Symbol::from(symbol))
            .asset_class(AssetClass::Equity)
            .exchange(Ustr::from(venue.as_str()))
            .underlying(Ustr::from("AAPL"))
            .option_kind(kind)
            .strike_price(Price::from("150.00"))
            .currency(Currency::USD())
            .activation_ns(UnixNanos::default())
            .expiration_ns(UnixNanos::from(2_000_000_000_000_000_000u64))
            .price_precision(2)
            .price_increment(Price::from("0.01"))
            .multiplier(Quantity::from(100))
            .lot_size(Quantity::from(1))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

fn matching_crypto_option(kind: OptionKind) -> InstrumentAny {
    let venue = Venue::new("DERIBIT");
    let symbol = match kind {
        OptionKind::Call => "BTC-28JUN24-50000-C",
        OptionKind::Put => "BTC-28JUN24-50000-P",
    };
    InstrumentAny::CryptoOption(
        CryptoOption::builder()
            .instrument_id(InstrumentId::from(format!("{symbol}.{venue}").as_str()))
            .raw_symbol(Symbol::from(symbol))
            .underlying(Currency::from("BTC"))
            .quote_currency(Currency::from("USD"))
            .settlement_currency(Currency::from("BTC"))
            .is_inverse(false)
            .option_kind(kind)
            .strike_price(Price::from("50000.00"))
            .activation_ns(UnixNanos::default())
            .expiration_ns(UnixNanos::from(2_000_000_000_000_000_000u64))
            .price_precision(2)
            .size_precision(1)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("0.1"))
            .multiplier(Quantity::from(1))
            .lot_size(Quantity::from(1))
            .min_quantity(Quantity::from("0.1"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

fn matching_option_quote(
    instrument: &InstrumentAny,
    bid: &str,
    ask: &str,
    ts: UnixNanos,
) -> QuoteTick {
    QuoteTick::new(
        instrument.id(),
        Price::from(bid),
        Price::from(ask),
        matching_option_quantity(instrument),
        matching_option_quantity(instrument),
        ts,
        ts,
    )
}

fn matching_option_trade_through_quote(instrument: &InstrumentAny, side: OrderSide) -> QuoteTick {
    match side {
        OrderSide::Buy => matching_option_quote(instrument, "98.00", "99.00", UnixNanos::from(3)),
        OrderSide::Sell => {
            matching_option_quote(instrument, "102.00", "103.00", UnixNanos::from(3))
        }
    }
}

fn matching_option_quantity(instrument: &InstrumentAny) -> Quantity {
    if instrument.size_precision() == 0 {
        Quantity::from(1)
    } else {
        Quantity::from("1.0")
    }
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
    exchange.borrow_mut().process_bar(bar).unwrap();

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
    exchange.borrow_mut().process_bar(bar_bid).unwrap();
    exchange.borrow_mut().process_bar(bar_ask).unwrap();

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
    exchange
        .borrow_mut()
        .process_order_book_delta(delta_buy)
        .unwrap();
    exchange
        .borrow_mut()
        .process_order_book_delta(delta_sell)
        .unwrap();

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
        .process_order_book_deltas(&orderbook_deltas)
        .unwrap();

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
        .process_instrument_status(instrument_status)
        .unwrap();

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

#[rstest]
fn test_adjust_account_overflow_emits_no_state() {
    let account_type = AccountType::Margin;
    let usd = Currency::USD();
    let maximum = Money::from_raw(MONEY_RAW_MAX, usd);
    let mut cache = Cache::default();
    let (handler, saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);
    let margin_account = MarginAccount::new(
        AccountState::new(
            AccountId::from("SIM-001"),
            account_type,
            vec![AccountBalance::new(maximum, Money::zero(usd), maximum)],
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
    let exchange = get_exchange(
        Venue::new("SIM"),
        account_type,
        BookType::L2_MBP,
        Some(Rc::new(RefCell::new(cache))),
    );
    exchange.borrow_mut().initialize_account();

    let adjusted = exchange
        .borrow_mut()
        .adjust_account(Money::from("0.01 USD"));

    assert!(!adjusted);
    assert_eq!(saving_handler.get_messages().len(), 1);
}

#[rstest]
fn test_process_funding_rate_settles_open_position(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let account_id = AccountId::from("BINANCE-001");
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut cache = Cache::default();
    pre_populate_margin_account_with_balance(&mut cache, "BINANCE-001", Money::from("1000 USDT"));
    cache.add_instrument(instrument.clone()).unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-001")),
        None,
        Some(Price::from("1000.00")),
        Some(Quantity::from("1.000")),
        None,
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(1)),
        Some(account_id),
    );
    let position = Position::new(&instrument, fill.into());
    let position_id = position.id;
    cache.add_position(&position, OmsType::Netting).unwrap();
    cache
        .add_mark_price(MarkPriceUpdate::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let cache = Rc::new(RefCell::new(cache));
    let (account_handler, account_saver) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), account_handler);
    let (position_handler, position_saver) =
        get_typed_message_saving_handler::<PositionEvent>(None);
    msgbus::subscribe_position_events("events.position.*".into(), position_handler, None);
    let (settlement_handler, settlement_saver) = get_any_saving_handler::<FundingSettlement>(None);
    msgbus::subscribe_any(
        "events.funding_settlements.*".into(),
        settlement_handler,
        None,
    );

    let exchange = build_exchange_with_options(
        Venue::new("BINANCE"),
        AccountType::Margin,
        false,
        false,
        cache.clone(),
    );
    exchange.borrow_mut().add_instrument(instrument).unwrap();
    let settlement_ns = UnixNanos::from(3);
    let scheduled_first = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.002").unwrap(),
            Some(480),
            Some(settlement_ns),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ));
    let scheduled = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.001").unwrap(),
            Some(480),
            Some(settlement_ns),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ));
    assert_eq!(scheduled_first.unwrap(), Some(settlement_ns));
    assert_eq!(scheduled.unwrap(), Some(settlement_ns));
    assert!(account_saver.get_messages().is_empty());
    assert!(position_saver.get_messages().is_empty());
    assert!(settlement_saver.get_messages().is_empty());

    exchange
        .borrow_mut()
        .process_funding_settlement(crypto_perpetual_ethusdt.id, settlement_ns)
        .unwrap();

    let position = cache.borrow().position_owned(&position_id).unwrap();
    let account_states = account_saver.get_messages();
    let position_events = position_saver.get_messages();
    let settlements = settlement_saver.get_messages();
    let [settlement] = settlements.as_slice() else {
        panic!("expected one FundingSettlement");
    };
    let [PositionEvent::PositionAdjusted(adjustment)] = position_events.as_slice() else {
        panic!("expected one PositionAdjusted event");
    };
    let [account_state] = account_states.as_slice() else {
        panic!("expected one AccountState");
    };

    assert_eq!(settlement.rate, Decimal::from_str("0.001").unwrap());
    assert_eq!(settlement.ts_event, settlement_ns);
    assert_eq!(position.adjustments.len(), 1);
    assert_eq!(position.realized_pnl, Some(Money::from("-1 USDT")));
    assert_eq!(adjustment.adjustment_type, PositionAdjustmentType::Funding);
    assert_eq!(adjustment.pnl_change, Some(Money::from("-1 USDT")));
    assert_eq!(account_state.balances[0].total, Money::from("999 USDT"));
}

#[rstest]
fn test_process_funding_rate_restores_position_when_database_update_fails(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let account_id = AccountId::from("BINANCE-001");
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let (database, database_control) = TestCacheDatabaseControl::create();
    let mut cache = Cache::new(None, Some(Box::new(database)));
    pre_populate_margin_account_with_balance(&mut cache, "BINANCE-001", Money::from("1000 USDT"));
    cache.add_instrument(instrument.clone()).unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-001")),
        None,
        Some(Price::from("1000.00")),
        Some(Quantity::from("1.000")),
        None,
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(1)),
        Some(account_id),
    );
    let position = Position::new(&instrument, fill.into());
    let position_id = position.id;
    let adjustments_before = position.adjustments.clone();
    let realized_pnl_before = position.realized_pnl;
    cache.add_position(&position, OmsType::Netting).unwrap();
    cache
        .add_mark_price(MarkPriceUpdate::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let cache = Rc::new(RefCell::new(cache));
    let (position_handler, position_saver) =
        get_typed_message_saving_handler::<PositionEvent>(None);
    msgbus::subscribe_position_events("events.position.*".into(), position_handler, None);
    let (settlement_handler, settlement_saver) = get_any_saving_handler::<FundingSettlement>(None);
    msgbus::subscribe_any(
        "events.funding_settlements.*".into(),
        settlement_handler,
        None,
    );
    let exchange = build_exchange_with_options(
        Venue::new("BINANCE"),
        AccountType::Margin,
        false,
        false,
        cache.clone(),
    );
    exchange.borrow_mut().add_instrument(instrument).unwrap();
    let settlement_ns = UnixNanos::from(3);
    exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.001").unwrap(),
            Some(480),
            Some(settlement_ns),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();
    database_control.set_fail_update_position(true);

    exchange
        .borrow_mut()
        .process_funding_settlement(crypto_perpetual_ethusdt.id, settlement_ns)
        .unwrap();

    let cached_position = cache.borrow().position_owned(&position_id).unwrap();
    assert_eq!(cached_position.adjustments, adjustments_before);
    assert_eq!(cached_position.realized_pnl, realized_pnl_before);
    assert!(position_saver.get_messages().is_empty());
    assert!(settlement_saver.get_messages().is_empty());

    database_control.set_fail_update_position(false);
    exchange
        .borrow_mut()
        .process_funding_settlement(crypto_perpetual_ethusdt.id, settlement_ns)
        .unwrap();
    assert_eq!(settlement_saver.get_messages().len(), 1);
}

#[rstest]
fn test_process_funding_rate_returns_instrument_boundary() {
    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let first_boundary = UnixNanos::from(3);
    let second_boundary = UnixNanos::from(4);
    let first_instrument = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let second_instrument = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    let first_scheduled = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            first_instrument,
            Decimal::from_str("0.001").unwrap(),
            Some(480),
            Some(first_boundary),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ));
    let second_scheduled = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            second_instrument,
            Decimal::from_str("0.002").unwrap(),
            Some(480),
            Some(second_boundary),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ));

    assert_eq!(first_scheduled.unwrap(), Some(first_boundary));
    assert_eq!(second_scheduled.unwrap(), Some(second_boundary));
}

#[rstest]
fn test_process_funding_rate_invalid_notional_emits_nothing_and_can_retry() {
    let inverse = xbtusd_bitmex();
    let instrument = InstrumentAny::CryptoPerpetual(inverse.clone());
    let account_id = AccountId::from("BITMEX-001");
    let mut cache = Cache::default();
    pre_populate_margin_account_with_balance(&mut cache, "BITMEX-001", Money::from("100 BTC"));
    cache.add_instrument(instrument.clone()).unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(inverse.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-INVERSE-ZERO")),
        None,
        Some(Price::from("10000.0")),
        Some(Quantity::from("100000")),
        None,
        Some(Money::from("0 BTC")),
        Some(UnixNanos::from(1)),
        Some(account_id),
    );
    let position = Position::new(&instrument, fill.into());
    cache.add_position(&position, OmsType::Netting).unwrap();
    cache
        .add_mark_price(MarkPriceUpdate::new(
            inverse.id,
            Price::from("0.0"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let cache = Rc::new(RefCell::new(cache));
    let (settlement_handler, settlement_saver) = get_any_saving_handler::<FundingSettlement>(None);
    msgbus::subscribe_any(
        "events.funding_settlements.*".into(),
        settlement_handler,
        None,
    );
    let exchange = build_exchange_with_options(
        Venue::new("BITMEX"),
        AccountType::Margin,
        false,
        false,
        cache.clone(),
    );
    exchange.borrow_mut().add_instrument(instrument).unwrap();
    let settlement_ns = UnixNanos::from(3);
    exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            inverse.id,
            Decimal::from_str("0.001").unwrap(),
            Some(480),
            Some(settlement_ns),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    exchange
        .borrow_mut()
        .process_funding_settlement(inverse.id, settlement_ns)
        .unwrap();
    assert!(settlement_saver.get_messages().is_empty());
    assert!(
        cache
            .borrow()
            .position(&position.id)
            .unwrap()
            .adjustments
            .is_empty()
    );

    cache
        .borrow_mut()
        .add_mark_price(MarkPriceUpdate::new(
            inverse.id,
            Price::from("10000.0"),
            UnixNanos::from(3),
            UnixNanos::from(3),
        ))
        .unwrap();
    exchange
        .borrow_mut()
        .process_funding_settlement(inverse.id, settlement_ns)
        .unwrap();

    assert_eq!(settlement_saver.get_messages().len(), 1);
    assert_eq!(
        cache
            .borrow()
            .position(&position.id)
            .unwrap()
            .adjustments
            .len(),
        1
    );
}

#[rstest]
fn test_process_funding_rate_uses_midpoint_and_credits_short_position(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let account_id = AccountId::from("BINANCE-001");
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut cache = Cache::default();
    pre_populate_margin_account_with_balance(&mut cache, "BINANCE-001", Money::from("1000 USDT"));
    cache.add_instrument(instrument.clone()).unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-001")),
        None,
        Some(Price::from("1000.00")),
        Some(Quantity::from("1.000")),
        None,
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(1)),
        Some(account_id),
    );
    let position = Position::new(&instrument, fill.into());
    let position_id = position.id;
    cache.add_position(&position, OmsType::Netting).unwrap();

    let cache = Rc::new(RefCell::new(cache));
    let (account_handler, account_saver) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), account_handler);
    let exchange = build_exchange_with_options(
        Venue::new("BINANCE"),
        AccountType::Margin,
        false,
        false,
        cache.clone(),
    );
    exchange.borrow_mut().add_instrument(instrument).unwrap();
    exchange
        .borrow_mut()
        .process_order_book_delta(OrderBookDelta::new(
            crypto_perpetual_ethusdt.id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("999.00"),
                Quantity::from("1.000"),
                1,
            ),
            0,
            0,
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();
    exchange
        .borrow_mut()
        .process_order_book_delta(OrderBookDelta::new(
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
        ))
        .unwrap();

    let settlement_ns = UnixNanos::from(3);
    let scheduled = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.001").unwrap(),
            Some(480),
            Some(settlement_ns),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ));
    exchange
        .borrow_mut()
        .process_funding_settlement(crypto_perpetual_ethusdt.id, settlement_ns)
        .unwrap();

    let position = cache.borrow().position_owned(&position_id).unwrap();
    let account_states = account_saver.get_messages();
    let [account_state] = account_states.as_slice() else {
        panic!("expected one AccountState");
    };

    assert_eq!(scheduled.unwrap(), Some(settlement_ns));
    assert_eq!(position.realized_pnl, Some(Money::from("1 USDT")));
    assert_eq!(account_state.balances[0].total, Money::from("1001 USDT"));
}

#[rstest]
fn test_process_funding_rate_without_open_positions_emits_no_settlement(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut cache = Cache::default();
    pre_populate_margin_account_with_balance(&mut cache, "BINANCE-001", Money::from("1000 USDT"));
    cache.add_instrument(instrument.clone()).unwrap();
    cache
        .add_mark_price(MarkPriceUpdate::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let cache = Rc::new(RefCell::new(cache));
    let (account_handler, account_saver) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), account_handler);
    let (position_handler, position_saver) =
        get_typed_message_saving_handler::<PositionEvent>(None);
    msgbus::subscribe_position_events("events.position.*".into(), position_handler, None);
    let (settlement_handler, settlement_saver) = get_any_saving_handler::<FundingSettlement>(None);
    msgbus::subscribe_any(
        "events.funding_settlements.*".into(),
        settlement_handler,
        None,
    );

    let exchange = build_exchange_with_options(
        Venue::new("BINANCE"),
        AccountType::Margin,
        false,
        false,
        cache,
    );
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let settlement_ns = UnixNanos::from(3);
    let scheduled = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.001").unwrap(),
            Some(480),
            Some(settlement_ns),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ));
    exchange
        .borrow_mut()
        .process_funding_settlement(crypto_perpetual_ethusdt.id, settlement_ns)
        .unwrap();

    assert_eq!(scheduled.unwrap(), Some(settlement_ns));
    assert!(account_saver.get_messages().is_empty());
    assert!(position_saver.get_messages().is_empty());
    assert!(settlement_saver.get_messages().is_empty());
}

#[rstest]
fn test_process_funding_rate_does_not_double_settle_boundary_update(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let account_id = AccountId::from("BINANCE-001");
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut cache = Cache::default();
    pre_populate_margin_account_with_balance(&mut cache, "BINANCE-001", Money::from("1000 USDT"));
    cache.add_instrument(instrument.clone()).unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-001")),
        None,
        Some(Price::from("1000.00")),
        Some(Quantity::from("1.000")),
        None,
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(1)),
        Some(account_id),
    );
    let position = Position::new(&instrument, fill.into());
    let position_id = position.id;
    cache.add_position(&position, OmsType::Netting).unwrap();
    cache
        .add_mark_price(MarkPriceUpdate::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let cache = Rc::new(RefCell::new(cache));
    let (account_handler, account_saver) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), account_handler);
    let exchange = build_exchange_with_options(
        Venue::new("BINANCE"),
        AccountType::Margin,
        false,
        false,
        cache.clone(),
    );
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let settlement_ns = UnixNanos::from(3);
    let scheduled = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.001").unwrap(),
            Some(480),
            Some(settlement_ns),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ));
    exchange.borrow().set_clock_time(settlement_ns);
    exchange
        .borrow_mut()
        .process_funding_settlement(crypto_perpetual_ethusdt.id, settlement_ns)
        .unwrap();
    let immediate = exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.002").unwrap(),
            Some(480),
            Some(settlement_ns),
            settlement_ns,
            settlement_ns,
        ));

    let position = cache.borrow().position_owned(&position_id).unwrap();
    let account_states = account_saver.get_messages();

    assert_eq!(scheduled.unwrap(), Some(settlement_ns));
    assert_eq!(immediate.unwrap(), None);
    assert_eq!(account_states.len(), 1);
    assert_eq!(position.realized_pnl, Some(Money::from("-1 USDT")));
    assert_eq!(account_states[0].balances[0].total, Money::from("999 USDT"));
}

#[rstest]
fn test_process_funding_rate_settles_only_on_interval_boundary(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let account_id = AccountId::from("BINANCE-001");
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
    let mut cache = Cache::default();
    pre_populate_margin_account_with_balance(&mut cache, "BINANCE-001", Money::from("1000 USDT"));
    cache.add_instrument(instrument.clone()).unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(crypto_perpetual_ethusdt.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-001")),
        None,
        Some(Price::from("1000.00")),
        Some(Quantity::from("1.000")),
        None,
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(1)),
        Some(account_id),
    );
    let position = Position::new(&instrument, fill.into());
    let position_id = position.id;
    cache.add_position(&position, OmsType::Netting).unwrap();
    cache
        .add_mark_price(MarkPriceUpdate::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let cache = Rc::new(RefCell::new(cache));
    let (account_handler, account_saver) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), account_handler);
    let exchange = build_exchange_with_options(
        Venue::new("BINANCE"),
        AccountType::Margin,
        false,
        false,
        cache.clone(),
    );
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    let off_boundary_ns = UnixNanos::from(60_000_000_001);
    exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.001").unwrap(),
            Some(1),
            None,
            off_boundary_ns,
            off_boundary_ns,
        ))
        .unwrap();
    assert!(account_saver.get_messages().is_empty());

    let boundary_ns = UnixNanos::from(120_000_000_000);
    exchange.borrow().set_clock_time(boundary_ns);
    exchange
        .borrow_mut()
        .process_funding_rate(FundingRateUpdate::new(
            crypto_perpetual_ethusdt.id,
            Decimal::from_str("0.001").unwrap(),
            Some(1),
            None,
            boundary_ns,
            boundary_ns,
        ))
        .unwrap();

    let position = cache.borrow().position_owned(&position_id).unwrap();
    let account_states = account_saver.get_messages();

    assert_eq!(account_states.len(), 1);
    assert_eq!(position.realized_pnl, Some(Money::from("-1 USDT")));
    assert_eq!(account_states[0].balances[0].total, Money::from("999 USDT"));
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
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel).into())
        .frozen_account(frozen_account)
        .allow_cash_borrowing(allow_cash_borrowing)
        .build()
        .unwrap();
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
    pre_populate_margin_account_with_balance(cache, account_id, Money::from("1000 USD"));
}

fn pre_populate_margin_account_with_balance(cache: &mut Cache, account_id: &str, balance: Money) {
    let margin_account = MarginAccount::new(
        AccountState::new(
            AccountId::from(account_id),
            AccountType::Margin,
            vec![AccountBalance::new(
                balance,
                Money::zero(balance.currency),
                balance,
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
        .set_latency_model(LatencyModelHandle::new(latency_model));
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
fn test_due_inflight_commands_drain_after_queued_commands(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let exchange = get_exchange(
        Venue::new("BINANCE"),
        AccountType::Margin,
        BookType::L2_MBP,
        None,
    );
    exchange
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt))
        .unwrap();

    let account_id = AccountId::test_default();
    let (queued_order, queued_cmd) = create_submit_order_command(UnixNanos::from(100), "O-QUEUED");
    let (inflight_order, inflight_cmd) =
        create_submit_order_command(UnixNanos::from(100), "O-INFLIGHT");

    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(queued_order.clone(), None, None, false)
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&queued_order, account_id))
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .add_order(inflight_order.clone(), None, None, false)
        .unwrap();
    exchange
        .borrow()
        .cache()
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&inflight_order, account_id))
        .unwrap();

    exchange.borrow_mut().send(queued_cmd);
    exchange
        .borrow_mut()
        .set_latency_model(LatencyModelHandle::new(StaticLatencyModel::new(
            UnixNanos::from(0),
            UnixNanos::from(0),
            UnixNanos::from(0),
            UnixNanos::from(0),
        )));
    exchange.borrow_mut().send(inflight_cmd);
    exchange.borrow_mut().process(UnixNanos::from(100));

    let messages = saving_handler.get_messages();
    let accepted = messages
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(accepted),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        accepted
            .iter()
            .map(|event| event.client_order_id)
            .collect::<Vec<_>>(),
        vec![
            ClientOrderId::new("O-QUEUED"),
            ClientOrderId::new("O-INFLIGHT")
        ]
    );
    assert_eq!(
        accepted
            .iter()
            .map(|event| (event.ts_event, event.ts_init))
            .collect::<Vec<_>>(),
        vec![
            (UnixNanos::from(100), UnixNanos::from(100)),
            (UnixNanos::from(100), UnixNanos::from(100))
        ]
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
        .set_latency_model(LatencyModelHandle::new(latency_model));
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
        .set_latency_model(LatencyModelHandle::new(latency_model));
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
        .set_latency_model(LatencyModelHandle::new(latency_model));
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
    assert_eq!(updated.ts_event, UnixNanos::from(1));
    assert_eq!(updated.ts_init, UnixNanos::from(1));
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
        .set_latency_model(LatencyModelHandle::new(latency_model));

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
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();

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

#[derive(Default)]
struct CapturingLogger {
    messages: Mutex<Vec<(Level, String)>>,
}

impl CapturingLogger {
    fn clear(&self) {
        self.messages.lock().clear();
    }

    fn messages(&self) -> Vec<(Level, String)> {
        self.messages.lock().clone()
    }
}

impl Log for CapturingLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Warn
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.messages
                .lock()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

static CAPTURING_LOGGER: CapturingLogger = CapturingLogger {
    messages: Mutex::new(Vec::new()),
};
static CAPTURING_LOGGER_TEST_LOCK: Mutex<()> = Mutex::new(());

struct AdjustmentSimulationModule {
    label: &'static str,
    adjustments: Vec<Money>,
    outcomes: Rc<RefCell<Vec<AccountAdjustmentOutcome>>>,
    sequence: Rc<RefCell<Vec<String>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailingModuleHook {
    PreProcess,
    Process,
    Acknowledge,
    LogDiagnostics,
    Reset,
}

#[derive(Debug)]
struct FailingSimulationModule {
    hook: FailingModuleHook,
}

impl SimulationModule for FailingSimulationModule {
    fn pre_process(&self, _data: &Data) -> anyhow::Result<()> {
        if self.hook == FailingModuleHook::PreProcess {
            anyhow::bail!("module boom");
        }
        Ok(())
    }

    fn process(
        &self,
        _ts_now: UnixNanos,
        _ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult> {
        if self.hook == FailingModuleHook::Process {
            anyhow::bail!("module boom");
        }
        Ok(if self.hook == FailingModuleHook::Acknowledge {
            SimulationModuleResult::Completed(Vec::new())
        } else {
            SimulationModuleResult::NotReady
        })
    }

    fn acknowledge(&self, _outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
        if self.hook == FailingModuleHook::Acknowledge {
            anyhow::bail!("module boom");
        }
        Ok(())
    }

    fn log_diagnostics(&self) -> anyhow::Result<()> {
        if self.hook == FailingModuleHook::LogDiagnostics {
            anyhow::bail!("module boom");
        }
        Ok(())
    }

    fn reset(&self) -> anyhow::Result<()> {
        if self.hook == FailingModuleHook::Reset {
            anyhow::bail!("module boom");
        }
        Ok(())
    }
}

impl SimulationModule for AdjustmentSimulationModule {
    fn pre_process(&self, _data: &Data) -> anyhow::Result<()> {
        Ok(())
    }

    fn process(
        &self,
        _ts_now: UnixNanos,
        _ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult> {
        self.sequence
            .borrow_mut()
            .push(format!("process-{}", self.label));

        Ok(SimulationModuleResult::Completed(self.adjustments.clone()))
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
        self.outcomes.borrow_mut().extend_from_slice(outcomes);
        Ok(())
    }

    fn log_diagnostics(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl MockSimulationModule {
    fn new(counts: MockModuleCounts) -> Self {
        Self { counts }
    }
}

impl SimulationModule for MockSimulationModule {
    fn pre_process(&self, _data: &Data) -> anyhow::Result<()> {
        self.counts
            .pre_process
            .set(self.counts.pre_process.get() + 1);
        Ok(())
    }

    fn process(
        &self,
        _ts_now: UnixNanos,
        _ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult> {
        self.counts.process.set(self.counts.process.get() + 1);
        Ok(SimulationModuleResult::Completed(Vec::new()))
    }

    fn acknowledge(&self, _outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
        Ok(())
    }

    fn log_diagnostics(&self) -> anyhow::Result<()> {
        self.counts
            .log_diagnostics
            .set(self.counts.log_diagnostics.get() + 1);
        Ok(())
    }

    fn reset(&self) -> anyhow::Result<()> {
        self.counts.reset.set(self.counts.reset.get() + 1);
        Ok(())
    }
}

fn get_exchange_with_module(
    venue: Venue,
    counts: MockModuleCounts,
) -> Rc<RefCell<SimulatedExchange>> {
    get_exchange_with_modules(
        venue,
        vec![SimulationModuleHandle::new(MockSimulationModule::new(
            counts,
        ))],
    )
}

fn get_exchange_with_modules(
    venue: Venue,
    modules: Vec<SimulationModuleHandle>,
) -> Rc<RefCell<SimulatedExchange>> {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));

    // Register msgbus handler so generate_account_state works during reset
    let (handler, _saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);

    let config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::new(1000.0, Currency::USD())])
        .default_leverage(Decimal::ONE)
        .modules(modules)
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel).into())
        .build()
        .unwrap();
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
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();

    assert_eq!(counts.pre_process.get(), 1);
    assert_eq!(counts.process.get(), 0);
}

#[rstest]
#[case(FailingModuleHook::PreProcess, "pre_process")]
#[case(FailingModuleHook::Process, "process")]
#[case(FailingModuleHook::Acknowledge, "acknowledge")]
#[case(FailingModuleHook::LogDiagnostics, "log_diagnostics")]
#[case(FailingModuleHook::Reset, "reset")]
fn test_module_hook_failures_propagate_with_context(
    crypto_perpetual_ethusdt: CryptoPerpetual,
    #[case] hook: FailingModuleHook,
    #[case] method: &str,
) {
    let exchange = get_exchange_with_modules(
        Venue::new("BINANCE"),
        vec![SimulationModuleHandle::new(FailingSimulationModule {
            hook,
        })],
    );
    exchange
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(
            crypto_perpetual_ethusdt.clone(),
        ))
        .unwrap();
    let quote = QuoteTick::new(
        crypto_perpetual_ethusdt.id,
        Price::from("1000.00"),
        Price::from("1001.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = match hook {
        FailingModuleHook::PreProcess => exchange.borrow_mut().process_quote_tick(&quote),
        FailingModuleHook::Process | FailingModuleHook::Acknowledge => {
            exchange.borrow_mut().process_modules(UnixNanos::default())
        }
        FailingModuleHook::LogDiagnostics => exchange.borrow().log_diagnostics(),
        FailingModuleHook::Reset => exchange.borrow_mut().reset(),
    };

    let method_error = format!("Simulation module 0 {method} failed: module boom");
    let expected = if hook == FailingModuleHook::Reset {
        format!("Simulation module failure requires exchange reset: {method_error}")
    } else {
        method_error.clone()
    };
    assert_eq!(result.unwrap_err().to_string(), expected);

    if hook != FailingModuleHook::LogDiagnostics {
        assert_eq!(
            exchange
                .borrow_mut()
                .process_modules(UnixNanos::default())
                .unwrap_err()
                .to_string(),
            format!("Simulation module failure requires exchange reset: {method_error}")
        );
    }
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
    exchange
        .borrow_mut()
        .process_instrument_status(status)
        .unwrap();

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

    exchange
        .borrow_mut()
        .process_modules(UnixNanos::from(100))
        .unwrap();

    assert_eq!(counts.process.get(), 1);
}

#[rstest]
#[case(true, true)]
#[case(false, false)]
fn test_process_modules_skips_when_account_adjustments_are_unavailable(
    #[case] frozen_account: bool,
    #[case] register_client: bool,
) {
    let outcomes = Rc::new(RefCell::new(Vec::new()));
    let sequence = Rc::new(RefCell::new(Vec::new()));
    let modules = vec![SimulationModuleHandle::new(AdjustmentSimulationModule {
        label: "guarded",
        adjustments: vec![Money::from("1 USD")],
        outcomes: outcomes.clone(),
        sequence: sequence.clone(),
    })];
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let config = SimulatedVenueConfig::builder()
        .venue(Venue::new("SIM"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::from("1000 USD")])
        .default_leverage(Decimal::ONE)
        .modules(modules)
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel).into())
        .frozen_account(frozen_account)
        .build()
        .unwrap();
    let exchange = Rc::new(RefCell::new(
        SimulatedExchange::new(config, cache.clone(), clock.clone()).unwrap(),
    ));

    if register_client {
        let execution_client = BacktestExecutionClient::new(
            TraderId::test_default(),
            AccountId::test_default(),
            &exchange,
            cache,
            clock,
            None,
            Some(frozen_account),
        );
        exchange
            .borrow_mut()
            .register_client(Rc::new(execution_client));
    }

    let (handler, account_saver) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);

    exchange
        .borrow_mut()
        .process_modules(UnixNanos::from(100))
        .unwrap();

    assert!(
        sequence.borrow().is_empty(),
        "module processing must be skipped"
    );
    assert!(
        outcomes.borrow().is_empty(),
        "module acknowledgement must be skipped"
    );
    assert!(account_saver.get_messages().is_empty());
}

#[rstest]
fn test_process_modules_forwards_real_outcomes_after_shared_snapshot() {
    let first_outcomes = Rc::new(RefCell::new(Vec::new()));
    let second_outcomes = Rc::new(RefCell::new(Vec::new()));
    let sequence = Rc::new(RefCell::new(Vec::new()));
    let modules = vec![
        SimulationModuleHandle::new(AdjustmentSimulationModule {
            label: "first",
            adjustments: vec![Money::from("1 USD"), Money::from("1 AUD")],
            outcomes: first_outcomes.clone(),
            sequence: sequence.clone(),
        }),
        SimulationModuleHandle::new(AdjustmentSimulationModule {
            label: "second",
            adjustments: Vec::new(),
            outcomes: second_outcomes.clone(),
            sequence: sequence.clone(),
        }),
    ];
    let exchange = get_exchange_with_modules(Venue::new("SIM"), modules);
    let cache = exchange.borrow().cache().clone();
    pre_populate_margin_account(&mut cache.borrow_mut(), "SIM-001");

    // The saving handler registered by the fixture never applies account
    // state back to the cache, so a balance probe cannot distinguish the
    // processing orders. Record the account-state EMISSION instead: the
    // successful USD adjustment emits exactly one AccountState, and under
    // the shared-snapshot contract it must come after every module's
    // process call, not between them.
    let handler_sequence = sequence.clone();
    msgbus::register_account_state_endpoint(
        "Portfolio.update_account".into(),
        TypedHandler::from(move |_: &AccountState| {
            handler_sequence
                .borrow_mut()
                .push("account-state".to_string());
        }),
    );

    exchange
        .borrow_mut()
        .process_modules(UnixNanos::from(100))
        .unwrap();

    assert_eq!(
        *first_outcomes.borrow(),
        vec![
            AccountAdjustmentOutcome::Applied,
            AccountAdjustmentOutcome::Failed(AccountAdjustmentError::MissingBalance(
                Currency::AUD()
            )),
        ]
    );
    assert!(second_outcomes.borrow().is_empty());
    assert_eq!(
        *sequence.borrow(),
        vec!["process-first", "process-second", "account-state"],
        "all modules must process against the same pre-adjustment snapshot, \
         with adjustments applied only after the read-only phase"
    );
}

fn rollover_records() -> Vec<InterestRateRecord> {
    ["2020-01", "2021-01"]
        .into_iter()
        .flat_map(|time| {
            [
                InterestRateRecord {
                    location: "AUS".to_string(),
                    time: time.to_string(),
                    value: 0.75,
                },
                InterestRateRecord {
                    location: "GBR".to_string(),
                    time: time.to_string(),
                    value: 0.50,
                },
                InterestRateRecord {
                    location: "USA".to_string(),
                    time: time.to_string(),
                    value: 1.50,
                },
            ]
        })
        .collect()
}

fn rollover_timestamp(year: i32, month: u32, day: u32) -> UnixNanos {
    let timezone = get_timezone("America/New_York").unwrap();
    let datetime = Date::new(
        i16::try_from(year).unwrap(),
        i8::try_from(month).unwrap(),
        i8::try_from(day).unwrap(),
    )
    .unwrap()
    .at(17, 1, 0, 0);
    let timestamp = timezone
        .to_ambiguous_timestamp(datetime)
        .unambiguous()
        .unwrap()
        .as_nanosecond();
    let timestamp = u64::try_from(timestamp).unwrap();
    UnixNanos::from(timestamp)
}

fn add_fx_position(
    cache: &mut Cache,
    instrument: &InstrumentAny,
    trade_id: &str,
    quantity: &str,
    price: &str,
) {
    cache.add_instrument(instrument.clone()).unwrap();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(quantity))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::from(trade_id)),
        Some(PositionId::from(format!("P-{trade_id}").as_str())),
        Some(Price::from(price)),
        Some(Quantity::from(quantity)),
        None,
        Some(Money::new(0.0, instrument.quote_currency())),
        Some(UnixNanos::from(1)),
        Some(AccountId::from("SIM-001")),
    );
    let position = Position::new(instrument, fill.into());
    cache.add_position(&position, OmsType::Netting).unwrap();
}

fn process_rollover(
    module: &FXRolloverInterestModule,
    exchange: &Rc<RefCell<SimulatedExchange>>,
    cache: &Rc<RefCell<Cache>>,
    instruments: &AHashMap<InstrumentId, InstrumentAny>,
    ts_now: UnixNanos,
) -> Vec<Money> {
    let exchange = exchange.borrow();
    let cache = cache.borrow();
    let ctx = ExchangeContext {
        venue: Venue::new("SIM"),
        base_currency: None,
        instruments,
        matching_engines: exchange.get_matching_engines(),
        cache: &cache,
    };

    match module.process(ts_now, &ctx).unwrap() {
        SimulationModuleResult::NotReady => Vec::new(),
        SimulationModuleResult::Completed(adjustments) => {
            module
                .acknowledge(&vec![AccountAdjustmentOutcome::Applied; adjustments.len()])
                .unwrap();
            adjustments
        }
    }
}

fn add_fx_quote(
    exchange: &Rc<RefCell<SimulatedExchange>>,
    cache: &Rc<RefCell<Cache>>,
    instrument_id: InstrumentId,
    bid: &str,
    ask: &str,
) {
    let quote = QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("1000000"),
        Quantity::from("1000000"),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    cache.borrow_mut().add_quote(quote).unwrap();
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();
}

fn cfd_rollover_timestamp(year: i16, month: i8, day: i8) -> UnixNanos {
    let timestamp = Date::new(year, month, day)
        .unwrap()
        .to_datetime(Time::constant(17, 1, 0, 0))
        .to_zoned(TimeZone::UTC)
        .unwrap()
        .timestamp()
        .as_nanosecond();
    UnixNanos::from(u64::try_from(timestamp).unwrap())
}

fn add_cfd_position(cache: &mut Cache, instrument: &InstrumentAny, side: OrderSide) {
    cache.add_instrument(instrument.clone()).unwrap();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(side)
        .quantity(Quantity::from("2"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::from("T-CFD-SWAP")),
        Some(PositionId::from("P-CFD-SWAP")),
        Some(Price::from("2000.00")),
        Some(Quantity::from("2")),
        None,
        Some(Money::from("0 USD")),
        Some(UnixNanos::from(1)),
        Some(AccountId::from("SIM-001")),
    );
    let position = Position::new(instrument, fill.into());
    cache.add_position(&position, OmsType::Netting).unwrap();
}

#[rstest]
#[case(OrderSide::Buy, 16, "999.60 USD")]
#[case(OrderSide::Sell, 16, "1000.80 USD")]
#[case(OrderSide::Buy, 17, "998.80 USD")]
#[case(OrderSide::Sell, 17, "1002.40 USD")]
fn test_cfd_swap_exact_long_short_and_triple_roll_balances(
    #[case] side: OrderSide,
    #[case] day: i8,
    #[case] expected_balance: &str,
) {
    let instrument = InstrumentAny::Cfd(cfd_gold());
    let module = CfdSwapModule::new(
        vec![CfdSwapRate::new(
            instrument.id(),
            Decimal::from_str("-0.0001").unwrap(),
            Decimal::from_str("0.0002").unwrap(),
        )],
        Time::constant(17, 0, 0, 0),
        Weekday::Friday,
    );
    let modules = vec![SimulationModuleHandle::from(SimulationModuleAny::CfdSwap(
        module,
    ))];
    let exchange = get_exchange_with_modules(Venue::new("SIM"), modules);
    let cache = exchange.borrow().cache().clone();
    pre_populate_margin_account(&mut cache.borrow_mut(), "SIM-001");
    add_cfd_position(&mut cache.borrow_mut(), &instrument, side);
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let quote = QuoteTick::new(
        instrument.id(),
        Price::from("1999.00"),
        Price::from("2001.00"),
        Quantity::from("10"),
        Quantity::from("10"),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();

    let (handler, saver) = get_typed_message_saving_handler::<AccountState>(None);
    msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);
    exchange
        .borrow_mut()
        .process_modules(cfd_rollover_timestamp(2020, 1, day))
        .unwrap();

    let states = saver.get_messages();
    let [state] = states.as_slice() else {
        panic!("expected one CFD swap account state for {side}");
    };
    assert_eq!(state.balances[0].total, Money::from(expected_balance));
    assert_eq!(state.balances[0].free, Money::from(expected_balance));
    assert_eq!(state.balances[0].locked, Money::from("0 USD"));
}

#[rstest]
fn test_cfd_swap_price_and_acknowledgement_retry_and_reset_isolation() {
    let _guard = CAPTURING_LOGGER_TEST_LOCK.lock();
    let _ = log::set_logger(&CAPTURING_LOGGER);
    log::set_max_level(LevelFilter::Warn);
    CAPTURING_LOGGER.clear();

    let instrument = InstrumentAny::Cfd(cfd_gold());
    let instrument_id = instrument.id();
    let module = SimulationModuleHandle::new(CfdSwapModule::new(
        vec![CfdSwapRate::new(
            instrument_id,
            Decimal::from_str("-0.0001").unwrap(),
            Decimal::from_str("0.0002").unwrap(),
        )],
        Time::constant(17, 0, 0, 0),
        Weekday::Friday,
    ));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        None,
    );
    let cache = exchange.borrow().cache().clone();
    add_cfd_position(&mut cache.borrow_mut(), &instrument, OrderSide::Buy);
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let instruments = AHashMap::from_iter([(instrument_id, instrument)]);
    let ts_now = cfd_rollover_timestamp(2020, 1, 16);

    let process = || {
        let exchange = exchange.borrow();
        let cache = cache.borrow();
        module
            .process(
                ts_now,
                &ExchangeContext {
                    venue: Venue::new("SIM"),
                    base_currency: None,
                    instruments: &instruments,
                    matching_engines: exchange.get_matching_engines(),
                    cache: &cache,
                },
            )
            .unwrap()
    };

    assert_eq!(process(), SimulationModuleResult::NotReady);
    assert_eq!(process(), SimulationModuleResult::NotReady);
    let messages = CAPTURING_LOGGER.messages();
    assert_eq!(
        messages
            .iter()
            .filter(|(level, message)| {
                *level == Level::Warn
                    && message
                        .contains("Cannot calculate CFD swap for GOLD-CFD.SIM: no settlement price")
            })
            .count(),
        1,
        "captured messages: {messages:?}"
    );
    exchange
        .borrow_mut()
        .process_quote_tick(&QuoteTick::new(
            instrument_id,
            Price::from("1999.00"),
            Price::from("2001.00"),
            Quantity::from("10"),
            Quantity::from("10"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let expected = SimulationModuleResult::Completed(vec![Money::from("-0.40 USD")]);
    assert_eq!(process(), expected);
    module
        .acknowledge(&[AccountAdjustmentOutcome::Failed(
            AccountAdjustmentError::TotalOverflow(Currency::USD()),
        )])
        .unwrap();
    assert_eq!(process(), expected);
    module
        .acknowledge(&[AccountAdjustmentOutcome::Applied])
        .unwrap();
    assert_eq!(process(), SimulationModuleResult::NotReady);

    module.reset().unwrap();
    assert_eq!(process(), expected);
}

#[rstest]
fn test_cfd_swap_converts_to_single_currency_account_base(gbpusd_sim: CurrencyPair) {
    let cfd = InstrumentAny::Cfd(cfd_gold());
    let cfd_id = cfd.id();
    let xrate_instrument = InstrumentAny::CurrencyPair(gbpusd_sim);
    let mut raw_cache = Cache::default();
    add_cfd_position(&mut raw_cache, &cfd, OrderSide::Buy);
    raw_cache.add_instrument(xrate_instrument.clone()).unwrap();
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange.borrow_mut().add_instrument(cfd.clone()).unwrap();
    exchange
        .borrow_mut()
        .add_instrument(xrate_instrument.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .process_quote_tick(&QuoteTick::new(
            cfd_id,
            Price::from("1999.00"),
            Price::from("2001.00"),
            Quantity::from("10"),
            Quantity::from("10"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();
    add_fx_quote(
        &exchange,
        &cache,
        xrate_instrument.id(),
        "2.00000",
        "2.00000",
    );
    let instruments =
        AHashMap::from_iter([(cfd_id, cfd), (xrate_instrument.id(), xrate_instrument)]);
    let module = CfdSwapModule::new(
        vec![CfdSwapRate::new(
            cfd_id,
            Decimal::from_str("-0.0001").unwrap(),
            Decimal::from_str("0.0002").unwrap(),
        )],
        Time::constant(17, 0, 0, 0),
        Weekday::Friday,
    );
    let exchange = exchange.borrow();
    let cache = cache.borrow();
    let ctx = ExchangeContext {
        venue: Venue::new("SIM"),
        base_currency: Some(Currency::GBP()),
        instruments: &instruments,
        matching_engines: exchange.get_matching_engines(),
        cache: &cache,
    };

    assert_eq!(
        module
            .process(cfd_rollover_timestamp(2020, 1, 16), &ctx)
            .unwrap(),
        SimulationModuleResult::Completed(vec![Money::from("-0.20 GBP")])
    );
}

#[rstest]
fn test_fx_rollover_retries_after_quote_arrives(audusd_sim: CurrencyPair) {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &instrument,
        "T-ROLLOVER-RETRY",
        "100000",
        "1.00000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let instruments = AHashMap::from([(instrument.id(), instrument.clone())]);
    let module = FXRolloverInterestModule::new(rollover_records()).unwrap();
    let rollover = rollover_timestamp(2020, 1, 15);

    assert!(process_rollover(&module, &exchange, &cache, &instruments, rollover).is_empty());

    add_fx_quote(&exchange, &cache, instrument.id(), "0.99990", "1.00010");
    assert_eq!(
        process_rollover(&module, &exchange, &cache, &instruments, rollover + 1).len(),
        1
    );
    assert!(process_rollover(&module, &exchange, &cache, &instruments, rollover + 2).is_empty());
}

#[rstest]
fn test_fx_rollover_catches_up_each_economic_day_in_order(audusd_sim: CurrencyPair) {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &instrument,
        "T-ROLLOVER-CROSS-DAY",
        "100000",
        "1.00000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let instruments = AHashMap::from([(instrument.id(), instrument.clone())]);
    let records = [
        ("AUS", "2024-01", 1.0),
        ("USA", "2024-01", 2.0),
        ("AUS", "2024-02", 5.0),
        ("USA", "2024-02", 1.0),
    ]
    .into_iter()
    .map(|(location, time, value)| InterestRateRecord {
        location: location.to_string(),
        time: time.to_string(),
        value,
    })
    .collect();
    let module = FXRolloverInterestModule::new(records).unwrap();

    // Wednesday's rollover is not ready because the quote is absent.
    assert!(
        process_rollover(
            &module,
            &exchange,
            &cache,
            &instruments,
            rollover_timestamp(2024, 1, 31),
        )
        .is_empty()
    );

    // The quote arrives on Friday. The pending Wednesday remains first and
    // the full due batch uses each booking date's rates and multiplier.
    add_fx_quote(&exchange, &cache, instrument.id(), "0.99990", "1.00010");
    assert_eq!(
        process_rollover(
            &module,
            &exchange,
            &cache,
            &instruments,
            rollover_timestamp(2024, 2, 2),
        ),
        vec![
            Money::from("-8.22 USD"),
            Money::from("10.96 USD"),
            Money::from("32.88 USD"),
        ]
    );

    // Acknowledgement advances the cursor to Friday, the stored batch end date.
    assert!(
        process_rollover(
            &module,
            &exchange,
            &cache,
            &instruments,
            rollover_timestamp(2024, 2, 2) + 1,
        )
        .is_empty()
    );
}

#[rstest]
fn test_fx_rollover_friday_to_monday_gap_books_monday_once(audusd_sim: CurrencyPair) {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &instrument,
        "T-ROLLOVER-WEEKEND",
        "100000",
        "1.00000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    add_fx_quote(&exchange, &cache, instrument.id(), "0.99990", "1.00010");
    let instruments = AHashMap::from([(instrument.id(), instrument)]);
    let module = FXRolloverInterestModule::new(rollover_records()).unwrap();

    assert_eq!(
        process_rollover(
            &module,
            &exchange,
            &cache,
            &instruments,
            rollover_timestamp(2020, 1, 17),
        ),
        vec![Money::from("-6.16 USD")]
    );
    let monday = rollover_timestamp(2020, 1, 20);
    assert_eq!(
        process_rollover(&module, &exchange, &cache, &instruments, monday),
        vec![Money::from("-2.05 USD")]
    );
    assert!(process_rollover(&module, &exchange, &cache, &instruments, monday + 1).is_empty());
}

#[rstest]
fn test_missing_xrate_retries_emit_one_module_warning_and_no_cache_errors(
    audusd_sim: CurrencyPair,
) {
    let _guard = CAPTURING_LOGGER_TEST_LOCK.lock();
    let _ = log::set_logger(&CAPTURING_LOGGER);
    log::set_max_level(LevelFilter::Warn);
    CAPTURING_LOGGER.clear();

    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &instrument,
        "T-ROLLOVER-XRATE",
        "100000",
        "1.00000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let quote = QuoteTick::new(
        instrument.id(),
        Price::from("0.99990"),
        Price::from("1.00010"),
        Quantity::from("1000000"),
        Quantity::from("1000000"),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();
    let instruments = AHashMap::from([(instrument.id(), instrument)]);
    let module = FXRolloverInterestModule::new(rollover_records()).unwrap();
    let ts_now = rollover_timestamp(2020, 1, 15);

    for _ in 0..2 {
        let exchange = exchange.borrow();
        let cache = cache.borrow();
        let ctx = ExchangeContext {
            venue: Venue::new("SIM"),
            base_currency: Some(Currency::GBP()),
            instruments: &instruments,
            matching_engines: exchange.get_matching_engines(),
            cache: &cache,
        };
        assert_eq!(
            module.process(ts_now, &ctx).unwrap(),
            SimulationModuleResult::NotReady
        );
    }

    let messages = CAPTURING_LOGGER.messages();
    assert_eq!(
        messages
            .iter()
            .filter(|(level, message)| {
                *level == Level::Warn
                    && message.contains(
                        "Cannot calculate rollover for AUD/USD.SIM: exchange rate from USD to GBP",
                    )
            })
            .count(),
        1,
        "captured messages: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|(_, message)| !message.contains("Failed to calculate xrate"))
    );
}

#[rstest]
fn test_missing_rates_skip_instrument_and_do_not_stall_later_days(
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let _guard = CAPTURING_LOGGER_TEST_LOCK.lock();
    let _ = log::set_logger(&CAPTURING_LOGGER);
    log::set_max_level(LevelFilter::Warn);
    CAPTURING_LOGGER.clear();

    let supported = InstrumentAny::CurrencyPair(audusd_sim);
    let missing_rates = InstrumentAny::CurrencyPair(gbpusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &supported,
        "T-ROLLOVER-SUPPORTED",
        "100000",
        "1.00000",
    );
    add_fx_position(
        &mut raw_cache,
        &missing_rates,
        "T-ROLLOVER-MISSING-RATES",
        "100000",
        "1.20000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(supported.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .add_instrument(missing_rates.clone())
        .unwrap();
    // Deliberately no quote for `missing_rates`: the permanently missing rate
    // must skip the instrument before the transient no-price check can retry
    // the whole day.
    add_fx_quote(&exchange, &cache, supported.id(), "0.99990", "1.00010");
    let instruments = AHashMap::from([
        (supported.id(), supported.clone()),
        (missing_rates.id(), missing_rates.clone()),
    ]);
    let records = rollover_records()
        .into_iter()
        .filter(|record| record.location != "GBR")
        .collect();
    let module = FXRolloverInterestModule::new(records).unwrap();
    let first_day = rollover_timestamp(2020, 1, 15);

    assert_eq!(
        process_rollover(&module, &exchange, &cache, &instruments, first_day),
        vec![Money::from("-6.16 USD")]
    );

    let only_missing = AHashMap::from([(missing_rates.id(), missing_rates)]);
    let exchange_ref = exchange.borrow();
    let cache_ref = cache.borrow();
    let ctx = ExchangeContext {
        venue: Venue::new("SIM"),
        base_currency: None,
        instruments: &only_missing,
        matching_engines: exchange_ref.get_matching_engines(),
        cache: &cache_ref,
    };
    assert_eq!(
        module
            .process(rollover_timestamp(2020, 1, 16), &ctx)
            .unwrap(),
        SimulationModuleResult::Completed(Vec::new())
    );
    module.acknowledge(&[]).unwrap();
    assert_eq!(
        module
            .process(rollover_timestamp(2020, 1, 17), &ctx)
            .unwrap(),
        SimulationModuleResult::Completed(Vec::new())
    );

    let messages = CAPTURING_LOGGER.messages();
    assert_eq!(
        messages
            .iter()
            .filter(|(level, message)| {
                *level == Level::Warn
                    && message.contains("Skipping rollover for GBP/USD.SIM on 2020-01-15")
            })
            .count(),
        1,
        "captured messages: {messages:?}"
    );
}

#[rstest]
fn test_unrepresentable_money_warns_once_without_error_across_recalculation(
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let _guard = CAPTURING_LOGGER_TEST_LOCK.lock();
    let _ = log::set_logger(&CAPTURING_LOGGER);
    log::set_max_level(LevelFilter::Warn);
    CAPTURING_LOGGER.clear();

    let unrepresentable = InstrumentAny::CurrencyPair(audusd_sim);
    let transient = InstrumentAny::CurrencyPair(gbpusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &unrepresentable,
        "T-ROLLOVER-MONEY",
        "100000",
        "1.00000",
    );
    add_fx_position(
        &mut raw_cache,
        &transient,
        "T-ROLLOVER-MONEY-RETRY",
        "100000",
        "1.20000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(unrepresentable.clone())
        .unwrap();
    exchange
        .borrow_mut()
        .add_instrument(transient.clone())
        .unwrap();
    add_fx_quote(
        &exchange,
        &cache,
        unrepresentable.id(),
        "0.99990",
        "1.00010",
    );
    let instruments = AHashMap::from([
        (unrepresentable.id(), unrepresentable.clone()),
        (transient.id(), transient.clone()),
    ]);
    let records = vec![
        InterestRateRecord {
            location: "AUS".to_string(),
            time: "2020-01".to_string(),
            value: f64::MAX,
        },
        InterestRateRecord {
            location: "GBR".to_string(),
            time: "2020-01".to_string(),
            value: 0.5,
        },
        InterestRateRecord {
            location: "USA".to_string(),
            time: "2020-01".to_string(),
            value: 1.5,
        },
    ];
    let module = FXRolloverInterestModule::new(records).unwrap();
    let rollover = rollover_timestamp(2020, 1, 15);

    assert!(process_rollover(&module, &exchange, &cache, &instruments, rollover).is_empty());
    add_fx_quote(&exchange, &cache, transient.id(), "1.19990", "1.20010");
    assert_eq!(
        process_rollover(&module, &exchange, &cache, &instruments, rollover + 1),
        vec![Money::from("-9.86 USD")]
    );

    let messages = CAPTURING_LOGGER.messages();
    assert_eq!(
        messages
            .iter()
            .filter(|(level, message)| {
                *level == Level::Warn
                    && message.contains("Skipping rollover for AUD/USD.SIM on 2020-01-15")
            })
            .count(),
        1,
        "captured messages: {messages:?}"
    );
    assert!(messages.iter().all(|(level, message)| {
        *level != Level::Error
            || !(message.contains("Skipping rollover for AUD/USD.SIM")
                && message.contains("invalid adjustment"))
    }));
}

#[rstest]
fn test_fx_rollover_is_atomic_across_instruments(
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let first = InstrumentAny::CurrencyPair(audusd_sim);
    let second = InstrumentAny::CurrencyPair(gbpusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &first,
        "T-ROLLOVER-ATOMIC-1",
        "100000",
        "1.00000",
    );
    add_fx_position(
        &mut raw_cache,
        &second,
        "T-ROLLOVER-ATOMIC-2",
        "100000",
        "1.20000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange.borrow_mut().add_instrument(first.clone()).unwrap();
    exchange
        .borrow_mut()
        .add_instrument(second.clone())
        .unwrap();
    let instruments = AHashMap::from([(first.id(), first.clone()), (second.id(), second.clone())]);
    let module = FXRolloverInterestModule::new(rollover_records()).unwrap();
    let rollover = rollover_timestamp(2020, 1, 15);

    add_fx_quote(&exchange, &cache, first.id(), "0.99990", "1.00010");
    assert!(process_rollover(&module, &exchange, &cache, &instruments, rollover).is_empty());

    add_fx_quote(&exchange, &cache, second.id(), "1.19990", "1.20010");
    assert_eq!(
        process_rollover(&module, &exchange, &cache, &instruments, rollover + 1).len(),
        2
    );
}

#[rstest]
fn test_fx_rollover_distinguishes_same_ordinal_across_years(audusd_sim: CurrencyPair) {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let mut raw_cache = Cache::default();
    add_fx_position(
        &mut raw_cache,
        &instrument,
        "T-ROLLOVER-CROSS-YEAR",
        "100000",
        "1.00000",
    );
    let cache = Rc::new(RefCell::new(raw_cache));
    let exchange = get_exchange(
        Venue::new("SIM"),
        AccountType::Margin,
        BookType::L1_MBP,
        Some(cache.clone()),
    );
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    add_fx_quote(&exchange, &cache, instrument.id(), "0.99990", "1.00010");
    let instruments = AHashMap::from([(instrument.id(), instrument)]);
    let module = FXRolloverInterestModule::new(rollover_records()).unwrap();

    assert_eq!(
        process_rollover(
            &module,
            &exchange,
            &cache,
            &instruments,
            rollover_timestamp(2020, 1, 15),
        )
        .len(),
        1
    );
    // The catch-up includes the remaining 12 January 2020 weekdays and the
    // first 11 January 2021 weekdays. Dates without configured rates skip.
    assert_eq!(
        process_rollover(
            &module,
            &exchange,
            &cache,
            &instruments,
            rollover_timestamp(2021, 1, 15),
        )
        .len(),
        23
    );
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

    exchange.borrow_mut().reset().unwrap();

    assert_eq!(counts.reset.get(), 1);
}

#[rstest]
fn test_module_log_diagnostics(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let counts = MockModuleCounts::new();
    let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    exchange.borrow_mut().add_instrument(instrument).unwrap();

    exchange.borrow().log_diagnostics().unwrap();

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
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();
    exchange.borrow_mut().process(UnixNanos::from(100));
    exchange
        .borrow_mut()
        .process_modules(UnixNanos::from(100))
        .unwrap();

    assert_eq!(counts.pre_process.get(), 2);
    assert_eq!(counts.process.get(), 1);
}

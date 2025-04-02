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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clock::TestClock};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, QuoteTick},
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{
        AccountState, OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted, PositionChanged,
        PositionClosed, PositionEvent, PositionOpened,
        account::stubs::cash_account_state,
        order::stubs::{order_accepted, order_filled, order_submitted},
    },
    identifiers::{
        AccountId, ClientOrderId, PositionId, StrategyId, Symbol, TradeId, VenueOrderId,
        stubs::{account_id, uuid4},
    },
    instruments::{
        CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny,
        stubs::{audusd_sim, currency_pair_btcusdt, default_fx_ccy, ethusdt_bitmex},
    },
    orders::{Order, OrderAny, OrderTestBuilder},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::{fixture, rstest};
use rust_decimal::{Decimal, prelude::FromPrimitive};

use crate::portfolio::Portfolio;

#[fixture]
fn simple_cache() -> Cache {
    Cache::new(None, None)
}

#[fixture]
fn clock() -> TestClock {
    TestClock::new()
}

#[fixture]
fn venue() -> Venue {
    Venue::new("SIM")
}

#[fixture]
fn instrument_audusd(audusd_sim: CurrencyPair) -> InstrumentAny {
    InstrumentAny::CurrencyPair(audusd_sim)
}

#[fixture]
fn instrument_gbpusd() -> InstrumentAny {
    InstrumentAny::CurrencyPair(default_fx_ccy(
        Symbol::from("GBP/USD"),
        Some(Venue::from("SIM")),
    ))
}

#[fixture]
fn instrument_btcusdt(currency_pair_btcusdt: CurrencyPair) -> InstrumentAny {
    InstrumentAny::CurrencyPair(currency_pair_btcusdt)
}

#[fixture]
fn instrument_ethusdt(ethusdt_bitmex: CryptoPerpetual) -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(ethusdt_bitmex)
}

#[fixture]
fn portfolio(
    mut simple_cache: Cache,
    clock: TestClock,
    instrument_audusd: InstrumentAny,
    instrument_gbpusd: InstrumentAny,
    instrument_btcusdt: InstrumentAny,
    instrument_ethusdt: InstrumentAny,
) -> Portfolio {
    simple_cache.add_instrument(instrument_audusd).unwrap();
    simple_cache.add_instrument(instrument_gbpusd).unwrap();
    simple_cache.add_instrument(instrument_btcusdt).unwrap();
    simple_cache.add_instrument(instrument_ethusdt).unwrap();

    Portfolio::new(
        Rc::new(RefCell::new(simple_cache)),
        Rc::new(RefCell::new(clock)),
        None,
    )
}

use std::collections::HashMap;

use nautilus_model::identifiers::Venue;

// Helpers
fn get_cash_account(accountid: Option<&str>) -> AccountState {
    AccountState::new(
        match accountid {
            Some(account_id_str) => AccountId::new(account_id_str),
            None => account_id(),
        },
        AccountType::Cash,
        vec![
            AccountBalance::new(
                Money::new(10.00000000, Currency::BTC()),
                Money::new(0.00000000, Currency::BTC()),
                Money::new(10.00000000, Currency::BTC()),
            ),
            AccountBalance::new(
                Money::new(10.000, Currency::USD()),
                Money::new(0.000, Currency::USD()),
                Money::new(10.000, Currency::USD()),
            ),
            AccountBalance::new(
                Money::new(100000.000, Currency::USDT()),
                Money::new(0.000, Currency::USDT()),
                Money::new(100000.000, Currency::USDT()),
            ),
            AccountBalance::new(
                Money::new(20.000, Currency::ETH()),
                Money::new(0.000, Currency::ETH()),
                Money::new(20.000, Currency::ETH()),
            ),
        ],
        vec![],
        true,
        uuid4(),
        0.into(),
        0.into(),
        None,
    )
}

fn get_margin_account(accountid: Option<&str>) -> AccountState {
    AccountState::new(
        match accountid {
            Some(account_id_str) => AccountId::new(account_id_str),
            None => account_id(),
        },
        AccountType::Margin,
        vec![
            AccountBalance::new(
                Money::new(10.000, Currency::BTC()),
                Money::new(0.000, Currency::BTC()),
                Money::new(10.000, Currency::BTC()),
            ),
            AccountBalance::new(
                Money::new(20.000, Currency::ETH()),
                Money::new(0.000, Currency::ETH()),
                Money::new(20.000, Currency::ETH()),
            ),
            AccountBalance::new(
                Money::new(100000.000, Currency::USDT()),
                Money::new(0.000, Currency::USDT()),
                Money::new(100000.000, Currency::USDT()),
            ),
            AccountBalance::new(
                Money::new(10.000, Currency::USD()),
                Money::new(0.000, Currency::USD()),
                Money::new(10.000, Currency::USD()),
            ),
            AccountBalance::new(
                Money::new(10.000, Currency::GBP()),
                Money::new(0.000, Currency::GBP()),
                Money::new(10.000, Currency::GBP()),
            ),
        ],
        Vec::new(),
        true,
        uuid4(),
        0.into(),
        0.into(),
        None,
    )
}

fn get_quote_tick(
    instrument: &InstrumentAny,
    bid: f64,
    ask: f64,
    bid_size: f64,
    ask_size: f64,
) -> QuoteTick {
    QuoteTick::new(
        instrument.id(),
        Price::new(bid, 0),
        Price::new(ask, 0),
        Quantity::new(bid_size, 0),
        Quantity::new(ask_size, 0),
        0.into(),
        0.into(),
    )
}

fn get_bar(
    instrument: &InstrumentAny,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
) -> Bar {
    let bar_type_str = format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id());
    Bar::new(
        BarType::from(bar_type_str.as_ref()),
        Price::new(open, 0),
        Price::new(high, 0),
        Price::new(low, 0),
        Price::new(close, 0),
        Quantity::new(volume, 0),
        0.into(),
        0.into(),
    )
}

fn submit_order(order: &OrderAny) -> OrderSubmitted {
    order_submitted(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id(),
        uuid4(),
    )
}

fn accept_order(order: &OrderAny) -> OrderAccepted {
    order_accepted(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id(),
        order.venue_order_id().unwrap_or(VenueOrderId::new("1")),
        uuid4(),
    )
}

fn fill_order(order: &OrderAny) -> OrderFilled {
    order_filled(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        uuid4(),
    )
}

fn get_open_position(position: &Position) -> PositionOpened {
    PositionOpened {
        trader_id: position.trader_id,
        strategy_id: position.strategy_id,
        instrument_id: position.instrument_id,
        position_id: position.id,
        account_id: position.account_id,
        opening_order_id: position.opening_order_id,
        entry: position.entry,
        side: position.side,
        signed_qty: position.signed_qty,
        quantity: position.quantity,
        last_qty: position.quantity,
        last_px: Price::new(position.avg_px_open, 0),
        currency: position.settlement_currency,
        avg_px_open: position.avg_px_open,
        event_id: UUID4::new(),
        ts_event: 0.into(),
        ts_init: 0.into(),
    }
}

fn get_changed_position(position: &Position) -> PositionChanged {
    PositionChanged {
        trader_id: position.trader_id,
        strategy_id: position.strategy_id,
        instrument_id: position.instrument_id,
        position_id: position.id,
        account_id: position.account_id,
        opening_order_id: position.opening_order_id,
        entry: position.entry,
        side: position.side,
        signed_qty: position.signed_qty,
        quantity: position.quantity,
        last_qty: position.quantity,
        last_px: Price::new(position.avg_px_open, 0),
        currency: position.settlement_currency,
        avg_px_open: position.avg_px_open,
        ts_event: 0.into(),
        ts_init: 0.into(),
        peak_quantity: position.quantity,
        avg_px_close: Some(position.avg_px_open),
        realized_return: position.avg_px_open,
        realized_pnl: Some(Money::new(10.0, Currency::USD())),
        unrealized_pnl: Money::new(10.0, Currency::USD()),
        event_id: UUID4::new(),
        ts_opened: 0.into(),
    }
}

fn get_close_position(position: &Position) -> PositionClosed {
    PositionClosed {
        trader_id: position.trader_id,
        strategy_id: position.strategy_id,
        instrument_id: position.instrument_id,
        position_id: position.id,
        account_id: position.account_id,
        opening_order_id: position.opening_order_id,
        entry: position.entry,
        side: position.side,
        signed_qty: position.signed_qty,
        quantity: position.quantity,
        last_qty: position.quantity,
        last_px: Price::new(position.avg_px_open, 0),
        currency: position.settlement_currency,
        avg_px_open: position.avg_px_open,
        ts_event: 0.into(),
        ts_init: 0.into(),
        peak_quantity: position.quantity,
        avg_px_close: Some(position.avg_px_open),
        realized_return: position.avg_px_open,
        realized_pnl: Some(Money::new(10.0, Currency::USD())),
        unrealized_pnl: Money::new(10.0, Currency::USD()),
        closing_order_id: Some(ClientOrderId::new("SSD")),
        duration: 0,
        event_id: UUID4::new(),
        ts_opened: 0.into(),
        ts_closed: None,
    }
}

// Tests
#[rstest]
fn test_account_when_account_returns_the_account_facade(mut portfolio: Portfolio) {
    let account_id = "BINANCE-1513111";
    let state = get_cash_account(Some(account_id));

    portfolio.update_account(&state);

    let cache = portfolio.cache.borrow_mut();
    let account = cache.account(&AccountId::new(account_id)).unwrap();
    assert_eq!(account.id().get_issuer(), "BINANCE".into());
    assert_eq!(account.id().get_issuers_id(), "1513111");
}

#[rstest]
fn test_balances_locked_when_no_account_for_venue_returns_none(portfolio: Portfolio, venue: Venue) {
    let result = portfolio.balances_locked(&venue);
    assert_eq!(result, HashMap::new());
}

#[rstest]
fn test_margins_init_when_no_account_for_venue_returns_none(portfolio: Portfolio, venue: Venue) {
    let result = portfolio.margins_init(&venue);
    assert_eq!(result, HashMap::new());
}

#[rstest]
fn test_margins_maint_when_no_account_for_venue_returns_none(portfolio: Portfolio, venue: Venue) {
    let result = portfolio.margins_maint(&venue);
    assert_eq!(result, HashMap::new());
}

#[rstest]
fn test_unrealized_pnl_for_instrument_when_no_instrument_returns_none(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let result = portfolio.unrealized_pnl(&instrument_audusd.id());
    assert!(result.is_none());
}

#[rstest]
fn test_unrealized_pnl_for_venue_when_no_account_returns_empty_dict(
    mut portfolio: Portfolio,
    venue: Venue,
) {
    let result = portfolio.unrealized_pnls(&venue);
    assert_eq!(result, HashMap::new());
}

#[rstest]
fn test_realized_pnl_for_instrument_when_no_instrument_returns_none(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let result = portfolio.realized_pnl(&instrument_audusd.id());
    assert!(result.is_none());
}

#[rstest]
fn test_realized_pnl_for_venue_when_no_account_returns_empty_dict(
    mut portfolio: Portfolio,
    venue: Venue,
) {
    let result = portfolio.realized_pnls(&venue);
    assert_eq!(result, HashMap::new());
}

#[rstest]
fn test_net_position_when_no_positions_returns_zero(
    portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let result = portfolio.net_position(&instrument_audusd.id());
    assert_eq!(result, Decimal::ZERO);
}

#[rstest]
fn test_net_exposures_when_no_positions_returns_none(portfolio: Portfolio, venue: Venue) {
    let result = portfolio.net_exposures(&venue);
    assert!(result.is_none());
}

#[rstest]
fn test_is_net_long_when_no_positions_returns_false(
    portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let result = portfolio.is_net_long(&instrument_audusd.id());
    assert!(!result);
}

#[rstest]
fn test_is_net_short_when_no_positions_returns_false(
    portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let result = portfolio.is_net_short(&instrument_audusd.id());
    assert!(!result);
}

#[rstest]
fn test_is_flat_when_no_positions_returns_true(
    portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let result = portfolio.is_flat(&instrument_audusd.id());
    assert!(result);
}

#[rstest]
fn test_is_completely_flat_when_no_positions_returns_true(portfolio: Portfolio) {
    let result = portfolio.is_completely_flat();
    assert!(result);
}

#[rstest]
fn test_open_value_when_no_account_returns_none(portfolio: Portfolio, venue: Venue) {
    let result = portfolio.net_exposures(&venue);
    assert!(result.is_none());
}

#[rstest]
fn test_update_tick(mut portfolio: Portfolio, instrument_audusd: InstrumentAny) {
    let tick = get_quote_tick(&instrument_audusd, 1.25, 1.251, 1.0, 1.0);
    portfolio.update_quote_tick(&tick);
    assert!(portfolio.unrealized_pnl(&instrument_audusd.id()).is_none());
}

//TODO: FIX: It should return an error
#[rstest]
fn test_exceed_free_balance_single_currency_raises_account_balance_negative_exception(
    mut portfolio: Portfolio,
    cash_account_state: AccountState,
    instrument_audusd: InstrumentAny,
) {
    portfolio.update_account(&cash_account_state);

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1000000.000"))
        .build();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let submitted = submit_order(&order);
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    portfolio.update_order(&OrderEventAny::Submitted(submitted));

    let order_filled = fill_order(&order);
    order.apply(OrderEventAny::Filled(order_filled)).unwrap();
    portfolio.update_order(&OrderEventAny::Filled(order_filled));
}

// TODO: It should return an error
#[rstest]
fn test_exceed_free_balance_multi_currency_raises_account_balance_negative_exception(
    mut portfolio: Portfolio,
    cash_account_state: AccountState,
    instrument_audusd: InstrumentAny,
) {
    portfolio.update_account(&cash_account_state);

    let account = portfolio
        .cache
        .borrow_mut()
        .account_for_venue(&Venue::from("SIM"))
        .unwrap()
        .clone();

    // Create Order
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("3.0"))
        .build();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let order_submitted = submit_order(&order);
    order
        .apply(OrderEventAny::Submitted(order_submitted))
        .unwrap();
    portfolio.update_order(&OrderEventAny::Submitted(order_submitted));

    // Assert
    assert_eq!(
        account.balances().iter().next().unwrap().1.total.as_f64(),
        1525000.00
    );
}

#[rstest]
fn test_update_orders_open_cash_account(
    mut portfolio: Portfolio,
    cash_account_state: AccountState,
    instrument_audusd: InstrumentAny,
) {
    portfolio.update_account(&cash_account_state);

    // Create Order
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::new(50000.0, 0))
        .build();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let order_submitted = submit_order(&order);
    order
        .apply(OrderEventAny::Submitted(order_submitted))
        .unwrap();
    portfolio.update_order(&OrderEventAny::Submitted(order_submitted));

    // ACCEPTED
    let order_accepted = accept_order(&order);
    order
        .apply(OrderEventAny::Accepted(order_accepted))
        .unwrap();
    portfolio.update_order(&OrderEventAny::Accepted(order_accepted));

    assert_eq!(
        portfolio
            .balances_locked(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        25000.0
    );
}

#[rstest]
fn test_update_orders_open_margin_account(
    mut portfolio: Portfolio,
    instrument_btcusdt: InstrumentAny,
) {
    let account_state = get_margin_account(Some("BINANCE-01234"));
    portfolio.update_account(&account_state);

    // Create Order
    let mut order1 = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_btcusdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100.000"))
        .price(Price::new(55.0, 1))
        .trigger_price(Price::new(35.0, 1))
        .build();

    let order2 = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_btcusdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1000.000"))
        .price(Price::new(45.0, 1))
        .trigger_price(Price::new(30.0, 1))
        .build();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, true)
        .unwrap();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order2, None, None, true)
        .unwrap();

    let order_submitted = submit_order(&order1);
    order1
        .apply(OrderEventAny::Submitted(order_submitted))
        .unwrap();
    portfolio.cache.borrow_mut().update_order(&order1).unwrap();

    // Push status to Accepted
    let order_accepted = accept_order(&order1);
    order1
        .apply(OrderEventAny::Accepted(order_accepted))
        .unwrap();
    portfolio.cache.borrow_mut().update_order(&order1).unwrap();

    // TODO: Replace with Execution Engine once implemented.
    portfolio
        .cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, true)
        .unwrap();

    let order_filled1 = fill_order(&order1);
    order1.apply(OrderEventAny::Filled(order_filled1)).unwrap();

    // Act
    let last = get_quote_tick(&instrument_btcusdt, 25001.0, 25002.0, 15.0, 12.0);
    portfolio.update_quote_tick(&last);
    portfolio.initialize_orders();

    // Assert
    assert_eq!(
        portfolio
            .margins_init(&Venue::from("BINANCE"))
            .get(&instrument_btcusdt.id())
            .unwrap()
            .as_f64(),
        10.5
    );
}

#[rstest]
fn test_order_accept_updates_margin_init(
    mut portfolio: Portfolio,
    instrument_btcusdt: InstrumentAny,
) {
    let account_state = get_margin_account(Some("BINANCE-01234"));
    portfolio.update_account(&account_state);

    // Create Order
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::new("55"))
        .instrument_id(instrument_btcusdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100.0"))
        .price(Price::new(5.0, 0))
        .build();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, true)
        .unwrap();

    let order_submitted = submit_order(&order);
    order
        .apply(OrderEventAny::Submitted(order_submitted))
        .unwrap();
    portfolio.cache.borrow_mut().update_order(&order).unwrap();

    let order_accepted = accept_order(&order);
    order
        .apply(OrderEventAny::Accepted(order_accepted))
        .unwrap();
    portfolio.cache.borrow_mut().update_order(&order).unwrap();

    // TODO: Replace with Execution Engine once implemented.
    portfolio
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, true)
        .unwrap();

    // Act
    portfolio.initialize_orders();

    // Assert
    assert_eq!(
        portfolio
            .margins_init(&Venue::from("BINANCE"))
            .get(&instrument_btcusdt.id())
            .unwrap()
            .as_f64(),
        1.5
    );
}

#[rstest]
fn test_update_positions(mut portfolio: Portfolio, instrument_audusd: InstrumentAny) {
    let account_state = get_cash_account(None);
    portfolio.update_account(&account_state);

    // Create Order
    let mut order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.50"))
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("10.50"))
        .build();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, true)
        .unwrap();
    portfolio
        .cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, true)
        .unwrap();

    let order1_submitted = submit_order(&order1);
    order1
        .apply(OrderEventAny::Submitted(order1_submitted))
        .unwrap();
    portfolio.update_order(&OrderEventAny::Submitted(order1_submitted));

    // ACCEPTED
    let order1_accepted = accept_order(&order1);
    order1
        .apply(OrderEventAny::Accepted(order1_accepted))
        .unwrap();
    portfolio.update_order(&OrderEventAny::Accepted(order1_accepted));

    let mut fill1 = fill_order(&order1);
    fill1.position_id = Some(PositionId::new("SSD"));

    let mut fill2 = fill_order(&order2);
    fill2.trade_id = TradeId::new("2");

    let mut position1 = Position::new(&instrument_audusd, fill1);
    position1.apply(&fill2);

    let order3 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("10.00"))
        .build();

    let mut fill3 = fill_order(&order3);
    fill3.position_id = Some(PositionId::new("SSsD"));

    let position2 = Position::new(&instrument_audusd, fill3);

    // Update the last quote
    let last = get_quote_tick(&instrument_audusd, 250001.0, 250002.0, 1.0, 1.0);

    // Act
    portfolio
        .cache
        .borrow_mut()
        .add_position(position1, OmsType::Hedging)
        .unwrap();
    portfolio
        .cache
        .borrow_mut()
        .add_position(position2, OmsType::Hedging)
        .unwrap();
    portfolio.cache.borrow_mut().add_quote(last).unwrap();
    portfolio.update_quote_tick(&last);
    portfolio.initialize_positions();

    // Assert
    assert!(portfolio.is_net_long(&instrument_audusd.id()));
}

#[rstest]
fn test_opening_one_long_position_updates_portfolio(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let account_state = get_margin_account(None);
    portfolio.update_account(&account_state);

    // Create Order
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.00"))
        .build();

    let mut fill = fill_order(&order);
    fill.position_id = Some(PositionId::new("SSD"));

    // Update the last quote
    let last = get_quote_tick(&instrument_audusd, 10510.0, 10511.0, 1.0, 1.0);
    portfolio.cache.borrow_mut().add_quote(last).unwrap();
    portfolio.update_quote_tick(&last);

    let position = Position::new(&instrument_audusd, fill);

    // Act
    portfolio
        .cache
        .borrow_mut()
        .add_position(position.clone(), OmsType::Hedging)
        .unwrap();

    let position_opened = get_open_position(&position);
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("SIM"))
            .unwrap()
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        10510.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -6445.89
    );
    assert_eq!(
        portfolio
            .realized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        0.0
    );
    assert_eq!(
        portfolio
            .net_exposure(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        10510.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        -6445.89
    );
    assert_eq!(
        portfolio
            .realized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        0.0
    );
    assert_eq!(
        portfolio.net_position(&instrument_audusd.id()),
        Decimal::new(561, 3)
    );
    assert!(portfolio.is_net_long(&instrument_audusd.id()));
    assert!(!portfolio.is_net_short(&instrument_audusd.id()));
    assert!(!portfolio.is_flat(&instrument_audusd.id()));
    assert!(!portfolio.is_completely_flat());
}

#[rstest]
fn test_opening_one_long_position_updates_portfolio_with_bar(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let account_state = get_margin_account(None);
    portfolio.update_account(&account_state);

    // Create Order
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.00"))
        .build();

    let mut fill = fill_order(&order);
    fill.position_id = Some(PositionId::new("SSD"));

    // Update the last quote
    let last = get_bar(&instrument_audusd, 10510.0, 10510.0, 10510.0, 10510.0, 0.0);
    portfolio.update_bar(&last);

    let position = Position::new(&instrument_audusd, fill);

    // Act
    portfolio
        .cache
        .borrow_mut()
        .add_position(position.clone(), OmsType::Hedging)
        .unwrap();

    let position_opened = get_open_position(&position);
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("SIM"))
            .unwrap()
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        10510.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -6445.89
    );
    assert_eq!(
        portfolio
            .realized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        0.0
    );
    assert_eq!(
        portfolio
            .net_exposure(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        10510.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        -6445.89
    );
    assert_eq!(
        portfolio
            .realized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        0.0
    );
    assert_eq!(
        portfolio.net_position(&instrument_audusd.id()),
        Decimal::new(561, 3)
    );
    assert!(portfolio.is_net_long(&instrument_audusd.id()));
    assert!(!portfolio.is_net_short(&instrument_audusd.id()));
    assert!(!portfolio.is_flat(&instrument_audusd.id()));
    assert!(!portfolio.is_completely_flat());
}

#[rstest]
fn test_opening_one_short_position_updates_portfolio(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let account_state = get_margin_account(None);
    portfolio.update_account(&account_state);

    // Create Order
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("2"))
        .build();

    let filled = OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order.order_side(),
        order.order_type(),
        order.quantity(),
        Price::new(10.0, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("12.2 USD")),
    );

    // Update the last quote
    let last = get_quote_tick(&instrument_audusd, 15510.15, 15510.25, 13.0, 4.0);

    portfolio.cache.borrow_mut().add_quote(last).unwrap();
    portfolio.update_quote_tick(&last);

    let position = Position::new(&instrument_audusd, filled);

    // Act
    portfolio
        .cache
        .borrow_mut()
        .add_position(position.clone(), OmsType::Hedging)
        .unwrap();

    let position_opened = get_open_position(&position);
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("SIM"))
            .unwrap()
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        31020.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -31000.0
    );
    assert_eq!(
        portfolio
            .realized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -12.2
    );
    assert_eq!(
        portfolio
            .net_exposure(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        31020.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        -31000.0
    );
    assert_eq!(
        portfolio
            .realized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        -12.2
    );
    assert_eq!(
        portfolio.net_position(&instrument_audusd.id()),
        Decimal::new(-2, 0)
    );

    assert!(!portfolio.is_net_long(&instrument_audusd.id()));
    assert!(portfolio.is_net_short(&instrument_audusd.id()));
    assert!(!portfolio.is_flat(&instrument_audusd.id()));
    assert!(!portfolio.is_completely_flat());
}

#[rstest]
fn test_opening_positions_with_multi_asset_account(
    mut portfolio: Portfolio,
    instrument_btcusdt: InstrumentAny,
    instrument_ethusdt: InstrumentAny,
) {
    let account_state = get_margin_account(Some("BITMEX-01234"));
    portfolio.update_account(&account_state);

    let last_ethusd = get_quote_tick(&instrument_ethusdt, 376.05, 377.10, 16.0, 25.0);
    let last_btcusd = get_quote_tick(&instrument_btcusdt, 10500.05, 10501.51, 2.54, 0.91);

    portfolio.cache.borrow_mut().add_quote(last_ethusd).unwrap();
    portfolio.cache.borrow_mut().add_quote(last_btcusd).unwrap();
    portfolio.update_quote_tick(&last_ethusd);
    portfolio.update_quote_tick(&last_btcusd);

    // Create Order
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_ethusdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10000"))
        .build();

    let filled = OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order.order_side(),
        order.order_type(),
        order.quantity(),
        Price::new(376.0, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("12.2 USD")),
    );

    let position = Position::new(&instrument_ethusdt, filled);

    // Act
    portfolio
        .cache
        .borrow_mut()
        .add_position(position.clone(), OmsType::Hedging)
        .unwrap();

    let position_opened = get_open_position(&position);
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("BITMEX"))
            .unwrap()
            .get(&Currency::ETH())
            .unwrap()
            .as_f64(),
        26.59574468
    );
    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("BITMEX"))
            .get(&Currency::ETH())
            .unwrap()
            .as_f64(),
        0.0
    );
    // TODO: fix
    // assert_eq!(
    //     portfolio
    //         .margins_maint(&Venue::from("SIM"))
    //         .get(&instrument_audusd.id())
    //         .unwrap()
    //         .as_f64(),
    //     0.0
    // );
    assert_eq!(
        portfolio
            .net_exposure(&instrument_ethusdt.id())
            .unwrap()
            .as_f64(),
        26.59574468
    );
}

#[rstest]
fn test_market_value_when_insufficient_data_for_xrate_returns_none(
    mut portfolio: Portfolio,
    instrument_btcusdt: InstrumentAny,
    instrument_ethusdt: InstrumentAny,
) {
    let account_state = get_margin_account(Some("BITMEX-01234"));
    portfolio.update_account(&account_state);

    // Create Order
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_ethusdt.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();

    let filled = OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order.order_side(),
        order.order_type(),
        order.quantity(),
        Price::new(376.05, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("12.2 USD")),
    );

    let last_ethusd = get_quote_tick(&instrument_ethusdt, 376.05, 377.10, 16.0, 25.0);
    let last_xbtusd = get_quote_tick(&instrument_btcusdt, 50000.00, 50000.00, 1.0, 1.0);

    let position = Position::new(&instrument_ethusdt, filled);
    let position_opened = get_open_position(&position);

    // Act
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened));
    portfolio
        .cache
        .borrow_mut()
        .add_position(position, OmsType::Hedging)
        .unwrap();
    portfolio.cache.borrow_mut().add_quote(last_ethusd).unwrap();
    portfolio.cache.borrow_mut().add_quote(last_xbtusd).unwrap();
    portfolio.update_quote_tick(&last_ethusd);
    portfolio.update_quote_tick(&last_xbtusd);

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("BITMEX"))
            .unwrap()
            .get(&Currency::ETH())
            .unwrap()
            .as_f64(),
        0.26595745
    );
}

#[rstest]
fn test_opening_several_positions_updates_portfolio(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
    instrument_gbpusd: InstrumentAny,
) {
    let account_state = get_margin_account(None);
    portfolio.update_account(&account_state);

    let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
    let last_gbpusd = get_quote_tick(&instrument_gbpusd, 1.30315, 1.30317, 1.0, 1.0);

    portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
    portfolio.cache.borrow_mut().add_quote(last_gbpusd).unwrap();
    portfolio.update_quote_tick(&last_audusd);
    portfolio.update_quote_tick(&last_gbpusd);

    // Create Order
    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_gbpusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();

    portfolio
        .cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, true)
        .unwrap();
    portfolio
        .cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, true)
        .unwrap();

    let fill1 = OrderFilled::new(
        order1.trader_id(),
        order1.strategy_id(),
        order1.instrument_id(),
        order1.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order1.order_side(),
        order1.order_type(),
        order1.quantity(),
        Price::new(376.05, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("12.2 USD")),
    );
    let fill2 = OrderFilled::new(
        order2.trader_id(),
        order2.strategy_id(),
        order2.instrument_id(),
        order2.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order2.order_side(),
        order2.order_type(),
        order2.quantity(),
        Price::new(376.05, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("12.2 USD")),
    );

    portfolio.cache.borrow_mut().update_order(&order1).unwrap();
    portfolio.cache.borrow_mut().update_order(&order2).unwrap();

    let position1 = Position::new(&instrument_audusd, fill1);
    let position2 = Position::new(&instrument_gbpusd, fill2);

    let position_opened1 = get_open_position(&position1);
    let position_opened2 = get_open_position(&position2);

    // Act
    portfolio
        .cache
        .borrow_mut()
        .add_position(position1, OmsType::Hedging)
        .unwrap();
    portfolio
        .cache
        .borrow_mut()
        .add_position(position2, OmsType::Hedging)
        .unwrap();
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened2));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("SIM"))
            .unwrap()
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        100000.0
    );

    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -37500000.0
    );

    assert_eq!(
        portfolio
            .realized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -12.2
    );
    // FIX: TODO: should not be empty
    assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
    assert_eq!(
        portfolio
            .net_exposure(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        100000.0
    );
    assert_eq!(
        portfolio
            .net_exposure(&instrument_gbpusd.id())
            .unwrap()
            .as_f64(),
        100000.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        0.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnl(&instrument_gbpusd.id())
            .unwrap()
            .as_f64(),
        -37500000.0
    );
    assert_eq!(
        portfolio
            .realized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        0.0
    );
    assert_eq!(
        portfolio
            .realized_pnl(&instrument_gbpusd.id())
            .unwrap()
            .as_f64(),
        -12.2
    );
    assert_eq!(
        portfolio.net_position(&instrument_audusd.id()),
        Decimal::from_f64(100000.0).unwrap()
    );
    assert_eq!(
        portfolio.net_position(&instrument_gbpusd.id()),
        Decimal::from_f64(100000.0).unwrap()
    );
    assert!(portfolio.is_net_long(&instrument_audusd.id()));
    assert!(!portfolio.is_net_short(&instrument_audusd.id()));
    assert!(!portfolio.is_flat(&instrument_audusd.id()));
    assert!(!portfolio.is_completely_flat());
}

#[rstest]
fn test_modifying_position_updates_portfolio(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let account_state = get_margin_account(None);
    portfolio.update_account(&account_state);

    let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
    portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
    portfolio.update_quote_tick(&last_audusd);

    // Create Order
    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();

    let fill1 = OrderFilled::new(
        order1.trader_id(),
        order1.strategy_id(),
        order1.instrument_id(),
        order1.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order1.order_side(),
        order1.order_type(),
        order1.quantity(),
        Price::new(376.05, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("12.2 USD")),
    );

    let mut position1 = Position::new(&instrument_audusd, fill1);
    portfolio
        .cache
        .borrow_mut()
        .add_position(position1.clone(), OmsType::Hedging)
        .unwrap();
    let position_opened1 = get_open_position(&position1);
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("50000"))
        .build();

    let fill2 = OrderFilled::new(
        order2.trader_id(),
        order2.strategy_id(),
        order2.instrument_id(),
        order2.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("2"),
        order2.order_side(),
        order2.order_type(),
        order2.quantity(),
        Price::new(1.00, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("1.2 USD")),
    );

    position1.apply(&fill2);
    let position1_changed = get_changed_position(&position1);

    // Act
    portfolio.update_position(&PositionEvent::PositionChanged(position1_changed));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("SIM"))
            .unwrap()
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        100000.0
    );

    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -37500000.0
    );

    assert_eq!(
        portfolio
            .realized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -12.2
    );
    // FIX: TODO: should not be empty
    assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
    assert_eq!(
        portfolio
            .net_exposure(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        100000.0
    );
    assert_eq!(
        portfolio
            .unrealized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        -37500000.0
    );
    assert_eq!(
        portfolio
            .realized_pnl(&instrument_audusd.id())
            .unwrap()
            .as_f64(),
        -12.2
    );
    assert_eq!(
        portfolio.net_position(&instrument_audusd.id()),
        Decimal::from_f64(100000.0).unwrap()
    );
    assert!(portfolio.is_net_long(&instrument_audusd.id()));
    assert!(!portfolio.is_net_short(&instrument_audusd.id()));
    assert!(!portfolio.is_flat(&instrument_audusd.id()));
    assert!(!portfolio.is_completely_flat());
    assert_eq!(
        portfolio.unrealized_pnls(&Venue::from("BINANCE")),
        HashMap::new()
    );
    assert_eq!(
        portfolio.realized_pnls(&Venue::from("BINANCE")),
        HashMap::new()
    );
    assert_eq!(portfolio.net_exposures(&Venue::from("BINANCE")), None);
}

#[rstest]
fn test_closing_position_updates_portfolio(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
) {
    let account_state = get_margin_account(None);
    portfolio.update_account(&account_state);

    let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
    portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
    portfolio.update_quote_tick(&last_audusd);

    // Create Order
    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();

    let fill1 = OrderFilled::new(
        order1.trader_id(),
        order1.strategy_id(),
        order1.instrument_id(),
        order1.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order1.order_side(),
        order1.order_type(),
        order1.quantity(),
        Price::new(376.05, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("12.2 USD")),
    );

    let mut position1 = Position::new(&instrument_audusd, fill1);
    portfolio
        .cache
        .borrow_mut()
        .add_position(position1.clone(), OmsType::Hedging)
        .unwrap();
    let position_opened1 = get_open_position(&position1);
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));

    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("50000"))
        .build();

    let fill2 = OrderFilled::new(
        order2.trader_id(),
        order2.strategy_id(),
        order2.instrument_id(),
        order2.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("2"),
        order2.order_side(),
        order2.order_type(),
        order2.quantity(),
        Price::new(1.00, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("SSD")),
        Some(Money::from("1.2 USD")),
    );

    position1.apply(&fill2);
    portfolio
        .cache
        .borrow_mut()
        .update_position(&position1)
        .unwrap();

    // Act
    let position1_closed = get_close_position(&position1);
    portfolio.update_position(&PositionEvent::PositionClosed(position1_closed));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("SIM"))
            .unwrap()
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        100000.00
    );
    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -37500000.00
    );
    assert_eq!(
        portfolio
            .realized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        -12.2
    );
    assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
}

#[rstest]
fn test_several_positions_with_different_instruments_updates_portfolio(
    mut portfolio: Portfolio,
    instrument_audusd: InstrumentAny,
    instrument_gbpusd: InstrumentAny,
) {
    let account_state = get_margin_account(None);
    portfolio.update_account(&account_state);

    // Create Order
    let order1 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();
    let order2 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_audusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();
    let order3 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_gbpusd.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100000"))
        .build();
    let order4 = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_gbpusd.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("100000"))
        .build();

    let fill1 = OrderFilled::new(
        order1.trader_id(),
        StrategyId::new("S-1"),
        order1.instrument_id(),
        order1.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("1"),
        order1.order_side(),
        order1.order_type(),
        order1.quantity(),
        Price::new(1.0, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("P-1")),
        None,
    );
    let fill2 = OrderFilled::new(
        order2.trader_id(),
        StrategyId::new("S-1"),
        order2.instrument_id(),
        order2.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("2"),
        order2.order_side(),
        order2.order_type(),
        order2.quantity(),
        Price::new(1.0, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("P-2")),
        None,
    );
    let fill3 = OrderFilled::new(
        order3.trader_id(),
        StrategyId::new("S-1"),
        order3.instrument_id(),
        order3.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("3"),
        order3.order_side(),
        order3.order_type(),
        order3.quantity(),
        Price::new(1.0, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("P-3")),
        None,
    );
    let fill4 = OrderFilled::new(
        order4.trader_id(),
        StrategyId::new("S-1"),
        order4.instrument_id(),
        order4.client_order_id(),
        VenueOrderId::new("123456"),
        AccountId::new("SIM-001"),
        TradeId::new("4"),
        order4.order_side(),
        order4.order_type(),
        order4.quantity(),
        Price::new(1.0, 0),
        Currency::USD(),
        LiquiditySide::Taker,
        uuid4(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::new("P-4")),
        None,
    );

    let position1 = Position::new(&instrument_audusd, fill1);
    let position2 = Position::new(&instrument_audusd, fill2);
    let mut position3 = Position::new(&instrument_gbpusd, fill3);

    let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
    let last_gbpusd = get_quote_tick(&instrument_gbpusd, 1.30315, 1.30317, 1.0, 1.0);

    portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
    portfolio.cache.borrow_mut().add_quote(last_gbpusd).unwrap();
    portfolio.update_quote_tick(&last_audusd);
    portfolio.update_quote_tick(&last_gbpusd);

    portfolio
        .cache
        .borrow_mut()
        .add_position(position1.clone(), OmsType::Hedging)
        .unwrap();
    portfolio
        .cache
        .borrow_mut()
        .add_position(position2.clone(), OmsType::Hedging)
        .unwrap();
    portfolio
        .cache
        .borrow_mut()
        .add_position(position3.clone(), OmsType::Hedging)
        .unwrap();

    let position_opened1 = get_open_position(&position1);
    let position_opened2 = get_open_position(&position2);
    let position_opened3 = get_open_position(&position3);

    portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened2));
    portfolio.update_position(&PositionEvent::PositionOpened(position_opened3));

    let position_closed3 = get_close_position(&position3);
    position3.apply(&fill4);
    portfolio
        .cache
        .borrow_mut()
        .add_position(position3.clone(), OmsType::Hedging)
        .unwrap();
    portfolio.update_position(&PositionEvent::PositionClosed(position_closed3));

    // Assert
    assert_eq!(
        portfolio
            .net_exposures(&Venue::from("SIM"))
            .unwrap()
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        200000.00
    );
    assert_eq!(
        portfolio
            .unrealized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        0.0
    );
    assert_eq!(
        portfolio
            .realized_pnls(&Venue::from("SIM"))
            .get(&Currency::USD())
            .unwrap()
            .as_f64(),
        0.0
    );
    // FIX: TODO: should not be empty
    assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
}

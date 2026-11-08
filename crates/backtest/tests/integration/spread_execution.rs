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

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
};
use nautilus_common::actor::DataActor;
use nautilus_core::UnixNanos;
use nautilus_execution::models::fee::{FeeModelAny, PerContractFeeModel};
use nautilus_model::{
    accounts::Account,
    data::{Data, QuoteTick},
    enums::{
        AccountType, BookType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType,
        PositionSide,
    },
    events::OrderFilled,
    identifiers::{InstrumentId, PositionId, StrategyId, Symbol, Venue},
    instruments::{
        Instrument, InstrumentAny,
        stubs::{
            default_fx_ccy, futures_contract_es, futures_spread_es, option_contract_appl,
            option_spread,
        },
    },
    orders::Order,
    position::Position,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore, nautilus_strategy};
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

struct SpreadRoundTrip {
    core: StrategyCore,
    spread_id: InstrumentId,
    trigger_id: InstrumentId,
    opening_side: OrderSide,
    fills: Rc<RefCell<Vec<OrderFilled>>>,
    positions_on_parent: Rc<RefCell<Vec<Vec<Position>>>>,
}

impl SpreadRoundTrip {
    fn new(
        spread_id: InstrumentId,
        trigger_id: InstrumentId,
        opening_side: OrderSide,
        fills: Rc<RefCell<Vec<OrderFilled>>>,
        positions_on_parent: Rc<RefCell<Vec<Vec<Position>>>>,
    ) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("SPREAD-001")),
                order_id_tag: Some("001".to_string()),
                ..Default::default()
            }),
            spread_id,
            trigger_id,
            opening_side,
            fills,
            positions_on_parent,
        }
    }
}

nautilus_strategy!(SpreadRoundTrip, {
    fn on_order_filled(&mut self, event: &OrderFilled) {
        self.fills.borrow_mut().push(event.clone());
        if event.instrument_id == self.spread_id {
            let positions = self.cache().positions(None, None, None, None, None);
            self.positions_on_parent.borrow_mut().push(positions);
        }
    }
});

impl Debug for SpreadRoundTrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SpreadRoundTrip)).finish()
    }
}

impl DataActor for SpreadRoundTrip {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.trigger_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        let side = match quote.ts_init.as_u64() {
            4_000_000_000 => self.opening_side,
            8_000_000_000 => self.opening_side.opposite(),
            _ => return Ok(()),
        };

        let order = self.order().market(
            self.spread_id,
            side,
            Quantity::from(3),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)
    }
}

#[rstest]
#[case::option_buy(
    true,
    OrderSide::Buy,
    AccountType::Margin,
    Money::from("1_000_000 USD")
)]
#[case::option_cash_buy(true, OrderSide::Buy, AccountType::Cash, Money::from("1_000_000 USD"))]
#[case::option_sell(
    true,
    OrderSide::Sell,
    AccountType::Margin,
    Money::from("1_000_000 USD")
)]
#[case::futures_buy(
    false,
    OrderSide::Buy,
    AccountType::Margin,
    Money::from("1_000_000 USD")
)]
#[case::futures_sell(
    false,
    OrderSide::Sell,
    AccountType::Margin,
    Money::from("1_000_000 USD")
)]
#[case::option_cash_buy_low_funded(
    true,
    OrderSide::Buy,
    AccountType::Cash,
    Money::from("2_000 USD")
)]
fn test_spread_round_trip_books_partial_leg_fills(
    #[case] options: bool,
    #[case] opening_side: OrderSide,
    #[case] account_type: AccountType,
    #[case] starting_balance: Money,
) {
    let (leg_a, leg_b, spread) = spread_instruments(options, Currency::USD());
    let leg_a_id = leg_a.id();
    let leg_b_id = leg_b.id();
    let spread_id = spread.id();
    let venue = spread_id.venue;

    let mut engine = BacktestEngine::new(BacktestEngineConfig {
        bypass_logging: true,
        run_analysis: false,
        ..Default::default()
    })
    .unwrap();

    let fee_model = PerContractFeeModel::new(Money::from("0.50 USD")).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(account_type)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![starting_balance])
        .fee_model(FeeModelAny::PerContract(fee_model).into())
        .build()
        .unwrap();
    engine.add_venue(venue_config).unwrap();
    for instrument in [&leg_a, &leg_b, &spread] {
        engine.add_instrument(instrument).unwrap();
    }

    let fills = Rc::new(RefCell::new(Vec::new()));
    let positions_on_parent = Rc::new(RefCell::new(Vec::new()));

    let strategy = SpreadRoundTrip::new(
        spread_id,
        leg_b_id,
        opening_side,
        Rc::clone(&fills),
        Rc::clone(&positions_on_parent),
    );

    engine.add_strategy(strategy).unwrap();
    let data = vec![
        spread_quote(leg_a_id, "10.00", "10.25", 100, 1),
        spread_quote(leg_b_id, "2.00", "2.25", 100, 2),
        spread_quote(spread_id, "5.50", "6.25", 1, 3),
        spread_quote(leg_b_id, "2.00", "2.25", 100, 4),
        spread_quote(leg_a_id, "12.00", "12.25", 100, 5),
        spread_quote(leg_b_id, "2.50", "2.75", 100, 6),
        spread_quote(spread_id, "6.50", "7.25", 1, 7),
        spread_quote(leg_b_id, "2.50", "2.75", 100, 8),
    ];
    engine.add_data(data, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let fills = fills.borrow();
    let snapshots = positions_on_parent.borrow();
    let cache = engine.kernel().cache.borrow();
    let orders = cache.orders(Some(&venue), None, None, None, None);
    let positions = cache.positions_closed(Some(&venue), None, None, None, None);
    let position_a = positions
        .iter()
        .find(|position| position.instrument_id == leg_a_id)
        .unwrap();
    let position_b = positions
        .iter()
        .find(|position| position.instrument_id == leg_b_id)
        .unwrap();
    let multiplier = leg_a.multiplier().as_decimal();

    let (prices_a, prices_b, prices_spread, gross_a, gross_b) = match opening_side {
        OrderSide::Buy => (
            ["10.25", "10.26", "12.00", "11.99"],
            ["2.00", "2.00", "2.75", "2.75"],
            ["6.25", "6.26", "6.50", "6.49"],
            dec!(5.21),
            dec!(-4.50),
        ),
        OrderSide::Sell => (
            ["10.00", "9.99", "12.25", "12.26"],
            ["2.25", "2.25", "2.50", "2.50"],
            ["5.50", "5.49", "7.25", "7.26"],
            dec!(-6.79),
            dec!(1.50),
        ),
    };

    let expected_pnl_a =
        Money::from_decimal(gross_a * multiplier - dec!(3.00), Currency::USD()).unwrap();
    let expected_pnl_b =
        Money::from_decimal(gross_b * multiplier - dec!(6.00), Currency::USD()).unwrap();
    let expected_balance = Money::from_decimal(
        starting_balance.as_decimal() + expected_pnl_a.as_decimal() + expected_pnl_b.as_decimal(),
        Currency::USD(),
    )
    .unwrap();
    let account = cache.account_for_venue(&venue).unwrap();

    assert_eq!(orders.len(), 2);

    for order in &orders {
        assert_eq!(order.instrument_id(), spread_id);
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.quantity(), Quantity::from(3));
        assert_eq!(order.filled_qty(), Quantity::from(3));
    }

    assert_eq!(fills.len(), 12);
    assert_eq!(snapshots.len(), 4);

    for (index, execution) in fills.as_chunks::<3>().0.iter().enumerate() {
        let [first, second, parent] = execution;
        let legs = [first, second];
        let fill_a = legs
            .iter()
            .copied()
            .find(|fill| fill.instrument_id == leg_a_id)
            .unwrap();
        let fill_b = legs
            .iter()
            .copied()
            .find(|fill| fill.instrument_id == leg_b_id)
            .unwrap();

        assert_leg_dispatch_order(execution, account_type, [leg_a_id, leg_b_id]);

        let quantity = if index % 2 == 0 { 1 } else { 2 };

        let side = if index < 2 {
            opening_side
        } else {
            opening_side.opposite()
        };

        assert_eq!(parent.instrument_id, spread_id);
        assert_eq!(parent.order_side, side);
        assert_eq!(parent.last_qty, Quantity::from(quantity));
        assert_eq!(parent.last_px, Price::from(prices_spread[index]));
        assert_eq!(
            parent.commission,
            Some(
                Money::from_decimal(Decimal::from(quantity) * dec!(1.50), Currency::USD()).unwrap()
            )
        );
        assert_eq!(
            parent.position_id,
            Some(PositionId::from(format!("{spread_id}-SPREAD-001"))),
        );
        let (trade_prefix, trade_sequence) = parent.trade_id.as_str().rsplit_once('-').unwrap();
        assert_eq!(trade_sequence.parse::<usize>().unwrap(), index * 3 + 3);

        for (leg_index, (fill, instrument, ratio, price)) in [
            (fill_a, &leg_a, 1, prices_a[index]),
            (fill_b, &leg_b, 2, prices_b[index]),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(fill.instrument_id, instrument.id());
            assert_eq!(fill.trader_id, parent.trader_id);
            assert_eq!(fill.strategy_id, parent.strategy_id);
            assert_eq!(fill.account_id, parent.account_id);
            assert_eq!(
                fill.position_id,
                Some(if leg_index == 0 {
                    position_a.id
                } else {
                    position_b.id
                }),
            );
            assert_eq!(
                fill.order_side,
                if leg_index == 0 {
                    side
                } else {
                    side.opposite()
                }
            );
            assert_eq!(fill.order_type, OrderType::Market);
            assert_eq!(fill.last_qty, Quantity::from(quantity * ratio));
            assert_eq!(fill.last_px, Price::from(price));
            assert_eq!(fill.currency, Currency::USD());
            assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
            assert_eq!(
                fill.commission,
                Some(
                    Money::from_decimal(
                        Decimal::from(quantity * ratio) * dec!(0.50),
                        Currency::USD()
                    )
                    .unwrap()
                )
            );
            assert_eq!(fill.ts_event, parent.ts_event);
            assert_eq!(fill.ts_init, parent.ts_init);
            let dispatch_index = legs
                .iter()
                .position(|candidate| candidate.instrument_id == instrument.id())
                .unwrap();
            assert_eq!(
                fill.trade_id.as_str(),
                format!("{trade_prefix}-{:03}", index * 3 + dispatch_index + 1)
            );
            assert_eq!(
                fill.client_order_id.as_str(),
                format!("{}-LEG-{}", parent.client_order_id, instrument.id().symbol)
            );
            assert_eq!(
                fill.venue_order_id.as_str(),
                format!("{}-LEG-{leg_index}", parent.venue_order_id)
            );
            assert!(!fill.reconciliation);
        }

        assert_eq!(
            fill_a.last_px.as_decimal() - dec!(2) * fill_b.last_px.as_decimal(),
            parent.last_px.as_decimal()
        );
    }

    assert_open_leg_positions(&snapshots, leg_a_id, leg_b_id, opening_side);
    for (instrument_id, quantity, commission) in [
        (leg_a_id, 3, Money::from("1.50 USD")),
        (leg_b_id, 6, Money::from("3.00 USD")),
    ] {
        let position = snapshots[1]
            .iter()
            .find(|position| position.instrument_id == instrument_id)
            .unwrap();
        assert_eq!(position.quantity, Quantity::from(quantity));
        assert_eq!(position.commissions(), vec![commission]);
        assert_eq!(position.realized_pnl, Some(-commission));
    }

    assert_eq!(positions.len(), 2);
    assert_eq!(
        cache.positions_open_count(Some(&venue), None, None, None, None),
        0
    );
    assert_eq!(
        cache
            .positions(None, Some(&spread_id), None, None, None)
            .len(),
        0
    );

    for (position, quantity, pnl, commission) in [
        (position_a, 3, expected_pnl_a, Money::from("3.00 USD")),
        (position_b, 6, expected_pnl_b, Money::from("6.00 USD")),
    ] {
        assert_eq!(position.quantity, Quantity::from(0));
        assert_eq!(position.peak_qty, Quantity::from(quantity));
        assert_eq!(position.buy_qty, Quantity::from(quantity));
        assert_eq!(position.sell_qty, Quantity::from(quantity));
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.multiplier, leg_a.multiplier());
        assert_eq!(position.quote_currency, Currency::USD());
        assert_eq!(position.settlement_currency, Currency::USD());
        assert_eq!(position.events.len(), 4);
        assert_eq!(position.trade_ids().len(), 4);
        assert_eq!(position.commissions(), vec![commission]);
        assert_eq!(position.realized_pnl, Some(pnl));
    }

    assert_eq!(
        account.balance_total(Some(Currency::USD())),
        Some(expected_balance)
    );
    assert_eq!(
        account.balance_free(Some(Currency::USD())),
        Some(expected_balance)
    );
    assert_eq!(
        account.balance_locked(Some(Currency::USD())),
        Some(Money::zero(Currency::USD()))
    );
    assert_eq!(
        engine
            .kernel()
            .portfolio
            .borrow_mut()
            .realized_pnl(&leg_a_id),
        Some(expected_pnl_a)
    );
    assert_eq!(
        engine
            .kernel()
            .portfolio
            .borrow_mut()
            .realized_pnl(&leg_b_id),
        Some(expected_pnl_b)
    );
}

#[rstest]
#[case::usd(Currency::USD(), None, dec!(1191.85), false)]
#[case::eur(Currency::EUR(), Some(Currency::USD()), dec!(1432.45), false)]
#[case::usd_credit(Currency::USD(), None, dec!(1185.85), true)]
#[case::eur_credit(Currency::EUR(), Some(Currency::USD()), dec!(1425.10), true)]
fn test_cash_spread_credits_precede_debits_with_fees_above_premium(
    #[case] currency: Currency,
    #[case] base_currency: Option<Currency>,
    #[case] balance: Decimal,
    #[case] credit: bool,
) {
    let (leg_a, leg_b, mut spread) = spread_instruments(true, currency);
    let leg_a_id = leg_a.id();
    let leg_b_id = leg_b.id();

    if !credit && let InstrumentAny::OptionSpread(instrument) = &mut spread {
        instrument.id = InstrumentId::from("(1)LEG-A___(2)LEG-B.OPRA");
    }

    let opening_side = if credit {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    let spread_price = if credit { "-3.99" } else { "4.01" };
    let spread_id = spread.id();
    let venue = spread_id.venue;

    let mut engine = BacktestEngine::new(BacktestEngineConfig {
        bypass_logging: true,
        run_analysis: false,
        ..Default::default()
    })
    .unwrap();

    let fee_model = PerContractFeeModel::new(Money::from("1.25 USD")).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Cash)
        .maybe_base_currency(base_currency)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::from("0.10 USD")])
        .fee_model(FeeModelAny::PerContract(fee_model).into())
        .build()
        .unwrap();
    engine.add_venue(venue_config).unwrap();
    for instrument in [&leg_a, &leg_b, &spread] {
        engine.add_instrument(instrument).unwrap();
    }

    if currency == Currency::EUR() {
        add_eur_usd_quote(&engine, venue);
    }

    let fills = Rc::new(RefCell::new(Vec::new()));
    let positions_on_parent = Rc::new(RefCell::new(Vec::new()));
    engine
        .add_strategy(SpreadRoundTrip::new(
            spread_id,
            leg_b_id,
            opening_side,
            Rc::clone(&fills),
            Rc::clone(&positions_on_parent),
        ))
        .unwrap();
    let data = vec![
        spread_quote(leg_a_id, "0.01", "0.01", 100, 1),
        spread_quote(leg_b_id, "2.00", "2.00", 100, 2),
        spread_quote(spread_id, spread_price, spread_price, 100, 3),
        spread_quote(leg_b_id, "2.00", "2.00", 100, 4),
    ];
    engine.add_data(data, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let fills = fills.borrow();
    let cache = engine.kernel().cache.borrow();
    let account = cache.account_for_venue(&venue).unwrap();
    let positions = cache.positions_open(Some(&venue), None, None, None, None);
    assert_eq!(account.base_currency(), base_currency);
    assert_eq!(fills.len(), 3);
    assert_eq!(positions.len(), 2);
    assert_eq!(positions_on_parent.borrow().len(), 1);

    for (index, (instrument, side, quantity, price, commission, ordinal)) in [
        (&leg_b, OrderSide::Sell, 6, "2.00", "7.50 USD", 1),
        (&leg_a, opening_side, 3, "0.01", "3.75 USD", 0),
        (&spread, opening_side, 3, spread_price, "11.25 USD", 2),
    ]
    .into_iter()
    .enumerate()
    {
        let fill = &fills[index];
        assert_eq!(fill.instrument_id, instrument.id());
        assert_eq!(fill.order_side, side);
        assert_eq!(fill.order_type, OrderType::Market);
        assert_eq!(fill.last_qty, Quantity::from(quantity));
        assert_eq!(fill.last_px, Price::from(price));
        assert_eq!(fill.currency, currency);
        assert_eq!(fill.commission, Some(Money::from(commission)));
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
        assert_eq!(fill.ts_event, UnixNanos::from(4_000_000_000u64));
        let (prefix, _) = fills[2].trade_id.as_str().rsplit_once('-').unwrap();
        assert_eq!(fill.trade_id.as_str(), format!("{prefix}-{:03}", index + 1));

        if index < 2 {
            assert_eq!(
                fill.venue_order_id.as_str(),
                format!("{}-LEG-{ordinal}", fills[2].venue_order_id)
            );
            let position = positions
                .iter()
                .find(|position| position.instrument_id == instrument.id())
                .unwrap();
            assert_eq!(
                position.side,
                if side == OrderSide::Buy {
                    PositionSide::Long
                } else {
                    PositionSide::Short
                }
            );
            assert_eq!(position.quantity, Quantity::from(quantity));
            assert_eq!(position.commissions(), vec![Money::from(commission)]);
            assert_eq!(
                position.realized_pnl,
                Some(if currency == Currency::USD() {
                    -Money::from(commission)
                } else {
                    Money::zero(currency)
                })
            );
        }
    }

    assert_eq!(
        account.balance_total(Some(Currency::USD())),
        Some(Money::from_decimal_native(balance, Currency::USD()).unwrap())
    );
    assert_eq!(
        account.balance_free(Some(Currency::USD())),
        Some(Money::from_decimal_native(balance, Currency::USD()).unwrap())
    );
    assert_eq!(
        account.balance_locked(Some(Currency::USD())),
        Some(Money::zero(Currency::USD()))
    );
    assert_eq!(
        cache
            .positions(None, Some(&spread_id), None, None, None)
            .len(),
        0
    );
}

#[rstest]
#[case::cash_balance(
    AccountType::Cash,
    Currency::EUR(),
    None,
    Currency::EUR(),
    Money::from("1 USD")
)]
#[case::margin_balance(
    AccountType::Margin,
    Currency::EUR(),
    None,
    Currency::EUR(),
    Money::from("1 USD")
)]
#[case::margin_native_balance(
    AccountType::Margin,
    Currency::EUR(),
    None,
    Currency::USD(),
    Money::from("1 EUR")
)]
#[case::margin_cost_balance(
    AccountType::Margin,
    Currency::EUR(),
    None,
    Currency::USD(),
    Money::from("1 USD")
)]
#[case::margin_fee_rate(
    AccountType::Margin,
    Currency::USD(),
    Some(Currency::USD()),
    Currency::USD(),
    Money::from("1 EUR")
)]
#[case::margin_pnl_rate(
    AccountType::Margin,
    Currency::EUR(),
    Some(Currency::USD()),
    Currency::USD(),
    Money::from("0 USD")
)]
fn test_spread_rejects_missing_account_currency(
    #[case] account_type: AccountType,
    #[case] currency: Currency,
    #[case] base_currency: Option<Currency>,
    #[case] balance_currency: Currency,
    #[case] commission: Money,
) {
    let (leg_a, leg_b, spread) = spread_instruments(true, currency);
    let spread_id = spread.id();
    let venue = spread_id.venue;

    let mut engine = BacktestEngine::new(BacktestEngineConfig {
        bypass_logging: true,
        run_analysis: false,
        ..Default::default()
    })
    .unwrap();

    let balance = Money::from_decimal_native(dec!(2500), balance_currency).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(account_type)
        .maybe_base_currency(base_currency)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![balance])
        .fee_model(FeeModelAny::PerContract(PerContractFeeModel::new(commission).unwrap()).into())
        .build()
        .unwrap();
    engine.add_venue(venue_config).unwrap();
    for instrument in [&leg_a, &leg_b, &spread] {
        engine.add_instrument(instrument).unwrap();
    }

    let fills = Rc::new(RefCell::new(Vec::new()));
    let positions_on_parent = Rc::new(RefCell::new(Vec::new()));
    engine
        .add_strategy(SpreadRoundTrip::new(
            spread_id,
            leg_b.id(),
            OrderSide::Buy,
            Rc::clone(&fills),
            Rc::clone(&positions_on_parent),
        ))
        .unwrap();
    engine
        .add_data(
            vec![
                spread_quote(leg_a.id(), "10.00", "10.25", 100, 1),
                spread_quote(leg_b.id(), "2.00", "2.25", 100, 2),
                spread_quote(spread_id, "5.50", "6.25", 100, 3),
                spread_quote(leg_b.id(), "2.00", "2.25", 100, 4),
                spread_quote(leg_a.id(), "11.00", "11.25", 100, 5),
                spread_quote(spread_id, "5.50", "6.25", 100, 6),
                spread_quote(leg_b.id(), "2.50", "2.75", 100, 8),
            ],
            None,
            true,
            true,
        )
        .unwrap();
    engine.run(None, None, None, false).unwrap();
    let cache = engine.kernel().cache.borrow();
    let account = cache.account_for_venue(&venue).unwrap();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(fills.borrow().len(), 0);
    assert_eq!(positions_on_parent.borrow().len(), 0);
    assert_eq!(cache.positions_total_count(None, None, None, None, None), 0);
    assert_eq!(orders.len(), 2);

    for order in orders {
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(order.quantity(), Quantity::from(3));
        assert_eq!(order.filled_qty(), Quantity::from(0));
        assert_eq!(order.leaves_qty(), Quantity::from(3));
    }

    assert_eq!(account.base_currency(), base_currency);
    assert_eq!(account.balances().len(), 1);
    assert_eq!(account.balance_total(Some(balance_currency)), Some(balance));
    assert_eq!(account.balance_free(Some(balance_currency)), Some(balance));
    assert_eq!(
        account.balance_locked(Some(balance_currency)),
        Some(Money::zero(balance_currency))
    );
}

#[rstest]
fn test_cash_spread_insufficient_foreign_commission_aborts_run() {
    let (leg_a, leg_b, spread) = spread_instruments(true, Currency::EUR());
    let spread_id = spread.id();
    let venue = spread_id.venue;

    let mut engine = BacktestEngine::new(BacktestEngineConfig {
        bypass_logging: true,
        run_analysis: false,
        ..Default::default()
    })
    .unwrap();

    let fee_model = PerContractFeeModel::new(Money::from("1 USD")).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Cash)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::from("2500 EUR"), Money::from("7 USD")])
        .fee_model(FeeModelAny::PerContract(fee_model).into())
        .build()
        .unwrap();
    engine.add_venue(venue_config).unwrap();
    for instrument in [&leg_a, &leg_b, &spread] {
        engine.add_instrument(instrument).unwrap();
    }

    let fills = Rc::new(RefCell::new(Vec::new()));
    engine
        .add_strategy(SpreadRoundTrip::new(
            spread_id,
            leg_b.id(),
            OrderSide::Buy,
            Rc::clone(&fills),
            Rc::new(RefCell::new(Vec::new())),
        ))
        .unwrap();
    engine
        .add_data(
            vec![
                spread_quote(leg_a.id(), "10.00", "10.25", 100, 1),
                spread_quote(leg_b.id(), "2.00", "2.25", 100, 2),
                spread_quote(spread_id, "5.50", "6.25", 100, 3),
                spread_quote(leg_b.id(), "2.00", "2.25", 100, 4),
            ],
            None,
            true,
            true,
        )
        .unwrap();

    let error = engine.run(None, None, None, false).unwrap_err().to_string();
    let account_id = engine
        .kernel()
        .cache
        .borrow()
        .account_for_venue(&venue)
        .unwrap()
        .id();
    let published = fills.borrow().clone();
    let retry_error = engine.run(None, None, None, false).unwrap_err().to_string();
    let (_, cause) = error.split_once(": ").unwrap();
    assert_eq!(
        cause,
        format!(
            "Cash account balance would become negative: -2.00 USD (borrowing not allowed for {account_id})"
        )
    );
    assert_eq!(retry_error, error);
    assert_eq!(*fills.borrow(), published);
}

#[rstest]
#[case::eur_fee(
    Money::from("0.50 EUR"),
    Some(Currency::USD()),
    None,
    dec!(90.775),
    ["3.00 EUR", "1.50 EUR", "4.50 EUR"],
    ["-3.00 EUR", "-1.50 EUR"],
    false,
)]
#[case::zero_usd_fee(
    Money::zero(Currency::USD()),
    Some(Currency::USD()),
    None,
    dec!(96.25),
    ["0 USD"; 3],
    ["0 EUR"; 2],
    false,
)]
#[case::usd_fee(
    Money::from("1.00 USD"),
    Some(Currency::USD()),
    None,
    dec!(87.25),
    ["6.00 USD", "3.00 USD", "9.00 USD"],
    ["0 EUR"; 2],
    false,
)]
#[case::zero_usd_fee_without_account_base(
    Money::zero(Currency::USD()),
    None,
    None,
    dec!(625),
    ["0 USD"; 3],
    ["0 EUR"; 2],
    false,
)]
#[case::usd_fee_without_account_base(
    Money::from("1.00 USD"),
    None,
    Some((Money::from("20 USD"), Money::from("11 USD"))),
    dec!(625),
    ["6.00 USD", "3.00 USD", "9.00 USD"],
    ["0 EUR"; 2],
    false,
)]
#[case::usd_fee_with_borrowing(
    Money::from("1.00 USD"),
    None,
    Some((Money::from("7 USD"), Money::from("-2 USD"))),
    dec!(625),
    ["6.00 USD", "3.00 USD", "9.00 USD"],
    ["0 EUR"; 2],
    true,
)]
#[case::usd_fee_exact_funding(
    Money::from("1.00 USD"),
    None,
    Some((Money::from("9 USD"), Money::from("0 USD"))),
    dec!(625),
    ["6.00 USD", "3.00 USD", "9.00 USD"],
    ["0 EUR"; 2],
    false,
)]
fn test_cash_spread_credits_precede_debits_with_converted_account_currency(
    #[case] commission: Money,
    #[case] base_currency: Option<Currency>,
    #[case] usd_balances: Option<(Money, Money)>,
    #[case] balance: Decimal,
    #[case] commissions: [&str; 3],
    #[case] realized_pnls: [&str; 2],
    #[case] allow_borrowing: bool,
) {
    let balance_currency = base_currency.unwrap_or(Currency::EUR());
    let (leg_a, leg_b, spread) = spread_instruments(true, Currency::EUR());
    let leg_a_id = leg_a.id();
    let leg_b_id = leg_b.id();
    let spread_id = spread.id();
    let venue = spread_id.venue;

    let mut engine = BacktestEngine::new(BacktestEngineConfig {
        bypass_logging: true,
        run_analysis: false,
        ..Default::default()
    })
    .unwrap();

    let mut starting_balances =
        vec![Money::from_decimal_native(dec!(2500), balance_currency).unwrap()];

    if let Some((starting_usd, _)) = usd_balances {
        starting_balances.push(starting_usd);
    }

    let fee_model = PerContractFeeModel::new(commission).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(venue)
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Cash)
        .allow_cash_borrowing(allow_borrowing)
        .maybe_base_currency(base_currency)
        .book_type(BookType::L1_MBP)
        .starting_balances(starting_balances)
        .fee_model(FeeModelAny::PerContract(fee_model).into())
        .build()
        .unwrap();
    engine.add_venue(venue_config).unwrap();
    for instrument in [&leg_a, &leg_b, &spread] {
        engine.add_instrument(instrument).unwrap();
    }

    if base_currency.is_some() {
        add_eur_usd_quote(&engine, venue);
    }

    let fills = Rc::new(RefCell::new(Vec::new()));
    let positions_on_parent = Rc::new(RefCell::new(Vec::new()));
    engine
        .add_strategy(SpreadRoundTrip::new(
            spread_id,
            leg_b_id,
            OrderSide::Buy,
            Rc::clone(&fills),
            Rc::clone(&positions_on_parent),
        ))
        .unwrap();
    let data = vec![
        spread_quote(leg_a_id, "10.00", "10.25", 100, 1),
        spread_quote(leg_b_id, "2.00", "2.25", 100, 2),
        spread_quote(spread_id, "5.50", "6.25", 100, 3),
        spread_quote(leg_b_id, "2.00", "2.25", 100, 4),
    ];
    engine.add_data(data, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let fills = fills.borrow();
    let cache = engine.kernel().cache.borrow();
    let account = cache.account_for_venue(&venue).unwrap();
    let positions = cache.positions_open(Some(&venue), None, None, None, None);
    let expected_balance = Money::from_decimal_native(balance, balance_currency).unwrap();
    assert_eq!(fills.len(), 3);
    assert_eq!(positions.len(), 2);
    assert_eq!(positions_on_parent.borrow().len(), 1);

    for (index, (instrument, side, quantity, price, commission, ordinal)) in [
        (&leg_b, OrderSide::Sell, 6, "2.00", commissions[0], 1),
        (&leg_a, OrderSide::Buy, 3, "10.25", commissions[1], 0),
        (&spread, OrderSide::Buy, 3, "6.25", commissions[2], 2),
    ]
    .into_iter()
    .enumerate()
    {
        let fill = &fills[index];
        assert_eq!(fill.instrument_id, instrument.id());
        assert_eq!(fill.order_side, side);
        assert_eq!(fill.order_type, OrderType::Market);
        assert_eq!(fill.last_qty, Quantity::from(quantity));
        assert_eq!(fill.last_px, Price::from(price));
        assert_eq!(fill.currency, Currency::EUR());
        assert_eq!(fill.commission, Some(Money::from(commission)));
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
        assert_eq!(fill.ts_event, UnixNanos::from(4_000_000_000u64));
        let (prefix, _) = fills[2].trade_id.as_str().rsplit_once('-').unwrap();
        assert_eq!(fill.trade_id.as_str(), format!("{prefix}-{:03}", index + 1));

        if index < 2 {
            assert_eq!(
                fill.venue_order_id.as_str(),
                format!("{}-LEG-{ordinal}", fills[2].venue_order_id)
            );
            let position = positions
                .iter()
                .find(|position| position.instrument_id == instrument.id())
                .unwrap();
            assert_eq!(
                position.side,
                if side == OrderSide::Buy {
                    PositionSide::Long
                } else {
                    PositionSide::Short
                }
            );
            assert_eq!(position.quantity, Quantity::from(quantity));
            assert_eq!(position.multiplier, Quantity::from(100));
            assert_eq!(position.quote_currency, Currency::EUR());
            assert_eq!(position.settlement_currency, Currency::EUR());
            assert_eq!(position.commissions(), vec![Money::from(commission)]);
            assert_eq!(
                position.realized_pnl,
                Some(Money::from(realized_pnls[index]))
            );
        }
    }

    assert_eq!(account.base_currency(), base_currency);
    assert_eq!(
        account.balance_total(Some(balance_currency)),
        Some(expected_balance)
    );
    assert_eq!(
        account.balance_free(Some(balance_currency)),
        Some(expected_balance)
    );
    assert_eq!(
        account.balance_locked(Some(balance_currency)),
        Some(Money::zero(balance_currency))
    );

    if let Some((_, expected_usd)) = usd_balances {
        assert_eq!(account.balances().len(), 2);
        assert_eq!(
            account.balance_total(Some(Currency::USD())),
            Some(expected_usd)
        );
        assert_eq!(
            account.balance_free(Some(Currency::USD())),
            Some(expected_usd)
        );
        assert_eq!(
            account.balance_locked(Some(Currency::USD())),
            Some(Money::zero(Currency::USD()))
        );
    } else {
        assert_eq!(account.balances().len(), 1);
    }

    assert_eq!(
        cache
            .positions(None, Some(&spread_id), None, None, None)
            .len(),
        0
    );
}

fn add_eur_usd_quote(engine: &BacktestEngine, venue: Venue) {
    // The FX pair prices conversions without trading in the single-currency cash venue
    let fx = default_fx_ccy(Symbol::from("EUR/USD"), Some(venue));
    let fx_id = fx.id();
    let mut cache = engine.kernel().cache.borrow_mut();
    cache.add_instrument(fx.into()).unwrap();
    cache
        .add_quote(QuoteTick::new(
            fx_id,
            Price::from("1.20000"),
            Price::from("1.25000"),
            Quantity::from(100),
            Quantity::from(100),
            UnixNanos::default(),
            UnixNanos::default(),
        ))
        .unwrap();
}

fn assert_leg_dispatch_order(
    execution: &[OrderFilled; 3],
    account_type: AccountType,
    leg_ids: [InstrumentId; 2],
) {
    let [first, second, _] = execution;

    if account_type == AccountType::Cash {
        assert_eq!(first.order_side, OrderSide::Sell);
        assert_eq!(second.order_side, OrderSide::Buy);
    } else {
        assert_eq!(first.instrument_id, leg_ids[0]);
        assert_eq!(second.instrument_id, leg_ids[1]);
    }
}

fn assert_open_leg_positions(
    snapshots: &[Vec<Position>],
    leg_a_id: InstrumentId,
    leg_b_id: InstrumentId,
    opening_side: OrderSide,
) {
    for snapshot in &snapshots[..2] {
        assert_eq!(snapshot.len(), 2);
        let a = snapshot
            .iter()
            .find(|position| position.instrument_id == leg_a_id)
            .unwrap();
        let b = snapshot
            .iter()
            .find(|position| position.instrument_id == leg_b_id)
            .unwrap();
        assert_eq!(
            a.side,
            if opening_side == OrderSide::Buy {
                PositionSide::Long
            } else {
                PositionSide::Short
            }
        );
        assert_eq!(
            b.side,
            if opening_side == OrderSide::Buy {
                PositionSide::Short
            } else {
                PositionSide::Long
            }
        );
    }
}

fn spread_instruments(
    options: bool,
    currency: Currency,
) -> (InstrumentAny, InstrumentAny, InstrumentAny) {
    let venue = if options { "OPRA" } else { "GLBX" };
    let leg_a_id = InstrumentId::from(format!("LEG-A.{venue}").as_str());
    let leg_b_id = InstrumentId::from(format!("LEG-B.{venue}").as_str());
    let spread_id = InstrumentId::from(format!("(1)LEG-A___((2))LEG-B.{venue}").as_str());
    let activation = UnixNanos::default();
    let expiration = UnixNanos::from(1_000_000_000_000u64);

    if options {
        let mut leg_a = option_contract_appl();
        leg_a.currency = currency;
        leg_a.id = leg_a_id;
        leg_a.raw_symbol = leg_a_id.symbol;
        leg_a.activation_ns = activation;
        leg_a.expiration_ns = expiration;
        leg_a.multiplier = Quantity::from(100);
        let mut leg_b = leg_a.clone();
        leg_b.id = leg_b_id;
        leg_b.raw_symbol = leg_b_id.symbol;
        leg_b.strike_price = Price::from("155.00");
        let mut spread = option_spread();
        spread.currency = currency;
        spread.id = spread_id;
        spread.raw_symbol = Symbol::from("AAPL-CALL-RATIO");
        spread.underlying = leg_a.underlying;
        spread.asset_class = leg_a.asset_class;
        spread.activation_ns = activation;
        spread.expiration_ns = expiration;
        spread.multiplier = leg_a.multiplier;
        (leg_a.into(), leg_b.into(), spread.into())
    } else {
        let mut leg_a = futures_contract_es(Some(activation), Some(expiration));
        leg_a.currency = currency;
        leg_a.id = leg_a_id;
        leg_a.raw_symbol = leg_a_id.symbol;
        leg_a.multiplier = Quantity::from(10);
        let mut leg_b = leg_a.clone();
        leg_b.id = leg_b_id;
        leg_b.raw_symbol = leg_b_id.symbol;
        let mut spread = futures_spread_es();
        spread.currency = currency;
        spread.id = spread_id;
        spread.raw_symbol = Symbol::from("ES-CALENDAR-RATIO");
        spread.activation_ns = activation;
        spread.expiration_ns = expiration;
        spread.multiplier = leg_a.multiplier;
        (leg_a.into(), leg_b.into(), spread.into())
    }
}

fn spread_quote(instrument_id: InstrumentId, bid: &str, ask: &str, size: u64, ts: u64) -> Data {
    Data::Quote(QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from(size),
        Quantity::from(size),
        UnixNanos::from(ts * 1_000_000_000),
        UnixNanos::from(ts * 1_000_000_000),
    ))
}

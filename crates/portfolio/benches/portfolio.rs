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

//! Benchmarks for `Portfolio` query hot paths.
//!
//! Covers the venue-scoped aggregators that a strategy or analyzer would call
//! repeatedly (e.g. once per bar or on a timer): `mark_values`, `equity` on
//! both cash and margin accounts, `unrealized_pnls`, `realized_pnls`, and
//! `net_exposures`. Each is measured at 5 / 20 / 100 open positions.

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{cache::Cache, clock::TestClock};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::AccountAny,
    data::QuoteTick,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{AccountState, OrderFilled, PositionEvent, PositionOpened},
    identifiers::{
        AccountId, ClientOrderId, PositionId, StrategyId, Symbol, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::default_fx_ccy},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_portfolio::Portfolio;

const VENUE: &str = "SIM";
const SIZES: [usize; 3] = [5, 20, 100];

fn venue() -> Venue {
    Venue::from(VENUE)
}

fn make_instruments(n: usize) -> Vec<InstrumentAny> {
    let symbols = [
        "AUD/USD", "GBP/USD", "EUR/USD", "NZD/USD", "USD/CAD", "USD/CHF", "EUR/GBP", "AUD/JPY",
    ];
    (0..n)
        .map(|i| {
            let base = symbols[i % symbols.len()];
            InstrumentAny::CurrencyPair(default_fx_ccy(Symbol::from(base), Some(venue())))
        })
        .collect()
}

fn make_account(account_type: AccountType, account_id: &AccountId) -> AccountState {
    let balances = vec![
        AccountBalance::new(
            Money::new(1_000_000.0, Currency::USD()),
            Money::new(0.0, Currency::USD()),
            Money::new(1_000_000.0, Currency::USD()),
        ),
        AccountBalance::new(
            Money::new(1_000_000.0, Currency::EUR()),
            Money::new(0.0, Currency::EUR()),
            Money::new(1_000_000.0, Currency::EUR()),
        ),
        AccountBalance::new(
            Money::new(1_000_000.0, Currency::GBP()),
            Money::new(0.0, Currency::GBP()),
            Money::new(1_000_000.0, Currency::GBP()),
        ),
    ];
    AccountState::new(
        *account_id,
        account_type,
        balances,
        Vec::new(),
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    )
}

fn make_quote(instrument: &InstrumentAny, bid: f64, ask: f64) -> QuoteTick {
    QuoteTick::new(
        instrument.id(),
        Price::new(bid, 0),
        Price::new(ask, 0),
        Quantity::new(1.0, 0),
        Quantity::new(1.0, 0),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn make_fill(
    instrument: &InstrumentAny,
    account_id: AccountId,
    side: OrderSide,
    quantity: Quantity,
    price: Price,
    position_id: PositionId,
) -> OrderFilled {
    let tag = format!("{position_id}-{}-{quantity}", side.as_ref());
    OrderFilled::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-001"),
        instrument.id(),
        ClientOrderId::new(format!("O-{tag}")),
        VenueOrderId::new(format!("V-{tag}")),
        account_id,
        TradeId::new(format!("T-{tag}")),
        side,
        OrderType::Market,
        quantity,
        price,
        instrument.settlement_currency(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(position_id),
        None,
    )
}

fn make_opened(position: &Position) -> PositionOpened {
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
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    }
}

// Builds a Portfolio pre-populated with `n` open positions at the venue.
// Positions alternate Buy/Sell to exercise both legs of the sign branching in
// `mark_values`. Quotes are cached for every instrument so price lookups succeed.
fn build_portfolio(account_type: AccountType, n: usize) -> Portfolio {
    let mut cache = Cache::new(None, None);
    let clock = TestClock::new();
    let account_id = AccountId::new("SIM-001");

    let instruments = make_instruments(n);
    for instrument in &instruments {
        cache.add_instrument(instrument.clone()).unwrap();
    }

    let state = make_account(account_type, &account_id);
    let account = AccountAny::from_events(&[state.clone()]).unwrap();
    cache.add_account(account).unwrap();

    for instrument in &instruments {
        cache
            .add_quote(make_quote(instrument, 100.0, 101.0))
            .unwrap();
    }

    let mut portfolio = Portfolio::new(
        Rc::new(RefCell::new(cache)),
        Rc::new(RefCell::new(clock)),
        None,
    );

    portfolio.update_account(&state);

    for instrument in &instruments {
        portfolio.update_quote_tick(&make_quote(instrument, 100.0, 101.0));
    }

    for (i, instrument) in instruments.iter().enumerate() {
        let side = if i % 2 == 0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };
        let fill = make_fill(
            instrument,
            account_id,
            side,
            Quantity::from("1"),
            Price::new(100.0, 0),
            PositionId::new(format!("P-{i:05}")),
        );
        let position = Position::new(instrument, fill);
        portfolio
            .cache()
            .borrow_mut()
            .add_position(&position, OmsType::Hedging)
            .unwrap();
        portfolio.update_position(&PositionEvent::PositionOpened(make_opened(&position)));
    }

    portfolio
}

fn bench_mark_values(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/mark_values");
    for n in SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("cash", n), &n, |b, &n| {
            let mut portfolio = build_portfolio(AccountType::Cash, n);
            let venue = venue();
            b.iter(|| {
                black_box(portfolio.mark_values(black_box(&venue), None));
            });
        });
    }
    group.finish();
}

fn bench_equity(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/equity");
    for n in SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("cash", n), &n, |b, &n| {
            let mut portfolio = build_portfolio(AccountType::Cash, n);
            let venue = venue();
            b.iter(|| {
                black_box(portfolio.equity(black_box(&venue), None));
            });
        });
        group.bench_with_input(BenchmarkId::new("margin", n), &n, |b, &n| {
            let mut portfolio = build_portfolio(AccountType::Margin, n);
            let venue = venue();
            // Prime the unrealized-PnL cache so we measure the steady-state path.
            let _ = portfolio.equity(&venue, None);
            b.iter(|| {
                black_box(portfolio.equity(black_box(&venue), None));
            });
        });
    }
    group.finish();
}

fn bench_unrealized_pnls(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/unrealized_pnls");
    for n in SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("margin", n), &n, |b, &n| {
            let mut portfolio = build_portfolio(AccountType::Margin, n);
            let venue = venue();
            let _ = portfolio.unrealized_pnls(&venue, None);
            b.iter(|| {
                black_box(portfolio.unrealized_pnls(black_box(&venue), None));
            });
        });
    }
    group.finish();
}

fn bench_realized_pnls(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/realized_pnls");
    for n in SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("margin", n), &n, |b, &n| {
            let mut portfolio = build_portfolio(AccountType::Margin, n);
            let venue = venue();
            b.iter(|| {
                black_box(portfolio.realized_pnls(black_box(&venue), None));
            });
        });
    }
    group.finish();
}

fn bench_net_exposures(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/net_exposures");
    for n in SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("cash", n), &n, |b, &n| {
            let portfolio = build_portfolio(AccountType::Cash, n);
            let venue = venue();
            b.iter(|| {
                black_box(portfolio.net_exposures(black_box(&venue), None));
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_mark_values,
    bench_equity,
    bench_unrealized_pnls,
    bench_realized_pnls,
    bench_net_exposures,
);
criterion_main!(benches);

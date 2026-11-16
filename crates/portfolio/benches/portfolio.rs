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

//! Benchmarks for `Portfolio` query and position-event hot paths.
//!
//! Covers the venue-scoped aggregators that a strategy or analyzer would call
//! repeatedly (e.g. once per bar or on a timer): `mark_values`, `equity` on
//! both cash and margin accounts, `unrealized_pnls`, `realized_pnls`, and
//! `net_exposures`. Each is measured at 5 / 20 / 100 open positions.
//!
//! Also covers `update_position`, which runs once per position event. One probe
//! position opens, reduces, and closes on an instrument that already holds
//! 10 / 100 / 1,000 HEDGING positions, either all open or all closed, so each
//! event kind shows how its cost grows with the positions it does not touch.

use std::{
    cell::RefCell,
    hint::black_box,
    rc::Rc,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    cache::Cache,
    clock::VirtualClock,
    msgbus::{self, MessageBus},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::{Account, AccountAny},
    data::QuoteTick,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{
        AccountState, OrderFilled, PositionChanged, PositionClosed, PositionEvent, PositionOpened,
    },
    identifiers::{
        AccountId, ClientOrderId, PositionId, StrategyId, Symbol, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::default_fx_ccy},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_portfolio::Portfolio;
use rust_decimal::Decimal;

const VENUE: &str = "SIM";
const SIZES: [usize; 3] = [5, 20, 100];
const EVENT_SIZES: [usize; 3] = [10, 100, 1_000];

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

fn make_account(account_type: AccountType, account_id: AccountId) -> AccountState {
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
        account_id,
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
        last_px: Price::from_decimal_dp(position.avg_px_open, 0).unwrap(),
        currency: position.settlement_currency,
        avg_px_open: position.avg_px_open,
        realized_pnl: position.realized_pnl,
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
    let clock = VirtualClock::new();
    let account_id = AccountId::new("SIM-001");

    let instruments = make_instruments(n);
    for instrument in &instruments {
        cache.add_instrument(instrument.clone()).unwrap();
    }

    let state = make_account(account_type, account_id);
    let account = AccountAny::from_events(std::slice::from_ref(&state)).unwrap();
    cache.add_account(account).unwrap();

    for instrument in &instruments {
        cache
            .add_quote(make_quote(instrument, 100.0, 101.0))
            .unwrap();
    }

    let mut portfolio = Portfolio::new(
        Rc::new(RefCell::new(clock)),
        Rc::new(RefCell::new(cache)),
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

#[derive(Clone, Copy, PartialEq)]
enum ProbeEvent {
    Open,
    Change,
    Close,
}

impl ProbeEvent {
    const ALL: [Self; 3] = [Self::Open, Self::Change, Self::Close];

    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Change => "change",
            Self::Close => "close",
        }
    }

    // Probe quantity left open and probe realized PnL in USD cents after this event
    fn outcome(self) -> (i64, i64) {
        match self {
            Self::Open => (1_000, 0),
            Self::Change => (600, 20),
            Self::Close => (0, 50),
        }
    }
}

// One margin account holding `open` open and `closed` closed HEDGING positions on a single
// instrument, each bought at 1.00000 for 1,000 units, with closed positions sold at 1.00050.
struct EventFixture {
    portfolio: Portfolio,
    instrument: InstrumentAny,
    account_id: AccountId,
    probe_id: PositionId,
    open: usize,
    closed: usize,
}

impl EventFixture {
    fn new(open: usize, closed: usize) -> Self {
        // The message bus keeps each portfolio's handlers, which hold its cache, so a fresh bus
        // releases the fixture a previous sample dropped
        *msgbus::get_message_bus().borrow_mut() = MessageBus::default();

        let mut cache = Cache::new(None, None);
        let account_id = AccountId::new("SIM-001");
        let instrument =
            InstrumentAny::CurrencyPair(default_fx_ccy(Symbol::from("AUD/USD"), Some(venue())));
        cache.add_instrument(instrument.clone()).unwrap();

        let state = make_account(AccountType::Margin, account_id);
        let account = AccountAny::from_events(std::slice::from_ref(&state)).unwrap();
        cache.add_account(account).unwrap();
        cache
            .account_mut(&account_id)
            .unwrap()
            .set_calculate_account_state(true);

        let quote = QuoteTick::new(
            instrument.id(),
            Price::from("1.00100"),
            Price::from("1.00200"),
            Quantity::from("1000000"),
            Quantity::from("1000000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        cache.add_quote(quote).unwrap();

        let mut portfolio = Portfolio::new(
            Rc::new(RefCell::new(VirtualClock::new())),
            Rc::new(RefCell::new(cache)),
            None,
        );

        portfolio.update_account(&state);
        portfolio.update_quote_tick(&quote);

        let mut fixture = Self {
            portfolio,
            instrument,
            account_id,
            probe_id: PositionId::new("P-PROBE"),
            open,
            closed,
        };

        for i in 0..open + closed {
            let position_id = PositionId::new(format!("P-{i:05}"));
            let opened = fixture.cache_opened(position_id, 0);
            fixture.portfolio.update_position(&opened);
            if i >= open {
                let closed = fixture.cache_reduced(position_id, "1000", 0);
                fixture.portfolio.update_position(&closed);
            }
        }

        fixture
    }

    fn cache_probe_event(&self, event: ProbeEvent, ts: u64) -> PositionEvent {
        match event {
            ProbeEvent::Open => self.cache_opened(self.probe_id, ts),
            ProbeEvent::Change => self.cache_reduced(self.probe_id, "400", ts),
            ProbeEvent::Close => self.cache_reduced(self.probe_id, "600", ts),
        }
    }

    // Adds the position to the cache as the execution engine does, without updating the portfolio
    fn cache_opened(&self, position_id: PositionId, ts: u64) -> PositionEvent {
        let fill = self.fill(position_id, OrderSide::Buy, "1000", "1.00000", ts);
        let position = Position::new(&self.instrument, fill.clone());
        self.portfolio
            .cache()
            .borrow_mut()
            .add_position(&position, OmsType::Hedging)
            .unwrap();

        PositionEvent::PositionOpened(PositionOpened::create(
            &position,
            &fill,
            UUID4::new(),
            ts.into(),
        ))
    }

    // Applies a sell fill to the cached position, without updating the portfolio
    fn cache_reduced(&self, position_id: PositionId, quantity: &str, ts: u64) -> PositionEvent {
        let fill = self.fill(position_id, OrderSide::Sell, quantity, "1.00050", ts);
        let position = self
            .portfolio
            .cache()
            .borrow_mut()
            .update_position_from_fill(position_id, &fill)
            .unwrap();

        if position.is_closed() {
            PositionEvent::PositionClosed(PositionClosed::create(
                &position,
                &fill,
                UUID4::new(),
                ts.into(),
            ))
        } else {
            PositionEvent::PositionChanged(PositionChanged::create(
                &position,
                &fill,
                UUID4::new(),
                ts.into(),
            ))
        }
    }

    fn fill(
        &self,
        position_id: PositionId,
        side: OrderSide,
        quantity: &str,
        px: &str,
        ts: u64,
    ) -> OrderFilled {
        let mut fill = make_fill(
            &self.instrument,
            self.account_id,
            side,
            Quantity::from(quantity),
            Price::from(px),
            position_id,
        );
        fill.ts_event = ts.into();
        fill
    }

    // Returns the cache to its state before the probe opened, keeping retained history bounded
    fn purge_probe(&self) {
        let mut cache = self.portfolio.cache().borrow_mut();
        cache.purge_position(self.probe_id, false).unwrap();
        cache
            .purge_account_events(UnixNanos::default(), 0, false)
            .unwrap();
    }

    fn assert_outcome(&mut self, probe: Option<ProbeEvent>) {
        let (probe_qty, probe_realized_cents) = probe.map_or((0, 0), ProbeEvent::outcome);
        let open = i64::try_from(self.open).unwrap();
        let closed = i64::try_from(self.closed).unwrap();
        let usd =
            |cents: i64| Money::from_decimal(Decimal::new(cents, 2), Currency::USD()).unwrap();

        // Longs mark at the 1.00100 bid, 0.1 cents above entry per unit
        let net_qty = 1_000 * open + probe_qty;
        let unrealized = usd(net_qty / 10);
        let realized = usd(50 * closed + probe_realized_cents);
        // 3% maintenance margin on the net quantity at the 1.00000 average entry
        let margin = usd(3 * net_qty);
        let total = usd(100_000_000);

        let instrument_id = self.instrument.id();
        assert_eq!(
            self.portfolio.net_position(&instrument_id),
            Decimal::from(net_qty)
        );
        assert_eq!(
            self.portfolio.unrealized_pnl(&instrument_id),
            Some(unrealized)
        );
        assert_eq!(self.portfolio.realized_pnl(&instrument_id), Some(realized));

        let cache = self.portfolio.cache().borrow();
        let account = cache.account(&self.account_id).unwrap();

        let AccountAny::Margin(margin_account) = &*account else {
            panic!("Expected a margin account");
        };

        let maintenance = margin_account
            .margin(&instrument_id)
            .map(|balance| (balance.initial, balance.maintenance));
        let expected_maintenance = (net_qty > 0).then(|| (usd(0), margin));
        assert_eq!(maintenance, expected_maintenance);

        let balance = margin_account.balance(Some(Currency::USD())).unwrap();
        assert_eq!(
            (balance.total, balance.locked, balance.free),
            (total, margin, total - margin)
        );

        let state = margin_account.last_event().unwrap();
        let state_balance = state
            .balances
            .iter()
            .find(|balance| balance.currency == Currency::USD())
            .unwrap();
        assert_eq!(state_balance, balance);
        assert_eq!(
            state
                .margins
                .iter()
                .map(|balance| (balance.instrument_id, balance.maintenance))
                .collect::<Vec<_>>(),
            expected_maintenance
                .map(|(_, maintenance)| (Some(instrument_id), maintenance))
                .into_iter()
                .collect::<Vec<_>>(),
        );
    }
}

// Checks every event's exact outcome over two probe cycles, so the second proves that purging
// the probe restores the fixture.
fn verify_probe_cycles(open: usize, closed: usize) {
    let mut fixture = EventFixture::new(open, closed);
    fixture.assert_outcome(None);

    for cycle in 1..=2 {
        for event in ProbeEvent::ALL {
            let position_event = fixture.cache_probe_event(event, cycle);
            fixture.portfolio.update_position(&position_event);
            fixture.assert_outcome(Some(event));
        }

        fixture.purge_probe();
    }
}

// Runs `iters` probe cycles and returns the time spent in `update_position` for `timed` events.
// Each cycle uses a new timestamp so every close records a new position cycle.
fn time_probe_cycles(iters: u64, open: usize, closed: usize, timed: ProbeEvent) -> Duration {
    let mut fixture = EventFixture::new(open, closed);
    let mut elapsed = Duration::ZERO;

    for cycle in 1..=iters {
        for event in ProbeEvent::ALL {
            let position_event = fixture.cache_probe_event(event, cycle);

            if event == timed {
                let start = Instant::now();
                fixture
                    .portfolio
                    .update_position(black_box(&position_event));
                elapsed += start.elapsed();
            } else {
                fixture.portfolio.update_position(&position_event);
            }
        }

        fixture.purge_probe();
    }

    elapsed
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
            let _ = portfolio.unrealized_pnls(&venue, None, None);
            b.iter(|| {
                black_box(portfolio.unrealized_pnls(black_box(&venue), None, None));
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
                black_box(portfolio.realized_pnls(black_box(&venue), None, None));
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
                black_box(portfolio.net_exposures(black_box(&venue), None, None));
            });
        });
    }
    group.finish();
}

fn bench_update_position(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/update_position");

    for n in EVENT_SIZES {
        for (positions, open, closed) in [("open_positions", n, 0), ("closed_positions", 0, n)] {
            verify_probe_cycles(open, closed);

            for event in ProbeEvent::ALL {
                let id = BenchmarkId::new(format!("{}/{positions}", event.name()), n);
                group.bench_function(id, |b| {
                    b.iter_custom(|iters| time_probe_cycles(iters, open, closed, event));
                });
            }
        }
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
    bench_update_position,
);
criterion_main!(benches);

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

//! Benchmarks for position fill application and corrected durable replay.
//!
//! Run with `cargo bench -p nautilus-model --bench position_replay_criterion`.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::{OrderFillVoided, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

const HISTORY_COUNTS: [usize; 3] = [8, 64, 512];
const POSITION_ID: &str = "P-BENCH";

fn bench_fill_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("position/fill_apply");

    for count in HISTORY_COUNTS {
        let (position, _) = position_with_fills(count);
        let unique = fill(count, OrderSide::Buy, Quantity::from(1));
        group.bench_with_input(
            BenchmarkId::new("unique", count),
            &(position, unique),
            |b, (position, unique)| {
                b.iter_batched_ref(
                    || position_for_fill(position),
                    |position| {
                        position.apply(black_box(unique));
                        black_box(&*position);
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        let position = position_after_fill_void(count);
        let unique = fill(count, OrderSide::Buy, Quantity::from(1));
        group.bench_with_input(
            BenchmarkId::new("unique_after_fill_void", count),
            &(position, unique),
            |b, (position, unique)| {
                b.iter_batched_ref(
                    || position_for_fill(position),
                    |position| {
                        position.apply(black_box(unique));
                        black_box(&*position);
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        let (position, first, last) = position_with_prior_cycle(count);
        group.bench_with_input(
            BenchmarkId::new("historical_duplicate_first", count),
            &(position.clone(), first),
            |b, (position, first)| {
                b.iter_batched_ref(
                    || position.clone(),
                    |position| {
                        position.apply(black_box(first));
                        black_box(&*position);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("historical_duplicate_last", count),
            &(position, last),
            |b, (position, last)| {
                b.iter_batched_ref(
                    || position.clone(),
                    |position| {
                        position.apply(black_box(last));
                        black_box(&*position);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_requires_replay_state(c: &mut Criterion) {
    let mut group = c.benchmark_group("position/requires_replay_state");

    for count in HISTORY_COUNTS {
        let (position, _) = position_with_fills(count);
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &position,
            |b, position| {
                b.iter(|| black_box(position.requires_replay_state()));
            },
        );
    }

    group.finish();
}

fn bench_fill_void_replay(c: &mut Criterion) {
    let mut group = c.benchmark_group("position/fill_void_replay");

    for count in HISTORY_COUNTS {
        let (position, first) = position_with_fills(count);
        let fill_voided = fill_void(&first);
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &(position, fill_voided),
            |b, (position, fill_voided)| {
                b.iter_batched_ref(
                    || (position.clone(), Some(fill_voided.clone())),
                    |(position, fill_voided)| {
                        let result = position
                            .apply_fill_void(fill_voided.take().unwrap(), Quantity::from(1), None)
                            .unwrap();
                        black_box(result);
                        black_box(&*position);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_accounting(c: &mut Criterion) {
    let (linear, _) = position_with_fills(8);
    let last = Price::from("1.12500");
    assert_eq!(
        linear.try_unrealized_pnl(last).unwrap(),
        Money::from("1 USD")
    );
    c.bench_function("position/accounting/linear_pnl", |b| {
        b.iter(|| black_box(linear.try_unrealized_pnl(black_box(last)).unwrap()));
    });

    let instrument = instrument_inverse();
    let mut opening = fill(0, OrderSide::Buy, Quantity::from(100_000));
    opening.instrument_id = instrument.id();
    opening.last_px = Price::from("10000.0");
    opening.currency = Currency::BTC();
    let inverse = Position::new(&instrument, opening);
    let last = Price::from("20000.0");
    assert_eq!(
        inverse.try_unrealized_pnl(last).unwrap(),
        Money::from("5 BTC")
    );
    c.bench_function("position/accounting/inverse_pnl", |b| {
        b.iter(|| black_box(inverse.try_unrealized_pnl(black_box(last)).unwrap()));
    });

    let mut reversal = fill(9, OrderSide::Sell, Quantity::from(2));
    reversal.commission = Some(Money::from("0.02 USD"));
    let event_id = UUID4::new();
    let (closing, opening) = reversal
        .split_for_position_flip(Quantity::from(1), None, event_id)
        .unwrap();
    assert_eq!(closing.commission, Some(Money::from("0.01 USD")));
    assert_eq!(opening.commission, Some(Money::from("0.01 USD")));
    c.bench_function("position/accounting/fee_split", |b| {
        b.iter(|| {
            black_box(
                reversal
                    .split_for_position_flip(black_box(Quantity::from(1)), None, event_id)
                    .unwrap(),
            )
        });
    });

    let price = Price::from("1.23456");
    let quantity = Quantity::from("1.23456789");
    let fee = Money::from_str_exact("0.000001 USD").unwrap();
    c.bench_function("position/accounting/domain_conversion", |b| {
        b.iter(|| {
            black_box((
                black_box(price).as_decimal(),
                black_box(quantity).as_decimal(),
                black_box(fee).as_decimal(),
            ))
        });
    });
}

fn position_for_fill(position: &Position) -> Position {
    let mut position = position.clone();
    position.events.reserve(1);
    position.replay_events.reserve(1);
    position.trade_ids.reserve(1);
    position
}

fn position_after_fill_void(count: usize) -> Position {
    let (mut position, _) = position_with_fills(count);
    let last = position.events.last().unwrap().clone();
    position
        .apply_fill_void(fill_void(&last), Quantity::from(1), None)
        .unwrap();
    position
}

fn position_with_prior_cycle(count: usize) -> (Position, OrderFilled, OrderFilled) {
    let (mut position, mut first) = position_with_fills(count);
    let mut last = position.events.last().unwrap().clone();
    position.apply(&fill(count, OrderSide::Sell, Quantity::from(count as u64)));
    position.apply(&fill(count + 1, OrderSide::Buy, Quantity::from(1)));
    first.event_id = UUID4::new();
    first.ts_event = UnixNanos::from((count + 2) as u64);
    last.event_id = UUID4::new();
    last.ts_event = UnixNanos::from((count + 3) as u64);
    (position, first, last)
}

fn position_with_fills(count: usize) -> (Position, OrderFilled) {
    let instrument = instrument();
    let opening = fill(0, OrderSide::Buy, Quantity::from(1));
    let first = opening.clone();
    let mut position = Position::new(&instrument, opening);

    for index in 1..count {
        position.apply(&fill(index, OrderSide::Buy, Quantity::from(1)));
    }

    (position, first)
}

fn instrument() -> InstrumentAny {
    InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("AUD/USD.SIM"))
            .raw_symbol(Symbol::from("AUD/USD"))
            .base_currency(Currency::AUD())
            .quote_currency(Currency::USD())
            .price_precision(5)
            .size_precision(0)
            .price_increment(Price::from("0.00001"))
            .size_increment(Quantity::from(1))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

fn instrument_inverse() -> InstrumentAny {
    let instrument = CryptoPerpetual::builder()
        .instrument_id(InstrumentId::from("BTCUSD.BYBIT"))
        .raw_symbol(Symbol::from("BTCUSD"))
        .base_currency(Currency::BTC())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::BTC())
        .is_inverse(true)
        .price_precision(1)
        .size_precision(0)
        .price_increment(Price::from("0.5"))
        .size_increment(Quantity::from("1"))
        .max_notional(Money::from("10000000 USD"))
        .min_notional(Money::from("1 USD"))
        .max_price(Price::from("10000000"))
        .min_price(Price::from("0.01"))
        .margin_init(dec!(0.01))
        .margin_maint(dec!(0.0035))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    InstrumentAny::CryptoPerpetual(instrument)
}

fn fill(index: usize, side: OrderSide, quantity: Quantity) -> OrderFilled {
    OrderFilled::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from(format!("O-{index}").as_str()),
        VenueOrderId::from(format!("V-{index}").as_str()),
        AccountId::from("SIM-001"),
        TradeId::from(format!("T-{index}").as_str()),
        side,
        OrderType::Market,
        quantity,
        Price::from("1.00000"),
        Currency::USD(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::from(index as u64),
        UnixNanos::from(index as u64),
        false,
        Some(PositionId::from(POSITION_ID)),
        None,
        None,
    )
}

fn fill_void(fill: &OrderFilled) -> OrderFillVoided {
    OrderFillVoided::new(
        fill.trader_id,
        fill.strategy_id,
        fill.instrument_id,
        fill.client_order_id,
        fill.venue_order_id,
        fill.account_id,
        Ustr::from("CORRECTION-1"),
        fill.trade_id,
        Quantity::from(1),
        None,
        fill.order_side,
        fill.order_type,
        fill.last_px,
        fill.currency,
        fill.liquidity_side,
        fill.position_id,
        None,
        None,
        UUID4::new(),
        fill.ts_event,
        fill.ts_init,
        true,
        false,
    )
}

criterion_group!(
    benches,
    bench_fill_apply,
    bench_accounting,
    bench_requires_replay_state,
    bench_fill_void_replay
);
criterion_main!(benches);

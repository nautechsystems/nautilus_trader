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

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::{OrderFillVoided, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{CurrencyPair, InstrumentAny},
    position::Position,
    types::{Currency, Price, Quantity},
};
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
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        InstrumentId::from("AUD/USD.SIM"),
        Symbol::from("AUD/USD"),
        Currency::AUD(),
        Currency::USD(),
        5,
        0,
        Price::from("0.00001"),
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
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

criterion_group!(benches, bench_fill_apply, bench_fill_void_replay);
criterion_main!(benches);

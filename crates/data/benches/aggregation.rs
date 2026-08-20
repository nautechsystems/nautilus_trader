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

//! Benchmarks for the side-aware value bar aggregators.
//!
//! Targets the notional-splitting hot path of [`ValueImbalanceBarAggregator`] and
//! [`ValueRunsBarAggregator`]: each trade accumulates a signed/absolute notional and repeatedly
//! splits across the step threshold, constructing a representable chunk quantity per emitted bar.
//! The workload mixes aggressor sides so the imbalance neutralization and run side-reset branches
//! are both exercised.
//!
//! Run with `cargo bench -p nautilus-data --bench aggregation`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_data::aggregation::{
    BarAggregator, ValueImbalanceBarAggregator, ValueRunsBarAggregator,
};
use nautilus_model::{
    data::{BarSpecification, BarType, trade::TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

const PRICE_PRECISION: u8 = 5;
const SIZE_PRECISION: u8 = 3;
const STEP: usize = 10;
const N_TRADES: u64 = 2_048;

fn bar_type(aggregation: BarAggregation) -> BarType {
    BarType::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        BarSpecification::new(STEP, aggregation, PriceType::Last),
        AggregationSource::Internal,
    )
}

fn build_trades() -> Vec<TradeTick> {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    // Fractional sizes so `value_needed / price` rounds to a representable chunk per split.
    let sizes = ["3.333", "5.500", "7.250", "4.125", "6.000", "2.750"];
    (0..N_TRADES)
        .map(|i| {
            // Flip aggressor every four trades: builds same-side runs that split, then neutralizes.
            let aggressor_side = if (i / 4) % 2 == 0 {
                AggressorSide::Buy
            } else {
                AggressorSide::Sell
            };
            TradeTick {
                instrument_id,
                price: Price::from("2.00000"),
                size: Quantity::from(sizes[(i as usize) % sizes.len()]),
                aggressor_side,
                trade_id: TradeId::from("123456"),
                ts_event: i.into(),
                ts_init: i.into(),
            }
        })
        .collect()
}

fn bench_value_side_aggregation(c: &mut Criterion) {
    let trades = build_trades();

    let mut group = c.benchmark_group("value_side_aggregation");
    group.throughput(Throughput::Elements(N_TRADES));

    group.bench_function("value_imbalance", |b| {
        b.iter_batched_ref(
            || {
                ValueImbalanceBarAggregator::new(
                    bar_type(BarAggregation::ValueImbalance),
                    PRICE_PRECISION,
                    SIZE_PRECISION,
                    |bar| {
                        black_box(bar);
                    },
                )
            },
            |aggregator| {
                for trade in &trades {
                    aggregator.handle_trade(black_box(*trade));
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("value_runs", |b| {
        b.iter_batched_ref(
            || {
                ValueRunsBarAggregator::new(
                    bar_type(BarAggregation::ValueRuns),
                    PRICE_PRECISION,
                    SIZE_PRECISION,
                    |bar| {
                        black_box(bar);
                    },
                )
            },
            |aggregator| {
                for trade in &trades {
                    aggregator.handle_trade(black_box(*trade));
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_value_side_aggregation);
criterion_main!(benches);

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

//! Benchmarks for bar and spread quote aggregation hot paths.
//!
//! The bar benchmarks cover the per-update core path, volume splitting, Renko no-brick and brick
//! emission paths, and side-aware value splitting. The spread benchmark covers quote-driven option
//! pricing with vegas, including leg price and size combination. Each case measures a public
//! ingestion method with reusable inputs and excludes aggregator construction from the timed region.
//!
//! Run with `cargo bench -p nautilus-data --bench aggregation`.

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::clock::TestClock;
use nautilus_core::UnixNanos;
use nautilus_data::aggregation::{
    BarAggregator, MapVegaProvider, RenkoBarAggregator, SpreadQuoteAggregator, TickBarAggregator,
    ValueImbalanceBarAggregator, ValueRunsBarAggregator, VolumeBarAggregator,
};
use nautilus_model::{
    data::{BarSpecification, BarType, QuoteTick, trade::TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

const PRICE_PRECISION: u8 = 5;
const SIZE_PRECISION: u8 = 3;
const N_UPDATES: u64 = 2_048;

fn bar_type(aggregation: BarAggregation, step: usize) -> BarType {
    BarType::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        BarSpecification::new(step, aggregation, PriceType::Last),
        AggregationSource::Internal,
    )
}

fn build_trades() -> Vec<TradeTick> {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    // Fractional sizes so `value_needed / price` rounds to a representable chunk per split.
    let sizes = ["3.333", "5.500", "7.250", "4.125", "6.000", "2.750"];
    (0..N_UPDATES)
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

fn build_timestamps() -> Vec<UnixNanos> {
    (0..N_UPDATES).map(UnixNanos::from).collect()
}

fn build_renko_updates(alternate_price: Price) -> Vec<(Price, UnixNanos)> {
    (0..N_UPDATES)
        .map(|i| {
            let price = if i.is_multiple_of(2) {
                Price::from("100.00")
            } else {
                alternate_price
            };
            (price, UnixNanos::from(i))
        })
        .collect()
}

fn quote_tick(instrument_id: InstrumentId, ts_init: UnixNanos) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from("100.00"),
        Price::from("100.05"),
        Quantity::from(100),
        Quantity::from(120),
        ts_init,
        ts_init,
    )
}

fn build_spread_quotes() -> Vec<QuoteTick> {
    let leg_id = InstrumentId::from("AAPL.XNAS");
    (0..N_UPDATES)
        .map(|i| quote_tick(leg_id, UnixNanos::from(i + 2)))
        .collect()
}

fn build_option_spread() -> SpreadQuoteAggregator {
    let leg1 = InstrumentId::from("AAPL.XNAS");
    let leg2 = InstrumentId::from("MSFT.XNAS");
    let spread_id = InstrumentId::from("AAPL-MSFT.SYNTH");
    let legs = [(leg1, 1), (leg2, -1)];
    let mut vega_provider = MapVegaProvider::new();
    vega_provider.insert(leg1, 0.15);
    vega_provider.insert(leg2, 0.12);

    let mut aggregator = SpreadQuoteAggregator::new(
        spread_id,
        &legs,
        false,
        2,
        0,
        Box::new(|quote| {
            black_box(quote);
        }),
        Rc::new(RefCell::new(TestClock::new())),
        false,
        None,
        0,
        false,
        10,
        Some(Box::new(vega_provider)),
        None,
    );
    aggregator.handle_quote_tick(quote_tick(leg1, UnixNanos::from(0)));
    aggregator.handle_quote_tick(quote_tick(leg2, UnixNanos::from(1)));
    aggregator
}

fn bench_bar_update(c: &mut Criterion) {
    let timestamps = build_timestamps();
    let renko_no_brick_updates = build_renko_updates(Price::from("100.09"));
    let renko_single_brick_updates = build_renko_updates(Price::from("100.10"));
    let renko_multi_brick_updates = build_renko_updates(Price::from("100.50"));
    let tick_price = Price::from("100.00000");
    let tick_size = Quantity::from("1.000");
    let volume_price = Price::from("100.00000");
    let volume_size = Quantity::from("250.000");
    let renko_size = Quantity::from(1);

    let mut group = c.benchmark_group("bar_aggregation");
    group.throughput(Throughput::Elements(N_UPDATES));

    group.bench_function("tick_update", |b| {
        b.iter_batched_ref(
            || {
                TickBarAggregator::new(
                    bar_type(BarAggregation::Tick, 100),
                    PRICE_PRECISION,
                    SIZE_PRECISION,
                    |bar| {
                        black_box(bar);
                    },
                )
            },
            |aggregator| {
                for ts_init in &timestamps {
                    aggregator.update(
                        black_box(tick_price),
                        black_box(tick_size),
                        black_box(*ts_init),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("volume_split", |b| {
        b.iter_batched_ref(
            || {
                VolumeBarAggregator::new(
                    bar_type(BarAggregation::Volume, 100),
                    PRICE_PRECISION,
                    SIZE_PRECISION,
                    |bar| {
                        black_box(bar);
                    },
                )
            },
            |aggregator| {
                for ts_init in &timestamps {
                    aggregator.update(
                        black_box(volume_price),
                        black_box(volume_size),
                        black_box(*ts_init),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    bench_renko_case(
        &mut group,
        "renko_no_brick",
        &renko_no_brick_updates,
        renko_size,
    );
    bench_renko_case(
        &mut group,
        "renko_single_brick",
        &renko_single_brick_updates,
        renko_size,
    );
    bench_renko_case(
        &mut group,
        "renko_multi_brick",
        &renko_multi_brick_updates,
        renko_size,
    );

    group.finish();
}

fn bench_renko_case(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    updates: &[(Price, UnixNanos)],
    size: Quantity,
) {
    group.bench_function(name, |b| {
        b.iter_batched_ref(
            || {
                RenkoBarAggregator::new(
                    bar_type(BarAggregation::Renko, 10),
                    2,
                    0,
                    Price::from("0.01"),
                    |bar| {
                        black_box(bar);
                    },
                )
            },
            |aggregator| {
                for (price, ts_init) in updates {
                    aggregator.update(black_box(*price), black_box(size), black_box(*ts_init));
                }
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_value_side_aggregation(c: &mut Criterion) {
    let trades = build_trades();

    let mut group = c.benchmark_group("value_side_aggregation");
    group.throughput(Throughput::Elements(N_UPDATES));

    group.bench_function("value_imbalance", |b| {
        b.iter_batched_ref(
            || {
                ValueImbalanceBarAggregator::new(
                    bar_type(BarAggregation::ValueImbalance, 10),
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
                    bar_type(BarAggregation::ValueRuns, 10),
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

fn bench_spread_quote_aggregation(c: &mut Criterion) {
    let quotes = build_spread_quotes();

    let mut group = c.benchmark_group("spread_quote_aggregation");
    group.throughput(Throughput::Elements(N_UPDATES));

    group.bench_function("option_vega", |b| {
        b.iter_batched_ref(
            build_option_spread,
            |aggregator| {
                for quote in &quotes {
                    aggregator.handle_quote_tick(black_box(*quote));
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_bar_update,
    bench_value_side_aggregation,
    bench_spread_quote_aggregation,
);
criterion_main!(benches);

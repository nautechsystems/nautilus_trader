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

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_model::{
    data::BookOrder,
    enums::{BookType, OrderSide},
    identifiers::InstrumentId,
    orderbook::OrderBook,
    types::{Price, Quantity},
};

fn bench_book_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_analysis");

    for depth in [10_u64, 100] {
        let mut book = OrderBook::new(InstrumentId::from("AAPL.XNAS"), BookType::L3_MBO);

        for level in 0..depth {
            for order in 0..4 {
                book.add(
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from_mantissa_exponent(10_000 + level as i64, -2, 2),
                        Quantity::from("2.50"),
                        level * 4 + order,
                    ),
                    0,
                    0,
                    0.into(),
                );
            }
        }

        let quantity = Quantity::from(depth * 5);
        group.bench_with_input(
            BenchmarkId::new("average_price", depth),
            &quantity,
            |b, qty| {
                b.iter(|| {
                    black_box(&book).get_avg_px_for_quantity(black_box(*qty), OrderSide::Buy)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("worst_price", depth),
            &quantity,
            |b, qty| {
                b.iter(|| {
                    black_box(&book).get_worst_px_for_quantity(black_box(*qty), OrderSide::Buy)
                });
            },
        );

        let exposure = Quantity::from(depth * 500);
        group.bench_with_input(BenchmarkId::new("exposure", depth), &exposure, |b, qty| {
            b.iter(|| {
                black_box(&book).get_avg_px_qty_for_exposure(black_box(*qty), OrderSide::Buy)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_book_analysis);
criterion_main!(benches);

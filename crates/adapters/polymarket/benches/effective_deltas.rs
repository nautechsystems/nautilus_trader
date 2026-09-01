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

//! Effective order book snapshot benches.
//!
//! Each case measures the state transition and emitted domain batch. Criterion clones the seeded
//! book outside the timed region so the result reflects production work rather than fixture reset.

#[path = "../src/data/effective_deltas.rs"]
mod effective_deltas;

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use effective_deltas::apply_snapshot_and_diff;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

const DEPTHS: &[usize] = &[10, 100];
const PRICE_PRECISION: u8 = 4;
const SIZE_PRECISION: u8 = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapshotChange {
    None,
    Resize,
    Replace,
}

fn bench_snapshots(c: &mut Criterion) {
    let instrument_id = InstrumentId::from("EFFECTIVE.POLYMARKET");
    let mut group = c.benchmark_group("effective_deltas/snapshot");

    for &depth in DEPTHS {
        let seed = make_snapshot(instrument_id, depth, SnapshotChange::None);
        let resized = make_snapshot(instrument_id, depth, SnapshotChange::Resize);
        let replaced = make_snapshot(instrument_id, depth, SnapshotChange::Replace);
        let book = seeded_book(instrument_id, &seed);

        group.throughput(Throughput::Elements((depth * 2) as u64));
        group.bench_with_input(BenchmarkId::new("unchanged", depth), &depth, |b, _| {
            b.iter_batched_ref(
                || book.clone(),
                |book| black_box(apply_snapshot_and_diff(book, black_box(&seed)).unwrap()),
                BatchSize::SmallInput,
            );
        });
        group.bench_with_input(
            BenchmarkId::new("ten_percent_resized", depth),
            &depth,
            |b, _| {
                b.iter_batched_ref(
                    || book.clone(),
                    |book| black_box(apply_snapshot_and_diff(book, black_box(&resized)).unwrap()),
                    BatchSize::SmallInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("ten_percent_replaced", depth),
            &depth,
            |b, _| {
                b.iter_batched_ref(
                    || book.clone(),
                    |book| black_box(apply_snapshot_and_diff(book, black_box(&replaced)).unwrap()),
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn seeded_book(instrument_id: InstrumentId, snapshot: &OrderBookDeltas) -> OrderBook {
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    book.apply_deltas(snapshot).unwrap();
    book
}

fn make_snapshot(
    instrument_id: InstrumentId,
    depth: usize,
    change: SnapshotChange,
) -> OrderBookDeltas {
    let ts = UnixNanos::from(1_700_000_000_000_000_000_u64);
    let snapshot = RecordFlag::F_SNAPSHOT as u8;
    let mut deltas = Vec::with_capacity(depth * 2 + 1);
    deltas.push(OrderBookDelta::clear(instrument_id, 0, ts, ts));

    for index in 0..depth * 2 {
        let side = if index < depth {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };
        let offset = i64::try_from(index % depth).unwrap();
        let replace = change == SnapshotChange::Replace && index % 10 == 0;
        let price_mantissa = match (side, replace) {
            (OrderSide::Buy, false) => 5_000 - offset,
            (OrderSide::Buy, true) => 4_500 - offset,
            (OrderSide::Sell, false) => 5_001 + offset,
            (OrderSide::Sell, true) => 5_500 + offset,
        };
        let size_mantissa = if change == SnapshotChange::Resize && index % 10 == 0 {
            1_001 + offset
        } else {
            1_000 + offset
        };
        let flags = if index == depth * 2 - 1 {
            snapshot | RecordFlag::F_LAST as u8
        } else {
            snapshot
        };

        deltas.push(delta(
            instrument_id,
            BookAction::Add,
            side,
            price_mantissa,
            size_mantissa,
            flags,
            ts,
        ));
    }

    OrderBookDeltas::new(instrument_id, deltas)
}

fn delta(
    instrument_id: InstrumentId,
    action: BookAction,
    side: OrderSide,
    price_mantissa: i64,
    size_mantissa: i64,
    flags: u8,
    ts: UnixNanos,
) -> OrderBookDelta {
    let price = Price::from_decimal_dp(
        Decimal::new(price_mantissa, u32::from(PRICE_PRECISION)),
        PRICE_PRECISION,
    )
    .unwrap();
    let size = Quantity::from_decimal_dp(
        Decimal::new(size_mantissa, u32::from(SIZE_PRECISION)),
        SIZE_PRECISION,
    )
    .unwrap();
    let order = BookOrder::new(side, price, size, 0);

    OrderBookDelta::new(instrument_id, action, order, flags, 0, ts, ts)
}

criterion_group!(benches, bench_snapshots);
criterion_main!(benches);

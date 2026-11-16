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

//! Canonical inbound benches: one WebSocket frame to a Nautilus domain value.
//!
//! Quotes and trades measure frame decode plus the data client's parse. Book rows measure frame
//! decode plus `BookSyncTracker::event`. Instrument setup and subscription confirmation stay
//! outside the timed region. Book rows use `iter_batched_ref`, so dropping the prepared tracker
//! stays outside the timed region too.

mod common;

use std::hint::black_box;

use common::{BookReady, decode_quote, decode_trade, live_instrument, quote_frame, trade_frame};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

fn bench_quotes(c: &mut Criterion) {
    let frame = quote_frame();
    let instrument = live_instrument();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("quotes", |b| {
        b.iter(|| {
            let tick = decode_quote(black_box(&frame), &instrument).unwrap();
            black_box(tick);
        });
    });

    group.finish();
}

fn bench_trades(c: &mut Criterion) {
    let frame = trade_frame();
    let instrument = live_instrument();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trades", |b| {
        b.iter(|| {
            let tick = decode_trade(black_box(&frame), &instrument).unwrap();
            black_box(tick);
        });
    });

    group.finish();
}

fn bench_book_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_snapshot", |b| {
        b.iter_batched_ref(
            || {
                let ready = BookReady::new();
                let frame = ready.snapshot_frame().to_vec();
                (ready, frame)
            },
            |(ready, frame)| {
                ready.apply_frame(frame);
                black_box(ready);
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_book_deltas(c: &mut Criterion) {
    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_deltas", |b| {
        b.iter_batched_ref(
            || {
                let ready = BookReady::new();
                let snapshot = ready.snapshot_frame().to_vec();
                ready.apply_frame(&snapshot);
                let frame = ready.delta_frame().to_vec();
                (ready, frame)
            },
            |(ready, frame)| {
                ready.apply_frame(frame);
                black_box(ready);
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    data,
    bench_quotes,
    bench_trades,
    bench_book_snapshot,
    bench_book_deltas
);
criterion_main!(data);

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

//! Decoder-only benches for inbound frames.
//!
//! Quote and trade decode dominate those pipeline rows, so these rows isolate that cost. Book-delta
//! decode is about half of its pipeline row once fixture teardown is excluded; the row shows
//! whether a book regression is in the decoder. Parse and request construction stay inside the
//! canonical pipeline benches.

mod common;

use std::hint::black_box;

use common::{BookReady, quote_frame, trade_frame};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_kalshi::websocket::parse::decode_frame;

fn bench_decode(c: &mut Criterion, name: &str, frame: &[u8]) {
    let mut group = c.benchmark_group("decode_only");
    group.throughput(Throughput::Elements(1));
    group.bench_function(name, |b| {
        b.iter(|| {
            let decoded = decode_frame(black_box(frame)).unwrap();
            black_box(decoded);
        });
    });

    group.finish();
}

fn bench_ticker(c: &mut Criterion) {
    bench_decode(c, "ticker", &quote_frame());
}

fn bench_trade(c: &mut Criterion) {
    bench_decode(c, "trade", &trade_frame());
}

fn bench_book_delta(c: &mut Criterion) {
    let ready = BookReady::new();
    bench_decode(c, "book_delta", ready.delta_frame());
}

criterion_group!(micros, bench_ticker, bench_trade, bench_book_delta);
criterion_main!(micros);

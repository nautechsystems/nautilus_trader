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

//! Inbound pipeline: raw WebSocket text frame to Nautilus domain data.
//!
//! Each iteration decodes the frame, looks up the instrument by `symbol_id`, and converts the
//! message the way the data client does, including book sequence tracking. No I/O, async runtime,
//! or channel send.

mod common;

use std::hint::black_box;

use common::{
    BITSTAMP_BTC_USD, COINBASE_BCH_USD, fixtures, instruments, levels_snapshot, synced_tracker,
};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_coinapi::{
    book::sync::{BookFrame, BookSyncTracker},
    websocket::{
        messages::{BookL3, CoinApiWsMessage, DataType, Frame},
        parse::{parse_bar, parse_book_depth, parse_quote_tick, parse_trade_tick},
    },
};
use nautilus_common::live::dst::time::Instant;
use nautilus_core::UnixNanos;
use nautilus_model::instruments::Instrument;

fn decode(bytes: &[u8]) -> CoinApiWsMessage {
    match Frame::decode(bytes).unwrap() {
        Frame::Data(message) => message,
        other => panic!("expected a data frame, was {other:?}"),
    }
}

fn bench_book(c: &mut Criterion) {
    let instruments = instruments();
    let instrument = instruments.get_cloned(&BITSTAMP_BTC_USD.into()).unwrap();
    let update = fixtures::book_update();
    let snapshot = levels_snapshot(&update);
    let now = Instant::now();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book", |b| {
        b.iter_batched_ref(
            || {
                synced_tracker(
                    &instrument,
                    BITSTAMP_BTC_USD,
                    DataType::Book,
                    BookFrame::Levels(&snapshot),
                )
            },
            |tracker: &mut BookSyncTracker| {
                let CoinApiWsMessage::Book(book) = decode(black_box(update.as_bytes())) else {
                    unreachable!()
                };

                let instrument = instruments.get_cloned(&book.symbol_id).unwrap();
                let (_, deltas) =
                    tracker.apply(instrument.id(), BookFrame::Levels(&book), &instrument, now);
                black_box(deltas.unwrap());
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_book_l3(c: &mut Criterion) {
    let instruments = instruments();
    let instrument = instruments.get_cloned(&COINBASE_BCH_USD.into()).unwrap();
    let snapshot: BookL3 = serde_json::from_str(fixtures::BOOK_L3_SNAPSHOT).unwrap();
    let now = Instant::now();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_l3", |b| {
        b.iter_batched_ref(
            || {
                synced_tracker(
                    &instrument,
                    COINBASE_BCH_USD,
                    DataType::BookL3,
                    BookFrame::Orders(&snapshot),
                )
            },
            |tracker: &mut BookSyncTracker| {
                let CoinApiWsMessage::BookL3(book) =
                    decode(black_box(fixtures::BOOK_L3_UPDATE.as_bytes()))
                else {
                    unreachable!()
                };

                let instrument = instruments.get_cloned(&book.symbol_id).unwrap();
                let (_, deltas) =
                    tracker.apply(instrument.id(), BookFrame::Orders(&book), &instrument, now);
                black_box(deltas.unwrap());
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_book20(c: &mut Criterion) {
    let instruments = instruments();
    let tracker = BookSyncTracker::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book20", |b| {
        b.iter(|| {
            let CoinApiWsMessage::Book(book) = decode(black_box(fixtures::BOOK20.as_bytes()))
            else {
                unreachable!()
            };

            let instrument = instruments.get_cloned(&book.symbol_id).unwrap();
            let mut depth = parse_book_depth(&book, 20, &instrument).unwrap();
            depth.sequence = tracker.depth_sequence(instrument.id(), book.sequence);
            black_box(depth);
        });
    });

    group.finish();
}

fn bench_quote(c: &mut Criterion) {
    let instruments = instruments();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("quote", |b| {
        b.iter(|| {
            let CoinApiWsMessage::Quote(quote) = decode(black_box(fixtures::QUOTE.as_bytes()))
            else {
                unreachable!()
            };

            let instrument = instruments.get_cloned(&quote.symbol_id).unwrap();
            black_box(parse_quote_tick(&quote, &instrument).unwrap());
        });
    });

    group.finish();
}

fn bench_trade(c: &mut Criterion) {
    let instruments = instruments();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trade", |b| {
        b.iter(|| {
            let CoinApiWsMessage::Trade(trade) = decode(black_box(fixtures::TRADE.as_bytes()))
            else {
                unreachable!()
            };

            let instrument = instruments.get_cloned(&trade.symbol_id).unwrap();
            black_box(parse_trade_tick(&trade, &instrument).unwrap());
        });
    });

    group.finish();
}

// Excludes the data client's period-close cache, which retains each update until a later period
// closes it
fn bench_ohlcv(c: &mut Criterion) {
    let instruments = instruments();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("ohlcv", |b| {
        b.iter(|| {
            let CoinApiWsMessage::Ohlcv(ohlcv) = decode(black_box(fixtures::OHLCV.as_bytes()))
            else {
                unreachable!()
            };

            let instrument = instruments.get_cloned(&ohlcv.symbol_id).unwrap();
            black_box(parse_bar(&ohlcv, &instrument, ts_init).unwrap());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_book,
    bench_book_l3,
    bench_book20,
    bench_quote,
    bench_trade,
    bench_ohlcv,
);
criterion_main!(benches);

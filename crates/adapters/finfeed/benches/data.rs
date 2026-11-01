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

//! REST decoding and checked conversion across batch sizes and book depths.
//! Network pacing, scheduling, deduplication, and publication are outside the timed region.

#[path = "../src/data/parse.rs"]
mod parse;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_finfeed::{
    common,
    http::{
        self,
        models::{ActivityBatch, Book, Ohlcv, OhlcvPeriod, Quote, Trade},
    },
};
use nautilus_model::{
    data::{Bar, BarType, QuoteTick, TradeTick},
    enums::{AggressorSide, BookType},
    instruments::BinaryOption,
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::de::DeserializeOwned;
use serde_json::{Value, json, value::RawValue};

const ACTIVITY: &str = include_str!("../test_data/poll-activity.json");
const BOOK: &str = include_str!("../test_data/polymarket-book.json");
const BARS: &str = include_str!("../test_data/client-bars.json");
const HISTORY_SIZES: &[usize] = &[1, 100, 10_000];

fn bench_activity(c: &mut Criterion) {
    let instrument = instrument();
    let template: Value = serde_json::from_str(ACTIVITY).unwrap();
    let mut group = c.benchmark_group("activity");

    for count in [1, 10, 100] {
        let quotes = repeat_rows(&template["quotes"].to_string(), count);
        let trades = repeat_rows(&template["trades"].to_string(), count);
        let payload = format!("{{\"quotes\":{quotes},\"trades\":{trades}}}");
        let source: ActivityBatch = serde_json::from_str(&payload).unwrap();
        assert_eq!(source.quotes.as_ref().unwrap().len(), count);
        assert_eq!(source.trades.as_ref().unwrap().len(), count);

        let mut quotes = source.quotes.as_ref().unwrap().iter();
        let mut trades = source.trades.as_ref().unwrap().iter();
        activity(
            &source,
            &instrument,
            |actual| assert_quote(quotes.next().unwrap(), &actual, &instrument),
            |actual| assert_trade(trades.next().unwrap(), &actual, &instrument),
        );
        assert_eq!(quotes.count(), 0);
        assert_eq!(trades.count(), 0);

        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("decode_convert", count),
            &payload,
            |b, payload| {
                b.iter(|| {
                    let source = serde_json::from_slice(black_box(payload.as_bytes())).unwrap();
                    activity(
                        &source,
                        &instrument,
                        |value| {
                            black_box(value);
                        },
                        |value| {
                            black_box(value);
                        },
                    );
                });
            },
        );

        group.throughput(Throughput::Elements((2 * count) as u64));
        group.bench_with_input(BenchmarkId::new("convert", count), &source, |b, source| {
            b.iter(|| {
                activity(
                    black_box(source),
                    &instrument,
                    |value| {
                        black_box(value);
                    },
                    |value| {
                        black_box(value);
                    },
                );
            });
        });
    }

    group.finish();
}

fn bench_history(c: &mut Criterion) {
    let instrument = instrument();
    bench_records::<Quote, _>(
        c,
        "history/quotes",
        include_str!("../test_data/client-quotes.json"),
        |source| parse::quote(source, &instrument).unwrap(),
        |source, actual| assert_quote(source, actual, &instrument),
    );
    bench_records::<Trade, _>(
        c,
        "history/trades",
        include_str!("../test_data/client-trades.json"),
        |source| parse::trade(source, &instrument).unwrap(),
        |source, actual| assert_trade(source, actual, &instrument),
    );
}

fn bench_book(c: &mut Criterion) {
    let instrument = instrument();
    let mut group = c.benchmark_group("book");
    let mut inputs = vec![("authored".to_owned(), BOOK.to_owned())];
    for levels in [1, 10, 100, 1_000] {
        inputs.push((format!("levels_{levels}"), book_payload(levels)));
    }

    for (name, payload) in inputs {
        let source: Book = serde_json::from_str(&payload).unwrap();
        let levels = source.bids.as_ref().unwrap().len() + source.asks.as_ref().unwrap().len();

        for depth in [None, Some(1), Some(10)] {
            let suffix = depth.map_or_else(|| "full".to_owned(), |depth| format!("depth_{depth}"));
            let name = format!("{name}/{suffix}");
            let actual = parse::book(&source, &instrument, depth).unwrap();
            assert_book(&source, &actual, &instrument, depth);
            group.throughput(Throughput::Bytes(payload.len() as u64));
            group.bench_with_input(
                BenchmarkId::new("decode_convert", &name),
                &payload,
                |b, payload| {
                    b.iter(|| {
                        let source = serde_json::from_slice(black_box(payload.as_bytes())).unwrap();
                        black_box(parse::book(&source, &instrument, depth).unwrap());
                    });
                },
            );

            group.throughput(Throughput::Elements(levels as u64));
            group.bench_with_input(BenchmarkId::new("convert", &name), &source, |b, source| {
                b.iter(|| black_box(parse::book(black_box(source), &instrument, depth).unwrap()));
            });
        }
    }

    group.finish();
}

fn bench_bars(c: &mut Criterion) {
    let instrument = instrument();
    let periods: Vec<OhlcvPeriod> =
        serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
    let period = periods
        .iter()
        .find(|period| period.period_id.as_ref().map(AsRef::as_ref) == Some("1MIN"))
        .unwrap();
    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id));

    for (name, payload) in [
        ("bars/priced", BARS),
        (
            "bars/normalized_volume",
            include_str!("../test_data/ohlcv-volume-noise.json"),
        ),
    ] {
        bench_records::<Ohlcv, _>(
            c,
            name,
            payload,
            |source| {
                parse::bar(source, bar_type, &instrument, period)
                    .unwrap()
                    .expect("priced benchmark bar")
            },
            |source, actual| assert_bar(source, actual, bar_type, &instrument),
        );
    }

    let mut empty: Value = serde_json::from_str(BARS).unwrap();
    for row in empty.as_array_mut().unwrap() {
        for field in ["price_open", "price_high", "price_low", "price_close"] {
            row[field] = Value::Null;
        }

        row["volume_traded"] = json!("0");
        row["trades_count"] = json!(0);
    }

    bench_records::<Ohlcv, _>(
        c,
        "bars/empty",
        &empty.to_string(),
        |source| parse::bar(source, bar_type, &instrument, period).unwrap(),
        |_, actual| assert_eq!(*actual, None),
    );
}

fn bench_records<T: DeserializeOwned, U>(
    c: &mut Criterion,
    name: &str,
    template: &str,
    convert: impl Fn(&T) -> U,
    verify: impl Fn(&T, &U),
) {
    let mut group = c.benchmark_group(name);

    for &count in HISTORY_SIZES {
        let payload = repeat_rows(template, count);
        let source: Vec<T> = serde_json::from_str(&payload).unwrap();
        assert_eq!(source.len(), count);
        let mut verified = 0;
        convert_records(&source, &convert, |index, actual| {
            assert_eq!(index, verified);
            verify(&source[index], &actual);
            verified += 1;
        });

        assert_eq!(verified, count);

        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("decode_convert", count),
            &payload,
            |b, payload| {
                b.iter(|| {
                    let source: Vec<T> =
                        serde_json::from_slice(black_box(payload.as_bytes())).unwrap();
                    convert_records(&source, &convert, |_, value| {
                        black_box(value);
                    });
                });
            },
        );

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("convert", count), &source, |b, source| {
            b.iter(|| {
                convert_records(black_box(source), &convert, |_, value| {
                    black_box(value);
                });
            });
        });
    }

    group.finish();
}

fn convert_records<T, U>(
    source: &[T],
    convert: impl Fn(&T) -> U,
    mut consume: impl FnMut(usize, U),
) {
    for (index, record) in source.iter().enumerate() {
        consume(index, convert(record));
    }
}

fn repeat_rows(template: &str, count: usize) -> String {
    let rows: Vec<&RawValue> = serde_json::from_str(template).unwrap();
    assert!(!rows.is_empty());
    let rows: Vec<_> = rows
        .iter()
        .cycle()
        .take(count)
        .map(|row| row.get())
        .collect();
    format!("[{}]", rows.join(","))
}

fn book_payload(levels: usize) -> String {
    let mut source: Value = serde_json::from_str(BOOK).unwrap();
    for (side, sign) in [("bids", -1), ("asks", 1)] {
        source[side] = (0..levels)
            .map(|index| {
                let index = i64::try_from(index).unwrap();
                let price = Decimal::new(5_000 + sign * (index + 1), 4);
                let size = Decimal::new(1_000_001 + index * 10 + sign, 6);
                json!({"price":price.to_string(), "size":size.to_string()})
            })
            .collect();
    }

    source.to_string()
}

fn activity(
    source: &ActivityBatch,
    instrument: &BinaryOption,
    mut quote: impl FnMut(QuoteTick),
    mut trade: impl FnMut(TradeTick),
) {
    for record in source.quotes.iter().flatten() {
        quote(parse::quote(record, instrument).unwrap());
    }

    for record in source.trades.iter().flatten() {
        trade(parse::trade(record, instrument).unwrap());
    }
}

fn assert_quote(source: &Quote, actual: &QuoteTick, instrument: &BinaryOption) {
    assert_eq!(actual.instrument_id, instrument.id);
    assert_eq!(actual.bid_price.as_decimal(), source.bid);
    assert_eq!(actual.ask_price.as_decimal(), source.ask);
    assert_eq!(actual.bid_size.as_decimal(), source.bid_volume);
    assert_eq!(actual.ask_size.as_decimal(), source.ask_volume);
    assert_eq!(
        i128::from(actual.ts_event.as_u64()),
        source.entry_time.as_nanosecond()
    );
    assert_eq!(
        i128::from(actual.ts_init.as_u64()),
        source.recv_time.as_nanosecond()
    );
    assert_eq!(
        [actual.bid_price.precision, actual.ask_price.precision],
        [instrument.price_precision; 2]
    );
    assert_eq!(
        [actual.bid_size.precision, actual.ask_size.precision],
        [instrument.size_precision; 2]
    );
}

fn assert_trade(source: &Trade, actual: &TradeTick, instrument: &BinaryOption) {
    assert_eq!(actual.instrument_id, instrument.id);
    assert_eq!(Some(actual.trade_id.as_str()), source.id.as_deref());
    assert_eq!(actual.price.as_decimal(), source.price);
    assert_eq!(actual.size.as_decimal(), source.quantity);
    assert_eq!(actual.price.precision, instrument.price_precision);
    assert_eq!(actual.size.precision, instrument.size_precision);
    assert_eq!(actual.aggressor_side, AggressorSide::NoAggressor);
    assert_eq!(
        i128::from(actual.ts_event.as_u64()),
        source.timestamp.as_nanosecond()
    );
    assert_eq!(actual.ts_init, actual.ts_event);
}

fn assert_book(source: &Book, actual: &OrderBook, instrument: &BinaryOption, depth: Option<usize>) {
    assert_eq!(actual.instrument_id, instrument.id);
    assert_eq!(actual.book_type, BookType::L2_MBP);
    assert_eq!(actual.sequence, 0);
    assert_eq!(
        i128::from(actual.ts_last.as_u64()),
        source.time_exchange.as_nanosecond()
    );
    book_check_integrity(actual).unwrap();

    for (source, actual) in [
        (source.bids.as_ref().unwrap(), actual.bids_as_map(None)),
        (source.asks.as_ref().unwrap(), actual.asks_as_map(None)),
    ] {
        let expected: Vec<_> = source
            .iter()
            .take(depth.unwrap_or(source.len()))
            .map(|level| (level.price, level.size))
            .collect();
        assert_eq!(actual.into_iter().collect::<Vec<_>>(), expected);
    }
}

fn assert_bar(source: &Ohlcv, actual: &Bar, bar_type: BarType, instrument: &BinaryOption) {
    assert_eq!(actual.bar_type, bar_type);
    assert_eq!(Some(actual.open.as_decimal()), source.price_open);
    assert_eq!(Some(actual.high.as_decimal()), source.price_high);
    assert_eq!(Some(actual.low.as_decimal()), source.price_low);
    assert_eq!(Some(actual.close.as_decimal()), source.price_close);
    assert_eq!(
        actual.volume.as_decimal(),
        source.volume_traded.round_dp_with_strategy(
            u32::from(instrument.size_precision),
            RoundingStrategy::MidpointNearestEven
        )
    );
    assert_eq!(
        [
            actual.open.precision,
            actual.high.precision,
            actual.low.precision,
            actual.close.precision
        ],
        [instrument.price_precision; 4]
    );
    assert_eq!(actual.volume.precision, instrument.size_precision);
    assert_eq!(
        i128::from(actual.ts_event.as_u64()),
        source.time_period_end.as_nanosecond()
    );
    assert_eq!(actual.ts_init, actual.ts_event);
}

fn instrument() -> BinaryOption {
    let market = serde_json::from_str(include_str!("../test_data/polymarket-market.json")).unwrap();
    http::parse::parse_instrument(&market, UnixNanos::default()).unwrap()
}

criterion_group!(
    benches,
    bench_activity,
    bench_history,
    bench_book,
    bench_bars
);
criterion_main!(benches);

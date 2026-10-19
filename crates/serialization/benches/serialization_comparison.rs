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

//! Comparison benchmarks across different serialization formats.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::serialization::{FromMsgPack, Serializable, ToMsgPack};
use nautilus_model::{
    data::{
        QuoteTick, TradeTick,
        bar::{Bar, BarSpecification, BarType},
    },
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

fn create_quote_tick() -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("AAPL.XNAS"),
        bid_price: Price::from("100.50"),
        ask_price: Price::from("100.55"),
        bid_size: Quantity::from("100"),
        ask_size: Quantity::from("100"),
        ts_event: 1_609_459_200_000_000_000.into(),
        ts_init: 1_609_459_200_000_000_000.into(),
    }
}

fn create_trade_tick() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("ETHUSDT.BINANCE"),
        price: Price::from("2500.75"),
        size: Quantity::from("1.5"),
        aggressor_side: AggressorSide::Buy,
        trade_id: TradeId::from("12345"),
        ts_event: 1_609_459_200_000_000_000.into(),
        ts_init: 1_609_459_200_000_000_000.into(),
    }
}

fn create_bar() -> Bar {
    let bar_type = BarType::new(
        InstrumentId::from("AAPL.XNAS"),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::Internal,
    );
    Bar::new(
        bar_type,
        Price::from("150.00"),
        Price::from("152.50"),
        Price::from("149.75"),
        Price::from("151.25"),
        Quantity::from("100000"),
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_000.into(),
    )
}

// QuoteTick benchmarks

fn bench_quote_tick_json_serialize(c: &mut Criterion) {
    let quote = create_quote_tick();
    c.bench_function("QuoteTick::json_serialize", |b| {
        b.iter(|| black_box(black_box(&quote).to_json_bytes().unwrap()));
    });
}

fn bench_quote_tick_json_deserialize(c: &mut Criterion) {
    let quote = create_quote_tick();
    let bytes = quote.to_json_bytes().unwrap();
    c.bench_function("QuoteTick::json_deserialize", |b| {
        b.iter(|| black_box(QuoteTick::from_json_bytes(black_box(&bytes)).unwrap()));
    });
}

fn bench_quote_tick_msgpack_serialize(c: &mut Criterion) {
    let quote = create_quote_tick();
    c.bench_function("QuoteTick::msgpack_serialize", |b| {
        b.iter(|| black_box(black_box(&quote).to_msgpack_bytes().unwrap()));
    });
}

fn bench_quote_tick_msgpack_deserialize(c: &mut Criterion) {
    let quote = create_quote_tick();
    let bytes = quote.to_msgpack_bytes().unwrap();
    c.bench_function("QuoteTick::msgpack_deserialize", |b| {
        b.iter(|| black_box(QuoteTick::from_msgpack_bytes(black_box(&bytes)).unwrap()));
    });
}

// TradeTick benchmarks

fn bench_trade_tick_json_serialize(c: &mut Criterion) {
    let trade = create_trade_tick();
    c.bench_function("TradeTick::json_serialize", |b| {
        b.iter(|| black_box(black_box(&trade).to_json_bytes().unwrap()));
    });
}

fn bench_trade_tick_json_deserialize(c: &mut Criterion) {
    let trade = create_trade_tick();
    let bytes = trade.to_json_bytes().unwrap();
    c.bench_function("TradeTick::json_deserialize", |b| {
        b.iter(|| black_box(TradeTick::from_json_bytes(black_box(&bytes)).unwrap()));
    });
}

fn bench_trade_tick_msgpack_serialize(c: &mut Criterion) {
    let trade = create_trade_tick();
    c.bench_function("TradeTick::msgpack_serialize", |b| {
        b.iter(|| black_box(black_box(&trade).to_msgpack_bytes().unwrap()));
    });
}

fn bench_trade_tick_msgpack_deserialize(c: &mut Criterion) {
    let trade = create_trade_tick();
    let bytes = trade.to_msgpack_bytes().unwrap();
    c.bench_function("TradeTick::msgpack_deserialize", |b| {
        b.iter(|| black_box(TradeTick::from_msgpack_bytes(black_box(&bytes)).unwrap()));
    });
}

// Bar benchmarks

fn bench_bar_json_serialize(c: &mut Criterion) {
    let bar = create_bar();
    c.bench_function("Bar::json_serialize", |b| {
        b.iter(|| black_box(black_box(&bar).to_json_bytes().unwrap()));
    });
}

fn bench_bar_json_deserialize(c: &mut Criterion) {
    let bar = create_bar();
    let bytes = bar.to_json_bytes().unwrap();
    c.bench_function("Bar::json_deserialize", |b| {
        b.iter(|| black_box(Bar::from_json_bytes(black_box(&bytes)).unwrap()));
    });
}

fn bench_bar_msgpack_serialize(c: &mut Criterion) {
    let bar = create_bar();
    c.bench_function("Bar::msgpack_serialize", |b| {
        b.iter(|| black_box(black_box(&bar).to_msgpack_bytes().unwrap()));
    });
}

fn bench_bar_msgpack_deserialize(c: &mut Criterion) {
    let bar = create_bar();
    let bytes = bar.to_msgpack_bytes().unwrap();
    c.bench_function("Bar::msgpack_deserialize", |b| {
        b.iter(|| black_box(Bar::from_msgpack_bytes(black_box(&bytes)).unwrap()));
    });
}

criterion_group!(
    quote_tick_benches,
    bench_quote_tick_json_serialize,
    bench_quote_tick_json_deserialize,
    bench_quote_tick_msgpack_serialize,
    bench_quote_tick_msgpack_deserialize,
);

criterion_group!(
    trade_tick_benches,
    bench_trade_tick_json_serialize,
    bench_trade_tick_json_deserialize,
    bench_trade_tick_msgpack_serialize,
    bench_trade_tick_msgpack_deserialize,
);

criterion_group!(
    bar_benches,
    bench_bar_json_serialize,
    bench_bar_json_deserialize,
    bench_bar_msgpack_serialize,
    bench_bar_msgpack_deserialize,
);

#[cfg(feature = "sbe")]
mod trade_batches {
    use criterion::{BatchSize, BenchmarkId, Throughput};
    use nautilus_serialization::sbe::{FromSbe, SbeCursor, ToSbe};
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct TradeBatch(Vec<TradeTick>);

    impl Serializable for TradeBatch {}

    type Decode = fn(&[u8]) -> Vec<TradeTick>;

    pub(super) fn bench(c: &mut Criterion) {
        let mut group = c.benchmark_group("TradeBatch::decode");
        let rotation = std::env::var("NAUTILUS_TRADE_BATCH_ROTATION")
            .map_or(0, |value| value.parse::<usize>().unwrap());

        for count in [100, 1_000] {
            let trades = TradeBatch(
                (0..count)
                    .map(|i| {
                        let mut trade = create_trade_tick();
                        trade.price = Price::from(format!("2500.{i:03}").as_str());
                        trade.size = Quantity::from(format!("1.{i:03}").as_str());

                        trade.aggressor_side = if i % 2 == 0 {
                            AggressorSide::Buy
                        } else {
                            AggressorSide::Sell
                        };

                        trade.trade_id = TradeId::from(format!("{i:08}").as_str());
                        trade.ts_event = (1_609_459_200_000_000_000_u64 + i as u64 * 2).into();
                        trade.ts_init = (1_609_459_200_000_000_001_u64 + i as u64 * 2).into();
                        trade
                    })
                    .collect(),
            );

            let json = trades.to_json_bytes().unwrap();
            let msgpack = trades.to_msgpack_bytes().unwrap();
            let sbe = encode_sbe(&trades.0);

            let mut formats: [(&str, &[u8], Decode); 3] = [
                ("json", &json, |bytes| {
                    TradeBatch::from_json_bytes(bytes).unwrap().0
                }),
                ("msgpack", &msgpack, |bytes| {
                    TradeBatch::from_msgpack_bytes(bytes).unwrap().0
                }),
                ("sbe", &sbe, decode_sbe),
            ];

            let rotation = rotation % formats.len();
            formats.rotate_left(rotation);
            group.throughput(Throughput::Elements(count as u64));

            for (format, bytes, decode) in formats {
                assert_eq!(decode(bytes), trades.0);
                println!(
                    "{}",
                    serde_json::json!({"trade_batch": count, "format": format, "bytes": bytes.len()})
                );
                group.bench_with_input(BenchmarkId::new(format, count), bytes, |b, bytes| {
                    b.iter_batched(
                        || (),
                        |()| black_box(decode(black_box(bytes))),
                        BatchSize::NumIterations(16),
                    );
                });
            }
        }

        group.finish();
    }

    fn encode_sbe(trades: &[TradeTick]) -> Vec<u8> {
        // Benchmark-only batch framing retains each existing SBE message behind a byte length
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(trades.len()).unwrap().to_le_bytes());

        for trade in trades {
            let payload = trade.to_sbe().unwrap();
            bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
            bytes.extend_from_slice(&payload);
        }

        bytes
    }

    fn decode_sbe(bytes: &[u8]) -> Vec<TradeTick> {
        let mut cursor = SbeCursor::new(bytes);
        let count = cursor.read_u32_le().unwrap();
        let mut trades = Vec::with_capacity(count as usize);

        for _ in 0..count {
            let len = cursor.read_u32_le().unwrap() as usize;
            let payload = cursor.read_bytes(len).unwrap();
            trades.push(TradeTick::from_sbe(payload).unwrap());
        }

        assert_eq!(cursor.remaining(), 0);
        trades
    }
}

#[cfg(feature = "sbe")]
criterion_group!(trade_batch_benches, trade_batches::bench);

#[cfg(feature = "sbe")]
criterion_main!(
    quote_tick_benches,
    trade_tick_benches,
    bar_benches,
    trade_batch_benches
);

#[cfg(not(feature = "sbe"))]
criterion_main!(quote_tick_benches, trade_tick_benches, bar_benches);

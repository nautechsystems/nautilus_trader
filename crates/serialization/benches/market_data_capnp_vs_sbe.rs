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

use capnp::{
    message::{Builder, ReaderOptions},
    serialize,
};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate,
        InstrumentClose, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick, stubs::stub_depth10,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, InstrumentCloseType,
        MarketStatusAction, OrderSide, PriceType,
    },
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_serialization::{
    capnp::{FromCapnp, ToCapnp, market_capnp},
    sbe::{DataAny, FromSbe, FromSbeReuse, ToSbe},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

macro_rules! capnp_helpers {
    ($encode_fn:ident, $decode_fn:ident, $ty:ty, $builder_ty:path, $reader_ty:path) => {
        fn $encode_fn(value: &$ty) -> Vec<u8> {
            let mut message = Builder::new_default();
            let builder = message.init_root::<$builder_ty>();
            value.to_capnp(builder);

            let mut bytes = Vec::new();
            serialize::write_message(&mut bytes, &message).unwrap();
            bytes
        }

        fn $decode_fn(bytes: &[u8]) -> $ty {
            let reader = serialize::read_message(&mut &bytes[..], ReaderOptions::new()).unwrap();
            let root = reader.get_root::<$reader_ty>().unwrap();
            <$ty as FromCapnp>::from_capnp(root).unwrap()
        }
    };
}

capnp_helpers!(
    encode_book_order_capnp,
    decode_book_order_capnp,
    BookOrder,
    market_capnp::book_order::Builder,
    market_capnp::book_order::Reader
);
capnp_helpers!(
    encode_order_book_delta_capnp,
    decode_order_book_delta_capnp,
    OrderBookDelta,
    market_capnp::order_book_delta::Builder,
    market_capnp::order_book_delta::Reader
);
capnp_helpers!(
    encode_order_book_deltas_capnp,
    decode_order_book_deltas_capnp,
    OrderBookDeltas,
    market_capnp::order_book_deltas::Builder,
    market_capnp::order_book_deltas::Reader
);
capnp_helpers!(
    encode_order_book_depth10_capnp,
    decode_order_book_depth10_capnp,
    OrderBookDepth10,
    market_capnp::order_book_depth10::Builder,
    market_capnp::order_book_depth10::Reader
);
capnp_helpers!(
    encode_quote_tick_capnp,
    decode_quote_tick_capnp,
    QuoteTick,
    market_capnp::quote_tick::Builder,
    market_capnp::quote_tick::Reader
);
capnp_helpers!(
    encode_trade_tick_capnp,
    decode_trade_tick_capnp,
    TradeTick,
    market_capnp::trade_tick::Builder,
    market_capnp::trade_tick::Reader
);
capnp_helpers!(
    encode_bar_type_capnp,
    decode_bar_type_capnp,
    BarType,
    market_capnp::bar_type::Builder,
    market_capnp::bar_type::Reader
);
capnp_helpers!(
    encode_bar_capnp,
    decode_bar_capnp,
    Bar,
    market_capnp::bar::Builder,
    market_capnp::bar::Reader
);
capnp_helpers!(
    encode_mark_price_update_capnp,
    decode_mark_price_update_capnp,
    MarkPriceUpdate,
    market_capnp::mark_price_update::Builder,
    market_capnp::mark_price_update::Reader
);
capnp_helpers!(
    encode_index_price_update_capnp,
    decode_index_price_update_capnp,
    IndexPriceUpdate,
    market_capnp::index_price_update::Builder,
    market_capnp::index_price_update::Reader
);
capnp_helpers!(
    encode_funding_rate_update_capnp,
    decode_funding_rate_update_capnp,
    FundingRateUpdate,
    market_capnp::funding_rate_update::Builder,
    market_capnp::funding_rate_update::Reader
);
capnp_helpers!(
    encode_instrument_status_capnp,
    decode_instrument_status_capnp,
    InstrumentStatus,
    market_capnp::instrument_status::Builder,
    market_capnp::instrument_status::Reader
);
capnp_helpers!(
    encode_instrument_close_capnp,
    decode_instrument_close_capnp,
    InstrumentClose,
    market_capnp::instrument_close::Builder,
    market_capnp::instrument_close::Reader
);

fn sample_book_order() -> BookOrder {
    BookOrder::new(
        OrderSide::Buy,
        Price::from("100.50"),
        Quantity::from("10"),
        123_456,
    )
}

fn sample_quote_tick() -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("AAPL.XNAS"),
        bid_price: Price::from("100.50"),
        ask_price: Price::from("100.55"),
        bid_size: Quantity::from("100"),
        ask_size: Quantity::from("125"),
        ts_event: 1_609_459_200_000_000_000.into(),
        ts_init: 1_609_459_200_000_000_001.into(),
    }
}

fn sample_trade_tick() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        price: Price::from("2500.75"),
        size: Quantity::from("1.5"),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::from("12345"),
        ts_event: 1_609_459_200_000_000_000.into(),
        ts_init: 1_609_459_200_000_000_001.into(),
    }
}

fn sample_bar_type() -> BarType {
    BarType::new(
        InstrumentId::from("AAPL.XNAS"),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::Internal,
    )
}

fn sample_bar() -> Bar {
    Bar::new(
        sample_bar_type(),
        Price::from("150.00"),
        Price::from("152.50"),
        Price::from("149.75"),
        Price::from("151.25"),
        Quantity::from("100000"),
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_001.into(),
    )
}

fn sample_mark_price_update() -> MarkPriceUpdate {
    MarkPriceUpdate::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50000.50"),
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_001.into(),
    )
}

fn sample_index_price_update() -> IndexPriceUpdate {
    IndexPriceUpdate::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50125.75"),
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_001.into(),
    )
}

fn sample_funding_rate_update() -> FundingRateUpdate {
    FundingRateUpdate::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        dec!(0.0001),
        Some(60),
        Some(1_609_459_260_000_000_000.into()),
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_001.into(),
    )
}

fn sample_instrument_status() -> InstrumentStatus {
    InstrumentStatus::new(
        InstrumentId::from("MSFT.XNAS"),
        MarketStatusAction::Trading,
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_001.into(),
        Some(Ustr::from("Regular trading")),
        Some(Ustr::from("Continuous trading")),
        Some(true),
        Some(true),
        Some(false),
    )
}

fn sample_instrument_close() -> InstrumentClose {
    InstrumentClose::new(
        InstrumentId::from("MSFT.XNAS"),
        Price::from("100.50"),
        InstrumentCloseType::EndOfSession,
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_001.into(),
    )
}

fn sample_order_book_delta() -> OrderBookDelta {
    OrderBookDelta::new(
        InstrumentId::from("AAPL.XNAS"),
        BookAction::Add,
        sample_book_order(),
        0,
        1,
        1_609_459_200_000_000_000.into(),
        1_609_459_200_000_000_001.into(),
    )
}

fn sample_order_book_deltas(count: usize) -> OrderBookDeltas {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut deltas = Vec::with_capacity(count);

    for i in 0..count {
        let order = BookOrder::new(
            if i % 2 == 0 {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            },
            Price::from(&format!("100.{i:02}")),
            Quantity::from("5.0"),
            i as u64,
        );

        let action = match i % 3 {
            0 => BookAction::Add,
            1 => BookAction::Update,
            _ => BookAction::Delete,
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            order,
            0,
            i as u64,
            (1_609_459_200_000_000_000 + i as u64).into(),
            (1_609_459_200_000_000_100 + i as u64).into(),
        ));
    }

    OrderBookDeltas::new(instrument_id, deltas)
}

fn sample_order_book_depth10() -> OrderBookDepth10 {
    let mut depth = stub_depth10();

    // The wire format stores book levels without order IDs
    for bid in &mut depth.bids {
        bid.order_id = 0;
    }

    for ask in &mut depth.asks {
        ask.order_id = 0;
    }

    depth
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "benchmark helper owns one sample value and reuses it across sub-benchmarks"
)]
fn bench_capnp_sbe_type<T>(
    c: &mut Criterion,
    name: &str,
    value: T,
    capnp_encode: fn(&T) -> Vec<u8>,
    capnp_decode: fn(&[u8]) -> T,
) where
    T: Clone + FromSbe + PartialEq + std::fmt::Debug + ToSbe,
{
    let sbe_bytes = value.to_sbe().unwrap();
    let capnp_bytes = capnp_encode(&value);

    assert_eq!(T::from_sbe(&sbe_bytes).unwrap(), value);
    assert_eq!(capnp_decode(&capnp_bytes), value);

    let mut group = c.benchmark_group(name);
    group.bench_function("sbe_encode", |b| {
        b.iter(|| black_box(black_box(&value).to_sbe().unwrap()));
    });
    group.bench_function("sbe_encode_reuse", |b| {
        let mut buf = Vec::new();
        b.iter(|| {
            black_box(&value).to_sbe_into(&mut buf).unwrap();
            black_box(buf.as_slice());
        });
    });
    group.bench_function("sbe_decode", |b| {
        b.iter(|| black_box(T::from_sbe(black_box(&sbe_bytes)).unwrap()));
    });
    group.bench_function("capnp_encode", |b| {
        b.iter(|| black_box(capnp_encode(black_box(&value))));
    });
    group.bench_function("capnp_decode", |b| {
        b.iter(|| black_box(capnp_decode(black_box(&capnp_bytes))));
    });
    group.finish();
}

fn bench_market_data_types(c: &mut Criterion) {
    bench_capnp_sbe_type(
        c,
        "BookOrder::wire",
        sample_book_order(),
        encode_book_order_capnp,
        decode_book_order_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "OrderBookDelta::wire",
        sample_order_book_delta(),
        encode_order_book_delta_capnp,
        decode_order_book_delta_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "OrderBookDeltas::wire",
        sample_order_book_deltas(10),
        encode_order_book_deltas_capnp,
        decode_order_book_deltas_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "OrderBookDepth10::wire",
        sample_order_book_depth10(),
        encode_order_book_depth10_capnp,
        decode_order_book_depth10_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "QuoteTick::wire",
        sample_quote_tick(),
        encode_quote_tick_capnp,
        decode_quote_tick_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "TradeTick::wire",
        sample_trade_tick(),
        encode_trade_tick_capnp,
        decode_trade_tick_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "BarType::wire",
        sample_bar_type(),
        encode_bar_type_capnp,
        decode_bar_type_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "Bar::wire",
        sample_bar(),
        encode_bar_capnp,
        decode_bar_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "MarkPriceUpdate::wire",
        sample_mark_price_update(),
        encode_mark_price_update_capnp,
        decode_mark_price_update_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "IndexPriceUpdate::wire",
        sample_index_price_update(),
        encode_index_price_update_capnp,
        decode_index_price_update_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "FundingRateUpdate::wire",
        sample_funding_rate_update(),
        encode_funding_rate_update_capnp,
        decode_funding_rate_update_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "InstrumentStatus::wire",
        sample_instrument_status(),
        encode_instrument_status_capnp,
        decode_instrument_status_capnp,
    );
    bench_capnp_sbe_type(
        c,
        "InstrumentClose::wire",
        sample_instrument_close(),
        encode_instrument_close_capnp,
        decode_instrument_close_capnp,
    );
}

fn bench_order_book_deltas_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("OrderBookDeltas::scaling");

    for count in [1usize, 10, 100] {
        let value = sample_order_book_deltas(count);
        let sbe_bytes = value.to_sbe().unwrap();
        let capnp_bytes = encode_order_book_deltas_capnp(&value);

        assert_eq!(OrderBookDeltas::from_sbe(&sbe_bytes).unwrap(), value);
        assert_eq!(decode_order_book_deltas_capnp(&capnp_bytes), value);

        group.bench_with_input(BenchmarkId::new("sbe_encode", count), &value, |b, value| {
            b.iter(|| black_box(black_box(value).to_sbe().unwrap()));
        });
        group.bench_with_input(
            BenchmarkId::new("sbe_encode_reuse", count),
            &value,
            |b, value| {
                let mut buf = Vec::new();
                b.iter(|| {
                    black_box(value).to_sbe_into(&mut buf).unwrap();
                    black_box(buf.as_slice());
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("sbe_decode", count),
            &sbe_bytes,
            |b, bytes| {
                b.iter(|| black_box(OrderBookDeltas::from_sbe(black_box(bytes)).unwrap()));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("sbe_decode_reuse", count),
            &sbe_bytes,
            |b, bytes| {
                let mut scratch: Vec<OrderBookDelta> = Vec::new();
                b.iter(|| {
                    let result =
                        OrderBookDeltas::from_sbe_reuse(black_box(bytes), &mut scratch).unwrap();
                    scratch = black_box(result).deltas;
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("capnp_encode", count),
            &value,
            |b, value| {
                b.iter(|| black_box(encode_order_book_deltas_capnp(black_box(value))));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("capnp_decode", count),
            &capnp_bytes,
            |b, bytes| {
                b.iter(|| black_box(decode_order_book_deltas_capnp(black_box(bytes))));
            },
        );
    }

    group.finish();
}

fn bench_data_any(c: &mut Criterion) {
    let cases = vec![
        ("OrderBookDelta", DataAny::from(sample_order_book_delta())),
        (
            "OrderBookDeltas",
            DataAny::from(sample_order_book_deltas(10)),
        ),
        (
            "OrderBookDepth10",
            DataAny::from(sample_order_book_depth10()),
        ),
        ("QuoteTick", DataAny::from(sample_quote_tick())),
        ("TradeTick", DataAny::from(sample_trade_tick())),
        ("Bar", DataAny::from(sample_bar())),
        ("MarkPriceUpdate", DataAny::from(sample_mark_price_update())),
        (
            "IndexPriceUpdate",
            DataAny::from(sample_index_price_update()),
        ),
        (
            "FundingRateUpdate",
            DataAny::from(sample_funding_rate_update()),
        ),
        (
            "InstrumentStatus",
            DataAny::from(sample_instrument_status()),
        ),
        ("InstrumentClose", DataAny::from(sample_instrument_close())),
    ];

    let mut group = c.benchmark_group("DataAny::sbe");

    for (name, value) in cases {
        let bytes = value.to_sbe().unwrap();
        assert_eq!(DataAny::from_sbe(&bytes).unwrap(), value);

        group.bench_with_input(BenchmarkId::new("encode", name), &value, |b, value| {
            b.iter(|| black_box(black_box(value).to_sbe().unwrap()));
        });
        group.bench_with_input(
            BenchmarkId::new("encode_reuse", name),
            &value,
            |b, value| {
                let mut buf = Vec::new();
                b.iter(|| {
                    black_box(value).to_sbe_into(&mut buf).unwrap();
                    black_box(buf.as_slice());
                });
            },
        );
        group.bench_with_input(BenchmarkId::new("decode", name), &bytes, |b, bytes| {
            b.iter(|| black_box(DataAny::from_sbe(black_box(bytes)).unwrap()));
        });
    }

    group.finish();
}

criterion_group!(
    market_data_benches,
    bench_market_data_types,
    bench_order_book_deltas_scaling,
    bench_data_any
);
criterion_main!(market_data_benches);

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_model::{
    data::{
        QuoteTick, TradeTick,
        bar::{Bar, BarSpecification, BarType},
        delta::OrderBookDelta,
        deltas::OrderBookDeltas,
        order::BookOrder,
    },
    enums::{AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_serialization::capnp::{FromCapnp, ToCapnp, market_capnp};

// Helper functions to create test data

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
        aggressor_side: AggressorSide::Buyer,
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

fn create_order_book_deltas(delta_count: usize) -> OrderBookDeltas {
    let instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
    let mut deltas = Vec::with_capacity(delta_count);

    for i in 0..delta_count {
        let order = BookOrder::new(
            if i % 2 == 0 {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            },
            Price::from(&format!("100.{}", i)),
            Quantity::from("5.0"),
            i as u64,
        );

        let action = match i % 3 {
            0 => BookAction::Add,
            1 => BookAction::Update,
            _ => BookAction::Delete,
        };

        let delta = OrderBookDelta::new(
            instrument_id,
            action,
            order,
            0,
            i as u64,
            (1_609_459_200_000_000_000 + i as u64).into(),
            (1_609_459_200_000_000_000 + i as u64).into(),
        );
        deltas.push(delta);
    }

    OrderBookDeltas::new(instrument_id, deltas)
}

// QuoteTick benchmarks

fn bench_quote_tick_serialize(c: &mut Criterion) {
    let quote = create_quote_tick();
    c.bench_function("QuoteTick::capnp_serialize", |b| {
        b.iter(|| {
            let mut message = capnp::message::Builder::new_default();
            let builder = message.init_root::<market_capnp::quote_tick::Builder>();
            black_box(&quote).to_capnp(builder);
            let mut bytes = Vec::new();
            capnp::serialize::write_message(&mut bytes, &message).unwrap();
            black_box(bytes)
        });
    });
}

fn bench_quote_tick_deserialize(c: &mut Criterion) {
    let quote = create_quote_tick();
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::quote_tick::Builder>();
    quote.to_capnp(builder);
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    c.bench_function("QuoteTick::capnp_deserialize", |b| {
        b.iter(|| {
            let reader = capnp::serialize::read_message(
                &mut black_box(&bytes[..]),
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let root = reader
                .get_root::<market_capnp::quote_tick::Reader>()
                .unwrap();
            black_box(QuoteTick::from_capnp(root).unwrap())
        });
    });
}

// TradeTick benchmarks

fn bench_trade_tick_serialize(c: &mut Criterion) {
    let trade = create_trade_tick();
    c.bench_function("TradeTick::capnp_serialize", |b| {
        b.iter(|| {
            let mut message = capnp::message::Builder::new_default();
            let builder = message.init_root::<market_capnp::trade_tick::Builder>();
            black_box(&trade).to_capnp(builder);
            let mut bytes = Vec::new();
            capnp::serialize::write_message(&mut bytes, &message).unwrap();
            black_box(bytes)
        });
    });
}

fn bench_trade_tick_deserialize(c: &mut Criterion) {
    let trade = create_trade_tick();
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::trade_tick::Builder>();
    black_box(&trade).to_capnp(builder);
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    c.bench_function("TradeTick::capnp_deserialize", |b| {
        b.iter(|| {
            let reader = capnp::serialize::read_message(
                &mut black_box(&bytes[..]),
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let root = reader
                .get_root::<market_capnp::trade_tick::Reader>()
                .unwrap();
            black_box(TradeTick::from_capnp(root).unwrap())
        });
    });
}

// Bar benchmarks

fn bench_bar_serialize(c: &mut Criterion) {
    let bar = create_bar();
    c.bench_function("Bar::capnp_serialize", |b| {
        b.iter(|| {
            let mut message = capnp::message::Builder::new_default();
            let builder = message.init_root::<market_capnp::bar::Builder>();
            black_box(&bar).to_capnp(builder);
            let mut bytes = Vec::new();
            capnp::serialize::write_message(&mut bytes, &message).unwrap();
            black_box(bytes)
        });
    });
}

fn bench_bar_deserialize(c: &mut Criterion) {
    let bar = create_bar();
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar::Builder>();
    black_box(&bar).to_capnp(builder);
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    c.bench_function("Bar::capnp_deserialize", |b| {
        b.iter(|| {
            let reader = capnp::serialize::read_message(
                &mut black_box(&bytes[..]),
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let root = reader.get_root::<market_capnp::bar::Reader>().unwrap();
            black_box(Bar::from_capnp(root).unwrap())
        });
    });
}

// OrderBookDeltas benchmarks (with varying delta counts)

fn bench_order_book_deltas_serialize_1(c: &mut Criterion) {
    let deltas = create_order_book_deltas(1);
    c.bench_function("OrderBookDeltas::capnp_serialize_1", |b| {
        b.iter(|| {
            let mut message = capnp::message::Builder::new_default();
            let builder = message.init_root::<market_capnp::order_book_deltas::Builder>();
            black_box(&deltas).to_capnp(builder);
            let mut bytes = Vec::new();
            capnp::serialize::write_message(&mut bytes, &message).unwrap();
            black_box(bytes)
        });
    });
}

fn bench_order_book_deltas_serialize_10(c: &mut Criterion) {
    let deltas = create_order_book_deltas(10);
    c.bench_function("OrderBookDeltas::capnp_serialize_10", |b| {
        b.iter(|| {
            let mut message = capnp::message::Builder::new_default();
            let builder = message.init_root::<market_capnp::order_book_deltas::Builder>();
            black_box(&deltas).to_capnp(builder);
            let mut bytes = Vec::new();
            capnp::serialize::write_message(&mut bytes, &message).unwrap();
            black_box(bytes)
        });
    });
}

fn bench_order_book_deltas_serialize_100(c: &mut Criterion) {
    let deltas = create_order_book_deltas(100);
    c.bench_function("OrderBookDeltas::capnp_serialize_100", |b| {
        b.iter(|| {
            let mut message = capnp::message::Builder::new_default();
            let builder = message.init_root::<market_capnp::order_book_deltas::Builder>();
            black_box(&deltas).to_capnp(builder);
            let mut bytes = Vec::new();
            capnp::serialize::write_message(&mut bytes, &message).unwrap();
            black_box(bytes)
        });
    });
}

fn bench_order_book_deltas_deserialize_1(c: &mut Criterion) {
    let deltas = create_order_book_deltas(1);
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::order_book_deltas::Builder>();
    black_box(&deltas).to_capnp(builder);
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    c.bench_function("OrderBookDeltas::capnp_deserialize_1", |b| {
        b.iter(|| {
            let reader = capnp::serialize::read_message(
                &mut black_box(&bytes[..]),
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let root = reader
                .get_root::<market_capnp::order_book_deltas::Reader>()
                .unwrap();
            black_box(OrderBookDeltas::from_capnp(root).unwrap())
        });
    });
}

fn bench_order_book_deltas_deserialize_10(c: &mut Criterion) {
    let deltas = create_order_book_deltas(10);
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::order_book_deltas::Builder>();
    black_box(&deltas).to_capnp(builder);
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    c.bench_function("OrderBookDeltas::capnp_deserialize_10", |b| {
        b.iter(|| {
            let reader = capnp::serialize::read_message(
                &mut black_box(&bytes[..]),
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let root = reader
                .get_root::<market_capnp::order_book_deltas::Reader>()
                .unwrap();
            black_box(OrderBookDeltas::from_capnp(root).unwrap())
        });
    });
}

fn bench_order_book_deltas_deserialize_100(c: &mut Criterion) {
    let deltas = create_order_book_deltas(100);
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::order_book_deltas::Builder>();
    black_box(&deltas).to_capnp(builder);
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    c.bench_function("OrderBookDeltas::capnp_deserialize_100", |b| {
        b.iter(|| {
            let reader = capnp::serialize::read_message(
                &mut black_box(&bytes[..]),
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let root = reader
                .get_root::<market_capnp::order_book_deltas::Reader>()
                .unwrap();
            black_box(OrderBookDeltas::from_capnp(root).unwrap())
        });
    });
}

criterion_group!(
    quote_tick_benches,
    bench_quote_tick_serialize,
    bench_quote_tick_deserialize,
);

criterion_group!(
    trade_tick_benches,
    bench_trade_tick_serialize,
    bench_trade_tick_deserialize,
);

criterion_group!(bar_benches, bench_bar_serialize, bench_bar_deserialize,);

criterion_group!(
    order_book_deltas_benches,
    bench_order_book_deltas_serialize_1,
    bench_order_book_deltas_serialize_10,
    bench_order_book_deltas_serialize_100,
    bench_order_book_deltas_deserialize_1,
    bench_order_book_deltas_deserialize_10,
    bench_order_book_deltas_deserialize_100,
);

criterion_main!(
    quote_tick_benches,
    trade_tick_benches,
    bar_benches,
    order_book_deltas_benches,
);

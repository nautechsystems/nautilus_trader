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

//! Component-level micro-benches that decompose the canonical pipeline numbers
//! into their constituent costs (`decode_only`, `parse_only`, atom-level
//! Decimal / Price / UUID construction, dispatch state churn).
//!
//! Use these when a `data.rs` or `exec.rs` bench regresses and you need to
//! localise where the time went, or when evaluating a structural change
//! (e.g. swapping the JSON tokenizer) and want to confirm the gain landed in
//! the layer it was supposed to.

mod common;

use std::{hint::black_box, str::FromStr};

use ahash::AHashMap;
use common::{btc_usd_spot, btc_usdt_spot, fixtures};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::OrderSide,
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_okx::websocket::{
    messages::{OKXTradeMsg, OKXWsFrame},
    parse::{parse_book_msg, parse_trade_msg},
};
use rust_decimal::Decimal;
use ustr::Ustr;

fn bench_decode_trade(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::TRADE)).unwrap();
            black_box(frame);
        });
    });
    group.finish();
}

fn bench_decode_book(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("book", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::BOOK_UPDATE)).unwrap();
            black_box(frame);
        });
    });
    group.finish();
}

fn bench_parse_trade(c: &mut Criterion) {
    // `fixtures::TRADE` loads `ws_trades.json` (instId = `BTC-USD`); use the
    // matching instrument so the produced `TradeTick` carries the correct
    // instrument id and precision.
    let instrument = btc_usd_spot();
    let frame: OKXWsFrame = serde_json::from_str(fixtures::TRADE).unwrap();
    let msgs: Vec<OKXTradeMsg> = match frame {
        OKXWsFrame::Data { data, .. } => serde_json::from_value(data).unwrap(),
        _ => unreachable!(),
    };
    let msg = &msgs[0];

    let mut group = c.benchmark_group("parse_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let tick = parse_trade_msg(
                black_box(msg),
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
                UnixNanos::default(),
            )
            .unwrap();
            black_box(tick);
        });
    });
    group.finish();
}

fn bench_parse_book_deltas(c: &mut Criterion) {
    let instrument = btc_usdt_spot();
    let frame: OKXWsFrame = serde_json::from_str(fixtures::BOOK_UPDATE).unwrap();
    let (msgs, action) = match frame {
        OKXWsFrame::BookData { data, action, .. } => (data, action),
        _ => unreachable!(),
    };
    let msg = &msgs[0];

    let mut group = c.benchmark_group("parse_only");
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let deltas = parse_book_msg(
                black_box(msg),
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
                &action,
                UnixNanos::default(),
            )
            .unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

fn bench_decimal_from_str(c: &mut Criterion) {
    let s = "98450.5";
    c.bench_function("atom/decimal_from_str", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            black_box(d);
        });
    });
}

fn bench_price_from_decimal_dp(c: &mut Criterion) {
    let d = Decimal::from_str("98450.5").unwrap();
    c.bench_function("atom/price_from_decimal_dp", |b| {
        b.iter(|| {
            let p = Price::from_decimal_dp(black_box(d), 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_price_combined(c: &mut Criterion) {
    let s = "98450.5";
    c.bench_function("atom/price_combined", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            let p = Price::from_decimal_dp(d, 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_trade_id_new(c: &mut Criterion) {
    let tid = "1518905529";
    c.bench_function("atom/trade_id_new", |b| {
        b.iter(|| {
            let id = TradeId::new(black_box(tid));
            black_box(id);
        });
    });
}

fn bench_uuid4_new(c: &mut Criterion) {
    c.bench_function("atom/uuid4_new", |b| {
        b.iter(|| {
            let u = UUID4::new();
            black_box(u);
        });
    });
}

fn bench_instrument_lookup(c: &mut Criterion) {
    let mut cache: AHashMap<Ustr, InstrumentAny> = AHashMap::new();
    let inst = btc_usdt_spot();
    cache.insert(Ustr::from("BTC-USDT"), inst);
    let key = Ustr::from("BTC-USDT");

    c.bench_function("atom/instrument_lookup", |b| {
        b.iter(|| {
            let v = cache.get(black_box(&key)).unwrap();
            black_box(v);
        });
    });
}

fn bench_book_order_construct(c: &mut Criterion) {
    use nautilus_model::data::BookOrder;
    let price = Price::from("98450.5");
    let qty = Quantity::from("2.5");

    c.bench_function("atom/book_order_construct", |b| {
        b.iter(|| {
            let order = BookOrder::new(OrderSide::Buy, black_box(price), black_box(qty), 0);
            black_box(order);
        });
    });
}

criterion_group!(
    benches,
    bench_decode_trade,
    bench_decode_book,
    bench_parse_trade,
    bench_parse_book_deltas,
    bench_decimal_from_str,
    bench_price_from_decimal_dp,
    bench_price_combined,
    bench_trade_id_new,
    bench_uuid4_new,
    bench_instrument_lookup,
    bench_book_order_construct,
);
criterion_main!(benches);

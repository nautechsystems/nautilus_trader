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

//! Benchmarks for Ax market data parsing (WS structs → Nautilus domain types).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_architect_ax::{
    common::consts::AX_VENUE,
    websocket::{
        data::parse::{
            parse_book_l1_quote, parse_book_l2_deltas, parse_book_l3_deltas, parse_candle_bar,
            parse_trade_tick,
        },
        messages::{AxMdBookL1, AxMdBookL2, AxMdBookL3, AxMdCandle, AxMdTrade},
    },
};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::{PerpetualContract, any::InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

fn create_eurusd_instrument() -> InstrumentAny {
    let price_precision = 4_u8;
    let size_precision = 0_u8;
    let symbol = "EURUSD-PERP";

    let price_increment =
        Price::from_decimal_dp(Decimal::new(1, price_precision as u32), price_precision).unwrap();
    let size_increment =
        Quantity::from_decimal_dp(Decimal::new(1, size_precision as u32), size_precision).unwrap();

    let instrument = PerpetualContract::new(
        InstrumentId::new(Symbol::new(symbol), *AX_VENUE),
        Symbol::new(symbol),
        Ustr::from("EURUSD"),
        AssetClass::FX,
        None,
        Currency::USD(),
        Currency::USD(),
        false,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,
        Some(size_increment),
        None,
        Some(size_increment),
        None,
        None,
        None,
        None,
        Some(Decimal::new(1, 2)),
        Some(Decimal::new(5, 3)),
        Some(Decimal::new(2, 4)),
        Some(Decimal::new(5, 4)),
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    InstrumentAny::PerpetualContract(instrument)
}

fn bench_parse_book_l1_quote(c: &mut Criterion) {
    let json = include_str!("../test_data/ws_md_book_l1_captured.json");
    let book: AxMdBookL1 = serde_json::from_str(json).unwrap();
    let instrument = create_eurusd_instrument();
    let ts_init = UnixNanos::default();

    c.bench_function("parse_book_l1_quote", |b| {
        b.iter(|| parse_book_l1_quote(black_box(&book), black_box(&instrument), ts_init).unwrap());
    });
}

fn bench_parse_book_l2_deltas(c: &mut Criterion) {
    let json = include_str!("../test_data/ws_md_book_l2_captured.json");
    let book: AxMdBookL2 = serde_json::from_str(json).unwrap();
    let instrument = create_eurusd_instrument();
    let ts_init = UnixNanos::default();

    c.bench_function("parse_book_l2_deltas (25 levels)", |b| {
        b.iter(|| {
            parse_book_l2_deltas(black_box(&book), black_box(&instrument), 1, ts_init).unwrap()
        });
    });
}

fn bench_parse_book_l3_deltas(c: &mut Criterion) {
    let json = include_str!("../test_data/ws_md_book_l3_captured.json");
    let book: AxMdBookL3 = serde_json::from_str(json).unwrap();
    let instrument = create_eurusd_instrument();
    let ts_init = UnixNanos::default();

    c.bench_function("parse_book_l3_deltas (29 levels)", |b| {
        b.iter(|| {
            parse_book_l3_deltas(black_box(&book), black_box(&instrument), 1, ts_init).unwrap()
        });
    });
}

fn bench_parse_trade_tick(c: &mut Criterion) {
    let json = include_str!("../test_data/ws_md_trade_captured.json");
    let trade: AxMdTrade = serde_json::from_str(json).unwrap();
    let instrument = create_eurusd_instrument();
    let ts_init = UnixNanos::default();

    c.bench_function("parse_trade_tick", |b| {
        b.iter(|| parse_trade_tick(black_box(&trade), black_box(&instrument), ts_init).unwrap());
    });
}

fn bench_parse_candle_bar(c: &mut Criterion) {
    let json = include_str!("../test_data/ws_md_candle.json");
    let candle: AxMdCandle = serde_json::from_str(json).unwrap();

    // Candle test data uses EURUSD-PERP
    let price_precision = 2_u8;
    let size_precision = 3_u8;
    let symbol = "EURUSD-PERP";

    let price_increment =
        Price::from_decimal_dp(Decimal::new(1, price_precision as u32), price_precision).unwrap();
    let size_increment =
        Quantity::from_decimal_dp(Decimal::new(1, size_precision as u32), size_precision).unwrap();

    let instrument = InstrumentAny::PerpetualContract(PerpetualContract::new(
        InstrumentId::new(Symbol::new(symbol), *AX_VENUE),
        Symbol::new(symbol),
        Ustr::from("EURUSD"),
        AssetClass::Cryptocurrency,
        None,
        Currency::USD(),
        Currency::USD(),
        false,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,
        Some(size_increment),
        None,
        Some(size_increment),
        None,
        None,
        None,
        None,
        Some(Decimal::new(1, 2)),
        Some(Decimal::new(5, 3)),
        Some(Decimal::new(2, 4)),
        Some(Decimal::new(5, 4)),
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ));
    let ts_init = UnixNanos::default();

    c.bench_function("parse_candle_bar", |b| {
        b.iter(|| parse_candle_bar(black_box(&candle), black_box(&instrument), ts_init).unwrap());
    });
}

criterion_group!(
    benches,
    bench_parse_book_l1_quote,
    bench_parse_book_l2_deltas,
    bench_parse_book_l3_deltas,
    bench_parse_trade_tick,
    bench_parse_candle_bar,
);
criterion_main!(benches);

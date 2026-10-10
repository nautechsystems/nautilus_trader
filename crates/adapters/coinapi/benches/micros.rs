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

//! Component breakdown of the inbound pipeline benches in `data.rs`.
//!
//! Decoding dominates each pipeline, so these benches split it into the whole frame decode, the
//! payload pass alone (the difference is the `type` envelope pass), and the per-field costs of an
//! exact decimal token and a timestamp, plus the instrument lookup every message makes.

mod common;

use std::hint::black_box;

use common::{COINBASE_BTC_USD, fixtures, instruments};
use criterion::{Criterion, criterion_group, criterion_main};
use jiff::Timestamp;
use nautilus_coinapi::websocket::messages::{Book, BookL3, Frame, Ohlcv, Quote, Trade};
use nautilus_core::serialization::decimal;
use rust_decimal::Decimal;
use serde::{Deserialize, de::DeserializeOwned};
use ustr::Ustr;

// Decodes one fixture's payload struct alone
type PayloadDecode = fn(&[u8]);

const FIXTURES: [(&str, &str, PayloadDecode); 5] = [
    ("trade", fixtures::TRADE, payload::<Trade>),
    ("quote", fixtures::QUOTE, payload::<Quote>),
    ("ohlcv", fixtures::OHLCV, payload::<Ohlcv>),
    ("book20", fixtures::BOOK20, payload::<Book>),
    ("book_l3", fixtures::BOOK_L3_UPDATE, payload::<BookL3>),
];

#[derive(Deserialize)]
struct DecimalProbe {
    #[serde(deserialize_with = "decimal::deserialize_json")]
    _price: Decimal,
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    for (name, fixture, _) in FIXTURES {
        group.bench_function(name, |b| {
            b.iter(|| black_box(Frame::decode(black_box(fixture.as_bytes())).unwrap()));
        });
    }

    group.finish();

    let mut group = c.benchmark_group("decode_payload");
    for (name, fixture, decode) in FIXTURES {
        group.bench_function(name, |b| b.iter(|| decode(black_box(fixture.as_bytes()))));
    }

    group.finish();
}

fn payload<T: DeserializeOwned>(bytes: &[u8]) {
    black_box(serde_json::from_slice::<T>(bytes).unwrap());
}

fn bench_atoms(c: &mut Criterion) {
    let instruments = instruments();
    let symbol_id = Ustr::from(COINBASE_BTC_USD);

    let mut group = c.benchmark_group("atom");
    group.bench_function("decimal_json", |b| {
        b.iter(|| {
            black_box(serde_json::from_str::<DecimalProbe>(black_box(
                r#"{"_price":84520.02}"#,
            )))
        });
    });

    group.bench_function("timestamp", |b| {
        b.iter(|| black_box(black_box("2026-09-25T01:29:06.0656180Z").parse::<Timestamp>()));
    });

    group.bench_function("instrument_lookup", |b| {
        b.iter(|| black_box(instruments.get_cloned(black_box(&symbol_id))));
    });

    group.finish();
}

criterion_group!(benches, bench_decode, bench_atoms);
criterion_main!(benches);

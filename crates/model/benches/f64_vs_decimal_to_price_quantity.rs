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

//! Benchmarks comparing f64 vs Decimal deserialization pipelines to Price/Quantity.
//!
//! Key insight: f64 requires JSON numbers (loses precision >15 digits),
//! Decimal handles both strings and numbers (preserves up to 28 digits).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::serialization::deserialize_decimal;
use nautilus_model::types::{Price, Quantity};
use rust_decimal::Decimal;
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Deserialize)]
struct F64Tick {
    p: f64,
    q: f64,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct DecimalTick {
    #[serde(deserialize_with = "deserialize_decimal")]
    p: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    q: Decimal,
}

/// Compare raw deserialization: f64 (from numbers) vs Decimal (from strings)
fn bench_deser_type(c: &mut Criterion) {
    let mut group = c.benchmark_group("deser_type");

    // f64 requires JSON numbers - already loses precision here
    let json_num = r#"{"p":50000.12345678,"q":1.5}"#;
    // Decimal handles strings - preserves full precision
    let json_str = r#"{"p":"50000.12345678","q":"1.5"}"#;

    group.bench_function("f64_from_number", |b| {
        b.iter(|| serde_json::from_str::<F64Tick>(json_num).unwrap());
    });
    group.bench_function("decimal_from_string", |b| {
        b.iter(|| serde_json::from_str::<DecimalTick>(json_str).unwrap());
    });
    // Also test Decimal from numbers for fair comparison
    group.bench_function("decimal_from_number", |b| {
        b.iter(|| serde_json::from_str::<DecimalTick>(json_num).unwrap());
    });
    group.finish();
}

/// Compare full pipeline: JSON → f64/Decimal → Price + Quantity
fn bench_json_to_price(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_to_price");
    let json_num = r#"{"p":50000.12345678,"q":1.5}"#;
    let json_str = r#"{"p":"50000.12345678","q":"1.5"}"#;
    let precision = 8u8;

    group.bench_function("via_f64", |b| {
        b.iter(|| {
            let t: F64Tick = serde_json::from_str(json_num).unwrap();
            (Price::new(t.p, precision), Quantity::new(t.q, precision))
        });
    });

    group.bench_function("via_decimal", |b| {
        b.iter(|| {
            let t: DecimalTick = serde_json::from_str(json_str).unwrap();
            (
                Price::from_decimal_dp(t.p, precision).unwrap(),
                Quantity::from_decimal_dp(t.q, precision).unwrap(),
            )
        });
    });
    group.finish();
}

/// Batch processing: 100 ticks through full pipeline
fn bench_batch_to_price(c: &mut Criterion) {
    // JSON numbers for f64 path
    let json_num: String = format!(
        "[{}]",
        (0..100)
            .map(|i| {
                format!(
                    r#"{{"p":{}.{:08},"q":{}.{:04}}}"#,
                    50000 + i,
                    i * 12345678 % 100000000,
                    (i % 10) + 1,
                    i * 1234 % 10000
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    );

    // JSON strings for Decimal path (preserves precision)
    let json_str: String = format!(
        "[{}]",
        (0..100)
            .map(|i| {
                format!(
                    r#"{{"p":"{}.{:08}","q":"{}.{:04}"}}"#,
                    50000 + i,
                    i * 12345678 % 100000000,
                    (i % 10) + 1,
                    i * 1234 % 10000
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    );

    let mut group = c.benchmark_group("batch_100_to_price");
    group.throughput(Throughput::Elements(100));

    group.bench_function("via_f64", |b| {
        b.iter(|| {
            serde_json::from_str::<Vec<F64Tick>>(&json_num)
                .unwrap()
                .into_iter()
                .map(|t| (Price::new(t.p, 8), Quantity::new(t.q, 4)))
                .collect::<Vec<_>>()
        });
    });

    group.bench_function("via_decimal", |b| {
        b.iter(|| {
            serde_json::from_str::<Vec<DecimalTick>>(&json_str)
                .unwrap()
                .into_iter()
                .map(|t| {
                    (
                        Price::from_decimal_dp(t.p, 8).unwrap(),
                        Quantity::from_decimal_dp(t.q, 4).unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_deser_type,
    bench_json_to_price,
    bench_batch_to_price
);
criterion_main!(benches);

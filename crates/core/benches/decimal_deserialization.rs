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

//! Benchmarks comparing old `serde_json::Value` approach vs new `DecimalVisitor`.

use std::str::FromStr;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, de::Error};

/// Old approach: allocates intermediate `serde_json::Value`.
fn deserialize_old<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => if s.contains('e') || s.contains('E') {
            Decimal::from_scientific(&s)
        } else {
            Decimal::from_str(&s)
        }
        .map_err(D::Error::custom),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Decimal::from)
            .or_else(|| n.as_f64().and_then(|f| Decimal::try_from(f).ok()))
            .ok_or_else(|| D::Error::custom("invalid number")),
        serde_json::Value::Null => Ok(Decimal::ZERO),
        _ => Err(D::Error::custom("expected decimal")),
    }
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct Old {
    #[serde(deserialize_with = "deserialize_old")]
    v: Decimal,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct New {
    #[serde(deserialize_with = "nautilus_core::serialization::deserialize_decimal")]
    v: Decimal,
}

fn bench_decimal_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("decimal");

    let cases = [
        ("string", r#"{"v":"123.456789012345678"}"#),
        ("integer", r#"{"v":123456789}"#),
        ("float", r#"{"v":123.456}"#),
        ("scientific", r#"{"v":"1.5e-8"}"#),
        ("null", r#"{"v":null}"#),
    ];

    for (name, json) in cases {
        group.bench_with_input(BenchmarkId::new("old", name), &json, |b, j| {
            b.iter(|| serde_json::from_str::<Old>(j).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("new", name), &json, |b, j| {
            b.iter(|| serde_json::from_str::<New>(j).unwrap());
        });
    }
    group.finish();
}

fn bench_realistic_batch(c: &mut Criterion) {
    #[allow(dead_code)]
    #[derive(Deserialize)]
    struct TickOld {
        #[serde(deserialize_with = "deserialize_old")]
        p: Decimal,
        #[serde(deserialize_with = "deserialize_old")]
        q: Decimal,
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    struct TickNew {
        #[serde(deserialize_with = "nautilus_core::serialization::deserialize_decimal")]
        p: Decimal,
        #[serde(deserialize_with = "nautilus_core::serialization::deserialize_decimal")]
        q: Decimal,
    }

    let json: String = format!(
        "[{}]",
        (0..100)
            .map(|i| format!(
                r#"{{"p":"{}.{:08}","q":"{}.{:04}"}}"#,
                50000 + i,
                i * 12345678 % 100000000,
                (i % 10) + 1,
                i * 1234 % 10000
            ))
            .collect::<Vec<_>>()
            .join(",")
    );

    let mut group = c.benchmark_group("batch_100_ticks");
    group.throughput(Throughput::Elements(100));

    group.bench_function("old", |b| {
        b.iter(|| serde_json::from_str::<Vec<TickOld>>(&json).unwrap());
    });
    group.bench_function("new", |b| {
        b.iter(|| serde_json::from_str::<Vec<TickNew>>(&json).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench_decimal_types, bench_realistic_batch);
criterion_main!(benches);

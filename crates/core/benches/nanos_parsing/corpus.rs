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

//! Opaque integer corpus and fallback controls for timestamp parsing.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};
use nautilus_core::UnixNanos;

pub(super) fn bench_integer_corpus(c: &mut Criterion) {
    let mut group = c.benchmark_group("nanos_corpus/integer");

    for width in 16..=19usize {
        let modulus = 10u64.pow(u32::try_from(width).unwrap());
        let inputs: Vec<String> = (0..64u64)
            .map(|seed| {
                let value = if seed.is_multiple_of(8) {
                    seed
                } else {
                    (1_707_578_323_456_789_123 + seed * 982_451_653) % modulus
                };
                format!("{value:0width$}")
            })
            .collect();

        for input in &inputs {
            assert_eq!(input.len(), width);
            assert_eq!(
                input.parse::<UnixNanos>().unwrap().as_u64(),
                input.parse::<u64>().unwrap()
            );
        }
        group.throughput(Throughput::Elements(width as u64));
        group.bench_with_input(BenchmarkId::from_parameter(width), &inputs, |b, inputs| {
            let mut cursor = 0;
            b.iter(|| {
                let input = &inputs[cursor];
                cursor = (cursor + 1) % inputs.len();
                black_box(black_box(input.as_str()).parse::<UnixNanos>().unwrap())
            });
        });
    }
    group.finish();
}

pub(super) fn bench_fallback_controls(c: &mut Criterion) {
    let mut group = c.benchmark_group("nanos_corpus/fallback");

    for (name, input, expected) in [
        ("short_integer", "123", 123),
        ("decimal_seconds", "1.500000000000000", 1_500_000_000),
        ("scientific_seconds", "00000000000015e-1", 1_500_000_000),
        ("signed_integer", "+000000000000123", 123),
        ("u64_max", "18446744073709551615", u64::MAX),
        ("legacy_date", "1970-1-2", 86_400_000_000_000),
        (
            "rfc3339",
            "2024-02-10T15:18:43.456789123Z",
            1_707_578_323_456_789_123,
        ),
    ] {
        assert_eq!(input.parse::<UnixNanos>().unwrap().as_u64(), expected);
        group.bench_with_input(BenchmarkId::from_parameter(name), input, |b, input| {
            b.iter(|| black_box(black_box(input).parse::<UnixNanos>().unwrap()));
        });
    }

    for (name, input, expected) in [
        (
            "numeric_overflow",
            "18446744073709551616",
            "Unix timestamp is out of range",
        ),
        (
            "invalid",
            "123456789012345x",
            "Invalid format: 123456789012345x",
        ),
    ] {
        assert_eq!(
            input.parse::<UnixNanos>().unwrap_err().to_string(),
            expected
        );
        group.bench_with_input(BenchmarkId::from_parameter(name), input, |b, input| {
            b.iter(|| black_box(black_box(input).parse::<UnixNanos>()));
        });
    }
    group.finish();
}

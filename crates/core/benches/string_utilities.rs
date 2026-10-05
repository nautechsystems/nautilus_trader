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

//! Benchmarks for precision parsing, Unicode grouping and credential masking.
//! Fixtures and semantic checks are outside timing; returned allocations are dropped inside it.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_core::string::{
    formatting::Separable,
    parsing::{min_increment_precision_from_str, precision_from_str},
    secret::mask_api_key,
};

fn bench_precision(c: &mut Criterion) {
    let cases = [
        ("integer", "50000", 0, 0),
        ("price", "50000.12340000", 8, 4),
        ("zero_fraction", "1.00000000", 8, 8),
        ("scientific", "1.2300E-8", 12, 10),
        ("padded", "  0.00001000  ", 8, 5),
    ];
    let mut group = c.benchmark_group("core_precision");

    for (name, input, precision, minimum) in cases {
        assert_eq!(precision_from_str(input), precision);
        assert_eq!(min_increment_precision_from_str(input), minimum);
        group.bench_with_input(BenchmarkId::new("precision", name), input, |b, input| {
            b.iter(|| black_box(precision_from_str(black_box(input))));
        });
        group.bench_with_input(BenchmarkId::new("minimum", name), input, |b, input| {
            b.iter(|| black_box(min_increment_precision_from_str(black_box(input))));
        });
    }

    group.finish();
}

fn bench_strings(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_strings");
    let keys = [
        ("short", "short", "*****"),
        ("ascii", "0123456789abcdef0123456789abcdef", "0123...cdef"),
        (
            "unicode",
            "\u{4e00}\u{4e8c}\u{4e09}\u{56db}\u{4e94}\u{516d}\u{4e03}\u{516b}\u{4e5d}\u{5341}\u{7532}\u{4e59}\u{4e19}\u{4e01}",
            "\u{4e00}\u{4e8c}\u{4e09}\u{56db}...\u{7532}\u{4e59}\u{4e19}\u{4e01}",
        ),
    ];

    for (name, key, expected) in keys {
        assert_eq!(mask_api_key(key), expected);
        group.bench_with_input(BenchmarkId::new("mask", name), key, |b, key| {
            b.iter(|| black_box(mask_api_key(black_box(key))));
        });
    }

    for (name, value, expected) in [
        ("integer", "1234567890", "1,234,567,890"),
        ("price", "-1234567890.12340000", "-1,234,567,890.12340000"),
        (
            "unicode",
            "\u{4e00}\u{4e8c}\u{4e09}\u{56db}\u{4e94}\u{516d}\u{4e03}",
            "\u{4e00},\u{4e8c}\u{4e09}\u{56db},\u{4e94}\u{516d}\u{4e03}",
        ),
    ] {
        assert_eq!(value.separate_with_commas(), expected);
        group.bench_with_input(BenchmarkId::new("separate", name), value, |b, value| {
            b.iter(|| black_box(black_box(value).separate_with_commas()));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_precision, bench_strings);
criterion_main!(benches);

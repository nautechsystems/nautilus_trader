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

//! Benchmarks comparing StackStr vs Ustr for identifier workloads.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_core::StackStr;
use ustr::Ustr;

const SHORT: &str = "BINANCE";
const MEDIUM: &str = "O-20231215-001-001";
const LONG: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890"; // 36 chars (StackStr max)
const POLYMARKET: &str = "0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a78";

fn bench_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("creation");

    for (name, s) in [("short", SHORT), ("medium", MEDIUM), ("long", LONG)] {
        group.bench_with_input(BenchmarkId::new("StackStr", name), s, |b, s| {
            b.iter(|| StackStr::new(black_box(s)));
        });
        group.bench_with_input(BenchmarkId::new("Ustr", name), s, |b, s| {
            b.iter(|| Ustr::from(black_box(s)));
        });
    }

    group.bench_with_input(
        BenchmarkId::new("Ustr", "polymarket"),
        POLYMARKET,
        |b, s| {
            b.iter(|| Ustr::from(black_box(s)));
        },
    );

    group.finish();
}

fn bench_equality_same(c: &mut Criterion) {
    let mut group = c.benchmark_group("eq_same");

    for (name, s) in [("short", SHORT), ("medium", MEDIUM), ("long", LONG)] {
        let stack_a = StackStr::new(s);
        let stack_b = StackStr::new(s);
        group.bench_with_input(
            BenchmarkId::new("StackStr", name),
            &(stack_a, stack_b),
            |b, (a, b_val)| {
                b.iter(|| black_box(a) == black_box(b_val));
            },
        );

        let ustr_a = Ustr::from(s);
        let ustr_b = Ustr::from(s);
        group.bench_with_input(
            BenchmarkId::new("Ustr", name),
            &(ustr_a, ustr_b),
            |b, (a, b_val)| {
                b.iter(|| black_box(a) == black_box(b_val));
            },
        );
    }

    let ustr_a = Ustr::from(POLYMARKET);
    let ustr_b = Ustr::from(POLYMARKET);
    group.bench_with_input(
        BenchmarkId::new("Ustr", "polymarket"),
        &(ustr_a, ustr_b),
        |b, (a, b_val)| {
            b.iter(|| black_box(a) == black_box(b_val));
        },
    );

    group.finish();
}

fn bench_equality_different(c: &mut Criterion) {
    let mut group = c.benchmark_group("eq_diff");

    let pairs = [
        ("short", "BINANCE", "POLYGON"),
        ("medium", "O-20231215-001-001", "O-20231215-001-002"),
        (
            "long",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567891",
        ),
    ];

    for (name, s1, s2) in pairs {
        let stack_a = StackStr::new(s1);
        let stack_b = StackStr::new(s2);
        group.bench_with_input(
            BenchmarkId::new("StackStr", name),
            &(stack_a, stack_b),
            |b, (a, b_val)| {
                b.iter(|| black_box(a) == black_box(b_val));
            },
        );

        let ustr_a = Ustr::from(s1);
        let ustr_b = Ustr::from(s2);
        group.bench_with_input(
            BenchmarkId::new("Ustr", name),
            &(ustr_a, ustr_b),
            |b, (a, b_val)| {
                b.iter(|| black_box(a) == black_box(b_val));
            },
        );
    }

    let ustr_a = Ustr::from("0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a78");
    let ustr_b = Ustr::from("0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a79");
    group.bench_with_input(
        BenchmarkId::new("Ustr", "polymarket"),
        &(ustr_a, ustr_b),
        |b, (a, b_val)| {
            b.iter(|| black_box(a) == black_box(b_val));
        },
    );

    group.finish();
}

fn bench_hash(c: &mut Criterion) {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    let mut group = c.benchmark_group("hash");

    for (name, s) in [("short", SHORT), ("medium", MEDIUM), ("long", LONG)] {
        let stack = StackStr::new(s);
        group.bench_with_input(BenchmarkId::new("StackStr", name), &stack, |b, val| {
            b.iter(|| {
                let mut hasher = DefaultHasher::new();
                black_box(val).hash(&mut hasher);
                hasher.finish()
            });
        });

        let ustr = Ustr::from(s);
        group.bench_with_input(BenchmarkId::new("Ustr", name), &ustr, |b, val| {
            b.iter(|| {
                let mut hasher = DefaultHasher::new();
                black_box(val).hash(&mut hasher);
                hasher.finish()
            });
        });
    }

    let ustr = Ustr::from(POLYMARKET);
    group.bench_with_input(BenchmarkId::new("Ustr", "polymarket"), &ustr, |b, val| {
        b.iter(|| {
            let mut hasher = DefaultHasher::new();
            black_box(val).hash(&mut hasher);
            hasher.finish()
        });
    });

    group.finish();
}

fn bench_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("clone");

    for (name, s) in [("short", SHORT), ("medium", MEDIUM), ("long", LONG)] {
        let stack = StackStr::new(s);
        group.bench_with_input(BenchmarkId::new("StackStr", name), &stack, |b, val| {
            b.iter(|| black_box(*val));
        });

        let ustr = Ustr::from(s);
        group.bench_with_input(BenchmarkId::new("Ustr", name), &ustr, |b, val| {
            b.iter(|| black_box(*val));
        });
    }

    group.finish();
}

fn bench_unique_ids(c: &mut Criterion) {
    let mut group = c.benchmark_group("unique_ids");

    let ids: Vec<String> = (0..1000)
        .map(|i| format!("O-20231215-{:03}-{:03}", i / 100, i % 100))
        .collect();

    group.bench_function("StackStr_create_1000", |b| {
        b.iter(|| {
            ids.iter()
                .map(|s| StackStr::new(black_box(s)))
                .collect::<Vec<_>>()
        });
    });

    group.bench_function("Ustr_create_1000", |b| {
        b.iter(|| {
            ids.iter()
                .map(|s| Ustr::from(black_box(s.as_str())))
                .collect::<Vec<_>>()
        });
    });

    let stack_ids: Vec<StackStr> = ids.iter().map(|s| StackStr::new(s)).collect();
    let ustr_ids: Vec<Ustr> = ids.iter().map(|s| Ustr::from(s.as_str())).collect();
    let target_stack = StackStr::new("O-20231215-005-050");
    let target_ustr = Ustr::from("O-20231215-005-050");

    group.bench_function("StackStr_find_in_1000", |b| {
        b.iter(|| stack_ids.iter().find(|id| *id == black_box(&target_stack)));
    });

    group.bench_function("Ustr_find_in_1000", |b| {
        b.iter(|| ustr_ids.iter().find(|id| *id == black_box(&target_ustr)));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_creation,
    bench_equality_same,
    bench_equality_different,
    bench_hash,
    bench_clone,
    bench_unique_ids,
);
criterion_main!(benches);

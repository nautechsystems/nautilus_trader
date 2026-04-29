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

//! Benchmarks comparing `AHashMap` and `IndexMap` for the access patterns in
//! `crates/`: insert, lookup, value iteration, key collection, and remove.
//!
//! Use these numbers when auditing whether a hash collection on the DST path
//! should keep `AHashMap` for performance or flip to `IndexMap` for
//! deterministic iteration order (see `docs/concepts/dst.md`).
//!
//! Map sizes (4, 32, 256) bracket the realistic ranges in production:
//! a few currencies per account, a few dozen subscriptions per client, a few
//! hundred resting orders or cached entries per matching engine.

use std::hint::black_box;

use ahash::AHashMap;
use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use indexmap::IndexMap;

const SIZES: [usize; 3] = [4, 32, 256];

fn keys(n: usize) -> Vec<u64> {
    (0..n as u64).collect()
}

fn populated_ahash(n: usize) -> AHashMap<u64, u64> {
    let mut map = AHashMap::with_capacity(n);
    for k in 0..n as u64 {
        map.insert(k, k);
    }
    map
}

fn populated_index(n: usize) -> IndexMap<u64, u64> {
    let mut map = IndexMap::with_capacity(n);
    for k in 0..n as u64 {
        map.insert(k, k);
    }
    map
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert");

    for &size in &SIZES {
        group.bench_with_input(BenchmarkId::new("AHashMap", size), &size, |b, &n| {
            b.iter(|| {
                let mut map = AHashMap::with_capacity(n);
                for k in 0..n as u64 {
                    map.insert(k, k);
                }
                black_box(map);
            });
        });

        group.bench_with_input(BenchmarkId::new("IndexMap", size), &size, |b, &n| {
            b.iter(|| {
                let mut map = IndexMap::with_capacity(n);
                for k in 0..n as u64 {
                    map.insert(k, k);
                }
                black_box(map);
            });
        });
    }
}

fn bench_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("lookup");

    for &size in &SIZES {
        let ks = keys(size);

        let ahash = populated_ahash(size);
        group.bench_with_input(BenchmarkId::new("AHashMap", size), &size, |b, _| {
            let mut idx = 0usize;
            b.iter(|| {
                let v = ahash.get(&ks[idx % ks.len()]).copied();
                black_box(v);
                idx = idx.wrapping_add(1);
            });
        });

        let index = populated_index(size);
        group.bench_with_input(BenchmarkId::new("IndexMap", size), &size, |b, _| {
            let mut idx = 0usize;
            b.iter(|| {
                let v = index.get(&ks[idx % ks.len()]).copied();
                black_box(v);
                idx = idx.wrapping_add(1);
            });
        });
    }
}

fn bench_iter_values(c: &mut Criterion) {
    let mut group = c.benchmark_group("iter_values_collect");

    for &size in &SIZES {
        let ahash = populated_ahash(size);
        group.bench_with_input(BenchmarkId::new("AHashMap", size), &size, |b, _| {
            b.iter(|| {
                let v: Vec<u64> = ahash.values().copied().collect();
                black_box(v);
            });
        });

        let index = populated_index(size);
        group.bench_with_input(BenchmarkId::new("IndexMap", size), &size, |b, _| {
            b.iter(|| {
                let v: Vec<u64> = index.values().copied().collect();
                black_box(v);
            });
        });
    }
}

fn bench_keys_collect(c: &mut Criterion) {
    let mut group = c.benchmark_group("keys_collect");

    for &size in &SIZES {
        let ahash = populated_ahash(size);
        group.bench_with_input(BenchmarkId::new("AHashMap", size), &size, |b, _| {
            b.iter(|| {
                let v: Vec<u64> = ahash.keys().copied().collect();
                black_box(v);
            });
        });

        let index = populated_index(size);
        group.bench_with_input(BenchmarkId::new("IndexMap", size), &size, |b, _| {
            b.iter(|| {
                let v: Vec<u64> = index.keys().copied().collect();
                black_box(v);
            });
        });
    }
}

fn bench_remove(c: &mut Criterion) {
    let mut group = c.benchmark_group("remove_one");

    for &size in &SIZES {
        let ks = keys(size);
        let target = ks[size / 2];

        group.bench_with_input(BenchmarkId::new("AHashMap.remove", size), &size, |b, &n| {
            b.iter_batched(
                || populated_ahash(n),
                |mut map| {
                    let v = map.remove(&target);
                    black_box(v);
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(
            BenchmarkId::new("IndexMap.shift_remove", size),
            &size,
            |b, &n| {
                b.iter_batched(
                    || populated_index(n),
                    |mut map| {
                        let v = map.shift_remove(&target);
                        black_box(v);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("IndexMap.swap_remove", size),
            &size,
            |b, &n| {
                b.iter_batched(
                    || populated_index(n),
                    |mut map| {
                        let v = map.swap_remove(&target);
                        black_box(v);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
}

fn bench_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("clone");

    for &size in &SIZES {
        let ahash = populated_ahash(size);
        group.bench_with_input(BenchmarkId::new("AHashMap", size), &size, |b, _| {
            b.iter(|| {
                let cloned = ahash.clone();
                black_box(cloned);
            });
        });

        let index = populated_index(size);
        group.bench_with_input(BenchmarkId::new("IndexMap", size), &size, |b, _| {
            b.iter(|| {
                let cloned = index.clone();
                black_box(cloned);
            });
        });
    }
}

fn bench_entry_accumulate(c: &mut Criterion) {
    let mut group = c.benchmark_group("entry_accumulate");

    // Mirrors the BaseAccount::update_commissions pattern: repeated
    // entry().and_modify().or_insert() under a small key set
    let cycle: Vec<u64> = vec![0, 1, 2, 0, 1, 0];

    for &size in &SIZES {
        group.bench_with_input(BenchmarkId::new("AHashMap", size), &size, |b, &n| {
            b.iter(|| {
                let mut map: AHashMap<u64, u64> = AHashMap::with_capacity(3);

                for _ in 0..n {
                    for &k in &cycle {
                        map.entry(k).and_modify(|v| *v += 1).or_insert(1);
                    }
                }
                black_box(map);
            });
        });

        group.bench_with_input(BenchmarkId::new("IndexMap", size), &size, |b, &n| {
            b.iter(|| {
                let mut map: IndexMap<u64, u64> = IndexMap::with_capacity(3);

                for _ in 0..n {
                    for &k in &cycle {
                        map.entry(k).and_modify(|v| *v += 1).or_insert(1);
                    }
                }
                black_box(map);
            });
        });
    }
}

criterion_group!(
    benches,
    bench_insert,
    bench_lookup,
    bench_iter_values,
    bench_keys_collect,
    bench_remove,
    bench_clone,
    bench_entry_accumulate,
);
criterion_main!(benches);

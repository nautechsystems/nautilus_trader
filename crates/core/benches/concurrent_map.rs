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

//! Benchmarks comparing `Arc<DashMap>`, `Arc<RwLock<AHashMap>>`, and `Arc<AtomicMap>`
//! for read-heavy concurrent access patterns.
//!
//! Hardware: AMD Ryzen 9 7950X (16C/32T), 128 GB RAM, Linux 6.8, rustc 1.94.0.
//!
//! Results (10k reads/thread, barrier-synced, String keys, u64 values):
//!
//! ```text
//! Single-threaded read (per-lookup, no contention)
//! ┌──────────┬──────────┬──────────┬───────────┐
//! │ Map size │ DashMap  │ RwLock   │ AtomicMap │
//! ├──────────┼──────────┼──────────┼───────────┤
//! │ 100      │ 16.3 ns  │  7.7 ns  │   9.3 ns  │
//! │ 1000     │ 17.5 ns  │  8.5 ns  │  10.3 ns  │
//! └──────────┴──────────┴──────────┴───────────┘
//!
//! Concurrent reads (100 entries)
//! ┌──────────┬──────────┬──────────┬───────────┐
//! │ Threads  │ DashMap  │ RwLock   │ AtomicMap │
//! ├──────────┼──────────┼──────────┼───────────┤
//! │  4       │  899 us  │  1.7 ms  │   181 us  │
//! │  8       │  1.8 ms  │  4.2 ms  │   244 us  │
//! │ 16       │  2.4 ms  │ 11.4 ms  │   445 us  │
//! └──────────┴──────────┴──────────┴───────────┘
//!
//! Write-once read-many (1000 entries)
//! ┌──────────┬──────────┬──────────┬───────────┐
//! │ Threads  │ DashMap  │ RwLock   │ AtomicMap │
//! ├──────────┼──────────┼──────────┼───────────┤
//! │  4       │  1.1 ms  │  2.0 ms  │   183 us  │
//! │  8       │  1.2 ms  │  4.6 ms  │   246 us  │
//! │ 16       │  2.5 ms  │  7.3 ms  │   443 us  │
//! └──────────┴──────────┴──────────┴───────────┘
//! ```

use std::{
    hint::black_box,
    sync::{Arc, Barrier, RwLock},
};

use ahash::AHashMap;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dashmap::DashMap;
use nautilus_core::AtomicMap;

const MAP_SIZES: [usize; 2] = [100, 1_000];
const THREAD_COUNTS: [usize; 4] = [1, 4, 8, 16];
const READS_PER_THREAD: usize = 10_000;
const WRITES_PER_CYCLE: usize = 10;

fn make_keys(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("BTCUSDT.BINANCE-{i:04}")).collect()
}

fn populated_dashmap(keys: &[String]) -> Arc<DashMap<String, u64>> {
    let map = DashMap::with_capacity(keys.len());
    for (i, key) in keys.iter().enumerate() {
        map.insert(key.clone(), i as u64);
    }
    Arc::new(map)
}

fn populated_rwlock(keys: &[String]) -> Arc<RwLock<AHashMap<String, u64>>> {
    let mut map = AHashMap::with_capacity(keys.len());
    for (i, key) in keys.iter().enumerate() {
        map.insert(key.clone(), i as u64);
    }
    Arc::new(RwLock::new(map))
}

fn populated_atomic_map(keys: &[String]) -> Arc<AtomicMap<String, u64>> {
    let mut map = AHashMap::with_capacity(keys.len());
    for (i, key) in keys.iter().enumerate() {
        map.insert(key.clone(), i as u64);
    }
    Arc::new(AtomicMap::from(map))
}

fn bench_single_thread_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_thread_read");

    for &size in &MAP_SIZES {
        let keys = make_keys(size);

        let dash = populated_dashmap(&keys);
        group.bench_with_input(BenchmarkId::new("DashMap", size), &size, |b, _| {
            let mut idx = 0usize;
            b.iter(|| {
                let key = &keys[idx % keys.len()];
                let val = dash.get(key).map(|r| *r.value());
                black_box(val);
                idx = idx.wrapping_add(1);
            });
        });

        let rwl = populated_rwlock(&keys);
        group.bench_with_input(BenchmarkId::new("RwLockAHashMap", size), &size, |b, _| {
            let mut idx = 0usize;
            b.iter(|| {
                let key = &keys[idx % keys.len()];
                let guard = rwl.read().unwrap();
                let val = guard.get(key).copied();
                drop(guard);
                black_box(val);
                idx = idx.wrapping_add(1);
            });
        });

        let atomic = populated_atomic_map(&keys);
        group.bench_with_input(BenchmarkId::new("AtomicMap", size), &size, |b, _| {
            let mut idx = 0usize;
            b.iter(|| {
                let key = &keys[idx % keys.len()];
                let val = atomic.load().get(key).copied();
                black_box(val);
                idx = idx.wrapping_add(1);
            });
        });
    }
    group.finish();
}

fn bench_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_reads");

    for &size in &MAP_SIZES {
        let keys = make_keys(size);

        for &threads in &THREAD_COUNTS {
            let param = format!("{size}_entries/{threads}_threads");

            let dash = populated_dashmap(&keys);
            let keys_arc = Arc::new(keys.clone());
            group.bench_with_input(BenchmarkId::new("DashMap", &param), &threads, |b, _| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    std::thread::scope(|s| {
                        for t in 0..threads {
                            let map = Arc::clone(&dash);
                            let ks = Arc::clone(&keys_arc);
                            let bar = Arc::clone(&barrier);
                            s.spawn(move || {
                                bar.wait();

                                for i in 0..READS_PER_THREAD {
                                    let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                    black_box(map.get(key).map(|r| *r.value()));
                                }
                            });
                        }
                    });
                });
            });

            let rwl = populated_rwlock(&keys);
            let keys_arc = Arc::new(keys.clone());
            group.bench_with_input(
                BenchmarkId::new("RwLockAHashMap", &param),
                &threads,
                |b, _| {
                    b.iter(|| {
                        let barrier = Arc::new(Barrier::new(threads));
                        std::thread::scope(|s| {
                            for t in 0..threads {
                                let map = Arc::clone(&rwl);
                                let ks = Arc::clone(&keys_arc);
                                let bar = Arc::clone(&barrier);
                                s.spawn(move || {
                                    bar.wait();

                                    for i in 0..READS_PER_THREAD {
                                        let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                        let guard = map.read().unwrap();
                                        black_box(guard.get(key).copied());
                                        drop(guard);
                                    }
                                });
                            }
                        });
                    });
                },
            );

            let atomic = populated_atomic_map(&keys);
            let keys_arc = Arc::new(keys.clone());
            group.bench_with_input(BenchmarkId::new("AtomicMap", &param), &threads, |b, _| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    std::thread::scope(|s| {
                        for t in 0..threads {
                            let map = Arc::clone(&atomic);
                            let ks = Arc::clone(&keys_arc);
                            let bar = Arc::clone(&barrier);
                            s.spawn(move || {
                                bar.wait();

                                for i in 0..READS_PER_THREAD {
                                    let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                    let guard = map.load();
                                    black_box(guard.get(key).copied());
                                }
                            });
                        }
                    });
                });
            });
        }
    }
    group.finish();
}

fn bench_read_heavy_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_heavy_mixed");

    for &size in &MAP_SIZES {
        let keys = make_keys(size);
        let write_keys: Vec<String> = (0..WRITES_PER_CYCLE)
            .map(|i| format!("WRITE-KEY-{i:04}"))
            .collect();

        for &threads in &THREAD_COUNTS {
            if threads < 2 {
                continue;
            }
            let readers = threads - 1;
            let param = format!("{size}_entries/{readers}r_1w");

            let dash = populated_dashmap(&keys);
            let keys_arc = Arc::new(keys.clone());
            let wk_arc = Arc::new(write_keys.clone());
            group.bench_with_input(BenchmarkId::new("DashMap", &param), &threads, |b, _| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    std::thread::scope(|s| {
                        let map = Arc::clone(&dash);
                        let wk = Arc::clone(&wk_arc);
                        let bar = Arc::clone(&barrier);
                        s.spawn(move || {
                            bar.wait();

                            for (i, key) in wk.iter().enumerate() {
                                map.insert(key.clone(), (size + i) as u64);
                            }

                            for key in wk.iter() {
                                map.remove(key);
                            }
                        });

                        for t in 0..readers {
                            let map = Arc::clone(&dash);
                            let ks = Arc::clone(&keys_arc);
                            let bar = Arc::clone(&barrier);
                            s.spawn(move || {
                                bar.wait();

                                for i in 0..READS_PER_THREAD {
                                    let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                    black_box(map.get(key).map(|r| *r.value()));
                                }
                            });
                        }
                    });
                });
            });

            let rwl = populated_rwlock(&keys);
            let keys_arc = Arc::new(keys.clone());
            let wk_arc = Arc::new(write_keys.clone());
            group.bench_with_input(
                BenchmarkId::new("RwLockAHashMap", &param),
                &threads,
                |b, _| {
                    b.iter(|| {
                        let barrier = Arc::new(Barrier::new(threads));
                        std::thread::scope(|s| {
                            let map = Arc::clone(&rwl);
                            let wk = Arc::clone(&wk_arc);
                            let bar = Arc::clone(&barrier);
                            s.spawn(move || {
                                bar.wait();

                                for (i, key) in wk.iter().enumerate() {
                                    let mut guard = map.write().unwrap();
                                    guard.insert(key.clone(), (size + i) as u64);
                                    drop(guard);
                                }

                                for key in wk.iter() {
                                    let mut guard = map.write().unwrap();
                                    guard.remove(key);
                                    drop(guard);
                                }
                            });

                            for t in 0..readers {
                                let map = Arc::clone(&rwl);
                                let ks = Arc::clone(&keys_arc);
                                let bar = Arc::clone(&barrier);
                                s.spawn(move || {
                                    bar.wait();

                                    for i in 0..READS_PER_THREAD {
                                        let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                        let guard = map.read().unwrap();
                                        black_box(guard.get(key).copied());
                                        drop(guard);
                                    }
                                });
                            }
                        });
                    });
                },
            );

            let atomic = populated_atomic_map(&keys);
            let keys_arc = Arc::new(keys.clone());
            let wk_arc = Arc::new(write_keys.clone());
            group.bench_with_input(BenchmarkId::new("AtomicMap", &param), &threads, |b, _| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    std::thread::scope(|s| {
                        let map = Arc::clone(&atomic);
                        let wk = Arc::clone(&wk_arc);
                        let bar = Arc::clone(&barrier);
                        s.spawn(move || {
                            bar.wait();
                            map.rcu(|m| {
                                for (i, key) in wk.iter().enumerate() {
                                    m.insert(key.clone(), (size + i) as u64);
                                }
                            });
                            map.rcu(|m| {
                                for key in wk.iter() {
                                    m.remove(key);
                                }
                            });
                        });

                        for t in 0..readers {
                            let map = Arc::clone(&atomic);
                            let ks = Arc::clone(&keys_arc);
                            let bar = Arc::clone(&barrier);
                            s.spawn(move || {
                                bar.wait();

                                for i in 0..READS_PER_THREAD {
                                    let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                    let guard = map.load();
                                    black_box(guard.get(key).copied());
                                }
                            });
                        }
                    });
                });
            });
        }
    }
    group.finish();
}

fn bench_write_once_read_many(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_once_read_many");

    for &size in &MAP_SIZES {
        let keys = make_keys(size);

        for &threads in &THREAD_COUNTS {
            let param = format!("{size}_entries/{threads}_threads");

            let dash = populated_dashmap(&keys);
            let keys_arc = Arc::new(keys.clone());
            group.bench_with_input(BenchmarkId::new("DashMap", &param), &threads, |b, _| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    std::thread::scope(|s| {
                        for t in 0..threads {
                            let map = Arc::clone(&dash);
                            let ks = Arc::clone(&keys_arc);
                            let bar = Arc::clone(&barrier);
                            s.spawn(move || {
                                bar.wait();

                                for i in 0..READS_PER_THREAD {
                                    let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                    black_box(map.get(key).map(|r| *r.value()));
                                }
                            });
                        }
                    });
                });
            });

            let rwl = populated_rwlock(&keys);
            let keys_arc = Arc::new(keys.clone());
            group.bench_with_input(
                BenchmarkId::new("RwLockAHashMap", &param),
                &threads,
                |b, _| {
                    b.iter(|| {
                        let barrier = Arc::new(Barrier::new(threads));
                        std::thread::scope(|s| {
                            for t in 0..threads {
                                let map = Arc::clone(&rwl);
                                let ks = Arc::clone(&keys_arc);
                                let bar = Arc::clone(&barrier);
                                s.spawn(move || {
                                    bar.wait();

                                    for i in 0..READS_PER_THREAD {
                                        let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                        let guard = map.read().unwrap();
                                        black_box(guard.get(key).copied());
                                        drop(guard);
                                    }
                                });
                            }
                        });
                    });
                },
            );

            let atomic = populated_atomic_map(&keys);
            let keys_arc = Arc::new(keys.clone());
            group.bench_with_input(BenchmarkId::new("AtomicMap", &param), &threads, |b, _| {
                b.iter(|| {
                    let barrier = Arc::new(Barrier::new(threads));
                    std::thread::scope(|s| {
                        for t in 0..threads {
                            let map = Arc::clone(&atomic);
                            let ks = Arc::clone(&keys_arc);
                            let bar = Arc::clone(&barrier);
                            s.spawn(move || {
                                bar.wait();

                                for i in 0..READS_PER_THREAD {
                                    let key = &ks[(t * READS_PER_THREAD + i) % ks.len()];
                                    let guard = map.load();
                                    black_box(guard.get(key).copied());
                                }
                            });
                        }
                    });
                });
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_single_thread_read,
    bench_concurrent_reads,
    bench_read_heavy_mixed,
    bench_write_once_read_many,
);
criterion_main!(benches);

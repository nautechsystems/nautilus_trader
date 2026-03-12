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

use std::{hint::black_box, num::NonZeroU32, sync::Arc};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_network::ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota};

fn make_limiter(rate: u32) -> RateLimiter<String, MonotonicClock> {
    let quota = Quota::per_second(NonZeroU32::new(rate).unwrap()).unwrap();
    RateLimiter::new_with_quota(Some(quota), vec![])
}

fn bench_check_key_uncontended(c: &mut Criterion) {
    let mut group = c.benchmark_group("ratelimiter/check_key_uncontended");

    // High rate so we never block in the benchmark loop
    let limiter = make_limiter(1_000_000_000);
    let key = "endpoint".to_string();

    group.bench_function("single_key", |b| {
        b.iter(|| black_box(limiter.check_key(black_box(&key))));
    });

    group.finish();
}

fn bench_check_key_contended(c: &mut Criterion) {
    let mut group = c.benchmark_group("ratelimiter/check_key_contended");

    for num_threads in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("threads", num_threads),
            &num_threads,
            |b, &num_threads| {
                let limiter = Arc::new(make_limiter(1_000_000_000));
                let key = "hot_key".to_string();

                b.iter(|| {
                    std::thread::scope(|s| {
                        for _ in 0..num_threads {
                            let limiter = &limiter;
                            let key = &key;
                            s.spawn(move || {
                                for _ in 0..100 {
                                    let _ = black_box(limiter.check_key(key));
                                }
                            });
                        }
                    });
                });
            },
        );
    }

    group.finish();
}

fn bench_await_keys_ready(c: &mut Criterion) {
    let mut group = c.benchmark_group("ratelimiter/await_keys_ready");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();

    let limiter = make_limiter(1_000_000_000);

    for num_keys in [0, 1, 2, 3, 5] {
        let keys: Vec<String> = (0..num_keys).map(|i| format!("key_{i}")).collect();

        group.bench_with_input(BenchmarkId::new("keys", num_keys), &keys, |b, keys| {
            b.iter(|| {
                rt.block_on(async {
                    limiter
                        .await_keys_ready(black_box(Some(keys.as_slice())))
                        .await;
                });
            });
        });
    }

    group.bench_function("keys/none", |b| {
        b.iter(|| {
            rt.block_on(async {
                limiter.await_keys_ready(black_box(None)).await;
            });
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_check_key_uncontended,
    bench_check_key_contended,
    bench_await_keys_ready,
);
criterion_main!(benches);

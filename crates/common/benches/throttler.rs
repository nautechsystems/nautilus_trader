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

//! Criterion coverage for `Throttler` hot paths used by risk command rate limits.

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    clock::TestClock,
    throttler::{RateLimit, Throttler},
};
use ustr::Ustr;

const INTERVAL_NS: u64 = 1_000_000_000;
const LIMIT: usize = 100;
const HIGH_RATE_MESSAGES: usize = 10_000;
const WINDOW_SIZES: [usize; 4] = [10, 100, 1_000, 10_000];

type BenchThrottler = Throttler<u64, fn(u64)>;

fn consume_message(msg: u64) {
    black_box(msg);
}

fn make_throttler(limit: usize, buffered: bool, actor_id: &str) -> BenchThrottler {
    let clock = Rc::new(RefCell::new(TestClock::new()));
    make_throttler_with_clock(limit, buffered, actor_id, clock)
}

fn make_throttler_with_clock(
    limit: usize,
    buffered: bool,
    actor_id: &str,
    clock: Rc<RefCell<TestClock>>,
) -> BenchThrottler {
    let output_drop = (!buffered).then_some(consume_message as fn(u64));

    Throttler::new(
        RateLimit::new(limit, INTERVAL_NS),
        clock,
        "throttler_bench",
        consume_message as fn(u64),
        output_drop,
        Ustr::from(actor_id),
    )
}

fn fill_window(throttler: &mut BenchThrottler, limit: usize) {
    for msg in 0..limit {
        throttler.send(black_box(msg as u64));
    }
}

fn make_full_slid_window(limit: usize) -> BenchThrottler {
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let mut throttler =
        make_throttler_with_clock(limit, true, "throttler-bench-full-slid", Rc::clone(&clock));

    fill_window(&mut throttler, limit);
    clock
        .borrow_mut()
        .advance_time((INTERVAL_NS + 1).into(), true);

    throttler
}

fn make_used_window(limit: usize, used: usize) -> BenchThrottler {
    let mut throttler = make_throttler(limit, true, "throttler-bench-used");
    fill_window(&mut throttler, used);
    throttler
}

fn make_full_active_window(limit: usize, buffered: bool) -> BenchThrottler {
    let mut throttler = make_throttler(limit, buffered, "throttler-bench-full-active");
    fill_window(&mut throttler, limit);
    throttler
}

fn bench_send(c: &mut Criterion) {
    let mut group = c.benchmark_group("throttler/send");
    group.throughput(Throughput::Elements(1));

    // Measures the admitted send path when the timestamp deque is full but the
    // previous window has already slid, matching steady traffic after warm-up.
    group.bench_function("admitted_full_window_slid", |b| {
        b.iter_batched(
            || make_full_slid_window(LIMIT),
            |mut throttler| throttler.send(black_box(42)),
            BatchSize::SmallInput,
        );
    });

    // Measures the first overflow in buffered mode: enqueue plus timer registration.
    group.bench_function("buffered_over_limit", |b| {
        b.iter_batched(
            || make_full_active_window(LIMIT, true),
            |mut throttler| throttler.send(black_box(42)),
            BatchSize::SmallInput,
        );
    });

    // Measures the first overflow in drop mode: drop callback plus timer registration.
    group.bench_function("dropping_over_limit", |b| {
        b.iter_batched(
            || make_full_active_window(LIMIT, false),
            |mut throttler| throttler.send(black_box(42)),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_try_reserve(c: &mut Criterion) {
    let mut group = c.benchmark_group("throttler/try_reserve");
    group.throughput(Throughput::Elements(1));

    // Measures the risk-engine batch reservation path with enough capacity.
    group.bench_function("accept_batch", |b| {
        b.iter_batched(
            || make_used_window(LIMIT, LIMIT / 2),
            |mut throttler| black_box(throttler.try_reserve(10)),
            BatchSize::SmallInput,
        );
    });

    // Measures a rejection before the window is full enough to arm a timer.
    group.bench_function("reject_batch_without_timer", |b| {
        b.iter_batched(
            || make_used_window(LIMIT, LIMIT / 2),
            |mut throttler| black_box(throttler.try_reserve(LIMIT)),
            BatchSize::SmallInput,
        );
    });

    // Measures a rejection from a full active window, including timer registration.
    group.bench_function("reject_batch_with_timer", |b| {
        b.iter_batched(
            || make_full_active_window(LIMIT, true),
            |mut throttler| black_box(throttler.try_reserve(1)),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_used_count_in_window(c: &mut Criterion) {
    let mut group = c.benchmark_group("throttler/count_in_window");

    for limit in WINDOW_SIZES {
        group.throughput(Throughput::Elements(limit as u64));
        group.bench_with_input(BenchmarkId::from_parameter(limit), &limit, |b, &limit| {
            b.iter_batched(
                || make_used_window(limit, limit),
                |throttler| black_box(throttler.used()),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_timer_registration(c: &mut Criterion) {
    let mut group = c.benchmark_group("throttler/timer_registration");
    group.throughput(Throughput::Elements(1));

    // Measures the buffered limiting path that creates a process callback and
    // registers a timer on the shared clock.
    group.bench_function("buffered", |b| {
        b.iter_batched(
            || make_full_active_window(LIMIT, true),
            |mut throttler| throttler.send(black_box(42)),
            BatchSize::SmallInput,
        );
    });

    // Measures the drop-mode limiting path that creates a resume callback and
    // registers a timer on the shared clock.
    group.bench_function("dropping", |b| {
        b.iter_batched(
            || make_full_active_window(LIMIT, false),
            |mut throttler| throttler.send(black_box(42)),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_high_message_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("throttler/high_message_rate");
    group.throughput(Throughput::Elements(HIGH_RATE_MESSAGES as u64));

    // Measures sustained admitted traffic below the configured limit.
    group.bench_function("admitted", |b| {
        b.iter_batched(
            || make_throttler(HIGH_RATE_MESSAGES + 1, true, "throttler-bench-high-admit"),
            |mut throttler| {
                for msg in 0..HIGH_RATE_MESSAGES {
                    throttler.send(black_box(msg as u64));
                }
            },
            BatchSize::SmallInput,
        );
    });

    // Measures high-rate overflow when messages are buffered after the first limit.
    group.bench_function("buffered_over_limit", |b| {
        b.iter_batched(
            || make_throttler(LIMIT, true, "throttler-bench-high-buffered"),
            |mut throttler| {
                for msg in 0..HIGH_RATE_MESSAGES {
                    throttler.send(black_box(msg as u64));
                }
            },
            BatchSize::SmallInput,
        );
    });

    // Measures high-rate overflow when messages are dropped after the first limit.
    group.bench_function("dropping_over_limit", |b| {
        b.iter_batched(
            || make_throttler(LIMIT, false, "throttler-bench-high-dropping"),
            |mut throttler| {
                for msg in 0..HIGH_RATE_MESSAGES {
                    throttler.send(black_box(msg as u64));
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_send,
    bench_try_reserve,
    bench_used_count_in_window,
    bench_timer_registration,
    bench_high_message_rate
);
criterion_main!(benches);

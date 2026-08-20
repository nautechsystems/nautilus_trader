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

//! LiveNode queue monitor benchmarks.
//!
//! The steady cases measure one five-channel maintenance-tick evaluation without crossings. The
//! trigger case measures the worst-case evaluation that emits both transitions for every channel.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_live::node::{RunnerChannelMetricsSnapshot, RunnerMetricsSnapshot};

mod metrics {
    pub(crate) use nautilus_live::node::{RunnerMetricsDelta, RunnerMetricsSnapshot};
}

#[allow(dead_code, unreachable_pub)]
#[path = "../src/node/queue.rs"]
mod queue;

use queue::{QueueMonitor, QueueMonitorConfig};

const PROFILE_TICK_COUNT: u64 = 1_024;

fn bench_evaluate(c: &mut Criterion) {
    let mut group = c.benchmark_group("LiveNode queue monitor/evaluate");
    group.throughput(Throughput::Elements(queue::SYSTEM_CHANNELS.len() as u64));

    bench_case(
        &mut group,
        "steady",
        &config(1_000, 500, 1_000, 500),
        snapshot(1, 100, 1),
    );
    bench_case(
        &mut group,
        "all_channels_dual_trigger",
        &config(10, 5, 100, 50),
        snapshot(1, 100, 10),
    );

    group.finish();
}

fn bench_evaluate_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("LiveNode queue monitor/evaluate_batch");
    group.throughput(Throughput::Elements(
        queue::SYSTEM_CHANNELS.len() as u64 * PROFILE_TICK_COUNT,
    ));

    let config = config(1_000, 500, 1_000, 500);
    let snapshots = (1..=PROFILE_TICK_COUNT)
        .map(|tick| snapshot(tick, tick * 100, 1))
        .collect::<Vec<_>>();

    group.bench_function("steady_1024_ticks", |b| {
        b.iter(|| {
            let mut monitor = QueueMonitor::new(&config, RunnerMetricsSnapshot::default());

            for snapshot in snapshots.iter().copied() {
                black_box(monitor.evaluate(black_box(snapshot)));
            }
        });
    });

    group.finish();
}

fn bench_case(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    config: &QueueMonitorConfig,
    snapshot: RunnerMetricsSnapshot,
) {
    group.bench_function(name, |b| {
        b.iter_batched(
            || QueueMonitor::new(config, RunnerMetricsSnapshot::default()),
            |mut monitor| black_box(monitor.evaluate(black_box(snapshot))),
            BatchSize::SmallInput,
        );
    });
}

fn config(
    queue_depth_trigger: usize,
    queue_depth_clear: usize,
    mean_dispatch_ns_trigger: u64,
    mean_dispatch_ns_clear: u64,
) -> QueueMonitorConfig {
    QueueMonitorConfig {
        queue_depth_trigger,
        queue_depth_clear,
        mean_dispatch_ns_trigger,
        mean_dispatch_ns_clear,
    }
}

fn snapshot(dispatched: u64, dispatch_busy_ns: u64, queue_depth: usize) -> RunnerMetricsSnapshot {
    let channel = channel_snapshot(dispatched, dispatch_busy_ns, queue_depth);
    let mut snapshot = RunnerMetricsSnapshot::default();
    snapshot.time_events = channel;
    snapshot.exec_events = channel;
    snapshot.exec_commands = channel;
    snapshot.data_events = channel;
    snapshot.data_commands = channel;
    snapshot.dispatch_busy_ns = dispatch_busy_ns * 5;
    snapshot.elapsed_ns = 100_000_000;
    snapshot
}

fn channel_snapshot(
    dispatched: u64,
    dispatch_busy_ns: u64,
    queue_depth: usize,
) -> RunnerChannelMetricsSnapshot {
    let mut snapshot = RunnerChannelMetricsSnapshot::default();
    snapshot.dispatched = dispatched;
    snapshot.dispatch_busy_ns = dispatch_busy_ns;
    snapshot.queue_depth = queue_depth;
    snapshot.last_dispatch_at_ns = 100_000_000;
    snapshot
}

criterion_group!(benches, bench_evaluate, bench_evaluate_batch);
criterion_main!(benches);

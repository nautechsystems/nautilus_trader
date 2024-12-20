// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{criterion_group, criterion_main, Criterion};
use nautilus_core::time::{duration_since_unix_epoch, nanos_since_unix_epoch};

// Benchmark `duration_since_unix_epoch` (SystemTime under the hood)
fn bench_system_time(c: &mut Criterion) {
    c.bench_function("duration_since_unix_epoch", |b| {
        b.iter(|| duration_since_unix_epoch());
    });
}

// Benchmark `nanos_since_unix_epoch` (libc syscall under the hood)
fn bench_rdtscp(c: &mut Criterion) {
    c.bench_function("nanos_since_unix_epoch", |b| {
        b.iter(|| nanos_since_unix_epoch());
    });
}

// Group benchmarks
criterion_group!(benches, bench_system_time, bench_rdtscp);
criterion_main!(benches);

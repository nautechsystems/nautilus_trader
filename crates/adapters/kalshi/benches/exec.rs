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

//! Canonical execution benches.
//!
//! `submit_rsa_pss` and `submit_ed25519` measure order construction, body encoding, and request
//! signing. Key parsing stays outside the timed region. `fill_tracked` and `fill_external`
//! measure report construction plus event dispatch through the private execution state.
//! Dispatch rows use `iter_batched_ref`, so dropping that state stays outside the timed region.

mod common;

use std::hint::black_box;

use common::{DispatchReady, credential, limit_order, live_instrument, sign_submit};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

fn bench_submit(c: &mut Criterion, rsa: bool, name: &str) {
    let signer = credential(rsa);
    let order = limit_order(&live_instrument());

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function(name, |b| {
        b.iter(|| {
            let signed = sign_submit(black_box(&signer), black_box(&order));
            black_box(signed);
        });
    });

    group.finish();
}

fn bench_submit_rsa_pss(c: &mut Criterion) {
    bench_submit(c, true, "submit_rsa_pss");
}

fn bench_submit_ed25519(c: &mut Criterion) {
    bench_submit(c, false, "submit_ed25519");
}

fn bench_fill_tracked(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("fill_tracked", |b| {
        b.iter_batched_ref(
            DispatchReady::tracked,
            |ready| {
                ready.dispatch();
                black_box(ready);
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_fill_external(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("fill_external", |b| {
        b.iter_batched_ref(
            DispatchReady::external,
            |ready| {
                ready.dispatch();
                black_box(ready);
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    exec,
    bench_submit_rsa_pss,
    bench_submit_ed25519,
    bench_fill_tracked,
    bench_fill_external
);
criterion_main!(exec);

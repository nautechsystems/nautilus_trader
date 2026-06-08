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

//! Field arithmetic benches: `Fp` and `Fp5` mul / square / invert.
//!
//! These primitives are called millions of times per signature; criterion
//! timing here is noisy at the per-op level, so the iai harness in
//! `signing_field_iai.rs` is the regression gate.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

mod common;
use common::{fp_inputs, fp5_inputs};

fn bench_fp_mul(c: &mut Criterion) {
    let (a, b_) = fp_inputs();
    c.bench_function("Fp::mul", |b| {
        b.iter(|| black_box(a) * black_box(b_));
    });
}

fn bench_fp_square(c: &mut Criterion) {
    let (a, _) = fp_inputs();
    c.bench_function("Fp::square", |b| {
        b.iter(|| black_box(a).square());
    });
}

fn bench_fp_invert(c: &mut Criterion) {
    let (a, _) = fp_inputs();
    c.bench_function("Fp::invert", |b| {
        b.iter(|| black_box(a).invert());
    });
}

fn bench_fp5_mul(c: &mut Criterion) {
    let (a, b_) = fp5_inputs();
    c.bench_function("Fp5::mul", |b| {
        b.iter(|| black_box(a) * black_box(b_));
    });
}

fn bench_fp5_square(c: &mut Criterion) {
    let (a, _) = fp5_inputs();
    c.bench_function("Fp5::square", |b| {
        b.iter(|| black_box(a).square());
    });
}

fn bench_fp5_invert(c: &mut Criterion) {
    let (a, _) = fp5_inputs();
    c.bench_function("Fp5::invert", |b| {
        b.iter(|| black_box(a).invert());
    });
}

criterion_group!(
    benches,
    bench_fp_mul,
    bench_fp_square,
    bench_fp_invert,
    bench_fp5_mul,
    bench_fp5_square,
    bench_fp5_invert,
);
criterion_main!(benches);

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

//! Curve operation benches: point arithmetic and scalar multiplication.
//!
//! Establishes baselines for both the variable-time `scalar_mul` (used by
//! verification) and the constant-time `scalar_mul_ct` (used by signing) so
//! later optimizations can be bench-compared without conflating the two paths.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_lighter::signing::curve::{Point, Scalar};

mod common;
use common::fixed_k;

fn bench_add_point(c: &mut Criterion) {
    let g = Point::GENERATOR;
    let g2 = g.double();
    c.bench_function("Point::add_point", |b| {
        b.iter(|| black_box(g).add_point(black_box(g2)));
    });
}

fn bench_double(c: &mut Criterion) {
    let g = Point::GENERATOR;
    c.bench_function("Point::double", |b| {
        b.iter(|| black_box(g).double());
    });
}

fn bench_mdouble_5(c: &mut Criterion) {
    let g = Point::GENERATOR;
    c.bench_function("Point::mdouble (n=5)", |b| {
        b.iter(|| black_box(g).mdouble(black_box(5)));
    });
}

fn bench_make_window_affine(c: &mut Criterion) {
    let g = Point::GENERATOR;
    c.bench_function("Point::make_window_affine", |b| {
        b.iter(|| black_box(g).make_window_affine());
    });
}

fn bench_scalar_mul_var_time(c: &mut Criterion) {
    let g = Point::GENERATOR;
    let s = fixed_k();
    c.bench_function("Point::scalar_mul (var-time)", |b| {
        b.iter(|| black_box(g).scalar_mul(black_box(s)));
    });
}

fn bench_scalar_mul_ct(c: &mut Criterion) {
    let g = Point::GENERATOR;
    let s = fixed_k();
    c.bench_function("Point::scalar_mul_ct", |b| {
        b.iter(|| black_box(g).scalar_mul_ct(black_box(s)));
    });
}

fn bench_decode(c: &mut Criterion) {
    let w = Point::GENERATOR.encode();
    c.bench_function("Point::decode", |b| {
        b.iter(|| Point::decode(black_box(w)));
    });
}

fn bench_encode(c: &mut Criterion) {
    let g = Point::GENERATOR;
    c.bench_function("Point::encode", |b| {
        b.iter(|| black_box(g).encode());
    });
}

fn bench_scalar_mul_arbitrary_base(c: &mut Criterion) {
    // Verification's `pk.scalar_mul(e)` runs over a non-generator base, so
    // bench an arbitrary point too — this is the path Strauss-Shamir
    // optimizes away.
    let base = Point::GENERATOR.scalar_mul(Scalar::from_limbs([7, 0, 0, 0, 0]));
    let s = fixed_k();
    c.bench_function("Point::scalar_mul (non-generator)", |b| {
        b.iter(|| black_box(base).scalar_mul(black_box(s)));
    });
}

criterion_group!(
    benches,
    bench_add_point,
    bench_double,
    bench_mdouble_5,
    bench_make_window_affine,
    bench_scalar_mul_var_time,
    bench_scalar_mul_ct,
    bench_decode,
    bench_encode,
    bench_scalar_mul_arbitrary_base,
);
criterion_main!(benches);

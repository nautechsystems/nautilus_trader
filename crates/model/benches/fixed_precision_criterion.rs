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

use std::hint::black_box;

use criterion::{Criterion, criterion_group};
use nautilus_model::types::fixed::{
    check_fixed_raw_i64, check_fixed_raw_i128, check_fixed_raw_u64, check_fixed_raw_u128,
    f64_to_fixed_i64, f64_to_fixed_i128,
};

pub fn bench_fixed_i64(c: &mut Criterion) {
    c.bench_function("f64_to_fixed_i64", |b| {
        b.iter(|| f64_to_fixed_i64(black_box(-1.0), black_box(1)));
    });
}

pub fn bench_fixed_i128(c: &mut Criterion) {
    c.bench_function("f64_to_fixed_i128", |b| {
        b.iter(|| f64_to_fixed_i128(black_box(-1.0), black_box(1)));
    });
}

pub fn bench_check_fixed_raw_u64(c: &mut Criterion) {
    // Valid raw value: 120 with precision 0 -> raw = 120 * 10^9
    let valid_raw: u64 = 120_000_000_000;
    c.bench_function("check_fixed_raw_u64_valid", |b| {
        b.iter(|| check_fixed_raw_u64(black_box(valid_raw), black_box(0)));
    });
}

pub fn bench_check_fixed_raw_u128(c: &mut Criterion) {
    let valid_raw: u128 = 120_000_000_000;
    c.bench_function("check_fixed_raw_u128_valid", |b| {
        b.iter(|| check_fixed_raw_u128(black_box(valid_raw), black_box(0)));
    });
}

pub fn bench_check_fixed_raw_i64(c: &mut Criterion) {
    let valid_raw: i64 = 120_000_000_000;
    c.bench_function("check_fixed_raw_i64_valid", |b| {
        b.iter(|| check_fixed_raw_i64(black_box(valid_raw), black_box(0)));
    });
}

pub fn bench_check_fixed_raw_i128(c: &mut Criterion) {
    let valid_raw: i128 = 120_000_000_000;
    c.bench_function("check_fixed_raw_i128_valid", |b| {
        b.iter(|| check_fixed_raw_i128(black_box(valid_raw), black_box(0)));
    });
}

pub fn bench_check_fixed_raw_u64_high_precision(c: &mut Criterion) {
    // Valid raw value with precision 8 -> raw = 120 * 10^(9-8) = 120 * 10 = 1200
    let valid_raw: u64 = 1200;
    c.bench_function("check_fixed_raw_u64_prec8", |b| {
        b.iter(|| check_fixed_raw_u64(black_box(valid_raw), black_box(8)));
    });
}

criterion_group!(
    benches,
    bench_fixed_i64,
    bench_fixed_i128,
    bench_check_fixed_raw_u64,
    bench_check_fixed_raw_u128,
    bench_check_fixed_raw_i64,
    bench_check_fixed_raw_i128,
    bench_check_fixed_raw_u64_high_precision,
);
criterion::criterion_main!(benches);

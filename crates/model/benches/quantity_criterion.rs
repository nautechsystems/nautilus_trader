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
use nautilus_model::types::{Quantity, quantity::QuantityRaw};
use rust_decimal_macros::dec;

pub fn bench_quantity_new(c: &mut Criterion) {
    c.bench_function("Quantity::new", |b| {
        b.iter(|| Quantity::new(black_box(123.45), black_box(2)));
    });
}

pub fn bench_quantity_from_decimal(c: &mut Criterion) {
    let decimal = dec!(123.45);
    c.bench_function("Quantity::from_decimal", |b| {
        b.iter(|| Quantity::from_decimal(black_box(decimal)));
    });
}

pub fn bench_quantity_from_decimal_high_scale(c: &mut Criterion) {
    // Decimal with more scale than target precision (triggers rounding)
    let decimal = dec!(123.456789);
    c.bench_function("Quantity::from_decimal_dp (high scale)", |b| {
        b.iter(|| Quantity::from_decimal_dp(black_box(decimal), black_box(2)));
    });
}

pub fn bench_quantity_from_raw(c: &mut Criterion) {
    let raw: QuantityRaw = 123_450_000_000;
    c.bench_function("Quantity::from_raw", |b| {
        b.iter(|| Quantity::from_raw(black_box(raw), black_box(2)));
    });
}

pub fn bench_quantity_from_mantissa_exponent(c: &mut Criterion) {
    c.bench_function("Quantity::from_mantissa_exponent", |b| {
        b.iter(|| Quantity::from_mantissa_exponent(black_box(12345), black_box(-2), black_box(2)));
    });
}

pub fn bench_quantity_add(c: &mut Criterion) {
    let a = Quantity::new(100.50, 2);
    let b = Quantity::new(200.75, 2);
    c.bench_function("Quantity + Quantity", |b_iter| {
        b_iter.iter(|| black_box(a) + black_box(b));
    });
}

pub fn bench_quantity_sub(c: &mut Criterion) {
    let a = Quantity::new(200.75, 2);
    let b = Quantity::new(100.50, 2);
    c.bench_function("Quantity - Quantity", |b_iter| {
        b_iter.iter(|| black_box(a) - black_box(b));
    });
}

pub fn bench_quantity_as_decimal(c: &mut Criterion) {
    let qty = Quantity::new(123.45, 2);
    c.bench_function("Quantity::as_decimal", |b| {
        b.iter(|| black_box(qty).as_decimal());
    });
}

pub fn bench_quantity_as_f64(c: &mut Criterion) {
    let qty = Quantity::new(123.45, 2);
    c.bench_function("Quantity::as_f64", |b| {
        b.iter(|| black_box(qty).as_f64());
    });
}

pub fn bench_quantity_mul_f64(c: &mut Criterion) {
    let qty = Quantity::new(100.00, 2);
    c.bench_function("Quantity * f64", |b| {
        b.iter(|| black_box(qty) * black_box(0.001));
    });
}

pub fn bench_quantity_mul_decimal(c: &mut Criterion) {
    let qty = Quantity::new(100.00, 2);
    let factor = dec!(0.001);
    c.bench_function("Quantity * Decimal", |b| {
        b.iter(|| black_box(qty) * black_box(factor));
    });
}

criterion_group!(
    benches,
    bench_quantity_new,
    bench_quantity_from_decimal,
    bench_quantity_from_decimal_high_scale,
    bench_quantity_from_raw,
    bench_quantity_from_mantissa_exponent,
    bench_quantity_add,
    bench_quantity_sub,
    bench_quantity_as_decimal,
    bench_quantity_as_f64,
    bench_quantity_mul_f64,
    bench_quantity_mul_decimal,
);
criterion::criterion_main!(benches);

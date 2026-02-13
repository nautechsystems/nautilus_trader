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
use nautilus_model::types::{Currency, Money, money::MoneyRaw};
use rust_decimal_macros::dec;

fn usd() -> Currency {
    Currency::USD()
}

pub fn bench_money_new(c: &mut Criterion) {
    let currency = usd();
    c.bench_function("Money::new", |b| {
        b.iter(|| Money::new(black_box(123.45), black_box(currency)));
    });
}

pub fn bench_money_from_decimal(c: &mut Criterion) {
    let currency = usd();
    let decimal = dec!(123.45);
    c.bench_function("Money::from_decimal", |b| {
        b.iter(|| Money::from_decimal(black_box(decimal), black_box(currency)));
    });
}

pub fn bench_money_from_raw(c: &mut Criterion) {
    let currency = usd();
    let raw: MoneyRaw = 123_450_000_000;
    c.bench_function("Money::from_raw", |b| {
        b.iter(|| Money::from_raw(black_box(raw), black_box(currency)));
    });
}

pub fn bench_money_add(c: &mut Criterion) {
    let currency = usd();
    let a = Money::new(100.50, currency);
    let b = Money::new(200.75, currency);
    c.bench_function("Money + Money", |b_iter| {
        b_iter.iter(|| black_box(a) + black_box(b));
    });
}

pub fn bench_money_sub(c: &mut Criterion) {
    let currency = usd();
    let a = Money::new(200.75, currency);
    let b = Money::new(100.50, currency);
    c.bench_function("Money - Money", |b_iter| {
        b_iter.iter(|| black_box(a) - black_box(b));
    });
}

pub fn bench_money_as_decimal(c: &mut Criterion) {
    let money = Money::new(123.45, usd());
    c.bench_function("Money::as_decimal", |b| {
        b.iter(|| black_box(money).as_decimal());
    });
}

pub fn bench_money_as_f64(c: &mut Criterion) {
    let money = Money::new(123.45, usd());
    c.bench_function("Money::as_f64", |b| {
        b.iter(|| black_box(money).as_f64());
    });
}

pub fn bench_money_mul_f64(c: &mut Criterion) {
    let money = Money::new(100.00, usd());
    c.bench_function("Money * f64", |b| {
        b.iter(|| black_box(money) * black_box(0.001));
    });
}

pub fn bench_money_from_decimal_high_scale(c: &mut Criterion) {
    let currency = usd();
    // Decimal with more scale than currency precision (triggers rounding)
    let decimal = dec!(123.456789);
    c.bench_function("Money::from_decimal (high scale)", |b| {
        b.iter(|| Money::from_decimal(black_box(decimal), black_box(currency)));
    });
}

pub fn bench_money_mul_decimal(c: &mut Criterion) {
    let money = Money::new(100.00, usd());
    let fee = dec!(0.001);
    c.bench_function("Money * Decimal", |b| {
        b.iter(|| black_box(money) * black_box(fee));
    });
}

pub fn bench_money_from_mantissa_exponent(c: &mut Criterion) {
    let currency = usd();
    c.bench_function("Money::from_mantissa_exponent", |b| {
        b.iter(|| {
            Money::from_mantissa_exponent(black_box(12345), black_box(-2), black_box(currency))
        });
    });
}

criterion_group!(
    benches,
    bench_money_new,
    bench_money_from_decimal,
    bench_money_from_decimal_high_scale,
    bench_money_from_raw,
    bench_money_from_mantissa_exponent,
    bench_money_add,
    bench_money_sub,
    bench_money_as_decimal,
    bench_money_as_f64,
    bench_money_mul_f64,
    bench_money_mul_decimal,
);
criterion::criterion_main!(benches);

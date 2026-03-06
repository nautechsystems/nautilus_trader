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

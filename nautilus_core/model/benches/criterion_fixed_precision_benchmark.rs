use criterion::{black_box, criterion_group, Criterion};
use nautilus_model::types::fixed::f64_to_fixed_i64;

pub fn criterion_fixed_precision_benchmark(c: &mut Criterion) {
    c.bench_function("f64_to_fixed_i64", |b| {
        b.iter(|| f64_to_fixed_i64(black_box(-1.0), black_box(1)))
    });
}

criterion_group!(benches, criterion_fixed_precision_benchmark);
criterion::criterion_main!(benches);

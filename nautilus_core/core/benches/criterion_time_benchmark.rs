use criterion::{criterion_group, Criterion};
use nautilus_core::time::unix_timestamp_ns;

#[allow(clippy::redundant_closure)]
pub fn criterion_time_benchmark(c: &mut Criterion) {
    c.bench_function("f64_to_fixed_i64", |b| b.iter(|| unix_timestamp_ns()));
}

criterion_group!(benches, criterion_time_benchmark);
criterion::criterion_main!(benches);

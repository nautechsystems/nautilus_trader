use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::time::{duration_since_unix_epoch, nanos_since_unix_epoch};

// Using `SystemTime` under the hood
fn bench_system_time(c: &mut Criterion) {
    c.bench_function("duration_since_unix_epoch", |b| {
        b.iter(duration_since_unix_epoch);
    });
}

// Using libc `clock_gettime` syscall
fn bench_rdtscp(c: &mut Criterion) {
    c.bench_function("nanos_since_unix_epoch", |b| {
        b.iter(nanos_since_unix_epoch);
    });
}

criterion_group!(benches, bench_system_time, bench_rdtscp);
criterion_main!(benches);

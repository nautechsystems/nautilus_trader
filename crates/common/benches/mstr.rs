use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_common::msgbus::{MStr, Pattern, Topic};
use ustr::Ustr;

const TOPIC_STR: &str = "data.quotes.BINANCE.ETHUSDT";

fn bench_mstr_from_str(c: &mut Criterion) {
    let mut group = c.benchmark_group("MStr::from_str");

    group.bench_function("Topic", |b| {
        b.iter(|| {
            let topic: MStr<Topic> = black_box(TOPIC_STR).into();
            black_box(topic)
        });
    });

    group.bench_function("Pattern", |b| {
        b.iter(|| {
            let pattern: MStr<Pattern> = black_box(TOPIC_STR).into();
            black_box(pattern)
        });
    });

    group.finish();
}

fn bench_mstr_from_ustr(c: &mut Criterion) {
    let mut group = c.benchmark_group("MStr::from_ustr");

    let ustr = Ustr::from(TOPIC_STR);

    group.bench_function("Topic", |b| {
        b.iter(|| {
            let topic: MStr<Topic> = black_box(ustr).into();
            black_box(topic)
        });
    });

    group.finish();
}

fn bench_mstr_as_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("MStr::as_bytes");

    let topic: MStr<Topic> = TOPIC_STR.into();
    let pattern: MStr<Pattern> = TOPIC_STR.into();

    group.bench_function("Topic", |b| {
        b.iter(|| {
            let bytes = topic.as_bytes();
            black_box(bytes.len())
        });
    });

    group.bench_function("Pattern", |b| {
        b.iter(|| {
            let bytes = pattern.as_bytes();
            black_box(bytes.len())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_mstr_from_str,
    bench_mstr_from_ustr,
    bench_mstr_as_bytes
);
criterion_main!(benches);

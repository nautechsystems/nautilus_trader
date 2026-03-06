//! Benchmarks focusing on the `CacheIndex` set intersections that power order
//! queries.  These benches isolate the cost of building the result sets for
//! various filter combinations without measuring deserialization or I/O.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_common::cache::Cache;
use nautilus_model::{
    identifiers::{InstrumentId, Venue},
    orders::stubs::create_order_list_sample,
};

/// Populate a `Cache` with the synthetic 100 k order universe used across
/// cache benchmarks (5 venues × 100 instruments × 200 orders).
fn build_populated_cache() -> Cache {
    let orders = create_order_list_sample(5, 100, 200);
    let mut cache = Cache::default();
    for order in orders {
        cache.add_order(order, None, None, false).unwrap();
    }
    cache
}

fn bench_set_intersections(c: &mut Criterion) {
    let cache = build_populated_cache();

    // Pre-create filter values so we don’t allocate in the hot loop
    let venue = Venue::from("VENUE-1");
    let instrument = InstrumentId::from("SYMBOL-1.1");

    let mut group = c.benchmark_group("Cache set intersections");

    // No filters → full set
    group.bench_function("all orders", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids(None, None, None, None));
        });
    });

    // Venue only
    group.bench_function("venue filter", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids(Some(&venue), None, None, None));
        });
    });

    // Instrument only
    group.bench_function("instrument filter", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids(None, Some(&instrument), None, None));
        });
    });

    // Venue + instrument
    group.bench_function("venue + instrument filter", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids(Some(&venue), Some(&instrument), None, None));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_set_intersections);
criterion_main!(benches);

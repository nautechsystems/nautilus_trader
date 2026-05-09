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
    let instrument = InstrumentId::from("SYMBOL-1.VENUE-1");

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

fn bench_count_methods(c: &mut Criterion) {
    let cache = build_populated_cache();

    let venue = Venue::from("VENUE-1");
    let instrument = InstrumentId::from("SYMBOL-1.VENUE-1");

    let mut group = c.benchmark_group("Cache count methods");

    // Total count: bucket size = full universe (100k)
    group.bench_function("orders_total_count no filter", |b| {
        b.iter(|| {
            black_box(cache.orders_total_count(None, None, None, None, None));
        });
    });
    group.bench_function("orders_total_count venue", |b| {
        b.iter(|| {
            black_box(cache.orders_total_count(Some(&venue), None, None, None, None));
        });
    });
    group.bench_function("orders_total_count venue + instrument", |b| {
        b.iter(|| {
            black_box(cache.orders_total_count(Some(&venue), Some(&instrument), None, None, None));
        });
    });

    // Active-local count: same population, hits bucket-membership iteration
    group.bench_function("orders_active_local_count no filter", |b| {
        b.iter(|| {
            black_box(cache.orders_active_local_count(None, None, None, None, None));
        });
    });
    group.bench_function("orders_active_local_count venue", |b| {
        b.iter(|| {
            black_box(cache.orders_active_local_count(Some(&venue), None, None, None, None));
        });
    });
    group.bench_function("orders_active_local_count venue + instrument", |b| {
        b.iter(|| {
            black_box(cache.orders_active_local_count(
                Some(&venue),
                Some(&instrument),
                None,
                None,
                None,
            ));
        });
    });

    // Empty bucket but filter still runs (mirrors strategy submit path)
    group.bench_function("orders_open_count empty bucket no filter", |b| {
        b.iter(|| {
            black_box(cache.orders_open_count(None, None, None, None, None));
        });
    });
    group.bench_function("orders_open_count empty bucket venue + instrument", |b| {
        b.iter(|| {
            black_box(cache.orders_open_count(Some(&venue), Some(&instrument), None, None, None));
        });
    });

    group.finish();
}

fn bench_state_bucket_queries(c: &mut Criterion) {
    let cache = build_populated_cache();

    let venue = Venue::from("VENUE-1");
    let instrument = InstrumentId::from("SYMBOL-1.VENUE-1");

    let mut group = c.benchmark_group("Cache state bucket queries");

    // index.orders_active_local has the full populated universe.
    group.bench_function("client_order_ids_active_local no filter", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids_active_local(None, None, None, None));
        });
    });
    group.bench_function("client_order_ids_active_local venue", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids_active_local(Some(&venue), None, None, None));
        });
    });
    group.bench_function("client_order_ids_active_local venue + instrument", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids_active_local(
                Some(&venue),
                Some(&instrument),
                None,
                None,
            ));
        });
    });

    // index.orders_open is empty in this fixture, so the filter still runs but the bucket
    // intersection short-circuits. This exercises the strategy submit-order path shape.
    group.bench_function(
        "client_order_ids_open empty bucket venue + instrument",
        |b| {
            b.iter(|| {
                black_box(cache.client_order_ids_open(Some(&venue), Some(&instrument), None, None));
            });
        },
    );

    group.finish();
}

fn bench_has_methods(c: &mut Criterion) {
    let cache = build_populated_cache();

    let venue = Venue::from("VENUE-1");
    let instrument = InstrumentId::from("SYMBOL-1.VENUE-1");

    let mut group = c.benchmark_group("Cache has_* existence");

    // index.orders_active_local has the full populated universe; existence is trivially true
    group.bench_function("has_orders_active_local no filter", |b| {
        b.iter(|| {
            black_box(cache.has_orders_active_local(None, None, None, None, None));
        });
    });
    group.bench_function("has_orders_active_local venue", |b| {
        b.iter(|| {
            black_box(cache.has_orders_active_local(Some(&venue), None, None, None, None));
        });
    });
    group.bench_function("has_orders_active_local venue + instrument", |b| {
        b.iter(|| {
            black_box(cache.has_orders_active_local(
                Some(&venue),
                Some(&instrument),
                None,
                None,
                None,
            ));
        });
    });

    // Empty bucket cases (mirrors strategy submit gating)
    group.bench_function("has_orders_open empty bucket no filter", |b| {
        b.iter(|| {
            black_box(cache.has_orders_open(None, None, None, None, None));
        });
    });
    group.bench_function("has_orders_open empty bucket venue + instrument", |b| {
        b.iter(|| {
            black_box(cache.has_orders_open(Some(&venue), Some(&instrument), None, None, None));
        });
    });

    group.finish();
}

fn bench_view_methods(c: &mut Criterion) {
    let cache = build_populated_cache();

    let venue = Venue::from("VENUE-1");
    let instrument = InstrumentId::from("SYMBOL-1.VENUE-1");

    let mut group = c.benchmark_group("Cache view borrowed/owned");

    // No filter: view returns Cow::Borrowed, owned counterpart pays a full bucket clone
    group.bench_function("client_order_ids_active_local_view no filter", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids_active_local_view(None, None, None, None));
        });
    });
    group.bench_function("client_order_ids_active_local_view venue", |b| {
        b.iter(|| {
            black_box(cache.client_order_ids_active_local_view(Some(&venue), None, None, None));
        });
    });
    group.bench_function(
        "client_order_ids_active_local_view venue + instrument",
        |b| {
            b.iter(|| {
                black_box(cache.client_order_ids_active_local_view(
                    Some(&venue),
                    Some(&instrument),
                    None,
                    None,
                ));
            });
        },
    );

    group.finish();
}

fn bench_iter_methods(c: &mut Criterion) {
    let cache = build_populated_cache();

    let venue = Venue::from("VENUE-1");
    let instrument = InstrumentId::from("SYMBOL-1.VENUE-1");

    let mut group = c.benchmark_group("Cache iter_* lazy");

    // Drain the iterator with .count() so the benchmark forces full traversal.
    group.bench_function("iter_client_order_ids_active_local no filter", |b| {
        b.iter(|| {
            black_box(
                cache
                    .iter_client_order_ids_active_local(None, None, None, None)
                    .count(),
            );
        });
    });
    group.bench_function("iter_client_order_ids_active_local venue", |b| {
        b.iter(|| {
            black_box(
                cache
                    .iter_client_order_ids_active_local(Some(&venue), None, None, None)
                    .count(),
            );
        });
    });
    group.bench_function(
        "iter_client_order_ids_active_local venue + instrument",
        |b| {
            b.iter(|| {
                black_box(
                    cache
                        .iter_client_order_ids_active_local(
                            Some(&venue),
                            Some(&instrument),
                            None,
                            None,
                        )
                        .count(),
                );
            });
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_set_intersections,
    bench_count_methods,
    bench_state_bucket_queries,
    bench_has_methods,
    bench_view_methods,
    bench_iter_methods,
);
criterion_main!(benches);

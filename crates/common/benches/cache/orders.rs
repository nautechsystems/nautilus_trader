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

use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::cache::Cache;
use nautilus_model::{
    identifiers::{InstrumentId, Venue},
    orders::{Order, OrderAny, stubs::create_order_list_sample},
};

fn cache_order_querying_venue_instrument(
    cache: &Cache,
    venue: &Venue,
    instrument: Option<&InstrumentId>,
) {
    let _ = cache.orders(Some(venue), instrument, None, None, None);
}

fn cache_orders_processing(orders: &[OrderAny]) {
    let mut cache = Cache::default();
    for order in orders {
        cache.add_order(order.clone(), None, None, false).unwrap();
    }
}

fn bench_order_indexing(c: &mut Criterion) {
    // Create 100k orders list and add it to the cache
    let all_orders = create_order_list_sample(5, 100, 200);
    let venue = Venue::from("VENUE-1");
    let instrument = InstrumentId::from("SYMBOL-1.VENUE-1");
    let mut expected_venue: Vec<_> = all_orders
        .iter()
        .filter(|order| order.instrument_id().venue == venue)
        .map(|order| order.client_order_id())
        .collect();
    let mut expected_instrument: Vec<_> = all_orders
        .iter()
        .filter(|order| order.instrument_id() == instrument)
        .map(|order| order.client_order_id())
        .collect();
    expected_venue.sort();
    expected_instrument.sort();
    assert_eq!(expected_venue.len(), 20_000);
    assert_eq!(expected_instrument.len(), 200);

    let mut cache = Cache::default();
    for order in all_orders {
        cache.add_order(order, None, None, false).unwrap();
    }

    for (venue_filter, instrument_filter, expected) in [
        (Some(&venue), None, &expected_venue),
        (None, Some(&instrument), &expected_instrument),
        (Some(&venue), Some(&instrument), &expected_instrument),
    ] {
        let actual: Vec<_> = cache
            .orders(venue_filter, instrument_filter, None, None, None)
            .iter()
            .map(|order| order.client_order_id())
            .collect();
        assert_eq!(&actual, expected);
    }

    c.bench_function("Cache query by instrument (200 orders)", |b| {
        b.iter(|| black_box(&cache).orders(None, Some(black_box(&instrument)), None, None, None));
    });

    c.bench_function("Cache query by venue", |b| {
        b.iter(|| {
            cache_order_querying_venue_instrument(
                black_box(&cache),
                black_box(&venue),
                black_box(None),
            );
        });
    });

    c.bench_function("Cache query by venue + instrument (200 orders)", |b| {
        b.iter(|| {
            cache_order_querying_venue_instrument(
                black_box(&cache),
                black_box(&venue),
                black_box(Some(&instrument)),
            );
        });
    });
}

fn bench_order_processing(c: &mut Criterion) {
    // Generate list with 100k orders which we slice per test (5 * 100 * 200 = 100k)
    let all_orders = create_order_list_sample(5, 100, 200);

    c.bench_function("Cache order processing one order", |b| {
        b.iter(|| cache_orders_processing(black_box(&all_orders[..1])));
    });

    c.bench_function("Cache order processing 10k orders", |b| {
        b.iter(|| cache_orders_processing(black_box(&all_orders[..10000])));
    });

    let mut large = c.benchmark_group("Cache order processing large");
    large.measurement_time(Duration::from_secs(10));
    large.bench_function("100k orders", |b| {
        b.iter(|| cache_orders_processing(black_box(&all_orders)));
    });
    large.finish();
}

fn bench_order_query_small(c: &mut Criterion) {
    // Check sorting overhead when queries return only a handful of orders
    let mut group = c.benchmark_group("Cache query small");

    for count in [1, 8, 32] {
        let mut cache = Cache::default();
        for order in create_order_list_sample(1, 1, count) {
            cache.add_order(order, None, None, false).unwrap();
        }

        assert_eq!(
            cache.orders(None, None, None, None, None).len(),
            count as usize
        );

        group.throughput(Throughput::Elements(u64::from(count)));
        group.bench_with_input(BenchmarkId::from_parameter(count), &cache, |b, cache| {
            b.iter(|| black_box(cache).orders(None, None, None, None, None));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_order_indexing,
    bench_order_processing,
    bench_order_query_small,
);
criterion_main!(benches);

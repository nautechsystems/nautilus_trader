use std::collections::{HashMap, HashSet};

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nautilus_common::cache::Cache;
use nautilus_model::{
    identifiers::{InstrumentId, Venue, VenueOrderId},
    orders::stubs::create_order_list_sample,
};

fn cache_orders_querying_only_venue(mut cache: &Cache, venue: &Venue) {
    let _ = cache.orders(Some(venue), None, None, None);
}

fn cache_order_querying_venue_instrument(
    mut cache: &Cache,
    venue: &Venue,
    instrument: &InstrumentId,
) {
    let _ = cache.orders(Some(venue), Some(instrument), None, None);
}

fn benchmark_order_indexing(c: &mut Criterion) {
    // crete 100k orders list and add it to the cache
    let all_orders = create_order_list_sample(5, 100, 200);
    let mut cache = Cache::default();
    for order in all_orders {
        cache.add_order(order, None, None, false).unwrap();
    }

    c.bench_function(
        "Cache with 100k orders - query orders with specific venue",
        |b| b.iter(|| cache_orders_querying_only_venue(black_box(&cache), &Venue::from("VENUE-1"))),
    );

    c.bench_function(
        "Cache with 100k orders - query orders with specific venue and instrument",
        |b| {
            b.iter(|| {
                cache_order_querying_venue_instrument(
                    black_box(&cache),
                    &Venue::from("VENUE-1"),
                    &InstrumentId::from("SYMBOL-1.1"),
                )
            })
        },
    );
}

criterion_group!(benches, benchmark_order_indexing);
criterion_main!(benches);

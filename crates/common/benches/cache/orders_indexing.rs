use std::collections::{HashMap, HashSet};

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nautilus_common::cache::Cache;
use nautilus_model::{
    identifiers::{InstrumentId, Venue, VenueOrderId},
    orders::stubs::create_order_list_sample,
};

fn cache_order_querying_venue_instrument(
    mut cache: &Cache,
    venue: &Venue,
    instrument: Option<&InstrumentId>,
) {
    let _ = cache.orders(Some(venue), instrument, None, None);
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
        |b| {
            b.iter(|| {
                cache_order_querying_venue_instrument(
                    black_box(&cache),
                    black_box(&Venue::from("VENUE-1")),
                    black_box(None),
                )
            })
        },
    );

    c.bench_function(
        "Cache with 100k orders - query orders with specific venue and instrument",
        |b| {
            b.iter(|| {
                cache_order_querying_venue_instrument(
                    black_box(&cache),
                    black_box(&Venue::from("VENUE-1")),
                    black_box(Some(&InstrumentId::from("SYMBOL-1.1"))),
                )
            })
        },
    );
}

criterion_group!(benches, benchmark_order_indexing);
criterion_main!(benches);

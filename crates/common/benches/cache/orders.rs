use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nautilus_common::cache::Cache;
use nautilus_model::{
    identifiers::{InstrumentId, Venue},
    orders::{OrderAny, stubs::create_order_list_sample},
};

fn cache_order_querying_venue_instrument(
    cache: &Cache,
    venue: &Venue,
    instrument: Option<&InstrumentId>,
) {
    let _ = cache.orders(Some(venue), instrument, None, None);
}

fn cache_orders_processing(orders: &[OrderAny]) {
    let mut cache = Cache::default();
    for order in orders {
        cache.add_order(order.clone(), None, None, false).unwrap();
    }
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

fn benchmark_order_processing(c: &mut Criterion) {
    // Generate list with 100k orders which we slice per test (5 * 100 * 200 = 100k)
    let all_orders = create_order_list_sample(5, 100, 200);

    c.bench_function("Cache order processing one order", |b| {
        b.iter(|| cache_orders_processing(black_box(&all_orders[..1])))
    });

    c.bench_function("Cache order processing 10k orders", |b| {
        b.iter(|| cache_orders_processing(black_box(&all_orders[..10000])))
    });

    c.bench_function("Cache order processing 100k orders", |b| {
        b.iter(|| cache_orders_processing(black_box(&all_orders)))
    });
}

criterion_group!(
    benches,
    benchmark_order_indexing,
    benchmark_order_processing
);
criterion_main!(benches);

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nautilus_common::cache::Cache;
use nautilus_model::orders::{OrderAny, stubs::create_order_list_sample};

fn cache_orders_processing(orders: &[OrderAny]) {
    let mut cache = Cache::default();
    for order in orders {
        cache.add_order(order.clone(), None, None, false).unwrap();
    }
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

criterion_group!(benches, benchmark_order_processing);
criterion_main!(benches);

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

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::cache::Cache;
use nautilus_model::{
    enums::{OmsType, OrderSide, OrderType},
    events::OrderEventAny,
    identifiers::{ClientOrderId, InstrumentId, PositionId, TradeId, Venue},
    instruments::{InstrumentAny, stubs::audusd_sim},
    orders::{
        Order, OrderAny,
        builder::OrderTestBuilder,
        stubs::{TestOrderEventStubs, create_order_list_sample},
    },
    position::Position,
    types::Quantity,
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

fn position_for_purge(fill_count: u64) -> Position {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(InstrumentId::from("AUD/USD.SIM"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from(fill_count))
        .build();
    let OrderEventAny::Filled(mut fill) = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-0")),
        Some(PositionId::from("P-PURGE")),
        None,
        Some(Quantity::from(1)),
        None,
        None,
        None,
        None,
    ) else {
        unreachable!();
    };
    let mut position = Position::new(&instrument, fill.clone());

    for index in 1..fill_count {
        fill.trade_id = TradeId::from(format!("T-{index}").as_str());
        fill.ts_event = index.into();
        position.apply(&fill);
    }

    fill.trade_id = TradeId::from("T-CLOSE");
    fill.client_order_id = ClientOrderId::from("O-CLOSE");
    fill.order_side = OrderSide::Sell;
    fill.last_qty = position.quantity;
    fill.ts_event = fill_count.into();
    position.apply(&fill);
    assert!(position.is_closed());

    position
}

fn bench_position_purge(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_position_purge");

    for fill_count in [1_u64, 128, 10_000] {
        let position = position_for_purge(fill_count);
        let position_id = position.id;

        group.throughput(Throughput::Elements(position.events.len() as u64));
        group.bench_function(BenchmarkId::from_parameter(fill_count), |b| {
            b.iter_batched_ref(
                || {
                    let mut cache = Cache::default();
                    cache
                        .add_position_without_order(&position, OmsType::Netting)
                        .unwrap();
                    cache.update_position(&position).unwrap();
                    assert_eq!(
                        cache.positions_closed_count(None, None, None, None, None),
                        1
                    );
                    cache
                },
                |cache| {
                    // Timing includes final destruction of the cached position and its history
                    cache.purge_position(black_box(position_id));
                    assert!(!cache.position_exists(&position_id));
                },
                BatchSize::PerIteration,
            );
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_order_indexing,
    bench_order_processing,
    bench_order_query_small,
    bench_position_purge,
);
criterion_main!(benches);

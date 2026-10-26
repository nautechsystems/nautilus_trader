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

//! Benchmarks for applying fills and reading the last order event.
//!
//! Measures complete fill batches applied through `Order::apply`.
//! Average-price arithmetic extends a cached `Decimal` fold, while duplicate detection scans
//! prior events. The fill counts expose both small-order overhead and history-dependent cost.
//!
//! Run with `cargo bench -p nautilus-model --bench order_fills_criterion`.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType, TimeInForce},
    events::{OrderAccepted, OrderEventAny, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    orders::{MarketOrder, Order, OrderAny},
    types::{Currency, Price, Quantity},
};

const FILL_COUNTS: [u64; 4] = [1, 8, 64, 512];
const TRADER_ID: &str = "TRADER-001";
const STRATEGY_ID: &str = "S-001";
const INSTRUMENT_ID: &str = "ETHUSDT-PERP.BINANCE";
const CLIENT_ORDER_ID: &str = "O-19700101-000000-001-001-1";
const VENUE_ORDER_ID: &str = "1";
const ACCOUNT_ID: &str = "SIM-001";
// Distinct prices so no fill folds into a previously accumulated notional by chance.
const PRICES: [&str; 6] = [
    "1.00001", "1.00002", "1.00003", "1.00004", "1.00005", "1.00006",
];

fn accepted_market_order(quantity: Quantity) -> MarketOrder {
    let mut order = MarketOrder::new(
        TraderId::from(TRADER_ID),
        StrategyId::from(STRATEGY_ID),
        InstrumentId::from(INSTRUMENT_ID),
        ClientOrderId::from(CLIENT_ORDER_ID),
        OrderSide::Buy,
        quantity,
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let accepted = OrderAccepted::new(
        TraderId::from(TRADER_ID),
        StrategyId::from(STRATEGY_ID),
        InstrumentId::from(INSTRUMENT_ID),
        ClientOrderId::from(CLIENT_ORDER_ID),
        VenueOrderId::from(VENUE_ORDER_ID),
        AccountId::from(ACCOUNT_ID),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    order
}

fn fill_events(count: u64) -> Vec<OrderEventAny> {
    (0..count)
        .map(|i| {
            OrderEventAny::Filled(OrderFilled::new(
                TraderId::from(TRADER_ID),
                StrategyId::from(STRATEGY_ID),
                InstrumentId::from(INSTRUMENT_ID),
                ClientOrderId::from(CLIENT_ORDER_ID),
                VenueOrderId::from(VENUE_ORDER_ID),
                AccountId::from(ACCOUNT_ID),
                TradeId::from(format!("TRADE-{i}").as_str()),
                OrderSide::Buy,
                OrderType::Market,
                Quantity::from(1),
                Price::from(PRICES[(i as usize) % PRICES.len()]),
                Currency::USDT(),
                LiquiditySide::Taker,
                UUID4::new(),
                i.into(),
                i.into(),
                false,
                None,
                None,
                None,
            ))
        })
        .collect()
}

fn bench_order_fills(c: &mut Criterion) {
    let mut group = c.benchmark_group("order_fills");

    for count in FILL_COUNTS {
        let events = fill_events(count);
        group.throughput(Throughput::Elements(count));
        group.bench_with_input(BenchmarkId::from_parameter(count), &events, |b, events| {
            b.iter_batched_ref(
                || accepted_market_order(Quantity::from(count)),
                |order| {
                    for event in events {
                        order.apply(black_box(event.clone())).unwrap();
                    }
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn generic_last_event<T: Order + ?Sized>(order: &T) -> &OrderEventAny {
    order.last_event()
}

fn bench_last_order_event(c: &mut Criterion) {
    let mut group = c.benchmark_group("last_order_event");

    for count in [0_u64, 128, 10_000] {
        let mut order = accepted_market_order(Quantity::from(count.max(1)));

        for event in fill_events(count) {
            order.apply(event).unwrap();
        }

        let event_count = order.event_count();
        let any = OrderAny::Market(order.clone());
        let erased: &dyn Order = &order;

        group.bench_function(BenchmarkId::new("concrete", event_count), |b| {
            b.iter(|| black_box(black_box(&order).last_event()));
        });
        group.bench_function(BenchmarkId::new("generic", event_count), |b| {
            b.iter(|| black_box(generic_last_event(black_box(&order))));
        });
        group.bench_function(BenchmarkId::new("dyn", event_count), |b| {
            b.iter(|| black_box(black_box(erased).last_event()));
        });
        group.bench_function(BenchmarkId::new("enum", event_count), |b| {
            b.iter(|| black_box(black_box(&any).last_event()));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_order_fills, bench_last_order_event);
criterion_main!(benches);

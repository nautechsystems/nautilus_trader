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

//! Benchmarks for [`OrderMatchingCore`].
//!
//! Workload sizes target four regimes (orders per side total, spread across
//! distinct price levels so the BTreeMap exercises tree depth):
//! - 4 orders: directional strategy
//! - 32 orders: active market maker
//! - 100 orders: depth across many price levels
//! - 1_000 orders: deep grid / many-level book
//!
//! Each seeded core is split evenly between bid and ask sides. Lookups go
//! through the `AHashMap` index so they are O(1) regardless of side balance.
//! Mutating benches use `iter_batched_ref` so the timed region excludes
//! input `Drop`.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_execution::matching_core::{OrderMatchingCore, RestingOrder};
use nautilus_model::{
    enums::{OrderSide, OrderSideSpecified, OrderType},
    events::{OrderEventAny, order::spec::OrderInitializedSpec},
    identifiers::{ClientOrderId, InstrumentId},
    orders::{Order, OrderAny, PassiveOrderAny},
    types::{Price, Quantity},
};

const SIZES: &[usize] = &[4, 32, 100, 1_000];

fn make_limit(side: OrderSide, price: &str, seq: usize) -> OrderAny {
    let init = OrderInitializedSpec::builder()
        .client_order_id(ClientOrderId::from(format!("O-{seq}").as_str()))
        .order_side(side)
        .order_type(OrderType::Limit)
        .quantity(Quantity::from("10"))
        .price(Price::from(price))
        .build();
    OrderAny::from_events(vec![OrderEventAny::Initialized(init)]).unwrap()
}

/// Builds a core seeded with `n` limit orders split evenly between bid and ask
/// sides. Returns the core, the ordered list of bid client_order_ids, and the
/// ordered list of ask client_order_ids.
fn seeded_core(n: usize) -> (OrderMatchingCore, Vec<ClientOrderId>, Vec<ClientOrderId>) {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut core = OrderMatchingCore::new(instrument_id, Price::from("0.01"));
    core.set_bid_raw(Price::from("100.00"));
    core.set_ask_raw(Price::from("101.00"));

    let mut bid_ids = Vec::with_capacity(n / 2 + 1);
    let mut ask_ids = Vec::with_capacity(n / 2);
    for i in 0..n {
        let (side, price) = if i % 2 == 0 {
            (OrderSide::Buy, format!("100.{:02}", (i / 2) % 100))
        } else {
            (OrderSide::Sell, format!("101.{:02}", (i / 2) % 100))
        };
        let order = make_limit(side, &price, i);
        let oid = order.client_order_id();
        match side {
            OrderSide::Buy => bid_ids.push(oid),
            OrderSide::Sell => ask_ids.push(oid),
            _ => unreachable!(),
        }
        core.add_order(RestingOrder::from(
            &PassiveOrderAny::try_from(order).unwrap(),
        ));
    }
    (core, bid_ids, ask_ids)
}

fn bench_add_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_core/add_order");
    // The duplicate-check in `add_order` is a `debug_assert!` and is stripped
    // in bench builds, so we measure only side routing + push at population n.
    let new_order = make_limit(OrderSide::Buy, "200.00", usize::MAX);
    let info = RestingOrder::from(&PassiveOrderAny::try_from(new_order).unwrap());

    for &n in SIZES {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched_ref(
                || seeded_core(n).0,
                |core| core.add_order(black_box(info)),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_get_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_core/get_order");

    for &n in SIZES {
        let (core, bid_ids, ask_ids) = seeded_core(n);
        // Target an ask-side order: hash index lookup is O(1) regardless of
        // side, so scaling shows index overhead vs bucket size, not scan cost.
        let target = *ask_ids.last().unwrap_or(bid_ids.last().unwrap());

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(core.get_order(black_box(target))));
        });
    }
    group.finish();
}

fn bench_order_exists(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_core/order_exists");

    for &n in SIZES {
        let (core, _, _) = seeded_core(n);
        // Miss: hash index `contains_key` returns false in O(1).
        let missing = ClientOrderId::from("O-MISSING");

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(core.order_exists(black_box(missing))));
        });
    }
    group.finish();
}

#[derive(Copy, Clone)]
enum DeleteCase {
    /// First bid (oldest at its level): hash index lookup + bucket head shift.
    BidHead,
    /// Last bid (newest at its level): hash index lookup + bucket tail pop.
    BidTail,
    /// First ask: hash index lookup + ask-bucket head shift.
    AskHead,
    /// Not present: hash index miss returns Err in O(1), no shift.
    Missing,
}

impl DeleteCase {
    const fn label(self) -> &'static str {
        match self {
            Self::BidHead => "bid_head",
            Self::BidTail => "bid_tail",
            Self::AskHead => "ask_head",
            Self::Missing => "missing",
        }
    }
}

fn bench_delete_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_core/delete_order");
    let cases = [
        DeleteCase::BidHead,
        DeleteCase::BidTail,
        DeleteCase::AskHead,
        DeleteCase::Missing,
    ];

    for case in cases {
        for &n in SIZES {
            group.throughput(Throughput::Elements(n as u64));
            group.bench_with_input(BenchmarkId::new(case.label(), n), &n, |b, &n| {
                b.iter_batched_ref(
                    || {
                        let (core, bids, asks) = seeded_core(n);
                        let target = match case {
                            DeleteCase::BidHead => bids[0],
                            DeleteCase::BidTail => *bids.last().unwrap(),
                            DeleteCase::AskHead => asks[0],
                            DeleteCase::Missing => ClientOrderId::from("O-MISSING"),
                        };
                        (core, target)
                    },
                    |(core, target)| {
                        // Missing returns Err: we don't unwrap to keep the
                        // hot path free of the panic branch.
                        let _ = black_box(core.delete_order(*target));
                    },
                    BatchSize::SmallInput,
                );
            });
        }
    }
    group.finish();
}

fn bench_iterate(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_core/iterate");

    for &n in SIZES {
        // no_fill: ask above all bid limits, bid below all ask limits.
        // Measures clone + per-order match dispatch with no MatchAction
        // pushes (allocation-free hot path).
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("no_fill", n), &n, |b, &n| {
            b.iter_batched_ref(
                || seeded_core(n).0,
                |core| black_box(core.iterate()),
                BatchSize::SmallInput,
            );
        });

        // all_fills: ask at 1.00 and bid at 200.00, every order matches.
        // Adds the per-order MatchAction push to the result vec on top of the
        // dispatch cost, exercising the `collect()` allocation path.
        group.bench_with_input(BenchmarkId::new("all_fills", n), &n, |b, &n| {
            b.iter_batched_ref(
                || {
                    let (mut core, _, _) = seeded_core(n);
                    core.set_ask_raw(Price::from("1.00"));
                    core.set_bid_raw(Price::from("200.00"));
                    core
                },
                |core| black_box(core.iterate()),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_predicates(c: &mut Criterion) {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut core = OrderMatchingCore::new(instrument_id, Price::from("0.01"));
    core.set_bid_raw(Price::from("100.00"));
    core.set_ask_raw(Price::from("100.01"));

    let buy_price = Price::from("100.00");

    // Single-call predicates; useful as a baseline against `iterate` per-order cost.
    c.bench_function("matching_core/is_limit_matched/buy", |b| {
        b.iter(|| black_box(core.is_limit_matched(OrderSideSpecified::Buy, black_box(buy_price))));
    });
    c.bench_function("matching_core/is_stop_matched/buy", |b| {
        b.iter(|| black_box(core.is_stop_matched(OrderSideSpecified::Buy, black_box(buy_price))));
    });
    c.bench_function("matching_core/is_touch_triggered/buy", |b| {
        b.iter(|| {
            black_box(core.is_touch_triggered(OrderSideSpecified::Buy, black_box(buy_price)))
        });
    });
    c.bench_function("matching_core/is_limit_fillable/buy", |b| {
        b.iter(|| black_box(core.is_limit_fillable(OrderSideSpecified::Buy, black_box(buy_price))));
    });
}

criterion_group!(
    benches,
    bench_add_order,
    bench_get_order,
    bench_order_exists,
    bench_delete_order,
    bench_iterate,
    bench_predicates,
);
criterion_main!(benches);

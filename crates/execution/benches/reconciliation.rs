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

//! Measures repeated venue-fill reconciliation and terminal status handling over native ID histories.
//! Fixture construction stays outside timing; logging is disabled and each fill is unseen by the order.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_execution::reconciliation::{reconcile_fill_report, reconcile_order_report};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderEventAny,
        order::spec::{OrderPendingUpdateSpec, OrderUpdatedSpec},
    },
    identifiers::{AccountId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};

const NATIVE_ID_COUNTS: &[usize] = &[1, 8, 64];

fn bench_fill_report(c: &mut Criterion) {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let mut group = c.benchmark_group("execution/reconciliation/fill");
    group.throughput(Throughput::Elements(1));

    for &count in NATIVE_ID_COUNTS {
        let order = accepted_order(&instrument, count);

        for (name, venue_order_id) in [
            ("current", order.venue_order_id().unwrap()),
            ("first_native", VenueOrderId::from("V-0")),
            ("unknown", VenueOrderId::from("V-UNKNOWN")),
        ] {
            let report = FillReport::new(
                AccountId::from("SIM-001"),
                instrument.id(),
                venue_order_id,
                TradeId::from("T-NEW"),
                OrderSide::Buy,
                Quantity::from(1),
                Price::from("1.00000"),
                Money::from("0.01 USD"),
                LiquiditySide::Maker,
                Some(order.client_order_id()),
                None,
                UnixNanos::from(1),
                UnixNanos::from(2),
                None,
            );
            let event =
                reconcile_fill_report(&order, &report, &instrument, UnixNanos::from(2), false)
                    .unwrap();

            let OrderEventAny::Filled(fill) = event else {
                panic!("Expected fill event");
            };

            assert_eq!(fill.client_order_id, order.client_order_id());
            assert_eq!(fill.trade_id, report.trade_id);
            assert_eq!(fill.last_qty, report.last_qty);
            assert_eq!(fill.last_px, report.last_px);
            assert_eq!(fill.commission, Some(report.commission));
            group.bench_with_input(BenchmarkId::new(name, count), &report, |b, report| {
                b.iter(|| {
                    black_box(reconcile_fill_report(
                        black_box(&order),
                        black_box(report),
                        black_box(&instrument),
                        UnixNanos::from(2),
                        false,
                    ))
                });
            });
        }
    }

    group.finish();
}

fn bench_terminal_report(c: &mut Criterion) {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let mut group = c.benchmark_group("execution/reconciliation/terminal");
    group.throughput(Throughput::Elements(1));

    for &count in NATIVE_ID_COUNTS {
        let order = accepted_order(&instrument, count);

        for (name, venue_order_id) in [
            ("current", order.venue_order_id().unwrap()),
            ("first_native", VenueOrderId::from("V-0")),
        ] {
            for status in [
                OrderStatus::Canceled,
                OrderStatus::Expired,
                OrderStatus::Filled,
            ] {
                let report = OrderStatusReport::new(
                    AccountId::from("SIM-001"),
                    instrument.id(),
                    Some(order.client_order_id()),
                    venue_order_id,
                    Some(OrderSide::Buy),
                    OrderType::Limit,
                    TimeInForce::Gtc,
                    status,
                    order.quantity(),
                    if status == OrderStatus::Filled {
                        order.quantity()
                    } else {
                        Quantity::from(0)
                    },
                    UnixNanos::from(1),
                    UnixNanos::from(1),
                    UnixNanos::from(2),
                    None,
                )
                .with_price(order.price().unwrap());

                group.bench_with_input(
                    BenchmarkId::new(format!("{status}_{name}"), count),
                    &report,
                    |b, report| {
                        b.iter(|| {
                            black_box(reconcile_order_report(
                                black_box(&order),
                                black_box(report),
                                Some(black_box(&instrument)),
                                UnixNanos::from(2),
                            ))
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

fn accepted_order(instrument: &InstrumentAny, count: usize) -> OrderAny {
    let account_id = AccountId::from("SIM-001");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();
    order
        .apply(TestOrderEventStubs::submitted(&order, account_id))
        .unwrap();
    order
        .apply(TestOrderEventStubs::accepted(
            &order,
            account_id,
            VenueOrderId::from("V-0"),
        ))
        .unwrap();

    for index in 1..count {
        let pending = OrderPendingUpdateSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(account_id)
            .maybe_venue_order_id(order.venue_order_id())
            .build();
        order.apply(OrderEventAny::PendingUpdate(pending)).unwrap();
        let updated = OrderUpdatedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .quantity(order.quantity())
            .maybe_price(order.price())
            .maybe_account_id(Some(account_id))
            .maybe_venue_order_id(Some(VenueOrderId::from(format!("V-{index}"))))
            .build();
        order.apply(OrderEventAny::Updated(updated)).unwrap();
    }

    assert_eq!(order.venue_order_ids().len(), count);
    order
}

criterion_group!(benches, bench_fill_report, bench_terminal_report);
criterion_main!(benches);

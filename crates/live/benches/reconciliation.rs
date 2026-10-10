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

//! Measures periodic mass-status reconciliation, including cache cloning and engine dispatch.
//! In-sync snapshots expose the cost shared by every venue; fill snapshots exercise queued projection.
//! Setup, snapshot construction, and fixture destruction stay outside the timed region.

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{cache::Cache, clock::VirtualClock};
use nautilus_core::UnixNanos;
use nautilus_execution::engine::{ExecutionEngine, stubs::StubExecutionClient};
use nautilus_live::manager::{ExecutionManager, ExecutionManagerConfig};
use nautilus_model::{
    accounts::AccountAny,
    enums::{LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType},
    events::{
        OrderEventAny,
        order::spec::{OrderPendingUpdateSpec, OrderUpdatedSpec},
    },
    identifiers::{ClientId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};

const ORDER_COUNTS: &[usize] = &[1, 64, 256];
const AMEND_COUNTS: &[usize] = &[0, 32];

struct MassFixture {
    manager: ExecutionManager,
    engine: RefCell<ExecutionEngine>,
    cache: Rc<RefCell<Cache>>,
    snapshot: ExecutionMassStatus,
}

fn bench_mass_status(c: &mut Criterion) {
    let mut group = c.benchmark_group("live/reconciliation/mass_status");
    for &count in ORDER_COUNTS {
        group.throughput(Throughput::Elements(count as u64));

        for &amends in AMEND_COUNTS {
            for (name, fill) in [("in_sync", false), ("new_fill", true)] {
                let mut check = mass_fixture(count, amends, fill);
                let result = check
                    .manager
                    .reconcile_execution_mass_status(&check.snapshot, &check.engine);
                assert_eq!(result.events.len(), if fill { count } else { 0 });
                assert_eq!(result.external_orders.len(), 0);
                assert_eq!(result.unresolved_positions.len(), 0);

                for report in check.snapshot.order_reports().values() {
                    let cache = check.cache.borrow();
                    let order = cache.order(&report.client_order_id.unwrap()).unwrap();
                    assert_eq!(order.status(), report.order_status);
                    assert_eq!(order.filled_qty(), report.filled_qty);
                    assert_eq!(order.venue_order_id(), Some(report.venue_order_id));
                    assert_eq!(order.trade_ids().len(), usize::from(fill));
                }

                group.bench_with_input(
                    BenchmarkId::new(format!("{name}_amends_{amends}"), count),
                    &count,
                    |b, &count| {
                        b.iter_batched_ref(
                            || mass_fixture(count, amends, fill),
                            |fixture| {
                                black_box(fixture.manager.reconcile_execution_mass_status(
                                    black_box(&fixture.snapshot),
                                    &fixture.engine,
                                ))
                            },
                            BatchSize::PerIteration,
                        );
                    },
                );
            }
        }
    }

    group.finish();
}

fn mass_fixture(count: usize, amends: usize, fill: bool) -> MassFixture {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let account = AccountAny::default();
    let account_id = account.id();
    let client_id = ClientId::from("RECONCILIATION-BENCH");
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache.borrow_mut().add_account(account).unwrap();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let manager = ExecutionManager::new(
        clock.clone(),
        cache.clone(),
        ExecutionManagerConfig::default(),
    )
    .unwrap();
    let mut engine = ExecutionEngine::new(clock.clone(), cache.clone(), None);
    engine
        .register_client(Box::new(StubExecutionClient::new(
            client_id,
            account_id,
            instrument.id().venue,
            OmsType::Netting,
            Some(clock),
        )))
        .unwrap();

    let mut snapshot = ExecutionMassStatus::new(
        client_id,
        account_id,
        instrument.id().venue,
        UnixNanos::from(2),
        None,
    );
    let mut reports = Vec::with_capacity(count);
    let mut fills = Vec::with_capacity(if fill { count } else { 0 });
    for index in 0..count {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(format!("O-{index}")))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();
        let venue_order_id = VenueOrderId::from(format!("V-{index}"));
        order
            .apply(TestOrderEventStubs::accepted(
                &order,
                account_id,
                venue_order_id,
            ))
            .unwrap();
        append_amends(&mut order, amends);

        let report = OrderStatusReport::new(
            account_id,
            instrument.id(),
            Some(order.client_order_id()),
            venue_order_id,
            Some(order.order_side()),
            order.order_type(),
            order.time_in_force(),
            if fill {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Accepted
            },
            order.quantity(),
            Quantity::from(usize::from(fill) as u64),
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(2),
            None,
        )
        .with_price(order.price().unwrap());

        reports.push(report);

        if fill {
            fills.push(FillReport::new(
                account_id,
                instrument.id(),
                venue_order_id,
                TradeId::from(format!("T-{index}")),
                OrderSide::Buy,
                Quantity::from(1),
                Price::from("1.00000"),
                Money::from("0.01 USD"),
                LiquiditySide::Maker,
                Some(order.client_order_id()),
                None,
                UnixNanos::from(2),
                UnixNanos::from(2),
                None,
            ));
        }

        cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();
    }

    snapshot.add_order_reports(reports);
    snapshot.add_fill_reports(fills);

    MassFixture {
        manager,
        engine: RefCell::new(engine),
        cache,
        snapshot,
    }
}

fn append_amends(order: &mut OrderAny, count: usize) {
    for _ in 0..count {
        let pending = OrderPendingUpdateSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(order.account_id().unwrap())
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
            .maybe_account_id(order.account_id())
            .maybe_venue_order_id(order.venue_order_id())
            .build();
        order.apply(OrderEventAny::Updated(updated)).unwrap();
    }
}

criterion_group!(benches, bench_mass_status);
criterion_main!(benches);

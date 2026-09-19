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

//! Benchmarks engine-level NETTING reopens over increasing archived-cycle histories.

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_common::{cache::Cache, clock::VirtualClock};
use nautilus_execution::engine::{
    ExecutionEngine, config::ExecutionEngineConfig, stubs::StubExecutionClient,
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{LiquiditySide, OmsType, OrderSide, OrderType},
    events::{OrderEventAny, OrderFilled, order::spec::OrderFilledSpec},
    identifiers::{ClientId, ClientOrderId, PositionId, TradeId},
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    types::{Currency, Price, Quantity},
};

const CYCLE_COUNTS: &[usize] = &[8, 64, 512, 4096];
const POSITION_ID: &str = "P-REPLAY-BENCH";

struct EngineFixture {
    engine: ExecutionEngine,
    filled: OrderEventAny,
}

struct SnapshotFixture {
    cache: Cache,
    position: Position,
}

fn bench_position_replay(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution/position_replay");
    group.sample_size(20);

    for &cycles in CYCLE_COUNTS {
        for (name, carry, candidate) in [
            ("reopen_carry_false", false, Candidate::Unique),
            ("reopen_carry_true", true, Candidate::Unique),
            ("duplicate_first", true, Candidate::FirstArchived),
            ("duplicate_last", true, Candidate::LastArchived),
        ] {
            group.bench_with_input(BenchmarkId::new(name, cycles), &cycles, |b, _| {
                b.iter_batched(
                    || engine_fixture(cycles, carry, candidate),
                    |mut fixture| {
                        fixture.engine.process(&fixture.filled);
                        black_box(fixture)
                    },
                    BatchSize::SmallInput,
                );
            });
        }

        group.bench_with_input(
            BenchmarkId::new("archive_snapshot", cycles),
            &cycles,
            |b, _| {
                b.iter_batched(
                    || snapshot_fixture(cycles),
                    |mut fixture| {
                        fixture.cache.snapshot_position(&fixture.position).unwrap();
                        black_box(fixture)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

#[derive(Clone, Copy)]
enum Candidate {
    Unique,
    FirstArchived,
    LastArchived,
}

fn engine_fixture(cycles: usize, carry_replay_events: bool, candidate: Candidate) -> EngineFixture {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let account = AccountAny::default();
    let account_id = account.id();
    let client_id = ClientId::from("REPLAY-BENCH");
    let cache = Rc::new(RefCell::new(Cache::default()));
    // A default-config reopen leaves only the final cycle in the canonical position, so the
    // carry=false case must not start from a cross-cycle log it would then drop inside timing.
    let position = if carry_replay_events {
        accumulated_position(&instrument, cycles)
    } else {
        final_cycle_position(&instrument, cycles)
    };

    {
        let mut cache = cache.borrow_mut();
        cache.add_instrument(instrument.clone()).unwrap();
        cache.add_account(account).unwrap();
        for _ in 0..cycles {
            cache.snapshot_position(&position).unwrap();
        }
        cache
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();
        // Reserve on the cached position: the insertion clone does not preserve capacity, and
        // an append into a full vector would measure allocation growth instead of the transfer.
        let mut cached = cache.position_mut(&PositionId::from(POSITION_ID)).unwrap();
        cached.replay_events.reserve(1);
        assert!(cached.replay_events.capacity() > cached.replay_events.len());
    }

    let config = ExecutionEngineConfig::builder()
        .carry_replay_events_on_reopen(carry_replay_events)
        .build()
        .unwrap();
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let mut engine = ExecutionEngine::new(clock.clone(), Rc::clone(&cache), Some(config));
    engine
        .register_client(Box::new(StubExecutionClient::new(
            client_id,
            account_id,
            instrument.id().venue,
            OmsType::Netting,
            Some(clock),
        )))
        .unwrap();

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from(format!("O-CANDIDATE-{cycles}")))
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .build();
    cache
        .borrow_mut()
        .add_order(
            order.clone(),
            Some(PositionId::from(POSITION_ID)),
            Some(client_id),
            false,
        )
        .unwrap();
    let submitted = TestOrderEventStubs::submitted(&order, account_id);
    engine.process(&submitted);
    order.apply(submitted).unwrap();
    let accepted =
        TestOrderEventStubs::accepted(&order, account_id, format!("V-CANDIDATE-{cycles}").into());
    engine.process(&accepted);
    order.apply(accepted).unwrap();
    let trade_id = match candidate {
        Candidate::Unique => TradeId::from(format!("T-CANDIDATE-{cycles}")),
        Candidate::FirstArchived => TradeId::from("T-OPEN-0"),
        Candidate::LastArchived => TradeId::from(format!("T-CLOSE-{}", cycles - 1)),
    };
    let filled = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(trade_id),
        Some(PositionId::from(POSITION_ID)),
        Some(Price::from("1.00000")),
        Some(Quantity::from(1)),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(account_id),
    );
    EngineFixture { engine, filled }
}

fn snapshot_fixture(cycles: usize) -> SnapshotFixture {
    SnapshotFixture {
        cache: Cache::default(),
        position: accumulated_position(&InstrumentAny::CurrencyPair(audusd_sim()), cycles),
    }
}

fn final_cycle_position(instrument: &InstrumentAny, cycles: usize) -> Position {
    let mut position = Position::new(instrument, fill(instrument, cycles, "OPEN", OrderSide::Buy));
    position.apply(&fill(instrument, cycles, "CLOSE", OrderSide::Sell));
    position
}

fn accumulated_position(instrument: &InstrumentAny, cycles: usize) -> Position {
    let mut position = Position::new(instrument, fill(instrument, 0, "OPEN", OrderSide::Buy));
    position.apply(&fill(instrument, 0, "CLOSE", OrderSide::Sell));
    for cycle in 1..=cycles {
        position.apply(&fill(instrument, cycle, "OPEN", OrderSide::Buy));
        position.apply(&fill(instrument, cycle, "CLOSE", OrderSide::Sell));
    }
    position
}

fn fill(instrument: &InstrumentAny, cycle: usize, phase: &str, side: OrderSide) -> OrderFilled {
    OrderFilledSpec::builder()
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::new(format!("O-{phase}-{cycle}")))
        .trade_id(TradeId::new(format!("T-{phase}-{cycle}")))
        .order_side(side)
        .last_qty(Quantity::from(1))
        .last_px(Price::from("1.00000"))
        .currency(Currency::USD())
        .liquidity_side(LiquiditySide::Maker)
        .position_id(PositionId::from(POSITION_ID))
        .build()
}

criterion_group!(benches, bench_position_replay);
criterion_main!(benches);

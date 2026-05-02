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

//! Benchmarks for the `DataEngine` ingestion path.
//!
//! Targets the trade-to-cache stage of the live data pipeline:
//!
//! - `DataEngine::process_data(Data::Trade(..))` -> `handle_trade` -> `Cache::add_trade`
//!   -> `msgbus::publish_trade` (publish has no subscribers in this bench).
//! - Direct `Cache::add_trade` to isolate cache write cost from engine plus publish.
//!
//! Run with `cargo bench -p nautilus-data --bench engine`.

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    msgbus::{self, MessageBus},
};
use nautilus_data::engine::DataEngine;
use nautilus_model::{
    data::{Data, trade::TradeTick},
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

fn sample_trade() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("EUR/USD.SIM"),
        price: Price::from("1.10000"),
        size: Quantity::from(100_000),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::from("123456"),
        ts_event: 0.into(),
        ts_init: 0.into(),
    }
}

fn install_thread_local_msgbus() {
    msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
}

fn build_engine() -> Rc<RefCell<DataEngine>> {
    install_thread_local_msgbus();
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));
    DataEngine::register_msgbus_handlers(&engine);
    engine
}

fn bench_process_trade(c: &mut Criterion) {
    let mut group = c.benchmark_group("DataEngine ingest");
    group.throughput(Throughput::Elements(1));

    group.bench_function("process_data_trade", |b| {
        let engine = build_engine();
        let trade = sample_trade();
        b.iter(|| {
            engine
                .borrow_mut()
                .process_data(Data::Trade(black_box(trade)));
        });
    });

    group.bench_function("cache_add_trade_only", |b| {
        install_thread_local_msgbus();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let trade = sample_trade();
        b.iter(|| {
            cache
                .borrow_mut()
                .add_trade(black_box(trade))
                .expect("add_trade");
        });
    });

    group.finish();
}

fn bench_process_trade_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("DataEngine ingest batch");

    for size in [1_000_usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            criterion::BenchmarkId::new("process_data_trade", size),
            &size,
            |b, &n| {
                let engine = build_engine();
                let trade = sample_trade();
                b.iter(|| {
                    let mut e = engine.borrow_mut();
                    for _ in 0..n {
                        e.process_data(Data::Trade(black_box(trade)));
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_process_trade, bench_process_trade_batch);
criterion_main!(benches);

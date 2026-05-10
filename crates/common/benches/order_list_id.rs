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

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_common::{clock::TestClock, generators::order_list_id::OrderListIdGenerator};
use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{StrategyId, TraderId};

const SECOND_NS: u64 = 1_000_000_000;

fn make_generator(clock: Rc<RefCell<TestClock>>) -> OrderListIdGenerator {
    OrderListIdGenerator::new(
        TraderId::from("TRADER-101"),
        StrategyId::from("STRATEGY-101"),
        0,
        clock,
    )
}

fn bench_same_second(c: &mut Criterion) {
    c.bench_function("order_list_id/same_second", |b| {
        let mut generator = make_generator(Rc::new(RefCell::new(TestClock::new())));
        b.iter(|| black_box(generator.generate()));
    });
}

fn bench_cross_second(c: &mut Criterion) {
    c.bench_function("order_list_id/cross_second", |b| {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let mut generator = make_generator(clock.clone());
        let mut next_ns = 0_u64;
        b.iter(|| {
            next_ns += SECOND_NS;
            clock.borrow_mut().set_time(UnixNanos::from(next_ns));
            black_box(generator.generate())
        });
    });
}

criterion_group!(benches, bench_same_second, bench_cross_second);
criterion_main!(benches);

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

use chrono::{DateTime, Datelike, Timelike};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_common::{
    clock::{Clock, TestClock},
    generators::client_order_id::ClientOrderIdGenerator,
};
use nautilus_core::{UnixNanos, uuid::UUID4};
use nautilus_model::identifiers::{ClientOrderId, StrategyId, TraderId};

const SECOND_NS: u64 = 1_000_000_000;

fn get_datetime_tag(unix_ms: u64) -> String {
    let now_utc = DateTime::from_timestamp_millis(unix_ms as i64)
        .expect("Milliseconds timestamp should be within valid range");
    format!(
        "{}{:02}{:02}-{:02}{:02}{:02}",
        now_utc.year(),
        now_utc.month(),
        now_utc.day(),
        now_utc.hour(),
        now_utc.minute(),
        now_utc.second(),
    )
}

/// Previous generator logic before this PR, used as a stable baseline in the same benchmark.
pub struct LegacyClientOrderIdGenerator {
    clock: Rc<RefCell<dyn Clock>>,
    trader_id: TraderId,
    strategy_id: StrategyId,
    count: usize,
    use_uuids: bool,
    use_hyphens: bool,
}

impl LegacyClientOrderIdGenerator {
    /// Creates a new [`LegacyClientOrderIdGenerator`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        initial_count: usize,
        clock: Rc<RefCell<dyn Clock>>,
        use_uuids: bool,
        use_hyphens: bool,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            count: initial_count,
            clock,
            use_uuids,
            use_hyphens,
        }
    }

    pub const fn set_count(&mut self, count: usize) {
        self.count = count;
    }

    pub const fn reset(&mut self) {
        self.count = 0;
    }

    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    pub fn generate(&mut self) -> ClientOrderId {
        let value = if self.use_uuids {
            let mut uuid_value = UUID4::new().to_string();

            if !self.use_hyphens {
                uuid_value = uuid_value.replace('-', "");
            }
            uuid_value
        } else {
            let datetime_tag = get_datetime_tag(self.clock.borrow().timestamp_ms());
            let trader_tag = self.trader_id.get_tag();
            let strategy_tag = self.strategy_id.get_tag();
            self.count += 1;

            if self.use_hyphens {
                format!(
                    "O-{}-{}-{}-{}",
                    datetime_tag, trader_tag, strategy_tag, self.count
                )
            } else {
                let datetime_no_hyphens = datetime_tag.replace('-', "");
                format!(
                    "O{}{}{}{}",
                    datetime_no_hyphens, trader_tag, strategy_tag, self.count
                )
            }
        };

        ClientOrderId::from(value)
    }
}

fn old_generator(use_hyphens: bool) -> LegacyClientOrderIdGenerator {
    LegacyClientOrderIdGenerator::new(
        TraderId::from("TRADER-101"),
        StrategyId::from("STRATEGY-101"),
        0,
        Rc::new(RefCell::new(TestClock::new())),
        false,
        use_hyphens,
    )
}

fn new_generator(use_hyphens: bool) -> ClientOrderIdGenerator {
    ClientOrderIdGenerator::new(
        TraderId::from("TRADER-202"),
        StrategyId::from("STRATEGY-202"),
        0,
        Rc::new(RefCell::new(TestClock::new())),
        false,
        use_hyphens,
    )
}

fn bench_same_second_hyphenated(c: &mut Criterion) {
    let mut group = c.benchmark_group("client_order_id/same_second_hyphenated");

    group.bench_function("old_format", |b| {
        let mut generator = old_generator(true);
        b.iter(|| black_box(generator.generate()));
    });

    group.bench_function("new_cached_prefix", |b| {
        let mut generator = new_generator(true);
        b.iter(|| black_box(generator.generate()));
    });

    group.finish();
}

fn bench_same_second_no_hyphens(c: &mut Criterion) {
    let mut group = c.benchmark_group("client_order_id/same_second_no_hyphens");

    group.bench_function("old_format_replace", |b| {
        let mut generator = old_generator(false);
        b.iter(|| black_box(generator.generate()));
    });

    group.bench_function("new_cached_prefix", |b| {
        let mut generator = new_generator(false);
        b.iter(|| black_box(generator.generate()));
    });

    group.finish();
}

fn bench_cross_second_hyphenated(c: &mut Criterion) {
    let mut group = c.benchmark_group("client_order_id/cross_second_hyphenated");

    group.bench_function("old_format", |b| {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let mut generator = LegacyClientOrderIdGenerator::new(
            TraderId::from("TRADER-303"),
            StrategyId::from("STRATEGY-303"),
            0,
            clock.clone(),
            false,
            true,
        );
        let mut next_ns = 0_u64;
        b.iter(|| {
            next_ns += SECOND_NS;
            clock.borrow_mut().set_time(UnixNanos::from(next_ns));
            black_box(generator.generate())
        });
    });

    group.bench_function("new_cached_prefix", |b| {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let mut generator = ClientOrderIdGenerator::new(
            TraderId::from("TRADER-303"),
            StrategyId::from("STRATEGY-303"),
            0,
            clock.clone(),
            false,
            true,
        );
        let mut next_ns = 0_u64;
        b.iter(|| {
            next_ns += SECOND_NS;
            clock.borrow_mut().set_time(UnixNanos::from(next_ns));
            black_box(generator.generate())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_same_second_hyphenated,
    bench_same_second_no_hyphens,
    bench_cross_second_hyphenated
);
criterion_main!(benches);

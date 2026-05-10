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

use std::{
    cell::RefCell,
    collections::HashMap,
    fmt::{Debug, Write},
    rc::Rc,
};

use chrono::{DateTime, Datelike, Timelike};
use itoa::Buffer;
use nautilus_model::identifiers::{PositionId, StrategyId, TraderId};

use crate::clock::Clock;

const DATETIME_TAG_LEN: usize = 15; // "YYYYMMDD-HHMMSS"
// Reserve for strategy_tag + "-" + decimal count + optional "F" past the cached fixed prefix
const STRATEGY_AND_COUNT_RESERVE: usize = 32;

#[repr(C)]
pub struct PositionIdGenerator {
    clock: Rc<RefCell<dyn Clock>>,
    trader_id: TraderId,
    counts: HashMap<StrategyId, usize>,
    trader_tag: String,
    buf: String,
    fixed_prefix_len: usize,
    epoch_second: u64,
    count_buf: Buffer,
}

impl Debug for PositionIdGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PositionIdGenerator))
            .field("trader_id", &self.trader_id)
            .field("counts", &self.counts)
            .field("trader_tag", &self.trader_tag)
            .field("buf", &self.buf)
            .field("fixed_prefix_len", &self.fixed_prefix_len)
            .field("epoch_second", &self.epoch_second)
            .finish_non_exhaustive()
    }
}

impl PositionIdGenerator {
    /// Creates a new [`PositionIdGenerator`] instance.
    #[must_use]
    pub fn new(trader_id: TraderId, clock: Rc<RefCell<dyn Clock>>) -> Self {
        let trader_tag = trader_id.get_tag().to_string();
        let buf = String::with_capacity(
            "P-".len()
                + DATETIME_TAG_LEN
                + "-".len()
                + trader_tag.len()
                + "-".len()
                + STRATEGY_AND_COUNT_RESERVE,
        );

        Self {
            clock,
            trader_id,
            counts: HashMap::new(),
            trader_tag,
            buf,
            fixed_prefix_len: 0,
            epoch_second: u64::MAX,
            count_buf: Buffer::new(),
        }
    }

    pub fn set_count(&mut self, count: usize, strategy_id: StrategyId) {
        self.counts.insert(strategy_id, count);
    }

    pub fn reset(&mut self) {
        self.counts.clear();
    }

    #[must_use]
    pub fn count(&self, strategy_id: StrategyId) -> usize {
        *self.counts.get(&strategy_id).unwrap_or(&0)
    }

    pub fn generate(&mut self, strategy_id: StrategyId, flipped: bool) -> PositionId {
        let next_count = self.count(strategy_id) + 1;
        self.set_count(next_count, strategy_id);

        let timestamp_ms = self.clock.borrow().timestamp_ms();
        self.refresh_fixed_prefix(timestamp_ms);

        self.buf.truncate(self.fixed_prefix_len);
        self.buf.push_str(strategy_id.get_tag());
        self.buf.push('-');
        self.buf.push_str(self.count_buf.format(next_count));
        if flipped {
            self.buf.push('F');
        }

        PositionId::from(self.buf.as_str())
    }

    #[inline]
    fn refresh_fixed_prefix(&mut self, timestamp_ms: u64) {
        let epoch_second = timestamp_ms / 1_000;
        if epoch_second == self.epoch_second {
            return;
        }

        write_fixed_prefix(&mut self.buf, &self.trader_tag, epoch_second);
        self.fixed_prefix_len = self.buf.len();
        self.epoch_second = epoch_second;
    }
}

fn write_fixed_prefix(buf: &mut String, trader_tag: &str, epoch_second: u64) {
    let now_utc = DateTime::from_timestamp_millis((epoch_second * 1_000) as i64)
        .expect("Milliseconds timestamp should be within valid range");

    buf.clear();

    write!(
        buf,
        "P-{:04}{:02}{:02}-{:02}{:02}{:02}-{trader_tag}-",
        now_utc.year(),
        now_utc.month(),
        now_utc.day(),
        now_utc.hour(),
        now_utc.minute(),
        now_utc.second(),
    )
    .expect("writing to String should not fail");
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::{PositionId, StrategyId, TraderId},
        stubs::TestDefault,
    };
    use rstest::rstest;

    use crate::{clock::TestClock, generators::position_id::PositionIdGenerator};

    fn get_position_id_generator() -> PositionIdGenerator {
        PositionIdGenerator::new(
            TraderId::test_default(),
            Rc::new(RefCell::new(TestClock::new())),
        )
    }

    #[rstest]
    fn test_generate_position_id_one_strategy() {
        let mut generator = get_position_id_generator();
        let result1 = generator.generate(StrategyId::from("S-001"), false);
        let result2 = generator.generate(StrategyId::from("S-001"), false);

        assert_eq!(result1, PositionId::from("P-19700101-000000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-000000-001-001-2"));
    }

    #[rstest]
    fn test_generate_position_id_multiple_strategies() {
        let mut generator = get_position_id_generator();
        let result1 = generator.generate(StrategyId::from("S-001"), false);
        let result2 = generator.generate(StrategyId::from("S-002"), false);
        let result3 = generator.generate(StrategyId::from("S-002"), false);

        assert_eq!(result1, PositionId::from("P-19700101-000000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-000000-001-002-1"));
        assert_eq!(result3, PositionId::from("P-19700101-000000-001-002-2"));
    }

    #[rstest]
    fn test_generate_position_id_with_flipped_appends_correctly() {
        let mut generator = get_position_id_generator();
        let result1 = generator.generate(StrategyId::from("S-001"), false);
        let result2 = generator.generate(StrategyId::from("S-002"), true);
        let result3 = generator.generate(StrategyId::from("S-001"), true);

        assert_eq!(result1, PositionId::from("P-19700101-000000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-000000-001-002-1F"));
        assert_eq!(result3, PositionId::from("P-19700101-000000-001-001-2F"));
    }

    #[rstest]
    fn test_generate_persists_fixed_prefix_in_buffer_within_same_second() {
        let mut generator = get_position_id_generator();

        let result1 = generator.generate(StrategyId::from("S-001"), false);
        let fixed_prefix = "P-19700101-000000-001-";
        let capacity_after_first = generator.buf.capacity();

        assert_eq!(result1, PositionId::from("P-19700101-000000-001-001-1"));
        assert_eq!(generator.fixed_prefix_len, fixed_prefix.len());
        assert_eq!(&generator.buf[..generator.fixed_prefix_len], fixed_prefix);

        let result2 = generator.generate(StrategyId::from("S-001"), false);

        assert_eq!(result2, PositionId::from("P-19700101-000000-001-001-2"));
        assert_eq!(generator.fixed_prefix_len, fixed_prefix.len());
        assert_eq!(&generator.buf[..generator.fixed_prefix_len], fixed_prefix);
        assert_eq!(generator.buf.capacity(), capacity_after_first);
    }

    #[rstest]
    fn test_generate_capacity_stable_across_strategies_same_second() {
        let mut generator = get_position_id_generator();

        // Prime the buffer; subsequent calls must not reallocate while strategies fit
        // within STRATEGY_AND_COUNT_RESERVE.
        generator.generate(StrategyId::from("S-001"), false);
        let capacity_after_warmup = generator.buf.capacity();

        for tag in ["S-002", "S-003", "STRATEGY-LONGER-001"] {
            generator.generate(StrategyId::from(tag), false);
        }

        assert_eq!(generator.buf.capacity(), capacity_after_warmup);
    }

    #[rstest]
    fn test_generate_refreshes_persistent_fixed_prefix_when_second_changes() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let mut generator = PositionIdGenerator::new(TraderId::test_default(), clock.clone());

        let result1 = generator.generate(StrategyId::from("S-001"), false);
        clock.borrow_mut().set_time(UnixNanos::from(1_000_000_000));
        let result2 = generator.generate(StrategyId::from("S-001"), false);

        assert_eq!(result1, PositionId::from("P-19700101-000000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-000001-001-001-2"));
        assert_eq!(generator.epoch_second, 1);
        assert_eq!(
            &generator.buf[..generator.fixed_prefix_len],
            "P-19700101-000001-001-"
        );
    }

    #[rstest]
    fn test_get_count_when_strategy_id_has_not_been_used() {
        let generator = get_position_id_generator();
        let result = generator.count(StrategyId::from("S-001"));

        assert_eq!(result, 0);
    }

    #[rstest]
    fn set_count_with_valid_strategy() {
        let mut generator = get_position_id_generator();
        generator.set_count(7, StrategyId::from("S-001"));
        let result = generator.count(StrategyId::from("S-001"));

        assert_eq!(result, 7);
    }

    #[rstest]
    fn test_reset() {
        let mut generator = get_position_id_generator();
        generator.generate(StrategyId::from("S-001"), false);
        generator.generate(StrategyId::from("S-001"), false);
        generator.reset();
        let result = generator.generate(StrategyId::from("S-001"), false);

        assert_eq!(result, PositionId::from("P-19700101-000000-001-001-1"));
    }
}

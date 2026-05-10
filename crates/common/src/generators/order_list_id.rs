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
    fmt::{Debug, Write},
    rc::Rc,
};

use chrono::{DateTime, Datelike, Timelike};
use itoa::Buffer;
use nautilus_model::identifiers::{OrderListId, StrategyId, TraderId};

use crate::clock::Clock;

const DATETIME_TAG_LEN: usize = 15; // "YYYYMMDD-HHMMSS"
const MAX_USIZE_DECIMAL_LEN: usize = 20; // Maximum decimal digits for a 64-bit usize

pub struct OrderListIdGenerator {
    clock: Rc<RefCell<dyn Clock>>,
    trader_id: TraderId,
    strategy_id: StrategyId,
    count: usize,
    trader_tag: String,
    strategy_tag: String,
    buf: String,
    fixed_prefix_len: usize,
    epoch_second: u64,
    count_buf: Buffer,
}

impl Debug for OrderListIdGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderListIdGenerator))
            .field("clock", &self.clock)
            .field("trader_id", &self.trader_id)
            .field("strategy_id", &self.strategy_id)
            .field("count", &self.count)
            .field("trader_tag", &self.trader_tag)
            .field("strategy_tag", &self.strategy_tag)
            .field("buf", &self.buf)
            .field("fixed_prefix_len", &self.fixed_prefix_len)
            .field("epoch_second", &self.epoch_second)
            .finish_non_exhaustive()
    }
}

impl OrderListIdGenerator {
    /// Creates a new [`OrderListIdGenerator`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        initial_count: usize,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        let trader_tag = trader_id.get_tag().to_string();
        let strategy_tag = strategy_id.get_tag().to_string();
        let buf = String::with_capacity(
            fixed_prefix_capacity(&trader_tag, &strategy_tag) + MAX_USIZE_DECIMAL_LEN,
        );

        Self {
            clock,
            trader_id,
            strategy_id,
            count: initial_count,
            trader_tag,
            strategy_tag,
            buf,
            fixed_prefix_len: 0,
            epoch_second: u64::MAX,
            count_buf: Buffer::new(),
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

    pub fn generate(&mut self) -> OrderListId {
        let timestamp_ms = self.clock.borrow().timestamp_ms();
        self.refresh_fixed_prefix(timestamp_ms);
        self.count += 1;

        self.buf.truncate(self.fixed_prefix_len);
        self.buf.push_str(self.count_buf.format(self.count));

        OrderListId::from(self.buf.as_str())
    }

    #[inline]
    fn refresh_fixed_prefix(&mut self, timestamp_ms: u64) {
        let epoch_second = timestamp_ms / 1_000;
        if epoch_second == self.epoch_second {
            return;
        }

        write_fixed_prefix(
            &mut self.buf,
            &self.trader_tag,
            &self.strategy_tag,
            epoch_second,
        );
        self.fixed_prefix_len = self.buf.len();
        self.epoch_second = epoch_second;
    }
}

#[inline]
fn fixed_prefix_capacity(trader_tag: &str, strategy_tag: &str) -> usize {
    "OL-".len()
        + DATETIME_TAG_LEN
        + "-".len()
        + trader_tag.len()
        + "-".len()
        + strategy_tag.len()
        + "-".len()
}

fn write_fixed_prefix(buf: &mut String, trader_tag: &str, strategy_tag: &str, epoch_second: u64) {
    let now_utc = DateTime::from_timestamp_millis((epoch_second * 1_000) as i64)
        .expect("Milliseconds timestamp should be within valid range");

    buf.clear();

    write!(
        buf,
        "OL-{:04}{:02}{:02}-{:02}{:02}{:02}-{trader_tag}-{strategy_tag}-",
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
        identifiers::{OrderListId, StrategyId, TraderId},
        stubs::TestDefault,
    };
    use rstest::rstest;

    use crate::{clock::TestClock, generators::order_list_id::OrderListIdGenerator};

    fn get_order_list_id_generator(initial_count: Option<usize>) -> OrderListIdGenerator {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        OrderListIdGenerator::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            initial_count.unwrap_or(0),
            clock,
        )
    }

    #[rstest]
    fn test_init() {
        let generator = get_order_list_id_generator(None);
        assert_eq!(generator.count(), 0);
    }

    #[rstest]
    fn test_init_with_initial_count() {
        let generator = get_order_list_id_generator(Some(7));
        assert_eq!(generator.count(), 7);
    }

    #[rstest]
    fn test_generate_order_list_id_from_start() {
        let mut generator = get_order_list_id_generator(None);
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();

        assert_eq!(result1, OrderListId::new("OL-19700101-000000-001-001-1"));
        assert_eq!(result2, OrderListId::new("OL-19700101-000000-001-001-2"));
        assert_eq!(result3, OrderListId::new("OL-19700101-000000-001-001-3"));
    }

    #[rstest]
    fn test_generate_order_list_id_from_initial() {
        let mut generator = get_order_list_id_generator(Some(5));
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();

        assert_eq!(result1, OrderListId::new("OL-19700101-000000-001-001-6"));
        assert_eq!(result2, OrderListId::new("OL-19700101-000000-001-001-7"));
        assert_eq!(result3, OrderListId::new("OL-19700101-000000-001-001-8"));
    }

    #[rstest]
    fn test_generate_persists_fixed_prefix_in_buffer_within_same_second() {
        let mut generator = get_order_list_id_generator(None);

        let result1 = generator.generate();
        let fixed_prefix = "OL-19700101-000000-001-001-";
        let capacity_after_first = generator.buf.capacity();

        assert_eq!(result1, OrderListId::new("OL-19700101-000000-001-001-1"));
        assert_eq!(generator.fixed_prefix_len, fixed_prefix.len());
        assert_eq!(&generator.buf[..generator.fixed_prefix_len], fixed_prefix);

        let result2 = generator.generate();

        assert_eq!(result2, OrderListId::new("OL-19700101-000000-001-001-2"));
        assert_eq!(generator.fixed_prefix_len, fixed_prefix.len());
        assert_eq!(&generator.buf[..generator.fixed_prefix_len], fixed_prefix);
        assert_eq!(generator.buf.capacity(), capacity_after_first);
    }

    #[rstest]
    fn test_generate_refreshes_persistent_fixed_prefix_when_second_changes() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let mut generator = OrderListIdGenerator::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            0,
            clock.clone(),
        );

        let result1 = generator.generate();
        clock.borrow_mut().set_time(UnixNanos::from(1_000_000_000));
        let result2 = generator.generate();

        assert_eq!(result1, OrderListId::new("OL-19700101-000000-001-001-1"));
        assert_eq!(result2, OrderListId::new("OL-19700101-000001-001-001-2"));
        assert_eq!(generator.epoch_second, 1);
        assert_eq!(
            &generator.buf[..generator.fixed_prefix_len],
            "OL-19700101-000001-001-001-"
        );
    }

    #[rstest]
    fn test_reset() {
        let mut generator = get_order_list_id_generator(None);
        generator.generate();
        generator.generate();
        generator.reset();
        let result = generator.generate();

        assert_eq!(result, OrderListId::new("OL-19700101-000000-001-001-1"));
    }
}

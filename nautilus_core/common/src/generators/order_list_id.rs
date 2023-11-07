// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use chrono::{Datelike, NaiveDateTime, Timelike};
use nautilus_model::identifiers::{
    order_list_id::OrderListId, strategy_id::StrategyId, trader_id::TraderId,
};

use crate::{clock::Clock, generators::IdentifierGenerator};

#[repr(C)]
pub struct OrderListIdGenerator {
    trader_id: TraderId,
    strategy_id: StrategyId,
    clock: Box<dyn Clock>,
    count: usize,
}

impl OrderListIdGenerator {
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        clock: Box<dyn Clock>,
        initial_count: Option<usize>,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            clock,
            count: initial_count.unwrap_or(0),
        }
    }
}

impl IdentifierGenerator<OrderListId> for OrderListIdGenerator {
    fn set_count(&mut self, count: usize) {
        self.count = count;
    }

    fn reset(&mut self) {
        self.count = 0;
    }

    fn count(&self) -> usize {
        self.count
    }

    fn generate(&mut self) -> OrderListId {
        let datetime_tag = self.get_datetime_tag();
        let trader_tag = self.trader_id.get_tag();
        let strategy_tag = self.strategy_id.get_tag();
        self.count += 1;
        let id = format!(
            "OL-{}-{}-{}-{}",
            datetime_tag, trader_tag, strategy_tag, self.count
        );
        OrderListId::new(&id).unwrap()
    }

    fn get_datetime_tag(&mut self) -> String {
        let millis = self.clock.timestamp_ms() as i64;
        let now_utc = NaiveDateTime::from_timestamp_millis(millis).unwrap();
        format!(
            "{}{:02}{:02}-{:02}{:02}",
            now_utc.year(),
            now_utc.month(),
            now_utc.day(),
            now_utc.hour(),
            now_utc.minute()
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::{
        order_list_id::OrderListId, strategy_id::StrategyId, trader_id::TraderId,
    };
    use rstest::rstest;

    use crate::{
        clock::TestClock,
        generators::{order_list_id::OrderListIdGenerator, IdentifierGenerator},
    };

    fn get_order_list_id_generator(initial_count: Option<usize>) -> OrderListIdGenerator {
        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("EMACross-001");
        let clock = TestClock::new();
        OrderListIdGenerator::new(trader_id, strategy_id, Box::new(clock), initial_count)
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
    fn test_datetime_tag() {
        let mut generator = get_order_list_id_generator(None);
        let tag = generator.get_datetime_tag();
        let result = "19700101-0000";
        assert_eq!(tag, result);
    }

    #[rstest]
    fn test_generate_order_list_id_from_start() {
        let mut generator = get_order_list_id_generator(None);
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();
        assert_eq!(
            result1,
            OrderListId::new("OL-19700101-0000-001-001-1").unwrap()
        );
        assert_eq!(
            result2,
            OrderListId::new("OL-19700101-0000-001-001-2").unwrap()
        );
        assert_eq!(
            result3,
            OrderListId::new("OL-19700101-0000-001-001-3").unwrap()
        );
    }

    #[rstest]
    fn test_generate_order_list_id_from_initial() {
        let mut generator = get_order_list_id_generator(Some(5));
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();
        assert_eq!(
            result1,
            OrderListId::new("OL-19700101-0000-001-001-6").unwrap()
        );
        assert_eq!(
            result2,
            OrderListId::new("OL-19700101-0000-001-001-7").unwrap()
        );
        assert_eq!(
            result3,
            OrderListId::new("OL-19700101-0000-001-001-8").unwrap()
        );
    }

    #[rstest]
    fn test_reset() {
        let mut generator = get_order_list_id_generator(None);
        generator.generate();
        generator.generate();
        generator.reset();
        let result = generator.generate();
        assert_eq!(
            result,
            OrderListId::new("OL-19700101-0000-001-001-1").unwrap()
        );
    }
}

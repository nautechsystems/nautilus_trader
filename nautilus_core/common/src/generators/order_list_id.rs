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

use nautilus_model::identifiers::{
    order_list_id::OrderListId, strategy_id::StrategyId, trader_id::TraderId,
};

use super::get_datetime_tag;
use crate::clock::Clock;

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

    pub fn set_count(&mut self, count: usize, _strategy_id: Option<StrategyId>) {
        self.count = count;
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }

    pub fn count(&self, _strategy_id: Option<StrategyId>) -> usize {
        self.count
    }

    pub fn generate(
        &mut self,
        _strategy_id: Option<StrategyId>,
        _flipped: Option<bool>,
    ) -> OrderListId {
        let datetime_tag = get_datetime_tag(self.clock.timestamp_ms());
        let trader_tag = self.trader_id.get_tag();
        let strategy_tag = self.strategy_id.get_tag();
        self.count += 1;
        let id = format!(
            "OL-{}-{}-{}-{}",
            datetime_tag, trader_tag, strategy_tag, self.count
        );
        OrderListId::from(id.as_str())
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

    use crate::{clock::TestClock, generators::order_list_id::OrderListIdGenerator};

    fn get_order_list_id_generator(initial_count: Option<usize>) -> OrderListIdGenerator {
        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("EMACross-001");
        let clock = TestClock::new();
        OrderListIdGenerator::new(trader_id, strategy_id, Box::new(clock), initial_count)
    }

    #[rstest]
    fn test_init() {
        let generator = get_order_list_id_generator(None);
        assert_eq!(generator.count(None), 0);
    }

    #[rstest]
    fn test_init_with_initial_count() {
        let generator = get_order_list_id_generator(Some(7));
        assert_eq!(generator.count(None), 7);
    }

    #[rstest]
    fn test_generate_order_list_id_from_start() {
        let mut generator = get_order_list_id_generator(None);
        let result1 = generator.generate(None, None);
        let result2 = generator.generate(None, None);
        let result3 = generator.generate(None, None);

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
        let result1 = generator.generate(None, None);
        let result2 = generator.generate(None, None);
        let result3 = generator.generate(None, None);

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
        generator.generate(None, None);
        generator.generate(None, None);
        generator.reset();
        let result = generator.generate(None, None);

        assert_eq!(
            result,
            OrderListId::new("OL-19700101-0000-001-001-1").unwrap()
        );
    }
}

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

use std::{cell::RefCell, rc::Rc};

use nautilus_model::identifiers::{OrderListId, StrategyId, TraderId};

use super::get_datetime_tag;
use crate::clock::Clock;

#[derive(Debug)]
pub struct OrderListIdGenerator {
    clock: Rc<RefCell<dyn Clock>>,
    trader_id: TraderId,
    strategy_id: StrategyId,
    count: usize,
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
        Self {
            clock,
            trader_id,
            strategy_id,
            count: initial_count,
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
        let datetime_tag = get_datetime_tag(self.clock.borrow().timestamp_ms());
        let trader_tag = self.trader_id.get_tag();
        let strategy_tag = self.strategy_id.get_tag();
        self.count += 1;
        let value = format!(
            "OL-{}-{}-{}-{}",
            datetime_tag, trader_tag, strategy_tag, self.count
        );
        OrderListId::from(value)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

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
    fn test_reset() {
        let mut generator = get_order_list_id_generator(None);
        generator.generate();
        generator.generate();
        generator.reset();
        let result = generator.generate();

        assert_eq!(result, OrderListId::new("OL-19700101-000000-001-001-1"));
    }
}

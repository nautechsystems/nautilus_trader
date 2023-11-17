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

use std::collections::HashMap;

use nautilus_model::identifiers::{
    position_id::PositionId, strategy_id::StrategyId, trader_id::TraderId,
};

use super::get_datetime_tag;
use crate::clock::Clock;

#[repr(C)]
pub struct PositionIdGenerator<'a> {
    trader_id: TraderId,
    clock: &'a mut Box<dyn Clock>,
    counts: HashMap<StrategyId, usize>,
}

impl<'a> PositionIdGenerator<'a> {
    #[must_use]
    pub fn new(trader_id: TraderId, clock: &'a mut Box<dyn Clock>) -> Self {
        Self {
            trader_id,
            clock,
            counts: HashMap::new(),
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
        let strategy = strategy_id;
        let next_count = self.count(strategy_id) + 1;
        self.set_count(next_count, strategy_id);
        let datetime_tag = get_datetime_tag(self.clock.timestamp_ms());
        let trader_tag = self.trader_id.get_tag();
        let strategy_tag = strategy.get_tag();
        let flipped = if flipped { "F" } else { "" };
        let id = format!("P-{datetime_tag}-{trader_tag}-{strategy_tag}-{next_count}{flipped}");
        PositionId::from(id.as_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::{
        position_id::PositionId, strategy_id::StrategyId, trader_id::TraderId,
    };
    use rstest::rstest;

    use crate::{
        clock::{stubs::test_clock, Clock, TestClock},
        generators::position_id::PositionIdGenerator,
    };

    fn get_position_id_generator<'a>(clock: &'a mut Box<dyn Clock>) -> PositionIdGenerator {
        let trader_id = TraderId::from("TRADER-001");
        PositionIdGenerator::new(trader_id, clock)
    }

    #[rstest]
    fn test_generate_position_id_one_strategy(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_position_id_generator(&mut test_clock);
        let result1 = generator.generate(StrategyId::from("S-001"), false);
        let result2 = generator.generate(StrategyId::from("S-001"), false);

        assert_eq!(result1, PositionId::from("P-19700101-0000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-0000-001-001-2"));
    }

    #[rstest]
    fn test_generate_position_id_multiple_strategies(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_position_id_generator(&mut test_clock);
        let result1 = generator.generate(StrategyId::from("S-001"), false);
        let result2 = generator.generate(StrategyId::from("S-002"), false);
        let result3 = generator.generate(StrategyId::from("S-002"), false);

        assert_eq!(result1, PositionId::from("P-19700101-0000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-0000-001-002-1"));
        assert_eq!(result3, PositionId::from("P-19700101-0000-001-002-2"));
    }

    #[rstest]
    fn test_generate_position_id_with_flipped_appends_correctly(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_position_id_generator(&mut test_clock);
        let result1 = generator.generate(StrategyId::from("S-001"), false);
        let result2 = generator.generate(StrategyId::from("S-002"), true);
        let result3 = generator.generate(StrategyId::from("S-001"), true);

        assert_eq!(result1, PositionId::from("P-19700101-0000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-0000-001-002-1F"));
        assert_eq!(result3, PositionId::from("P-19700101-0000-001-001-2F"));
    }

    #[rstest]
    fn test_get_count_when_strategy_id_has_not_been_used(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let generator = get_position_id_generator(&mut test_clock);
        let result = generator.count(StrategyId::from("S-001"));

        assert_eq!(result, 0);
    }

    #[rstest]
    fn set_count_with_valid_strategy(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_position_id_generator(&mut test_clock);
        generator.set_count(7, StrategyId::from("S-001"));
        let result = generator.count(StrategyId::from("S-001"));

        assert_eq!(result, 7);
    }

    #[rstest]
    fn test_reset(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_position_id_generator(&mut test_clock);
        generator.generate(StrategyId::from("S-001"), false);
        generator.generate(StrategyId::from("S-001"), false);
        generator.reset();
        let result = generator.generate(StrategyId::from("S-001"), false);

        assert_eq!(result, PositionId::from("P-19700101-0000-001-001-1"));
    }
}

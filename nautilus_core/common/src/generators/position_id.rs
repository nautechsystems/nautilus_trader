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

use chrono::{Datelike, NaiveDateTime, Timelike};
use nautilus_model::identifiers::{
    position_id::PositionId, strategy_id::StrategyId, trader_id::TraderId,
};

use crate::{clock::Clock, generators::IdentifierGenerator};

#[repr(C)]
pub struct PositionIdGenerator {
    trader_id: TraderId,
    clock: Box<dyn Clock>,
    counts: HashMap<StrategyId, usize>,
}

impl PositionIdGenerator {
    pub fn new(trader_id: TraderId, clock: Box<dyn Clock>) -> Self {
        Self {
            trader_id,
            clock,
            counts: HashMap::new(),
        }
    }
}

impl IdentifierGenerator<PositionId> for PositionIdGenerator {
    fn set_count(&mut self, count: usize, strategy_id: Option<StrategyId>) {
        self.counts.insert(strategy_id.unwrap(), count);
    }

    fn reset(&mut self) {
        self.counts.clear();
    }

    fn count(&self, strategy_id: Option<StrategyId>) -> usize {
        *self.counts.get(&strategy_id.unwrap()).unwrap_or(&0)
    }

    fn generate(&mut self, strategy_id: Option<StrategyId>, flipped: Option<bool>) -> PositionId {
        let strategy = strategy_id.unwrap();
        let next_count = self.count(strategy_id) + 1;
        self.set_count(next_count, strategy_id);
        let datetime_tag = self.get_datetime_tag();
        let trader_tag = self.trader_id.get_tag();
        let strategy_tag = strategy.get_tag();
        let flipped = if flipped.unwrap_or(false) { "F" } else { "" };
        let id = format!(
            "P-{}-{}-{}-{}{}",
            datetime_tag, trader_tag, strategy_tag, next_count, flipped
        );
        PositionId::new(&id).unwrap()
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
        position_id::PositionId, strategy_id::StrategyId, trader_id::TraderId,
    };
    use rstest::rstest;

    use crate::{
        clock::TestClock,
        generators::{position_id::PositionIdGenerator, IdentifierGenerator},
    };

    fn get_position_id_generator() -> PositionIdGenerator {
        let trader_id = TraderId::from("TRADER-001");
        let clock = TestClock::new();
        PositionIdGenerator::new(trader_id, Box::new(clock))
    }

    #[rstest]
    fn test_generate_position_id_one_strategy() {
        let mut generator = get_position_id_generator();
        let result1 = generator.generate(Some(StrategyId::from("S-001")), None);
        let result2 = generator.generate(Some(StrategyId::from("S-001")), None);

        assert_eq!(result1, PositionId::from("P-19700101-0000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-0000-001-001-2"));
    }

    #[rstest]
    fn test_generate_position_id_multiple_strategies() {
        let mut generator = get_position_id_generator();
        let result1 = generator.generate(Some(StrategyId::from("S-001")), None);
        let result2 = generator.generate(Some(StrategyId::from("S-002")), None);
        let result3 = generator.generate(Some(StrategyId::from("S-002")), None);

        assert_eq!(result1, PositionId::from("P-19700101-0000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-0000-001-002-1"));
        assert_eq!(result3, PositionId::from("P-19700101-0000-001-002-2"));
    }

    #[rstest]
    fn test_generate_position_id_with_flipped_appends_correctly() {
        let mut generator = get_position_id_generator();
        let result1 = generator.generate(Some(StrategyId::from("S-001")), None);
        let result2 = generator.generate(Some(StrategyId::from("S-002")), Some(true));
        let result3 = generator.generate(Some(StrategyId::from("S-001")), Some(true));

        assert_eq!(result1, PositionId::from("P-19700101-0000-001-001-1"));
        assert_eq!(result2, PositionId::from("P-19700101-0000-001-002-1F"));
        assert_eq!(result3, PositionId::from("P-19700101-0000-001-001-2F"));
    }

    #[rstest]
    fn test_get_count_when_strategy_id_has_not_been_used() {
        let generator = get_position_id_generator();
        let result = generator.count(Some(StrategyId::from("S-001")));
        assert_eq!(result, 0);
    }

    #[rstest]
    fn set_count_with_valid_strategy() {
        let mut generator = get_position_id_generator();
        generator.set_count(7, Some(StrategyId::from("S-001")));
        let result = generator.count(Some(StrategyId::from("S-001")));
        assert_eq!(result, 7);
    }

    #[rstest]
    fn test_reset() {
        let mut generator = get_position_id_generator();
        generator.generate(Some(StrategyId::from("S-001")), None);
        generator.generate(Some(StrategyId::from("S-001")), None);
        generator.reset();
        let result = generator.generate(Some(StrategyId::from("S-001")), None);
        assert_eq!(result, PositionId::from("P-19700101-0000-001-001-1"));
    }
}

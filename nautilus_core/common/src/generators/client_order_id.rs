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
    client_order_id::ClientOrderId, strategy_id::StrategyId, trader_id::TraderId,
};

use super::get_datetime_tag;
use crate::clock::Clock;

#[repr(C)]
pub struct ClientOrderIdGenerator<'a> {
    trader_id: TraderId,
    strategy_id: StrategyId,
    clock: &'a mut Box<dyn Clock>,
    count: usize,
}

impl<'a> ClientOrderIdGenerator<'a> {
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        clock: &'a mut Box<dyn Clock>,
        initial_count: usize,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            clock,
            count: initial_count,
        }
    }

    pub fn set_count(&mut self, count: usize) {
        self.count = count;
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn generate(&mut self) -> ClientOrderId {
        let datetime_tag = get_datetime_tag(self.clock.timestamp_ms());
        let trader_tag = self.trader_id.get_tag();
        let strategy_tag = self.strategy_id.get_tag();
        self.count += 1;
        let id = format!(
            "O-{}-{}-{}-{}",
            datetime_tag, trader_tag, strategy_tag, self.count
        );
        ClientOrderId::from(id.as_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::{
        client_order_id::ClientOrderId, strategy_id::StrategyId, trader_id::TraderId,
    };
    use rstest::rstest;

    use crate::{
        clock::{stubs::test_clock, Clock, TestClock},
        generators::client_order_id::ClientOrderIdGenerator,
    };

    fn get_client_order_id_generator(
        clock: &mut Box<dyn Clock>,
        initial_count: Option<usize>,
    ) -> ClientOrderIdGenerator {
        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("EMACross-001");
        ClientOrderIdGenerator::new(trader_id, strategy_id, clock, initial_count.unwrap_or(0))
    }

    #[rstest]
    fn test_init(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let generator = get_client_order_id_generator(&mut test_clock, None);
        assert_eq!(generator.count(), 0);
    }

    #[rstest]
    fn test_init_with_initial_count(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let generator = get_client_order_id_generator(&mut test_clock, Some(7));
        assert_eq!(generator.count(), 7);
    }

    #[rstest]
    fn test_generate_client_order_id_from_start(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_client_order_id_generator(&mut test_clock, None);
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();

        assert_eq!(
            result1,
            ClientOrderId::new("O-19700101-0000-001-001-1").unwrap()
        );
        assert_eq!(
            result2,
            ClientOrderId::new("O-19700101-0000-001-001-2").unwrap()
        );
        assert_eq!(
            result3,
            ClientOrderId::new("O-19700101-0000-001-001-3").unwrap()
        );
    }

    #[rstest]
    fn test_generate_client_order_id_from_initial(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_client_order_id_generator(&mut test_clock, Some(5));
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();

        assert_eq!(
            result1,
            ClientOrderId::new("O-19700101-0000-001-001-6").unwrap()
        );
        assert_eq!(
            result2,
            ClientOrderId::new("O-19700101-0000-001-001-7").unwrap()
        );
        assert_eq!(
            result3,
            ClientOrderId::new("O-19700101-0000-001-001-8").unwrap()
        );
    }

    #[rstest]
    fn test_reset(test_clock: TestClock) {
        let mut test_clock: Box<dyn Clock> = Box::new(test_clock);
        let mut generator = get_client_order_id_generator(&mut test_clock, None);
        generator.generate();
        generator.generate();
        generator.reset();
        let result = generator.generate();

        assert_eq!(
            result,
            ClientOrderId::new("O-19700101-0000-001-001-1").unwrap()
        );
    }
}

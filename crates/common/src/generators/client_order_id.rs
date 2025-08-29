// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{AtomicTime, uuid::UUID4};
use nautilus_model::identifiers::{ClientOrderId, StrategyId, TraderId};

use super::get_datetime_tag;

#[repr(C)]
#[derive(Debug)]
pub struct ClientOrderIdGenerator {
    clock: &'static AtomicTime,
    trader_id: TraderId,
    strategy_id: StrategyId,
    count: usize,
    use_uuids: bool,
    use_hyphens: bool,
}

impl ClientOrderIdGenerator {
    /// Creates a new [`ClientOrderIdGenerator`] instance.
    #[must_use]
    pub const fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        initial_count: usize,
        clock: &'static AtomicTime,
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
            let datetime_tag = get_datetime_tag(self.clock.get_time_ms());
            let trader_tag = self.trader_id.get_tag();
            let strategy_tag = self.strategy_id.get_tag();
            self.count += 1;

            if !self.use_hyphens {
                let datetime_no_hyphens = datetime_tag.replace('-', "");
                format!(
                    "O{}{}{}{}",
                    datetime_no_hyphens, trader_tag, strategy_tag, self.count
                )
            } else {
                format!(
                    "O-{}-{}-{}-{}",
                    datetime_tag, trader_tag, strategy_tag, self.count
                )
            }
        };

        ClientOrderId::from(value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::time::get_atomic_clock_static;
    use nautilus_model::identifiers::{ClientOrderId, StrategyId, TraderId};
    use rstest::rstest;

    use crate::generators::client_order_id::ClientOrderIdGenerator;

    fn get_client_order_id_generator(
        initial_count: Option<usize>,
        use_uuids: bool,
        use_hyphens: bool,
    ) -> ClientOrderIdGenerator {
        ClientOrderIdGenerator::new(
            TraderId::default(),
            StrategyId::default(),
            initial_count.unwrap_or(0),
            get_atomic_clock_static(),
            use_uuids,
            use_hyphens,
        )
    }

    #[rstest]
    fn test_init() {
        let generator = get_client_order_id_generator(None, false, true);
        assert_eq!(generator.count(), 0);
    }

    #[rstest]
    fn test_init_with_initial_count() {
        let generator = get_client_order_id_generator(Some(7), false, true);
        assert_eq!(generator.count(), 7);
    }

    #[rstest]
    fn test_generate_client_order_id_from_start() {
        let mut generator = get_client_order_id_generator(None, false, true);
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();

        assert_eq!(result1, ClientOrderId::new("O-19700101-000000-001-001-1"));
        assert_eq!(result2, ClientOrderId::new("O-19700101-000000-001-001-2"));
        assert_eq!(result3, ClientOrderId::new("O-19700101-000000-001-001-3"));
    }

    #[rstest]
    fn test_generate_client_order_id_from_initial() {
        let mut generator = get_client_order_id_generator(Some(5), false, true);
        let result1 = generator.generate();
        let result2 = generator.generate();
        let result3 = generator.generate();

        assert_eq!(result1, ClientOrderId::new("O-19700101-000000-001-001-6"));
        assert_eq!(result2, ClientOrderId::new("O-19700101-000000-001-001-7"));
        assert_eq!(result3, ClientOrderId::new("O-19700101-000000-001-001-8"));
    }

    #[rstest]
    fn test_generate_client_order_id_with_hyphens_removed() {
        let mut generator = get_client_order_id_generator(None, false, false);
        let result = generator.generate();

        assert_eq!(result, ClientOrderId::new("O197001010000000010011"));
    }

    #[rstest]
    fn test_generate_uuid_client_order_id() {
        let mut generator = get_client_order_id_generator(None, true, true);
        let result = generator.generate();

        // UUID should be 36 characters with hyphens
        assert_eq!(result.as_str().len(), 36);
        assert!(result.as_str().contains('-'));
    }

    #[rstest]
    fn test_generate_uuid_client_order_id_with_hyphens_removed() {
        let mut generator = get_client_order_id_generator(None, true, false);
        let result = generator.generate();

        // UUID without hyphens should be 32 characters
        assert_eq!(result.as_str().len(), 32);
        assert!(!result.as_str().contains('-'));
    }

    #[rstest]
    fn test_reset() {
        let mut generator = get_client_order_id_generator(None, false, true);
        generator.generate();
        generator.generate();
        generator.reset();
        let result = generator.generate();

        assert_eq!(result, ClientOrderId::new("O-19700101-000000-001-001-1"));
    }
}

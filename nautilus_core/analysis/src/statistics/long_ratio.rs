// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::{enums::OrderSide, position::Position};

use crate::statistic::PortfolioStatistic;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct LongRatio {
    precision: usize,
}

impl LongRatio {
    /// Creates a new [`LongRatio`] instance.
    #[must_use]
    pub fn new(precision: Option<usize>) -> Self {
        Self {
            precision: precision.unwrap_or(2),
        }
    }
}

impl PortfolioStatistic for LongRatio {
    type Item = String;

    fn name(&self) -> String {
        stringify!(LongRatio).to_string()
    }

    fn calculate_from_positions(&self, positions: &[Position]) -> Option<Self::Item> {
        if positions.is_empty() {
            return None;
        }

        let longs: Vec<&Position> = positions
            .iter()
            .filter(|p| matches!(p.entry, OrderSide::Buy))
            .collect();

        let value = longs.len() as f64 / positions.len() as f64;
        Some(format!("{:.1$}", value, self.precision))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nautilus_core::nanos::UnixNanos;
    use nautilus_model::{
        enums::OrderSide,
        identifiers::{
            stubs::{instrument_id_aud_usd_sim, strategy_id_ema_cross, trader_id},
            AccountId, ClientOrderId, PositionId,
        },
        types::{currency::Currency, quantity::Quantity},
    };

    use super::*;

    fn create_test_position(side: OrderSide) -> Position {
        Position {
            events: Vec::new(),
            trader_id: trader_id(),
            strategy_id: strategy_id_ema_cross(),
            instrument_id: instrument_id_aud_usd_sim(),
            id: PositionId::new("test-position"),
            account_id: AccountId::new("test-account"),
            opening_order_id: ClientOrderId::default(),
            closing_order_id: None,
            entry: side,
            side: nautilus_model::enums::PositionSide::NoPositionSide,
            signed_qty: 0.0,
            quantity: Quantity::default(),
            peak_qty: Quantity::default(),
            price_precision: 2,
            size_precision: 2,
            multiplier: Quantity::default(),
            is_inverse: false,
            base_currency: None,
            quote_currency: Currency::USD(),
            settlement_currency: Currency::USD(),
            ts_init: UnixNanos::default(),
            ts_opened: UnixNanos::default(),
            ts_last: UnixNanos::default(),
            ts_closed: None,
            duration_ns: 2,
            avg_px_open: 0.0,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            trade_ids: Vec::new(),
            buy_qty: Quantity::default(),
            sell_qty: Quantity::default(),
            commissions: HashMap::new(),
        }
    }

    #[test]
    fn test_empty_positions() {
        let long_ratio = LongRatio::new(None);
        let result = long_ratio.calculate_from_positions(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_all_long_positions() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Buy),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "1.00");
    }

    #[test]
    fn test_all_short_positions() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![
            create_test_position(OrderSide::Sell),
            create_test_position(OrderSide::Sell),
            create_test_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "0.00");
    }

    #[test]
    fn test_mixed_positions() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Sell),
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "0.50");
    }

    #[test]
    fn test_custom_precision() {
        let long_ratio = LongRatio::new(Some(3));
        let positions = vec![
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "0.667");
    }

    #[test]
    fn test_single_position_long() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![create_test_position(OrderSide::Buy)];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "1.00");
    }

    #[test]
    fn test_single_position_short() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![create_test_position(OrderSide::Sell)];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "0.00");
    }

    #[test]
    fn test_zero_precision() {
        let long_ratio = LongRatio::new(Some(0));
        let positions = vec![
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Buy),
            create_test_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "1");
    }

    #[test]
    fn test_name() {
        let long_ratio = LongRatio::new(None);
        assert_eq!(long_ratio.name(), "LongRatio");
    }
}

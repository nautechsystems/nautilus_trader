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

use std::fmt::Display;

use nautilus_model::{enums::OrderSide, position::Position};

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the ratio of long positions to total positions.
///
/// A position counts as long when its entry (opening order) side is `Buy`.
/// The result is in `[0, 1]`, rounded to `precision` decimal places, and is
/// `None` for an empty position list.
#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.analysis", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.analysis")
)]
pub struct LongRatio {
    /// The number of decimal places to round the ratio to (default: 2).
    pub precision: usize,
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

impl Display for LongRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Long Ratio")
    }
}

impl PortfolioStatistic for LongRatio {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_positions(&self, positions: &[Position]) -> Option<Self::Item> {
        if positions.is_empty() {
            return None;
        }

        // Use `entry` (the opening order side) rather than `side` because
        // closed positions have side == PositionSide::Flat
        let long_count = positions
            .iter()
            .filter(|p| p.entry == OrderSide::Buy)
            .count();

        let value = long_count as f64 / positions.len() as f64;

        let scale = 10f64.powi(self.precision as i32);
        Some((value * scale).round() / scale)
    }
    fn calculate_from_returns(&self, _returns: &Returns) -> Option<Self::Item> {
        None
    }

    fn calculate_from_realized_pnls(&self, _realized_pnls: &[f64]) -> Option<Self::Item> {
        None
    }
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use indexmap::IndexMap;
    use nautilus_core::{DurationNanos, UnixNanos, approx_eq};
    use nautilus_model::{
        enums::{InstrumentClass, OrderSide, PositionSide},
        identifiers::{
            AccountId, ClientOrderId, PositionId,
            stubs::{instrument_id_aud_usd_sim, strategy_id_ema_cross, trader_id},
        },
        instruments::stubs::audusd_sim,
        stubs::{TestDefault, stub_position_long},
        types::{Currency, Quantity},
    };
    use rstest::rstest;

    use super::*;

    /// Creates a closed position with the given entry side.
    /// Closed positions have side == Flat, so we test with `entry` field.
    fn create_closed_position(entry: OrderSide) -> Position {
        let mut position = stub_position_long(audusd_sim());
        position.events.clear();
        position.adjustments.clear();
        position.replay_events.clear();
        position.fill_voids.clear();
        position.trader_id = trader_id();
        position.strategy_id = strategy_id_ema_cross();
        position.instrument_id = instrument_id_aud_usd_sim();
        position.id = PositionId::new("test-position");
        position.account_id = AccountId::new("test-account");
        position.opening_order_id = ClientOrderId::test_default();
        position.closing_order_id = None;
        position.entry = entry;
        position.side = PositionSide::Flat;
        position.signed_qty = 0.0;
        position.quantity = Quantity::default();
        position.peak_qty = Quantity::default();
        position.price_precision = 2;
        position.size_precision = 2;
        position.multiplier = Quantity::default();
        position.is_inverse = false;
        position.base_currency = None;
        position.quote_currency = Currency::USD();
        position.settlement_currency = Currency::USD();
        position.ts_init = UnixNanos::default();
        position.ts_opened = UnixNanos::default();
        position.ts_last = UnixNanos::default();
        position.ts_closed = Some(UnixNanos::from(1));
        position.duration_ns = DurationNanos::new(2);
        position.avg_px_open = 0.0;
        position.avg_px_close = Some(0.0);
        position.realized_return = 0.0;
        position.realized_pnl = None;
        position.trade_ids = AHashSet::new();
        position.buy_qty = Quantity::default();
        position.sell_qty = Quantity::default();
        position.commissions = IndexMap::new();
        position.instrument_class = InstrumentClass::Spot;
        position.is_currency_pair = true;
        position.rebuild_replay_index();
        position
    }

    #[rstest]
    fn test_empty_positions() {
        let long_ratio = LongRatio::new(None);
        let result = long_ratio.calculate_from_positions(&[]);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_all_long_positions() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Buy),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 1.00, epsilon = 1e-9));
    }

    #[rstest]
    fn test_all_short_positions() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![
            create_closed_position(OrderSide::Sell),
            create_closed_position(OrderSide::Sell),
            create_closed_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 0.00, epsilon = 1e-9));
    }

    #[rstest]
    fn test_mixed_positions() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Sell),
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 0.50, epsilon = 1e-9));
    }

    #[rstest]
    fn test_custom_precision() {
        let long_ratio = LongRatio::new(Some(3));
        let positions = vec![
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 0.667, epsilon = 1e-9));
    }

    #[rstest]
    fn test_single_position_long() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![create_closed_position(OrderSide::Buy)];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 1.00, epsilon = 1e-9));
    }

    #[rstest]
    fn test_single_position_short() {
        let long_ratio = LongRatio::new(None);
        let positions = vec![create_closed_position(OrderSide::Sell)];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 0.00, epsilon = 1e-9));
    }

    #[rstest]
    fn test_zero_precision() {
        let long_ratio = LongRatio::new(Some(0));
        let positions = vec![
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Buy),
            create_closed_position(OrderSide::Sell),
        ];

        let result = long_ratio.calculate_from_positions(&positions);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 1.00, epsilon = 1e-9));
    }

    #[rstest]
    fn test_name() {
        let long_ratio = LongRatio::new(None);
        assert_eq!(long_ratio.name(), "Long Ratio");
    }
}

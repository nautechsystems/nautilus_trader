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

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::PositionAdjustmentType,
    identifiers::{AccountId, InstrumentId, PositionId, StrategyId, TraderId},
    types::Money,
};

/// Represents an adjustment to a position's quantity or realized PnL.
///
/// This event is used to track changes to positions that occur outside of normal
/// order fills, such as:
/// - Commission adjustments that affect the actual quantity held (e.g., crypto spot commissions)
/// - Funding payments that affect realized PnL (e.g., perpetual futures funding)
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PositionAdjusted {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The position ID associated with the event.
    pub position_id: PositionId,
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The type of adjustment.
    pub adjustment_type: PositionAdjustmentType,
    /// The quantity change (if applicable). Positive increases quantity, negative decreases.
    pub quantity_change: Option<Decimal>,
    /// The PnL change (if applicable). Can be positive or negative.
    pub pnl_change: Option<Money>,
    /// Optional reason or reference for the adjustment (e.g., order ID, funding period).
    pub reason: Option<Ustr>,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl PositionAdjusted {
    /// Creates a new [`PositionAdjusted`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        position_id: PositionId,
        account_id: AccountId,
        adjustment_type: PositionAdjustmentType,
        quantity_change: Option<Decimal>,
        pnl_change: Option<Money>,
        reason: Option<Ustr>,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            adjustment_type,
            quantity_change,
            pnl_change,
            reason,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::UnixNanos;
    use rstest::*;

    use super::*;
    use crate::{
        enums::PositionAdjustmentType,
        identifiers::{AccountId, InstrumentId, PositionId, StrategyId, TraderId},
        types::{Currency, Money},
    };

    fn create_test_commission_adjustment() -> PositionAdjusted {
        PositionAdjusted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            PositionId::from("P-001"),
            AccountId::from("BINANCE-001"),
            PositionAdjustmentType::Commission,
            Some(Decimal::from_str("-0.001").unwrap()),
            None,
            Some(Ustr::from("O-123")),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    fn create_test_funding_adjustment() -> PositionAdjusted {
        PositionAdjusted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("BTCUSD-PERP.BINANCE"),
            PositionId::from("P-002"),
            AccountId::from("BINANCE-001"),
            PositionAdjustmentType::Funding,
            None,
            Some(Money::new(-5.50, Currency::USD())),
            Some(Ustr::from("funding_2024_01_15_08:00")),
            Default::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    #[rstest]
    fn test_position_adjustment_commission_new() {
        let adjustment = create_test_commission_adjustment();

        assert_eq!(adjustment.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(adjustment.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            adjustment.instrument_id,
            InstrumentId::from("BTCUSDT.BINANCE")
        );
        assert_eq!(adjustment.position_id, PositionId::from("P-001"));
        assert_eq!(adjustment.account_id, AccountId::from("BINANCE-001"));
        assert_eq!(
            adjustment.adjustment_type,
            PositionAdjustmentType::Commission
        );
        assert_eq!(
            adjustment.quantity_change,
            Some(Decimal::from_str("-0.001").unwrap())
        );
        assert_eq!(adjustment.pnl_change, None);
        assert_eq!(adjustment.reason, Some(Ustr::from("O-123")));
        assert_eq!(adjustment.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(adjustment.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_position_adjustment_funding_new() {
        let adjustment = create_test_funding_adjustment();

        assert_eq!(adjustment.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(adjustment.strategy_id, StrategyId::from("EMA-CROSS"));
        assert_eq!(
            adjustment.instrument_id,
            InstrumentId::from("BTCUSD-PERP.BINANCE")
        );
        assert_eq!(adjustment.position_id, PositionId::from("P-002"));
        assert_eq!(adjustment.account_id, AccountId::from("BINANCE-001"));
        assert_eq!(adjustment.adjustment_type, PositionAdjustmentType::Funding);
        assert_eq!(adjustment.quantity_change, None);
        assert_eq!(
            adjustment.pnl_change,
            Some(Money::new(-5.50, Currency::USD()))
        );
        assert_eq!(
            adjustment.reason,
            Some(Ustr::from("funding_2024_01_15_08:00"))
        );
        assert_eq!(adjustment.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(adjustment.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_position_adjustment_clone() {
        let adjustment1 = create_test_commission_adjustment();
        let adjustment2 = adjustment1;

        assert_eq!(adjustment1, adjustment2);
    }

    #[rstest]
    fn test_position_adjustment_debug() {
        let adjustment = create_test_commission_adjustment();
        let debug_str = format!("{adjustment:?}");

        assert!(debug_str.contains("PositionAdjusted"));
        assert!(debug_str.contains("TRADER-001"));
        assert!(debug_str.contains("EMA-CROSS"));
        assert!(debug_str.contains("BTCUSDT.BINANCE"));
        assert!(debug_str.contains("P-001"));
        assert!(debug_str.contains("Commission"));
    }

    #[rstest]
    fn test_position_adjustment_partial_eq() {
        let adjustment1 = create_test_commission_adjustment();
        let mut adjustment2 = create_test_commission_adjustment();
        adjustment2.event_id = adjustment1.event_id;

        let mut adjustment3 = create_test_commission_adjustment();
        adjustment3.event_id = adjustment1.event_id;
        adjustment3.quantity_change = Some(Decimal::from_str("-0.002").unwrap());

        assert_eq!(adjustment1, adjustment2);
        assert_ne!(adjustment1, adjustment3);
    }

    #[rstest]
    fn test_position_adjustment_different_types() {
        let commission = create_test_commission_adjustment();
        let funding = create_test_funding_adjustment();

        assert_eq!(
            commission.adjustment_type,
            PositionAdjustmentType::Commission
        );
        assert_eq!(funding.adjustment_type, PositionAdjustmentType::Funding);
        assert_ne!(commission.adjustment_type, funding.adjustment_type);
    }

    #[rstest]
    fn test_position_adjustment_timestamps() {
        let adjustment = create_test_commission_adjustment();

        assert_eq!(adjustment.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(adjustment.ts_init, UnixNanos::from(2_000_000_000));
        assert!(adjustment.ts_event < adjustment.ts_init);
    }

    #[rstest]
    fn test_position_adjustment_serialization() {
        let original = create_test_commission_adjustment();

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PositionAdjusted = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }
}

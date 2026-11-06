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

//! Non-order position operation legs and replay entries.

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::{AccountId, InstrumentId, PositionId, StrategyId, TraderId},
    types::Quantity,
};

/// The managed operation which changed a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionOperationType {
    /// Splits collateral into a complete set of outcome tokens.
    Split,
    /// Merges a complete set of outcome tokens into collateral.
    Merge,
    /// Redeems resolved outcome tokens.
    Redeem,
    /// Converts selected negative-risk NO positions.
    Convert,
}

impl PositionOperationType {
    /// Returns the lowercase operation name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Merge => "merge",
            Self::Redeem => "redeem",
            Self::Convert => "convert",
        }
    }
}

impl std::fmt::Display for PositionOperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether an operation leg increased or decreased position quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionOperationDirection {
    /// The operation increased position quantity.
    Open,
    /// The operation decreased position quantity.
    Close,
}

impl PositionOperationDirection {
    /// Returns the lowercase direction name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Close => "close",
        }
    }
}

impl std::fmt::Display for PositionOperationDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One position-level leg of a managed position operation.
///
/// A leg records the exact quantity and cost basis transferred by an operation. It is used as a
/// replay entry, so applying the same leg twice must not change position state.
///
/// The `cost` is an exact cost-basis amount: the cost added for an [`PositionOperationDirection::Open`]
/// leg, or the cost released for a [`PositionOperationDirection::Close`] leg. `income` holds the
/// settlement-currency proceeds allocated to a close leg; it is `None` for open legs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PositionOperationLeg {
    /// The stable operation identity. This is the managed command UUID.
    pub operation_id: UUID4,
    /// The operation which produced this leg.
    pub operation: PositionOperationType,
    /// Whether this leg opened or closed position quantity.
    pub direction: PositionOperationDirection,
    /// The trader which owns the position leg.
    pub trader_id: TraderId,
    /// The strategy which owns the position leg.
    pub strategy_id: StrategyId,
    /// The account which owns the position leg.
    pub account_id: AccountId,
    /// The instrument associated with the leg.
    pub instrument_id: InstrumentId,
    /// The position associated with the leg.
    pub position_id: PositionId,
    /// The operated quantity.
    pub quantity: Quantity,
    /// The exact cost basis added or released by the leg.
    pub cost: Decimal,
    /// The exact settlement-currency proceeds allocated to a close leg.
    pub income: Option<Decimal>,
    /// Whether this leg records physical payout of an already economically settled position.
    ///
    /// Settled legs are retained in operation records but are not inserted into position replay
    /// and do not change position quantity, cost, or realized PnL.
    #[serde(default)]
    pub settled: bool,
    /// The unique identifier for the leg event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the operation occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the leg event was initialized.
    pub ts_init: UnixNanos,
}

impl PositionOperationLeg {
    /// Returns the stable identity of the leg without its event timestamp.
    #[must_use]
    pub const fn identity(&self) -> (UUID4, PositionOperationDirection, InstrumentId, PositionId) {
        (
            self.operation_id,
            self.direction,
            self.instrument_id,
            self.position_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::identifiers::PositionId;

    #[rstest]
    fn test_operation_type_and_direction_display() {
        assert_eq!(PositionOperationType::Split.to_string(), "split");
        assert_eq!(PositionOperationType::Merge.to_string(), "merge");
        assert_eq!(PositionOperationType::Redeem.to_string(), "redeem");
        assert_eq!(PositionOperationType::Convert.to_string(), "convert");
        assert_eq!(PositionOperationDirection::Open.to_string(), "open");
        assert_eq!(PositionOperationDirection::Close.to_string(), "close");
    }

    #[rstest]
    fn test_leg_identity_is_stable() {
        let leg = PositionOperationLeg {
            operation_id: UUID4::new(),
            operation: PositionOperationType::Convert,
            direction: PositionOperationDirection::Open,
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            account_id: AccountId::from("ACCOUNT-001"),
            instrument_id: InstrumentId::from("TOKEN.SIM"),
            position_id: PositionId::from("P-001"),
            quantity: Quantity::from("100"),
            cost: dec!(50),
            income: None,
            settled: false,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(2),
        };

        assert_eq!(leg.identity().0, leg.operation_id);
        assert_eq!(leg.identity().1, PositionOperationDirection::Open);
        assert_eq!(leg.identity().2, leg.instrument_id);
        assert_eq!(leg.identity().3, leg.position_id);

        let serialized = serde_json::to_string(&leg).unwrap();
        let restored: PositionOperationLeg = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored, leg);
    }
}

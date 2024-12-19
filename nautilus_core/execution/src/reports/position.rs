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

use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    enums::PositionSide,
    identifiers::{AccountId, InstrumentId, PositionId},
    types::Quantity,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.execution")
)]
pub struct PositionStatusReport {
    pub account_id: AccountId,
    pub instrument_id: InstrumentId,
    pub position_side: PositionSide,
    pub quantity: Quantity,
    pub signed_decimal_qty: Decimal,
    pub report_id: UUID4,
    pub ts_last: UnixNanos,
    pub ts_init: UnixNanos,
    pub venue_position_id: Option<PositionId>,
}

impl PositionStatusReport {
    /// Creates a new [`PositionStatusReport`] instance with required fields.
    #[must_use]
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        position_side: PositionSide,
        quantity: Quantity,
        venue_position_id: Option<PositionId>,
        ts_last: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        // Calculate signed decimal quantity based on position side
        let signed_decimal_qty = match position_side {
            PositionSide::Long => quantity.as_decimal(),
            PositionSide::Short => -quantity.as_decimal(),
            PositionSide::Flat => Decimal::ZERO,
            PositionSide::NoPositionSide => Decimal::ZERO, // TODO: Consider disallowing this?
        };

        Self {
            account_id,
            instrument_id,
            position_side,
            quantity,
            signed_decimal_qty,
            report_id: UUID4::new(),
            ts_last,
            ts_init,
            venue_position_id,
        }
    }

    /// Checks if the position has a venue position ID.
    #[must_use]
    pub const fn has_venue_position_id(&self) -> bool {
        self.venue_position_id.is_some()
    }

    /// Checks if this is a flat position (quantity is zero).
    #[must_use]
    pub const fn is_flat(&self) -> bool {
        matches!(
            self.position_side,
            PositionSide::Flat | PositionSide::NoPositionSide
        )
    }

    /// Checks if this is a long position.
    #[must_use]
    pub fn is_long(&self) -> bool {
        self.position_side == PositionSide::Long
    }

    /// Checks if this is a short position.
    #[must_use]
    pub fn is_short(&self) -> bool {
        self.position_side == PositionSide::Short
    }
}

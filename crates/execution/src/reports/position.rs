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

use std::fmt::{Debug, Display};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::PositionSide,
    identifiers::{AccountId, InstrumentId, PositionId},
    types::Quantity,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Represents a position status at a point in time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.execution")
)]
pub struct PositionStatusReport {
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The position side.
    pub position_side: PositionSide,
    /// The current open quantity.
    pub quantity: Quantity,
    /// The current signed quantity as a decimal (positive for position side `LONG`, negative for `SHORT`).
    pub signed_decimal_qty: Decimal,
    /// The unique identifier for the event.
    pub report_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the last event occurred.
    pub ts_last: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// The position ID (assigned by the venue).
    pub venue_position_id: Option<PositionId>,
}

impl PositionStatusReport {
    /// Creates a new [`PositionStatusReport`] instance with required fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        position_side: PositionSide,
        quantity: Quantity,
        venue_position_id: Option<PositionId>,
        ts_last: UnixNanos,
        ts_init: UnixNanos,
        report_id: Option<UUID4>,
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
            report_id: report_id.unwrap_or_default(),
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

impl Display for PositionStatusReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PositionStatusReport(account={}, instrument={}, side={}, qty={}, venue_pos_id={:?}, ts_last={}, ts_init={})",
            self.account_id,
            self.instrument_id,
            self.position_side,
            self.signed_decimal_qty,
            self.venue_position_id,
            self.ts_last,
            self.ts_init
        )
    }
}

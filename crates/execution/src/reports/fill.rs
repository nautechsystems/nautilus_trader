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

use std::fmt::Display;

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    types::{Money, Price, Quantity},
};
use serde::{Deserialize, Serialize};

/// Represents a fill report of a single order execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.execution")
)]
pub struct FillReport {
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The venue assigned order ID.
    pub venue_order_id: VenueOrderId,
    /// The trade match ID (assigned by the venue).
    pub trade_id: TradeId,
    /// The order side.
    pub order_side: OrderSide,
    /// The last fill quantity for the position.
    pub last_qty: Quantity,
    /// The last fill price for the position.
    pub last_px: Price,
    /// The commission generated from the fill.
    pub commission: Money,
    /// The liquidity side of the execution.
    pub liquidity_side: LiquiditySide,
    /// The unique identifier for the event.
    pub report_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// The client order ID.
    pub client_order_id: Option<ClientOrderId>,
    /// The position ID (assigned by the venue).
    pub venue_position_id: Option<PositionId>,
}

impl FillReport {
    /// Creates a new [`FillReport`] instance with required fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        order_side: OrderSide,
        last_qty: Quantity,
        last_px: Price,
        commission: Money,
        liquidity_side: LiquiditySide,
        client_order_id: Option<ClientOrderId>,
        venue_position_id: Option<PositionId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        report_id: Option<UUID4>,
    ) -> Self {
        Self {
            account_id,
            instrument_id,
            venue_order_id,
            trade_id,
            order_side,
            last_qty,
            last_px,
            commission,
            liquidity_side,
            report_id: report_id.unwrap_or_default(),
            ts_event,
            ts_init,
            client_order_id,
            venue_position_id,
        }
    }

    /// Checks if the fill has a client order ID.
    #[must_use]
    pub const fn has_client_order_id(&self) -> bool {
        self.client_order_id.is_some()
    }

    /// Utility method to check if the fill has a venue position ID.
    #[must_use]
    pub const fn has_venue_position_id(&self) -> bool {
        self.venue_position_id.is_some()
    }
}

impl Display for FillReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FillReport(instrument={}, side={}, qty={}, last_px={}, trade_id={}, venue_order_id={}, commission={}, liquidity={})",
            self.instrument_id,
            self.order_side,
            self.last_qty,
            self.last_px,
            self.trade_id,
            self.venue_order_id,
            self.commission,
            self.liquidity_side,
        )
    }
}

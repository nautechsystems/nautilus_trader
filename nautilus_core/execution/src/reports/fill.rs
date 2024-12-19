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
    enums::{LiquiditySide, OrderSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, OrderListId, TradeId, VenueOrderId},
    types::{Money, Price, Quantity},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.execution")
)]
pub struct FillReport {
    pub account_id: AccountId,
    pub instrument_id: InstrumentId,
    pub venue_order_id: VenueOrderId,
    pub trade_id: TradeId,
    pub order_side: OrderSide,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub commission: Money,
    pub liquidity_side: LiquiditySide,
    pub report_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub client_order_id: Option<ClientOrderId>,
    pub venue_position_id: Option<OrderListId>,
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
        venue_position_id: Option<OrderListId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
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
            report_id: UUID4::new(),
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

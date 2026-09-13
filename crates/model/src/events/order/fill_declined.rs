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

use nautilus_core::{UUID4, UnixNanos};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::{FillDeclinedReason, OrderSide},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    types::{Price, Quantity},
};

/// Represents an order fill the execution engine declined to apply.
///
/// The fill was received from the venue but was applied to neither the order nor a position,
/// so local state has diverged from the venue's. This is not an order lifecycle event and
/// changes no order state; it is published on the `events.order_fill_declined.{strategy_id}`
/// topic so a Rust subscriber can fail closed.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
pub struct OrderFillDeclined {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub trade_id: TradeId,
    pub position_id: Option<PositionId>,
    pub order_side: OrderSide,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub reason: FillDeclinedReason,
    pub details: Ustr,
    pub event_id: UUID4,
    pub causation_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl Display for OrderFillDeclined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(client_order_id={}, trade_id={}, reason={}, details={})",
            stringify!(OrderFillDeclined),
            self.client_order_id,
            self.trade_id,
            self.reason,
            self.details,
        )
    }
}

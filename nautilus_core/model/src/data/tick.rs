// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::enums::OrderSide;
use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::trade_id::TradeId;
use crate::primitives::price::Price;
use crate::primitives::quantity::Quantity;
use nautilus_core::time::Timestamp;

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct QuoteTick {
    pub instrument_id: InstrumentId,
    pub bid: Price,
    pub ask: Price,
    pub bid_size: Quantity,
    pub ask_size: Quantity,
    pub ts_event: Timestamp,
    pub ts_init: Timestamp,
}

/// Represents a single trade tick in a financial market.
#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct TradeTick {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub size: Quantity,
    pub side: OrderSide,
    pub trade_id: TradeId,
    pub ts_event: Timestamp,
    pub ts_init: Timestamp,
}

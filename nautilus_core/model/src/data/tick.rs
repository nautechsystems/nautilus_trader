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
use crate::types::price::Price;
use crate::types::quantity::Quantity;
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
    pub aggressor_side: OrderSide,
    pub trade_id: TradeId,
    pub ts_event: Timestamp,
    pub ts_init: Timestamp,
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn quote_tick_free(tick: QuoteTick) {
    drop(tick); // Memory freed here
}

#[no_mangle]
pub extern "C" fn quote_tick_new(
    instrument_id: InstrumentId,
    bid: Price,
    ask: Price,
    bid_size: Quantity,
    ask_size: Quantity,
    ts_event: i64,
    ts_init: i64,
) -> QuoteTick {
    QuoteTick {
        instrument_id,
        bid,
        ask,
        bid_size,
        ask_size,
        ts_event: Timestamp { value: ts_event },
        ts_init: Timestamp { value: ts_init },
    }
}

#[no_mangle]
pub extern "C" fn quote_tick_from_raw(
    instrument_id: InstrumentId,
    bid: i64,
    ask: i64,
    price_prec: u8,
    bid_size: u64,
    ask_size: u64,
    size_prec: u8,
    ts_event: i64,
    ts_init: i64,
) -> QuoteTick {
    QuoteTick {
        instrument_id,
        bid: Price::from_raw(bid, price_prec),
        ask: Price::from_raw(ask, price_prec),
        bid_size: Quantity::from_raw(bid_size, size_prec),
        ask_size: Quantity::from_raw(ask_size, size_prec),
        ts_event: Timestamp { value: ts_event },
        ts_init: Timestamp { value: ts_init },
    }
}

#[no_mangle]
pub extern "C" fn trade_tick_free(tick: TradeTick) {
    drop(tick); // Memory freed here
}

#[no_mangle]
pub extern "C" fn trade_tick_from_raw(
    instrument_id: InstrumentId,
    price: i64,
    price_prec: u8,
    size: u64,
    size_prec: u8,
    aggressor_side: OrderSide,
    trade_id: TradeId,
    ts_event: i64,
    ts_init: i64,
) -> TradeTick {
    TradeTick {
        instrument_id,
        price: Price::from_raw(price, price_prec),
        size: Quantity::from_raw(size, size_prec),
        aggressor_side,
        trade_id,
        ts_event: Timestamp { value: ts_event },
        ts_init: Timestamp { value: ts_init },
    }
}

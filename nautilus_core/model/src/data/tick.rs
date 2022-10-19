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

use std::fmt::{Display, Formatter, Result};

use pyo3::ffi;

use crate::enums::OrderSide;
use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::trade_id::TradeId;
use crate::types::price::Price;
use crate::types::quantity::Quantity;
use nautilus_core::correctness;
use nautilus_core::string::string_to_pystr;
use nautilus_core::time::Timestamp;

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QuoteTick {
    pub instrument_id: InstrumentId,
    pub bid: Price,
    pub ask: Price,
    pub bid_size: Quantity,
    pub ask_size: Quantity,
    pub ts_event: Timestamp,
    pub ts_init: Timestamp,
}

impl QuoteTick {
    pub fn new(
        instrument_id: InstrumentId,
        bid: Price,
        ask: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: Timestamp,
        ts_init: Timestamp,
    ) -> QuoteTick {
        correctness::u8_equal(
            bid.precision,
            ask.precision,
            "bid.precision",
            "ask.precision",
        );
        correctness::u8_equal(
            bid_size.precision,
            ask_size.precision,
            "bid_size.precision",
            "ask_size.precision",
        );
        QuoteTick {
            instrument_id,
            bid,
            ask,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        }
    }
}

impl Display for QuoteTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id, self.bid, self.ask, self.bid_size, self.ask_size, self.ts_event,
        )
    }
}

/// Represents a single trade tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TradeTick {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub size: Quantity,
    pub aggressor_side: OrderSide,
    pub trade_id: TradeId,
    pub ts_event: Timestamp,
    pub ts_init: Timestamp,
}

impl TradeTick {
    pub fn new(
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: OrderSide,
        trade_id: TradeId,
        ts_event: Timestamp,
        ts_init: Timestamp,
    ) -> TradeTick {
        TradeTick {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        }
    }
}

impl Display for TradeTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id,
            self.price,
            self.size,
            self.aggressor_side,
            self.trade_id,
            self.ts_event,
        )
    }
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
    ts_event: Timestamp,
    ts_init: Timestamp,
) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        bid,
        ask,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

#[no_mangle]
pub extern "C" fn quote_tick_from_raw(
    instrument_id: InstrumentId,
    bid: i64,
    ask: i64,
    bid_price_prec: u8,
    ask_price_prec: u8,
    bid_size: u64,
    ask_size: u64,
    bid_size_prec: u8,
    ask_size_prec: u8,
    ts_event: Timestamp,
    ts_init: Timestamp,
) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from_raw(bid, bid_price_prec),
        Price::from_raw(ask, ask_price_prec),
        Quantity::from_raw(bid_size, bid_size_prec),
        Quantity::from_raw(ask_size, ask_size_prec),
        ts_event,
        ts_init,
    )
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn quote_tick_to_pystr(tick: &QuoteTick) -> *mut ffi::PyObject {
    string_to_pystr(tick.to_string().as_str())
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
    ts_event: u64,
    ts_init: u64,
) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from_raw(price, price_prec),
        Quantity::from_raw(size, size_prec),
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn trade_tick_to_pystr(tick: &TradeTick) -> *mut ffi::PyObject {
    string_to_pystr(tick.to_string().as_str())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::data::tick::{QuoteTick, TradeTick};
    use crate::enums::OrderSide;
    use crate::identifiers::instrument_id::InstrumentId;
    use crate::identifiers::trade_id::TradeId;
    use crate::types::price::Price;
    use crate::types::quantity::Quantity;

    #[test]
    fn test_quote_tick_to_string() {
        let tick = QuoteTick {
            instrument_id: InstrumentId::from("ETH-PERP.FTX"),
            bid: Price::new(10000.0, 4),
            ask: Price::new(10001.0, 4),
            bid_size: Quantity::new(1.0, 8),
            ask_size: Quantity::new(1.0, 8),
            ts_event: 0,
            ts_init: 0,
        };

        assert_eq!(
            tick.to_string(),
            "ETH-PERP.FTX,10000.0000,10001.0000,1.00000000,1.00000000,0"
        );
    }

    #[test]
    fn test_trade_tick_to_string() {
        let tick = TradeTick {
            instrument_id: InstrumentId::from("ETH-PERP.FTX"),
            price: Price::new(10000.0, 4),
            size: Quantity::new(1.0, 8),
            aggressor_side: OrderSide::Buy,
            trade_id: TradeId::new("123456789"),
            ts_event: 0,
            ts_init: 0,
        };

        assert_eq!(
            tick.to_string(),
            "ETH-PERP.FTX,10000.0000,1.00000000,BUY,123456789,0"
        );
    }
}

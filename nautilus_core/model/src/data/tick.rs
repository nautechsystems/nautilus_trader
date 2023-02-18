// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::ffi::c_char;
use std::fmt::{Display, Formatter, Result};

use nautilus_core::correctness;
use nautilus_core::string::string_to_cstr;
use nautilus_core::time::UnixNanos;

use crate::enums::AggressorSide;
use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::trade_id::TradeId;
use crate::types::price::Price;
use crate::types::quantity::Quantity;

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QuoteTick {
    pub instrument_id: InstrumentId,
    pub bid: Price,
    pub ask: Price,
    pub bid_size: Quantity,
    pub ask_size: Quantity,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl QuoteTick {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        bid: Price,
        ask: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
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
    pub aggressor_side: AggressorSide,
    pub trade_id: TradeId,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl TradeTick {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
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
pub extern "C" fn quote_tick_copy(tick: &QuoteTick) -> QuoteTick {
    tick.clone()
}

#[no_mangle]
pub extern "C" fn quote_tick_new(
    instrument_id: InstrumentId,
    bid: Price,
    ask: Price,
    bid_size: Quantity,
    ask_size: Quantity,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
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
    ts_event: UnixNanos,
    ts_init: UnixNanos,
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

/// Returns a [`QuoteTick`] as a C string pointer.
#[no_mangle]
pub extern "C" fn quote_tick_to_cstr(tick: &QuoteTick) -> *const c_char {
    string_to_cstr(&tick.to_string())
}

#[no_mangle]
pub extern "C" fn trade_tick_free(tick: TradeTick) {
    drop(tick); // Memory freed here
}

#[no_mangle]
pub extern "C" fn trade_tick_copy(tick: &TradeTick) -> TradeTick {
    tick.clone()
}

#[no_mangle]
pub extern "C" fn trade_tick_from_raw(
    instrument_id: InstrumentId,
    price: i64,
    price_prec: u8,
    size: u64,
    size_prec: u8,
    aggressor_side: AggressorSide,
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

/// Returns a [`TradeTick`] as a C string pointer.
#[no_mangle]
pub extern "C" fn trade_tick_to_cstr(tick: &TradeTick) -> *const c_char {
    string_to_cstr(&tick.to_string())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::data::tick::{QuoteTick, TradeTick};
    use crate::enums::AggressorSide;
    use crate::identifiers::instrument_id::InstrumentId;
    use crate::identifiers::trade_id::TradeId;
    use crate::types::price::Price;
    use crate::types::quantity::Quantity;

    #[test]
    fn test_quote_tick_to_string() {
        let tick = QuoteTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            bid: Price::new(10000.0, 4),
            ask: Price::new(10001.0, 4),
            bid_size: Quantity::new(1.0, 8),
            ask_size: Quantity::new(1.0, 8),
            ts_event: 0,
            ts_init: 0,
        };
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,10001.0000,1.00000000,1.00000000,0"
        );
    }

    #[test]
    fn test_trade_tick_to_string() {
        let tick = TradeTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            price: Price::new(10000.0, 4),
            size: Quantity::new(1.0, 8),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789"),
            ts_event: 0,
            ts_init: 0,
        };
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,1.00000000,BUYER,123456789,0"
        );
    }
}

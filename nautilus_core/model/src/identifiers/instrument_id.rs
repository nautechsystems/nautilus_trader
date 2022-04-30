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

use crate::identifiers::symbol::Symbol;
use crate::identifiers::venue::Venue;
use nautilus_core::buffer::{Buffer128, Buffer16};
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct InstrumentId {
    pub symbol: Symbol,
    pub venue: Venue,
}

impl From<&str> for InstrumentId {
    fn from(value: &str) -> Self {
        let pieces: Vec<&str> = value.split('.').collect();
        assert!(pieces.len() >= 2, "malformed `InstrumentId` string");
        InstrumentId {
            symbol: Symbol::from(pieces[0]),
            venue: Venue::from(pieces[1]),
        }
    }
}

impl InstrumentId {
    pub fn new(symbol: Symbol, venue: Venue) -> InstrumentId {
        InstrumentId { symbol, venue }
    }
}

impl Display for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}.{}", self.symbol, self.venue)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn instrument_id_free(instrument_id: InstrumentId) {
    drop(instrument_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn instrument_id_from_buffers(symbol: Buffer128, venue: Buffer16) -> InstrumentId {
    let symbol = Symbol { value: symbol };
    let venue = Venue { value: venue };
    InstrumentId::new(symbol, venue)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::identifiers::instrument_id::InstrumentId;

    #[test]
    fn test_instrument_id_from_str() {
        let instrument_id1 = InstrumentId::from("ETH/USDT.BINANCE");
        let instrument_id2 = InstrumentId::from("XBT/USD.BITMEX");

        assert_eq!(instrument_id1, instrument_id1);
        assert_ne!(instrument_id1, instrument_id2);
        assert_eq!(instrument_id1.to_string(), "ETH/USDT.BINANCE")
    }
}

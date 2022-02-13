// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq)]
pub struct InstrumentId {
    pub symbol: Symbol,
    pub venue: Venue,
    pub value: Box<String>,
}

impl InstrumentId {
    pub fn new(symbol: Symbol, venue: Venue) -> InstrumentId {
        let mut s = symbol.to_string();
        s.push_str(".");
        s.push_str(&venue.value);
        InstrumentId {
            symbol,
            venue,
            value: Box::new(s),
        }
    }

    pub fn from_str(value: &str) -> InstrumentId {
        let pieces: Vec<&str> = value.split(".").collect();
        assert!(pieces.len() >= 2);
        InstrumentId {
            symbol: Symbol::from(&String::from(pieces[0])),
            venue: Venue::from(&String::from(pieces[1])),
            value: Box::new(value.parse().unwrap()),
        }
    }

    pub fn to_string(self) -> String {
        let mut output = self.symbol.value.to_string();
        output.push_str("."); // Delimiter
        output.push_str(&self.venue.to_string());
        output
    }

    //##########################################################################
    // C API
    //##########################################################################
    pub unsafe fn instrument_id_new(ptr: *mut u8, length: usize) -> InstrumentId {
        // SAFETY: Checks ptr is a valid UTF-8 string
        let vec = Vec::from_raw_parts(ptr, length, length);
        let s = String::from_utf8(vec).expect("Invalid UTF-8 string");
        let pieces: Vec<&str> = s.split(".").collect();
        assert!(pieces.len() >= 2);
        InstrumentId::new(
            Symbol::from(&String::from(pieces[0])),
            Venue::from(&String::from(pieces[1])),
        )
    }

    #[no_mangle]
    pub extern "C" fn instrument_id_free(id: InstrumentId) {
        drop(id); // Memory freed here
    }

    #[no_mangle]
    pub extern "C" fn instrument_id_len(id: InstrumentId) -> usize {
        id.symbol.value.len()
    }

    #[no_mangle]
    pub extern "C" fn instrument_id_as_utf8(&self) -> *const u8 {
        self.value.as_ptr()
    }
}

impl Debug for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}.{}", self.symbol.value, self.venue.value)
    }
}

impl Display for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}.{}", self.symbol.value, self.venue.value)
    }
}

#[cfg(test)]
mod tests {
    use crate::identifiers::instrument_id::InstrumentId;

    #[test]
    fn test_instrument_id_from_str() {
        let instrument_id1 = InstrumentId::from_str("ETH/USDT.BINANCE");
        let instrument_id2 = InstrumentId::from_str("XBT/USD.BITMEX");

        assert_eq!(instrument_id1, instrument_id1);
        assert_ne!(instrument_id1, instrument_id2);
        assert_eq!(instrument_id1.symbol.value.len(), 8);
        assert_eq!(instrument_id1.venue.value.len(), 7);
        assert_eq!(instrument_id1.to_string(), "ETH/USDT.BINANCE")
    }
}

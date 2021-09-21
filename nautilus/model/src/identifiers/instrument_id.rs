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
use std::ffi::CStr;
use std::fmt::{Debug, Display, Formatter, Result};
use std::os::raw::c_char;

#[repr(C)]
#[derive(Clone, Hash, PartialEq)]
pub struct InstrumentId {
    pub symbol: Symbol,
    pub venue: Venue,
}

impl InstrumentId {
    pub fn new(symbol: Symbol, venue: Venue) -> InstrumentId {
        InstrumentId { symbol, venue }
    }

    pub fn from_str(value: &str) -> InstrumentId {
        let pieces: Vec<&str> = value.split(".").collect();
        assert!(pieces.len() >= 2);
        InstrumentId {
            symbol: Symbol::from_str(&String::from(pieces[0])),
            venue: Venue::from_str(&String::from(pieces[1])),
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
    pub unsafe fn instrument_id_from_raw(ptr: *const c_char) -> InstrumentId {
        // SAFETY: checks `ptr` can be parsed into a valid C string
        let s = CStr::from_ptr(ptr).to_str().expect("invalid UTF-8 string");
        let pieces: Vec<&str> = s.split(".").collect();
        assert!(pieces.len() >= 2);
        InstrumentId {
            symbol: Symbol::from_str(&String::from(pieces[0])),
            venue: Venue::from_str(&String::from(pieces[1])),
        }
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

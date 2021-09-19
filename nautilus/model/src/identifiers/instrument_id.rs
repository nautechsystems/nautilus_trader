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
use std::ffi::CString;
use std::os::raw::c_char;

#[repr(C)]
#[derive(Copy, Clone, Hash, PartialEq, Debug)]
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

    pub unsafe fn from_raw(value: *mut c_char) -> InstrumentId {
        let s = CString::from_raw(value)
            .into_string()
            .expect("Cannot parse `value` to InstrumentId");
        let pieces: Vec<&str> = s.split(".").collect();
        InstrumentId {
            symbol: Symbol::from_str(&String::from(pieces[0])),
            venue: Venue::from_str(&String::from(pieces[1])),
        }
    }

    pub unsafe fn to_string(self) -> String {
        let mut output = self.symbol.to_string();
        output.push_str("."); // Delimiter
        output.push_str(&self.venue.to_string());
        output
    }

    pub unsafe fn to_cstring(self) -> CString {
        let mut output = self.symbol.to_string();
        output.push_str("."); // Delimiter
        output.push_str(&self.venue.to_string());
        CString::new(output).unwrap()
    }

    #[no_mangle]
    pub unsafe extern "C" fn new_instrument_id(value: *mut c_char) -> Symbol {
        Symbol::from_raw(value)
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
        assert_eq!(instrument_id1.symbol.len, 8);
        assert_eq!(instrument_id1.venue.len, 7);
    }
}

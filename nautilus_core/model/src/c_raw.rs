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
// ------------------------------------------------------------------------------------------------

use crate::enums::BookLevel;
use crate::identifiers::base::Identifier;
use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::symbol::Symbol;
use crate::identifiers::venue::Venue;
use crate::objects::price::Price;
use crate::objects::quantity::Quantity;
use crate::orderbook::book::OrderBook;
use std::ffi::CStr;
use std::os::raw::c_char;

#[no_mangle]
pub unsafe extern "C" fn symbol_new(ptr: *mut u8, length: usize) -> Symbol {
    // SAFETY: Checks ptr is a valid UTF-8 string
    let vec = Vec::from_raw_parts(ptr, length, length);
    let s = String::from_utf8(vec).expect("Invalid UTF-8 string");
    Symbol::from_str(s.as_str())
}

#[no_mangle]
pub extern "C" fn symbol_free(s: Symbol) {
    drop(s); // Memory freed here
}

#[no_mangle]
pub extern "C" fn symbol_as_utf8(s: Symbol) -> *const u8 {
    s.as_str().as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn venue_new(ptr: *mut u8, length: usize) -> Venue {
    // SAFETY: Expects `ptr` is an array of valid UTF-8 chars
    let vec = Vec::from_raw_parts(ptr, length, length);
    let s = String::from_utf8(vec).expect("Invalid UTF-8 string");
    Venue::from_str(s.as_str())
}

#[no_mangle]
pub extern "C" fn venue_free(v: Venue) {
    drop(v); // Memory freed here
}

#[no_mangle]
pub extern "C" fn venue_as_utf8(v: Venue) -> *const u8 {
    v.as_str().as_ptr()
}

/// Expects `ptr` to be an array of valid UTF-8 chars with a null byte terminator.
#[no_mangle]
pub unsafe fn instrument_id_from_raw(ptr: *const c_char) -> InstrumentId {
    // SAFETY: Checks ptr is a valid UTF-8 string
    let s = CStr::from_ptr(ptr).to_str().expect("invalid C string");
    let pieces: Vec<&str> = s.split('.').collect();
    assert!(pieces.len() >= 2);
    InstrumentId::new(Symbol::from_str(pieces[0]), Venue::from_str(pieces[1]))
}

#[no_mangle]
pub extern "C" fn instrument_id_free(id: InstrumentId) {
    drop(id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn instrument_id_as_utf8(id: InstrumentId) -> *const u8 {
    id.as_str().as_ptr()
}

#[no_mangle]
pub extern "C" fn price_new(value: f64, precision: u8) -> Price {
    Price::new(value, precision)
}

#[no_mangle]
pub extern "C" fn quantity_new(value: f64, precision: u8) -> Quantity {
    Quantity::new(value, precision)
}

#[cfg(test)]
mod tests {
    use crate::c_raw::instrument_id_from_raw;
    use crate::identifiers::base::Identifier;
    use crate::identifiers::symbol::Symbol;
    use crate::identifiers::venue::Venue;
    use std::ffi::CString;

    #[test]
    fn test_instrument_id_new() {
        unsafe {
            let cstring = CString::new("ETH/USDT.BINANCE").unwrap();

            let result = instrument_id_from_raw(cstring.as_ptr());

            assert_eq!(result.symbol, Symbol::from_str("ETH/USDT"));
            assert_eq!(result.venue, Venue::from_str("BINANCE"));
        }
    }
}

#[no_mangle]
pub extern "C" fn order_book_new(instrument_id: InstrumentId, book_level: BookLevel) -> OrderBook {
    OrderBook::new(instrument_id, book_level)
}

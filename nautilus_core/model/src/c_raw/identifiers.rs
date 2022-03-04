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

use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::symbol::Symbol;
use crate::identifiers::venue::Venue;
use nautilus_core::string::{from_cstring, into_cstring};
use std::os::raw::c_char;

////////////////////////////////////////////////////////////////////////////////
// Symbol
////////////////////////////////////////////////////////////////////////////////

/// Creates a new `Symbol` from the given raw C string `ptr`.
///
/// # Safety
/// Expects `ptr` to be an array of valid UTF-8 chars with the trailing nul byte terminator.
#[no_mangle]
pub unsafe extern "C" fn symbol_from_cstring(ptr: *const c_char) -> Symbol {
    Symbol::from_string(from_cstring(ptr))
}

/// Create a C string pointer to the identifiers underlying UTF-8 chars value.
#[no_mangle]
pub extern "C" fn symbol_to_cstring(symbol: &Symbol) -> *const c_char {
    into_cstring(symbol.to_string())
}

/// Move the given `Symbol` identifier to Rust and then drop.
#[no_mangle]
pub extern "C" fn symbol_free(symbol: Symbol) {
    drop(symbol); // Memory freed here
}

////////////////////////////////////////////////////////////////////////////////
// Venue
////////////////////////////////////////////////////////////////////////////////

/// Creates a new `Venue` from the given raw C string `ptr`.
///
/// # Safety
/// Expects `ptr` to be an array of valid UTF-8 chars with the trailing nul byte terminator.
#[no_mangle]
pub unsafe extern "C" fn venue_from_cstring(ptr: *const c_char) -> Venue {
    Venue::from_string(from_cstring(ptr))
}

/// Create a C string pointer to the identifiers underlying UTF-8 chars value.
#[no_mangle]
pub extern "C" fn venue_to_cstring(venue: &Venue) -> *const c_char {
    into_cstring(venue.to_string())
}

/// Move the given `Venue` identifier to Rust and then drop.
#[no_mangle]
pub extern "C" fn venue_free(venue: Venue) {
    drop(venue); // Memory freed here
}

////////////////////////////////////////////////////////////////////////////////
// InstrumentId
////////////////////////////////////////////////////////////////////////////////

/// Creates a new `InstrumentId` from the given raw C string `ptr`.
///
/// # Safety
/// Expects `ptr` to be an array of valid UTF-8 chars with the trailing nul byte terminator.
#[no_mangle]
pub unsafe fn instrument_id_from_cstring(ptr: *const c_char) -> InstrumentId {
    // SAFETY: Checks ptr is a valid UTF-8 string
    let s = from_cstring(ptr);
    let pieces: Vec<&str> = s.split('.').collect();
    assert!(pieces.len() >= 2);
    InstrumentId::new(Symbol::from_str(pieces[0]), Venue::from_str(pieces[1]))
}

/// Create a C string pointer to the identifiers underlying UTF-8 chars value.
#[no_mangle]
pub extern "C" fn instrument_id_to_cstring(instrument_id: &InstrumentId) -> *const c_char {
    into_cstring(instrument_id.to_string())
}

/// Move the given `InstrumentId` to Rust and then drop.
#[no_mangle]
pub extern "C" fn instrument_id_free(instrument_id: InstrumentId) {
    drop(instrument_id); // Memory freed here
}

#[cfg(test)]
mod tests {
    use crate::c_raw::identifiers::{instrument_id_free, instrument_id_from_cstring, instrument_id_to_cstring, symbol_free, symbol_from_cstring, symbol_to_cstring, venue_free, venue_from_cstring, venue_to_cstring};
    use crate::identifiers::symbol::Symbol;
    use crate::identifiers::venue::Venue;
    use crate::identifiers::instrument_id::InstrumentId;

    #[test]
    fn test_symbol_cstring_conversions() {
        unsafe {
            let symbol = Symbol::from_str("ETHUSDT");

            let cstring = symbol_to_cstring(&symbol);
            let result = symbol_from_cstring(cstring);

            assert_eq!(result, symbol);
        }
    }

    #[test]
    fn test_symbol_free() {
        let symbol = Symbol::from_str("ETHUSDT");
        symbol_free(symbol);
    }

    #[test]
    fn test_venue_cstring_conversions() {
        unsafe {
            let venue = Venue::from_str("BINANCE");

            let cstring = venue_to_cstring(&venue);
            let result = venue_from_cstring(cstring);

            assert_eq!(result, venue);
        }
    }

    #[test]
    fn test_venue_free() {
        let venue = Venue::from_str("BINANCE");
        venue_free(venue);
    }

    #[test]
    fn test_instrument_id_cstring_conversions() {
        unsafe {
            let instrument_id = InstrumentId::from_str("ETHUSDT.BINANCE");

            let cstring = instrument_id_to_cstring(&instrument_id);
            let result = instrument_id_from_cstring(cstring);

            assert_eq!(result, instrument_id);
        }
    }

    #[test]
    fn test_instrument_id_free() {
        let instrument_id = InstrumentId::from_str("ETHUSDT.BINANCE");
        instrument_id_free(instrument_id);
    }
}

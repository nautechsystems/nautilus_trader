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

use crate::identifiers::symbol::{symbol_from_pystr, Symbol};
use crate::identifiers::venue::{venue_from_pystr, Venue};
use nautilus_core::string::string_to_pystr;
use pyo3::ffi;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
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

impl From<&String> for InstrumentId {
    fn from(value: &String) -> Self {
        let pieces: Vec<&str> = value.split('.').collect();
        assert!(pieces.len() >= 2, "malformed `InstrumentId` string");
        InstrumentId {
            symbol: Symbol::from(pieces[0]),
            venue: Venue::from(pieces[1]),
        }
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

/// Returns a Nautilus identifier from valid Python object pointers.
///
/// # Safety
/// - `symbol_ptr` and `venue_ptr` must be borrowed from a valid Python UTF-8 `str`(s).
#[no_mangle]
pub unsafe extern "C" fn instrument_id_from_pystrs(
    symbol_ptr: *mut ffi::PyObject,
    venue_ptr: *mut ffi::PyObject,
) -> InstrumentId {
    let symbol = symbol_from_pystr(symbol_ptr);
    let venue = venue_from_pystr(venue_ptr);
    InstrumentId { symbol, venue }
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn instrument_id_to_pystr(
    instrument_id: &InstrumentId,
) -> *mut ffi::PyObject {
    string_to_pystr(instrument_id.to_string().as_str())
}

#[no_mangle]
pub extern "C" fn instrument_id_eq(lhs: &InstrumentId, rhs: &InstrumentId) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn instrument_id_hash(instrument_id: &InstrumentId) -> u64 {
    let mut h = DefaultHasher::new();
    instrument_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::InstrumentId;
    use crate::identifiers::instrument_id::instrument_id_free;

    #[test]
    fn test_equality() {
        let id1 = InstrumentId::from("ETH/USDT.BINANCE");
        let id2 = InstrumentId::from("XBT/USD.BITMEX");

        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = InstrumentId::from("ETH/USDT.BINANCE");

        assert_eq!(id.to_string(), "ETH/USDT.BINANCE");
        assert_eq!(format!("{id}"), "ETH/USDT.BINANCE");
    }

    #[test]
    fn test_instrument_id_free() {
        let id = InstrumentId::from("ETH/USDT.BINANCE");

        instrument_id_free(id); // No panic
    }
}

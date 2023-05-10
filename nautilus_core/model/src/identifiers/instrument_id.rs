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

use std::collections::hash_map::DefaultHasher;
use std::ffi::c_char;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use nautilus_core::string::{cstr_to_string, string_to_cstr};
use thiserror::Error;

use crate::identifiers::symbol::Symbol;
use crate::identifiers::venue::Venue;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct InstrumentId {
    pub symbol: Symbol,
    pub venue: Venue,
}

#[derive(Debug, Error)]
#[error("Error parsing `InstrumentId` from '{input}'")]
pub struct InstrumentIdParseError {
    input: String,
}

impl FromStr for InstrumentId {
    type Err = InstrumentIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.rsplit_once('.') {
            Some((symbol_part, venue_part)) => Ok(Self {
                symbol: Symbol::new(symbol_part),
                venue: Venue::new(venue_part),
            }),
            None => Err(InstrumentIdParseError {
                input: s.to_string(),
            }),
        }
    }
}

impl Display for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.symbol, self.venue)
    }
}

impl InstrumentId {
    #[must_use]
    pub fn new(symbol: Symbol, venue: Venue) -> Self {
        Self { symbol, venue }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn instrument_id_new(symbol: &Symbol, venue: &Venue) -> InstrumentId {
    let symbol = symbol.clone();
    let venue = venue.clone();
    InstrumentId::new(symbol, venue)
}

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn instrument_id_new_from_cstr(ptr: *const c_char) -> InstrumentId {
    InstrumentId::from_str(cstr_to_string(ptr).as_str()).unwrap()
}

#[no_mangle]
pub extern "C" fn instrument_id_clone(instrument_id: &InstrumentId) -> InstrumentId {
    instrument_id.clone()
}

/// Frees the memory for the given `instrument_id` by dropping.
#[no_mangle]
pub extern "C" fn instrument_id_drop(instrument_id: InstrumentId) {
    drop(instrument_id); // Memory freed here
}

/// Returns an [`InstrumentId`] as a C string pointer.
#[no_mangle]
pub extern "C" fn instrument_id_to_cstr(instrument_id: &InstrumentId) -> *const c_char {
    string_to_cstr(&instrument_id.to_string())
}

#[no_mangle]
pub extern "C" fn instrument_id_eq(lhs: &InstrumentId, rhs: &InstrumentId) -> u8 {
    u8::from(lhs == rhs)
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
    use std::{ffi::CStr, str::FromStr};

    use super::InstrumentId;
    use crate::identifiers::instrument_id::{
        instrument_id_drop, instrument_id_to_cstr, InstrumentIdParseError,
    };

    #[test]
    fn test_instrument_id_parse_success() {
        let instrument_id = InstrumentId::from_str("ETH/USDT.BINANCE").unwrap();
        assert_eq!(instrument_id.symbol.to_string(), "ETH/USDT");
        assert_eq!(instrument_id.venue.to_string(), "BINANCE");
    }

    #[test]
    fn test_instrument_id_parse_failure_no_dot() {
        let result = InstrumentId::from_str("ETHUSDT-BINANCE");
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(matches!(error, InstrumentIdParseError { .. }));
        assert_eq!(
            error.to_string(),
            "Error parsing `InstrumentId` from 'ETHUSDT-BINANCE'"
        );
    }

    #[ignore] // Cannot implement yet due Betfair instrument IDs
    #[test]
    fn test_instrument_id_parse_failure_multiple_dots() {
        let result = InstrumentId::from_str("ETH.USDT.BINANCE");
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(matches!(error, InstrumentIdParseError { .. }));
        assert_eq!(
            error.to_string(),
            "Error parsing `InstrumentId` from 'ETH.USDT.BINANCE'"
        );
    }

    #[test]
    fn test_equality() {
        let id1 = InstrumentId::from_str("ETH/USDT.BINANCE").unwrap();
        let id2 = InstrumentId::from_str("XBT/USD.BITMEX").unwrap();
        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = InstrumentId::from_str("ETH/USDT.BINANCE").unwrap();
        assert_eq!(id.to_string(), "ETH/USDT.BINANCE");
        assert_eq!(format!("{id}"), "ETH/USDT.BINANCE");
    }

    #[test]
    fn test_to_cstr() {
        unsafe {
            let id = InstrumentId::from_str("ETH/USDT.BINANCE").unwrap();
            let result = instrument_id_to_cstr(&id);
            assert_eq!(CStr::from_ptr(result).to_str().unwrap(), "ETH/USDT.BINANCE");
        }
    }

    #[test]
    fn test_instrument_id_drop() {
        let id = InstrumentId::from_str("ETH/USDT.BINANCE").unwrap();

        instrument_id_drop(id); // No panic
    }
}

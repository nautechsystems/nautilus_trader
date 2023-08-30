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

use std::{
    collections::hash_map::DefaultHasher,
    ffi::c_char,
    fmt::{Debug, Display, Formatter},
    hash::{Hash, Hasher},
    str::FromStr,
};

use anyhow::Result;
use nautilus_core::string::{cstr_to_string, str_to_cstr};
use pyo3::prelude::*;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror;

use crate::identifiers::{symbol::Symbol, venue::Venue};

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
#[pyclass]
pub struct InstrumentId {
    pub symbol: Symbol,
    pub venue: Venue,
}

#[derive(thiserror::Error, Debug)]
#[error("Error parsing `InstrumentId` from '{input}'")]
pub struct InstrumentIdParseError {
    input: String,
}

impl InstrumentId {
    pub fn new(symbol: Symbol, venue: Venue) -> Self {
        Self { symbol, venue }
    }

    pub fn is_synthetic(&self) -> bool {
        self.venue.is_synthetic()
    }
}

impl FromStr for InstrumentId {
    type Err = InstrumentIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.rsplit_once('.') {
            Some((symbol_part, venue_part)) => Ok(Self {
                symbol: Symbol::new(symbol_part).unwrap(), // Implement error handling
                venue: Venue::new(venue_part).unwrap(),    // Implement error handling
            }),
            None => Err(InstrumentIdParseError {
                input: s.to_string(),
            }),
        }
    }
}

impl From<&str> for InstrumentId {
    fn from(input: &str) -> Self {
        Self::from_str(input).unwrap()
    }
}

impl Debug for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}.{}\"", self.symbol, self.venue)
    }
}

impl Display for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.symbol, self.venue)
    }
}

impl Serialize for InstrumentId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for InstrumentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let instrument_id_str = String::deserialize(deserializer)?;
        InstrumentId::from_str(&instrument_id_str)
            .map_err(|err| serde::de::Error::custom(err.to_string()))
    }
}

#[pymethods]
impl InstrumentId {
    #[getter]
    fn value(&self) -> String {
        self.to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn instrument_id_new(symbol: Symbol, venue: Venue) -> InstrumentId {
    InstrumentId::new(symbol, venue)
}

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn instrument_id_new_from_cstr(ptr: *const c_char) -> InstrumentId {
    InstrumentId::from(cstr_to_string(ptr).as_str())
}

/// Returns an [`InstrumentId`] as a C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn instrument_id_to_cstr(instrument_id: &InstrumentId) -> *const c_char {
    str_to_cstr(&instrument_id.to_string())
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn instrument_id_hash(instrument_id: &InstrumentId) -> u64 {
    let mut h = DefaultHasher::new();
    instrument_id.hash(&mut h);
    h.finish()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn instrument_id_is_synthetic(instrument_id: &InstrumentId) -> u8 {
    u8::from(instrument_id.is_synthetic())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{ffi::CStr, str::FromStr};

    use super::InstrumentId;
    use crate::identifiers::{
        instrument_id::{
            instrument_id_new_from_cstr, instrument_id_to_cstr, InstrumentIdParseError,
        },
        symbol::Symbol,
        venue::Venue,
    };

    #[test]
    fn test_instrument_id_parse_success() {
        let instrument_id = InstrumentId::from("ETH/USDT.BINANCE");
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
    fn test_string_reprs() {
        let id = InstrumentId::from("ETH/USDT.BINANCE");
        assert_eq!(id.to_string(), "ETH/USDT.BINANCE");
        assert_eq!(format!("{id}"), "ETH/USDT.BINANCE");
    }

    #[test]
    fn test_to_cstr() {
        unsafe {
            let id = InstrumentId::from("ETH/USDT.BINANCE");
            let result = instrument_id_to_cstr(&id);
            assert_eq!(CStr::from_ptr(result).to_str().unwrap(), "ETH/USDT.BINANCE");
        }
    }

    #[test]
    fn test_to_cstr_and_back() {
        unsafe {
            let id = InstrumentId::from("ETH/USDT.BINANCE");
            let result = instrument_id_to_cstr(&id);
            let id2 = instrument_id_new_from_cstr(result);
            assert_eq!(id, id2);
        }
    }

    #[test]
    fn test_from_symbol_and_back() {
        unsafe {
            let id = InstrumentId::new(Symbol::from("ETH/USDT"), Venue::from("BINANCE"));
            let result = instrument_id_to_cstr(&id);
            let id2 = instrument_id_new_from_cstr(result);
            assert_eq!(id, id2);
        }
    }
}

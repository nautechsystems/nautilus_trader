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

use anyhow::{anyhow, bail, Result};
use nautilus_core::{
    python::to_pyvalue_err,
    string::{cstr_to_str, str_to_cstr},
};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyString, PyTuple},
};
use serde::{Deserialize, Deserializer, Serialize};

use crate::identifiers::{symbol::Symbol, venue::Venue};

/// Represents a valid instrument ID.
///
/// The symbol and venue combination should uniquely identify the instrument.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct InstrumentId {
    /// The instruments ticker symbol.
    pub symbol: Symbol,
    /// The instruments trading venue.
    pub venue: Venue,
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
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.rsplit_once('.') {
            Some((symbol_part, venue_part)) => Ok(Self {
                symbol: Symbol::new(symbol_part)
                    .map_err(|e| anyhow!(err_message(s, e.to_string())))?,
                venue: Venue::new(venue_part)
                    .map_err(|e| anyhow!(err_message(s, e.to_string())))?,
            }),
            None => {
                bail!(err_message(
                    s,
                    "Missing '.' separator between symbol and venue components".to_string()
                ))
            }
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

fn err_message(s: &str, e: String) -> String {
    format!("Error parsing `InstrumentId` from '{s}': {e}")
}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl InstrumentId {
    #[new]
    fn py_new(symbol: Symbol, venue: Venue) -> PyResult<Self> {
        Ok(InstrumentId::new(symbol, venue))
    }

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let tuple: (&PyString, &PyString) = state.extract(py)?;
        self.symbol = Symbol::new(tuple.0.extract()?).map_err(to_pyvalue_err)?;
        self.venue = Venue::new(tuple.1.extract()?).map_err(to_pyvalue_err)?;
        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((self.symbol.to_string(), self.venue.to_string()).to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(InstrumentId::from_str("NULL.NULL").unwrap()) // Safe default
    }

    fn __richcmp__(&self, other: PyObject, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        if let Ok(other) = other.extract::<InstrumentId>(py) {
            match op {
                CompareOp::Eq => self.eq(&other).into_py(py),
                CompareOp::Ne => self.ne(&other).into_py(py),
                _ => py.NotImplemented(),
            }
        } else {
            py.NotImplemented()
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{}('{}')", stringify!(InstrumentId), self)
    }

    #[getter]
    #[pyo3(name = "symbol")]
    fn py_symbol(&self) -> Symbol {
        self.symbol
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    fn value(&self) -> String {
        self.to_string()
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<InstrumentId> {
        InstrumentId::from_str(value).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_synthetic")]
    fn py_is_synthetic(&self) -> bool {
        self.is_synthetic()
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

/// Returns any [`InstrumentId`] parsing error from the provided C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn instrument_id_check_parsing(ptr: *const c_char) -> *const c_char {
    match InstrumentId::from_str(cstr_to_str(ptr)) {
        Ok(_) => str_to_cstr(""),
        Err(e) => str_to_cstr(&e.to_string()),
    }
}

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn instrument_id_from_cstr(ptr: *const c_char) -> InstrumentId {
    InstrumentId::from(cstr_to_str(ptr))
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

#[cfg(test)]
pub mod stubs {
    use std::str::FromStr;

    use rstest::fixture;

    use crate::identifiers::{
        instrument_id::InstrumentId,
        symbol::{stubs::*, Symbol},
        venue::{stubs::*, Venue},
    };

    #[fixture]
    pub fn btc_usdt_perp_binance() -> InstrumentId {
        InstrumentId::from_str("BTCUSDT-PERP.BINANCE").unwrap()
    }

    #[fixture]
    pub fn audusd_sim(aud_usd: Symbol, sim: Venue) -> InstrumentId {
        InstrumentId {
            symbol: aud_usd,
            venue: sim,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{ffi::CStr, str::FromStr};

    use rstest::rstest;

    use super::InstrumentId;
    use crate::identifiers::{
        instrument_id::{instrument_id_from_cstr, instrument_id_to_cstr},
        symbol::Symbol,
        venue::Venue,
    };

    #[rstest]
    fn test_instrument_id_parse_success() {
        let instrument_id = InstrumentId::from("ETH/USDT.BINANCE");
        assert_eq!(instrument_id.symbol.to_string(), "ETH/USDT");
        assert_eq!(instrument_id.venue.to_string(), "BINANCE");
    }

    #[rstest]
    fn test_instrument_id_parse_failure_no_dot() {
        let result = InstrumentId::from_str("ETHUSDT-BINANCE");
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert_eq!(
            error.to_string(),
            "Error parsing `InstrumentId` from 'ETHUSDT-BINANCE': Missing '.' separator between symbol and venue components"
        );
    }

    #[rstest]
    fn test_string_reprs() {
        let id = InstrumentId::from("ETH/USDT.BINANCE");
        assert_eq!(id.to_string(), "ETH/USDT.BINANCE");
        assert_eq!(format!("{id}"), "ETH/USDT.BINANCE");
    }

    #[rstest]
    fn test_to_cstr() {
        unsafe {
            let id = InstrumentId::from("ETH/USDT.BINANCE");
            let result = instrument_id_to_cstr(&id);
            assert_eq!(CStr::from_ptr(result).to_str().unwrap(), "ETH/USDT.BINANCE");
        }
    }

    #[rstest]
    fn test_to_cstr_and_back() {
        unsafe {
            let id = InstrumentId::from("ETH/USDT.BINANCE");
            let result = instrument_id_to_cstr(&id);
            let id2 = instrument_id_from_cstr(result);
            assert_eq!(id, id2);
        }
    }

    #[rstest]
    fn test_from_symbol_and_back() {
        unsafe {
            let id = InstrumentId::new(Symbol::from("ETH/USDT"), Venue::from("BINANCE"));
            let result = instrument_id_to_cstr(&id);
            let id2 = instrument_id_from_cstr(result);
            assert_eq!(id, id2);
        }
    }
}

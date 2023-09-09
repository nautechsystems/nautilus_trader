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
    ffi::{c_char, CStr},
    hash::{Hash, Hasher},
    str::FromStr,
};

use anyhow::{anyhow, Result};
use nautilus_core::{
    correctness::check_valid_string,
    python::to_pyvalue_err,
    string::{cstr_to_string, str_to_cstr},
};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyLong, PyString, PyTuple},
};
use serde::{Deserialize, Serialize, Serializer};
use ustr::Ustr;

use super::fixed::check_fixed_precision;
use crate::{currencies::CURRENCY_MAP, enums::CurrencyType};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Currency {
    pub code: Ustr,
    pub precision: u8,
    pub iso4217: u16,
    pub name: Ustr,
    pub currency_type: CurrencyType,
}

impl Currency {
    pub fn new(
        code: &str,
        precision: u8,
        iso4217: u16,
        name: &str,
        currency_type: CurrencyType,
    ) -> Result<Self> {
        check_valid_string(code, "`Currency` code")?;
        check_valid_string(name, "`Currency` name")?;
        check_fixed_precision(precision)?;

        Ok(Self {
            code: Ustr::from(code),
            precision,
            iso4217,
            name: Ustr::from(name),
            currency_type,
        })
    }
}

impl PartialEq for Currency {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
    }
}

impl Hash for Currency {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.code.hash(state);
    }
}

impl FromStr for Currency {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let map_guard = CURRENCY_MAP
            .lock()
            .map_err(|e| anyhow!("Failed to acquire lock on `CURRENCY_MAP`: {e}"))?;
        map_guard
            .get(s)
            .copied()
            .ok_or_else(|| anyhow!("Unknown currency: {s}"))
    }
}

impl From<&str> for Currency {
    fn from(input: &str) -> Self {
        input.parse().unwrap()
    }
}

impl Serialize for Currency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.code.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let currency_str: &str = Deserialize::deserialize(deserializer)?;
        Currency::from_str(currency_str).map_err(serde::de::Error::custom)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl Currency {
    #[new]
    fn py_new(
        code: &str,
        precision: u8,
        iso4217: u16,
        name: &str,
        currency_type: CurrencyType,
    ) -> PyResult<Self> {
        Self::new(code, precision, iso4217, name, currency_type).map_err(to_pyvalue_err)
    }

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let tuple: (&PyString, &PyLong, &PyLong, &PyString, &PyString) = state.extract(py)?;
        self.code = Ustr::from(tuple.0.extract()?);
        self.precision = tuple.1.extract::<u8>()?;
        self.iso4217 = tuple.2.extract::<u16>()?;
        self.name = Ustr::from(tuple.3.extract()?);
        self.currency_type = tuple.4.extract()?;
        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((
            self.code.to_string(),
            self.precision,
            self.iso4217,
            self.name.to_string(),
            self.currency_type.to_string(),
        )
            .to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Currency::AUD()) // Safe default
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        self.code.precomputed_hash() as isize
    }

    fn __str__(&self) -> &'static str {
        self.code.as_str()
    }

    fn __repr__(&self) -> String {
        format!("{}('{:?}')", stringify!(Currency), self)
    }

    #[getter]
    #[pyo3(name = "code")]
    fn py_code(&self) -> &'static str {
        self.code.as_str()
    }

    #[getter]
    #[pyo3(name = "precision")]
    fn py_precision(&self) -> u8 {
        self.precision
    }

    #[getter]
    #[pyo3(name = "iso4217")]
    fn py_iso4217(&self) -> u16 {
        self.iso4217
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &'static str {
        self.name.as_str()
    }

    #[getter]
    #[pyo3(name = "currency_type")]
    fn py_currency_type(&self) -> CurrencyType {
        self.currency_type
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Currency> {
        Currency::from_str(value).map_err(to_pyvalue_err)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a [`Currency`] from pointers and primitives.
///
/// # Safety
///
/// - Assumes `code_ptr` is a valid C string pointer.
/// - Assumes `name_ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn currency_from_py(
    code_ptr: *const c_char,
    precision: u8,
    iso4217: u16,
    name_ptr: *const c_char,
    currency_type: CurrencyType,
) -> Currency {
    assert!(!code_ptr.is_null(), "`code_ptr` was NULL");
    assert!(!name_ptr.is_null(), "`name_ptr` was NULL");

    Currency::new(
        CStr::from_ptr(code_ptr)
            .to_str()
            .expect("CStr::from_ptr failed for `code_ptr`"),
        precision,
        iso4217,
        CStr::from_ptr(name_ptr)
            .to_str()
            .expect("CStr::from_ptr failed for `name_ptr`"),
        currency_type,
    )
    .unwrap()
}

#[no_mangle]
pub extern "C" fn currency_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(format!("{currency:?}").as_str())
}

#[no_mangle]
pub extern "C" fn currency_code_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(&currency.code)
}

#[no_mangle]
pub extern "C" fn currency_name_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(&currency.name)
}

#[no_mangle]
pub extern "C" fn currency_hash(currency: &Currency) -> u64 {
    currency.code.precomputed_hash()
}

#[no_mangle]
pub extern "C" fn currency_register(currency: Currency) {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .insert(currency.code.to_string(), currency);
}

/// # Safety
///
/// - Assumes `code_ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn currency_exists(code_ptr: *const c_char) -> u8 {
    let code = cstr_to_string(code_ptr);
    u8::from(CURRENCY_MAP.lock().unwrap().contains_key(&code))
}

/// # Safety
///
/// - Assumes `code_ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn currency_from_cstr(code_ptr: *const c_char) -> Currency {
    let code = cstr_to_string(code_ptr);
    Currency::from_str(&code).unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};

    use nautilus_core::string::str_to_cstr;
    use rstest::rstest;

    use super::*;
    use crate::{
        enums::CurrencyType,
        types::currency::{currency_exists, Currency},
    };

    #[rstest]
    #[should_panic(expected = "`Currency` code")]
    fn test_invalid_currency_code() {
        let _ = Currency::new("", 2, 840, "United States dollar", CurrencyType::Fiat).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision() {
        // Precision out of range for fixed
        let _ = Currency::new("USD", 10, 840, "United States dollar", CurrencyType::Fiat).unwrap();
    }

    #[rstest]
    fn test_new_for_fiat() {
        let currency =
            Currency::new("AUD", 2, 36, "Australian dollar", CurrencyType::Fiat).unwrap();
        assert_eq!(currency, currency);
        assert_eq!(currency.code.as_str(), "AUD");
        assert_eq!(currency.precision, 2);
        assert_eq!(currency.iso4217, 36);
        assert_eq!(currency.name.as_str(), "Australian dollar");
        assert_eq!(currency.currency_type, CurrencyType::Fiat);
    }

    #[rstest]
    fn test_new_for_crypto() {
        let currency = Currency::new("ETH", 8, 0, "Ether", CurrencyType::Crypto).unwrap();
        assert_eq!(currency, currency);
        assert_eq!(currency.code.as_str(), "ETH");
        assert_eq!(currency.precision, 8);
        assert_eq!(currency.iso4217, 0);
        assert_eq!(currency.name.as_str(), "Ether");
        assert_eq!(currency.currency_type, CurrencyType::Crypto);
    }

    #[rstest]
    fn test_equality() {
        let currency1 =
            Currency::new("USD", 2, 840, "United States dollar", CurrencyType::Fiat).unwrap();
        let currency2 =
            Currency::new("USD", 2, 840, "United States dollar", CurrencyType::Fiat).unwrap();
        assert_eq!(currency1, currency2);
    }

    #[rstest]
    fn test_serialization_deserialization() {
        let currency = Currency::USD();
        let serialized = serde_json::to_string(&currency).unwrap();
        let deserialized: Currency = serde_json::from_str(&serialized).unwrap();
        assert_eq!(currency, deserialized);
    }

    #[rstest]
    fn test_registration() {
        let currency = Currency::new("MYC", 4, 0, "My Currency", CurrencyType::Crypto).unwrap();
        currency_register(currency);
        unsafe {
            assert_eq!(currency_exists(str_to_cstr("MYC")), 1);
        }
    }

    #[rstest]
    fn test_currency_from_py() {
        let code = CString::new("MYC").unwrap();
        let name = CString::new("My Currency").unwrap();
        let currency = unsafe {
            super::currency_from_py(code.as_ptr(), 4, 0, name.as_ptr(), CurrencyType::Crypto)
        };
        assert_eq!(currency.code.as_str(), "MYC");
        assert_eq!(currency.name.as_str(), "My Currency");
        assert_eq!(currency.currency_type, CurrencyType::Crypto);
    }

    #[rstest]
    fn test_currency_to_cstr() {
        let currency = Currency::USD();
        let cstr = unsafe { CStr::from_ptr(currency_to_cstr(&currency)) };
        let expected_output = format!("{:?}", currency);
        assert_eq!(cstr.to_str().unwrap(), expected_output);
    }

    #[rstest]
    fn test_currency_code_to_cstr() {
        let currency = Currency::USD();
        let cstr = unsafe { CStr::from_ptr(currency_code_to_cstr(&currency)) };
        assert_eq!(cstr.to_str().unwrap(), "USD");
    }

    #[rstest]
    fn test_currency_name_to_cstr() {
        let currency = Currency::USD();
        let cstr = unsafe { CStr::from_ptr(currency_name_to_cstr(&currency)) };
        assert_eq!(cstr.to_str().unwrap(), "United States dollar");
    }

    #[rstest]
    fn test_currency_hash() {
        let currency = Currency::USD();
        let hash = super::currency_hash(&currency);
        assert_eq!(hash, currency.code.precomputed_hash());
    }

    #[rstest]
    fn test_currency_from_cstr() {
        let code = CString::new("USD").unwrap();
        let currency = unsafe { currency_from_cstr(code.as_ptr()) };
        assert_eq!(currency, Currency::USD());
    }

    #[rstest]
    #[should_panic(expected = "`code_ptr` was NULL")]
    fn test_currency_from_py_null_code_ptr() {
        let name = CString::new("My Currency").unwrap();
        let _ = unsafe {
            currency_from_py(std::ptr::null(), 4, 0, name.as_ptr(), CurrencyType::Crypto)
        };
    }

    #[rstest]
    #[should_panic(expected = "`name_ptr` was NULL")]
    fn test_currency_from_py_null_name_ptr() {
        let code = CString::new("MYC").unwrap();
        let _ = unsafe {
            currency_from_py(code.as_ptr(), 4, 0, std::ptr::null(), CurrencyType::Crypto)
        };
    }
}

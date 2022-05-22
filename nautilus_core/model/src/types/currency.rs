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

use crate::enums::CurrencyType;
use nautilus_core::string::{pystr_to_string, string_to_pystr};
use pyo3::ffi;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[repr(C)]
#[derive(Eq, PartialEq, Clone, Hash, Debug)]
pub struct Currency {
    pub code: Box<String>,
    pub precision: u8,
    pub iso4217: u16,
    pub name: Box<String>,
    pub currency_type: CurrencyType,
}

impl Currency {
    pub fn new(
        code: &str,
        precision: u8,
        iso4217: u16,
        name: &str,
        currency_type: CurrencyType,
    ) -> Currency {
        Currency {
            code: Box::new(code.to_string()),
            precision,
            iso4217,
            name: Box::new(name.to_string()),
            currency_type,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a `Currency` from valid Python object pointers and primitives.
///
/// # Safety
/// - `code_ptr` and `name_ptr` must be borrowed from a valid Python UTF-8 `str`(s).
#[no_mangle]
pub unsafe extern "C" fn currency_from_py(
    code_ptr: *mut ffi::PyObject,
    precision: u8,
    iso4217: u16,
    name_ptr: *mut ffi::PyObject,
    currency_type: CurrencyType,
) -> Currency {
    Currency {
        code: Box::from(pystr_to_string(code_ptr)),
        precision,
        iso4217,
        name: Box::from(pystr_to_string(name_ptr)),
        currency_type,
    }
}

#[no_mangle]
pub extern "C" fn currency_free(currency: Currency) {
    drop(currency); // Memory freed here
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn currency_to_pystr(currency: &Currency) -> *mut ffi::PyObject {
    string_to_pystr(format!("{:?}", currency).as_str())
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn currency_code_to_pystr(currency: &Currency) -> *mut ffi::PyObject {
    string_to_pystr(currency.code.as_str())
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn currency_name_to_pystr(currency: &Currency) -> *mut ffi::PyObject {
    string_to_pystr(currency.name.as_str())
}

#[no_mangle]
pub extern "C" fn currency_eq(lhs: &Currency, rhs: &Currency) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn currency_hash(currency: &Currency) -> u64 {
    let mut h = DefaultHasher::new();
    currency.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::enums::CurrencyType;
    use crate::types::currency::Currency;

    #[test]
    fn test_currency_new() {
        let currency = Currency::new("AUD", 8, 036, "Australian dollar", CurrencyType::Fiat);

        assert_eq!(currency, currency);
        assert_eq!(currency.code.as_str(), "AUD");
        assert_eq!(currency.precision, 8);
        assert_eq!(currency.iso4217, 036);
        assert_eq!(currency.name.as_str(), "Australian dollar");
        assert_eq!(currency.currency_type, CurrencyType::Fiat);
    }
}

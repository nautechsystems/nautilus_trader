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

use nautilus_core::{
    correctness,
    string::{cstr_to_string, str_to_cstr},
};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize, Serializer};
use ustr::Ustr;

use crate::{currencies::CURRENCY_MAP, enums::CurrencyType};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq)]
#[pyclass]
pub struct Currency {
    pub code: Ustr,
    pub precision: u8,
    pub iso4217: u16,
    pub name: Ustr,
    pub currency_type: CurrencyType,
}

impl Currency {
    #[must_use]
    pub fn new(
        code: &str,
        precision: u8,
        iso4217: u16,
        name: &str,
        currency_type: CurrencyType,
    ) -> Self {
        correctness::valid_string(code, "`Currency` code");
        correctness::valid_string(name, "`Currency` name");
        correctness::u8_in_range_inclusive(precision, 0, 9, "`Currency` precision");

        Self {
            code: Ustr::from(code),
            precision,
            iso4217,
            name: Ustr::from(name),
            currency_type,
        }
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
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CURRENCY_MAP
            .lock()
            .unwrap()
            .get(s)
            .cloned()
            .ok_or_else(|| format!("Unknown currency: {}", s))
    }
}

impl From<&str> for Currency {
    fn from(input: &str) -> Self {
        input.parse().unwrap_or_else(|err| panic!("{}", err))
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
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a [`Currency`] from pointers and primitives.
///
/// # Safety
///
/// - Assumes `code_ptr` is a valid C string pointer.
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn currency_from_py(
    code_ptr: *const c_char,
    precision: u8,
    iso4217: u16,
    name_ptr: *const c_char,
    currency_type: CurrencyType,
) -> Currency {
    Currency::new(
        CStr::from_ptr(code_ptr)
            .to_str()
            .expect("CStr::from_ptr failed"),
        precision,
        iso4217,
        CStr::from_ptr(name_ptr)
            .to_str()
            .expect("CStr::from_ptr failed"),
        currency_type,
    )
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
    use crate::{enums::CurrencyType, types::currency::Currency};

    #[test]
    fn test_currency_new_for_fiat() {
        let currency = Currency::new("AUD", 2, 36, "Australian dollar", CurrencyType::Fiat);
        assert_eq!(currency, currency);
        assert_eq!(currency.code.as_str(), "AUD");
        assert_eq!(currency.precision, 2);
        assert_eq!(currency.iso4217, 36);
        assert_eq!(currency.name.as_str(), "Australian dollar");
        assert_eq!(currency.currency_type, CurrencyType::Fiat);
    }

    #[test]
    fn test_currency_new_for_crypto() {
        let currency = Currency::new("ETH", 8, 0, "Ether", CurrencyType::Crypto);
        assert_eq!(currency, currency);
        assert_eq!(currency.code.as_str(), "ETH");
        assert_eq!(currency.precision, 8);
        assert_eq!(currency.iso4217, 0);
        assert_eq!(currency.name.as_str(), "Ether");
        assert_eq!(currency.currency_type, CurrencyType::Crypto);
    }
}

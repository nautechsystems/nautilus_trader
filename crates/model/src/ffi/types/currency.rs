// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{ffi::c_char, str::FromStr};

use nautilus_core::{
    MUTEX_POISONED,
    ffi::string::{cstr_as_str, str_to_cstr},
};

use crate::{currencies::CURRENCY_MAP, enums::CurrencyType, types::Currency};

/// Returns a [`Currency`] from pointers and primitives.
///
/// # Safety
///
/// This function assumes:
/// - `code_ptr` is a valid C string pointer.
/// - `name_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currency_from_py(
    code_ptr: *const c_char,
    precision: u8,
    iso4217: u16,
    name_ptr: *const c_char,
    currency_type: CurrencyType,
) -> Currency {
    Currency::new(
        unsafe { cstr_as_str(code_ptr) },
        precision,
        iso4217,
        unsafe { cstr_as_str(name_ptr) },
        currency_type,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn currency_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(format!("{currency:?}").as_str())
}

#[unsafe(no_mangle)]
pub extern "C" fn currency_code_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(&currency.code)
}

#[unsafe(no_mangle)]
pub extern "C" fn currency_name_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(&currency.name)
}

#[unsafe(no_mangle)]
pub extern "C" fn currency_hash(currency: &Currency) -> u64 {
    currency.code.precomputed_hash()
}

/// Registers a currency in the global map for FFI.
///
/// # Panics
///
/// Panics if the internal mutex `CURRENCY_MAP` is poisoned when locking.
#[unsafe(no_mangle)]
pub extern "C" fn currency_register(currency: Currency) {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .insert(currency.code.to_string(), currency);
}

/// Checks whether a currency code exists in the global map for FFI.
///
/// # Panics
///
/// Panics if the internal mutex `CURRENCY_MAP` is poisoned when locking.
///
/// # Safety
///
/// Assumes `code_ptr` is a valid NUL-terminated UTF-8 C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currency_exists(code_ptr: *const c_char) -> u8 {
    let code = unsafe { cstr_as_str(code_ptr) };
    u8::from(
        CURRENCY_MAP
            .lock()
            .expect(MUTEX_POISONED)
            .contains_key(code),
    )
}

/// Converts a C string pointer to a `Currency` for FFI.
///
/// # Panics
///
/// Panics if the provided code string is invalid or not found (`unwrap`).
///
/// # Safety
///
/// Assumes `code_ptr` is a valid NUL-terminated UTF-8 C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currency_from_cstr(code_ptr: *const c_char) -> Currency {
    let code = unsafe { cstr_as_str(code_ptr) };
    Currency::from_str(code).unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};

    use rstest::rstest;

    use super::*;
    use crate::{enums::CurrencyType, types::Currency};

    #[rstest]
    fn test_registration() {
        let currency = Currency::new("MYC", 4, 0, "My Currency", CurrencyType::Crypto);
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
        let expected_output = format!("{currency:?}");
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
}

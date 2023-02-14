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
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use nautilus_core::correctness;
use nautilus_core::string::{cstr_to_string, string_to_cstr};

use crate::enums::CurrencyType;

#[repr(C)]
#[derive(Eq, PartialEq, Clone, Hash, Debug)]
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct Currency {
    pub code: Box<Rc<String>>,
    pub precision: u8,
    pub iso4217: u16,
    pub name: Box<Rc<String>>,
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

        Currency {
            code: Box::new(Rc::new(code.to_string())),
            precision,
            iso4217,
            name: Box::new(Rc::new(name.to_string())),
            currency_type,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a [`Currency`] from pointers and primitives.
///
/// # Safety
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
    Currency {
        code: Box::from(Rc::new(cstr_to_string(code_ptr))),
        precision,
        iso4217,
        name: Box::from(Rc::new(cstr_to_string(name_ptr))),
        currency_type,
    }
}

#[no_mangle]
pub extern "C" fn currency_clone(currency: &Currency) -> Currency {
    currency.clone()
}

#[no_mangle]
pub extern "C" fn currency_free(currency: Currency) {
    drop(currency); // Memory freed here
}

#[no_mangle]
pub extern "C" fn currency_to_cstr(currency: &Currency) -> *const c_char {
    string_to_cstr(format!("{currency:?}").as_str())
}

#[no_mangle]
pub extern "C" fn currency_code_to_cstr(currency: &Currency) -> *const c_char {
    string_to_cstr(&currency.code)
}

#[no_mangle]
pub extern "C" fn currency_name_to_cstr(currency: &Currency) -> *const c_char {
    string_to_cstr(&currency.name)
}

#[no_mangle]
pub extern "C" fn currency_eq(lhs: &Currency, rhs: &Currency) -> u8 {
    u8::from(lhs == rhs)
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
    use crate::types::currency::{currency_eq, Currency};

    #[test]
    fn test_currency_equality() {
        let currency1 = Currency::new("AUD", 2, 36, "Australian dollar", CurrencyType::Fiat);
        let currency2 = Currency::new("AUD", 2, 36, "Australian dollar", CurrencyType::Fiat);
        assert_ne!(currency_eq(&currency1, &currency2), 0);
    }

    #[test]
    fn test_currency_new_for_fiat() {
        let currency = Currency::new("AUD", 2, 36, "Australian dollar", CurrencyType::Fiat);
        assert_ne!(currency_eq(&currency, &currency), 0);
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

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
use std::ffi::{c_char, CStr};
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use nautilus_core::correctness;
use nautilus_core::string::string_to_cstr;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct AccountId {
    pub value: Box<Rc<String>>,
}

impl Display for AccountId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl AccountId {
    #[must_use]
    pub fn new(s: &str) -> Self {
        correctness::valid_string(s, "`AccountId` value");
        correctness::string_contains(s, "-", "`TraderId` value");

        AccountId {
            value: Box::new(Rc::new(s.to_string())),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn account_id_new(ptr: *const c_char) -> AccountId {
    AccountId::new(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[no_mangle]
pub extern "C" fn account_id_clone(account_id: &AccountId) -> AccountId {
    account_id.clone()
}

/// Frees the memory for the given `account_id` by dropping.
#[no_mangle]
pub extern "C" fn account_id_free(account_id: AccountId) {
    drop(account_id); // Memory freed here
}

/// Returns an [`AccountId`] as a C string pointer.
#[no_mangle]
pub extern "C" fn account_id_to_cstr(account_id: &AccountId) -> *const c_char {
    string_to_cstr(&account_id.value)
}

#[no_mangle]
pub extern "C" fn account_id_eq(lhs: &AccountId, rhs: &AccountId) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn account_id_hash(account_id: &AccountId) -> u64 {
    let mut h = DefaultHasher::new();
    account_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::*;

    #[test]
    fn test_account_id_new_invalid_string() {
        let s = "";
        let result = std::panic::catch_unwind(|| AccountId::new(s));
        assert!(result.is_err());
    }

    #[test]
    fn test_account_id_new_missing_hyphen() {
        let s = "123456789";
        let result = std::panic::catch_unwind(|| AccountId::new(s));
        assert!(result.is_err());
    }

    #[test]
    fn test_account_id_new() {
        let s = "IB-U123456789";
        let account_id = AccountId::new(s);
        assert_eq!(account_id.value.as_str(), s);
    }

    #[test]
    fn test_account_id_fmt() {
        let s = "IB-U123456789";
        let account_id = AccountId::new(s);
        let formatted = format!("{account_id}");
        assert_eq!(formatted, s);
    }

    #[test]
    fn test_equality() {
        let id1 = AccountId::new("IB-123456789");
        let id2 = AccountId::new("IB-234567890");
        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = AccountId::new("IB-1234567890");
        assert_eq!(id.to_string(), "IB-1234567890");
    }

    #[test]
    fn test_account_id_free_c() {
        let id = AccountId::new("IB-1234567890");
        account_id_free(id); // No panic
    }

    #[test]
    fn test_account_id_new_c() {
        let s = "IB-U123456789";
        let c_string = CString::new(s).unwrap();
        let ptr = c_string.as_ptr();
        let account_id = unsafe { account_id_new(ptr) };
        assert_eq!(account_id.value.as_ref().as_str(), s);
    }

    #[test]
    fn test_account_id_clone_c() {
        let s = "IB-U123456789";
        let c_string = CString::new(s).unwrap();
        let ptr = c_string.as_ptr();
        let account_id = unsafe { account_id_new(ptr) };
        let cloned_account_id = account_id_clone(&account_id);
        assert_eq!(cloned_account_id.value.as_ref().as_str(), s);
    }

    #[test]
    fn test_account_id_to_cstr_c() {
        let s = "IB-U123456789";
        let c_string = CString::new(s).unwrap();
        let ptr = c_string.as_ptr();
        let account_id = unsafe { account_id_new(ptr) };
        let cstr_ptr = account_id_to_cstr(&account_id);
        let c_str = unsafe { CStr::from_ptr(cstr_ptr) };
        assert_eq!(c_str.to_str().unwrap(), s);
    }

    #[test]
    fn test_account_id_eq_c() {
        let s1 = "IB-U123456789";
        let c_string1 = CString::new(s1).unwrap();
        let ptr1 = c_string1.as_ptr();
        let account_id1 = unsafe { account_id_new(ptr1) };

        let s2 = "IB-U123456789";
        let c_string2 = CString::new(s2).unwrap();
        let ptr2 = c_string2.as_ptr();
        let account_id2 = unsafe { account_id_new(ptr2) };

        let result1 = account_id_eq(&account_id1, &account_id2);

        let s3 = "IB-U993456789";
        let c_string3 = CString::new(s3).unwrap();
        let ptr3 = c_string3.as_ptr();
        let account_id3 = unsafe { account_id_new(ptr3) };

        let result2 = account_id_eq(&account_id1, &account_id3);
        assert_eq!(result1, 1);
        assert_eq!(result2, 0);
    }

    #[test]
    fn test_account_id_hash_c() {
        let s1 = "IB-U123456789";
        let c_string1 = CString::new(s1).unwrap();
        let ptr1 = c_string1.as_ptr();
        let account_id1 = unsafe { account_id_new(ptr1) };

        let s2 = "IB-U123456789";
        let c_string2 = CString::new(s2).unwrap();
        let ptr2 = c_string2.as_ptr();
        let account_id2 = unsafe { account_id_new(ptr2) };

        let hash1 = account_id_hash(&account_id1);
        let hash2 = account_id_hash(&account_id2);

        let s3 = "IB-U987456789";
        let c_string3 = CString::new(s3).unwrap();
        let ptr3 = c_string3.as_ptr();
        let account_id3 = unsafe { account_id_new(ptr3) };

        let hash3 = account_id_hash(&account_id3);
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
}

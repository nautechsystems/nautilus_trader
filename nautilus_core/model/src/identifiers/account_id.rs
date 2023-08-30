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
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use anyhow::Result;
use nautilus_core::correctness::{check_string_contains, check_valid_string};
use pyo3::prelude::*;
use ustr::Ustr;

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[pyclass]
pub struct AccountId {
    pub value: Ustr,
}

impl AccountId {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`accountid` value")?;
        check_string_contains(s, "-", "`traderid` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self {
            value: Ustr::from("SIM-001"),
        }
    }
}

impl Debug for AccountId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for AccountId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for AccountId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn account_id_new(ptr: *const c_char) -> AccountId {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    AccountId::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn account_id_hash(id: &AccountId) -> u64 {
    id.value.precomputed_hash()
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
        let result = AccountId::new(s);
        assert!(result.is_err());
    }

    #[test]
    fn test_account_id_new_missing_hyphen() {
        let s = "123456789";
        let result = AccountId::new(s);
        assert!(result.is_err());
    }

    #[test]
    fn test_account_id_fmt() {
        let s = "IB-U123456789";
        let account_id = AccountId::new(s).unwrap();
        let formatted = format!("{account_id}");
        assert_eq!(formatted, s);
    }

    #[test]
    fn test_string_reprs() {
        let id = AccountId::from("IB-1234567890");
        assert_eq!(id.to_string(), "IB-1234567890");
    }

    #[test]
    fn test_account_id_round_trip() {
        let s = "IB-U123456789";
        let c_string = CString::new(s).unwrap();
        let ptr = c_string.as_ptr();
        let account_id = unsafe { account_id_new(ptr) };
        let char_ptr = account_id.value.as_char_ptr();
        let account_id_2 = unsafe { account_id_new(char_ptr) };
        assert_eq!(account_id, account_id_2);
    }

    #[test]
    fn test_account_id_to_cstr_and_back() {
        let s = "IB-U123456789";
        let c_string = CString::new(s).unwrap();
        let ptr = c_string.as_ptr();
        let account_id = unsafe { account_id_new(ptr) };
        let cstr_ptr = account_id.value.as_char_ptr();
        let c_str = unsafe { CStr::from_ptr(cstr_ptr) };
        assert_eq!(c_str.to_str().unwrap(), s);
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

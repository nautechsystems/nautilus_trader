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

use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::AccountId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn account_id_new(ptr: *const c_char) -> AccountId {
    let value = unsafe { cstr_as_str(ptr) };
    AccountId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn account_id_hash(id: &AccountId) -> u64 {
    id.inner().precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_account_id_round_trip() {
        let s = "IB-U123456789";
        let c_string = CString::new(s).unwrap();
        let ptr = c_string.as_ptr();
        let account_id = unsafe { account_id_new(ptr) };
        let char_ptr = account_id.inner().as_char_ptr();
        let account_id_2 = unsafe { account_id_new(char_ptr) };
        assert_eq!(account_id, account_id_2);
    }

    #[rstest]
    fn test_account_id_to_cstr_and_back() {
        let s = "IB-U123456789";
        let c_string = CString::new(s).unwrap();
        let ptr = c_string.as_ptr();
        let account_id = unsafe { account_id_new(ptr) };
        let cstr_ptr = account_id.inner().as_char_ptr();
        let c_str = unsafe { CStr::from_ptr(cstr_ptr) };
        assert_eq!(c_str.to_str().unwrap(), s);
    }

    #[rstest]
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

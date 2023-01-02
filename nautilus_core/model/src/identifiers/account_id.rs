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
    pub fn new(s: &str) -> AccountId {
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

/// Returns an [AccountId] as a C string pointer.
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
    use super::AccountId;
    use crate::identifiers::account_id::account_id_free;

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
    fn test_account_id_free() {
        let id = AccountId::new("IB-1234567890");

        account_id_free(id); // No panic
    }

    // #[test]
    // fn test_account_id_new() {
    //     prepare_freethreaded_python();
    //     Python::with_gil(|py| {
    //         let pystr = PyString::new(py, "SIM-02851908").into_ptr();
    //
    //         let id = unsafe { account_id_new(pystr) };
    //
    //         assert_eq!(id.to_string(), "SIM-02851908")
    //     });
    // }
}

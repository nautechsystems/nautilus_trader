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
pub struct ClientId {
    pub value: Box<Rc<String>>,
}

impl Display for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl ClientId {
    pub fn new(s: &str) -> ClientId {
        correctness::valid_string(s, "`ClientId` value");

        ClientId {
            value: Box::new(Rc::new(s.to_string())),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from C string pointer.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn client_id_new(ptr: *const c_char) -> ClientId {
    ClientId::new(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[no_mangle]
pub extern "C" fn client_id_clone(client_id: &ClientId) -> ClientId {
    client_id.clone()
}

/// Frees the memory for the given `client_id` by dropping.
#[no_mangle]
pub extern "C" fn client_id_free(client_id: ClientId) {
    drop(client_id); // Memory freed here
}

/// Returns a [ClientId] identifier as a C string pointer.
#[no_mangle]
pub extern "C" fn client_id_to_cstr(client_id: &ClientId) -> *const c_char {
    string_to_cstr(client_id.value.as_str())
}

#[no_mangle]
pub extern "C" fn client_id_eq(lhs: &ClientId, rhs: &ClientId) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn client_id_hash(client_id: &ClientId) -> u64 {
    let mut h = DefaultHasher::new();
    client_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::ClientId;
    use crate::identifiers::client_id::client_id_free;

    #[test]
    fn test_equality() {
        let id1 = ClientId::new("BINANCE");
        let id2 = ClientId::new("DYDX");

        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = ClientId::new("BINANCE");

        assert_eq!(id.to_string(), "BINANCE");
        assert_eq!(format!("{id}"), "BINANCE");
    }

    #[test]
    fn test_client_id_free() {
        let id = ClientId::new("BINANCE");

        client_id_free(id); // No panic
    }

    // #[test]
    // fn test_client_id_new() {
    //     prepare_freethreaded_python();
    //     Python::with_gil(|py| {
    //         let pystr = PyString::new(py, "BINANCE").into_ptr();
    //         let identifier = unsafe { client_id_new(pystr) };
    //
    //         assert_eq!(identifier.to_string(), "BINANCE")
    //     });
    // }
}

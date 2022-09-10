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
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};

use pyo3::ffi;

use nautilus_core::correctness;
use nautilus_core::string::{pystr_to_string, string_to_pystr};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct OrderListId {
    value: Box<String>,
}

impl From<&str> for OrderListId {
    fn from(s: &str) -> OrderListId {
        OrderListId {
            value: Box::new(s.to_string()),
        }
    }
}

impl Display for OrderListId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl OrderListId {
    pub fn new(s: &str) -> OrderListId {
        correctness::valid_string(s, "`OrderListId` value");

        OrderListId {
            value: Box::new(s.to_string()),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

/// Returns a Nautilus identifier from a valid Python object pointer.
///
/// # Safety
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn order_list_id_new(ptr: *mut ffi::PyObject) -> OrderListId {
    OrderListId::new(pystr_to_string(ptr).as_str())
}

/// Frees the memory for the given `order_list_id` by dropping.
#[no_mangle]
pub extern "C" fn order_list_id_free(order_list_id: OrderListId) {
    drop(order_list_id); // Memory freed here
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn order_list_id_to_pystr(order_list_id: &OrderListId) -> *mut ffi::PyObject {
    string_to_pystr(order_list_id.value.as_str())
}

#[no_mangle]
pub extern "C" fn order_list_id_eq(lhs: &OrderListId, rhs: &OrderListId) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn order_list_id_hash(order_list_id: &OrderListId) -> u64 {
    let mut h = DefaultHasher::new();
    order_list_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::OrderListId;
    use crate::identifiers::order_list_id::order_list_id_free;

    #[test]
    fn test_equality() {
        let id1 = OrderListId::new("001");
        let id2 = OrderListId::new("002");

        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = OrderListId::new("001");

        assert_eq!(id.to_string(), "001");
        assert_eq!(format!("{id}"), "001");
    }

    #[test]
    fn test_order_list_id_free() {
        let id = OrderListId::new("001");

        order_list_id_free(id); // No panic
    }
}

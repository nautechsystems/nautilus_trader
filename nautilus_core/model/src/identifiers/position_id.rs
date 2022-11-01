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
use std::rc::Rc;

use pyo3::ffi;

use nautilus_core::correctness;
use nautilus_core::string::{pystr_to_string, string_to_pystr};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct PositionId {
    value: Box<Rc<String>>,
}

impl Display for PositionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl PositionId {
    pub fn new(s: &str) -> PositionId {
        correctness::valid_string(s, "`PositionId` value");

        PositionId {
            value: Box::new(Rc::new(s.to_string())),
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
pub unsafe extern "C" fn position_id_new(ptr: *mut ffi::PyObject) -> PositionId {
    PositionId::new(pystr_to_string(ptr).as_str())
}

#[no_mangle]
pub extern "C" fn position_id_copy(position_id: &PositionId) -> PositionId {
    position_id.clone()
}

/// Frees the memory for the given `position_id` by dropping.
#[no_mangle]
pub extern "C" fn position_id_free(position_id: PositionId) {
    drop(position_id); // Memory freed here
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn position_id_to_pystr(position_id: &PositionId) -> *mut ffi::PyObject {
    string_to_pystr(position_id.value.as_str())
}

#[no_mangle]
pub extern "C" fn position_id_eq(lhs: &PositionId, rhs: &PositionId) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn position_id_hash(position_id: &PositionId) -> u64 {
    let mut h = DefaultHasher::new();
    position_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::PositionId;
    use crate::identifiers::position_id::position_id_free;

    #[test]
    fn test_equality() {
        let id1 = PositionId::new("P-123456789");
        let id2 = PositionId::new("P-234567890");

        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = PositionId::new("P-123456789");

        assert_eq!(id.to_string(), "P-123456789");
        assert_eq!(format!("{id}"), "P-123456789");
    }

    #[test]
    fn test_position_id_free() {
        let id = PositionId::new("001");

        position_id_free(id); // No panic
    }
}

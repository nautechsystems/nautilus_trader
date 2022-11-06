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
pub struct ExecAlgorithmId {
    value: Box<Rc<String>>,
}

impl Display for ExecAlgorithmId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl ExecAlgorithmId {
    pub fn new(s: &str) -> ExecAlgorithmId {
        correctness::valid_string(s, "`ExecAlgorithmId` value");

        ExecAlgorithmId {
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
pub unsafe extern "C" fn exec_algorithm_id_new(ptr: *mut ffi::PyObject) -> ExecAlgorithmId {
    ExecAlgorithmId::new(pystr_to_string(ptr).as_str())
}

#[no_mangle]
pub extern "C" fn exec_algorithm_id_copy(exec_algorithm_id: &ExecAlgorithmId) -> ExecAlgorithmId {
    exec_algorithm_id.clone()
}

/// Frees the memory for the given `exec_algorithm_id` by dropping.
#[no_mangle]
pub extern "C" fn exec_algorithm_id_free(exec_algorithm_id: ExecAlgorithmId) {
    drop(exec_algorithm_id); // Memory freed here
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn exec_algorithm_id_to_pystr(
    exec_algorithm_id: &ExecAlgorithmId,
) -> *mut ffi::PyObject {
    string_to_pystr(exec_algorithm_id.value.as_str())
}

#[no_mangle]
pub extern "C" fn exec_algorithm_id_eq(lhs: &ExecAlgorithmId, rhs: &ExecAlgorithmId) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn exec_algorithm_id_hash(exec_algorithm_id: &ExecAlgorithmId) -> u64 {
    let mut h = DefaultHasher::new();
    exec_algorithm_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::ExecAlgorithmId;
    use crate::identifiers::exec_algorithm_id::exec_algorithm_id_free;

    #[test]
    fn test_equality() {
        let id1 = ExecAlgorithmId::new("VWAP");
        let id2 = ExecAlgorithmId::new("TWAP");

        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = ExecAlgorithmId::new("001");

        assert_eq!(id.to_string(), "001");
        assert_eq!(format!("{id}"), "001");
    }

    #[test]
    fn test_exec_algorithm_id_free() {
        let id = ExecAlgorithmId::new("001");

        exec_algorithm_free(id); // No panic
    }
}

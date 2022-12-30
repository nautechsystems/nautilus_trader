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
use std::fmt::{Debug, Display, Formatter, Result};
use std::rc::Rc;

use pyo3::ffi;

use nautilus_core::correctness;
use nautilus_core::string::pystr_to_string;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct TraderId {
    pub value: Box<Rc<String>>,
}

impl Display for TraderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl TraderId {
    pub fn new(s: &str) -> TraderId {
        correctness::valid_string(s, "`TraderId` value");
        correctness::string_contains(s, "-", "`TraderId` value");

        TraderId {
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
pub unsafe extern "C" fn trader_id_new(ptr: *mut ffi::PyObject) -> TraderId {
    TraderId::new(pystr_to_string(ptr).as_str())
}

#[no_mangle]
pub extern "C" fn trader_id_clone(trader_id: &TraderId) -> TraderId {
    trader_id.clone()
}

/// Frees the memory for the given `trader_id` by dropping.
#[no_mangle]
pub extern "C" fn trader_id_free(trader_id: TraderId) {
    drop(trader_id); // Memory freed here
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::TraderId;
    use crate::identifiers::trader_id::trader_id_free;

    #[test]
    fn test_equality() {
        let trader_id1 = TraderId::new("TRADER-001");
        let trader_id2 = TraderId::new("TRADER-002");

        assert_eq!(trader_id1, trader_id1);
        assert_ne!(trader_id1, trader_id2);
    }

    #[test]
    fn test_string_reprs() {
        let trader_id = TraderId::new("TRADER-001");

        assert_eq!(trader_id.to_string(), "TRADER-001");
        assert_eq!(format!("{trader_id}"), "TRADER-001");
    }

    #[test]
    fn test_trader_id_free() {
        let id = TraderId::new("TRADER-001");

        trader_id_free(id); // No panic
    }
}

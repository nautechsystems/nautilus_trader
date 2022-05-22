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

use nautilus_core::string::{pystr_to_string, string_to_pystr};
use pyo3::ffi;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct TradeId {
    value: Box<String>,
}

impl From<&str> for TradeId {
    fn from(s: &str) -> TradeId {
        TradeId {
            value: Box::new(s.to_string()),
        }
    }
}

impl Display for TradeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn trade_id_free(trade_id: TradeId) {
    drop(trade_id); // Memory freed here
}

/// Returns a Nautilus identifier from a valid Python object pointer.
///
/// # Safety
/// - `ptr` must be borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn trade_id_from_pystr(ptr: *mut ffi::PyObject) -> TradeId {
    TradeId {
        value: Box::new(pystr_to_string(ptr)),
    }
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn trade_id_to_pystr(trade_id: &TradeId) -> *mut ffi::PyObject {
    string_to_pystr(trade_id.value.as_str())
}

#[no_mangle]
pub extern "C" fn trade_id_eq(lhs: &TradeId, rhs: &TradeId) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn trade_id_hash(trade_id: &TradeId) -> u64 {
    let mut h = DefaultHasher::new();
    trade_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::TradeId;
    use crate::identifiers::trade_id::trade_id_free;

    #[test]
    fn test_equality() {
        let trade_id1 = TradeId::from("123456789");
        let trade_id2 = TradeId::from("234567890");

        assert_eq!(trade_id1, trade_id1);
        assert_ne!(trade_id1, trade_id2);
    }

    #[test]
    fn test_string_reprs() {
        let trade_id = TradeId::from("1234567890");

        assert_eq!(trade_id.to_string(), "1234567890");
        assert_eq!(format!("{trade_id}"), "1234567890");
    }

    #[test]
    fn test_trade_id_free() {
        let id = TradeId::from("123456789");

        trade_id_free(id); // No panic
    }
}

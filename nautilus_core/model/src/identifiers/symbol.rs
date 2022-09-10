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
pub struct Symbol {
    value: Box<String>,
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl Symbol {
    pub fn new(s: &str) -> Symbol {
        correctness::valid_string(s, "`Symbol` value");

        Symbol {
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
pub unsafe extern "C" fn symbol_new(ptr: *mut ffi::PyObject) -> Symbol {
    Symbol::new(pystr_to_string(ptr).as_str())
}

/// Frees the memory for the given `symbol` by dropping.
#[no_mangle]
pub extern "C" fn symbol_free(symbol: Symbol) {
    drop(symbol); // Memory freed here
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn symbol_to_pystr(symbol: &Symbol) -> *mut ffi::PyObject {
    string_to_pystr(symbol.value.as_str())
}

#[no_mangle]
pub extern "C" fn symbol_eq(lhs: &Symbol, rhs: &Symbol) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn symbol_hash(symbol: &Symbol) -> u64 {
    let mut h = DefaultHasher::new();
    symbol.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::Symbol;
    use crate::identifiers::symbol::symbol_free;

    #[test]
    fn test_equality() {
        let symbol1 = Symbol::new("XRD/USD");
        let symbol2 = Symbol::new("BTC/USD");

        assert_eq!(symbol1, symbol1);
        assert_ne!(symbol1, symbol2);
    }

    #[test]
    fn test_string_reprs() {
        let symbol = Symbol::new("ETH-PERP");

        assert_eq!(symbol.to_string(), "ETH-PERP");
        assert_eq!(format!("{symbol}"), "ETH-PERP");
    }

    #[test]
    fn test_symbol_free() {
        let id = Symbol::new("ETH-PERP");

        symbol_free(id); // No panic
    }
}

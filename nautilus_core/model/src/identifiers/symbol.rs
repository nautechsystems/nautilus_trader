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
    collections::hash_map::DefaultHasher,
    ffi::{c_char, CStr},
    fmt::{Debug, Display, Formatter},
    hash::{Hash, Hasher},
    sync::Arc,
};

use nautilus_core::{correctness, string::str_to_cstr};
use pyo3::prelude::*;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq)]
#[pyclass]
pub struct Symbol {
    pub value: Box<Arc<String>>,
}

impl Debug for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl Default for Symbol {
    fn default() -> Self {
        Self {
            value: Box::new(Arc::new(String::from("AUD/USD"))),
        }
    }
}

impl Symbol {
    #[must_use]
    pub fn new(s: &str) -> Self {
        correctness::valid_string(s, "`Symbol` value");

        Self {
            value: Box::new(Arc::new(s.to_string())),
        }
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
#[no_mangle]
pub unsafe extern "C" fn symbol_new(ptr: *const c_char) -> Symbol {
    Symbol::new(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[no_mangle]
pub extern "C" fn symbol_clone(symbol: &Symbol) -> Symbol {
    symbol.clone()
}

/// Frees the memory for the given [Symbol] by dropping.
#[no_mangle]
pub extern "C" fn symbol_drop(symbol: Symbol) {
    drop(symbol); // Memory freed here
}

/// Returns a [`Symbol`] as a C string pointer.
#[no_mangle]
pub extern "C" fn symbol_to_cstr(symbol: &Symbol) -> *const c_char {
    str_to_cstr(&symbol.value)
}

#[no_mangle]
pub extern "C" fn symbol_eq(lhs: &Symbol, rhs: &Symbol) -> u8 {
    u8::from(lhs == rhs)
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
    use crate::identifiers::symbol::symbol_drop;

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
    fn test_symbol_drop() {
        let id = Symbol::new("ETH-PERP");
        symbol_drop(id); // No panic
    }
}

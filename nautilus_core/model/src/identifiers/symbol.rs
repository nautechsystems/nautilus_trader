// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

#[repr(C)]
#[derive(Clone, Hash, PartialEq)]
pub struct Symbol {
    pub value: Box<String>,
}

impl Symbol {
    pub fn from(s: &str) -> Symbol {
        Symbol {
            value: Box::from(s.to_owned()),
        }
    }

    //##########################################################################
    // C API
    //##########################################################################
    #[no_mangle]
    pub unsafe extern "C" fn symbol_new(ptr: *mut u8, length: usize) -> Symbol {
        // SAFETY: Checks ptr is a valid UTF-8 string
        let vec = Vec::from_raw_parts(ptr, length, length);
        let s = String::from_utf8(vec).expect("Invalid UTF-8 string");
        Symbol {
            value: Box::from(s),
        }
    }

    #[no_mangle]
    pub extern "C" fn symbol_free(s: Symbol) {
        drop(s); // Memory freed here
    }

    #[no_mangle]
    pub extern "C" fn symbol_len(s: Symbol) -> usize {
        s.value.len()
    }

    #[no_mangle]
    pub extern "C" fn symbol_as_utf8(&self) -> *const u8 {
        self.value.as_ptr()
    }
}

impl Debug for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use crate::identifiers::symbol::Symbol;

    #[test]
    fn symbol_from_str() {
        let symbol1 = Symbol::from("XRD/USD");
        let symbol2 = Symbol::from("BTC/USD");

        assert_eq!(symbol1, symbol1);
        assert_ne!(symbol1, symbol2);
        assert_eq!(symbol1.to_string(), "XRD/USD")
    }
}

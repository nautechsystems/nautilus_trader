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

use std::ffi::CString;
use std::os::raw::c_char;

#[repr(C)]
#[derive(Copy, Clone, Hash, PartialEq, Debug)]
pub struct Symbol {
    value: *mut c_char,
    pub len: u8,
}

impl Symbol {
    pub fn from_str(value: &str) -> Symbol {
        Symbol {
            value: CString::new(value).unwrap().into_raw(),
            len: value.len() as u8,
        }
    }

    pub unsafe fn from_raw(value: *mut c_char) -> Symbol {
        // Here we always check `value` can be parsed into a valid C string
        let s = CString::from_raw(value)
            .into_string()
            .expect("Cannot parse `value` Symbol");
        Symbol {
            value,
            len: s.len() as u8,
        }
    }

    pub unsafe fn to_string(self) -> String {
        String::from_raw_parts(self.value as *mut u8, self.len as usize, self.len as usize)
    }

    pub unsafe fn to_cstring(self) -> CString {
        CString::from_raw(self.value)
    }

    #[no_mangle]
    pub unsafe extern "C" fn new_symbol(value: *mut c_char) -> Symbol {
        Symbol::from_raw(value)
    }
}

#[cfg(test)]
mod tests {
    use crate::identifiers::symbol::Symbol;

    #[test]
    fn symbol_from_str() {
        let symbol1 = Symbol::from_str("XRD/USD");
        let symbol2 = Symbol::from_str("BTC/USD");

        assert_eq!(symbol1, symbol1);
        assert_ne!(symbol1, symbol2);
        assert_eq!(symbol1.len, 7);
        unsafe { assert_eq!(symbol1.to_string(), "XRD/USD") }
    }
}

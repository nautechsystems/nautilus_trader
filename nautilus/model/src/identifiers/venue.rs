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

use std::ffi::{CStr, CString};
use std::fmt::{Debug, Display, Formatter, Result};
use std::os::raw::c_char;

#[repr(C)]
#[derive(Clone, Hash, PartialEq)]
pub struct Venue {
    pub value: Box<String>,
}

impl Venue {
    pub fn from_str(s: &str) -> Venue {
        Venue {
            value: Box::new(s.to_string()),
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn venue_new(ptr: *const c_char) -> Venue {
        // SAFETY: checks `ptr` can be parsed into a valid C string
        let s = CStr::from_ptr(ptr);
        Venue {
            value: Box::new(s.to_str().expect("invalid UTF-8 string").to_string()),
        }
    }

    #[no_mangle]
    pub extern "C" fn venue_as_bytes(self) -> *const c_char {
        CString::new(self.value.to_string()).unwrap().into_raw()
    }
}

impl Debug for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl Display for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use crate::identifiers::venue::Venue;

    #[test]
    fn venue_from_str() {
        let venue1 = Venue::from_str("XRD/USD");
        let venue2 = Venue::from_str("BTC/USD");

        assert_eq!(venue1, venue1);
        assert_ne!(venue1, venue2);
        assert_eq!(venue1.value.len(), 7);
        assert_eq!(venue1.to_string(), "XRD/USD")
    }
}

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
pub struct Venue {
    pub value: Box<String>,
}

impl Venue {
    pub fn from(s: &str) -> Venue {
        Venue {
            value: Box::from(s.to_owned()),
        }
    }

    //##########################################################################
    // C API
    //##########################################################################
    #[no_mangle]
    pub unsafe extern "C" fn venue_new(ptr: *mut u8, length: usize) -> Venue {
        // SAFETY: Checks ptr is a valid UTF-8 string
        let vec = Vec::from_raw_parts(ptr, length, length);
        let s = String::from_utf8(vec).expect("invalid UTF-8 string");
        Venue {
            value: Box::from(s.to_string()),
        }
    }

    #[no_mangle]
    pub extern "C" fn venue_free(v: Venue) {
        drop(v); // Memory freed here
    }

    #[no_mangle]
    pub extern "C" fn venue_len(v: Venue) -> usize {
        v.value.len()
    }

    #[no_mangle]
    pub extern "C" fn venue_as_utf8(&self) -> *const u8 {
        self.value.as_ptr()
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
        let venue1 = Venue::from("XRD/USD");
        let venue2 = Venue::from("BTC/USD");

        assert_eq!(venue1, venue1);
        assert_ne!(venue1, venue2);
        assert_eq!(venue1.value.len(), 7);
        assert_eq!(venue1.to_string(), "XRD/USD")
    }
}

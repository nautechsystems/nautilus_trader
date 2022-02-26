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

use crate::string::into_cstring;
use std::ffi::CStr;
use std::fmt::{Debug, Display, Formatter, Result};
use std::os::raw::c_char;
use uuid::Uuid;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct UUID4 {
    value: Box<String>,
}

impl UUID4 {
    pub fn new() -> UUID4 {
        UUID4 {
            value: Box::new(Uuid::new_v4().to_string()),
        }
    }

    pub fn from_str(s: &str) -> UUID4 {
        UUID4 {
            value: Box::new(
                Uuid::parse_str(s)
                    .expect("Invalid UUID4 string")
                    .to_string(),
            ),
        }
    }
}

impl Default for UUID4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn uuid4_new() -> UUID4 {
    UUID4::new()
}

#[no_mangle]
pub extern "C" fn uuid4_free(uuid4: UUID4) {
    drop(uuid4); // Memory freed here
}

/// Expects `ptr` to be an array of valid UTF-8 chars with a null byte terminator.
#[no_mangle]
pub unsafe extern "C" fn uuid4_from_cstring(ptr: *const c_char) -> UUID4 {
    // SAFETY: Wraps and checks raw C string `ptr`, then converts to owned `String`
    let s = CStr::from_ptr(ptr).to_str().expect("invalid C string");
    UUID4::from_str(s)
}

#[no_mangle]
pub extern "C" fn uuid4_to_cstring(uuid: &UUID4) -> *const c_char {
    into_cstring(uuid.to_string())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::uuid::{uuid4_from_cstring, uuid4_new, uuid4_to_cstring, UUID4};
    use std::ffi::{CStr, CString};

    #[test]
    fn test_new() {
        let uuid = UUID4::new();

        assert_eq!(uuid.to_string().len(), 36)
    }

    #[test]
    fn test_from_str() {
        let uuid = UUID4::from_str("2d89666b-1a1e-4a75-b193-4eb3b454c757");

        assert_eq!(uuid.to_string().len(), 36);
        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
    }

    #[test]
    fn test_uuid4_new() {
        let uuid = uuid4_new();

        assert_eq!(uuid.to_string().len(), 36)
    }

    #[test]
    fn test_uuid4_from_cstring() {
        unsafe {
            let cstring = CString::new("2d89666b-1a1e-4a75-b193-4eb3b454c757").unwrap();
            let uuid = uuid4_from_cstring(cstring.as_ptr());

            assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757")
        }
    }

    #[test]
    fn test_uuid4_to_cstring() {
        unsafe {
            let uuid = UUID4::new();
            let ptr = uuid4_to_cstring(&uuid);

            assert_eq!(CStr::from_ptr(ptr).to_str().unwrap().len(), 36)
        }
    }
}

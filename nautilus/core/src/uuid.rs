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
use uuid::Uuid;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct UUID4 {
    pub value: Box<String>,
}

impl UUID4 {
    pub fn new() -> UUID4 {
        UUID4 {
            value: Box::new(Uuid::new_v4().to_string()),
        }
    }

    pub fn from(s: &str) -> UUID4 {
        UUID4 {
            value: Box::new(
                Uuid::parse_str(s)
                    .expect("Invalid UUID4 string")
                    .to_string(),
            ),
        }
    }

    //##########################################################################
    // C API
    //##########################################################################
    #[no_mangle]
    pub extern "C" fn uuid4_new() -> UUID4 {
        UUID4::new()
    }

    #[no_mangle]
    pub unsafe extern "C" fn uuid4_from_raw(ptr: *const c_char) -> UUID4 {
        // SAFETY: Wraps and checks raw C string `ptr`, then converts to owned `String`
        UUID4::from(CStr::from_ptr(ptr).to_str().expect("invalid C string"))
    }

    #[no_mangle]
    pub extern "C" fn uuid4_to_raw(&self) -> *const c_char {
        let bytes = self.value.to_string().into_bytes();
        CString::new(bytes).expect("CString::new failed").into_raw()
    }

    #[no_mangle]
    pub unsafe extern "C" fn uuid4_free_raw(ptr: *mut c_char) {
        // SAFETY: Retakes ownership of C string `ptr`, then drops
        drop(CString::from_raw(ptr));
    }

    #[no_mangle]
    pub extern "C" fn uuid4_free(uuid: UUID4) {
        drop(uuid); // Memory freed here
    }
}

impl Debug for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_string())
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::uuid::UUID4;
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    #[test]
    fn test_new() {
        let uuid = UUID4::new();

        assert_eq!(uuid.to_string().len(), 36)
    }

    #[test]
    fn test_from_str() {
        let uuid = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");

        assert_eq!(uuid.to_string().len(), 36);
        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
    }
    //##########################################################################
    // C API tests
    //##########################################################################
    #[test]
    fn test_uuid4_new() {
        let uuid = UUID4::uuid4_new();

        assert_eq!(uuid.to_string().len(), 36)
    }

    #[test]
    fn test_uuid4_from_raw() {
        unsafe {
            let cstring = CString::new("2d89666b-1a1e-4a75-b193-4eb3b454c757").unwrap();
            let uuid = UUID4::uuid4_from_raw(cstring.as_ptr());

            assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757")
        }
    }

    #[test]
    fn test_uuid4_to_raw() {
        unsafe {
            let uuid = UUID4::new();
            let ptr = UUID4::uuid4_to_raw(&uuid);

            assert_eq!(CStr::from_ptr(ptr).to_str().unwrap().len(), 36)
        }
    }

    #[test]
    fn test_uuid4_free_raw() {
        unsafe {
            let uuid = UUID4::new();
            let ptr = UUID4::uuid4_to_raw(&uuid);
            UUID4::uuid4_free_raw(ptr as *mut c_char);
        }
    }
}

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
        // SAFETY: Checks ptr is a valid null terminated C string
        UUID4::from(CStr::from_ptr(ptr).to_str().expect("invalid C string"))
    }

    #[no_mangle]
    pub extern "C" fn uuid4_to_raw(&self) -> *const c_char {
        let bytes = self.value.to_string().into_bytes();
        CString::new(bytes).expect("CString::new failed").into_raw()
    }

    #[no_mangle]
    pub unsafe extern "C" fn uuid4_free_raw(ptr: *mut c_char) {
        // SAFETY: Checks ptr is a valid null terminated C string
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
    use crate::uuid;

    #[test]
    fn new_produces_correct_length_bytes() {
        let uuid = uuid::UUID4::new();

        println!("{}", uuid.to_string())
    }

    #[test]
    fn new_to_string() {
        let uuid = uuid::UUID4::new();

        println!("{}", uuid.to_string())
    }

    #[test]
    fn from_str() {
        let uuid = uuid::UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");

        assert_eq!(uuid.to_string().len(), 36);
        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
    }
}

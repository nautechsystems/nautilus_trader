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

use std::convert::TryFrom;
use std::ffi::CStr;
use std::fmt::{Debug, Display, Formatter, Result};
use std::os::raw::c_char;
use uuid::Uuid;

#[repr(C)]
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct UUID4 {
    value: [u8; 36], // UTF-8 encoded bytes
}

impl UUID4 {
    pub fn new() -> UUID4 {
        UUID4 {
            value: <[u8; 36]>::try_from(Uuid::new_v4().to_string().as_bytes()).unwrap(),
        }
    }

    pub fn from_str(s: &str) -> UUID4 {
        UUID4 {
            value: <[u8; 36]>::try_from(
                Uuid::parse_str(s)
                    .expect("Invalid `value` not UUID4 specification.")
                    .to_string()
                    .as_bytes(),
            ) // Uuid::parse_str now guarantees a valid UUID
            .unwrap(),
        }
    }

    pub fn to_string(&self) -> String {
        // self.value expected to be valid
        String::from_utf8(Vec::from(self.value)).unwrap()
    }

    /// Initializes a new instance of the UUID4 struct.
    #[no_mangle]
    pub extern "C" fn uuid_new() -> UUID4 {
        UUID4::new()
    }

    /// Initializes a new instance of the UUID4 struct.
    #[no_mangle]
    pub unsafe extern "C" fn uuid_from_raw(ptr: *const c_char) -> UUID4 {
        let s = CStr::from_ptr(ptr);
        UUID4::from_str(s.to_str().expect("Not a valid UTF-8 string"))
    }

    /// Returns a UTF-8 encoded bytes representation of the UUID value.
    #[no_mangle]
    pub extern "C" fn uuid_to_bytes(&self) -> &[u8; 36] {
        &self.value
    }
}

impl Debug for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.to_string())
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::uuid;

    #[test]
    fn new_produces_correct_length_bytes() {
        let uuid = uuid::UUID4::new();

        assert_eq!(uuid.value.len(), 36);
        println!("{}", uuid.to_string())
    }

    #[test]
    fn new_to_string() {
        let uuid = uuid::UUID4::new();

        assert_eq!(uuid.to_string().len(), 36);
        println!("{}", uuid.to_string())
    }

    #[test]
    fn from_str() {
        let uuid = uuid::UUID4::from_str("2d89666b-1a1e-4a75-b193-4eb3b454c757");

        assert_eq!(uuid.to_string().len(), 36);
        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
    }

    #[test]
    fn uuid_to_bytes() {
        let uuid = uuid::UUID4::new();

        assert_eq!(uuid.uuid_to_bytes().len(), 36);
    }
}

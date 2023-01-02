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

use std::collections::hash_map::DefaultHasher;
use std::ffi::{c_char, CStr};
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::string::string_to_cstr;
use uuid::Uuid;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct UUID4 {
    pub value: Box<Rc<String>>,
}

impl UUID4 {
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        UUID4 {
            value: Box::new(Rc::new(uuid.to_string())),
        }
    }
}

impl From<&str> for UUID4 {
    fn from(s: &str) -> Self {
        let uuid = Uuid::try_parse(s).expect("invalid UUID string");
        Self {
            value: Box::new(Rc::new(uuid.to_string())),
        }
    }
}

impl Default for UUID4 {
    fn default() -> Self {
        Self::new()
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
pub extern "C" fn uuid4_clone(uuid4: &UUID4) -> UUID4 {
    uuid4.clone()
}

#[no_mangle]
pub extern "C" fn uuid4_free(uuid4: UUID4) {
    drop(uuid4); // Memory freed here
}

/// Drops the string from a C string pointer.
///
/// # Safety
/// - Assumes `ptr` is a valid C String pointer.
/// # Panics
/// - If `ptr` is null.
#[no_mangle]
pub unsafe extern "C" fn uuid4_from_cstr(ptr: *const c_char) -> UUID4 {
    UUID4::from(CStr::from_ptr(ptr).to_str().unwrap_or_else(|_|panic!("CStr::from_ptr failed")))
}

#[no_mangle]
pub extern "C" fn uuid4_to_cstr(uuid: &UUID4) -> *const c_char {
    string_to_cstr(&uuid.value)
}

#[no_mangle]
pub extern "C" fn uuid4_eq(lhs: &UUID4, rhs: &UUID4) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn uuid4_hash(uuid: &UUID4) -> u64 {
    let mut h = DefaultHasher::new();
    uuid.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::uuid::{uuid4_free, uuid4_new, UUID4};

    #[test]
    fn test_equality() {
        let uuid1 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid2 = UUID4::from("46922ecb-4324-4e40-a56c-841e0d774cef");

        assert_eq!(uuid1, uuid1);
        assert_ne!(uuid1, uuid2);
    }

    #[test]
    fn test_string_reprs() {
        let uuid = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");

        assert_eq!(uuid.to_string().len(), 36);
        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
        assert_eq!(format!("{uuid}"), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
    }

    #[test]
    fn test_uuid4_new() {
        let uuid = uuid4_new();

        println!("{uuid}");
        assert_eq!(uuid.to_string().len(), 36);
    }

    #[test]
    fn test_uuid4_free() {
        let uuid = uuid4_new();

        uuid4_free(uuid); // No panic
    }

    // #[test]
    // fn test_uuid4_from_pystr() {
    //     prepare_freethreaded_python();
    //     Python::with_gil(|py| {
    //         let pystr = PyString::new(py, "2d89666b-1a1e-4a75-b193-4eb3b454c757").into_ptr();
    //
    //         let uuid = unsafe { uuid4_from_pystr(pystr) };
    //
    //         assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
    //     });
    // }
}

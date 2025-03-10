// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    ffi::{CStr, c_char},
    hash::{Hash, Hasher},
};

use crate::UUID4;

#[unsafe(no_mangle)]
pub extern "C" fn uuid4_new() -> UUID4 {
    UUID4::new()
}

/// Returns a [`UUID4`] from C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// This function panics:
/// - If `ptr` cannot be cast to a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uuid4_from_cstr(ptr: *const c_char) -> UUID4 {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    let cstr = unsafe { CStr::from_ptr(ptr) };
    let value = cstr.to_str().expect("Failed to convert C string to UTF-8");
    UUID4::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn uuid4_to_cstr(uuid: &UUID4) -> *const c_char {
    uuid.to_cstr().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn uuid4_eq(lhs: &UUID4, rhs: &UUID4) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
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
    use std::ffi::CString;

    use rstest::*;
    use uuid::{self, Uuid};

    use super::*;

    #[rstest]
    fn test_new() {
        let uuid = uuid4_new();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
    }

    #[rstest]
    fn test_from_cstr() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid_cstring = CString::new(uuid_string).expect("CString::new failed");
        let uuid_ptr = uuid_cstring.as_ptr();
        let uuid = unsafe { uuid4_from_cstr(uuid_ptr) };
        assert_eq!(uuid_string, uuid.to_string());
    }

    #[rstest]
    fn test_to_cstr() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid = UUID4::from(uuid_string);
        let uuid_ptr = uuid4_to_cstr(&uuid);
        let uuid_cstr = unsafe { CStr::from_ptr(uuid_ptr) };
        let uuid_result_string = uuid_cstr.to_str().expect("CStr::to_str failed").to_string();
        assert_eq!(uuid_string, uuid_result_string);
    }

    #[rstest]
    fn test_eq() {
        let uuid1 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid2 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid3 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c758");
        assert_eq!(uuid4_eq(&uuid1, &uuid2), 1);
        assert_eq!(uuid4_eq(&uuid1, &uuid3), 0);
    }

    #[rstest]
    fn test_hash() {
        let uuid1 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid2 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid3 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c758");
        assert_eq!(uuid4_hash(&uuid1), uuid4_hash(&uuid2));
        assert_ne!(uuid4_hash(&uuid1), uuid4_hash(&uuid3));
    }
}

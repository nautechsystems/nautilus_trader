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

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

pub fn into_cstring(s: String) -> *const c_char {
    CString::new(s.into_bytes())
        .expect("CString::new failed")
        .into_raw()
}

pub unsafe fn from_cstring(ptr: *const c_char) -> String {
    // SAFETY: Retakes ownership of C string `ptr`
    CStr::from_ptr(ptr)
        .to_str()
        .expect("CStr::from_ptr failed")
        .to_owned()
}

/// Expects `ptr` to be an array of valid UTF-8 chars with a null byte terminator.
#[no_mangle]
pub unsafe extern "C" fn cstring_free(ptr: *const c_char) {
    // SAFETY: Retakes ownership of C string `ptr`, then drops
    drop(from_cstring(ptr))
}

pub fn precision_from_str(s: &str) -> u8 {
    let lower_s = s.to_lowercase();
    if lower_s.find('.').is_none() {
        return 0;
    }
    return lower_s.split('.').last().unwrap().len() as u8;
    // TODO(cs): Implement scientific notation parsing
}

#[cfg(test)]
mod tests {
    use crate::text::precision_from_str;
    use crate::text::{cstring_free, from_cstring, into_cstring};
    use std::ffi::CString;

    #[test]
    fn test_into_cstring() {
        unsafe {
            let value = String::from("hello, world!");
            let ptr = into_cstring(value);
            cstring_free(ptr);
        }
    }

    #[test]
    fn test_from_cstring() {
        unsafe {
            let value = String::from("hello, world!");
            let ptr = into_cstring(value);
            let s = from_cstring(ptr);

            assert_eq!(s, "hello, world!");
        }
    }

    #[test]
    fn test_cstring_free() {
        unsafe {
            let cstring = CString::new("hello, world!").unwrap();
            cstring_free(cstring.into_raw());
        }
    }

    #[test]
    fn test_precision_from_str() {
        assert_eq!(precision_from_str("1"), 0);
        assert_eq!(precision_from_str("2.1"), 1);
        assert_eq!(precision_from_str("2.204622"), 6);
    }
}

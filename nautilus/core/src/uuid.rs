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
use uuid::Uuid;

#[no_mangle]
pub extern "C" fn uuid_chars_new() -> *mut c_char {
    return CString::new(Uuid::new_v4().to_string()).unwrap().into_raw();
}

#[no_mangle]
pub extern "C" fn uuid_chars_free(s: *mut c_char) {
    unsafe {
        if s.is_null() {
            return;
        }
        CString::from_raw(s) // Frees memory here
    };
}

#[cfg(test)]
mod tests {
    use crate::uuid;

    #[test]
    fn uuid_chars_new_returns_none_null_ptr() {
        let uuid_chars = uuid::uuid_chars_new();
        assert!(!uuid_chars.is_null());
    }

    #[test]
    fn uuid_chars_free_returns_none_null_ptr() {
        let uuid_chars = uuid::uuid_chars_new();
        uuid::uuid_chars_free(uuid_chars)

        // TODO(cs): Check uuid_chars is freed
    }
}

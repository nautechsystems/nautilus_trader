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

use std::ffi::{c_char, CStr, CString};

use pyo3::types::PyString;
use pyo3::{ffi, FromPyPointer, Python};

/// Returns an owned string from a valid Python object pointer.
///
/// # Safety
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
/// # Panics
/// - If `ptr` is null.
#[must_use]
pub unsafe fn pystr_to_string(ptr: *mut ffi::PyObject) -> String {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    Python::with_gil(|py| PyString::from_borrowed_ptr(py, ptr).to_string())
}

/// Convert a C string pointer into an owned `String`.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
/// # Panics
/// - If `ptr` is null.
#[must_use]
pub unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    CStr::from_ptr(ptr)
        .to_str()
        .expect("CStr::from_ptr failed")
        .to_string()
}

/// Create a C string pointer to newly allocated memory from a [&str].
#[must_use]
pub fn string_to_cstr(s: &str) -> *const c_char {
    CString::new(s).expect("CString::new failed").into_raw()
}

/// Drops the C string memory at the pointer.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
/// # Panics
/// - If `ptr` is null.
#[no_mangle]
pub unsafe extern "C" fn cstr_free(ptr: *const c_char) {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    let cstring = CString::from_raw(ptr as *mut c_char);
    drop(cstring);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use pyo3::AsPyPointer;

    use super::*;

    #[test]
    fn test_pystr_to_string() {
        pyo3::prepare_freethreaded_python();
        // Test with valid Python object pointer
        let ptr = Python::with_gil(|py| PyString::new(py, "test string1").as_ptr());
        let result = unsafe { pystr_to_string(ptr) };
        assert_eq!(result, "test string1");
    }

    #[test]
    #[should_panic]
    fn test_pystr_to_string_with_null_ptr() {
        // Test with null Python object pointer
        let ptr: *mut ffi::PyObject = std::ptr::null_mut();
        unsafe { pystr_to_string(ptr) };
    }

    #[test]
    fn test_cstr_to_string() {
        // Test with valid C string pointer
        let c_string = CString::new("test string2").expect("CString::new failed");
        let ptr = c_string.as_ptr();
        let result = unsafe { cstr_to_string(ptr) };
        assert_eq!(result, "test string2");
    }

    #[test]
    #[should_panic]
    fn test_cstr_to_string_with_null_ptr() {
        // Test with null C string pointer
        let ptr: *const c_char = std::ptr::null();
        unsafe { cstr_to_string(ptr) };
    }

    #[test]
    fn test_string_to_cstr() {
        let s = "test string";
        let c_str_ptr = string_to_cstr(s);
        let c_str = unsafe { CStr::from_ptr(c_str_ptr) };
        let result = c_str.to_str().expect("CStr::from_ptr failed");
        assert_eq!(result, s);
    }

    #[test]
    fn test_cstr_free() {
        let c_string = CString::new("test string3").expect("CString::new failed");
        let ptr = c_string.into_raw(); // <-- pointer _must_ be obtained this way
        unsafe { cstr_free(ptr) };
    }
}

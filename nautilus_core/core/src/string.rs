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

use std::{
    ffi::{c_char, CStr, CString},
    str,
};

use pyo3::{ffi, types::PyString, FromPyPointer, Python};
use ustr::Ustr;

/// Returns an owned string from a valid Python object pointer.
///
/// # Safety
///
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
///
/// # Panics
///
/// - If `ptr` is null.
#[must_use]
pub unsafe fn pystr_to_string(ptr: *mut ffi::PyObject) -> String {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    Python::with_gil(|py| PyString::from_borrowed_ptr(py, ptr).to_string())
}

/// Convert a C string pointer into an owned `String`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// - If `ptr` is null.
#[must_use]
pub unsafe fn cstr_to_ustr(ptr: *const c_char) -> Ustr {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    Ustr::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

/// Convert a C string pointer into an owned `Option<Ustr>`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer or NULL.
///
/// # Panics
///
/// - If `ptr` is null.
#[must_use]
pub unsafe fn optional_cstr_to_ustr(ptr: *const c_char) -> Option<Ustr> {
    if !ptr.is_null() {
        Some(cstr_to_ustr(ptr))
    } else {
        None
    }
}

/// Convert a C string pointer into a string slice.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// - If `ptr` is null.
#[must_use]
pub unsafe fn cstr_to_str(ptr: *const c_char) -> &'static str {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed")
}

/// Convert a C string pointer into an owned `String`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// - If `ptr` is null.
#[must_use]
pub unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    cstr_to_str(ptr).to_string()
}

/// Convert a C string pointer into an owned `Option<String>`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[must_use]
pub unsafe fn optional_cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(cstr_to_string(ptr))
    }
}

/// Create a C string pointer to newly allocated memory from a [&str].
#[must_use]
pub fn str_to_cstr(s: &str) -> *const c_char {
    CString::new(s).expect("CString::new failed").into_raw()
}

/// Drops the C string memory at the pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// - If `ptr` is null.
#[no_mangle]
pub unsafe extern "C" fn cstr_drop(ptr: *const c_char) {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    let cstring = CString::from_raw(ptr.cast_mut());
    drop(cstring);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use pyo3::AsPyPointer;
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_pystr_to_string() {
        pyo3::prepare_freethreaded_python();
        // Create a valid Python object pointer
        let ptr = Python::with_gil(|py| PyString::new(py, "test string1").as_ptr());
        let result = unsafe { pystr_to_string(ptr) };
        assert_eq!(result, "test string1");
    }

    #[rstest]
    #[should_panic]
    fn test_pystr_to_string_with_null_ptr() {
        // Create a null Python object pointer
        let ptr: *mut ffi::PyObject = std::ptr::null_mut();
        unsafe {
            let _ = pystr_to_string(ptr);
        };
    }

    #[rstest]
    fn test_cstr_to_str() {
        // Create a valid C string pointer
        let c_string = CString::new("test string2").expect("CString::new failed");
        let ptr = c_string.as_ptr();
        let result = unsafe { cstr_to_str(ptr) };
        assert_eq!(result, "test string2");
    }

    #[rstest]
    fn test_cstr_to_string() {
        // Create a valid C string pointer
        let c_string = CString::new("test string2").expect("CString::new failed");
        let ptr = c_string.as_ptr();
        let result = unsafe { cstr_to_string(ptr) };
        assert_eq!(result, "test string2");
    }

    #[rstest]
    #[should_panic]
    fn test_cstr_to_string_with_null_ptr() {
        // Create a null C string pointer
        let ptr: *const c_char = std::ptr::null();
        unsafe {
            let _ = cstr_to_string(ptr);
        };
    }

    #[rstest]
    fn test_optional_cstr_to_string_with_null_ptr() {
        // Call optional_cstr_to_string with null pointer
        let ptr = std::ptr::null();
        let result = unsafe { optional_cstr_to_string(ptr) };
        assert!(result.is_none());
    }

    #[rstest]
    fn test_optional_cstr_to_string_with_valid_ptr() {
        // Create a valid C string
        let input_str = "hello world";
        let c_str = CString::new(input_str).expect("CString::new failed");
        let result = unsafe { optional_cstr_to_string(c_str.as_ptr()) };
        assert!(result.is_some());
        assert_eq!(result.unwrap(), input_str);
    }

    #[rstest]
    fn test_string_to_cstr() {
        let s = "test string";
        let c_str_ptr = str_to_cstr(s);
        let c_str = unsafe { CStr::from_ptr(c_str_ptr) };
        let result = c_str.to_str().expect("CStr::from_ptr failed");
        assert_eq!(result, s);
    }

    #[rstest]
    fn test_cstr_drop() {
        let c_string = CString::new("test string3").expect("CString::new failed");
        let ptr = c_string.into_raw(); // <-- pointer _must_ be obtained this way
        unsafe { cstr_drop(ptr) };
    }
}

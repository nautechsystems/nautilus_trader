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
use std::ffi::CString;
use std::os::raw::c_char;
use pyo3::types::PyString;
use pyo3::{ffi, FromPyPointer, IntoPyPointer, Py, Python};

/// Returns an owned string from a valid Python object pointer.
///
/// # Safety
/// - Panics if `ptr` is null.
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
#[inline(always)]
pub unsafe fn pystr_to_string(ptr: *mut ffi::PyObject) -> String {
    assert!(!ptr.is_null(), "pointer was NULL");
    Python::with_gil(|py| PyString::from_borrowed_ptr(py, ptr).to_string())
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[inline(always)]
pub unsafe fn string_to_pystr(s: &str) -> *mut ffi::PyObject {
    let py = Python::assume_gil_acquired();
    let pystr: Py<PyString> = PyString::new(py, s).into();
    pystr.into_ptr()
}

pub fn precision_from_str(s: &str) -> u8 {
    let lower_s = s.to_lowercase();
    // Handle scientific notation
    if lower_s.contains("e-") {
        return lower_s.split("e-").last().unwrap().parse::<u8>().unwrap();
    }
    if !lower_s.contains('.') {
        return 0;
    }
    return lower_s.split('.').last().unwrap().len() as u8;
}

#[inline(always)]
pub fn string_to_cstr(s: &str) -> *const c_char {
    CString::new(s).expect("CString::new failed").into_raw()
}

#[no_mangle]
pub unsafe extern fn cstring_free(s: *const c_char) {
    let str = CString::from_raw(s as *mut i8);
    drop(str);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::PyString;
    use pyo3::{prepare_freethreaded_python, IntoPyPointer, Python};

    #[test]
    fn test_pystr_to_string() {
        prepare_freethreaded_python();
        Python::with_gil(|py| {
            let pystr = PyString::new(py, "hello, world").into_ptr();

            let string = unsafe { pystr_to_string(pystr) };

            assert_eq!(string.to_string(), "hello, world")
        });
    }

    #[test]
    fn test_string_to_pystr() {
        prepare_freethreaded_python();
        Python::with_gil(|_| {
            let string = String::from("hello, world");
            let ptr = unsafe { string_to_pystr(&string) };

            let s = unsafe { pystr_to_string(ptr) };

            assert_eq!(s, "hello, world")
        });
    }

    #[test]
    fn test_precision_from_str() {
        assert_eq!(precision_from_str(""), 0);
        assert_eq!(precision_from_str("0"), 0);
        assert_eq!(precision_from_str("1"), 0);
        assert_eq!(precision_from_str("1.0"), 1);
        assert_eq!(precision_from_str("2.1"), 1);
        assert_eq!(precision_from_str("2.204622"), 6);
        assert_eq!(precision_from_str("0.000000001"), 9);
        assert_eq!(precision_from_str("1e-8"), 8);
        assert_eq!(precision_from_str("2e-9"), 9);
        assert_eq!(precision_from_str("1e8"), 0);
        assert_eq!(precision_from_str("2e8"), 0);
    }
}

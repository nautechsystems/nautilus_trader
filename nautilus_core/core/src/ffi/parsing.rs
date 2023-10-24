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
    collections::HashMap,
    ffi::{c_char, CStr, CString},
};

use serde_json::{Result, Value};
use ustr::Ustr;

use crate::{ffi::string::cstr_to_string, parsing::precision_from_str};

/// Convert a C bytes pointer into an owned `Vec<String>`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[must_use]
pub unsafe fn bytes_to_string_vec(ptr: *const c_char) -> Vec<String> {
    assert!(!ptr.is_null(), "`ptr` was NULL");

    let c_str = CStr::from_ptr(ptr);
    let bytes = c_str.to_bytes();
    let json_string = std::str::from_utf8(bytes).unwrap();
    let parsed_value: serde_json::Value = serde_json::from_str(json_string).unwrap();

    match parsed_value {
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .filter_map(|value| match value {
                serde_json::Value::String(string_value) => Some(string_value),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[must_use]
pub fn string_vec_to_bytes(strings: Vec<String>) -> *const c_char {
    let json_string = serde_json::to_string(&strings).unwrap();
    let c_string = CString::new(json_string).unwrap();
    c_string.into_raw()
}

/// Convert a C bytes pointer into an owned `Option<HashMap<String, Value>>`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[must_use]
pub unsafe fn optional_bytes_to_json(ptr: *const c_char) -> Option<HashMap<String, Value>> {
    if ptr.is_null() {
        None
    } else {
        let c_str = CStr::from_ptr(ptr);
        let bytes = c_str.to_bytes();
        let json_string = std::str::from_utf8(bytes).unwrap();
        let result: Result<HashMap<String, Value>> = serde_json::from_str(json_string);
        match result {
            Ok(map) => Some(map),
            Err(e) => {
                eprintln!("Error parsing JSON: {e}");
                None
            }
        }
    }
}

/// Convert a C bytes pointer into an owned `Option<HashMap<Ustr, Ustr>>`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[must_use]
pub unsafe fn optional_bytes_to_str_map(ptr: *const c_char) -> Option<HashMap<Ustr, Ustr>> {
    if ptr.is_null() {
        None
    } else {
        let c_str = CStr::from_ptr(ptr);
        let bytes = c_str.to_bytes();
        let json_string = std::str::from_utf8(bytes).unwrap();
        let result: Result<HashMap<Ustr, Ustr>> = serde_json::from_str(json_string);
        match result {
            Ok(map) => Some(map),
            Err(e) => {
                eprintln!("Error parsing JSON: {e}");
                None
            }
        }
    }
}

/// Convert a C bytes pointer into an owned `Option<Vec<String>>`.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[must_use]
pub unsafe fn optional_bytes_to_str_vec(ptr: *const c_char) -> Option<Vec<String>> {
    if ptr.is_null() {
        None
    } else {
        let c_str = CStr::from_ptr(ptr);
        let bytes = c_str.to_bytes();
        let json_string = std::str::from_utf8(bytes).unwrap();
        let result: Result<Vec<String>> = serde_json::from_str(json_string);
        match result {
            Ok(map) => Some(map),
            Err(e) => {
                eprintln!("Error parsing JSON: {e}");
                None
            }
        }
    }
}

/// Return the decimal precision inferred from the given C string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// - If `ptr` is null.
#[no_mangle]
pub unsafe extern "C" fn precision_from_cstr(ptr: *const c_char) -> u8 {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    precision_from_str(&cstr_to_string(ptr))
}

/// Return a `bool` value from the given `u8`.
#[must_use]
pub fn u8_to_bool(value: u8) -> bool {
    value != 0
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_optional_bytes_to_json_null() {
        let ptr = std::ptr::null();
        let result = unsafe { optional_bytes_to_json(ptr) };
        assert_eq!(result, None);
    }

    #[rstest]
    fn test_optional_bytes_to_json_empty() {
        let json_str = CString::new("{}").unwrap();
        let ptr = json_str.as_ptr() as *const c_char;
        let result = unsafe { optional_bytes_to_json(ptr) };
        assert_eq!(result, Some(HashMap::new()));
    }

    #[rstest]
    fn test_string_vec_to_bytes_valid() {
        let strings = vec!["value1", "value2", "value3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<String>>();

        let ptr = string_vec_to_bytes(strings.clone());

        let result = unsafe { bytes_to_string_vec(ptr) };
        assert_eq!(result, strings);
    }

    #[rstest]
    fn test_string_vec_to_bytes_empty() {
        let strings = Vec::new();
        let ptr = string_vec_to_bytes(strings.clone());

        let result = unsafe { bytes_to_string_vec(ptr) };
        assert_eq!(result, strings);
    }

    #[rstest]
    fn test_bytes_to_string_vec_valid() {
        let json_str = CString::new(r#"["value1", "value2", "value3"]"#).unwrap();
        let ptr = json_str.as_ptr() as *const c_char;
        let result = unsafe { bytes_to_string_vec(ptr) };

        let expected_vec = vec!["value1", "value2", "value3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<String>>();

        assert_eq!(result, expected_vec);
    }

    #[rstest]
    fn test_bytes_to_string_vec_invalid() {
        let json_str = CString::new(r#"["value1", 42, "value3"]"#).unwrap();
        let ptr = json_str.as_ptr() as *const c_char;
        let result = unsafe { bytes_to_string_vec(ptr) };

        let expected_vec = vec!["value1", "value3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<String>>();

        assert_eq!(result, expected_vec);
    }

    #[rstest]
    fn test_optional_bytes_to_json_valid() {
        let json_str = CString::new(r#"{"key1": "value1", "key2": 2}"#).unwrap();
        let ptr = json_str.as_ptr() as *const c_char;
        let result = unsafe { optional_bytes_to_json(ptr) };
        let mut expected_map = HashMap::new();
        expected_map.insert("key1".to_owned(), Value::String("value1".to_owned()));
        expected_map.insert(
            "key2".to_owned(),
            Value::Number(serde_json::Number::from(2)),
        );
        assert_eq!(result, Some(expected_map));
    }

    #[rstest]
    fn test_optional_bytes_to_json_invalid() {
        let json_str = CString::new(r#"{"key1": "value1", "key2": }"#).unwrap();
        let ptr = json_str.as_ptr() as *const c_char;
        let result = unsafe { optional_bytes_to_json(ptr) };
        assert_eq!(result, None);
    }

    #[rstest]
    #[case("1e8", 0)]
    #[case("123", 0)]
    #[case("123.45", 2)]
    #[case("123.456789", 6)]
    #[case("1.23456789e-2", 2)]
    #[case("1.23456789e-12", 12)]
    fn test_precision_from_cstr(#[case] input: &str, #[case] expected: u8) {
        let c_str = CString::new(input).unwrap();
        assert_eq!(unsafe { precision_from_cstr(c_str.as_ptr()) }, expected);
    }
}

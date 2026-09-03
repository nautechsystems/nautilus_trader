// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Converts common C types (primarily UTF-8 encoded `char *` pointers) into
//! the Rust data structures used throughout NautilusTrader.
//!
//! The conversions are opinionated:
//!
//! - JSON is used as the interchange format for complex structures.
//! - `ustr::Ustr` is preferred over `String` where possible for its performance benefits.
//!
//! All functions are `#[must_use]` and, unless otherwise noted, **assume** that the input pointer
//! is non-null and points to a valid, *null-terminated* UTF-8 string.

use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_char},
};

use serde::de::DeserializeOwned;
use serde_json::Value;
use ustr::Ustr;

use crate::{
    ffi::{abort_on_panic, string::cstr_as_str},
    string::parsing::min_increment_precision_from_str,
};

/// Convert a C bytes pointer into an owned `Vec<String>`.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if `ptr` is null, contains invalid UTF-8/JSON, or the JSON value
/// is not an array of strings.
#[must_use]
pub unsafe fn bytes_to_string_vec(ptr: *const c_char) -> Vec<String> {
    assert!(!ptr.is_null(), "`ptr` was NULL");

    // SAFETY: Caller guarantees ptr is valid per function contract
    let c_str = unsafe { CStr::from_ptr(ptr) };
    let bytes = c_str.to_bytes();

    let json_string = std::str::from_utf8(bytes).expect("C string contains invalid UTF-8");
    let value: serde_json::Value =
        serde_json::from_str(json_string).expect("C string contains invalid JSON");

    let arr = value
        .as_array()
        .expect("C string JSON must be an array of strings");

    arr.iter()
        .map(|value| {
            value
                .as_str()
                .expect("C string JSON array must contain only strings")
                .to_owned()
        })
        .collect()
}

/// Convert a slice of `String` into a C string pointer (JSON encoded).
///
/// # Panics
///
/// Panics if JSON serialization fails or if the generated string contains interior null bytes.
#[must_use]
pub fn string_vec_to_bytes(strings: &[String]) -> *const c_char {
    let json_string = serde_json::to_string(strings).expect("Failed to serialize strings to JSON");
    let c_string = CString::new(json_string).expect("JSON string contains interior null bytes");

    c_string.into_raw()
}

/// Convert a C bytes pointer into an owned `Option<HashMap<String, Value>>`.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if `ptr` is not null but contains invalid UTF-8 or JSON.
#[must_use]
pub unsafe fn optional_bytes_to_json(ptr: *const c_char) -> Option<HashMap<String, Value>> {
    // SAFETY: A non-null pointer is valid under the caller's contract
    unsafe { optional_json_from_cstr(ptr) }
}

/// Convert a C bytes pointer into an owned `Option<HashMap<Ustr, Ustr>>`.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if `ptr` is not null but contains invalid UTF-8 or JSON.
#[must_use]
pub unsafe fn optional_bytes_to_str_map(ptr: *const c_char) -> Option<HashMap<Ustr, Ustr>> {
    // SAFETY: A non-null pointer is valid under the caller's contract
    unsafe { optional_json_from_cstr(ptr) }
}

/// Convert a C bytes pointer into an owned `Option<Vec<String>>`.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if `ptr` is not null but contains invalid UTF-8 or JSON.
#[must_use]
pub unsafe fn optional_bytes_to_str_vec(ptr: *const c_char) -> Option<Vec<String>> {
    // SAFETY: A non-null pointer is valid under the caller's contract
    unsafe { optional_json_from_cstr(ptr) }
}

/// # Safety
///
/// If `ptr` is non-null, it must reference a valid, null-terminated UTF-8 C string that remains
/// unchanged for the duration of this call.
unsafe fn optional_json_from_cstr<T>(ptr: *const c_char) -> Option<T>
where
    T: DeserializeOwned,
{
    if ptr.is_null() {
        return None;
    }

    // SAFETY: A non-null pointer is valid under the caller's contract
    let json = unsafe { cstr_as_str(ptr) };
    let result = serde_json::from_str(json).expect("C string contains invalid JSON");
    Some(result)
}

/// Return the decimal precision inferred from the given C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if `ptr` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn precision_from_cstr(ptr: *const c_char) -> u8 {
    abort_on_panic(|| {
        assert!(!ptr.is_null(), "`ptr` was NULL");
        // SAFETY: Caller guarantees ptr is valid per function contract
        let s = unsafe { cstr_as_str(ptr) };
        precision_from_v1_str(s)
    })
}

/// Return the minimum price increment decimal precision inferred from the given C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if `ptr` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn min_increment_precision_from_cstr(ptr: *const c_char) -> u8 {
    abort_on_panic(|| {
        assert!(!ptr.is_null(), "`ptr` was NULL");
        // SAFETY: Caller guarantees ptr is valid per function contract
        let s = unsafe { cstr_as_str(ptr) };
        min_increment_precision_from_str(s)
    })
}

// TODO: Remove this temporary parser when v1 drops its legacy source-text precision contract
fn precision_from_v1_str(value: &str) -> u8 {
    let value = value.trim().to_ascii_lowercase();

    if value.contains("e-") {
        let exponent = value
            .split("e-")
            .nth(1)
            .expect("Invalid scientific notation format: missing exponent after 'e-'");

        if let Ok(exponent) = exponent.parse::<u64>() {
            return u8::try_from(exponent).unwrap_or(u8::MAX);
        }

        assert!(
            !exponent.is_empty(),
            "Invalid scientific notation format: missing exponent after 'e-'"
        );

        if exponent.chars().all(|c| c.is_ascii_digit()) {
            return u8::MAX;
        }

        panic!("Invalid scientific notation exponent '{exponent}': must be a valid number");
    }

    value.split_once('.').map_or(0, |(_, decimal)| {
        u8::try_from(decimal.len()).unwrap_or(u8::MAX)
    })
}

/// Return a `bool` value from the given `u8`.
#[must_use]
pub const fn u8_as_bool(value: u8) -> bool {
    value != 0
}

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
        let ptr = json_str.as_ptr().cast::<c_char>();
        let result = unsafe { optional_bytes_to_json(ptr) };
        assert_eq!(result, Some(HashMap::new()));
    }

    #[rstest]
    fn test_string_vec_to_bytes_valid() {
        let strings = vec!["value1", "value2", "value3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<String>>();

        let ptr = string_vec_to_bytes(&strings);

        let result = unsafe { bytes_to_string_vec(ptr) };
        assert_eq!(result, strings);
    }

    #[rstest]
    fn test_string_vec_to_bytes_empty() {
        let strings = Vec::new();
        let ptr = string_vec_to_bytes(&strings);

        let result = unsafe { bytes_to_string_vec(ptr) };
        assert_eq!(result, strings);
    }

    #[rstest]
    fn test_bytes_to_string_vec_valid() {
        let json_str = CString::new(r#"["value1", "value2", "value3"]"#).unwrap();
        let ptr = json_str.as_ptr().cast::<c_char>();
        let result = unsafe { bytes_to_string_vec(ptr) };

        let expected_vec = vec!["value1", "value2", "value3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<String>>();

        assert_eq!(result, expected_vec);
    }

    #[rstest]
    #[should_panic(expected = "array must contain only strings")]
    fn test_bytes_to_string_vec_invalid() {
        let json_str = CString::new(r#"["value1", 42, "value3"]"#).unwrap();
        let ptr = json_str.as_ptr().cast::<c_char>();
        let _ = unsafe { bytes_to_string_vec(ptr) };
    }

    #[rstest]
    fn test_optional_bytes_to_json_valid() {
        let json_str = CString::new(r#"{"key1": "value1", "key2": 2}"#).unwrap();
        let ptr = json_str.as_ptr().cast::<c_char>();
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
    fn test_optional_bytes_to_str_map_valid() {
        let json_str = CString::new(r#"{"key1": "value1", "key2": "value2"}"#).unwrap();
        let ptr = json_str.as_ptr().cast::<c_char>();
        let result = unsafe { optional_bytes_to_str_map(ptr) };
        let expected_map = HashMap::from([
            (Ustr::from("key1"), Ustr::from("value1")),
            (Ustr::from("key2"), Ustr::from("value2")),
        ]);
        assert_eq!(result, Some(expected_map));
    }

    #[rstest]
    fn test_optional_bytes_to_str_vec_valid() {
        let json_str = CString::new(r#"["value1", "value2", "value3"]"#).unwrap();
        let ptr = json_str.as_ptr().cast::<c_char>();
        let result = unsafe { optional_bytes_to_str_vec(ptr) };
        let expected_vec = vec![
            "value1".to_string(),
            "value2".to_string(),
            "value3".to_string(),
        ];
        assert_eq!(result, Some(expected_vec));
    }

    #[rstest]
    #[should_panic(expected = "C string contains invalid JSON")]
    fn test_optional_bytes_to_json_invalid() {
        let json_str = CString::new(r#"{"key1": "value1", "key2": }"#).unwrap();
        let ptr = json_str.as_ptr().cast::<c_char>();
        let _result = unsafe { optional_bytes_to_json(ptr) };
    }

    #[rstest]
    #[case("1e8", 0)]
    #[case("123", 0)]
    #[case("123.45", 2)]
    #[case("123.456789", 6)]
    #[case("2.5e4", 3)]
    #[case("7.89E1", 4)]
    #[case("1.23456789e-2", 2)]
    #[case("1.23456789e-12", 12)]
    fn test_precision_from_cstr(#[case] input: &str, #[case] expected: u8) {
        let c_str = CString::new(input).unwrap();
        assert_eq!(unsafe { precision_from_cstr(c_str.as_ptr()) }, expected);
    }

    #[rstest]
    #[case("1.010", 2)]
    #[case("1.5e-2", 3)]
    #[case("0.0001000", 4)]
    fn test_min_increment_precision_from_cstr(#[case] input: &str, #[case] expected: u8) {
        let c_str = CString::new(input).unwrap();
        assert_eq!(
            unsafe { min_increment_precision_from_cstr(c_str.as_ptr()) },
            expected
        );
    }

    #[rstest]
    #[case(0, false)]
    #[case(1, true)]
    #[case(u8::MAX, true)]
    fn test_u8_as_bool(#[case] input: u8, #[case] expected: bool) {
        assert_eq!(u8_as_bool(input), expected);
    }
}

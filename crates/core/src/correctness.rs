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

//! Functions for correctness checks similar to the *design by contract* philosophy.
//!
//! This module provides validation checking of function or method conditions.
//!
//! A condition is a predicate which must be true just prior to the execution of
//! some section of code - for correct behavior as per the design specification.
//!
//! An [`anyhow::Result`] is returned with a descriptive message when the
//! condition check fails.

use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
    hash::Hash,
};

use indexmap::IndexMap;

/// A message prefix that can be used with calls to `expect` or other assertion-related functions.
///
/// This constant provides a standard message that can be used to indicate a failure condition
/// when a predicate or condition does not hold true. It is typically used in conjunction with
/// functions like `expect` to provide a consistent error message.
pub const FAILED: &str = "Condition failed";

/// Checks the `predicate` is true.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_predicate_true(predicate: bool, fail_msg: &str) -> anyhow::Result<()> {
    if !predicate {
        anyhow::bail!("{fail_msg}")
    }
    Ok(())
}

/// Checks the `predicate` is false.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_predicate_false(predicate: bool, fail_msg: &str) -> anyhow::Result<()> {
    if predicate {
        anyhow::bail!("{fail_msg}")
    }
    Ok(())
}

/// Checks if the string `s` is not empty.
///
/// This function performs a basic check to ensure the string has at least one character.
/// Unlike `check_valid_string`, it does not validate ASCII characters or check for whitespace.
///
/// # Errors
///
/// This function returns an error if `s` is empty.
#[inline(always)]
pub fn check_nonempty_string<T: AsRef<str>>(s: T, param: &str) -> anyhow::Result<()> {
    if s.as_ref().is_empty() {
        anyhow::bail!("invalid string for '{param}', was empty");
    }
    Ok(())
}

/// Checks the string `s` has semantic meaning and contains only ASCII characters.
///
/// # Errors
///
/// This function returns an error:
/// - If `s` is an empty string.
/// - If `s` consists solely of whitespace characters.
/// - If `s` contains one or more non-ASCII characters.
#[inline(always)]
pub fn check_valid_string<T: AsRef<str>>(s: T, param: &str) -> anyhow::Result<()> {
    let s = s.as_ref();

    if s.is_empty() {
        anyhow::bail!("invalid string for '{param}', was empty");
    }

    // Ensure string is only traversed once
    let mut has_non_whitespace = false;
    for c in s.chars() {
        if !c.is_whitespace() {
            has_non_whitespace = true;
        }
        if !c.is_ascii() {
            anyhow::bail!("invalid string for '{param}' contained a non-ASCII char, was '{s}'");
        }
    }

    if !has_non_whitespace {
        anyhow::bail!("invalid string for '{param}', was all whitespace");
    }

    Ok(())
}

/// Checks the string `s` if Some, contains only ASCII characters and has semantic meaning.
///
/// # Errors
///
/// This function returns an error:
/// - If `s` is an empty string.
/// - If `s` consists solely of whitespace characters.
/// - If `s` contains one or more non-ASCII characters.
#[inline(always)]
pub fn check_valid_string_optional<T: AsRef<str>>(s: Option<T>, param: &str) -> anyhow::Result<()> {
    let s = s.as_ref();
    if let Some(s) = s {
        check_valid_string(s, param)?;
    }
    Ok(())
}

/// Checks the string `s` contains the pattern `pat`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_string_contains<T: AsRef<str>>(s: T, pat: &str, param: &str) -> anyhow::Result<()> {
    let s = s.as_ref();
    if !s.contains(pat) {
        anyhow::bail!("invalid string for '{param}' did not contain '{pat}', was '{s}'")
    }
    Ok(())
}

/// Checks the values are equal.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_equal<T: PartialEq + Debug + Display>(
    lhs: T,
    rhs: T,
    lhs_param: &str,
    rhs_param: &str,
) -> anyhow::Result<()> {
    if lhs != rhs {
        anyhow::bail!("'{lhs_param}' value of {lhs} was not equal to '{rhs_param}' value of {rhs}");
    }
    Ok(())
}

/// Checks the `u8` values are equal.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_equal_u8(lhs: u8, rhs: u8, lhs_param: &str, rhs_param: &str) -> anyhow::Result<()> {
    if lhs != rhs {
        anyhow::bail!("'{lhs_param}' u8 of {lhs} was not equal to '{rhs_param}' u8 of {rhs}")
    }
    Ok(())
}

/// Checks the `usize` values are equal.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_equal_usize(
    lhs: usize,
    rhs: usize,
    lhs_param: &str,
    rhs_param: &str,
) -> anyhow::Result<()> {
    if lhs != rhs {
        anyhow::bail!("'{lhs_param}' usize of {lhs} was not equal to '{rhs_param}' usize of {rhs}")
    }
    Ok(())
}

/// Checks the `u64` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_u64(value: u64, param: &str) -> anyhow::Result<()> {
    if value == 0 {
        anyhow::bail!("invalid u64 for '{param}' not positive, was {value}")
    }
    Ok(())
}

/// Checks the `u128` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_u128(value: u128, param: &str) -> anyhow::Result<()> {
    if value == 0 {
        anyhow::bail!("invalid u128 for '{param}' not positive, was {value}")
    }
    Ok(())
}

/// Checks the `i64` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_i64(value: i64, param: &str) -> anyhow::Result<()> {
    if value <= 0 {
        anyhow::bail!("invalid i64 for '{param}' not positive, was {value}")
    }
    Ok(())
}

/// Checks the `i64` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_i128(value: i128, param: &str) -> anyhow::Result<()> {
    if value <= 0 {
        anyhow::bail!("invalid i64 for '{param}' not positive, was {value}")
    }
    Ok(())
}

/// Checks the `f64` value is non-negative (< 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_non_negative_f64(value: f64, param: &str) -> anyhow::Result<()> {
    if value.is_nan() || value.is_infinite() {
        anyhow::bail!("invalid f64 for '{param}', was {value}")
    }
    if value < 0.0 {
        anyhow::bail!("invalid f64 for '{param}' negative, was {value}")
    }
    Ok(())
}

/// Checks the `u8` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_u8(value: u8, l: u8, r: u8, param: &str) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("invalid u8 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Checks the `u64` value is range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_u64(value: u64, l: u64, r: u64, param: &str) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("invalid u64 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Checks the `i64` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_i64(value: i64, l: i64, r: i64, param: &str) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("invalid i64 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Checks the `f64` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_f64(value: f64, l: f64, r: f64, param: &str) -> anyhow::Result<()> {
    const EPSILON: f64 = 1e-15; // Epsilon to account for floating-point precision issues

    if value.is_nan() || value.is_infinite() {
        anyhow::bail!("invalid f64 for '{param}', was {value}")
    }
    if value < l - EPSILON || value > r + EPSILON {
        anyhow::bail!("invalid f64 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Checks the `usize` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_usize(
    value: usize,
    l: usize,
    r: usize,
    param: &str,
) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("invalid usize for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Checks the slice is empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_slice_empty<T>(slice: &[T], param: &str) -> anyhow::Result<()> {
    if !slice.is_empty() {
        anyhow::bail!(
            "the '{param}' slice `&[{}]` was not empty",
            std::any::type_name::<T>()
        )
    }
    Ok(())
}

/// Checks the slice is **not** empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_slice_not_empty<T>(slice: &[T], param: &str) -> anyhow::Result<()> {
    if slice.is_empty() {
        anyhow::bail!(
            "the '{param}' slice `&[{}]` was empty",
            std::any::type_name::<T>()
        )
    }
    Ok(())
}

/// Checks the hashmap is empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_map_empty<K, V>(map: &HashMap<K, V>, param: &str) -> anyhow::Result<()> {
    if !map.is_empty() {
        anyhow::bail!(
            "the '{param}' map `&<{}, {}>` was not empty",
            std::any::type_name::<K>(),
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

/// Checks the map is **not** empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_map_not_empty<K, V>(map: &HashMap<K, V>, param: &str) -> anyhow::Result<()> {
    if map.is_empty() {
        anyhow::bail!(
            "the '{param}' map `&<{}, {}>` was empty",
            std::any::type_name::<K>(),
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

/// Checks the `key` is **not** in the `map`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_key_not_in_map<K, V>(
    key: &K,
    map: &HashMap<K, V>,
    key_name: &str,
    map_name: &str,
) -> anyhow::Result<()>
where
    K: Hash + Eq + Display + Clone,
    V: Debug,
{
    if map.contains_key(key) {
        anyhow::bail!(
            "the '{key_name}' key {key} was already in the '{map_name}' map `&<{}, {}>`",
            std::any::type_name::<K>(),
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

/// Checks the `key` is in the `map`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_key_in_map<K, V>(
    key: &K,
    map: &HashMap<K, V>,
    key_name: &str,
    map_name: &str,
) -> anyhow::Result<()>
where
    K: Hash + Eq + Display + Clone,
    V: Debug,
{
    if !map.contains_key(key) {
        anyhow::bail!(
            "the '{key_name}' key {key} was not in the '{map_name}' map `&<{}, {}>`",
            std::any::type_name::<K>(),
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

/// Checks the `key` is **not** in the `map`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_key_not_in_index_map<K, V>(
    key: &K,
    map: &IndexMap<K, V>,
    key_name: &str,
    map_name: &str,
) -> anyhow::Result<()>
where
    K: Hash + Eq + Display + Clone,
    V: Debug,
{
    if map.contains_key(key) {
        anyhow::bail!(
            "the '{key_name}' key {key} was already in the '{map_name}' map `&<{}, {}>`",
            std::any::type_name::<K>(),
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

/// Checks the `key` is in the `map`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_key_in_index_map<K, V>(
    key: &K,
    map: &IndexMap<K, V>,
    key_name: &str,
    map_name: &str,
) -> anyhow::Result<()>
where
    K: Hash + Eq + Display + Clone,
    V: Debug,
{
    if !map.contains_key(key) {
        anyhow::bail!(
            "the '{key_name}' key {key} was not in the '{map_name}' map `&<{}, {}>`",
            std::any::type_name::<K>(),
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

/// Checks the `member` is **not** in the `set`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_member_not_in_set<V>(
    member: &V,
    set: &HashSet<V>,
    member_name: &str,
    set_name: &str,
) -> anyhow::Result<()>
where
    V: Hash + Eq + Display + Clone,
{
    if set.contains(member) {
        anyhow::bail!(
            "the '{member_name}' member was already in the '{set_name}' set `&<{}>`",
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

/// Checks the `member` is in the `set`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_member_in_set<V>(
    member: &V,
    set: &HashSet<V>,
    member_name: &str,
    set_name: &str,
) -> anyhow::Result<()>
where
    V: Hash + Eq + Display + Clone,
{
    if !set.contains(member) {
        anyhow::bail!(
            "the '{member_name}' member was not in the '{set_name}' set `&<{}>`",
            std::any::type_name::<V>(),
        )
    }
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::fmt::Display;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(false, false)]
    #[case(true, true)]
    fn test_check_predicate_true(#[case] predicate: bool, #[case] expected: bool) {
        let result = check_predicate_true(predicate, "the predicate was false").is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(false, true)]
    #[case(true, false)]
    fn test_check_predicate_false(#[case] predicate: bool, #[case] expected: bool) {
        let result = check_predicate_false(predicate, "the predicate was true").is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("a")]
    #[case(" ")] // <-- whitespace is allowed
    #[case("  ")] // <-- multiple whitespace is allowed
    #[case("🦀")] // <-- non-ASCII is allowed
    #[case(" a")]
    #[case("a ")]
    #[case("abc")]
    fn test_check_nonempty_string_with_valid_values(#[case] s: &str) {
        assert!(check_nonempty_string(s, "value").is_ok());
    }

    #[rstest]
    #[case("")] // empty string
    fn test_check_nonempty_string_with_invalid_values(#[case] s: &str) {
        assert!(check_nonempty_string(s, "value").is_err());
    }

    #[rstest]
    #[case(" a")]
    #[case("a ")]
    #[case("a a")]
    #[case(" a ")]
    #[case("abc")]
    fn test_check_valid_string_with_valid_value(#[case] s: &str) {
        assert!(check_valid_string(s, "value").is_ok());
    }

    #[rstest]
    #[case("")] // <-- empty string
    #[case(" ")] // <-- whitespace-only
    #[case("  ")] // <-- whitespace-only string
    #[case("🦀")] // <-- contains non-ASCII char
    fn test_check_valid_string_with_invalid_values(#[case] s: &str) {
        assert!(check_valid_string(s, "value").is_err());
    }

    #[rstest]
    #[case(None)]
    #[case(Some(" a"))]
    #[case(Some("a "))]
    #[case(Some("a a"))]
    #[case(Some(" a "))]
    #[case(Some("abc"))]
    fn test_check_valid_string_optional_with_valid_value(#[case] s: Option<&str>) {
        assert!(check_valid_string_optional(s, "value").is_ok());
    }

    #[rstest]
    #[case("a", "a")]
    fn test_check_string_contains_when_does_contain(#[case] s: &str, #[case] pat: &str) {
        assert!(check_string_contains(s, pat, "value").is_ok());
    }

    #[rstest]
    #[case("a", "b")]
    fn test_check_string_contains_when_does_not_contain(#[case] s: &str, #[case] pat: &str) {
        assert!(check_string_contains(s, pat, "value").is_err());
    }

    #[rstest]
    #[case(0u8, 0u8, "left", "right", true)]
    #[case(1u8, 1u8, "left", "right", true)]
    #[case(0u8, 1u8, "left", "right", false)]
    #[case(1u8, 0u8, "left", "right", false)]
    #[case(10i32, 10i32, "left", "right", true)]
    #[case(10i32, 20i32, "left", "right", false)]
    #[case("hello", "hello", "left", "right", true)]
    #[case("hello", "world", "left", "right", false)]
    fn test_check_equal<T: PartialEq + Debug + Display>(
        #[case] lhs: T,
        #[case] rhs: T,
        #[case] lhs_param: &str,
        #[case] rhs_param: &str,
        #[case] expected: bool,
    ) {
        let result = check_equal(lhs, rhs, lhs_param, rhs_param).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0, 0, "left", "right", true)]
    #[case(1, 1, "left", "right", true)]
    #[case(0, 1, "left", "right", false)]
    #[case(1, 0, "left", "right", false)]
    fn test_check_equal_u8_when_equal(
        #[case] lhs: u8,
        #[case] rhs: u8,
        #[case] lhs_param: &str,
        #[case] rhs_param: &str,
        #[case] expected: bool,
    ) {
        let result = check_equal_u8(lhs, rhs, lhs_param, rhs_param).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0, 0, "left", "right", true)]
    #[case(1, 1, "left", "right", true)]
    #[case(0, 1, "left", "right", false)]
    #[case(1, 0, "left", "right", false)]
    fn test_check_equal_usize_when_equal(
        #[case] lhs: usize,
        #[case] rhs: usize,
        #[case] lhs_param: &str,
        #[case] rhs_param: &str,
        #[case] expected: bool,
    ) {
        let result = check_equal_usize(lhs, rhs, lhs_param, rhs_param).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(1, "value")]
    fn test_check_positive_u64_when_positive(#[case] value: u64, #[case] param: &str) {
        assert!(check_positive_u64(value, param).is_ok());
    }

    #[rstest]
    #[case(0, "value")]
    fn test_check_positive_u64_when_not_positive(#[case] value: u64, #[case] param: &str) {
        assert!(check_positive_u64(value, param).is_err());
    }

    #[rstest]
    #[case(1, "value")]
    fn test_check_positive_i64_when_positive(#[case] value: i64, #[case] param: &str) {
        assert!(check_positive_i64(value, param).is_ok());
    }

    #[rstest]
    #[case(0, "value")]
    #[case(-1, "value")]
    fn test_check_positive_i64_when_not_positive(#[case] value: i64, #[case] param: &str) {
        assert!(check_positive_i64(value, param).is_err());
    }

    #[rstest]
    #[case(0.0, "value")]
    #[case(1.0, "value")]
    fn test_check_non_negative_f64_when_not_negative(#[case] value: f64, #[case] param: &str) {
        assert!(check_non_negative_f64(value, param).is_ok());
    }

    #[rstest]
    #[case(f64::NAN, "value")]
    #[case(f64::INFINITY, "value")]
    #[case(f64::NEG_INFINITY, "value")]
    #[case(-0.1, "value")]
    fn test_check_non_negative_f64_when_negative(#[case] value: f64, #[case] param: &str) {
        assert!(check_non_negative_f64(value, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_u8_when_in_range(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] desc: &str,
    ) {
        assert!(check_in_range_inclusive_u8(value, l, r, desc).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_u8_when_out_of_range(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_u8(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_u64_when_in_range(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_u64(value, l, r, param).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_u64_when_out_of_range(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_u64(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_i64_when_in_range(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_i64(value, l, r, param).is_ok());
    }

    #[rstest]
    #[case(0.0, 0.0, 0.0, "value")]
    #[case(0.0, 0.0, 1.0, "value")]
    #[case(1.0, 0.0, 1.0, "value")]
    fn test_check_in_range_inclusive_f64_when_in_range(
        #[case] value: f64,
        #[case] l: f64,
        #[case] r: f64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_f64(value, l, r, param).is_ok());
    }

    #[rstest]
    #[case(-1e16, 0.0, 0.0, "value")]
    #[case(1.0 + 1e16, 0.0, 1.0, "value")]
    fn test_check_in_range_inclusive_f64_when_out_of_range(
        #[case] value: f64,
        #[case] l: f64,
        #[case] r: f64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_f64(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_i64_when_out_of_range(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_i64(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_usize_when_in_range(
        #[case] value: usize,
        #[case] l: usize,
        #[case] r: usize,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_usize(value, l, r, param).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_usize_when_out_of_range(
        #[case] value: usize,
        #[case] l: usize,
        #[case] r: usize,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_usize(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(vec![], true)]
    #[case(vec![1_u8], false)]
    fn test_check_slice_empty(#[case] collection: Vec<u8>, #[case] expected: bool) {
        let result = check_slice_empty(collection.as_slice(), "param").is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(vec![], false)]
    #[case(vec![1_u8], true)]
    fn test_check_slice_not_empty(#[case] collection: Vec<u8>, #[case] expected: bool) {
        let result = check_slice_not_empty(collection.as_slice(), "param").is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(HashMap::new(), true)]
    #[case(HashMap::from([("A".to_string(), 1_u8)]), false)]
    fn test_check_map_empty(#[case] map: HashMap<String, u8>, #[case] expected: bool) {
        let result = check_map_empty(&map, "param").is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(HashMap::new(), false)]
    #[case(HashMap::from([("A".to_string(), 1_u8)]), true)]
    fn test_check_map_not_empty(#[case] map: HashMap<String, u8>, #[case] expected: bool) {
        let result = check_map_not_empty(&map, "param").is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(&HashMap::<u32, u32>::new(), 5, "key", "map", true)] // empty map
    #[case(&HashMap::from([(1, 10), (2, 20)]), 1, "key", "map", false)] // key exists
    #[case(&HashMap::from([(1, 10), (2, 20)]), 5, "key", "map", true)] // key doesn't exist
    fn test_check_key_not_in_map(
        #[case] map: &HashMap<u32, u32>,
        #[case] key: u32,
        #[case] key_name: &str,
        #[case] map_name: &str,
        #[case] expected: bool,
    ) {
        let result = check_key_not_in_map(&key, map, key_name, map_name).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(&HashMap::<u32, u32>::new(), 5, "key", "map", false)] // empty map
    #[case(&HashMap::from([(1, 10), (2, 20)]), 1, "key", "map", true)] // key exists
    #[case(&HashMap::from([(1, 10), (2, 20)]), 5, "key", "map", false)] // key doesn't exist
    fn test_check_key_in_map(
        #[case] map: &HashMap<u32, u32>,
        #[case] key: u32,
        #[case] key_name: &str,
        #[case] map_name: &str,
        #[case] expected: bool,
    ) {
        let result = check_key_in_map(&key, map, key_name, map_name).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(&IndexMap::<u32, u32>::new(), 5, "key", "map", true)] // empty map
    #[case(&IndexMap::from([(1, 10), (2, 20)]), 1, "key", "map", false)] // key exists
    #[case(&IndexMap::from([(1, 10), (2, 20)]), 5, "key", "map", true)] // key doesn't exist
    fn test_check_key_not_in_index_map(
        #[case] map: &IndexMap<u32, u32>,
        #[case] key: u32,
        #[case] key_name: &str,
        #[case] map_name: &str,
        #[case] expected: bool,
    ) {
        let result = check_key_not_in_index_map(&key, map, key_name, map_name).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(&IndexMap::<u32, u32>::new(), 5, "key", "map", false)] // empty map
    #[case(&IndexMap::from([(1, 10), (2, 20)]), 1, "key", "map", true)] // key exists
    #[case(&IndexMap::from([(1, 10), (2, 20)]), 5, "key", "map", false)] // key doesn't exist
    fn test_check_key_in_index_map(
        #[case] map: &IndexMap<u32, u32>,
        #[case] key: u32,
        #[case] key_name: &str,
        #[case] map_name: &str,
        #[case] expected: bool,
    ) {
        let result = check_key_in_index_map(&key, map, key_name, map_name).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(&HashSet::<u32>::new(), 5, "member", "set", true)] // Empty set
    #[case(&HashSet::from([1, 2]), 1, "member", "set", false)] // Member exists
    #[case(&HashSet::from([1, 2]), 5, "member", "set", true)] // Member doesn't exist
    fn test_check_member_not_in_set(
        #[case] set: &HashSet<u32>,
        #[case] member: u32,
        #[case] member_name: &str,
        #[case] set_name: &str,
        #[case] expected: bool,
    ) {
        let result = check_member_not_in_set(&member, set, member_name, set_name).is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(&HashSet::<u32>::new(), 5, "member", "set", false)] // Empty set
    #[case(&HashSet::from([1, 2]), 1, "member", "set", true)] // Member exists
    #[case(&HashSet::from([1, 2]), 5, "member", "set", false)] // Member doesn't exist
    fn test_check_member_in_set(
        #[case] set: &HashSet<u32>,
        #[case] member: u32,
        #[case] member_name: &str,
        #[case] set_name: &str,
        #[case] expected: bool,
    ) {
        let result = check_member_in_set(&member, set, member_name, set_name).is_ok();
        assert_eq!(result, expected);
    }
}

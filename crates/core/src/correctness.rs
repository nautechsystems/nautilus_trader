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

//! Functions for correctness checks similar to the *design by contract* philosophy.
//!
//! This module provides validation checking of function or method conditions.
//!
//! A condition is a predicate which must be true just prior to the execution of
//! some section of code - for correct behavior as per the design specification.
//!
//! A typed [`Result`] is returned with a descriptive message when the condition
//! check fails.

use std::fmt::{Debug, Display};

use rust_decimal::Decimal;
use thiserror::Error;

use crate::collections::{MapLike, SetLike};

/// A message prefix that can be used with calls to `expect` or other assertion-related functions.
///
/// This constant provides a standard message that can be used to indicate a failure condition
/// when a predicate or condition does not hold true. It is typically used in conjunction with
/// functions like `expect` to provide a consistent error message.
pub const FAILED: &str = "Condition failed";

/// Error type for correctness checks.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CorrectnessError {
    /// A predicate or invariant check failed.
    #[error("{message}")]
    PredicateViolation {
        /// The failure message.
        message: String,
    },
    /// A string was empty.
    #[error("invalid string for '{param}', was empty")]
    EmptyString {
        /// The parameter name.
        param: String,
    },
    /// A string was all whitespace.
    #[error("invalid string for '{param}', was all whitespace")]
    WhitespaceString {
        /// The parameter name.
        param: String,
    },
    /// A string contained a non-ASCII character.
    #[error("invalid string for '{param}' contained a non-ASCII char, was '{value}'")]
    NonAsciiString {
        /// The parameter name.
        param: String,
        /// The provided value.
        value: String,
    },
    /// A string did not contain an expected pattern.
    #[error("invalid string for '{param}' did not contain '{pattern}', was '{value}'")]
    MissingSubstring {
        /// The parameter name.
        param: String,
        /// The expected substring.
        pattern: String,
        /// The provided value.
        value: String,
    },
    /// Two values were not equal.
    #[error(
        "'{lhs_param}' {type_name} of {lhs} was not equal to '{rhs_param}' {type_name} of {rhs}"
    )]
    EqualityMismatch {
        /// The left parameter name.
        lhs_param: String,
        /// The right parameter name.
        rhs_param: String,
        /// The left value.
        lhs: String,
        /// The right value.
        rhs: String,
        /// The displayed type name.
        type_name: &'static str,
    },
    /// A value that must be positive was not positive.
    #[error("invalid {type_name} for '{param}' not positive, was {value}")]
    NotPositive {
        /// The parameter name.
        param: String,
        /// The provided value.
        value: String,
        /// The displayed type name.
        type_name: &'static str,
    },
    /// A value that must not be negative was negative.
    #[error("invalid {type_name} for '{param}' negative, was {value}")]
    NegativeValue {
        /// The parameter name.
        param: String,
        /// The provided value.
        value: String,
        /// The displayed type name.
        type_name: &'static str,
    },
    /// A value was invalid for its type.
    #[error("invalid {type_name} for '{param}', was {value}")]
    InvalidValue {
        /// The parameter name.
        param: String,
        /// The provided value.
        value: String,
        /// The displayed type name.
        type_name: &'static str,
    },
    /// A value was outside an inclusive range.
    #[error("invalid {type_name} for '{param}' not in range [{min}, {max}], was {value}")]
    OutOfRange {
        /// The parameter name.
        param: String,
        /// The lower bound.
        min: String,
        /// The upper bound.
        max: String,
        /// The provided value.
        value: String,
        /// The displayed type name.
        type_name: &'static str,
    },
    /// A collection that must be empty was not empty.
    #[error("the '{param}' {collection_kind} `{type_repr}` was not empty")]
    CollectionNotEmpty {
        /// The parameter name.
        param: String,
        /// The collection kind.
        collection_kind: &'static str,
        /// The collection type representation.
        type_repr: String,
    },
    /// A collection that must not be empty was empty.
    #[error("the '{param}' {collection_kind} `{type_repr}` was empty")]
    CollectionEmpty {
        /// The parameter name.
        param: String,
        /// The collection kind.
        collection_kind: &'static str,
        /// The collection type representation.
        type_repr: String,
    },
    /// A map key was already present.
    #[error("the '{key_name}' key {key} was already in the '{map_name}' map `{map_type_repr}`")]
    KeyPresent {
        /// The key parameter name.
        key_name: String,
        /// The map parameter name.
        map_name: String,
        /// The key value.
        key: String,
        /// The map type representation.
        map_type_repr: String,
    },
    /// A map key was missing.
    #[error("the '{key_name}' key {key} was not in the '{map_name}' map `{map_type_repr}`")]
    KeyMissing {
        /// The key parameter name.
        key_name: String,
        /// The map parameter name.
        map_name: String,
        /// The key value.
        key: String,
        /// The map type representation.
        map_type_repr: String,
    },
    /// A set member was already present.
    #[error("the '{member_name}' member was already in the '{set_name}' set `{set_type_repr}`")]
    MemberPresent {
        /// The member parameter name.
        member_name: String,
        /// The set parameter name.
        set_name: String,
        /// The set type representation.
        set_type_repr: String,
    },
    /// A set member was missing.
    #[error("the '{member_name}' member was not in the '{set_name}' set `{set_type_repr}`")]
    MemberMissing {
        /// The member parameter name.
        member_name: String,
        /// The set parameter name.
        set_name: String,
        /// The set type representation.
        set_type_repr: String,
    },
}

/// Result type for correctness checks.
pub type Result<T> = std::result::Result<T, CorrectnessError>;

/// Result type alias for APIs that want to name the correctness error domain explicitly.
pub type CorrectnessResult<T> = Result<T>;

/// Extension trait for [`CorrectnessResult`] that panics with the error's
/// [`Display`] form rather than its `Debug` form.
///
/// Use this instead of [`std::result::Result::expect`] when unwrapping a
/// correctness result: `expect` formats the error with `{:?}`, which exposes
/// the internal [`CorrectnessError`] struct layout in panic output, while
/// [`CorrectnessResultExt::expect_display`] preserves the human-readable
/// message defined on each variant.
pub trait CorrectnessResultExt<T> {
    /// Returns the contained [`Ok`] value, panicking with `msg: <error display>`
    /// on [`Err`].
    fn expect_display(self, msg: &str) -> T;
}

impl<T> CorrectnessResultExt<T> for CorrectnessResult<T> {
    #[inline]
    #[track_caller]
    fn expect_display(self, msg: &str) -> T {
        match self {
            Ok(value) => value,
            Err(e) => panic!("{msg}: {e}"),
        }
    }
}

/// Checks the `predicate` is true.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_predicate_true(predicate: bool, fail_msg: &str) -> Result<()> {
    if !predicate {
        return Err(CorrectnessError::PredicateViolation {
            message: fail_msg.to_string(),
        });
    }
    Ok(())
}

/// Checks the `predicate` is false.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_predicate_false(predicate: bool, fail_msg: &str) -> Result<()> {
    if predicate {
        return Err(CorrectnessError::PredicateViolation {
            message: fail_msg.to_string(),
        });
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
/// Returns an error if `s` is empty.
#[inline(always)]
pub fn check_nonempty_string<T: AsRef<str>>(s: T, param: &str) -> Result<()> {
    if s.as_ref().is_empty() {
        return Err(CorrectnessError::EmptyString {
            param: param.to_string(),
        });
    }
    Ok(())
}

/// Checks the string `s` has semantic meaning and contains only ASCII characters.
///
/// # Errors
///
/// Returns an error if:
/// - `s` is an empty string.
/// - `s` consists solely of whitespace characters.
/// - `s` contains one or more non-ASCII characters.
#[inline(always)]
pub fn check_valid_string_ascii<T: AsRef<str>>(s: T, param: &str) -> Result<()> {
    let s = s.as_ref();

    if s.is_empty() {
        return Err(CorrectnessError::EmptyString {
            param: param.to_string(),
        });
    }

    // Ensure string is only traversed once
    let mut has_non_whitespace = false;

    for c in s.chars() {
        if !c.is_whitespace() {
            has_non_whitespace = true;
        }

        if !c.is_ascii() {
            return Err(CorrectnessError::NonAsciiString {
                param: param.to_string(),
                value: s.to_string(),
            });
        }
    }

    if !has_non_whitespace {
        return Err(CorrectnessError::WhitespaceString {
            param: param.to_string(),
        });
    }

    Ok(())
}

/// Checks the string `s` has semantic meaning and allows UTF-8 characters.
///
/// This is a relaxed version of [`check_valid_string_ascii`] that permits non-ASCII UTF-8 characters.
/// Use this for external identifiers (e.g., exchange symbols) that may contain Unicode characters.
///
/// # Errors
///
/// Returns an error if:
/// - `s` is an empty string.
/// - `s` consists solely of whitespace characters.
#[inline(always)]
pub fn check_valid_string_utf8<T: AsRef<str>>(s: T, param: &str) -> Result<()> {
    let s = s.as_ref();

    if s.is_empty() {
        return Err(CorrectnessError::EmptyString {
            param: param.to_string(),
        });
    }

    let has_non_whitespace = s.chars().any(|c| !c.is_whitespace());

    if !has_non_whitespace {
        return Err(CorrectnessError::WhitespaceString {
            param: param.to_string(),
        });
    }

    Ok(())
}

/// Checks the string `s` if Some, contains only ASCII characters and has semantic meaning.
///
/// # Errors
///
/// Returns an error if:
/// - `s` is an empty string.
/// - `s` consists solely of whitespace characters.
/// - `s` contains one or more non-ASCII characters.
#[inline(always)]
pub fn check_valid_string_ascii_optional<T: AsRef<str>>(s: Option<T>, param: &str) -> Result<()> {
    if let Some(s) = s {
        check_valid_string_ascii(s, param)?;
    }
    Ok(())
}

/// Checks the string `s` contains the pattern `pat`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_string_contains<T: AsRef<str>>(s: T, pat: &str, param: &str) -> Result<()> {
    let s = s.as_ref();
    if !s.contains(pat) {
        return Err(CorrectnessError::MissingSubstring {
            param: param.to_string(),
            pattern: pat.to_string(),
            value: s.to_string(),
        });
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
    lhs: &T,
    rhs: &T,
    lhs_param: &str,
    rhs_param: &str,
) -> Result<()> {
    if lhs != rhs {
        return Err(CorrectnessError::EqualityMismatch {
            lhs_param: lhs_param.to_string(),
            rhs_param: rhs_param.to_string(),
            lhs: lhs.to_string(),
            rhs: rhs.to_string(),
            type_name: "value",
        });
    }
    Ok(())
}

/// Checks the `u8` values are equal.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_equal_u8(lhs: u8, rhs: u8, lhs_param: &str, rhs_param: &str) -> Result<()> {
    if lhs != rhs {
        return Err(CorrectnessError::EqualityMismatch {
            lhs_param: lhs_param.to_string(),
            rhs_param: rhs_param.to_string(),
            lhs: lhs.to_string(),
            rhs: rhs.to_string(),
            type_name: "u8",
        });
    }
    Ok(())
}

/// Checks the `usize` values are equal.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_equal_usize(lhs: usize, rhs: usize, lhs_param: &str, rhs_param: &str) -> Result<()> {
    if lhs != rhs {
        return Err(CorrectnessError::EqualityMismatch {
            lhs_param: lhs_param.to_string(),
            rhs_param: rhs_param.to_string(),
            lhs: lhs.to_string(),
            rhs: rhs.to_string(),
            type_name: "usize",
        });
    }
    Ok(())
}

/// Checks the `u64` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_u64(value: u64, param: &str) -> Result<()> {
    if value == 0 {
        return Err(CorrectnessError::NotPositive {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "u64",
        });
    }
    Ok(())
}

/// Checks the `u128` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_u128(value: u128, param: &str) -> Result<()> {
    if value == 0 {
        return Err(CorrectnessError::NotPositive {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "u128",
        });
    }
    Ok(())
}

/// Checks the `i64` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_i64(value: i64, param: &str) -> Result<()> {
    if value <= 0 {
        return Err(CorrectnessError::NotPositive {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "i64",
        });
    }
    Ok(())
}

/// Checks the `i64` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_i128(value: i128, param: &str) -> Result<()> {
    if value <= 0 {
        return Err(CorrectnessError::NotPositive {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "i128",
        });
    }
    Ok(())
}

/// Checks the `f64` value is non-negative (>= 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_non_negative_f64(value: f64, param: &str) -> Result<()> {
    if value.is_nan() || value.is_infinite() {
        return Err(CorrectnessError::InvalidValue {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "f64",
        });
    }

    if value < 0.0 {
        return Err(CorrectnessError::NegativeValue {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "f64",
        });
    }
    Ok(())
}

/// Checks the `u8` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_u8(value: u8, l: u8, r: u8, param: &str) -> Result<()> {
    if value < l || value > r {
        return Err(CorrectnessError::OutOfRange {
            param: param.to_string(),
            min: l.to_string(),
            max: r.to_string(),
            value: value.to_string(),
            type_name: "u8",
        });
    }
    Ok(())
}

/// Checks the `u64` value is range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_u64(value: u64, l: u64, r: u64, param: &str) -> Result<()> {
    if value < l || value > r {
        return Err(CorrectnessError::OutOfRange {
            param: param.to_string(),
            min: l.to_string(),
            max: r.to_string(),
            value: value.to_string(),
            type_name: "u64",
        });
    }
    Ok(())
}

/// Checks the `i64` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_i64(value: i64, l: i64, r: i64, param: &str) -> Result<()> {
    if value < l || value > r {
        return Err(CorrectnessError::OutOfRange {
            param: param.to_string(),
            min: l.to_string(),
            max: r.to_string(),
            value: value.to_string(),
            type_name: "i64",
        });
    }
    Ok(())
}

/// Checks the `f64` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_f64(value: f64, l: f64, r: f64, param: &str) -> Result<()> {
    // Hardcoded epsilon is intentional and appropriate here because:
    // - 1e-15 is conservative for IEEE 754 double precision (machine epsilon ~2.22e-16)
    // - This function is used for validation, not high-precision calculations
    // - The epsilon prevents spurious failures due to floating-point representation
    // - Making it configurable would complicate the API for minimal benefit
    const EPSILON: f64 = 1e-15;

    if value.is_nan() || value.is_infinite() {
        return Err(CorrectnessError::InvalidValue {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "f64",
        });
    }

    if value < l - EPSILON || value > r + EPSILON {
        return Err(CorrectnessError::OutOfRange {
            param: param.to_string(),
            min: l.to_string(),
            max: r.to_string(),
            value: value.to_string(),
            type_name: "f64",
        });
    }
    Ok(())
}

/// Checks the `usize` value is in range [`l`, `r`] (inclusive).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_in_range_inclusive_usize(value: usize, l: usize, r: usize, param: &str) -> Result<()> {
    if value < l || value > r {
        return Err(CorrectnessError::OutOfRange {
            param: param.to_string(),
            min: l.to_string(),
            max: r.to_string(),
            value: value.to_string(),
            type_name: "usize",
        });
    }
    Ok(())
}

/// Checks the slice is empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_slice_empty<T>(slice: &[T], param: &str) -> Result<()> {
    if !slice.is_empty() {
        return Err(CorrectnessError::CollectionNotEmpty {
            param: param.to_string(),
            collection_kind: "slice",
            type_repr: slice_type_repr::<T>(),
        });
    }
    Ok(())
}

/// Checks the slice is **not** empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_slice_not_empty<T>(slice: &[T], param: &str) -> Result<()> {
    if slice.is_empty() {
        return Err(CorrectnessError::CollectionEmpty {
            param: param.to_string(),
            collection_kind: "slice",
            type_repr: slice_type_repr::<T>(),
        });
    }
    Ok(())
}

/// Checks the hashmap is empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_map_empty<M>(map: &M, param: &str) -> Result<()>
where
    M: MapLike,
{
    if !map.is_empty() {
        return Err(CorrectnessError::CollectionNotEmpty {
            param: param.to_string(),
            collection_kind: "map",
            type_repr: map_type_repr::<M>(),
        });
    }
    Ok(())
}

/// Checks the map is **not** empty.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_map_not_empty<M>(map: &M, param: &str) -> Result<()>
where
    M: MapLike,
{
    if map.is_empty() {
        return Err(CorrectnessError::CollectionEmpty {
            param: param.to_string(),
            collection_kind: "map",
            type_repr: map_type_repr::<M>(),
        });
    }
    Ok(())
}

/// Checks the `key` is **not** in the `map`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_key_not_in_map<M>(key: &M::Key, map: &M, key_name: &str, map_name: &str) -> Result<()>
where
    M: MapLike,
{
    if map.contains_key(key) {
        return Err(CorrectnessError::KeyPresent {
            key_name: key_name.to_string(),
            map_name: map_name.to_string(),
            key: key.to_string(),
            map_type_repr: map_type_repr::<M>(),
        });
    }
    Ok(())
}

/// Checks the `key` is in the `map`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_key_in_map<M>(key: &M::Key, map: &M, key_name: &str, map_name: &str) -> Result<()>
where
    M: MapLike,
{
    if !map.contains_key(key) {
        return Err(CorrectnessError::KeyMissing {
            key_name: key_name.to_string(),
            map_name: map_name.to_string(),
            key: key.to_string(),
            map_type_repr: map_type_repr::<M>(),
        });
    }
    Ok(())
}

/// Checks the `member` is **not** in the `set`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_member_not_in_set<S>(
    member: &S::Item,
    set: &S,
    member_name: &str,
    set_name: &str,
) -> Result<()>
where
    S: SetLike,
{
    if set.contains(member) {
        return Err(CorrectnessError::MemberPresent {
            member_name: member_name.to_string(),
            set_name: set_name.to_string(),
            set_type_repr: set_type_repr::<S>(),
        });
    }
    Ok(())
}

/// Checks the `member` is in the `set`.
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_member_in_set<S>(
    member: &S::Item,
    set: &S,
    member_name: &str,
    set_name: &str,
) -> Result<()>
where
    S: SetLike,
{
    if !set.contains(member) {
        return Err(CorrectnessError::MemberMissing {
            member_name: member_name.to_string(),
            set_name: set_name.to_string(),
            set_type_repr: set_type_repr::<S>(),
        });
    }
    Ok(())
}

/// Checks the `Decimal` value is positive (> 0).
///
/// # Errors
///
/// Returns an error if the validation check fails.
#[inline(always)]
pub fn check_positive_decimal(value: Decimal, param: &str) -> Result<()> {
    if value <= Decimal::ZERO {
        return Err(CorrectnessError::NotPositive {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "Decimal",
        });
    }
    Ok(())
}

fn slice_type_repr<T>() -> String {
    format!("&[{}]", std::any::type_name::<T>())
}

fn map_type_repr<M>() -> String
where
    M: MapLike,
{
    format!(
        "&<{}, {}>",
        std::any::type_name::<M::Key>(),
        std::any::type_name::<M::Value>(),
    )
}

fn set_type_repr<S>() -> String
where
    S: SetLike,
{
    format!("&<{}>", std::any::type_name::<S::Item>())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        fmt::Display,
        str::FromStr,
    };

    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_check_predicate_true_returns_typed_error_with_stable_display() {
        let error = check_predicate_true(false, "the predicate was false").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::PredicateViolation {
                message: "the predicate was false".to_string(),
            }
        );
        assert_eq!(error.to_string(), "the predicate was false");
    }

    #[rstest]
    fn test_expect_display_returns_ok_value() {
        let result: CorrectnessResult<i32> = Ok(42);
        assert_eq!(result.expect_display(FAILED), 42);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: invalid string for 'value', was empty")]
    fn test_expect_display_panics_with_display_form_on_err() {
        let result: CorrectnessResult<()> = Err(CorrectnessError::EmptyString {
            param: "value".to_string(),
        });
        result.expect_display(FAILED);
    }

    #[rstest]
    #[should_panic(expected = "custom prefix: the predicate was false")]
    fn test_expect_display_uses_provided_prefix() {
        let result: CorrectnessResult<()> = Err(CorrectnessError::PredicateViolation {
            message: "the predicate was false".to_string(),
        });
        result.expect_display("custom prefix");
    }

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
    fn test_check_valid_string_ascii_with_valid_value(#[case] s: &str) {
        assert!(check_valid_string_ascii(s, "value").is_ok());
    }

    #[rstest]
    #[case("")] // <-- empty string
    #[case(" ")] // <-- whitespace-only
    #[case("  ")] // <-- whitespace-only string
    #[case("🦀")] // <-- contains non-ASCII char
    fn test_check_valid_string_ascii_with_invalid_values(#[case] s: &str) {
        assert!(check_valid_string_ascii(s, "value").is_err());
    }

    #[rstest]
    fn test_check_valid_string_ascii_returns_empty_string_error_with_stable_display() {
        let error = check_valid_string_ascii("", "value").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::EmptyString {
                param: "value".to_string(),
            }
        );
        assert_eq!(error.to_string(), "invalid string for 'value', was empty");
    }

    #[rstest]
    fn test_check_valid_string_ascii_returns_non_ascii_error_with_stable_display() {
        let error = check_valid_string_ascii("🦀", "value").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::NonAsciiString {
                param: "value".to_string(),
                value: "🦀".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "invalid string for 'value' contained a non-ASCII char, was '🦀'"
        );
    }

    #[rstest]
    fn test_check_valid_string_ascii_returns_whitespace_string_error_with_stable_display() {
        let error = check_valid_string_ascii("   ", "value").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::WhitespaceString {
                param: "value".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "invalid string for 'value', was all whitespace"
        );
    }

    #[rstest]
    #[case(" a")]
    #[case("a ")]
    #[case("abc")]
    #[case("ETHUSDT")]
    fn test_check_valid_string_utf8_with_valid_values(#[case] s: &str) {
        assert!(check_valid_string_utf8(s, "value").is_ok());
    }

    #[rstest]
    #[case("")] // <-- empty string
    #[case(" ")] // <-- whitespace-only
    #[case("  ")] // <-- whitespace-only string
    fn test_check_valid_string_utf8_with_invalid_values(#[case] s: &str) {
        assert!(check_valid_string_utf8(s, "value").is_err());
    }

    #[rstest]
    #[case(None)]
    #[case(Some(" a"))]
    #[case(Some("a "))]
    #[case(Some("a a"))]
    #[case(Some(" a "))]
    #[case(Some("abc"))]
    fn test_check_valid_string_ascii_optional_with_valid_value(#[case] s: Option<&str>) {
        assert!(check_valid_string_ascii_optional(s, "value").is_ok());
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
        let result = check_equal(&lhs, &rhs, lhs_param, rhs_param).is_ok();
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
    fn test_check_equal_u8_returns_equality_mismatch_with_stable_display() {
        let error = check_equal_u8(1, 2, "left", "right").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::EqualityMismatch {
                lhs_param: "left".to_string(),
                rhs_param: "right".to_string(),
                lhs: "1".to_string(),
                rhs: "2".to_string(),
                type_name: "u8",
            }
        );
        assert_eq!(
            error.to_string(),
            "'left' u8 of 1 was not equal to 'right' u8 of 2"
        );
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
    fn test_check_in_range_inclusive_usize_returns_out_of_range_error_with_stable_display() {
        let error = check_in_range_inclusive_usize(3, 1, 2, "value").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::OutOfRange {
                param: "value".to_string(),
                min: "1".to_string(),
                max: "2".to_string(),
                value: "3".to_string(),
                type_name: "usize",
            }
        );
        assert_eq!(
            error.to_string(),
            "invalid usize for 'value' not in range [1, 2], was 3"
        );
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
    fn test_check_slice_not_empty_returns_collection_empty_error_with_stable_display() {
        let error = check_slice_not_empty::<u8>(&[], "param").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::CollectionEmpty {
                param: "param".to_string(),
                collection_kind: "slice",
                type_repr: "&[u8]".to_string(),
            }
        );
        assert_eq!(error.to_string(), "the 'param' slice `&[u8]` was empty");
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
    fn test_check_key_in_map_returns_key_missing_error_with_stable_display() {
        let map = HashMap::<u32, u32>::new();
        let error = check_key_in_map(&5, &map, "key", "map").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::KeyMissing {
                key_name: "key".to_string(),
                map_name: "map".to_string(),
                key: "5".to_string(),
                map_type_repr: "&<u32, u32>".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "the 'key' key 5 was not in the 'map' map `&<u32, u32>`"
        );
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

    #[rstest]
    #[case("1", true)] // simple positive integer
    #[case("0.0000000000000000000000000001", true)] // smallest positive (1 × 10⁻²⁸)
    #[case("79228162514264337593543950335", true)] // very large positive (≈ Decimal::MAX)
    #[case("0", false)] // zero should fail
    #[case("-0.0000000000000000000000000001", false)] // tiny negative
    #[case("-1", false)] // simple negative integer
    fn test_check_positive_decimal(#[case] raw: &str, #[case] expected: bool) {
        let value = Decimal::from_str(raw).expect("valid decimal literal");
        let result = super::check_positive_decimal(value, "param").is_ok();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(1, true)]
    #[case(u128::MAX, true)]
    #[case(0, false)]
    fn test_check_positive_u128(#[case] value: u128, #[case] expected: bool) {
        assert_eq!(check_positive_u128(value, "value").is_ok(), expected);
    }

    #[rstest]
    #[case(1, true)]
    #[case(i128::MAX, true)]
    #[case(0, false)]
    #[case(-1, false)]
    #[case(i128::MIN, false)]
    fn test_check_positive_i128(#[case] value: i128, #[case] expected: bool) {
        assert_eq!(check_positive_i128(value, "value").is_ok(), expected);
    }

    #[rstest]
    fn test_check_positive_decimal_returns_not_positive_error_with_stable_display() {
        let error = check_positive_decimal(Decimal::ZERO, "param").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::NotPositive {
                param: "param".to_string(),
                value: "0".to_string(),
                type_name: "Decimal",
            }
        );
        assert_eq!(
            error.to_string(),
            "invalid Decimal for 'param' not positive, was 0"
        );
    }
}

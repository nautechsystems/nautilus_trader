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

//! Common serialization traits and functions.
//!
//! This module provides custom serde deserializers and serializers for common
//! patterns encountered when parsing exchange API responses, particularly:
//!
//! - Empty strings that should be interpreted as `None` or zero.
//! - Type conversions from strings to primitives.
//! - Decimal values represented as strings.

use std::str::FromStr;

use bytes::Bytes;
use rust_decimal::Decimal;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error, Unexpected, Visitor},
    ser::SerializeSeq,
};
use ustr::Ustr;

/// Sorted serialization for `AHashSet<T>` where element order must be deterministic.
///
/// Use with `#[serde(with = "nautilus_core::serialization::sorted_hashset")]`.
pub mod sorted_hashset {
    use ahash::AHashSet;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes an `AHashSet<T>` as a sorted array for deterministic output.
    ///
    /// # Errors
    ///
    /// Returns any error produced by the underlying [`Serializer`] when writing
    /// the sorted vector.
    pub fn serialize<T, S>(set: &AHashSet<T>, s: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize + Ord,
        S: Serializer,
    {
        let mut sorted: Vec<&T> = set.iter().collect();
        sorted.sort_unstable();
        sorted.serialize(s)
    }

    /// Deserializes an array into an `AHashSet<T>`.
    ///
    /// # Errors
    ///
    /// Returns any error produced by the underlying [`Deserializer`] when reading
    /// the source array.
    pub fn deserialize<'de, T, D>(d: D) -> Result<AHashSet<T>, D::Error>
    where
        T: Deserialize<'de> + Eq + std::hash::Hash,
        D: Deserializer<'de>,
    {
        let vec = Vec::<T>::deserialize(d)?;
        Ok(vec.into_iter().collect())
    }
}

struct BoolVisitor;

/// Zero-allocation decimal visitor for maximum deserialization performance.
///
/// Directly visits JSON tokens without intermediate `serde_json::Value` allocation.
/// Handles all JSON numeric representations: strings, integers, floats, and null.
struct DecimalVisitor;

impl Visitor<'_> for DecimalVisitor {
    type Value = Decimal;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a decimal number as string, integer, or float")
    }

    // Fast path: borrowed string (zero-copy)
    fn visit_str<E: Error>(self, v: &str) -> Result<Self::Value, E> {
        if v.is_empty() {
            return Ok(Decimal::ZERO);
        }
        // Check for scientific notation
        if v.contains('e') || v.contains('E') {
            Decimal::from_scientific(v).map_err(E::custom)
        } else {
            Decimal::from_str(v).map_err(E::custom)
        }
    }

    // Owned string (rare case, delegates to visit_str)
    fn visit_string<E: Error>(self, v: String) -> Result<Self::Value, E> {
        self.visit_str(&v)
    }

    // Direct integer handling - no string conversion needed
    fn visit_i64<E: Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Decimal::from(v))
    }

    fn visit_u64<E: Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Decimal::from(v))
    }

    fn visit_i128<E: Error>(self, v: i128) -> Result<Self::Value, E> {
        Ok(Decimal::from(v))
    }

    fn visit_u128<E: Error>(self, v: u128) -> Result<Self::Value, E> {
        Ok(Decimal::from(v))
    }

    // Float handling - direct conversion
    fn visit_f64<E: Error>(self, v: f64) -> Result<Self::Value, E> {
        if v.is_nan() {
            return Err(E::invalid_value(Unexpected::Float(v), &self));
        }

        if v.is_infinite() {
            return Err(E::invalid_value(Unexpected::Float(v), &self));
        }
        Decimal::try_from(v).map_err(E::custom)
    }

    // Null → zero (matches existing behavior)
    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(Decimal::ZERO)
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(Decimal::ZERO)
    }
}

/// Zero-allocation optional decimal visitor for maximum deserialization performance.
///
/// Handles null values as `None` and empty strings as `None`.
/// Uses `deserialize_any` approach to handle all JSON value types uniformly.
struct OptionalDecimalVisitor;

impl Visitor<'_> for OptionalDecimalVisitor {
    type Value = Option<Decimal>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("null or a decimal number as string, integer, or float")
    }

    // Fast path: borrowed string (zero-copy)
    // Empty string → None (different from DecimalVisitor which returns ZERO)
    fn visit_str<E: Error>(self, v: &str) -> Result<Self::Value, E> {
        if v.is_empty() {
            return Ok(None);
        }
        DecimalVisitor.visit_str(v).map(Some)
    }

    fn visit_string<E: Error>(self, v: String) -> Result<Self::Value, E> {
        self.visit_str(&v)
    }

    fn visit_i64<E: Error>(self, v: i64) -> Result<Self::Value, E> {
        DecimalVisitor.visit_i64(v).map(Some)
    }

    fn visit_u64<E: Error>(self, v: u64) -> Result<Self::Value, E> {
        DecimalVisitor.visit_u64(v).map(Some)
    }

    fn visit_i128<E: Error>(self, v: i128) -> Result<Self::Value, E> {
        DecimalVisitor.visit_i128(v).map(Some)
    }

    fn visit_u128<E: Error>(self, v: u128) -> Result<Self::Value, E> {
        DecimalVisitor.visit_u128(v).map(Some)
    }

    fn visit_f64<E: Error>(self, v: f64) -> Result<Self::Value, E> {
        DecimalVisitor.visit_f64(v).map(Some)
    }

    // Null → None
    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
}

/// Represents types which are serializable for JSON specifications.
pub trait Serializable: Serialize + for<'de> Deserialize<'de> {
    /// Deserialize an object from JSON encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns serialization errors.
    fn from_json_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    /// Serialize an object to JSON encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns serialization errors.
    fn to_json_bytes(&self) -> Result<Bytes, serde_json::Error> {
        serde_json::to_vec(self).map(Bytes::from)
    }
}

pub use self::msgpack::{FromMsgPack, MsgPackSerializable, ToMsgPack};

/// Provides `MsgPack` serialization support for types implementing [`Serializable`].
///
/// This module contains traits for `MsgPack` serialization and deserialization,
/// separated from the core [`Serializable`] trait to allow independent opt-in.
pub mod msgpack {
    use bytes::Bytes;
    use serde::{Deserialize, Serialize};

    use super::Serializable;

    /// Provides deserialization from `MsgPack` encoded bytes.
    pub trait FromMsgPack: for<'de> Deserialize<'de> + Sized {
        /// Deserialize an object from `MsgPack` encoded bytes.
        ///
        /// # Errors
        ///
        /// Returns serialization errors.
        fn from_msgpack_bytes(data: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
            rmp_serde::from_slice(data)
        }
    }

    /// Provides serialization to `MsgPack` encoded bytes.
    pub trait ToMsgPack: Serialize {
        /// Serialize an object to `MsgPack` encoded bytes.
        ///
        /// # Errors
        ///
        /// Returns serialization errors.
        fn to_msgpack_bytes(&self) -> Result<Bytes, rmp_serde::encode::Error> {
            rmp_serde::to_vec_named(self).map(Bytes::from)
        }
    }

    /// Marker trait combining [`Serializable`], [`FromMsgPack`], and [`ToMsgPack`].
    ///
    /// This trait is automatically implemented for all types that implement [`Serializable`].
    pub trait MsgPackSerializable: Serializable + FromMsgPack + ToMsgPack {}

    impl<T> FromMsgPack for T where T: Serializable {}

    impl<T> ToMsgPack for T where T: Serializable {}

    impl<T> MsgPackSerializable for T where T: Serializable {}
}

impl Visitor<'_> for BoolVisitor {
    type Value = u8;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a boolean as u8")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(u8::from(value))
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "Intentional for parsing, value range validated"
    )]
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // Only 0 or 1 are considered valid representations when provided as an
        // integer. We deliberately reject values outside this range to avoid
        // silently truncating larger integers into impl-defined boolean
        // semantics.
        if value > 1 {
            Err(E::invalid_value(Unexpected::Unsigned(value), &self))
        } else {
            Ok(value as u8)
        }
    }
}

/// Serde default value function that returns `true`.
///
/// Use with `#[serde(default = "default_true")]` on boolean fields.
#[must_use]
pub const fn default_true() -> bool {
    true
}

/// Serde default value function that returns `false`.
///
/// Use with `#[serde(default = "default_false")]` on boolean fields.
#[must_use]
pub const fn default_false() -> bool {
    false
}

/// Deserialize the boolean value as a `u8`.
///
/// # Errors
///
/// Returns serialization errors.
pub fn from_bool_as_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(BoolVisitor)
}

/// Deserializes a `Decimal` from either a JSON string or number.
///
/// High-performance implementation using a custom visitor that avoids intermediate
/// `serde_json::Value` allocations. Handles all JSON numeric representations:
///
/// - JSON string: `"123.456"` → Decimal (zero-copy for borrowed strings)
/// - JSON integer: `123` → Decimal (direct conversion, no string allocation)
/// - JSON float: `123.456` → Decimal
/// - JSON null: → `Decimal::ZERO`
/// - Scientific notation: `"1.5e-8"` → Decimal
///
/// # Performance
///
/// This implementation is optimized for high-frequency trading scenarios:
/// - Zero allocations for string values (uses borrowed `&str`)
/// - Direct integer conversion without string intermediary
/// - No intermediate `serde_json::Value` heap allocation
///
/// # Errors
///
/// Returns an error if the value cannot be parsed as a valid decimal.
pub fn deserialize_decimal<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(DecimalVisitor)
}

/// Deserializes an `Option<Decimal>` from a JSON string, number, or null.
///
/// High-performance implementation using a custom visitor that avoids intermediate
/// `serde_json::Value` allocations. Handles all JSON numeric representations:
///
/// - JSON string: `"123.456"` → Some(Decimal) (zero-copy for borrowed strings)
/// - JSON integer: `123` → Some(Decimal) (direct conversion)
/// - JSON float: `123.456` → Some(Decimal)
/// - JSON null: → `None`
/// - Empty string: `""` → `None`
/// - Scientific notation: `"1.5e-8"` → Some(Decimal)
///
/// # Performance
///
/// This implementation is optimized for high-frequency trading scenarios:
/// - Zero allocations for string values (uses borrowed `&str`)
/// - Direct integer conversion without string intermediary
/// - No intermediate `serde_json::Value` heap allocation
///
/// # Errors
///
/// Returns an error if the value cannot be parsed as a valid decimal.
pub fn deserialize_optional_decimal<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    // Use deserialize_any to handle all JSON value types uniformly
    // (deserialize_option would route non-null through visit_some, losing empty string handling)
    deserializer.deserialize_any(OptionalDecimalVisitor)
}

/// Serializes a `Decimal` as a JSON number (float).
///
/// Used for outgoing requests where exchange APIs expect JSON numbers.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn serialize_decimal<S: Serializer>(d: &Decimal, s: S) -> Result<S::Ok, S::Error> {
    rust_decimal::serde::float::serialize(d, s)
}

/// Serializes an `Option<Decimal>` as a JSON number or null.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn serialize_optional_decimal<S: Serializer>(
    d: &Option<Decimal>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match d {
        Some(decimal) => rust_decimal::serde::float::serialize(decimal, s),
        None => s.serialize_none(),
    }
}

/// Deserializes a `Decimal` from a JSON string.
///
/// This is the strict form that requires the value to be a string, rejecting
/// numeric JSON values to avoid precision loss.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_decimal_from_str<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Decimal::from_str(&s).map_err(D::Error::custom)
}

/// Deserializes a `Decimal` from a string field that might be empty.
///
/// Handles edge cases where empty string "" or "0" becomes `Decimal::ZERO`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_decimal_or_zero<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    if s.is_empty() || s == "0" {
        Ok(Decimal::ZERO)
    } else {
        Decimal::from_str(&s).map_err(D::Error::custom)
    }
}

/// Deserializes an optional `Decimal` from a string field.
///
/// Returns `None` if the string is empty or "0", otherwise parses to `Decimal`.
/// This is a strict string-only deserializer; for flexible handling of strings,
/// numbers, and null, use [`deserialize_optional_decimal`].
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_optional_decimal_str<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    if s.is_empty() || s == "0" {
        Ok(None)
    } else {
        Decimal::from_str(&s).map(Some).map_err(D::Error::custom)
    }
}

/// Deserializes an optional `Decimal` from a string-only field.
///
/// Returns `None` if the value is null or the string is empty, otherwise
/// parses to `Decimal`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_optional_decimal_from_str<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) if !s.is_empty() => Decimal::from_str(&s).map(Some).map_err(D::Error::custom),
        _ => Ok(None),
    }
}

/// Deserializes a `Decimal` from an optional string field, defaulting to zero.
///
/// Handles edge cases: `None`, empty string "", or "0" all become `Decimal::ZERO`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_optional_decimal_or_zero<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Deserialize::deserialize(deserializer)?;
    match opt {
        None => Ok(Decimal::ZERO),
        Some(s) if s.is_empty() || s == "0" => Ok(Decimal::ZERO),
        Some(s) => Decimal::from_str(&s).map_err(D::Error::custom),
    }
}

/// Deserializes a `Vec<Decimal>` from a JSON array of strings.
///
/// # Errors
///
/// Returns an error if any string cannot be parsed as a valid decimal.
pub fn deserialize_vec_decimal_from_str<'de, D>(deserializer: D) -> Result<Vec<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let strings = Vec::<String>::deserialize(deserializer)?;
    strings
        .into_iter()
        .map(|s| Decimal::from_str(&s).map_err(D::Error::custom))
        .collect()
}

/// Serializes a `Decimal` as a string (lossless, no scientific notation).
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn serialize_decimal_as_str<S>(decimal: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&decimal.to_string())
}

/// Serializes an optional `Decimal` as a string.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn serialize_optional_decimal_as_str<S>(
    decimal: &Option<Decimal>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match decimal {
        Some(d) => serializer.serialize_str(&d.to_string()),
        None => serializer.serialize_none(),
    }
}

/// Serializes a `Vec<Decimal>` as an array of strings.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn serialize_vec_decimal_as_str<S>(
    decimals: &Vec<Decimal>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut seq = serializer.serialize_seq(Some(decimals.len()))?;
    for decimal in decimals {
        seq.serialize_element(&decimal.to_string())?;
    }
    seq.end()
}

/// Parses a string to `Decimal`, returning an error if parsing fails.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a Decimal.
pub fn parse_decimal(s: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str(s).map_err(|e| anyhow::anyhow!("Failed to parse decimal from '{s}': {e}"))
}

/// Parses an optional string to `Decimal`, returning `None` if the string is `None` or empty.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a Decimal.
pub fn parse_optional_decimal(s: &Option<String>) -> anyhow::Result<Option<Decimal>> {
    match s {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => parse_decimal(s).map(Some),
    }
}

/// Deserializes an empty string into `None`.
///
/// Many exchange APIs represent null string fields as an empty string (`""`).
/// When such a payload is mapped onto `Option<String>` the default behavior
/// would yield `Some("")`, which is semantically different from the intended
/// absence of a value. This helper ensures that empty strings are normalized
/// to `None` during deserialization.
///
/// # Errors
///
/// Returns an error if the JSON value cannot be deserialized into a string.
pub fn deserialize_empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Deserializes an empty [`Ustr`] into `None`.
///
/// # Errors
///
/// Returns an error if the JSON value cannot be deserialized into a string.
pub fn deserialize_empty_ustr_as_none<'de, D>(deserializer: D) -> Result<Option<Ustr>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Ustr>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Deserializes a `u8` from a string field.
///
/// Returns 0 if the string is empty.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a u8.
pub fn deserialize_string_to_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    if s.is_empty() {
        return Ok(0);
    }
    s.parse::<u8>().map_err(D::Error::custom)
}

/// Deserializes a `u64` from a string field.
///
/// Returns 0 if the string is empty.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a u64.
pub fn deserialize_string_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(0)
    } else {
        s.parse::<u64>().map_err(D::Error::custom)
    }
}

/// Deserializes an optional `u64` from a string field.
///
/// Returns `None` if the value is null or the string is empty.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a u64.
pub fn deserialize_optional_string_to_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse().map(Some).map_err(D::Error::custom),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use rstest::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde::{Deserialize, Serialize};
    use ustr::Ustr;

    use super::{
        Serializable, deserialize_decimal, deserialize_decimal_from_str,
        deserialize_decimal_or_zero, deserialize_empty_string_as_none,
        deserialize_empty_ustr_as_none, deserialize_optional_decimal,
        deserialize_optional_decimal_or_zero, deserialize_optional_decimal_str,
        deserialize_optional_string_to_u64, deserialize_string_to_u8, deserialize_string_to_u64,
        deserialize_vec_decimal_from_str, from_bool_as_u8,
        msgpack::{FromMsgPack, ToMsgPack},
        parse_decimal, parse_optional_decimal, serialize_decimal, serialize_decimal_as_str,
        serialize_optional_decimal, serialize_optional_decimal_as_str,
        serialize_vec_decimal_as_str,
    };

    #[derive(Deserialize)]
    pub struct TestStruct {
        #[serde(deserialize_with = "from_bool_as_u8")]
        pub value: u8,
    }

    #[rstest]
    #[case(r#"{"value": true}"#, 1)]
    #[case(r#"{"value": false}"#, 0)]
    fn test_deserialize_bool_as_u8_with_boolean(#[case] json_str: &str, #[case] expected: u8) {
        let test_struct: TestStruct = serde_json::from_str(json_str).unwrap();
        assert_eq!(test_struct.value, expected);
    }

    #[rstest]
    #[case(r#"{"value": 1}"#, 1)]
    #[case(r#"{"value": 0}"#, 0)]
    fn test_deserialize_bool_as_u8_with_u64(#[case] json_str: &str, #[case] expected: u8) {
        let test_struct: TestStruct = serde_json::from_str(json_str).unwrap();
        assert_eq!(test_struct.value, expected);
    }

    #[rstest]
    fn test_deserialize_bool_as_u8_with_invalid_integer() {
        // Any integer other than 0/1 is invalid and should error
        let json = r#"{"value": 2}"#;
        let result: Result<TestStruct, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct SerializableTestStruct {
        id: u32,
        name: String,
        value: f64,
    }

    impl Serializable for SerializableTestStruct {}

    #[rstest]
    fn test_serializable_json_roundtrip() {
        let original = SerializableTestStruct {
            id: 42,
            name: "test".to_string(),
            value: std::f64::consts::PI,
        };

        let json_bytes = original.to_json_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_json_bytes(&json_bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_serializable_msgpack_roundtrip() {
        let original = SerializableTestStruct {
            id: 123,
            name: "msgpack_test".to_string(),
            value: std::f64::consts::E,
        };

        let msgpack_bytes = original.to_msgpack_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_msgpack_bytes(&msgpack_bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_serializable_json_invalid_data() {
        let invalid_json = b"invalid json data";
        let result = SerializableTestStruct::from_json_bytes(invalid_json);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serializable_msgpack_invalid_data() {
        let invalid_msgpack = b"invalid msgpack data";
        let result = SerializableTestStruct::from_msgpack_bytes(invalid_msgpack);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serializable_json_empty_values() {
        let test_struct = SerializableTestStruct {
            id: 0,
            name: String::new(),
            value: 0.0,
        };

        let json_bytes = test_struct.to_json_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_json_bytes(&json_bytes).unwrap();

        assert_eq!(test_struct, deserialized);
    }

    #[rstest]
    fn test_serializable_msgpack_empty_values() {
        let test_struct = SerializableTestStruct {
            id: 0,
            name: String::new(),
            value: 0.0,
        };

        let msgpack_bytes = test_struct.to_msgpack_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_msgpack_bytes(&msgpack_bytes).unwrap();

        assert_eq!(test_struct, deserialized);
    }

    #[derive(Deserialize)]
    struct TestOptionalDecimalStr {
        #[serde(deserialize_with = "deserialize_optional_decimal_str")]
        value: Option<Decimal>,
    }

    #[derive(Deserialize)]
    struct TestDecimalOrZero {
        #[serde(deserialize_with = "deserialize_decimal_or_zero")]
        value: Decimal,
    }

    #[derive(Deserialize)]
    struct TestOptionalDecimalOrZero {
        #[serde(deserialize_with = "deserialize_optional_decimal_or_zero")]
        value: Decimal,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct TestDecimalRoundtrip {
        #[serde(
            serialize_with = "serialize_decimal_as_str",
            deserialize_with = "deserialize_decimal_from_str"
        )]
        value: Decimal,
        #[serde(
            serialize_with = "serialize_optional_decimal_as_str",
            deserialize_with = "super::deserialize_optional_decimal_from_str"
        )]
        optional_value: Option<Decimal>,
    }

    #[rstest]
    #[case(r#"{"value":"123.45"}"#, Some(dec!(123.45)))]
    #[case(r#"{"value":"0"}"#, None)]
    #[case(r#"{"value":""}"#, None)]
    fn test_deserialize_optional_decimal_str(
        #[case] json: &str,
        #[case] expected: Option<Decimal>,
    ) {
        let result: TestOptionalDecimalStr = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value":"123.45"}"#, dec!(123.45))]
    #[case(r#"{"value":"0"}"#, Decimal::ZERO)]
    #[case(r#"{"value":""}"#, Decimal::ZERO)]
    fn test_deserialize_decimal_or_zero(#[case] json: &str, #[case] expected: Decimal) {
        let result: TestDecimalOrZero = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value":"123.45"}"#, dec!(123.45))]
    #[case(r#"{"value":"0"}"#, Decimal::ZERO)]
    #[case(r#"{"value":null}"#, Decimal::ZERO)]
    fn test_deserialize_optional_decimal_or_zero(#[case] json: &str, #[case] expected: Decimal) {
        let result: TestOptionalDecimalOrZero = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    fn test_decimal_serialization_roundtrip() {
        let original = TestDecimalRoundtrip {
            value: dec!(123.456789012345678),
            optional_value: Some(dec!(0.000000001)),
        };

        let json = serde_json::to_string(&original).unwrap();

        // Check that it's serialized as strings
        assert!(json.contains("\"123.456789012345678\""));
        assert!(json.contains("\"0.000000001\""));

        let deserialized: TestDecimalRoundtrip = serde_json::from_str(&json).unwrap();
        assert_eq!(original.value, deserialized.value);
        assert_eq!(original.optional_value, deserialized.optional_value);
    }

    #[rstest]
    fn test_decimal_optional_none_handling() {
        let test_struct = TestDecimalRoundtrip {
            value: dec!(42.0),
            optional_value: None,
        };

        let json = serde_json::to_string(&test_struct).unwrap();
        assert!(json.contains("null"));

        let parsed: TestDecimalRoundtrip = serde_json::from_str(&json).unwrap();
        assert_eq!(test_struct.value, parsed.value);
        assert_eq!(None, parsed.optional_value);
    }

    #[derive(Deserialize)]
    struct TestEmptyStringAsNone {
        #[serde(deserialize_with = "deserialize_empty_string_as_none")]
        value: Option<String>,
    }

    #[rstest]
    #[case(r#"{"value":"hello"}"#, Some("hello".to_string()))]
    #[case(r#"{"value":""}"#, None)]
    #[case(r#"{"value":null}"#, None)]
    fn test_deserialize_empty_string_as_none(#[case] json: &str, #[case] expected: Option<String>) {
        let result: TestEmptyStringAsNone = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[derive(Deserialize)]
    struct TestEmptyUstrAsNone {
        #[serde(deserialize_with = "deserialize_empty_ustr_as_none")]
        value: Option<Ustr>,
    }

    #[rstest]
    #[case(r#"{"value":"hello"}"#, Some(Ustr::from("hello")))]
    #[case(r#"{"value":""}"#, None)]
    #[case(r#"{"value":null}"#, None)]
    fn test_deserialize_empty_ustr_as_none(#[case] json: &str, #[case] expected: Option<Ustr>) {
        let result: TestEmptyUstrAsNone = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct TestVecDecimal {
        #[serde(
            serialize_with = "serialize_vec_decimal_as_str",
            deserialize_with = "deserialize_vec_decimal_from_str"
        )]
        values: Vec<Decimal>,
    }

    #[rstest]
    fn test_vec_decimal_roundtrip() {
        let original = TestVecDecimal {
            values: vec![dec!(1.5), dec!(2.25), dec!(100.001)],
        };

        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("[\"1.5\",\"2.25\",\"100.001\"]"));

        let parsed: TestVecDecimal = serde_json::from_str(&json).unwrap();
        assert_eq!(original.values, parsed.values);
    }

    #[rstest]
    fn test_vec_decimal_empty() {
        let original = TestVecDecimal { values: vec![] };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: TestVecDecimal = serde_json::from_str(&json).unwrap();
        assert_eq!(original.values, parsed.values);
    }

    #[derive(Deserialize)]
    struct TestStringToU8 {
        #[serde(deserialize_with = "deserialize_string_to_u8")]
        value: u8,
    }

    #[rstest]
    #[case(r#"{"value":"42"}"#, 42)]
    #[case(r#"{"value":"0"}"#, 0)]
    #[case(r#"{"value":"255"}"#, 255)]
    #[case(r#"{"value":""}"#, 0)]
    fn test_deserialize_string_to_u8(#[case] json: &str, #[case] expected: u8) {
        let result: TestStringToU8 = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value":"256"}"#)]
    #[case(r#"{"value":"999"}"#)]
    #[case(r#"{"value":"abc"}"#)]
    fn test_deserialize_string_to_u8_invalid(#[case] json: &str) {
        let result: Result<TestStringToU8, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[derive(Deserialize)]
    struct TestStringToU64 {
        #[serde(deserialize_with = "deserialize_string_to_u64")]
        value: u64,
    }

    #[rstest]
    #[case(r#"{"value":"12345678901234"}"#, 12_345_678_901_234)]
    #[case(r#"{"value":"0"}"#, 0)]
    #[case(r#"{"value":"18446744073709551615"}"#, u64::MAX)]
    #[case(r#"{"value":""}"#, 0)]
    fn test_deserialize_string_to_u64(#[case] json: &str, #[case] expected: u64) {
        let result: TestStringToU64 = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value":"18446744073709551616"}"#)]
    #[case(r#"{"value":"abc"}"#)]
    #[case(r#"{"value":"-1"}"#)]
    fn test_deserialize_string_to_u64_invalid(#[case] json: &str) {
        let result: Result<TestStringToU64, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[derive(Deserialize)]
    struct TestOptionalStringToU64 {
        #[serde(deserialize_with = "deserialize_optional_string_to_u64")]
        value: Option<u64>,
    }

    #[rstest]
    #[case(r#"{"value":"12345678901234"}"#, Some(12_345_678_901_234))]
    #[case(r#"{"value":"0"}"#, Some(0))]
    #[case(r#"{"value":""}"#, None)]
    #[case(r#"{"value":null}"#, None)]
    fn test_deserialize_optional_string_to_u64(#[case] json: &str, #[case] expected: Option<u64>) {
        let result: TestOptionalStringToU64 = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case("123.45", dec!(123.45))]
    #[case("0", Decimal::ZERO)]
    #[case("0.0", Decimal::ZERO)]
    fn test_parse_decimal(#[case] input: &str, #[case] expected: Decimal) {
        let result = parse_decimal(input).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_parse_decimal_invalid() {
        assert!(parse_decimal("invalid").is_err());
        assert!(parse_decimal("").is_err());
    }

    #[rstest]
    #[case(&Some("123.45".to_string()), Some(dec!(123.45)))]
    #[case(&Some("0".to_string()), Some(Decimal::ZERO))]
    #[case(&Some(String::new()), None)]
    #[case(&None, None)]
    fn test_parse_optional_decimal(
        #[case] input: &Option<String>,
        #[case] expected: Option<Decimal>,
    ) {
        let result = parse_optional_decimal(input).unwrap();
        assert_eq!(result, expected);
    }

    // Tests for flexible decimal deserializers (handles both string and number JSON values)

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestFlexibleDecimal {
        #[serde(
            serialize_with = "serialize_decimal",
            deserialize_with = "deserialize_decimal"
        )]
        value: Decimal,
        #[serde(
            serialize_with = "serialize_optional_decimal",
            deserialize_with = "deserialize_optional_decimal"
        )]
        optional_value: Option<Decimal>,
    }

    #[rstest]
    #[case(r#"{"value": 123.456, "optional_value": 789.012}"#, dec!(123.456), Some(dec!(789.012)))]
    #[case(r#"{"value": "123.456", "optional_value": "789.012"}"#, dec!(123.456), Some(dec!(789.012)))]
    #[case(r#"{"value": 100, "optional_value": null}"#, dec!(100), None)]
    #[case(r#"{"value": null, "optional_value": null}"#, Decimal::ZERO, None)]
    fn test_deserialize_flexible_decimal(
        #[case] json: &str,
        #[case] expected_value: Decimal,
        #[case] expected_optional: Option<Decimal>,
    ) {
        let result: TestFlexibleDecimal = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected_value);
        assert_eq!(result.optional_value, expected_optional);
    }

    #[rstest]
    fn test_flexible_decimal_roundtrip() {
        let original = TestFlexibleDecimal {
            value: dec!(123.456),
            optional_value: Some(dec!(789.012)),
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: TestFlexibleDecimal = serde_json::from_str(&json).unwrap();

        assert_eq!(original.value, deserialized.value);
        assert_eq!(original.optional_value, deserialized.optional_value);
    }

    #[rstest]
    fn test_flexible_decimal_scientific_notation() {
        // Test that scientific notation from serde_json is handled correctly.
        // serde_json outputs very small numbers like 0.00000001 as "1e-8".
        // Note: JSON numbers are parsed as f64, so values are limited to ~15 significant digits.
        let json = r#"{"value": 0.00000001, "optional_value": 12345678.12345}"#;
        let parsed: TestFlexibleDecimal = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.value, dec!(0.00000001));
        assert_eq!(parsed.optional_value, Some(dec!(12345678.12345)));
    }

    #[rstest]
    fn test_flexible_decimal_empty_string_optional() {
        let json = r#"{"value": 100, "optional_value": ""}"#;
        let parsed: TestFlexibleDecimal = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.value, dec!(100));
        assert_eq!(parsed.optional_value, None);
    }

    // Additional tests for DecimalVisitor edge cases

    #[derive(Debug, Deserialize)]
    struct TestDecimalOnly {
        #[serde(deserialize_with = "deserialize_decimal")]
        value: Decimal,
    }

    #[rstest]
    #[case(r#"{"value": "1.5e-8"}"#, dec!(0.000000015))]
    #[case(r#"{"value": "1E10"}"#, dec!(10000000000))]
    #[case(r#"{"value": "-1.23e5"}"#, dec!(-123000))]
    fn test_deserialize_decimal_scientific_string(#[case] json: &str, #[case] expected: Decimal) {
        let result: TestDecimalOnly = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value": 9223372036854775807}"#, dec!(9223372036854775807))] // i64::MAX
    #[case(r#"{"value": -9223372036854775808}"#, dec!(-9223372036854775808))] // i64::MIN
    #[case(r#"{"value": 0}"#, Decimal::ZERO)]
    fn test_deserialize_decimal_large_integers(#[case] json: &str, #[case] expected: Decimal) {
        let result: TestDecimalOnly = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value": "-123.456789"}"#, dec!(-123.456789))]
    #[case(r#"{"value": -999.99}"#, dec!(-999.99))]
    fn test_deserialize_decimal_negative(#[case] json: &str, #[case] expected: Decimal) {
        let result: TestDecimalOnly = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value": "123456789.123456789012345678"}"#)] // High precision string
    fn test_deserialize_decimal_high_precision(#[case] json: &str) {
        let result: TestDecimalOnly = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, dec!(123456789.123456789012345678));
    }

    #[derive(Debug, Deserialize)]
    struct TestOptionalDecimalOnly {
        #[serde(deserialize_with = "deserialize_optional_decimal")]
        value: Option<Decimal>,
    }

    #[rstest]
    #[case(r#"{"value": "1.5e-8"}"#, Some(dec!(0.000000015)))]
    #[case(r#"{"value": null}"#, None)]
    #[case(r#"{"value": ""}"#, None)]
    #[case(r#"{"value": 42}"#, Some(dec!(42)))]
    #[case(r#"{"value": -100.5}"#, Some(dec!(-100.5)))]
    fn test_deserialize_optional_decimal_various(
        #[case] json: &str,
        #[case] expected: Option<Decimal>,
    ) {
        let result: TestOptionalDecimalOnly = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }
}

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

//! A `UnixNanos` type for working with timestamps in nanoseconds since the UNIX epoch.
//!
//! This module provides a strongly-typed representation of timestamps as nanoseconds
//! since the UNIX epoch (January 1, 1970, 00:00:00 UTC). The `UnixNanos` type offers
//! conversion utilities, arithmetic operations, and comparison methods.
//!
//! # Features
//!
//! - Zero-cost abstraction with appropriate operator implementations.
//! - Conversion to/from `DateTime<Utc>`.
//! - RFC 3339 string formatting.
//! - Duration calculations.
//! - Flexible parsing and serialization.
//!
//! # Parsing and Serialization
//!
//! `UnixNanos` can be created from and serialized to various formats:
//!
//! * Integer values are interpreted as nanoseconds since the UNIX epoch.
//! * Floating-point values are interpreted as seconds since the UNIX epoch (converted to nanoseconds).
//! * String values may be:
//!   - A numeric string (interpreted as nanoseconds).
//!   - A floating-point string (interpreted as seconds, converted to nanoseconds).
//!   - An RFC 3339 formatted timestamp (ISO 8601 with timezone).
//!   - A simple date string in YYYY-MM-DD format (interpreted as midnight UTC on that date).
//!
//! # Limitations
//!
//! * Negative timestamps are invalid and will result in an error.
//! * Arithmetic operations will panic on overflow/underflow rather than wrapping.

use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Add, AddAssign, Deref, Sub, SubAssign},
    str::FromStr,
};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, Visitor},
};

/// Represents a duration in nanoseconds.
pub type DurationNanos = u64;

/// Represents a timestamp in nanoseconds since the UNIX epoch.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct UnixNanos(u64);

impl UnixNanos {
    /// Creates a new [`UnixNanos`] instance.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying value as `u64`.
    #[must_use]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Returns the underlying value as `i64`.
    #[must_use]
    pub const fn as_i64(&self) -> i64 {
        self.0 as i64
    }

    /// Returns the underlying value as `f64`.
    #[must_use]
    pub const fn as_f64(&self) -> f64 {
        self.0 as f64
    }

    /// Converts the underlying value to a datetime (UTC).
    #[must_use]
    pub const fn to_datetime_utc(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(self.0 as i64)
    }

    /// Converts the underlying value to an ISO 8601 (RFC 3339) string.
    #[must_use]
    pub fn to_rfc3339(&self) -> String {
        self.to_datetime_utc().to_rfc3339()
    }

    /// Calculates the duration in nanoseconds since another [`UnixNanos`] instance.
    ///
    /// Returns `Some(duration)` if `self` is later than `other`, otherwise `None` if `other` is
    /// greater than `self` (indicating a negative duration is not possible with `DurationNanos`).
    #[must_use]
    pub const fn duration_since(&self, other: &Self) -> Option<DurationNanos> {
        self.0.checked_sub(other.0)
    }

    fn parse_string(s: &str) -> Result<Self, String> {
        // Try parsing as an integer (nanoseconds)
        if let Ok(int_value) = s.parse::<u64>() {
            return Ok(Self(int_value));
        }

        // Try parsing as a floating point number (seconds)
        if let Ok(float_value) = s.parse::<f64>() {
            if float_value < 0.0 {
                return Err("Unix timestamp cannot be negative".into());
            }
            let nanos = (float_value * 1_000_000_000.0).round() as u64;
            return Ok(Self(nanos));
        }

        // Try parsing as an RFC 3339 timestamp
        if let Ok(datetime) = DateTime::parse_from_rfc3339(s) {
            let nanos = datetime
                .timestamp_nanos_opt()
                .ok_or_else(|| "Timestamp out of range".to_string())?;
            if nanos < 0 {
                return Err("Unix timestamp cannot be negative".into());
            }
            return Ok(Self(nanos as u64));
        }

        // Try parsing as a simple date string (YYYY-MM-DD format)
        if let Ok(datetime) = NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(|date| date.and_hms_opt(0, 0, 0).unwrap())
            .map(|naive_dt| DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc))
        {
            let nanos = datetime
                .timestamp_nanos_opt()
                .ok_or_else(|| "Timestamp out of range".to_string())?;
            return Ok(Self(nanos as u64));
        }

        Err(format!("Invalid format: {s}"))
    }
}

impl Deref for UnixNanos {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<u64> for UnixNanos {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u64> for UnixNanos {
    fn partial_cmp(&self, other: &u64) -> Option<Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialEq<Option<u64>> for UnixNanos {
    fn eq(&self, other: &Option<u64>) -> bool {
        match other {
            Some(value) => self.0 == *value,
            None => false,
        }
    }
}

impl PartialOrd<Option<u64>> for UnixNanos {
    fn partial_cmp(&self, other: &Option<u64>) -> Option<Ordering> {
        match other {
            Some(value) => self.0.partial_cmp(value),
            None => Some(Ordering::Greater),
        }
    }
}

impl PartialEq<UnixNanos> for u64 {
    fn eq(&self, other: &UnixNanos) -> bool {
        *self == other.0
    }
}

impl PartialOrd<UnixNanos> for u64 {
    fn partial_cmp(&self, other: &UnixNanos) -> Option<Ordering> {
        self.partial_cmp(&other.0)
    }
}

impl From<u64> for UnixNanos {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<UnixNanos> for u64 {
    fn from(value: UnixNanos) -> Self {
        value.0
    }
}

impl From<&str> for UnixNanos {
    fn from(value: &str) -> Self {
        value
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse string into UnixNanos: {e}"))
    }
}

impl From<String> for UnixNanos {
    fn from(value: String) -> Self {
        value
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse string into UnixNanos: {e}"))
    }
}

impl From<DateTime<Utc>> for UnixNanos {
    fn from(value: DateTime<Utc>) -> Self {
        Self::from(value.timestamp_nanos_opt().expect("Invalid timestamp") as u64)
    }
}

impl FromStr for UnixNanos {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_string(s).map_err(std::convert::Into::into)
    }
}

impl Add for UnixNanos {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_add(rhs.0)
                .expect("Error adding with overflow"),
        )
    }
}

impl Sub for UnixNanos {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_sub(rhs.0)
                .expect("Error subtracting with underflow"),
        )
    }
}

impl Add<u64> for UnixNanos {
    type Output = Self;

    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0.checked_add(rhs).expect("Error adding with overflow"))
    }
}

impl Sub<u64> for UnixNanos {
    type Output = Self;

    fn sub(self, rhs: u64) -> Self::Output {
        Self(
            self.0
                .checked_sub(rhs)
                .expect("Error subtracting with underflow"),
        )
    }
}

impl<T: Into<u64>> AddAssign<T> for UnixNanos {
    fn add_assign(&mut self, other: T) {
        let other_u64 = other.into();
        self.0 = self
            .0
            .checked_add(other_u64)
            .expect("Error adding with overflow");
    }
}

impl<T: Into<u64>> SubAssign<T> for UnixNanos {
    fn sub_assign(&mut self, other: T) {
        let other_u64 = other.into();
        self.0 = self
            .0
            .checked_sub(other_u64)
            .expect("Error subtracting with underflow");
    }
}

impl Display for UnixNanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<UnixNanos> for DateTime<Utc> {
    fn from(value: UnixNanos) -> Self {
        value.to_datetime_utc()
    }
}

impl<'de> Deserialize<'de> for UnixNanos {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UnixNanosVisitor;

        impl Visitor<'_> for UnixNanosVisitor {
            type Value = UnixNanos;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an integer, a string integer, or an RFC 3339 timestamp")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UnixNanos(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value < 0 {
                    return Err(E::custom("Unix timestamp cannot be negative"));
                }
                Ok(UnixNanos(value as u64))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value < 0.0 {
                    return Err(E::custom("Unix timestamp cannot be negative"));
                }
                // Convert from seconds to nanoseconds
                let nanos = (value * 1_000_000_000.0).round() as u64;
                Ok(UnixNanos(nanos))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                UnixNanos::parse_string(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(UnixNanosVisitor)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new() {
        let nanos = UnixNanos::new(123);
        assert_eq!(nanos.as_u64(), 123);
        assert_eq!(nanos.as_i64(), 123);
    }

    #[rstest]
    fn test_from_u64() {
        let nanos = UnixNanos::from(123);
        assert_eq!(nanos.as_u64(), 123);
        assert_eq!(nanos.as_i64(), 123);
    }

    #[rstest]
    fn test_default() {
        let nanos = UnixNanos::default();
        assert_eq!(nanos.as_u64(), 0);
        assert_eq!(nanos.as_i64(), 0);
    }

    #[rstest]
    fn test_into_from() {
        let nanos: UnixNanos = 456.into();
        let value: u64 = nanos.into();
        assert_eq!(value, 456);
    }

    #[rstest]
    #[case(0, "1970-01-01T00:00:00+00:00")]
    #[case(1_000_000_000, "1970-01-01T00:00:01+00:00")]
    #[case(1_000_000_000_000_000_000, "2001-09-09T01:46:40+00:00")]
    #[case(1_500_000_000_000_000_000, "2017-07-14T02:40:00+00:00")]
    #[case(1_707_577_123_456_789_000, "2024-02-10T14:58:43.456789+00:00")]
    fn test_to_datetime_utc(#[case] nanos: u64, #[case] expected: &str) {
        let nanos = UnixNanos::from(nanos);
        let datetime = nanos.to_datetime_utc();
        assert_eq!(datetime.to_rfc3339(), expected);
    }

    #[rstest]
    #[case(0, "1970-01-01T00:00:00+00:00")]
    #[case(1_000_000_000, "1970-01-01T00:00:01+00:00")]
    #[case(1_000_000_000_000_000_000, "2001-09-09T01:46:40+00:00")]
    #[case(1_500_000_000_000_000_000, "2017-07-14T02:40:00+00:00")]
    #[case(1_707_577_123_456_789_000, "2024-02-10T14:58:43.456789+00:00")]
    fn test_to_rfc3339(#[case] nanos: u64, #[case] expected: &str) {
        let nanos = UnixNanos::from(nanos);
        assert_eq!(nanos.to_rfc3339(), expected);
    }

    #[rstest]
    fn test_from_str() {
        let nanos: UnixNanos = "123".parse().unwrap();
        assert_eq!(nanos.as_u64(), 123);
    }

    #[rstest]
    fn test_from_str_invalid() {
        let result = "abc".parse::<UnixNanos>();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_try_from_datetime_valid() {
        use chrono::TimeZone;
        let datetime = Utc.timestamp_opt(1_000_000_000, 0).unwrap(); // 1 billion seconds since epoch
        let nanos = UnixNanos::from(datetime);
        assert_eq!(nanos.as_u64(), 1_000_000_000_000_000_000);
    }

    #[rstest]
    fn test_eq() {
        let nanos = UnixNanos::from(100);
        assert_eq!(nanos, 100);
        assert_eq!(nanos, Some(100));
        assert_ne!(nanos, 200);
        assert_ne!(nanos, Some(200));
        assert_ne!(nanos, None);
    }

    #[rstest]
    fn test_partial_cmp() {
        let nanos = UnixNanos::from(100);
        assert_eq!(nanos.partial_cmp(&100), Some(Ordering::Equal));
        assert_eq!(nanos.partial_cmp(&200), Some(Ordering::Less));
        assert_eq!(nanos.partial_cmp(&50), Some(Ordering::Greater));
        assert_eq!(nanos.partial_cmp(&None), Some(Ordering::Greater));
    }

    #[rstest]
    fn test_edge_case_max_value() {
        let nanos = UnixNanos::from(u64::MAX);
        assert_eq!(format!("{nanos}"), format!("{}", u64::MAX));
    }

    #[rstest]
    fn test_display() {
        let nanos = UnixNanos::from(123);
        assert_eq!(format!("{nanos}"), "123");
    }

    #[rstest]
    fn test_addition() {
        let nanos1 = UnixNanos::from(100);
        let nanos2 = UnixNanos::from(200);
        let result = nanos1 + nanos2;
        assert_eq!(result.as_u64(), 300);
    }

    #[rstest]
    fn test_add_assign() {
        let mut nanos = UnixNanos::from(100);
        nanos += 50_u64;
        assert_eq!(nanos.as_u64(), 150);
    }

    #[rstest]
    fn test_subtraction() {
        let nanos1 = UnixNanos::from(200);
        let nanos2 = UnixNanos::from(100);
        let result = nanos1 - nanos2;
        assert_eq!(result.as_u64(), 100);
    }

    #[rstest]
    fn test_sub_assign() {
        let mut nanos = UnixNanos::from(200);
        nanos -= 50_u64;
        assert_eq!(nanos.as_u64(), 150);
    }

    #[rstest]
    #[should_panic(expected = "Error adding with overflow")]
    fn test_overflow_add() {
        let nanos = UnixNanos::from(u64::MAX);
        let _ = nanos + UnixNanos::from(1); // This should panic due to overflow
    }

    #[rstest]
    #[should_panic(expected = "Error adding with overflow")]
    fn test_overflow_add_u64() {
        let nanos = UnixNanos::from(u64::MAX);
        let _ = nanos + 1_u64; // This should panic due to overflow
    }

    #[rstest]
    #[should_panic(expected = "Error subtracting with underflow")]
    fn test_overflow_sub() {
        let _ = UnixNanos::default() - UnixNanos::from(1); // This should panic due to underflow
    }

    #[rstest]
    #[should_panic(expected = "Error subtracting with underflow")]
    fn test_overflow_sub_u64() {
        let _ = UnixNanos::default() - 1_u64; // This should panic due to underflow
    }

    #[rstest]
    #[case(100, 50, Some(50))]
    #[case(1_000_000_000, 500_000_000, Some(500_000_000))]
    #[case(u64::MAX, u64::MAX - 1, Some(1))]
    #[case(50, 50, Some(0))]
    #[case(50, 100, None)]
    #[case(0, 1, None)]
    fn test_duration_since(
        #[case] time1: u64,
        #[case] time2: u64,
        #[case] expected: Option<DurationNanos>,
    ) {
        let nanos1 = UnixNanos::from(time1);
        let nanos2 = UnixNanos::from(time2);
        assert_eq!(nanos1.duration_since(&nanos2), expected);
    }

    #[rstest]
    fn test_duration_since_same_moment() {
        let moment = UnixNanos::from(1_707_577_123_456_789_000);
        assert_eq!(moment.duration_since(&moment), Some(0));
    }

    #[rstest]
    fn test_duration_since_chronological() {
        // Create a reference time (Feb 10, 2024)
        let earlier = Utc.with_ymd_and_hms(2024, 2, 10, 12, 0, 0).unwrap();

        // Create a time 1 hour, 30 minutes, and 45 seconds later (with nanoseconds)
        let later = earlier
            + Duration::hours(1)
            + Duration::minutes(30)
            + Duration::seconds(45)
            + Duration::nanoseconds(500_000_000);

        let earlier_nanos = UnixNanos::from(earlier);
        let later_nanos = UnixNanos::from(later);

        // Calculate expected duration in nanoseconds
        let expected_duration = 60 * 60 * 1_000_000_000 + // 1 hour
        30 * 60 * 1_000_000_000 + // 30 minutes
        45 * 1_000_000_000 + // 45 seconds
        500_000_000; // 500 million nanoseconds

        assert_eq!(
            later_nanos.duration_since(&earlier_nanos),
            Some(expected_duration)
        );
        assert_eq!(earlier_nanos.duration_since(&later_nanos), None);
    }

    #[rstest]
    fn test_duration_since_with_edge_cases() {
        // Test with maximum value
        let max = UnixNanos::from(u64::MAX);
        let smaller = UnixNanos::from(u64::MAX - 1000);

        assert_eq!(max.duration_since(&smaller), Some(1000));
        assert_eq!(smaller.duration_since(&max), None);

        // Test with minimum value
        let min = UnixNanos::default(); // Zero timestamp
        let larger = UnixNanos::from(1000);

        assert_eq!(min.duration_since(&min), Some(0));
        assert_eq!(larger.duration_since(&min), Some(1000));
        assert_eq!(min.duration_since(&larger), None);
    }

    #[rstest]
    fn test_serde_json() {
        let nanos = UnixNanos::from(123);
        let json = serde_json::to_string(&nanos).unwrap();
        let deserialized: UnixNanos = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, nanos);
    }

    #[rstest]
    fn test_serde_edge_cases() {
        let nanos = UnixNanos::from(u64::MAX);
        let json = serde_json::to_string(&nanos).unwrap();
        let deserialized: UnixNanos = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, nanos);
    }

    #[rstest]
    #[case("123", 123)] // Integer string
    #[case("1234.567", 1_234_567_000_000)] // Float string (seconds to nanos)
    #[case("2024-02-10", 1707523200000000000)] // Simple date (midnight UTC)
    #[case("2024-02-10T14:58:43Z", 1707577123000000000)] // RFC3339 without fractions
    #[case("2024-02-10T14:58:43.456789Z", 1707577123456789000)] // RFC3339 with fractions
    fn test_from_str_formats(#[case] input: &str, #[case] expected: u64) {
        let parsed: UnixNanos = input.parse().unwrap();
        assert_eq!(parsed.as_u64(), expected);
    }

    #[rstest]
    #[case("abc")] // Random string
    #[case("not a timestamp")] // Non-timestamp string
    #[case("2024-02-10 14:58:43")] // Space-separated format (not RFC3339)
    fn test_from_str_invalid_formats(#[case] input: &str) {
        let result = input.parse::<UnixNanos>();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_deserialize_u64() {
        let json = "123456789";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 123456789);
    }

    #[rstest]
    fn test_deserialize_string_with_int() {
        let json = "\"123456789\"";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 123456789);
    }

    #[rstest]
    fn test_deserialize_float() {
        let json = "1234.567";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 1_234_567_000_000);
    }

    #[rstest]
    fn test_deserialize_string_with_float() {
        let json = "\"1234.567\"";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 1_234_567_000_000);
    }

    #[rstest]
    #[case("\"2024-02-10T14:58:43.456789Z\"", 1707577123456789000)]
    #[case("\"2024-02-10T14:58:43Z\"", 1707577123000000000)]
    fn test_deserialize_timestamp_strings(#[case] input: &str, #[case] expected: u64) {
        let deserialized: UnixNanos = serde_json::from_str(input).unwrap();
        assert_eq!(deserialized.as_u64(), expected);
    }

    #[rstest]
    fn test_deserialize_negative_int_fails() {
        let json = "-123456789";
        let result: Result<UnixNanos, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_deserialize_negative_float_fails() {
        let json = "-1234.567";
        let result: Result<UnixNanos, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_deserialize_invalid_string_fails() {
        let json = "\"not a timestamp\"";
        let result: Result<UnixNanos, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_deserialize_edge_cases() {
        // Test zero
        let json = "0";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 0);

        // Test large value
        let json = "18446744073709551615"; // u64::MAX
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), u64::MAX);
    }
}

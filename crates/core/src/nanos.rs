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
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]
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

    /// Creates a new [`UnixNanos`] instance with the maximum valid value.
    #[must_use]
    pub const fn max() -> Self {
        Self(u64::MAX)
    }

    /// Returns `true` if the value of this instance is zero.
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Returns the underlying value as `u64`.
    #[must_use]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Returns the underlying value as `i64`.
    ///
    /// # Panics
    ///
    /// Panics if the value exceeds `i64::MAX` (approximately year 2262).
    #[must_use]
    pub const fn as_i64(&self) -> i64 {
        assert!(
            self.0 <= i64::MAX as u64,
            "UnixNanos value exceeds i64::MAX"
        );
        self.0 as i64
    }

    /// Returns the underlying value as `f64`.
    #[must_use]
    pub const fn as_f64(&self) -> f64 {
        self.0 as f64
    }

    /// Converts the underlying value to a datetime (UTC).
    ///
    /// # Panics
    ///
    /// Panics if the value exceeds `i64::MAX` (approximately year 2262).
    #[must_use]
    pub const fn to_datetime_utc(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(self.as_i64())
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

        // If the string is composed solely of digits but didn't fit in a u64 we
        // treat that as an overflow error rather than attempting to interpret
        // it as seconds in floating-point form. This avoids the surprising
        // situation where a caller provides nanoseconds but gets an out-of-
        // range float interpretation instead.
        if s.chars().all(|c| c.is_ascii_digit()) {
            return Err("Unix timestamp is out of range".into());
        }

        // Try parsing as a floating point number (seconds)
        if let Ok(float_value) = s.parse::<f64>() {
            if !float_value.is_finite() {
                return Err("Unix timestamp must be finite".into());
            }

            if float_value < 0.0 {
                return Err("Unix timestamp cannot be negative".into());
            }

            // Convert seconds to nanoseconds while checking for overflow
            // We perform the multiplication in `f64`, then validate the
            // result fits inside `u64` *before* rounding / casting.
            const MAX_NS_F64: f64 = u64::MAX as f64;
            let nanos_f64 = float_value * 1_000_000_000.0;

            if nanos_f64 > MAX_NS_F64 {
                return Err("Unix timestamp is out of range".into());
            }

            let nanos = nanos_f64.round() as u64;
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

            // SAFETY: Checked that nanos >= 0, so cast to u64 is safe
            return Ok(Self(nanos as u64));
        }

        // Try parsing as a simple date string (YYYY-MM-DD format)
        if let Ok(datetime) = NaiveDate::parse_from_str(s, "%Y-%m-%d")
            // SAFETY: unwrap() is safe here because and_hms_opt(0, 0, 0) always succeeds
            // for valid dates (midnight is always a valid time)
            .map(|date| date.and_hms_opt(0, 0, 0).unwrap())
            .map(|naive_dt| DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc))
        {
            let nanos = datetime
                .timestamp_nanos_opt()
                .ok_or_else(|| "Timestamp out of range".to_string())?;
            if nanos < 0 {
                return Err("Unix timestamp cannot be negative".into());
            }
            return Ok(Self(nanos as u64));
        }

        Err(format!("Invalid format: {s}"))
    }

    /// Returns `Some(self + rhs)` or `None` if the addition would overflow
    #[must_use]
    pub fn checked_add<T: Into<u64>>(self, rhs: T) -> Option<Self> {
        self.0.checked_add(rhs.into()).map(Self)
    }

    /// Returns `Some(self - rhs)` or `None` if the subtraction would underflow
    #[must_use]
    pub fn checked_sub<T: Into<u64>>(self, rhs: T) -> Option<Self> {
        self.0.checked_sub(rhs.into()).map(Self)
    }

    /// Saturating addition – if overflow occurs the value is clamped to `u64::MAX`.
    #[must_use]
    pub fn saturating_add_ns<T: Into<u64>>(self, rhs: T) -> Self {
        Self(self.0.saturating_add(rhs.into()))
    }

    /// Saturating subtraction – if underflow occurs the value is clamped to `0`.
    #[must_use]
    pub fn saturating_sub_ns<T: Into<u64>>(self, rhs: T) -> Self {
        Self(self.0.saturating_sub(rhs.into()))
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

/// Converts a string slice to [`UnixNanos`].
///
/// # Panics
///
/// This implementation will panic if the string cannot be parsed into a valid [`UnixNanos`].
/// This is intentional fail-fast behavior where invalid timestamps indicate a critical
/// logic error that should halt execution rather than silently propagate incorrect data.
///
/// For error handling without panicking, use [`str::parse::<UnixNanos>()`] which returns
/// a [`Result`].
///
/// # Examples
///
/// ```
/// use nautilus_core::UnixNanos;
///
/// let nanos = UnixNanos::from("1234567890");
/// assert_eq!(nanos.as_u64(), 1234567890);
/// ```
impl From<&str> for UnixNanos {
    fn from(value: &str) -> Self {
        value
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse string '{value}' into UnixNanos: {e}. Use str::parse() for non-panicking error handling."))
    }
}

/// Converts a [`String`] to [`UnixNanos`].
///
/// # Panics
///
/// This implementation will panic if the string cannot be parsed into a valid [`UnixNanos`].
/// This is intentional fail-fast behavior where invalid timestamps indicate a critical
/// logic error that should halt execution rather than silently propagate incorrect data.
///
/// For error handling without panicking, use [`str::parse::<UnixNanos>()`] which returns
/// a [`Result`].
impl From<String> for UnixNanos {
    fn from(value: String) -> Self {
        value
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse string '{value}' into UnixNanos: {e}. Use str::parse() for non-panicking error handling."))
    }
}

impl From<DateTime<Utc>> for UnixNanos {
    fn from(value: DateTime<Utc>) -> Self {
        let nanos = value
            .timestamp_nanos_opt()
            .expect("DateTime timestamp out of range for UnixNanos");

        assert!(nanos >= 0, "DateTime timestamp cannot be negative: {nanos}");

        Self::from(nanos as u64)
    }
}

impl FromStr for UnixNanos {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_string(s).map_err(std::convert::Into::into)
    }
}

/// Adds two [`UnixNanos`] values.
///
/// # Panics
///
/// Panics on overflow. This is intentional fail-fast behavior: overflow in timestamp
/// arithmetic indicates a logic error in calculations that would corrupt data.
/// Use [`UnixNanos::checked_add()`] or [`UnixNanos::saturating_add_ns()`] if you need
/// explicit overflow handling.
impl Add for UnixNanos {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_add(rhs.0)
                .expect("UnixNanos overflow in addition - invalid timestamp calculation"),
        )
    }
}

/// Subtracts one [`UnixNanos`] from another.
///
/// # Panics
///
/// Panics on underflow. This is intentional fail-fast behavior: underflow in timestamp
/// arithmetic indicates a logic error in calculations that would corrupt data.
/// Use [`UnixNanos::checked_sub()`] or [`UnixNanos::saturating_sub_ns()`] if you need
/// explicit underflow handling.
impl Sub for UnixNanos {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_sub(rhs.0)
                .expect("UnixNanos underflow in subtraction - invalid timestamp calculation"),
        )
    }
}

/// Adds a `u64` nanosecond value to [`UnixNanos`].
///
/// # Panics
///
/// Panics on overflow. This is intentional fail-fast behavior for timestamp arithmetic.
/// Use [`UnixNanos::checked_add()`] for explicit overflow handling.
impl Add<u64> for UnixNanos {
    type Output = Self;

    fn add(self, rhs: u64) -> Self::Output {
        Self(
            self.0
                .checked_add(rhs)
                .expect("UnixNanos overflow in addition"),
        )
    }
}

/// Subtracts a `u64` nanosecond value from [`UnixNanos`].
///
/// # Panics
///
/// Panics on underflow. This is intentional fail-fast behavior for timestamp arithmetic.
/// Use [`UnixNanos::checked_sub()`] for explicit underflow handling.
impl Sub<u64> for UnixNanos {
    type Output = Self;

    fn sub(self, rhs: u64) -> Self::Output {
        Self(
            self.0
                .checked_sub(rhs)
                .expect("UnixNanos underflow in subtraction"),
        )
    }
}

/// Add-assigns a value to [`UnixNanos`].
///
/// # Panics
///
/// Panics on overflow. This is intentional fail-fast behavior for timestamp arithmetic.
impl<T: Into<u64>> AddAssign<T> for UnixNanos {
    fn add_assign(&mut self, other: T) {
        let other_u64 = other.into();
        self.0 = self
            .0
            .checked_add(other_u64)
            .expect("UnixNanos overflow in add_assign");
    }
}

/// Sub-assigns a value from [`UnixNanos`].
///
/// # Panics
///
/// Panics on underflow. This is intentional fail-fast behavior for timestamp arithmetic.
impl<T: Into<u64>> SubAssign<T> for UnixNanos {
    fn sub_assign(&mut self, other: T) {
        let other_u64 = other.into();
        self.0 = self
            .0
            .checked_sub(other_u64)
            .expect("UnixNanos underflow in sub_assign");
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
                if !value.is_finite() {
                    return Err(E::custom(format!(
                        "Unix timestamp must be finite, got {value}"
                    )));
                }
                if value < 0.0 {
                    return Err(E::custom("Unix timestamp cannot be negative"));
                }
                // Convert from seconds to nanoseconds with overflow check
                const MAX_NS_F64: f64 = u64::MAX as f64;
                let nanos_f64 = value * 1_000_000_000.0;
                if nanos_f64 > MAX_NS_F64 {
                    return Err(E::custom(format!(
                        "Unix timestamp {value} seconds is out of range"
                    )));
                }
                let nanos = nanos_f64.round() as u64;
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
    fn test_max() {
        let nanos = UnixNanos::max();
        assert_eq!(nanos.as_u64(), u64::MAX);
    }

    #[rstest]
    fn test_is_zero() {
        assert!(UnixNanos::default().is_zero());
        assert!(!UnixNanos::max().is_zero());
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
    fn test_from_str_pre_epoch_date() {
        let err = "1969-12-31".parse::<UnixNanos>().unwrap_err();
        assert_eq!(err.to_string(), "Unix timestamp cannot be negative");
    }

    #[rstest]
    fn test_from_str_pre_epoch_rfc3339() {
        let err = "1969-12-31T23:59:59Z".parse::<UnixNanos>().unwrap_err();
        assert_eq!(err.to_string(), "Unix timestamp cannot be negative");
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
    #[should_panic(expected = "UnixNanos overflow")]
    fn test_overflow_add() {
        let nanos = UnixNanos::from(u64::MAX);
        let _ = nanos + UnixNanos::from(1); // This should panic due to overflow
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos overflow")]
    fn test_overflow_add_u64() {
        let nanos = UnixNanos::from(u64::MAX);
        let _ = nanos + 1_u64; // This should panic due to overflow
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos underflow")]
    fn test_overflow_sub() {
        let _ = UnixNanos::default() - UnixNanos::from(1); // This should panic due to underflow
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos underflow")]
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
    #[case("2024-02-10", 1_707_523_200_000_000_000)] // Simple date (midnight UTC)
    #[case("2024-02-10T14:58:43Z", 1_707_577_123_000_000_000)] // RFC3339 without fractions
    #[case("2024-02-10T14:58:43.456789Z", 1_707_577_123_456_789_000)] // RFC3339 with fractions
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
    fn test_from_str_integer_overflow() {
        // One more digit than u64::MAX (20 digits) so definitely overflows
        let input = "184467440737095516160";
        let result = input.parse::<UnixNanos>();
        assert!(result.is_err());
    }

    // ---------- checked / saturating arithmetic ----------

    #[rstest]
    fn test_checked_add_overflow_returns_none() {
        let max = UnixNanos::from(u64::MAX);
        assert_eq!(max.checked_add(1_u64), None);
    }

    #[rstest]
    fn test_checked_sub_underflow_returns_none() {
        let zero = UnixNanos::default();
        assert_eq!(zero.checked_sub(1_u64), None);
    }

    #[rstest]
    fn test_saturating_add_overflow() {
        let max = UnixNanos::from(u64::MAX);
        let result = max.saturating_add_ns(1_u64);
        assert_eq!(result, UnixNanos::from(u64::MAX));
    }

    #[rstest]
    fn test_saturating_sub_underflow() {
        let zero = UnixNanos::default();
        let result = zero.saturating_sub_ns(1_u64);
        assert_eq!(result, UnixNanos::default());
    }

    #[rstest]
    fn test_from_str_float_overflow() {
        // Use scientific notation so we take the floating-point parsing path.
        let input = "2e10"; // 20 billion seconds ~ 634 years (> u64::MAX nanoseconds)
        let result = input.parse::<UnixNanos>();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_deserialize_u64() {
        let json = "123456789";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 123_456_789);
    }

    #[rstest]
    fn test_deserialize_string_with_int() {
        let json = "\"123456789\"";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 123_456_789);
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
    #[case("\"2024-02-10T14:58:43.456789Z\"", 1_707_577_123_456_789_000)]
    #[case("\"2024-02-10T14:58:43Z\"", 1_707_577_123_000_000_000)]
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
    fn test_deserialize_nan_fails() {
        // JSON doesn't support NaN directly, test the internal deserializer
        use serde::de::{
            IntoDeserializer,
            value::{Error as ValueError, F64Deserializer},
        };
        let deserializer: F64Deserializer<ValueError> = f64::NAN.into_deserializer();
        let result: Result<UnixNanos, _> = UnixNanos::deserialize(deserializer);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be finite"));
    }

    #[rstest]
    fn test_deserialize_infinity_fails() {
        use serde::de::{
            IntoDeserializer,
            value::{Error as ValueError, F64Deserializer},
        };
        let deserializer: F64Deserializer<ValueError> = f64::INFINITY.into_deserializer();
        let result: Result<UnixNanos, _> = UnixNanos::deserialize(deserializer);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be finite"));
    }

    #[rstest]
    fn test_deserialize_negative_infinity_fails() {
        use serde::de::{
            IntoDeserializer,
            value::{Error as ValueError, F64Deserializer},
        };
        let deserializer: F64Deserializer<ValueError> = f64::NEG_INFINITY.into_deserializer();
        let result: Result<UnixNanos, _> = UnixNanos::deserialize(deserializer);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be finite"));
    }

    #[rstest]
    fn test_deserialize_overflow_float_fails() {
        // Test a float that would overflow u64 when converted to nanoseconds
        // u64::MAX is ~18.4e18, so u64::MAX / 1e9 = ~18.4e9 seconds
        let result: Result<UnixNanos, _> = serde_json::from_str("1e20");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of range"));
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

    #[rstest]
    #[should_panic(expected = "UnixNanos value exceeds i64::MAX")]
    fn test_as_i64_overflow_panics() {
        let nanos = UnixNanos::from(u64::MAX);
        let _ = nanos.as_i64(); // Should panic
    }

    ////////////////////////////////////////////////////////////////////////////////
    // Property-based testing
    ////////////////////////////////////////////////////////////////////////////////

    use proptest::prelude::*;

    fn unix_nanos_strategy() -> impl Strategy<Value = UnixNanos> {
        prop_oneof![
            // Small values
            0u64..1_000_000u64,
            // Medium values (microseconds range)
            1_000_000u64..1_000_000_000_000u64,
            // Large values (nanoseconds since 1970, but safe for arithmetic)
            1_000_000_000_000u64..=i64::MAX as u64,
            // Edge cases
            Just(0u64),
            Just(1u64),
            Just(1_000_000_000u64),             // 1 second in nanos
            Just(1_000_000_000_000u64),         // ~2001 timestamp
            Just(1_700_000_000_000_000_000u64), // ~2023 timestamp
            Just((i64::MAX / 2) as u64),        // Safe for doubling
        ]
        .prop_map(UnixNanos::from)
    }

    fn unix_nanos_pair_strategy() -> impl Strategy<Value = (UnixNanos, UnixNanos)> {
        (unix_nanos_strategy(), unix_nanos_strategy())
    }

    proptest! {
        #[rstest]
        fn prop_unix_nanos_construction_roundtrip(value in 0u64..=i64::MAX as u64) {
            let nanos = UnixNanos::from(value);
            prop_assert_eq!(nanos.as_u64(), value);
            prop_assert_eq!(nanos.as_f64(), value as f64);

            // Test i64 conversion only for values within i64 range
            if i64::try_from(value).is_ok() {
                prop_assert_eq!(nanos.as_i64(), value as i64);
            }
        }

        #[rstest]
        fn prop_unix_nanos_addition_commutative(
            (nanos1, nanos2) in unix_nanos_pair_strategy()
        ) {
            // Addition should be commutative when no overflow occurs
            if let (Some(sum1), Some(sum2)) = (
                nanos1.checked_add(nanos2.as_u64()),
                nanos2.checked_add(nanos1.as_u64())
            ) {
                prop_assert_eq!(sum1, sum2, "Addition should be commutative");
            }
        }

        #[rstest]
        fn prop_unix_nanos_addition_associative(
            nanos1 in unix_nanos_strategy(),
            nanos2 in unix_nanos_strategy(),
            nanos3 in unix_nanos_strategy(),
        ) {
            // Addition should be associative when no overflow occurs
            if let (Some(sum1), Some(sum2)) = (
                nanos1.as_u64().checked_add(nanos2.as_u64()),
                nanos2.as_u64().checked_add(nanos3.as_u64())
            )
                && let (Some(left), Some(right)) = (
                    sum1.checked_add(nanos3.as_u64()),
                    nanos1.as_u64().checked_add(sum2)
                ) {
                    let left_result = UnixNanos::from(left);
                    let right_result = UnixNanos::from(right);
                    prop_assert_eq!(left_result, right_result, "Addition should be associative");
                }
        }

        #[rstest]
        fn prop_unix_nanos_subtraction_inverse(
            (nanos1, nanos2) in unix_nanos_pair_strategy()
        ) {
            // Subtraction should be the inverse of addition when no underflow occurs
            if let Some(sum) = nanos1.checked_add(nanos2.as_u64()) {
                let diff = sum - nanos2;
                prop_assert_eq!(diff, nanos1, "Subtraction should be inverse of addition");
            }
        }

        #[rstest]
        fn prop_unix_nanos_zero_identity(nanos in unix_nanos_strategy()) {
            // Zero should be additive identity
            let zero = UnixNanos::default();
            prop_assert_eq!(nanos + zero, nanos, "Zero should be additive identity");
            prop_assert_eq!(zero + nanos, nanos, "Zero should be additive identity (commutative)");
            prop_assert!(zero.is_zero(), "Zero should be recognized as zero");
        }

        #[rstest]
        fn prop_unix_nanos_ordering_consistency(
            (nanos1, nanos2) in unix_nanos_pair_strategy()
        ) {
            // Ordering operations should be consistent
            let eq = nanos1 == nanos2;
            let lt = nanos1 < nanos2;
            let gt = nanos1 > nanos2;
            let le = nanos1 <= nanos2;
            let ge = nanos1 >= nanos2;

            // Exactly one of eq, lt, gt should be true
            let exclusive_count = [eq, lt, gt].iter().filter(|&&x| x).count();
            prop_assert_eq!(exclusive_count, 1, "Exactly one of ==, <, > should be true");

            // Consistency checks
            prop_assert_eq!(le, eq || lt, "<= should equal == || <");
            prop_assert_eq!(ge, eq || gt, ">= should equal == || >");
            prop_assert_eq!(lt, nanos2 > nanos1, "< should be symmetric with >");
            prop_assert_eq!(le, nanos2 >= nanos1, "<= should be symmetric with >=");
        }

        #[rstest]
        fn prop_unix_nanos_string_roundtrip(nanos in unix_nanos_strategy()) {
            // String serialization should round-trip correctly
            let string_repr = nanos.to_string();
            let parsed = UnixNanos::from_str(&string_repr);
            prop_assert!(parsed.is_ok(), "String parsing should succeed for valid UnixNanos");
            if let Ok(parsed_nanos) = parsed {
                prop_assert_eq!(parsed_nanos, nanos, "String should round-trip exactly");
            }
        }

        #[rstest]
        fn prop_unix_nanos_datetime_conversion(nanos in unix_nanos_strategy()) {
            // DateTime conversion should be consistent (only test values within i64 range)
            if i64::try_from(nanos.as_u64()).is_ok() {
                let datetime = nanos.to_datetime_utc();
                let converted_back = UnixNanos::from(datetime);
                prop_assert_eq!(converted_back, nanos, "DateTime conversion should round-trip");

                // RFC3339 string should also round-trip for valid dates
                let rfc3339 = nanos.to_rfc3339();
                if let Ok(parsed_from_rfc3339) = UnixNanos::from_str(&rfc3339) {
                    prop_assert_eq!(parsed_from_rfc3339, nanos, "RFC3339 string should round-trip");
                }
            }
        }

        #[rstest]
        fn prop_unix_nanos_duration_since(
            (nanos1, nanos2) in unix_nanos_pair_strategy()
        ) {
            // duration_since should be consistent with comparison and arithmetic
            let duration = nanos1.duration_since(&nanos2);

            if nanos1 >= nanos2 {
                // If nanos1 >= nanos2, duration should be Some and equal to difference
                prop_assert!(duration.is_some(), "Duration should be Some when first >= second");
                if let Some(dur) = duration {
                    prop_assert_eq!(dur, nanos1.as_u64() - nanos2.as_u64(),
                        "Duration should equal the difference");
                    prop_assert_eq!(nanos2 + dur, nanos1.as_u64(),
                        "second + duration should equal first");
                }
            } else {
                // If nanos1 < nanos2, duration should be None
                prop_assert!(duration.is_none(), "Duration should be None when first < second");
            }
        }

        #[rstest]
        fn prop_unix_nanos_checked_arithmetic(
            (nanos1, nanos2) in unix_nanos_pair_strategy()
        ) {
            // Checked arithmetic should be consistent with regular arithmetic when no overflow/underflow
            let checked_add = nanos1.checked_add(nanos2.as_u64());
            let checked_sub = nanos1.checked_sub(nanos2.as_u64());

            // If checked_add succeeds, regular addition should produce the same result
            if let Some(sum) = checked_add
                && nanos1.as_u64().checked_add(nanos2.as_u64()).is_some() {
                    prop_assert_eq!(sum, nanos1 + nanos2, "Checked add should match regular add when no overflow");
                }

            // If checked_sub succeeds, regular subtraction should produce the same result
            if let Some(diff) = checked_sub
                && nanos1.as_u64() >= nanos2.as_u64() {
                    prop_assert_eq!(diff, nanos1 - nanos2, "Checked sub should match regular sub when no underflow");
                }
        }

        #[rstest]
        fn prop_unix_nanos_saturating_arithmetic(
            (nanos1, nanos2) in unix_nanos_pair_strategy()
        ) {
            // Saturating arithmetic should never panic and produce reasonable results
            let sat_add = nanos1.saturating_add_ns(nanos2.as_u64());
            let sat_sub = nanos1.saturating_sub_ns(nanos2.as_u64());

            // Saturating add should be >= both operands
            prop_assert!(sat_add >= nanos1, "Saturating add result should be >= first operand");
            prop_assert!(sat_add.as_u64() >= nanos2.as_u64(), "Saturating add result should be >= second operand");

            // Saturating sub should be <= first operand
            prop_assert!(sat_sub <= nanos1, "Saturating sub result should be <= first operand");

            // If no overflow/underflow would occur, saturating should match checked
            if let Some(checked_sum) = nanos1.checked_add(nanos2.as_u64()) {
                prop_assert_eq!(sat_add, checked_sum, "Saturating add should match checked add when no overflow");
            } else {
                prop_assert_eq!(sat_add, UnixNanos::from(u64::MAX), "Saturating add should be MAX on overflow");
            }

            if let Some(checked_diff) = nanos1.checked_sub(nanos2.as_u64()) {
                prop_assert_eq!(sat_sub, checked_diff, "Saturating sub should match checked sub when no underflow");
            } else {
                prop_assert_eq!(sat_sub, UnixNanos::default(), "Saturating sub should be zero on underflow");
            }
        }
    }
}

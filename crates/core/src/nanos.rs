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

//! Nanosecond timestamp and duration types.
//!
//! [`UnixNanos`] represents a timestamp since the UNIX epoch, while [`DurationNanos`]
//! represents an unsigned elapsed duration. Timestamp differences produce durations, and
//! timestamps accept durations for arithmetic so the two concepts cannot be mixed implicitly.
//!
//! # Features
//!
//! - Zero-cost abstraction with appropriate operator implementations.
//! - Conversion to/from `Timestamp` and [`std::time::Duration`].
//! - RFC 3339 string formatting.
//! - Duration calculations.
//! - Flexible parsing and serialization.
//!
//! # Parsing and Serialization
//!
//! `UnixNanos` can be created from and serialized to various formats:
//!
//! - Integer values are interpreted as nanoseconds since the UNIX epoch.
//! - Floating-point values are interpreted as seconds since the UNIX epoch (converted to nanoseconds
//!   using truncation, not rounding, for consistency with [`secs_to_nanos`](crate::datetime::secs_to_nanos)).
//! - String values may be:
//!   - A numeric string (interpreted as nanoseconds).
//!   - A floating-point string (interpreted as seconds, converted to nanoseconds).
//!   - An RFC 3339 formatted timestamp (ISO 8601 with timezone).
//!   - A simple date string in YYYY-MM-DD format (interpreted as midnight UTC on that date).
//!
//! # Limitations
//!
//! - Negative timestamps are invalid and will result in an error.
//! - Arithmetic operations will panic on overflow/underflow rather than wrapping.
//! - The `as_i64()` method will panic for timestamps beyond approximately year 2262
//!   (when nanoseconds exceed `i64::MAX`).

use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Add, AddAssign, Deref, Div, DivAssign, Mul, MulAssign, Sub, SubAssign},
    str::FromStr,
    time::{Duration, SystemTime},
};

use jiff::{SignedDuration, Timestamp, civil::Date, tz::Offset};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, Visitor},
};
use thiserror::Error;

use crate::datetime::{
    NANOSECONDS_IN_DAY, NANOSECONDS_IN_MICROSECOND, NANOSECONDS_IN_MILLISECOND,
    NANOSECONDS_IN_MINUTE, NANOSECONDS_IN_SECOND, SECONDS_IN_HOUR, U64_UPPER_BOUND_F64,
};

/// Represents an unsigned duration in nanoseconds.
#[repr(transparent)]
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct DurationNanos(u64);

/// Error returned when a duration cannot be represented as nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("duration {value} {unit} exceeds the nanosecond range")]
pub struct DurationNanosOutOfRangeError {
    value: u64,
    unit: &'static str,
}

impl DurationNanos {
    /// A duration of zero nanoseconds.
    pub const ZERO: Self = Self(0);

    /// The maximum duration representable by this type.
    pub const MAX: Self = Self(u64::MAX);

    /// Creates a duration from an exact nanosecond count.
    #[must_use]
    pub const fn new(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Creates a duration from a number of whole microseconds.
    ///
    /// # Panics
    ///
    /// Panics if the result exceeds [`DurationNanos::MAX`].
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        match Self::try_from_micros(micros) {
            Ok(duration) => duration,
            Err(_) => panic!("DurationNanos overflow in from_micros"),
        }
    }

    /// Creates a duration from a number of whole microseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the result exceeds [`DurationNanos::MAX`].
    pub const fn try_from_micros(micros: u64) -> Result<Self, DurationNanosOutOfRangeError> {
        Self::try_from_units(micros, NANOSECONDS_IN_MICROSECOND, "microseconds")
    }

    /// Creates a duration from a number of whole milliseconds.
    ///
    /// # Panics
    ///
    /// Panics if the result exceeds [`DurationNanos::MAX`].
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        match Self::try_from_millis(millis) {
            Ok(duration) => duration,
            Err(_) => panic!("DurationNanos overflow in from_millis"),
        }
    }

    /// Creates a duration from a number of whole milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the result exceeds [`DurationNanos::MAX`].
    pub const fn try_from_millis(millis: u64) -> Result<Self, DurationNanosOutOfRangeError> {
        Self::try_from_units(millis, NANOSECONDS_IN_MILLISECOND, "milliseconds")
    }

    /// Creates a duration from a number of whole seconds.
    ///
    /// # Panics
    ///
    /// Panics if the result exceeds [`DurationNanos::MAX`].
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        match Self::try_from_secs(secs) {
            Ok(duration) => duration,
            Err(_) => panic!("DurationNanos overflow in from_secs"),
        }
    }

    /// Creates a duration from a number of whole seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the result exceeds [`DurationNanos::MAX`].
    pub const fn try_from_secs(secs: u64) -> Result<Self, DurationNanosOutOfRangeError> {
        Self::try_from_units(secs, NANOSECONDS_IN_SECOND, "seconds")
    }

    /// Creates a duration from a number of whole minutes.
    ///
    /// # Panics
    ///
    /// Panics if the result exceeds [`DurationNanos::MAX`].
    #[must_use]
    pub const fn from_mins(mins: u64) -> Self {
        match Self::try_from_mins(mins) {
            Ok(duration) => duration,
            Err(_) => panic!("DurationNanos overflow in from_mins"),
        }
    }

    /// Creates a duration from a number of whole minutes.
    ///
    /// # Errors
    ///
    /// Returns an error if the result exceeds [`DurationNanos::MAX`].
    pub const fn try_from_mins(mins: u64) -> Result<Self, DurationNanosOutOfRangeError> {
        Self::try_from_units(mins, NANOSECONDS_IN_MINUTE, "minutes")
    }

    /// Creates a duration from a number of whole hours.
    ///
    /// # Panics
    ///
    /// Panics if the result exceeds [`DurationNanos::MAX`].
    #[must_use]
    pub const fn from_hours(hours: u64) -> Self {
        match Self::try_from_hours(hours) {
            Ok(duration) => duration,
            Err(_) => panic!("DurationNanos overflow in from_hours"),
        }
    }

    /// Creates a duration from a number of whole hours.
    ///
    /// # Errors
    ///
    /// Returns an error if the result exceeds [`DurationNanos::MAX`].
    pub const fn try_from_hours(hours: u64) -> Result<Self, DurationNanosOutOfRangeError> {
        Self::try_from_units(hours, SECONDS_IN_HOUR * NANOSECONDS_IN_SECOND, "hours")
    }

    /// Creates a duration from a number of whole days.
    ///
    /// # Panics
    ///
    /// Panics if the result exceeds [`DurationNanos::MAX`].
    #[must_use]
    pub const fn from_days(days: u64) -> Self {
        match Self::try_from_days(days) {
            Ok(duration) => duration,
            Err(_) => panic!("DurationNanos overflow in from_days"),
        }
    }

    /// Creates a duration from a number of whole days.
    ///
    /// # Errors
    ///
    /// Returns an error if the result exceeds [`DurationNanos::MAX`].
    pub const fn try_from_days(days: u64) -> Result<Self, DurationNanosOutOfRangeError> {
        Self::try_from_units(days, NANOSECONDS_IN_DAY, "days")
    }

    const fn try_from_units(
        value: u64,
        nanos_per_unit: u64,
        unit: &'static str,
    ) -> Result<Self, DurationNanosOutOfRangeError> {
        match value.checked_mul(nanos_per_unit) {
            Some(nanos) => Ok(Self(nanos)),
            None => Err(DurationNanosOutOfRangeError { value, unit }),
        }
    }

    /// Returns `true` if the duration is zero.
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Returns the exact duration in nanoseconds as `u64`.
    #[must_use]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Returns the total duration in whole microseconds.
    #[must_use]
    pub const fn as_micros(&self) -> u64 {
        self.0 / NANOSECONDS_IN_MICROSECOND
    }

    /// Returns the total duration in whole milliseconds.
    #[must_use]
    pub const fn as_millis(&self) -> u64 {
        self.0 / NANOSECONDS_IN_MILLISECOND
    }

    /// Returns the total duration in whole seconds.
    #[must_use]
    pub const fn as_secs(&self) -> u64 {
        self.0 / NANOSECONDS_IN_SECOND
    }

    /// Returns the total duration in seconds as `f64`.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "subnanosecond precision is unavailable and large durations may lose precision"
    )]
    pub const fn as_secs_f64(&self) -> f64 {
        self.as_secs() as f64 + self.subsec_nanos() as f64 / NANOSECONDS_IN_SECOND as f64
    }

    /// Returns the total duration in whole minutes.
    #[must_use]
    pub const fn as_mins(&self) -> u64 {
        self.0 / NANOSECONDS_IN_MINUTE
    }

    /// Returns the total duration in whole hours.
    #[must_use]
    pub const fn as_hours(&self) -> u64 {
        self.0 / (SECONDS_IN_HOUR * NANOSECONDS_IN_SECOND)
    }

    /// Returns the total duration in whole 24-hour days.
    #[must_use]
    pub const fn as_days(&self) -> u64 {
        self.0 / NANOSECONDS_IN_DAY
    }

    /// Returns the fractional part of this duration in whole milliseconds.
    #[must_use]
    pub const fn subsec_millis(&self) -> u64 {
        (self.0 % NANOSECONDS_IN_SECOND) / NANOSECONDS_IN_MILLISECOND
    }

    /// Returns the fractional part of this duration in whole microseconds.
    #[must_use]
    pub const fn subsec_micros(&self) -> u64 {
        (self.0 % NANOSECONDS_IN_SECOND) / NANOSECONDS_IN_MICROSECOND
    }

    /// Returns the fractional part of this duration in nanoseconds.
    #[must_use]
    pub const fn subsec_nanos(&self) -> u64 {
        self.0 % NANOSECONDS_IN_SECOND
    }

    /// Returns `Some(self + rhs)` or `None` if the addition would overflow.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns `Some(self - rhs)` or `None` if the subtraction would underflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Adds `rhs`, saturating at [`DurationNanos::MAX`].
    #[must_use]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    /// Subtracts `rhs`, saturating at zero.
    #[must_use]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// Returns `Some(self * rhs)` or `None` if the multiplication would overflow.
    #[must_use]
    pub const fn checked_mul(self, rhs: u64) -> Option<Self> {
        match self.0.checked_mul(rhs) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Multiplies by `rhs`, saturating at [`DurationNanos::MAX`].
    #[must_use]
    pub const fn saturating_mul(self, rhs: u64) -> Self {
        Self(self.0.saturating_mul(rhs))
    }

    /// Returns `Some(self / rhs)` or `None` if `rhs` is zero.
    #[must_use]
    pub const fn checked_div(self, rhs: u64) -> Option<Self> {
        match self.0.checked_div(rhs) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

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

    /// Returns the timestamp as seconds, truncating sub-second precision.
    #[must_use]
    pub const fn as_seconds(&self) -> u64 {
        self.0 / NANOSECONDS_IN_SECOND
    }

    /// Returns the timestamp as milliseconds, truncating sub-millisecond precision.
    #[must_use]
    pub const fn as_millis(&self) -> u64 {
        self.0 / NANOSECONDS_IN_MILLISECOND
    }

    /// Returns the timestamp as microseconds, truncating sub-microsecond precision.
    #[must_use]
    pub const fn as_micros(&self) -> u64 {
        self.0 / NANOSECONDS_IN_MICROSECOND
    }

    /// Creates a new [`UnixNanos`] from a second timestamp.
    ///
    /// # Panics
    ///
    /// Panics if the result overflows `u64`.
    #[must_use]
    pub const fn from_seconds(seconds: u64) -> Self {
        match seconds.checked_mul(NANOSECONDS_IN_SECOND) {
            Some(nanos) => Self(nanos),
            None => panic!("UnixNanos overflow in from_seconds"),
        }
    }

    /// Creates a new [`UnixNanos`] from a millisecond timestamp.
    ///
    /// # Panics
    ///
    /// Panics if the result overflows `u64`.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        match millis.checked_mul(NANOSECONDS_IN_MILLISECOND) {
            Some(nanos) => Self(nanos),
            None => panic!("UnixNanos overflow in from_millis"),
        }
    }

    /// Creates a new [`UnixNanos`] from a signed millisecond timestamp.
    ///
    /// Returns `None` if `millis` is negative or the result overflows `u64`.
    #[must_use]
    pub const fn from_millis_checked(millis: i64) -> Option<Self> {
        Self::from_units_checked(millis, NANOSECONDS_IN_MILLISECOND)
    }

    /// Creates a new [`UnixNanos`] from a microsecond timestamp.
    ///
    /// # Panics
    ///
    /// Panics if the result overflows `u64`.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        match micros.checked_mul(NANOSECONDS_IN_MICROSECOND) {
            Some(nanos) => Self(nanos),
            None => panic!("UnixNanos overflow in from_micros"),
        }
    }

    /// Creates a new [`UnixNanos`] from a signed microsecond timestamp.
    ///
    /// Returns `None` if `micros` is negative or the result overflows `u64`.
    #[must_use]
    pub const fn from_micros_checked(micros: i64) -> Option<Self> {
        Self::from_units_checked(micros, NANOSECONDS_IN_MICROSECOND)
    }

    const fn from_units_checked(value: i64, nanos_per_unit: u64) -> Option<Self> {
        if value < 0 {
            return None;
        }

        match value.cast_unsigned().checked_mul(nanos_per_unit) {
            Some(nanos) => Some(Self(nanos)),
            None => None,
        }
    }

    /// Returns the underlying value as `i64`.
    ///
    /// # Panics
    ///
    /// Panics if the value exceeds `i64::MAX` (approximately year 2262).
    #[must_use]
    pub const fn as_i64(&self) -> i64 {
        assert!(
            self.0 <= i64::MAX.cast_unsigned(),
            "UnixNanos value exceeds i64::MAX"
        );
        self.0.cast_signed()
    }

    /// Returns the underlying value as `f64`.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "u64 to f64 is inherently lossy above 2^53; accepted for float interop"
    )]
    pub const fn as_f64(&self) -> f64 {
        self.0 as f64
    }

    /// Converts the underlying value to a datetime (UTC).
    ///
    /// # Panics
    ///
    /// Panics if Jiff's supported timestamp range no longer includes all `u64` nanosecond values.
    #[must_use]
    pub fn to_datetime_utc(&self) -> Timestamp {
        Timestamp::from_nanosecond(i128::from(self.0))
            .expect("UnixNanos is within Jiff's timestamp range")
    }

    /// Converts the underlying value to an ISO 8601 (RFC 3339) string.
    #[must_use]
    pub fn to_rfc3339(&self) -> String {
        let datetime = self.to_datetime_utc();
        let display = datetime.display_with_offset(Offset::UTC);

        match datetime.subsec_nanosecond() {
            0 => format!("{display:.0}"),
            nanos if nanos % 1_000_000 == 0 => format!("{display:.3}"),
            nanos if nanos % 1_000 == 0 => format!("{display:.6}"),
            _ => format!("{display:.9}"),
        }
    }

    /// Calculates the duration in nanoseconds since another [`UnixNanos`] instance.
    ///
    /// Returns `Some(duration)` if `self` is later than `other`, otherwise `None` if `other` is
    /// greater than `self` (indicating a negative duration is not possible with `DurationNanos`).
    #[must_use]
    pub const fn duration_since(&self, other: &Self) -> Option<DurationNanos> {
        match self.0.checked_sub(other.0) {
            Some(duration) => Some(DurationNanos(duration)),
            None => None,
        }
    }

    /// Calculates the duration in nanoseconds since `earlier`, saturating at zero.
    #[must_use]
    pub const fn saturating_duration_since(&self, earlier: Self) -> DurationNanos {
        DurationNanos(self.0.saturating_sub(earlier.0))
    }

    /// Rounds this timestamp down to the nearest multiple of `interval` since the UNIX epoch.
    ///
    /// # Panics
    ///
    /// Panics if `interval` is zero.
    #[must_use]
    pub const fn floor(self, interval: DurationNanos) -> Self {
        assert!(
            !interval.is_zero(),
            "cannot floor UnixNanos to a zero interval"
        );
        Self(self.0 - self.0 % interval.0)
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
            return f64_seconds_to_nanos(float_value).map(Self);
        }

        // The legacy parser accepted upper/lowercase `T` and a space separator, but not RFC 9557
        // annotations. Preserve that input contract instead of adopting Jiff's broader grammar.
        let is_compatible_rfc3339 = matches!(s.as_bytes().get(10), Some(b'T' | b't' | b' '))
            && !s.as_bytes().contains(&b'[');
        if is_compatible_rfc3339 && let Ok(datetime) = s.parse::<Timestamp>() {
            let nanos = datetime.as_nanosecond();
            let nanos = u64::try_from(nanos)
                .map_err(|_| "Unix timestamp cannot be negative".to_string())?;
            return Ok(Self(nanos));
        }

        // The legacy `%Y-%m-%d` parser accepted one- or two-digit months and days.
        if let Ok(date) = Date::strptime("%Y-%m-%d", s) {
            let datetime = date
                .at(0, 0, 0, 0)
                .to_zoned(jiff::tz::TimeZone::UTC)
                .map_err(|e| e.to_string())?;
            let nanos = datetime.timestamp().as_nanosecond();
            let nanos = u64::try_from(nanos)
                .map_err(|_| "Unix timestamp cannot be negative".to_string())?;
            return Ok(Self(nanos));
        }

        Err(format!("Invalid format: {s}"))
    }

    /// Returns `Some(self + rhs)` or `None` if the addition would overflow
    #[must_use]
    pub const fn checked_add(self, rhs: DurationNanos) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns `Some(self - rhs)` or `None` if the subtraction would underflow
    #[must_use]
    pub const fn checked_sub(self, rhs: DurationNanos) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Adds `rhs`, saturating at [`UnixNanos::max`].
    #[must_use]
    pub const fn saturating_add(self, rhs: DurationNanos) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    /// Subtracts `rhs`, saturating at zero.
    #[must_use]
    pub const fn saturating_sub(self, rhs: DurationNanos) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

// Converts non-negative float seconds to nanoseconds, truncating (not rounding)
// sub-nanosecond precision for consistency with `datetime::secs_to_nanos`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is checked finite, non-negative, and within u64 range before the cast"
)]
fn f64_seconds_to_nanos(value: f64) -> Result<u64, String> {
    if !value.is_finite() {
        return Err(format!("Unix timestamp must be finite, was {value}"));
    }

    if value < 0.0 {
        return Err("Unix timestamp cannot be negative".to_string());
    }

    // Convert seconds to nanoseconds while checking for overflow.
    // We perform the multiplication in `f64`, then validate the
    // result fits inside `u64` *before* truncating / casting.
    let nanos_f64 = value * 1_000_000_000.0;

    if nanos_f64 >= U64_UPPER_BOUND_F64 {
        return Err(format!("Unix timestamp {value} seconds is out of range"));
    }

    Ok(nanos_f64.trunc() as u64)
}

impl From<DurationNanos> for Duration {
    fn from(value: DurationNanos) -> Self {
        Self::from_nanos(value.0)
    }
}

impl From<DurationNanos> for SignedDuration {
    fn from(value: DurationNanos) -> Self {
        Self::from_nanos_i128(i128::from(value.0))
    }
}

impl TryFrom<SignedDuration> for DurationNanos {
    type Error = std::num::TryFromIntError;

    fn try_from(value: SignedDuration) -> Result<Self, Self::Error> {
        u64::try_from(value.as_nanos()).map(Self)
    }
}

impl TryFrom<Duration> for DurationNanos {
    type Error = std::num::TryFromIntError;

    fn try_from(value: Duration) -> Result<Self, Self::Error> {
        u64::try_from(value.as_nanos()).map(Self)
    }
}

/// Adds two [`DurationNanos`] values.
///
/// # Panics
///
/// Panics if the result exceeds [`DurationNanos::MAX`]. Use
/// [`DurationNanos::checked_add`] or [`DurationNanos::saturating_add`] for explicit overflow
/// handling.
impl Add for DurationNanos {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(rhs)
            .expect("DurationNanos overflow in addition")
    }
}

/// Subtracts one [`DurationNanos`] value from another.
///
/// # Panics
///
/// Panics if `rhs` exceeds `self`. Use [`DurationNanos::checked_sub`] or
/// [`DurationNanos::saturating_sub`] for explicit underflow handling.
impl Sub for DurationNanos {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(rhs)
            .expect("DurationNanos underflow in subtraction")
    }
}

/// Add-assigns a duration.
///
/// # Panics
///
/// Panics if the result exceeds [`DurationNanos::MAX`].
impl AddAssign for DurationNanos {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

/// Sub-assigns a duration.
///
/// # Panics
///
/// Panics if `rhs` exceeds `self`.
impl SubAssign for DurationNanos {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

/// Multiplies a duration by an unsigned scalar.
///
/// # Panics
///
/// Panics if the result exceeds [`DurationNanos::MAX`]. Use
/// [`DurationNanos::checked_mul`] or [`DurationNanos::saturating_mul`] for explicit overflow
/// handling.
impl Mul<u64> for DurationNanos {
    type Output = Self;

    fn mul(self, rhs: u64) -> Self::Output {
        self.checked_mul(rhs)
            .expect("DurationNanos overflow in multiplication")
    }
}

/// Multiply-assigns a duration by an unsigned scalar.
///
/// # Panics
///
/// Panics if the result exceeds [`DurationNanos::MAX`].
impl MulAssign<u64> for DurationNanos {
    fn mul_assign(&mut self, rhs: u64) {
        *self = *self * rhs;
    }
}

/// Divides a duration by an unsigned scalar.
///
/// # Panics
///
/// Panics if `rhs` is zero. Use [`DurationNanos::checked_div`] when the divisor may be zero.
impl Div<u64> for DurationNanos {
    type Output = Self;

    fn div(self, rhs: u64) -> Self::Output {
        self.checked_div(rhs)
            .expect("DurationNanos division by zero")
    }
}

/// Divide-assigns a duration by an unsigned scalar.
///
/// # Panics
///
/// Panics if `rhs` is zero.
impl DivAssign<u64> for DurationNanos {
    fn div_assign(&mut self, rhs: u64) {
        *self = *self / rhs;
    }
}

impl Display for DurationNanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
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

impl From<Timestamp> for UnixNanos {
    fn from(value: Timestamp) -> Self {
        let nanos = value.as_nanosecond();

        assert!(nanos >= 0, "DateTime timestamp cannot be negative: {nanos}");

        Self::from(u64::try_from(nanos).expect("DateTime timestamp out of range for UnixNanos"))
    }
}

impl From<SystemTime> for UnixNanos {
    fn from(value: SystemTime) -> Self {
        let duration = value
            .duration_since(std::time::UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH");

        let nanos =
            u64::try_from(duration.as_nanos()).expect("SystemTime overflowed u64 nanoseconds");

        Self::from(nanos)
    }
}

impl FromStr for UnixNanos {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_string(s).map_err(std::convert::Into::into)
    }
}

/// Returns the elapsed duration between two timestamps.
///
/// # Panics
///
/// Panics if `rhs` is later than `self`. Use [`UnixNanos::duration_since`] or
/// [`UnixNanos::saturating_duration_since`] when the timestamps may be out of order.
impl Sub for UnixNanos {
    type Output = DurationNanos;

    fn sub(self, rhs: Self) -> Self::Output {
        self.duration_since(&rhs)
            .expect("UnixNanos underflow in timestamp subtraction")
    }
}

/// Adds a duration to a timestamp.
///
/// # Panics
///
/// Panics on overflow. Use [`UnixNanos::checked_add`] or [`UnixNanos::saturating_add`] for
/// explicit overflow handling.
impl Add<DurationNanos> for UnixNanos {
    type Output = Self;

    fn add(self, rhs: DurationNanos) -> Self::Output {
        self.checked_add(rhs)
            .expect("UnixNanos overflow in duration addition")
    }
}

/// Subtracts a duration from a timestamp.
///
/// # Panics
///
/// Panics on underflow. Use [`UnixNanos::checked_sub`] or [`UnixNanos::saturating_sub`] for
/// explicit underflow handling.
impl Sub<DurationNanos> for UnixNanos {
    type Output = Self;

    fn sub(self, rhs: DurationNanos) -> Self::Output {
        self.checked_sub(rhs)
            .expect("UnixNanos underflow in duration subtraction")
    }
}

/// Add-assigns a duration to [`UnixNanos`].
///
/// # Panics
///
/// Panics on overflow. This is intentional fail-fast behavior for timestamp arithmetic.
impl AddAssign<DurationNanos> for UnixNanos {
    fn add_assign(&mut self, rhs: DurationNanos) {
        *self = *self + rhs;
    }
}

/// Sub-assigns a duration from [`UnixNanos`].
///
/// # Panics
///
/// Panics on underflow. This is intentional fail-fast behavior for timestamp arithmetic.
impl SubAssign<DurationNanos> for UnixNanos {
    fn sub_assign(&mut self, rhs: DurationNanos) {
        *self = *self - rhs;
    }
}

impl Display for UnixNanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<UnixNanos> for Timestamp {
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
                u64::try_from(value)
                    .map(UnixNanos)
                    .map_err(|_| E::custom("Unix timestamp cannot be negative"))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                f64_seconds_to_nanos(value)
                    .map(UnixNanos)
                    .map_err(E::custom)
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

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;
    use rstest::rstest;

    use super::*;
    use crate::approx_eq;

    fn timestamp(value: &str) -> Timestamp {
        value.parse().unwrap()
    }

    #[rstest]
    fn test_duration_nanos_construction_and_conversion() {
        let duration = DurationNanos::new(123);
        let standard = Duration::from(duration);
        let signed = SignedDuration::from(duration);
        let signed_max = SignedDuration::from(DurationNanos::MAX);

        assert_eq!(duration.as_u64(), 123);
        assert_eq!(standard, Duration::from_nanos(123));
        assert_eq!(DurationNanos::try_from(standard), Ok(duration));
        assert_eq!(signed, SignedDuration::from_nanos(123));
        assert_eq!(DurationNanos::try_from(signed), Ok(duration));
        assert_eq!(signed_max.as_nanos(), i128::from(u64::MAX));
        assert_eq!(DurationNanos::try_from(signed_max), Ok(DurationNanos::MAX));
        assert!(DurationNanos::try_from(SignedDuration::from_nanos(-1)).is_err());
    }

    #[rstest]
    fn test_duration_nanos_unit_construction_and_accessors() {
        let duration = DurationNanos::from_hours(1)
            + DurationNanos::from_mins(2)
            + DurationNanos::from_secs(3)
            + DurationNanos::new(456_789_123);

        assert_eq!(DurationNanos::from_micros(1), DurationNanos::new(1_000));
        assert_eq!(DurationNanos::from_millis(1), DurationNanos::new(1_000_000));
        assert_eq!(DurationNanos::from_days(1), DurationNanos::from_hours(24));
        assert_eq!(duration.as_micros(), 3_723_456_789);
        assert_eq!(duration.as_millis(), 3_723_456);
        assert_eq!(duration.as_secs(), 3_723);
        assert!(approx_eq!(
            f64,
            duration.as_secs_f64(),
            3_723.456_789_123,
            epsilon = 1e-12
        ));
        let expected_max_secs = Duration::from_nanos(u64::MAX).as_secs_f64();
        assert!(approx_eq!(
            f64,
            DurationNanos::MAX.as_secs_f64(),
            expected_max_secs,
            epsilon = expected_max_secs * f64::EPSILON
        ));
        assert_eq!(duration.as_mins(), 62);
        assert_eq!(duration.as_hours(), 1);
        assert_eq!(DurationNanos::from_hours(49).as_days(), 2);
        assert_eq!(duration.subsec_millis(), 456);
        assert_eq!(duration.subsec_micros(), 456_789);
        assert_eq!(duration.subsec_nanos(), 456_789_123);
    }

    #[rstest]
    fn test_duration_nanos_fallible_unit_construction() {
        assert_eq!(
            DurationNanos::try_from_secs(1),
            Ok(DurationNanos::from_secs(1))
        );
        assert_eq!(
            DurationNanos::try_from_secs(u64::MAX)
                .unwrap_err()
                .to_string(),
            "duration 18446744073709551615 seconds exceeds the nanosecond range"
        );
        assert!(DurationNanos::try_from_micros(u64::MAX).is_err());
        assert!(DurationNanos::try_from_millis(u64::MAX).is_err());
        assert!(DurationNanos::try_from_secs(u64::MAX).is_err());
        assert!(DurationNanos::try_from_mins(u64::MAX).is_err());
        assert!(DurationNanos::try_from_hours(u64::MAX).is_err());
        assert!(DurationNanos::try_from_days(u64::MAX).is_err());

        let max_hours = DurationNanos::MAX.as_hours();
        assert_eq!(
            DurationNanos::try_from_hours(max_hours),
            Ok(DurationNanos::from_hours(max_hours))
        );
        assert!(DurationNanos::try_from_hours(max_hours + 1).is_err());

        let max_days = DurationNanos::MAX.as_days();
        assert_eq!(
            DurationNanos::try_from_days(max_days),
            Ok(DurationNanos::from_days(max_days))
        );
        assert!(DurationNanos::try_from_days(max_days + 1).is_err());
    }

    #[rstest]
    fn test_duration_nanos_zero_and_max() {
        assert_eq!(DurationNanos::default(), DurationNanos::ZERO);
        assert!(DurationNanos::ZERO.is_zero());
        assert_eq!(DurationNanos::MAX.as_u64(), u64::MAX);
        assert!(!DurationNanos::MAX.is_zero());
    }

    #[rstest]
    fn test_duration_nanos_layout_matches_u64() {
        assert_eq!(
            std::mem::size_of::<DurationNanos>(),
            std::mem::size_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<DurationNanos>(),
            std::mem::align_of::<u64>()
        );
    }

    #[rstest]
    fn test_duration_nanos_format_and_ordering() {
        let shorter = DurationNanos::new(123);
        let longer = DurationNanos::new(456);

        assert_eq!(shorter.to_string(), "123");
        assert_eq!(format!("{shorter:?}"), "DurationNanos(123)");
        assert!(shorter < longer);
        assert_eq!(shorter, DurationNanos::new(123));
    }

    #[rstest]
    fn test_duration_nanos_serde_preserves_u64_format() {
        let duration = DurationNanos::MAX;
        let json = serde_json::to_string(&duration).unwrap();
        let deserialized: DurationNanos = serde_json::from_str(&json).unwrap();

        assert_eq!(json, u64::MAX.to_string());
        assert_eq!(deserialized, duration);
    }

    #[rstest]
    #[case("-1")]
    #[case("1.5")]
    #[case("\"1\"")]
    fn test_duration_nanos_serde_rejects_non_u64_formats(#[case] json: &str) {
        assert!(serde_json::from_str::<DurationNanos>(json).is_err());
    }

    #[rstest]
    fn test_duration_nanos_checked_arithmetic_boundaries() {
        let zero = DurationNanos::ZERO;
        let one = DurationNanos::new(1);
        let max = DurationNanos::MAX;

        assert_eq!(zero.checked_sub(one), None);
        assert_eq!(max.checked_add(one), None);
        assert_eq!(one.checked_sub(one), Some(zero));
        assert_eq!(zero.checked_add(max), Some(max));
    }

    #[rstest]
    fn test_duration_nanos_saturating_arithmetic_boundaries() {
        let zero = DurationNanos::ZERO;
        let one = DurationNanos::new(1);
        let max = DurationNanos::MAX;

        assert_eq!(zero.saturating_sub(one), zero);
        assert_eq!(max.saturating_add(one), max);
        assert_eq!(max.saturating_mul(2), max);
    }

    #[rstest]
    fn test_duration_nanos_scalar_arithmetic() {
        let duration = DurationNanos::new(12);

        assert_eq!(duration.checked_mul(3), Some(DurationNanos::new(36)));
        assert_eq!(DurationNanos::MAX.checked_mul(2), None);
        assert_eq!(duration.checked_div(3), Some(DurationNanos::new(4)));
        assert_eq!(duration.checked_div(0), None);
        assert_eq!(duration * 3, DurationNanos::new(36));
        assert_eq!(duration / 3, DurationNanos::new(4));
    }

    #[rstest]
    #[should_panic(expected = "DurationNanos overflow in addition")]
    fn test_duration_nanos_addition_panics_on_overflow() {
        let _ = DurationNanos::MAX + DurationNanos::new(1);
    }

    #[rstest]
    #[should_panic(expected = "DurationNanos underflow in subtraction")]
    fn test_duration_nanos_subtraction_panics_on_underflow() {
        let _ = DurationNanos::default() - DurationNanos::new(1);
    }

    #[rstest]
    #[should_panic(expected = "DurationNanos overflow in multiplication")]
    fn test_duration_nanos_multiplication_panics_on_overflow() {
        let _ = DurationNanos::MAX * 2;
    }

    #[rstest]
    #[should_panic(expected = "DurationNanos division by zero")]
    fn test_duration_nanos_division_panics_on_zero() {
        let _ = DurationNanos::new(1) / 0;
    }

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
        assert_eq!(
            datetime.display_with_offset(Offset::UTC).to_string(),
            expected
        );
    }

    #[rstest]
    #[case(0, "1970-01-01T00:00:00+00:00")]
    #[case(1_000_000_000, "1970-01-01T00:00:01+00:00")]
    #[case(1_000_000_000_000_000_000, "2001-09-09T01:46:40+00:00")]
    #[case(1_500_000_000_000_000_000, "2017-07-14T02:40:00+00:00")]
    #[case(1_500_000_000_500_000_000, "2017-07-14T02:40:00.500+00:00")]
    #[case(1_500_000_000_123_456_000, "2017-07-14T02:40:00.123456+00:00")]
    #[case(1_500_000_000_123_456_789, "2017-07-14T02:40:00.123456789+00:00")]
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
    fn test_from_str_date() {
        let nanos: UnixNanos = "2024-02-10".parse().unwrap();
        assert_eq!(nanos.as_u64(), 1_707_523_200_000_000_000);
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
        let datetime = Timestamp::from_second(1_000_000_000).unwrap();
        let nanos = UnixNanos::from(datetime);
        assert_eq!(nanos.as_u64(), 1_000_000_000_000_000_000);
    }

    #[rstest]
    fn test_from_system_time() {
        let system_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        let nanos = UnixNanos::from(system_time);
        assert_eq!(nanos.as_u64(), 1_000_000_000_000_000_000);
    }

    #[rstest]
    #[should_panic(expected = "SystemTime before UNIX EPOCH")]
    fn test_from_system_time_before_epoch() {
        let system_time = std::time::UNIX_EPOCH - std::time::Duration::from_secs(1);
        let _ = UnixNanos::from(system_time);
    }

    #[rstest]
    #[should_panic(expected = "SystemTime overflowed u64 nanoseconds")]
    fn test_from_system_time_overflow_panics() {
        // One second beyond the largest whole-second duration representable in u64 nanoseconds
        let system_time =
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::MAX / 1_000_000_000 + 1);
        let _ = UnixNanos::from(system_time);
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
        let nanos = UnixNanos::from(100);
        let duration = DurationNanos::new(200);
        let result = nanos + duration;
        assert_eq!(result.as_u64(), 300);
    }

    #[rstest]
    fn test_add_assign() {
        let mut nanos = UnixNanos::from(100);
        nanos += DurationNanos::new(50);
        assert_eq!(nanos.as_u64(), 150);
    }

    #[rstest]
    fn test_subtraction() {
        let nanos1 = UnixNanos::from(200);
        let nanos2 = UnixNanos::from(100);
        let result = nanos1 - nanos2;
        assert_eq!(result, DurationNanos::new(100));
    }

    #[rstest]
    fn test_sub_assign() {
        let mut nanos = UnixNanos::from(200);
        nanos -= DurationNanos::new(50);
        assert_eq!(nanos.as_u64(), 150);
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos overflow")]
    fn test_overflow_add() {
        let nanos = UnixNanos::from(u64::MAX);
        let _ = nanos + DurationNanos::new(1);
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos underflow")]
    fn test_overflow_sub() {
        let _ = UnixNanos::default() - DurationNanos::new(1);
    }

    #[rstest]
    #[case(100, 50, Some(DurationNanos::new(50)))]
    #[case(1_000_000_000, 500_000_000, Some(DurationNanos::new(500_000_000)))]
    #[case(u64::MAX, u64::MAX - 1, Some(DurationNanos::new(1)))]
    #[case(50, 50, Some(DurationNanos::ZERO))]
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
        assert_eq!(
            moment.duration_since(&moment),
            Some(DurationNanos::default())
        );
    }

    #[rstest]
    #[case::later(100, 50, DurationNanos::new(50))]
    #[case::same(50, 50, DurationNanos::ZERO)]
    #[case::earlier(50, 100, DurationNanos::ZERO)]
    #[case::full_range(u64::MAX, 0, DurationNanos::new(u64::MAX))]
    fn test_saturating_duration_since(
        #[case] time: u64,
        #[case] earlier: u64,
        #[case] expected: DurationNanos,
    ) {
        assert_eq!(
            UnixNanos::from(time).saturating_duration_since(UnixNanos::from(earlier)),
            expected
        );
    }

    #[rstest]
    #[case(100, 30, 90)]
    #[case(90, 30, 90)]
    #[case(100, 101, 0)]
    #[case(u64::MAX, u64::MAX, u64::MAX)]
    #[case(u64::MAX, 1_000_000_000, 18_446_744_073_000_000_000)]
    fn test_floor(#[case] time: u64, #[case] interval: u64, #[case] expected: u64) {
        assert_eq!(
            UnixNanos::new(time).floor(DurationNanos::new(interval)),
            UnixNanos::new(expected)
        );
    }

    #[rstest]
    #[should_panic(expected = "cannot floor UnixNanos to a zero interval")]
    fn test_floor_panics_for_zero_interval() {
        let _ = UnixNanos::default().floor(DurationNanos::ZERO);
    }

    #[rstest]
    fn test_duration_since_chronological() {
        // Create a reference time (Feb 10, 2024)
        let earlier = timestamp("2024-02-10T12:00:00Z");

        // Create a time 1 hour, 30 minutes, and 45 seconds later (with nanoseconds)
        let later = earlier
            + SignedDuration::from_hours(1)
            + SignedDuration::from_mins(30)
            + SignedDuration::from_secs(45)
            + SignedDuration::from_nanos(500_000_000);

        let earlier_nanos = UnixNanos::from(earlier);
        let later_nanos = UnixNanos::from(later);

        // Calculate expected duration in nanoseconds
        let expected_duration =
            (60 * 60 + 30 * 60 + 45) * NANOSECONDS_IN_SECOND + 500 * NANOSECONDS_IN_MILLISECOND;

        assert_eq!(
            later_nanos.duration_since(&earlier_nanos),
            Some(DurationNanos::new(expected_duration))
        );
        assert_eq!(earlier_nanos.duration_since(&later_nanos), None);
    }

    #[rstest]
    fn test_duration_since_with_edge_cases() {
        // Test with maximum value
        let max = UnixNanos::from(u64::MAX);
        let smaller = UnixNanos::from(u64::MAX - 1000);

        assert_eq!(max.duration_since(&smaller), Some(DurationNanos::new(1000)));
        assert_eq!(smaller.duration_since(&max), None);

        // Test with minimum value
        let min = UnixNanos::default(); // Zero timestamp
        let larger = UnixNanos::from(1000);

        assert_eq!(min.duration_since(&min), Some(DurationNanos::default()));
        assert_eq!(larger.duration_since(&min), Some(DurationNanos::new(1000)));
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
    #[case("2024-2-10", 1_707_523_200_000_000_000)] // Legacy-compatible short month
    #[case("2024-02-1", 1_706_745_600_000_000_000)] // Legacy-compatible short day
    #[case("2024-02-10T14:58:43Z", 1_707_577_123_000_000_000)] // RFC3339 without fractions
    #[case("2024-02-10t14:58:43Z", 1_707_577_123_000_000_000)] // Lowercase RFC3339 separator
    #[case("2024-02-10 14:58:43Z", 1_707_577_123_000_000_000)] // Space RFC3339 separator
    #[case("2024-02-10T14:58:43.456789Z", 1_707_577_123_456_789_000)] // RFC3339 with fractions
    fn test_from_str_formats(#[case] input: &str, #[case] expected: u64) {
        let parsed: UnixNanos = input.parse().unwrap();
        assert_eq!(parsed.as_u64(), expected);
    }

    #[rstest]
    #[case("abc")] // Random string
    #[case("not a timestamp")] // Non-timestamp string
    #[case("2024-02-10 14:58:43")] // Space-separated format (not RFC3339)
    #[case("2024-02-10T14:58:43Z[UTC]")] // RFC 9557 annotation was not accepted previously
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

    #[rstest]
    fn test_checked_add_overflow_returns_none() {
        let max = UnixNanos::from(u64::MAX);
        assert_eq!(max.checked_add(DurationNanos::new(1)), None);
    }

    #[rstest]
    fn test_checked_sub_underflow_returns_none() {
        let zero = UnixNanos::default();
        assert_eq!(zero.checked_sub(DurationNanos::new(1)), None);
    }

    #[rstest]
    fn test_saturating_add_overflow() {
        let max = UnixNanos::from(u64::MAX);
        let result = max.saturating_add(DurationNanos::new(1));
        assert_eq!(result, UnixNanos::from(u64::MAX));
    }

    #[rstest]
    fn test_saturating_sub_underflow() {
        let zero = UnixNanos::default();
        let result = zero.saturating_sub(DurationNanos::new(1));
        assert_eq!(result, UnixNanos::default());
    }

    #[rstest]
    fn test_from_str_float_overflow() {
        // Use scientific notation so we take the floating-point parsing path.
        let input = "2e10"; // 20 billion seconds ~ 634 years (> u64::MAX nanoseconds)
        let err = input.parse::<UnixNanos>().unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[rstest]
    #[case("NaN")]
    #[case("nan")]
    #[case("inf")]
    #[case("-inf")]
    fn test_from_str_non_finite_float_errors(#[case] input: &str) {
        let err = input.parse::<UnixNanos>().unwrap_err();
        assert!(err.to_string().contains("must be finite"));
    }

    #[rstest]
    #[case("-1.5")]
    #[case("-0.000001")]
    fn test_from_str_negative_float_errors(#[case] input: &str) {
        let err = input.parse::<UnixNanos>().unwrap_err();
        assert!(err.to_string().contains("cannot be negative"));
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
    fn test_deserialize_float_uses_truncation() {
        // Truncation (not rounding) for consistency with secs_to_nanos() etc
        let json = "0.9999999999";
        let deserialized: UnixNanos = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.as_u64(), 999_999_999); // Truncated, not rounded to 1B
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot be negative")
        );
    }

    #[rstest]
    fn test_deserialize_negative_float_fails() {
        let json = "-1234.567";
        let result: Result<UnixNanos, _> = serde_json::from_str(json);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot be negative")
        );
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
    fn test_deserialize_float_u64_boundary_fails() {
        let deserializer = serde::de::value::F64Deserializer::<serde::de::value::Error>::new(
            18_446_744_073.709_553,
        );
        let err = UnixNanos::deserialize(deserializer).unwrap_err();
        assert!(err.to_string().contains("out of range"));
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

    #[rstest]
    fn test_as_i64_at_i64_max_boundary() {
        let nanos = UnixNanos::from(i64::MAX.cast_unsigned());
        assert_eq!(nanos.as_i64(), i64::MAX);
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos value exceeds i64::MAX")]
    fn test_as_i64_just_above_i64_max_panics() {
        let nanos = UnixNanos::from(i64::MAX.cast_unsigned() + 1);
        let _ = nanos.as_i64();
    }

    use proptest::prelude::*;

    fn unix_nanos_strategy() -> impl Strategy<Value = UnixNanos> {
        prop_oneof![
            // Small values
            0u64..1_000_000u64,
            // Medium values (microseconds range)
            1_000_000u64..1_000_000_000_000u64,
            // Large values (nanoseconds since 1970)
            1_000_000_000_000u64..=i64::MAX.cast_unsigned(),
            // Values above i64::MAX (sentinel range, GTC/infinity)
            (i64::MAX.cast_unsigned() + 1)..=u64::MAX,
            // Edge cases
            Just(0u64),
            Just(1u64),
            Just(1_000_000_000u64),               // 1 second in nanos
            Just(1_000_000_000_000u64),           // ~2001 timestamp
            Just(1_700_000_000_000_000_000u64),   // ~2023 timestamp
            Just((i64::MAX / 2).cast_unsigned()), // Safe for doubling
            Just(i64::MAX.cast_unsigned()),       // i64 boundary
            Just(u64::MAX),                       // Sentinel / max value
        ]
        .prop_map(UnixNanos::from)
    }

    fn unix_nanos_pair_strategy() -> impl Strategy<Value = (UnixNanos, UnixNanos)> {
        (unix_nanos_strategy(), unix_nanos_strategy())
    }

    fn duration_nanos_strategy() -> impl Strategy<Value = DurationNanos> {
        any::<u64>().prop_map(DurationNanos::new)
    }

    fn duration_nanos_pair_strategy() -> impl Strategy<Value = (DurationNanos, DurationNanos)> {
        (duration_nanos_strategy(), duration_nanos_strategy())
    }

    proptest! {
        #[rstest]
        #[expect(
            clippy::float_cmp,
            clippy::cast_precision_loss,
            reason = "roundtrip: both sides go through the same u64->f64 cast"
        )]
        fn prop_unix_nanos_construction_roundtrip(nanos in unix_nanos_strategy()) {
            let value = nanos.as_u64();
            prop_assert_eq!(UnixNanos::from(value).as_u64(), value);
            prop_assert_eq!(nanos.as_f64(), value as f64);

            // Test i64 conversion only for values within i64 range
            if i64::try_from(value).is_ok() {
                prop_assert_eq!(nanos.as_i64(), value.cast_signed());
            }
        }

        #[rstest]
        fn prop_duration_nanos_addition_commutative(
            (duration1, duration2) in duration_nanos_pair_strategy()
        ) {
            if let (Some(sum1), Some(sum2)) = (
                duration1.checked_add(duration2),
                duration2.checked_add(duration1)
            ) {
                prop_assert_eq!(sum1, sum2, "Addition should be commutative");
            }
        }

        #[rstest]
        fn prop_duration_nanos_addition_associative(
            duration1 in duration_nanos_strategy(),
            duration2 in duration_nanos_strategy(),
            duration3 in duration_nanos_strategy(),
        ) {
            let expected = duration1
                .checked_add(duration2)
                .and_then(|sum| sum.checked_add(duration3));

            if let Some(expected) = expected {
                let left = (duration1 + duration2) + duration3;
                let right = duration1 + (duration2 + duration3);
                prop_assert_eq!(left, expected);
                prop_assert_eq!(right, expected);
            }
        }

        #[rstest]
        fn prop_unix_nanos_duration_arithmetic_roundtrip(
            nanos in unix_nanos_strategy(),
            duration in duration_nanos_strategy(),
        ) {
            if let Some(sum) = nanos.checked_add(duration) {
                prop_assert_eq!(sum - duration, nanos);
                prop_assert_eq!(sum - nanos, duration);
            }
        }

        #[rstest]
        fn prop_duration_nanos_zero_identity(duration in duration_nanos_strategy()) {
            let zero = DurationNanos::default();
            prop_assert_eq!(duration + zero, duration);
            prop_assert_eq!(zero + duration, duration);
            prop_assert!(zero.is_zero());
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
            let saturating_duration = nanos1.saturating_duration_since(nanos2);

            if nanos1 >= nanos2 {
                // If nanos1 >= nanos2, duration should be Some and equal to difference
                prop_assert!(duration.is_some(), "Duration should be Some when first >= second");
                if let Some(dur) = duration {
                    prop_assert_eq!(dur.as_u64(), nanos1.as_u64() - nanos2.as_u64(),
                        "Duration should equal the difference");
                    prop_assert_eq!(saturating_duration, dur,
                        "Saturating duration should equal the difference");
                    prop_assert_eq!(nanos2 + dur, nanos1,
                        "second + duration should equal first");
                }
            } else {
                // If nanos1 < nanos2, duration should be None
                prop_assert!(duration.is_none(), "Duration should be None when first < second");
                prop_assert_eq!(saturating_duration, DurationNanos::default(),
                    "Saturating duration should be zero when first < second");
            }
        }

        #[rstest]
        fn prop_unix_nanos_checked_arithmetic(
            nanos in unix_nanos_strategy(),
            duration in duration_nanos_strategy(),
        ) {
            let checked_add = nanos.checked_add(duration);
            let checked_sub = nanos.checked_sub(duration);

            if let Some(sum) = checked_add {
                prop_assert_eq!(sum, nanos + duration, "Checked add should match regular add when no overflow");
            }

            if let Some(diff) = checked_sub {
                prop_assert_eq!(diff, nanos - duration, "Checked sub should match regular sub when no underflow");
            }
        }

        #[rstest]
        fn prop_unix_nanos_saturating_arithmetic(
            nanos in unix_nanos_strategy(),
            duration in duration_nanos_strategy(),
        ) {
            let sat_add = nanos.saturating_add(duration);
            let sat_sub = nanos.saturating_sub(duration);

            prop_assert!(sat_add >= nanos, "Saturating add result should be >= timestamp");
            prop_assert!(sat_sub <= nanos, "Saturating sub result should be <= timestamp");

            if let Some(checked_sum) = nanos.checked_add(duration) {
                prop_assert_eq!(sat_add, checked_sum, "Saturating add should match checked add when no overflow");
            } else {
                prop_assert_eq!(sat_add, UnixNanos::from(u64::MAX), "Saturating add should be MAX on overflow");
            }

            if let Some(checked_diff) = nanos.checked_sub(duration) {
                prop_assert_eq!(sat_sub, checked_diff, "Saturating sub should match checked sub when no underflow");
            } else {
                prop_assert_eq!(sat_sub, UnixNanos::default(), "Saturating sub should be zero on underflow");
            }
        }

        #[rstest]
        fn prop_unix_nanos_assign_mirrors_op(
            nanos in unix_nanos_strategy(),
            duration in duration_nanos_strategy(),
        ) {
            if let Some(expected) = nanos.checked_add(duration) {
                let mut add_result = nanos;
                add_result += duration;
                prop_assert_eq!(add_result, expected, "AddAssign should mirror Add");
            }

            if let Some(expected) = nanos.checked_sub(duration) {
                let mut sub_result = nanos;
                sub_result -= duration;
                prop_assert_eq!(sub_result, expected, "SubAssign should mirror Sub");
            }
        }

        #[rstest]
        fn prop_unix_nanos_serde_roundtrip(nanos in unix_nanos_strategy()) {
            let json = serde_json::to_string(&nanos).unwrap();
            let deserialized: UnixNanos = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(deserialized, nanos, "Serde JSON should round-trip exactly");
        }

        #[rstest]
        fn prop_unix_nanos_f64_deserialize_never_panics(val: f64) {
            // Use IntoDeserializer to hit visit_f64 directly,
            // bypassing JSON text encoding ambiguity
            use serde::de::{IntoDeserializer, value::{Error as ValueError, F64Deserializer}};
            let deserializer: F64Deserializer<ValueError> = val.into_deserializer();
            let result = UnixNanos::deserialize(deserializer);

            let upper_bound = 2.0_f64.powi(64);
            if val.is_finite() && val >= 0.0 && val * 1_000_000_000.0 < upper_bound {
                prop_assert!(result.is_ok(), "Should succeed for valid f64: {}", val);
            } else {
                prop_assert!(result.is_err(), "Should error for invalid f64: {}", val);
            }
        }
    }

    #[rstest]
    fn test_from_seconds_zero() {
        let nanos = UnixNanos::from_seconds(0);
        assert_eq!(nanos.as_u64(), 0);
    }

    #[rstest]
    fn test_from_seconds_one() {
        let nanos = UnixNanos::from_seconds(1);
        assert_eq!(nanos.as_u64(), 1_000_000_000);
    }

    #[rstest]
    fn test_from_seconds_realistic_timestamp() {
        let nanos = UnixNanos::from_seconds(1_700_000_000);
        assert_eq!(nanos.as_u64(), 1_700_000_000_000_000_000);
        assert_eq!(nanos.to_datetime_utc(), timestamp("2023-11-14T22:13:20Z"));
    }

    #[rstest]
    fn test_from_seconds_max_safe() {
        let max_seconds = u64::MAX / 1_000_000_000;
        let nanos = UnixNanos::from_seconds(max_seconds);
        assert_eq!(nanos.as_u64(), max_seconds * 1_000_000_000);
    }

    #[rstest]
    fn test_from_millis_zero() {
        let nanos = UnixNanos::from_millis(0);
        assert_eq!(nanos.as_u64(), 0);
    }

    #[rstest]
    fn test_from_millis_one() {
        let nanos = UnixNanos::from_millis(1);
        assert_eq!(nanos.as_u64(), 1_000_000);
    }

    #[rstest]
    fn test_from_millis_one_second() {
        let nanos = UnixNanos::from_millis(1_000);
        assert_eq!(nanos.as_u64(), 1_000_000_000);
    }

    #[rstest]
    fn test_from_millis_realistic_timestamp() {
        // 2023-11-14T22:13:20Z = 1700000000000 ms
        let nanos = UnixNanos::from_millis(1_700_000_000_000);
        assert_eq!(nanos.as_u64(), 1_700_000_000_000_000_000);
        assert_eq!(nanos.to_datetime_utc(), timestamp("2023-11-14T22:13:20Z"));
    }

    #[rstest]
    fn test_from_millis_max_safe() {
        let max_ms = u64::MAX / 1_000_000;
        let nanos = UnixNanos::from_millis(max_ms);
        assert_eq!(nanos.as_u64(), max_ms * 1_000_000);
    }

    #[rstest]
    fn test_from_millis_matches_manual_conversion() {
        let ms = 1_625_474_304_765_u64;
        let expected = ms * 1_000_000;
        assert_eq!(UnixNanos::from_millis(ms).as_u64(), expected);
    }

    #[rstest]
    #[case::valid(1_700_000_000_123, Some(1_700_000_000_123_000_000))]
    #[case::negative(-1, None)]
    #[case::overflow(i64::MAX, None)]
    fn test_from_millis_checked(#[case] millis: i64, #[case] expected: Option<u64>) {
        assert_eq!(
            UnixNanos::from_millis_checked(millis).map(|value| value.as_u64()),
            expected
        );
    }

    #[rstest]
    #[case(0, 0)]
    #[case(999_999_999, 0)]
    #[case(1_000_000_000, 1)]
    #[case(1_700_000_000_123_456_789, 1_700_000_000)]
    fn test_as_seconds(#[case] nanos: u64, #[case] expected: u64) {
        assert_eq!(UnixNanos::from(nanos).as_seconds(), expected);
    }

    #[rstest]
    #[case(0, 0)]
    #[case(999_999, 0)]
    #[case(1_000_000, 1)]
    #[case(1_700_000_000_000_123_456, 1_700_000_000_000)]
    fn test_as_millis(#[case] nanos: u64, #[case] expected: u64) {
        assert_eq!(UnixNanos::from(nanos).as_millis(), expected);
    }

    #[rstest]
    #[case(0, 0)]
    #[case(999, 0)]
    #[case(1_000, 1)]
    #[case(1_700_000_000_000_123_456, 1_700_000_000_000_123)]
    fn test_as_micros(#[case] nanos: u64, #[case] expected: u64) {
        assert_eq!(UnixNanos::from(nanos).as_micros(), expected);
    }

    #[rstest]
    fn test_from_micros_zero() {
        let nanos = UnixNanos::from_micros(0);
        assert_eq!(nanos.as_u64(), 0);
    }

    #[rstest]
    fn test_from_micros_one() {
        let nanos = UnixNanos::from_micros(1);
        assert_eq!(nanos.as_u64(), 1_000);
    }

    #[rstest]
    fn test_from_micros_one_second() {
        let nanos = UnixNanos::from_micros(1_000_000);
        assert_eq!(nanos.as_u64(), 1_000_000_000);
    }

    #[rstest]
    fn test_from_micros_one_millisecond() {
        let nanos = UnixNanos::from_micros(1_000);
        assert_eq!(nanos.as_u64(), 1_000_000);
        assert_eq!(UnixNanos::from_micros(1_000), UnixNanos::from_millis(1));
    }

    #[rstest]
    fn test_from_micros_realistic_timestamp() {
        let micros = 1_700_000_000_000_000_u64;
        let nanos = UnixNanos::from_micros(micros);
        assert_eq!(nanos.as_u64(), 1_700_000_000_000_000_000);
    }

    #[rstest]
    fn test_from_micros_max_safe() {
        let max_us = u64::MAX / 1_000;
        let nanos = UnixNanos::from_micros(max_us);
        assert_eq!(nanos.as_u64(), max_us * 1_000);
    }

    #[rstest]
    fn test_from_micros_matches_manual_conversion() {
        let us = 1_000_000_123_456_u64;
        let expected = us * 1_000;
        assert_eq!(UnixNanos::from_micros(us).as_u64(), expected);
    }

    #[rstest]
    #[case::valid(1_700_000_000_123_456, Some(1_700_000_000_123_456_000))]
    #[case::negative(-1, None)]
    #[case::overflow(i64::MAX, None)]
    fn test_from_micros_checked(#[case] micros: i64, #[case] expected: Option<u64>) {
        assert_eq!(
            UnixNanos::from_micros_checked(micros).map(|value| value.as_u64()),
            expected
        );
    }

    #[rstest]
    fn test_from_seconds_millis_and_micros_consistency() {
        assert_eq!(UnixNanos::from_seconds(1), UnixNanos::from_millis(1_000));
        assert_eq!(
            UnixNanos::from_seconds(60),
            UnixNanos::from_micros(60_000_000)
        );
        assert_eq!(
            UnixNanos::from_millis(1_000),
            UnixNanos::from_micros(1_000_000)
        );
        assert_eq!(
            UnixNanos::from_millis(60_000),
            UnixNanos::from_micros(60_000_000)
        );
    }

    #[rstest]
    fn test_from_millis_round_trip_to_datetime() {
        let ms = 1_707_577_123_456_u64;
        let nanos = UnixNanos::from_millis(ms);
        let dt = nanos.to_datetime_utc();
        assert_eq!(dt.as_millisecond().cast_unsigned(), ms);
    }

    #[rstest]
    fn test_from_micros_preserves_sub_millisecond() {
        let micros = 1_700_000_000_000_123_u64;
        let nanos = UnixNanos::from_micros(micros);
        assert_eq!(nanos.as_u64() % 1_000_000, 123_000);
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos overflow in from_seconds")]
    fn test_from_seconds_overflow_panics() {
        let _ = UnixNanos::from_seconds(u64::MAX / 1_000_000_000 + 1);
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos overflow in from_millis")]
    fn test_from_millis_overflow_panics() {
        let _ = UnixNanos::from_millis(u64::MAX / 1_000_000 + 1);
    }

    #[rstest]
    #[should_panic(expected = "UnixNanos overflow in from_micros")]
    fn test_from_micros_overflow_panics() {
        let _ = UnixNanos::from_micros(u64::MAX / 1_000 + 1);
    }
}

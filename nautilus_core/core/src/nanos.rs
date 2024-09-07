// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Add, AddAssign, Deref, Sub, SubAssign},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

/// Represents a timestamp in nanoseconds since the UNIX epoch.
#[repr(C)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct UnixNanos(u64);

impl UnixNanos {
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
        Self(
            value
                .parse()
                .expect("Value is not a valid u64 string representation"),
        )
    }
}

impl FromStr for UnixNanos {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(UnixNanos)
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

/// Represents a duration in nanoseconds.
pub type DurationNanos = u64;

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new() {
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
    fn test_serde_json() {
        let nanos = UnixNanos::from(123);
        let json = serde_json::to_string(&nanos).unwrap();
        let deserialized: UnixNanos = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, nanos);
    }
}

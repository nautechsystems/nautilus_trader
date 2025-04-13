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

//! A time-keeping abstraction (nanoseconds) that works for storing in an atomic integer.

use std::{
    fmt::Debug,
    ops::{Add, Div, Mul},
    prelude::v1::*,
    time::Duration,
};

use super::clock;

/// A number of nanoseconds from a reference point.
///
/// Nanos can not represent durations >584 years, but hopefully that
/// should not be a problem in real-world applications.
#[derive(PartialEq, Eq, Default, Clone, Copy, PartialOrd, Ord)]
pub struct Nanos(u64);

impl Nanos {
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Nanos as used by Jitter and other std-only features.
#[cfg(feature = "std")]
impl Nanos {
    pub const fn new(u: u64) -> Self {
        Self(u)
    }
}

impl From<Duration> for Nanos {
    fn from(d: Duration) -> Self {
        // This will panic:
        Self(
            d.as_nanos()
                .try_into()
                .expect("Duration is longer than 584 years"),
        )
    }
}

impl Debug for Nanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        let d = Duration::from_nanos(self.0);
        write!(f, "Nanos({d:?})")
    }
}

impl Add<Self> for Nanos {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Mul<u64> for Nanos {
    type Output = Self;

    fn mul(self, rhs: u64) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl Div<Self> for Nanos {
    type Output = u64;

    fn div(self, rhs: Self) -> Self::Output {
        self.0 / rhs.0
    }
}

impl From<u64> for Nanos {
    fn from(u: u64) -> Self {
        Self(u)
    }
}

impl From<Nanos> for u64 {
    fn from(n: Nanos) -> Self {
        n.0
    }
}

impl From<Nanos> for Duration {
    fn from(n: Nanos) -> Self {
        Self::from_nanos(n.0)
    }
}

impl Nanos {
    #[inline]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl clock::Reference for Nanos {
    #[inline]
    fn duration_since(&self, earlier: Self) -> Nanos {
        (*self as Self).saturating_sub(earlier)
    }

    #[inline]
    fn saturating_sub(&self, duration: Nanos) -> Self {
        (*self as Self).saturating_sub(duration)
    }
}

impl Add<Duration> for Nanos {
    type Output = Self;

    fn add(self, other: Duration) -> Self {
        let other: Self = other.into();
        self + other
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(all(feature = "std", test))]
mod test {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn nanos_impls() {
        let n = Nanos::new(20);
        assert_eq!("Nanos(20ns)", format!("{n:?}"));
    }

    #[rstest]
    fn nanos_arith_coverage() {
        let n = Nanos::new(20);
        let n_half = Nanos::new(10);
        assert_eq!(n / n_half, 2);
        assert_eq!(30, (n + Duration::from_nanos(10)).as_u64());

        assert_eq!(n_half.saturating_sub(n), Nanos::new(0));
        assert_eq!(n.saturating_sub(n_half), n_half);
        assert_eq!(clock::Reference::saturating_sub(&n_half, n), Nanos::new(0));
    }
}

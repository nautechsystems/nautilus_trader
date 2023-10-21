use std::{num::NonZeroU32, prelude::v1::*, time::Duration};

use nonzero_ext::nonzero;
use pyo3::{exceptions::PyException, prelude::*};

use super::nanos::Nanos;

/// A rate-limiting quota.
///
/// Quotas are expressed in a positive number of "cells" (the maximum number of positive decisions /
/// allowed items until the rate limiter needs to replenish) and the amount of time for the rate
/// limiter to replenish a single cell.
///
/// Neither the number of cells nor the replenishment unit of time may be zero.
///
/// # Burst sizes
/// There are multiple ways of expressing the same quota: a quota given as `Quota::per_second(1)`
/// allows, on average, the same number of cells through as a quota given as `Quota::per_minute(60)`.
/// However, the quota of `Quota::per_minute(60)` has a burst size of 60 cells, meaning it is
/// possible to accomodate 60 cells in one go, after which the equivalent of a minute of inactivity
/// is required for the burst allowance to be fully restored.
///
/// Burst size gets really important when you construct a rate limiter that should allow multiple
/// elements through at one time (using [`RateLimiter.check_n`](struct.RateLimiter.html#method.check_n)
/// and its related functions): Only
/// at most as many cells can be let through in one call as are given as the burst size.
///
/// In other words, the burst size is the maximum number of cells that the rate limiter will ever
/// allow through without replenishing them.
#[pyclass]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Quota {
    pub(crate) max_burst: NonZeroU32,
    pub(crate) replenish_1_per: Duration,
}

#[pymethods]
impl Quota {
    #[staticmethod]
    pub fn rate_per_second(max_burst: u32) -> PyResult<Self> {
        match NonZeroU32::new(max_burst) {
            Some(max_burst) => Ok(Self::per_second(max_burst)),
            None => Err(PyErr::new::<PyException, _>(
                "Max burst capacity should be a non-zero integer",
            )),
        }
    }

    #[staticmethod]
    pub fn rate_per_minute(max_burst: u32) -> PyResult<Self> {
        match NonZeroU32::new(max_burst) {
            Some(max_burst) => Ok(Self::per_minute(max_burst)),
            None => Err(PyErr::new::<PyException, _>(
                "Max burst capacity should be a non-zero integer",
            )),
        }
    }

    #[staticmethod]
    pub fn rate_per_hour(max_burst: u32) -> PyResult<Self> {
        match NonZeroU32::new(max_burst) {
            Some(max_burst) => Ok(Self::per_hour(max_burst)),
            None => Err(PyErr::new::<PyException, _>(
                "Max burst capacity should be a non-zero integer",
            )),
        }
    }
}

/// Constructors for Quotas
impl Quota {
    /// Construct a quota for a number of cells per second. The given number of cells is also
    /// assumed to be the maximum burst size.
    pub const fn per_second(max_burst: NonZeroU32) -> Self {
        let replenish_interval_ns = Duration::from_secs(1).as_nanos() / (max_burst.get() as u128);
        Self {
            max_burst,
            replenish_1_per: Duration::from_nanos(replenish_interval_ns as u64),
        }
    }

    /// Construct a quota for a number of cells per 60-second period. The given number of cells is
    /// also assumed to be the maximum burst size.
    pub const fn per_minute(max_burst: NonZeroU32) -> Self {
        let replenish_interval_ns = Duration::from_secs(60).as_nanos() / (max_burst.get() as u128);
        Self {
            max_burst,
            replenish_1_per: Duration::from_nanos(replenish_interval_ns as u64),
        }
    }

    /// Construct a quota for a number of cells per 60-minute (3600-second) period. The given number
    /// of cells is also assumed to be the maximum burst size.
    pub const fn per_hour(max_burst: NonZeroU32) -> Self {
        let replenish_interval_ns =
            Duration::from_secs(60 * 60).as_nanos() / (max_burst.get() as u128);
        Self {
            max_burst,
            replenish_1_per: Duration::from_nanos(replenish_interval_ns as u64),
        }
    }

    /// Construct a quota that replenishes one cell in a given
    /// interval.
    ///
    /// This constructor is meant to replace [`::new`](#method.new),
    /// in cases where a longer refresh period than 1 cell/hour is
    /// necessary.
    ///
    /// If the time interval is zero, returns `None`.
    pub fn with_period(replenish_1_per: Duration) -> Option<Self> {
        if replenish_1_per.as_nanos() == 0 {
            None
        } else {
            Some(Self {
                max_burst: nonzero!(1u32),
                replenish_1_per,
            })
        }
    }

    /// Adjusts the maximum burst size for a quota to construct a rate limiter with a capacity
    /// for at most the given number of cells.
    pub const fn allow_burst(self, max_burst: NonZeroU32) -> Self {
        Self { max_burst, ..self }
    }

    /// Construct a quota for a given burst size, replenishing the entire burst size in that
    /// given unit of time.
    ///
    /// Returns `None` if the duration is zero.
    ///
    /// This constructor allows greater control over the resulting
    /// quota, but doesn't make as much intuitive sense as other
    /// methods of constructing the same quotas. Unless your quotas
    /// are given as "max burst size, and time it takes to replenish
    /// that burst size", you are better served by the
    /// [`Quota::per_second`](#method.per_second) (and similar)
    /// constructors with the [`allow_burst`](#method.allow_burst)
    /// modifier.
    #[deprecated(
        since = "0.2.0",
        note = "This constructor is often confusing and non-intuitive. \
    Use the `per_(interval)` / `with_period` and `max_burst` constructors instead."
    )]
    pub fn new(max_burst: NonZeroU32, replenish_all_per: Duration) -> Option<Self> {
        if replenish_all_per.as_nanos() == 0 {
            None
        } else {
            Some(Self {
                max_burst,
                replenish_1_per: replenish_all_per / max_burst.get(),
            })
        }
    }
}

/// Retrieving information about a quota
impl Quota {
    /// The time it takes for a rate limiter with an exhausted burst budget to replenish
    /// a single element.
    pub const fn replenish_interval(&self) -> Duration {
        self.replenish_1_per
    }

    /// The maximum number of cells that can be allowed in one burst.
    pub const fn burst_size(&self) -> NonZeroU32 {
        self.max_burst
    }

    /// The time it takes to replenish the entire maximum burst size.
    pub const fn burst_size_replenished_in(&self) -> Duration {
        let fill_in_ns = self.replenish_1_per.as_nanos() * self.max_burst.get() as u128;
        Duration::from_nanos(fill_in_ns as u64)
    }
}

impl Quota {
    /// A way to reconstruct a Quota from an in-use Gcra.
    ///
    /// This is useful mainly for [`crate::middleware::RateLimitingMiddleware`]
    /// where custom code may want to construct information based on
    /// the amount of burst balance remaining.
    pub(crate) fn from_gcra_parameters(t: Nanos, tau: Nanos) -> Self {
        // Safety assurance: As we're calling this from this crate
        // only, and we do not allow creating a Gcra from 0
        // parameters, this is, in fact, safe.
        //
        // The casts may look a little sketch, but again, they're
        // constructed from parameters that came from the crate
        // exactly like that.
        let max_burst = unsafe { NonZeroU32::new_unchecked((tau.as_u64() / t.as_u64()) as u32) };
        let replenish_1_per = t.into();
        Self {
            max_burst,
            replenish_1_per,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
// #[cfg(test)]
// mod test {
//     use nonzero_ext::nonzero;

//     use super::*;

//     #[test]
//     fn time_multiples() {
//         let hourly = Quota::per_hour(nonzero!(1u32));
//         let minutely = Quota::per_minute(nonzero!(1u32));
//         let secondly = Quota::per_second(nonzero!(1u32));

//         assert_eq!(
//             hourly.replenish_interval() / 60,
//             minutely.replenish_interval()
//         );
//         assert_eq!(
//             minutely.replenish_interval() / 60,
//             secondly.replenish_interval()
//         );
//     }

//     #[test]
//     fn period_error_cases() {
//         assert!(Quota::with_period(Duration::from_secs(0)).is_none());

//         #[allow(deprecated)]
//         {
//             assert!(Quota::new(nonzero!(1u32), Duration::from_secs(0)).is_none());
//         }
//     }
// }

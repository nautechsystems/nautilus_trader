//! Time sources for rate limiters.
//!
//! The time sources contained in this module allow the rate limiter
//! to be (optionally) independent of std, and additionally
//! allow mocking the passage of time.
//!
//! You can supply a custom time source by implementing both [`Reference`]
//! and [`Clock`] for your own types, and by implementing `Add<Nanos>` for
//! your [`Reference`] type:
use std::{
    convert::TryInto,
    fmt::Debug,
    ops::Add,
    prelude::v1::*,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime},
};

use super::nanos::Nanos;

/// A measurement from a clock.
pub trait Reference:
    Sized + Add<Nanos, Output = Self> + PartialEq + Eq + Ord + Copy + Clone + Send + Sync + Debug
{
    /// Determines the time that separates two measurements of a
    /// clock. Implementations of this must perform a saturating
    /// subtraction - if the `earlier` timestamp should be later,
    /// `duration_since` must return the zero duration.
    fn duration_since(&self, earlier: Self) -> Nanos;

    /// Returns a reference point that lies at most `duration` in the
    /// past from the current reference. If an underflow should occur,
    /// returns the current reference.
    fn saturating_sub(&self, duration: Nanos) -> Self;
}

/// A time source used by rate limiters.
pub trait Clock: Clone {
    /// A measurement of a monotonically increasing clock.
    type Instant: Reference;

    /// Returns a measurement of the clock.
    fn now(&self) -> Self::Instant;
}

impl Reference for Duration {
    /// The internal duration between this point and another.
    fn duration_since(&self, earlier: Self) -> Nanos {
        self.checked_sub(earlier)
            .unwrap_or_else(|| Duration::new(0, 0))
            .into()
    }

    /// The internal duration between this point and another.
    fn saturating_sub(&self, duration: Nanos) -> Self {
        self.checked_sub(duration.into()).unwrap_or(*self)
    }
}

impl Add<Nanos> for Duration {
    type Output = Self;

    fn add(self, other: Nanos) -> Self {
        let other: Duration = other.into();
        self + other
    }
}

/// A mock implementation of a clock. All it does is keep track of
/// what "now" is (relative to some point meaningful to the program),
/// and returns that.
///
/// # Thread safety
/// The mock time is represented as an atomic u64 count of nanoseconds, behind an [`Arc`].
/// Clones of this clock will all show the same time, even if the original advances.
#[derive(Debug, Clone, Default)]
pub struct FakeRelativeClock {
    now: Arc<AtomicU64>,
}

impl FakeRelativeClock {
    /// Advances the fake clock by the given amount.
    pub fn advance(&self, by: Duration) {
        let by: u64 = by
            .as_nanos()
            .try_into()
            .expect("Can not represent times past ~584 years");

        let mut prev = self.now.load(Ordering::Acquire);
        let mut next = prev + by;
        while let Err(next_prev) =
            self.now
                .compare_exchange_weak(prev, next, Ordering::Release, Ordering::Relaxed)
        {
            prev = next_prev;
            next = prev + by;
        }
    }
}

impl PartialEq for FakeRelativeClock {
    fn eq(&self, other: &Self) -> bool {
        self.now.load(Ordering::Relaxed) == other.now.load(Ordering::Relaxed)
    }
}

impl Clock for FakeRelativeClock {
    type Instant = Nanos;

    fn now(&self) -> Self::Instant {
        self.now.load(Ordering::Relaxed).into()
    }
}

#[cfg(all(feature = "std", test))]
mod test {
    use std::{iter::repeat, sync::Arc, thread, time::Duration};

    use super::*;
    use crate::nanos::Nanos;

    #[test]
    fn fake_clock_parallel_advances() {
        let clock = Arc::new(FakeRelativeClock::default());
        let threads = repeat(())
            .take(10)
            .map(move |_| {
                let clock = Arc::clone(&clock);
                thread::spawn(move || {
                    for _ in 0..1000000 {
                        let now = clock.now();
                        clock.advance(Duration::from_nanos(1));
                        assert!(clock.now() > now);
                    }
                })
            })
            .collect::<Vec<_>>();
        for t in threads {
            t.join().unwrap();
        }
    }

    #[test]
    fn duration_addition_coverage() {
        let d = Duration::from_secs(1);
        let one_ns = Nanos::new(1);
        assert!(d + one_ns > d);
    }
}

/// The monotonic clock implemented by [`Instant`].
#[derive(Clone, Debug, Default)]
pub struct MonotonicClock;

impl Add<Nanos> for Instant {
    type Output = Instant;

    fn add(self, other: Nanos) -> Instant {
        let other: Duration = other.into();
        self + other
    }
}

impl Reference for Instant {
    fn duration_since(&self, earlier: Self) -> Nanos {
        if earlier < *self {
            (*self - earlier).into()
        } else {
            Nanos::from(Duration::new(0, 0))
        }
    }

    fn saturating_sub(&self, duration: Nanos) -> Self {
        self.checked_sub(duration.into()).unwrap_or(*self)
    }
}

impl Clock for MonotonicClock {
    type Instant = Instant;

    fn now(&self) -> Self::Instant {
        Instant::now()
    }
}

/// The non-monotonic clock implemented by [`SystemTime`].
#[derive(Clone, Debug, Default)]
pub struct SystemClock;

impl Reference for SystemTime {
    /// Returns the difference in times between the two
    /// SystemTimes. Due to the fallible nature of SystemTimes,
    /// returns the zero duration if a negative duration would
    /// result (e.g. due to system clock adjustments).
    fn duration_since(&self, earlier: Self) -> Nanos {
        self.duration_since(earlier)
            .unwrap_or_else(|_| Duration::new(0, 0))
            .into()
    }

    fn saturating_sub(&self, duration: Nanos) -> Self {
        self.checked_sub(duration.into()).unwrap_or(*self)
    }
}

impl Add<Nanos> for SystemTime {
    type Output = SystemTime;

    fn add(self, other: Nanos) -> SystemTime {
        let other: Duration = other.into();
        self + other
    }
}

impl Clock for SystemClock {
    type Instant = SystemTime;

    fn now(&self) -> Self::Instant {
        SystemTime::now()
    }
}

/// Identifies clocks that run similarly to the monotonic realtime clock.
///
/// Clocks implementing this trait can be used with rate-limiters functions that operate
/// asynchronously.
pub trait ReasonablyRealtime: Clock {
    /// Returns a reference point at the start of an operation.
    fn reference_point(&self) -> Self::Instant {
        self.now()
    }
}

impl ReasonablyRealtime for MonotonicClock {}

impl ReasonablyRealtime for SystemClock {}

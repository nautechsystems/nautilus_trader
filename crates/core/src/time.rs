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

//! The core `AtomicTime` for real-time and static clocks.
//!
//! This module provides an atomic time abstraction that supports both real-time and static
//! clocks. It ensures thread-safe operations and monotonic time retrieval with nanosecond precision.
//!
//! # Modes
//!
//! - **Real-time mode:** The clock continuously syncs with system wall-clock time (via
//!   [`SystemTime::now()`]). To ensure strict monotonic increments across multiple threads,
//!   the internal updates use an atomic compare-and-exchange loop (`time_since_epoch`).
//!   While this guarantees that every new timestamp is at least one nanosecond greater than the
//!   last, it may introduce higher contention if many threads call it heavily.
//!
//! - **Static mode:** The clock is manually controlled via [`AtomicTime::set_time`] or [`AtomicTime::increment_time`],
//!   which can be useful for simulations or backtesting. You can switch modes at runtime using
//!   [`AtomicTime::make_realtime`] or [`AtomicTime::make_static`]. In **static mode**, we use
//!   acquire/release semantics so that updates from one thread can be observed by another;
//!   however, we do not enforce strict global ordering for manual updates. If you need strong,
//!   multi-threaded ordering in **static mode**, you must coordinate higher-level synchronization yourself.

use std::{
    ops::Deref,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_MICROSECOND, NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};

/// Global atomic time in **real-time mode** for use across the system.
///
/// This clock operates in **real-time mode**, synchronizing with the system clock.
/// It provides globally unique, strictly increasing timestamps across threads.
pub static ATOMIC_CLOCK_REALTIME: OnceLock<AtomicTime> = OnceLock::new();

/// Global atomic time in **static mode** for use across the system.
///
/// This clock operates in **static mode**, where the time value can be set or incremented
/// manually. Useful for backtesting or simulated time control.
pub static ATOMIC_CLOCK_STATIC: OnceLock<AtomicTime> = OnceLock::new();

/// Returns a static reference to the global atomic clock in **real-time mode**.
///
/// This clock uses [`AtomicTime::time_since_epoch`] under the hood, ensuring strictly increasing
/// timestamps across threads.
pub fn get_atomic_clock_realtime() -> &'static AtomicTime {
    ATOMIC_CLOCK_REALTIME.get_or_init(AtomicTime::default)
}

/// Returns a static reference to the global atomic clock in **static mode**.
///
/// This clock allows manual time control via [`AtomicTime::set_time`] or [`AtomicTime::increment_time`],
/// and does not automatically sync with system time.
pub fn get_atomic_clock_static() -> &'static AtomicTime {
    ATOMIC_CLOCK_STATIC.get_or_init(|| AtomicTime::new(false, UnixNanos::default()))
}

/// Returns the duration since the UNIX epoch based on [`SystemTime::now()`].
///
/// Panics if the system time is set before the UNIX epoch.
#[inline(always)]
#[must_use]
pub fn duration_since_unix_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Error calling `SystemTime`")
}

/// Returns the current UNIX time in nanoseconds, based on [`SystemTime::now()`].
#[inline(always)]
#[must_use]
pub fn nanos_since_unix_epoch() -> u64 {
    duration_since_unix_epoch().as_nanos() as u64
}

/// Represents an atomic timekeeping structure.
///
/// [`AtomicTime`] can act as a real-time clock or static clock based on its mode.
/// It uses an [`AtomicU64`] to atomically update the value using only immutable
/// references.
///
/// The `realtime` flag indicates which mode the clock is currently in.
/// For concurrency, this struct uses atomic operations with appropriate memory orderings:
/// - **Acquire/Release** for reading/writing in **static mode**,
/// - **Compare-and-exchange (`AcqRel`)** in real-time mode to guarantee monotonic increments.
#[repr(C)]
#[derive(Debug)]
pub struct AtomicTime {
    /// Indicates whether the clock is operating in **real-time mode** (`true`) or **static mode** (`false`)
    pub realtime: AtomicBool,
    /// The last recorded time (in UNIX nanoseconds). Updated atomically with compare-and-exchange
    /// in **real-time mode**, or simple store/fetch in **static mode**.
    pub timestamp_ns: AtomicU64,
}

impl Deref for AtomicTime {
    type Target = AtomicU64;

    fn deref(&self) -> &Self::Target {
        &self.timestamp_ns
    }
}

impl Default for AtomicTime {
    /// Creates a new default [`AtomicTime`] instance in **real-time mode**, starting at the current system time.
    fn default() -> Self {
        Self::new(true, UnixNanos::default())
    }
}

impl AtomicTime {
    /// Creates a new [`AtomicTime`] instance.
    ///
    /// - If `realtime` is `true`, the provided `time` is used only as an initial placeholder
    ///   and will quickly be overridden by calls to [`AtomicTime::time_since_epoch`].
    /// - If `realtime` is `false`, this clock starts in **static mode**, with the given `time`
    ///   as its current value.
    #[must_use]
    pub fn new(realtime: bool, time: UnixNanos) -> Self {
        Self {
            realtime: AtomicBool::new(realtime),
            timestamp_ns: AtomicU64::new(time.into()),
        }
    }

    /// Returns the current time in nanoseconds, based on the clock’s mode.
    ///
    /// - In **real-time mode**, calls [`AtomicTime::time_since_epoch`], ensuring strictly increasing
    ///   timestamps across threads, using `AcqRel` semantics for the underlying atomic.
    /// - In **static mode**, reads the stored time using [`Ordering::Acquire`]. Updates by other
    ///   threads using [`AtomicTime::set_time`] or [`AtomicTime::increment_time`] (Release/AcqRel)
    ///   will be visible here.
    #[must_use]
    pub fn get_time_ns(&self) -> UnixNanos {
        if self.realtime.load(Ordering::Acquire) {
            self.time_since_epoch()
        } else {
            UnixNanos::from(self.timestamp_ns.load(Ordering::Acquire))
        }
    }

    /// Return the current time as microseconds.
    #[must_use]
    pub fn get_time_us(&self) -> u64 {
        self.get_time_ns().as_u64() / NANOSECONDS_IN_MICROSECOND
    }

    /// Return the current time as milliseconds.
    #[must_use]
    pub fn get_time_ms(&self) -> u64 {
        self.get_time_ns().as_u64() / NANOSECONDS_IN_MILLISECOND
    }

    /// Return the current time as seconds.
    #[must_use]
    pub fn get_time(&self) -> f64 {
        self.get_time_ns().as_f64() / (NANOSECONDS_IN_SECOND as f64)
    }

    /// Manually sets a new time for the clock (only meaningful in **static mode**).
    ///
    /// This uses an atomic store with [`Ordering::Release`], so any thread reading with
    /// [`Ordering::Acquire`] will see the updated time. This does *not* enforce a total ordering
    /// among all threads, but is enough to ensure that once a thread sees this update, it also
    /// sees all writes made before this call in the writing thread.
    ///
    /// Typically used in single-threaded scenarios or coordinated concurrency in **static mode**,
    /// since there’s no global ordering across threads.
    pub fn set_time(&self, time: UnixNanos) {
        self.store(time.into(), Ordering::Release);
    }

    /// Increments the current time by `delta` nanoseconds and returns the *updated* value
    /// (only meaningful in **static mode**).
    ///
    /// Uses `fetch_add` with [`Ordering::AcqRel`], ensuring that:
    /// - The increment is atomic (no lost updates if multiple threads do increments).
    /// - Other threads reading with [`Ordering::Acquire`] will see the incremented result.
    ///
    /// Typically used in single-threaded scenarios or coordinated concurrency in **static mode**,
    /// since there’s no global ordering across threads.
    pub fn increment_time(&self, delta: u64) -> UnixNanos {
        UnixNanos::from(self.fetch_add(delta, Ordering::AcqRel) + delta)
    }

    /// Retrieves and updates the current “real-time” clock, returning a strictly increasing
    /// timestamp based on system time.
    ///
    /// Internally:
    /// - We fetch `now` from [`SystemTime::now()`].
    /// - We do an atomic compare-and-exchange (using [`Ordering::AcqRel`]) to ensure the stored
    ///   timestamp is never less than the last timestamp.
    ///
    /// This ensures:
    /// 1. **Monotonic increments**: The returned timestamp is strictly greater than the previous
    ///    one (by at least 1 nanosecond).
    /// 2. **No backward jumps**: If the OS time moves backward, we ignore that shift to preserve
    ///    monotonicity.
    /// 3. **Visibility**: In a multi-threaded environment, other threads see the updated value
    ///    once this compare-and-exchange completes.
    ///
    /// Note that under heavy contention (many threads calling this in tight loops), the CAS loop
    /// may increase latency. If you need extremely high-frequency, concurrent updates, consider
    /// using a more specialized approach or relaxing some ordering requirements.
    pub fn time_since_epoch(&self) -> UnixNanos {
        // This method guarantees strict consistency but may incur a performance cost under
        // high contention due to retries in the `compare_exchange` loop.
        let now = nanos_since_unix_epoch();
        loop {
            // Acquire to observe the latest stored value
            let last = self.load(Ordering::Acquire);
            let next = now.max(last + 1);
            // AcqRel on success ensures this new value is published,
            // Acquire on failure reloads if we lost a CAS race.
            match self.compare_exchange(last, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return UnixNanos::from(next),
                Err(_) => continue,
            }
        }
    }

    /// Switches the clock to **real-time mode** (`realtime = true`).
    ///
    /// Uses [`Ordering::SeqCst`] for the mode store, which ensures a global ordering for the
    /// mode switch if other threads also do `SeqCst` loads/stores of `realtime`.
    /// Typically, switching modes is done infrequently, so the performance impact of `SeqCst`
    /// here is acceptable.
    pub fn make_realtime(&self) {
        self.realtime.store(true, Ordering::SeqCst);
    }

    /// Switches the clock to **static mode** (`realtime = false`).
    ///
    /// Uses [`Ordering::SeqCst`] for the mode store, which ensures a global ordering for the
    /// mode switch if other threads also do `SeqCst` loads/stores of `realtime`.
    pub fn make_static(&self) {
        self.realtime.store(false, Ordering::SeqCst);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::*;

    use super::*;

    #[rstest]
    fn test_global_clocks_initialization() {
        let realtime_clock = get_atomic_clock_realtime();
        assert!(realtime_clock.get_time_ns().as_u64() > 0);

        let static_clock = get_atomic_clock_static();
        static_clock.set_time(UnixNanos::from(500_000_000)); // 500 ms
        assert_eq!(static_clock.get_time_ns().as_u64(), 500_000_000);
    }

    #[rstest]
    fn test_mode_switching() {
        let time = AtomicTime::new(true, UnixNanos::default());

        // Verify real-time mode
        let realtime_ns = time.get_time_ns();
        assert!(realtime_ns.as_u64() > 0);

        // Switch to static mode
        time.make_static();
        time.set_time(UnixNanos::from(1_000_000_000)); // 1 second
        let static_ns = time.get_time_ns();
        assert_eq!(static_ns.as_u64(), 1_000_000_000);

        // Switch back to real-time mode
        time.make_realtime();
        let new_realtime_ns = time.get_time_ns();
        assert!(new_realtime_ns.as_u64() > static_ns.as_u64());
    }

    #[rstest]
    fn test_mode_switching_concurrent() {
        let clock = Arc::new(AtomicTime::new(true, UnixNanos::default()));
        let num_threads = 4;
        let iterations = 10000;
        let mut handles = Vec::with_capacity(num_threads);

        for _ in 0..num_threads {
            let clock_clone = Arc::clone(&clock);
            let handle = std::thread::spawn(move || {
                for i in 0..iterations {
                    if i % 2 == 0 {
                        clock_clone.make_static();
                    } else {
                        clock_clone.make_realtime();
                    }
                    // Retrieve the time; we’re not asserting a particular value here,
                    // but at least we’re exercising the mode switch logic under concurrency.
                    let _ = clock_clone.get_time_ns();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[rstest]
    fn test_static_time_is_stable() {
        // Create a clock in static mode with an initial value
        let clock = AtomicTime::new(false, UnixNanos::from(42));
        let time1 = clock.get_time_ns();

        // Sleep a bit to give the system time to change, if the clock were using real-time
        std::thread::sleep(std::time::Duration::from_millis(10));
        let time2 = clock.get_time_ns();

        // In static mode, the value should remain unchanged
        assert_eq!(time1, time2);
    }

    #[rstest]
    fn test_increment_time() {
        // Start in static mode
        let time = AtomicTime::new(false, UnixNanos::from(0));

        let updated_time = time.increment_time(500);
        assert_eq!(updated_time.as_u64(), 500);

        let updated_time = time.increment_time(1_000);
        assert_eq!(updated_time.as_u64(), 1_500);
    }

    #[rstest]
    fn test_nanos_since_unix_epoch_vs_system_time() {
        let unix_nanos = nanos_since_unix_epoch();
        let system_ns = duration_since_unix_epoch().as_nanos() as u64;
        assert!((unix_nanos as i64 - system_ns as i64).abs() < NANOSECONDS_IN_SECOND as i64);
    }

    #[rstest]
    fn test_time_since_epoch_monotonicity() {
        let clock = get_atomic_clock_realtime();
        let mut previous = clock.time_since_epoch();
        for _ in 0..1_000_000 {
            let current = clock.time_since_epoch();
            assert!(current > previous);
            previous = current;
        }
    }

    #[rstest]
    fn test_time_since_epoch_strictly_increasing_concurrent() {
        let time = Arc::new(AtomicTime::new(true, UnixNanos::default()));
        let num_threads = 4;
        let iterations = 100_000;
        let mut handles = Vec::with_capacity(num_threads);

        for thread_id in 0..num_threads {
            let time_clone = Arc::clone(&time);

            let handle = std::thread::spawn(move || {
                let mut previous = time_clone.time_since_epoch().as_u64();

                for i in 0..iterations {
                    let current = time_clone.time_since_epoch().as_u64();
                    assert!(
                        current > previous,
                        "Thread {thread_id}: iteration {i}: time did not increase: previous={previous}, current={current}",
                    );
                    previous = current;
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[rstest]
    fn test_duration_since_unix_epoch() {
        let time = AtomicTime::new(true, UnixNanos::default());
        let duration = Duration::from_nanos(time.get_time_ns().into());
        let now = SystemTime::now();

        // Check if the duration is close to the actual difference between now and UNIX_EPOCH
        let delta = now
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .checked_sub(duration);
        assert!(delta.unwrap_or_default() < Duration::from_millis(100));

        // Check if the duration is greater than a certain value (assuming the test is run after that point)
        assert!(duration > Duration::from_secs(1_650_000_000));
    }

    #[rstest]
    fn test_unix_timestamp_is_monotonic_increasing() {
        let time = AtomicTime::new(true, UnixNanos::default());
        let result1 = time.get_time();
        let result2 = time.get_time();
        let result3 = time.get_time();
        let result4 = time.get_time();
        let result5 = time.get_time();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1_650_000_000.0);
    }

    #[rstest]
    fn test_unix_timestamp_ms_is_monotonic_increasing() {
        let time = AtomicTime::new(true, UnixNanos::default());
        let result1 = time.get_time_ms();
        let result2 = time.get_time_ms();
        let result3 = time.get_time_ms();
        let result4 = time.get_time_ms();
        let result5 = time.get_time_ms();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 >= 1_650_000_000_000);
    }

    #[rstest]
    fn test_unix_timestamp_us_is_monotonic_increasing() {
        let time = AtomicTime::new(true, UnixNanos::default());
        let result1 = time.get_time_us();
        let result2 = time.get_time_us();
        let result3 = time.get_time_us();
        let result4 = time.get_time_us();
        let result5 = time.get_time_us();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1_650_000_000_000_000);
    }

    #[rstest]
    fn test_unix_timestamp_ns_is_monotonic_increasing() {
        let time = AtomicTime::new(true, UnixNanos::default());
        let result1 = time.get_time_ns();
        let result2 = time.get_time_ns();
        let result3 = time.get_time_ns();
        let result4 = time.get_time_ns();
        let result5 = time.get_time_ns();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1.as_u64() > 1_650_000_000_000_000_000);
    }
}

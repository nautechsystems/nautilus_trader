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

//! The core `AtomicTime` for real-time and static clocks.
//!
//! This module provides an atomic time abstraction that supports both real-time and static
//! clocks. It ensures thread-safe operations and monotonic time retrieval with nanosecond precision.

use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    datetime::{NANOSECONDS_IN_MICROSECOND, NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
    nanos::UnixNanos,
};

/// Global atomic time in real-time mode for use across the system.
///
/// This clock operates in real-time mode, where it synchronizes with the system clock
/// or hardware counters, ensuring unique and monotonic timestamps.
pub static ATOMIC_CLOCK_REALTIME: OnceLock<AtomicTime> = OnceLock::new();

/// Global atomic time in static mode for use across the system.
///
/// This clock operates in static mode, where time is manually set and can be controlled
/// programmatically, useful for backtesting or simulation.
pub static ATOMIC_CLOCK_STATIC: OnceLock<AtomicTime> = OnceLock::new();

/// Returns a static reference to the global atomic clock in real-time mode.
///
/// This clock synchronizes with the system time, providing unique, monotonic timestamps
/// across the system.
pub fn get_atomic_clock_realtime() -> &'static AtomicTime {
    ATOMIC_CLOCK_REALTIME.get_or_init(AtomicTime::default)
}

/// Returns a static reference to the global atomic clock in static mode.
///
/// This clock allows manual time control and does not synchronize with the system clock.
pub fn get_atomic_clock_static() -> &'static AtomicTime {
    ATOMIC_CLOCK_STATIC.get_or_init(|| AtomicTime::new(false, UnixNanos::default()))
}

/// Returns the duration since the UNIX epoch.
///
/// This implementation relies on `SystemTime::now` and provides timestamps with nanosecond
/// precision. It panics if the system time is set before the UNIX epoch.
#[inline(always)]
#[must_use]
pub fn duration_since_unix_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Error calling `SystemTime`")
}

/// Returns the current UNIX time in nanoseconds.
#[inline(always)]
#[must_use]
pub fn nanos_since_unix_epoch() -> u64 {
    duration_since_unix_epoch().as_nanos() as u64
}

/// Represents an atomic timekeeping structure.
///
/// `AtomicTime` can act as a real-time clock or static clock based on its mode.
/// It uses `AtomicU64` to atomically update the value using only immutable
/// references.
///
/// This struct provides thread-safe access to a stored nanosecond time value,
/// useful for when concurrent access to time information is required.
#[repr(C)]
#[derive(Debug)]
pub struct AtomicTime {
    /// Indicates whether the clock is operating in real-time mode.
    /// When `false`, the clock is in a manual or static mode, allowing for controlled time setting.
    pub realtime: AtomicBool,
    /// The last recorded time for the clock in UNIX nanoseconds.
    /// This value is atomically updated and represents the precise time measurement.
    pub timestamp_ns: AtomicU64,
}

impl Deref for AtomicTime {
    type Target = AtomicU64;

    fn deref(&self) -> &Self::Target {
        &self.timestamp_ns
    }
}

impl Default for AtomicTime {
    /// Creates a new default [`AtomicTime`] instance.
    fn default() -> Self {
        Self::new(true, UnixNanos::default())
    }
}

impl AtomicTime {
    /// Creates a new [`AtomicTime`] instance.
    ///
    /// The `realtime` flag will determine whether the atomic time is based off system time.
    /// The time will be set to the given UNIX `time` (nanoseconds).
    #[must_use]
    pub fn new(realtime: bool, time: UnixNanos) -> Self {
        Self {
            realtime: AtomicBool::new(realtime),
            timestamp_ns: AtomicU64::new(time.into()),
        }
    }

    /// Get time in nanoseconds.
    ///
    /// - **Real-time mode**: Returns the current wall clock time since the UNIX epoch,
    ///   ensuring monotonicity across threads using `Ordering::SeqCst`.
    /// - **Static mode**: Returns the currently stored time, which uses `Ordering::Relaxed`
    ///   and is suitable for single-threaded scenarios where strict synchronization is unnecessary.
    #[must_use]
    pub fn get_time_ns(&self) -> UnixNanos {
        match self.realtime.load(Ordering::Relaxed) {
            true => self.time_since_epoch(),
            false => UnixNanos::from(self.timestamp_ns.load(Ordering::Relaxed)),
        }
    }

    /// Get time as microseconds.
    #[must_use]
    pub fn get_time_us(&self) -> u64 {
        self.get_time_ns().as_u64() / NANOSECONDS_IN_MICROSECOND
    }

    /// Get time as milliseconds.
    #[must_use]
    pub fn get_time_ms(&self) -> u64 {
        self.get_time_ns().as_u64() / NANOSECONDS_IN_MILLISECOND
    }

    /// Get time as seconds.
    #[must_use]
    pub fn get_time(&self) -> f64 {
        self.get_time_ns().as_f64() / (NANOSECONDS_IN_SECOND as f64)
    }

    /// Sets new time for the clock.
    ///
    /// Intended for single-threaded use, as it relies on `Ordering::Relaxed` and
    /// does not enforce strict synchronization.
    pub fn set_time(&self, time: UnixNanos) {
        self.store(time.into(), Ordering::Relaxed);
    }

    /// Increments the current time by the specified delta and returns the updated value.
    ///
    /// Intended for single-threaded use, as it relies on `Ordering::Relaxed` and
    /// does not enforce strict synchronization.
    pub fn increment_time(&self, delta: u64) -> UnixNanos {
        UnixNanos::from(self.fetch_add(delta, Ordering::Relaxed) + delta)
    }

    /// Stores and returns current time.
    ///
    /// This method uses `Ordering::SeqCst` (Sequential Consistency) ordering to ensure that:
    /// 1. Timestamps are monotonically increasing and thread-safe.
    /// 2. The returned timestamp is never less than the current system time.
    /// 3. Each timestamp is at least 1 nanosecond greater than the last stored value.
    pub fn time_since_epoch(&self) -> UnixNanos {
        // This method guarantees strict consistency but may incur a performance cost under
        // high contention due to retries in the `compare_exchange` loop.
        let now = nanos_since_unix_epoch();
        loop {
            let last = self.load(Ordering::SeqCst);
            let next = now.max(last + 1);
            match self.compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return UnixNanos::from(next),
                Err(_) => continue,
            }
        }
    }

    /// Switches the clock to real-time mode.
    ///
    /// Intended for single-threaded use, as it uses `Ordering::Relaxed` for updating the mode.
    pub fn make_realtime(&self) {
        self.realtime.store(true, Ordering::Relaxed);
    }

    /// Switches the clock to static mode.
    ///
    /// Intended for single-threaded use, as it uses `Ordering::Relaxed` for updating the mode.
    pub fn make_static(&self) {
        self.realtime.store(false, Ordering::Relaxed);
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
    fn test_switching_modes() {
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
    fn test_increment_time() {
        // Start in static mode
        let time = AtomicTime::new(false, UnixNanos::from(0));

        let updated_time = time.increment_time(500);
        assert_eq!(updated_time.as_u64(), 500);

        let updated_time = time.increment_time(1_000);
        assert_eq!(updated_time.as_u64(), 1_500);
    }

    #[cfg(target_os = "linux")]
    #[rstest]
    fn test_nanos_since_unix_epoch_vs_system_time() {
        let unix_nanos = nanos_since_unix_epoch();
        let system_ns = duration_since_unix_epoch().as_nanos() as u64;

        // Allow for a reasonable delta due to overhead
        assert!(
            (unix_nanos as i64 - system_ns as i64).abs() < NANOSECONDS_IN_SECOND as i64,
            "CLOCK_MONOTONIC_COARSE and SystemTime differ significantly"
        );
    }

    #[cfg(target_os = "linux")]
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

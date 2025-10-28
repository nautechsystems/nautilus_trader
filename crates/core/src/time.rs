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
/// # Panics
///
/// Panics if the system time is set before the UNIX epoch.
#[inline(always)]
#[must_use]
pub fn duration_since_unix_epoch() -> Duration {
    // SAFETY: The expect() is acceptable here because:
    // - SystemTime failure indicates catastrophic system clock issues
    // - This would affect the entire application's ability to function
    // - Alternative error handling would complicate all time-dependent code paths
    // - Such failures are extremely rare in practice and indicate hardware/OS problems
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Error calling `SystemTime`")
}

/// Returns the current UNIX time in nanoseconds, based on [`SystemTime::now()`].
///
/// # Panics
///
/// Panics if the duration in nanoseconds exceeds `u64::MAX`.
#[inline(always)]
#[must_use]
pub fn nanos_since_unix_epoch() -> u64 {
    let ns = duration_since_unix_epoch().as_nanos();
    assert!(
        ns <= u128::from(u64::MAX),
        "System time overflow: value exceeds u64::MAX nanoseconds"
    );
    ns as u64
}

/// Represents an atomic timekeeping structure.
///
/// [`AtomicTime`] can act as a real-time clock or static clock based on its mode.
/// It uses an [`AtomicU64`] to atomically update the value using only immutable
/// references.
///
/// The `realtime` flag indicates which mode the clock is currently in.
/// For concurrency, this struct uses atomic operations with appropriate memory orderings:
/// - **Acquire/Release** for reading/writing in **static mode**.
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

    /// Returns the current time as microseconds.
    #[must_use]
    pub fn get_time_us(&self) -> u64 {
        self.get_time_ns().as_u64() / NANOSECONDS_IN_MICROSECOND
    }

    /// Returns the current time as milliseconds.
    #[must_use]
    pub fn get_time_ms(&self) -> u64 {
        self.get_time_ns().as_u64() / NANOSECONDS_IN_MILLISECOND
    }

    /// Returns the current time as seconds.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
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
    ///
    /// # Panics
    ///
    /// Panics if invoked when in real-time mode.
    pub fn set_time(&self, time: UnixNanos) {
        assert!(
            !self.realtime.load(Ordering::Acquire),
            "Cannot set time while clock is in realtime mode"
        );

        self.store(time.into(), Ordering::Release);
    }

    /// Increments the current (static-mode) time by `delta` nanoseconds and returns the updated value.
    ///
    /// Internally this uses [`AtomicU64::fetch_update`] with [`Ordering::AcqRel`] to ensure the increment is
    /// atomic and visible to readers using `Acquire` loads.
    ///
    /// # Errors
    ///
    /// Returns an error if the increment would overflow `u64::MAX`.
    ///
    /// # Panics
    ///
    /// Panics if called while the clock is in real-time mode.
    pub fn increment_time(&self, delta: u64) -> anyhow::Result<UnixNanos> {
        assert!(
            !self.realtime.load(Ordering::Acquire),
            "Cannot increment time while clock is in realtime mode"
        );

        let previous =
            match self
                .timestamp_ns
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                    current.checked_add(delta)
                }) {
                Ok(prev) => prev,
                Err(_) => anyhow::bail!("Cannot increment time beyond u64::MAX"),
            };

        Ok(UnixNanos::from(previous + delta))
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
    /// # Panics
    ///
    /// Panics if the internal counter has reached `u64::MAX`, which would indicate the process has
    /// been running for longer than the representable range (~584 years) *or* the clock was
    /// manually corrupted.
    pub fn time_since_epoch(&self) -> UnixNanos {
        // This method guarantees strict consistency but may incur a performance cost under
        // high contention due to retries in the `compare_exchange` loop.
        let now = nanos_since_unix_epoch();
        loop {
            // Acquire to observe the latest stored value
            let last = self.load(Ordering::Acquire);
            // Ensure we never wrap past u64::MAX – treat that as a fatal error
            let incremented = last
                .checked_add(1)
                .expect("AtomicTime overflow: reached u64::MAX");
            let next = now.max(incremented);
            // AcqRel on success ensures this new value is published,
            // Acquire on failure reloads if we lost a CAS race.
            //
            // Note that under heavy contention (many threads calling this in tight loops),
            // the CAS loop may increase latency.
            //
            // However, in practice, the loop terminates quickly because:
            // - System time naturally advances between iterations
            // - Each iteration increments time by at least 1ns, preventing ABA problems
            // - True contention requiring retry is rare in normal usage patterns
            //
            // The concurrent stress test (4 threads × 100k iterations) validates this approach.
            if self
                .compare_exchange(last, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return UnixNanos::from(next);
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
    #[should_panic(expected = "Cannot set time while clock is in realtime mode")]
    fn test_set_time_panics_in_realtime_mode() {
        let clock = AtomicTime::new(true, UnixNanos::default());
        clock.set_time(UnixNanos::from(123));
    }

    #[rstest]
    #[should_panic(expected = "Cannot increment time while clock is in realtime mode")]
    fn test_increment_time_panics_in_realtime_mode() {
        let clock = AtomicTime::new(true, UnixNanos::default());
        let _ = clock.increment_time(1);
    }

    #[rstest]
    #[should_panic(expected = "AtomicTime overflow")]
    fn test_time_since_epoch_overflow_panics() {
        use std::sync::atomic::{AtomicBool, AtomicU64};

        // Manually construct a clock with the counter already at u64::MAX
        let clock = AtomicTime {
            realtime: AtomicBool::new(true),
            timestamp_ns: AtomicU64::new(u64::MAX),
        };

        // This call will attempt to add 1 and must panic
        let _ = clock.time_since_epoch();
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

        let updated_time = time.increment_time(500).unwrap();
        assert_eq!(updated_time.as_u64(), 500);

        let updated_time = time.increment_time(1_000).unwrap();
        assert_eq!(updated_time.as_u64(), 1_500);
    }

    #[rstest]
    fn test_increment_time_overflow_errors() {
        let time = AtomicTime::new(false, UnixNanos::from(u64::MAX - 5));

        let err = time.increment_time(10).unwrap_err();
        assert_eq!(err.to_string(), "Cannot increment time beyond u64::MAX");
    }

    #[rstest]
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
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

    #[rstest]
    fn test_acquire_release_contract_static_mode() {
        // This test explicitly proves the Acquire/Release memory ordering contract:
        // - Writer thread uses set_time() which does Release store (see AtomicTime::set_time)
        // - Reader thread uses get_time_ns() which does Acquire load (see AtomicTime::get_time_ns)
        // - The Release-Acquire pair ensures all writes before Release are visible after Acquire

        let clock = Arc::new(AtomicTime::new(false, UnixNanos::from(0)));
        let aux_data = Arc::new(AtomicU64::new(0));
        let done = Arc::new(AtomicBool::new(false));

        // Writer thread: updates auxiliary data, then releases via set_time
        let writer_clock = Arc::clone(&clock);
        let writer_aux = Arc::clone(&aux_data);
        let writer_done = Arc::clone(&done);

        let writer = std::thread::spawn(move || {
            for i in 1..=1_000u64 {
                writer_aux.store(i, Ordering::Relaxed);

                // Release store via set_time creates a release fence - all prior writes (including aux_data)
                // must be visible to any thread that observes this time value via Acquire load
                writer_clock.set_time(UnixNanos::from(i * 1000));

                // Yield to encourage interleaving
                std::thread::yield_now();
            }
            writer_done.store(true, Ordering::Release);
        });

        // Reader thread: acquires via get_time_ns, then checks auxiliary data
        let reader_clock = Arc::clone(&clock);
        let reader_aux = Arc::clone(&aux_data);
        let reader_done = Arc::clone(&done);

        let reader = std::thread::spawn(move || {
            let mut last_time = 0u64;
            let mut max_aux_seen = 0u64;

            // Poll until writer is done, with no iteration limit
            while !reader_done.load(Ordering::Acquire) {
                let current_time = reader_clock.get_time_ns().as_u64();

                if current_time > last_time {
                    // The Acquire in get_time_ns synchronizes with the Release in set_time,
                    // making aux_data visible
                    let aux_value = reader_aux.load(Ordering::Relaxed);

                    // Invariant: aux_value must never go backwards (proves Release-Acquire sync works)
                    if aux_value > 0 {
                        assert!(
                            aux_value >= max_aux_seen,
                            "Acquire/Release contract violated: aux went backwards from {} to {}",
                            max_aux_seen,
                            aux_value
                        );
                        max_aux_seen = aux_value;
                    }

                    last_time = current_time;
                }

                std::thread::yield_now();
            }

            // Check final state after writer completes to ensure we observe updates
            let final_time = reader_clock.get_time_ns().as_u64();
            if final_time > last_time {
                let final_aux = reader_aux.load(Ordering::Relaxed);
                if final_aux > 0 {
                    assert!(
                        final_aux >= max_aux_seen,
                        "Acquire/Release contract violated: final aux {} < max {}",
                        final_aux,
                        max_aux_seen
                    );
                    max_aux_seen = final_aux;
                }
            }

            max_aux_seen
        });

        writer.join().unwrap();
        let max_observed = reader.join().unwrap();

        // Ensure the reader actually observed updates (not vacuously satisfied)
        assert!(max_observed > 0, "Reader must observe writer updates");
    }

    #[rstest]
    fn test_acquire_release_contract_increment_time() {
        // Similar test for increment_time, which uses fetch_update with AcqRel (see AtomicTime::increment_time)

        let clock = Arc::new(AtomicTime::new(false, UnixNanos::from(0)));
        let aux_data = Arc::new(AtomicU64::new(0));
        let done = Arc::new(AtomicBool::new(false));

        let writer_clock = Arc::clone(&clock);
        let writer_aux = Arc::clone(&aux_data);
        let writer_done = Arc::clone(&done);

        let writer = std::thread::spawn(move || {
            for i in 1..=1_000u64 {
                writer_aux.store(i, Ordering::Relaxed);
                let _ = writer_clock.increment_time(1000).unwrap();
                std::thread::yield_now();
            }
            writer_done.store(true, Ordering::Release);
        });

        let reader_clock = Arc::clone(&clock);
        let reader_aux = Arc::clone(&aux_data);
        let reader_done = Arc::clone(&done);

        let reader = std::thread::spawn(move || {
            let mut last_time = 0u64;
            let mut max_aux = 0u64;

            // Poll until writer is done, with no iteration limit
            while !reader_done.load(Ordering::Acquire) {
                let current_time = reader_clock.get_time_ns().as_u64();

                if current_time > last_time {
                    let aux_value = reader_aux.load(Ordering::Relaxed);

                    // Invariant: aux_value must never regress (proves AcqRel sync works)
                    if aux_value > 0 {
                        assert!(
                            aux_value >= max_aux,
                            "AcqRel contract violated: aux regressed from {} to {}",
                            max_aux,
                            aux_value
                        );
                        max_aux = aux_value;
                    }

                    last_time = current_time;
                }

                std::thread::yield_now();
            }

            // Check final state after writer completes to ensure we observe updates
            let final_time = reader_clock.get_time_ns().as_u64();
            if final_time > last_time {
                let final_aux = reader_aux.load(Ordering::Relaxed);
                if final_aux > 0 {
                    assert!(
                        final_aux >= max_aux,
                        "AcqRel contract violated: final aux {} < max {}",
                        final_aux,
                        max_aux
                    );
                    max_aux = final_aux;
                }
            }

            max_aux
        });

        writer.join().unwrap();
        let max_observed = reader.join().unwrap();

        // Ensure the reader actually observed updates (not vacuously satisfied)
        assert!(max_observed > 0, "Reader must observe writer updates");
    }
}

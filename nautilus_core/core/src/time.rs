// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::datetime::{
    NANOSECONDS_IN_MICROSECOND, NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND,
};

/// Represents a timestamp in nanoseconds since UNIX epoch.
pub type UnixNanos = u64;

/// Represents a timedelta in nanoseconds.
pub type TimedeltaNanos = i64;

#[must_use]
pub fn duration_since_unix_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Error calling `SystemTime::now.duration_since`")
}

#[derive(Debug, Clone, Copy)]
pub enum ClockMode {
    LIVE,
    STATIC,
}

/// Atomic clock stores the last recorded time in nanoseconds.
///
/// It uses `AtomicU64` to atomically update the value using only immutable
/// references.
///
/// `AtomicClock` can act as a live clock and static clock based on its mode.
#[derive(Debug, Clone)]
pub struct AtomicTime {
    /// Atomic clock is operating in live or static mode.
    mode: ClockMode,
    /// The last recorded time in nanoseconds for the clock.
    timestamp_ns: Arc<AtomicU64>,
}

impl Deref for AtomicTime {
    type Target = AtomicU64;

    fn deref(&self) -> &Self::Target {
        &self.timestamp_ns
    }
}

impl AtomicTime {
    /// New atomic clock set with the given time.
    #[must_use]
    pub fn new(mode: ClockMode, time: u64) -> Self {
        Self {
            mode,
            timestamp_ns: Arc::new(AtomicU64::new(time)),
        }
    }

    /// Get time in nanoseconds.
    ///
    /// * Live mode returns current wall clock time since UNIX epoch (unique and monotonic)
    /// * Static mode returns currently stored time.
    #[must_use]
    pub fn get_time_ns(&self) -> u64 {
        match self.mode {
            ClockMode::LIVE => self.time_since_epoch(),
            ClockMode::STATIC => self.timestamp_ns.load(Ordering::Relaxed),
        }
    }

    /// Get time as microseconds.
    #[must_use]
    pub fn get_time_us(&self) -> u64 {
        self.get_time_ns() / NANOSECONDS_IN_MICROSECOND
    }

    /// Get time as milliseconds.
    #[must_use]
    pub fn get_time_ms(&self) -> u64 {
        self.get_time_ns() / NANOSECONDS_IN_MILLISECOND
    }

    /// Get time as seconds.
    #[must_use]
    pub fn get_time(&self) -> f64 {
        self.get_time_ns() as f64 / (NANOSECONDS_IN_SECOND as f64)
    }

    /// Sets new time for the clock.
    pub fn set_time(&self, time: u64) {
        self.store(time, Ordering::Relaxed);
    }

    /// Increments current time with a delta and returns the updated time.
    #[must_use]
    pub fn increment_time(&self, delta: u64) -> u64 {
        self.fetch_add(delta, Ordering::Relaxed) + delta
    }

    /// Stores and returns current time.
    #[must_use]
    pub fn time_since_epoch(&self) -> u64 {
        // Increment by 1 nanosecond to keep increasing time
        let now = duration_since_unix_epoch().as_nanos() as u64 + 1;
        let last = self.load(Ordering::SeqCst) + 1;
        let new = now.max(last);
        self.store(new, Ordering::SeqCst);
        new
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use rstest::*;

    use super::*;

    #[rstest]
    fn test_duration_since_unix_epoch() {
        let time = AtomicTime::new(ClockMode::LIVE, 0);
        let duration = Duration::from_nanos(time.get_time_ns());
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
        let time = AtomicTime::new(ClockMode::LIVE, 0);
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
        let time = AtomicTime::new(ClockMode::LIVE, 0);
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
        let time = AtomicTime::new(ClockMode::LIVE, 0);
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
        let time = AtomicTime::new(ClockMode::LIVE, 0);
        let result1 = time.get_time_ns();
        let result2 = time.get_time_ns();
        let result3 = time.get_time_ns();
        let result4 = time.get_time_ns();
        let result5 = time.get_time_ns();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1_650_000_000_000_000_000);
    }
}

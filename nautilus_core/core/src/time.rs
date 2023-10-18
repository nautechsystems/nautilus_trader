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

use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Returns the current seconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp() -> f64 {
    duration_since_unix_epoch().as_secs_f64()
}

/// Returns the current milliseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_ms() -> u64 {
    duration_since_unix_epoch().as_millis() as u64
}

/// Returns the current microseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_us() -> u64 {
    duration_since_unix_epoch().as_micros() as u64
}

/// Returns the current nanoseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_ns() -> u64 {
    duration_since_unix_epoch().as_nanos() as u64
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
        let duration = duration_since_unix_epoch();
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
        let result1 = unix_timestamp();
        let result2 = unix_timestamp();
        let result3 = unix_timestamp();
        let result4 = unix_timestamp();
        let result5 = unix_timestamp();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1_650_000_000.0);
    }

    #[rstest]
    fn test_unix_timestamp_ms_is_monotonic_increasing() {
        let result1 = unix_timestamp_ms();
        let result2 = unix_timestamp_ms();
        let result3 = unix_timestamp_ms();
        let result4 = unix_timestamp_ms();
        let result5 = unix_timestamp_ms();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 >= 1_650_000_000_000);
    }

    #[rstest]
    fn test_unix_timestamp_us_is_monotonic_increasing() {
        let result1 = unix_timestamp_us();
        let result2 = unix_timestamp_us();
        let result3 = unix_timestamp_us();
        let result4 = unix_timestamp_us();
        let result5 = unix_timestamp_us();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1_650_000_000_000_000);
    }

    #[rstest]
    fn test_unix_timestamp_ns_is_monotonic_increasing() {
        let result1 = unix_timestamp_ns();
        let result2 = unix_timestamp_ns();
        let result3 = unix_timestamp_ns();
        let result4 = unix_timestamp_ns();
        let result5 = unix_timestamp_ns();

        // println!("{}", result1);
        // println!("{}", result2);
        // println!("{}", result3);
        // println!("{}", result4);
        // println!("{}", result5);

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1_650_000_000_000_000_000);
    }
}

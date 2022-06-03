// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use lazy_static::lazy_static;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// Represents a timestamp in nanoseconds since UNIX epoch.
pub type Timestamp = u64;

/// Represents a timedelta in nanoseconds.
pub type Timedelta = i64;

// A static reference to an instant of system time.
// This is used to calculate monotonic durations of elapsed time.
lazy_static! {
    pub static ref INSTANT: Instant = Instant::now();
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

/// Returns monotonic time elapsed from given instant
#[no_mangle]
pub extern "C" fn mono_unix_timestamp() -> f64 {
    INSTANT.elapsed().as_secs_f64()
}

/// Returns monotonic time elapsed from given instant in milliseconds
#[no_mangle]
pub extern "C" fn mono_unix_timestamp_ms() -> u64 {
    INSTANT.elapsed().as_millis() as u64
}

/// Returns monotonic time elapsed from given instant in microseconds
#[no_mangle]
pub extern "C" fn mono_unix_timestamp_us() -> u64 {
    INSTANT.elapsed().as_micros() as u64
}

/// Returns monotonic time elapsed from given instant in nanoseconds
#[no_mangle]
pub extern "C" fn mono_unix_timestamp_ns() -> u64 {
    INSTANT.elapsed().as_nanos() as u64
}

/// Returns the current seconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_secs_f64()
}

/// Returns the current milliseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_millis() as u64
}

/// Returns the current microseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_micros() as u64
}

/// Returns the current nanoseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_nanos() as u64
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::time;

    #[test]
    fn test_unix_timestamp_is_monotonic_increasing() {
        let result1 = time::unix_timestamp();
        let result2 = time::unix_timestamp();
        let result3 = time::unix_timestamp();
        let result4 = time::unix_timestamp();
        let result5 = time::unix_timestamp();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1650000000.0)
    }

    #[test]
    fn test_unix_timestamp_ms_is_monotonic_increasing() {
        let result1 = time::unix_timestamp_ms();
        let result2 = time::unix_timestamp_ms();
        let result3 = time::unix_timestamp_ms();
        let result4 = time::unix_timestamp_ms();
        let result5 = time::unix_timestamp_ms();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1650000000000)
    }

    #[test]
    fn test_unix_timestamp_us_is_monotonic_increasing() {
        let result1 = time::unix_timestamp_us();
        let result2 = time::unix_timestamp_us();
        let result3 = time::unix_timestamp_us();
        let result4 = time::unix_timestamp_us();
        let result5 = time::unix_timestamp_us();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1650000000000000)
    }

    #[test]
    fn test_unix_timestamp_ns_is_monotonic_increasing() {
        let result1 = time::unix_timestamp_ns();
        let result2 = time::unix_timestamp_ns();
        let result3 = time::unix_timestamp_ns();
        let result4 = time::unix_timestamp_ns();
        let result5 = time::unix_timestamp_ns();

        // println!("{}", result1);
        // println!("{}", result2);
        // println!("{}", result3);
        // println!("{}", result4);
        // println!("{}", result5);

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 1650000000000000000)
    }

    #[test]
    fn test_mono_unix_timestamp_is_monotonic_increasing() {
        let result1 = time::mono_unix_timestamp();
        let result2 = time::mono_unix_timestamp();
        let result3 = time::mono_unix_timestamp();
        let result4 = time::mono_unix_timestamp();
        let result5 = time::mono_unix_timestamp();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 0.0)
    }

    #[test]
    #[allow(unused_comparisons)]
    fn test_mono_unix_timestamp_ms_is_monotonic_increasing() {
        let result1 = time::mono_unix_timestamp_ms();
        let result2 = time::mono_unix_timestamp_ms();
        let result3 = time::mono_unix_timestamp_ms();
        let result4 = time::mono_unix_timestamp_ms();
        let result5 = time::mono_unix_timestamp_ms();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        // monotonic time elapsed may yield 0 because
        // not enough time has passed since instant
        assert!(result1 >= 0)
    }

    #[test]
    fn test_mono_unix_timestamp_us_is_monotonic_increasing() {
        let result1 = time::mono_unix_timestamp_us();
        let result2 = time::mono_unix_timestamp_us();
        let result3 = time::mono_unix_timestamp_us();
        let result4 = time::mono_unix_timestamp_us();
        let result5 = time::mono_unix_timestamp_us();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 0)
    }

    #[test]
    fn test_mono_unix_timestamp_ns_is_monotonic_increasing() {
        let result1 = time::mono_unix_timestamp_ns();
        let result2 = time::mono_unix_timestamp_ns();
        let result3 = time::mono_unix_timestamp_ns();
        let result4 = time::mono_unix_timestamp_ns();
        let result5 = time::mono_unix_timestamp_ns();

        assert!(result2 >= result1);
        assert!(result3 >= result2);
        assert!(result4 >= result3);
        assert!(result5 >= result4);
        assert!(result1 > 0)
    }
}

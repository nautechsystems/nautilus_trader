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

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// Represents a timestamp in UNIX nanoseconds.
pub type Timestamp = i64;

/// Represents a timedelta in nanoseconds.
pub type Timedelta = i64;

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
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
pub extern "C" fn unix_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_millis() as i64
}

/// Returns the current microseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_micros() as i64
}

/// Returns the current nanoseconds since the UNIX epoch.
#[no_mangle]
pub extern "C" fn unix_timestamp_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_nanos() as i64
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::time;

    #[test]
    fn test_unix_timestamp_returns_positive() {
        let result = time::unix_timestamp();
        assert!(result > 1610000000.0)
    }

    #[test]
    fn test_unix_timestamp_ms_returns_positive() {
        let result = time::unix_timestamp_ms();
        assert!(result > 1610000000000)
    }

    #[test]
    fn test_unix_timestamp_us_returns_positive() {
        let result = time::unix_timestamp_us();
        assert!(result > 1610000000000000)
    }

    #[test]
    fn test_unix_timestamp_ns_returns_positive() {
        let result = time::unix_timestamp_ns();
        assert!(result > 1610000000000000000)
    }
}

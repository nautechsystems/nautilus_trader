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

use std::time::{Duration, UNIX_EPOCH};

use chrono::prelude::{DateTime, Utc};
use chrono::SecondsFormat;

const MILLISECONDS_IN_SECOND: u64 = 1_000;
const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;
const NANOSECONDS_IN_MILLISECOND: u64 = 1_000_000;
const NANOSECONDS_IN_MICROSECOND: u64 = 1_000;

/// Converts seconds to nanoseconds (ns).
#[no_mangle]
#[inline]
pub extern "C" fn secs_to_nanos(secs: f64) -> u64 {
    (secs * NANOSECONDS_IN_SECOND as f64) as u64
}

/// Converts seconds to milliseconds (ms).
#[no_mangle]
#[inline]
pub extern "C" fn secs_to_millis(secs: f64) -> u64 {
    (secs * MILLISECONDS_IN_SECOND as f64) as u64
}

/// Converts milliseconds (ms) to nanoseconds (ns).
#[no_mangle]
#[inline]
pub extern "C" fn millis_to_nanos(millis: f64) -> u64 {
    (millis * NANOSECONDS_IN_MILLISECOND as f64) as u64
}

/// Converts microseconds (μs) to nanoseconds (ns).
#[no_mangle]
#[inline]
pub extern "C" fn micros_to_nanos(micros: f64) -> u64 {
    (micros * NANOSECONDS_IN_MICROSECOND as f64) as u64
}

/// Converts nanoseconds (ns) to seconds.
#[no_mangle]
#[inline]
pub extern "C" fn nanos_to_secs(nanos: u64) -> f64 {
    nanos as f64 / NANOSECONDS_IN_SECOND as f64
}

/// Converts nanoseconds (ns) to milliseconds (ms).
#[no_mangle]
#[inline]
pub extern "C" fn nanos_to_millis(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MILLISECOND
}

/// Converts nanoseconds (ns) to microseconds (μs).
#[no_mangle]
#[inline]
pub extern "C" fn nanos_to_micros(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MICROSECOND
}

#[inline]
#[must_use]
pub fn unix_nanos_to_iso8601(timestamp_ns: u64) -> String {
    let dt = DateTime::<Utc>::from(UNIX_EPOCH + Duration::from_nanos(timestamp_ns));
    dt.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(0.0, 0)]
    #[case(1.0, 1_000_000_000)]
    #[case(1.1, 1_100_000_000)]
    #[case(42.0, 42_000_000_000)]
    #[case(0.000_123_5, 123_500)]
    #[case(0.000_000_01, 10)]
    #[case(0.000_000_001, 1)]
    #[case(9.999_999_999, 9_999_999_999)]
    fn test_secs_to_nanos(#[case] value: f64, #[case] expected: u64) {
        let result = secs_to_nanos(value);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0.0, 0)]
    #[case(1.0, 1_000)]
    #[case(1.1, 1_100)]
    #[case(42.0, 42_000)]
    #[case(0.012_34, 12)]
    #[case(0.001, 1)]
    fn test_secs_to_millis(#[case] value: f64, #[case] expected: u64) {
        let result = secs_to_millis(value);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0.0, 0)]
    #[case(1.0, 1_000_000)]
    #[case(1.1, 1_100_000)]
    #[case(42.0, 42_000_000)]
    #[case(0.000_123_4, 123)]
    #[case(0.000_01, 10)]
    #[case(0.000_001, 1)]
    #[case(9.999_999, 9_999_999)]
    fn test_millis_to_nanos(#[case] value: f64, #[case] expected: u64) {
        let result = millis_to_nanos(value);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0.0, 0)]
    #[case(1.0, 1_000)]
    #[case(1.1, 1_100)]
    #[case(42.0, 42_000)]
    #[case(0.1234, 123)]
    #[case(0.01, 10)]
    #[case(0.001, 1)]
    #[case(9.999, 9_999)]
    fn test_micros_to_nanos(#[case] value: f64, #[case] expected: u64) {
        let result = micros_to_nanos(value);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0, 0.0)]
    #[case(1, 1e-09)]
    #[case(1_000_000_000, 1.0)]
    #[case(42_897_123_111, 42.897_123_111)]
    fn test_nanos_to_secs(#[case] value: u64, #[case] expected: f64) {
        let result = nanos_to_secs(value);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0, 0)]
    #[case(1_000_000, 1)]
    #[case(1_000_000_000, 1000)]
    #[case(42_897_123_111, 42897)]
    fn test_nanos_to_millis(#[case] value: u64, #[case] expected: u64) {
        let result = nanos_to_millis(value);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0, 0)]
    #[case(1_000, 1)]
    #[case(1_000_000_000, 1_000_000)]
    #[case(42_897_123, 42_897)]
    fn test_nanos_to_micros(#[case] value: u64, #[case] expected: u64) {
        let result = nanos_to_micros(value);
        assert_eq!(result, expected);
    }
}

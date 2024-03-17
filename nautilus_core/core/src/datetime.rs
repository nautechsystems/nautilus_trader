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

use std::time::{Duration, UNIX_EPOCH};

use chrono::{
    prelude::{DateTime, Utc},
    Datelike, NaiveDate, SecondsFormat, TimeDelta, Weekday,
};

use crate::time::UnixNanos;

pub const MILLISECONDS_IN_SECOND: u64 = 1_000;
pub const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;
pub const NANOSECONDS_IN_MILLISECOND: u64 = 1_000_000;
pub const NANOSECONDS_IN_MICROSECOND: u64 = 1_000;

pub const WEEKDAYS: [Weekday; 5] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
];

/// Converts seconds to nanoseconds (ns).
#[inline]
#[no_mangle]
pub extern "C" fn secs_to_nanos(secs: f64) -> u64 {
    (secs * NANOSECONDS_IN_SECOND as f64) as u64
}

/// Converts seconds to milliseconds (ms).
#[inline]
#[no_mangle]
pub extern "C" fn secs_to_millis(secs: f64) -> u64 {
    (secs * MILLISECONDS_IN_SECOND as f64) as u64
}

/// Converts milliseconds (ms) to nanoseconds (ns).
#[inline]
#[no_mangle]
pub extern "C" fn millis_to_nanos(millis: f64) -> u64 {
    (millis * NANOSECONDS_IN_MILLISECOND as f64) as u64
}

/// Converts microseconds (μs) to nanoseconds (ns).
#[inline]
#[no_mangle]
pub extern "C" fn micros_to_nanos(micros: f64) -> u64 {
    (micros * NANOSECONDS_IN_MICROSECOND as f64) as u64
}

/// Converts nanoseconds (ns) to seconds.
#[inline]
#[no_mangle]
pub extern "C" fn nanos_to_secs(nanos: u64) -> f64 {
    nanos as f64 / NANOSECONDS_IN_SECOND as f64
}

/// Converts nanoseconds (ns) to milliseconds (ms).
#[inline]
#[no_mangle]
pub extern "C" fn nanos_to_millis(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MILLISECOND
}

/// Converts nanoseconds (ns) to microseconds (μs).
#[inline]
#[no_mangle]
pub extern "C" fn nanos_to_micros(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MICROSECOND
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 formatted string.
#[inline]
#[must_use]
pub fn unix_nanos_to_iso8601(timestamp_ns: u64) -> String {
    let dt = DateTime::<Utc>::from(UNIX_EPOCH + Duration::from_nanos(timestamp_ns));
    dt.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub fn last_weekday_nanos(year: i32, month: u32, day: u32) -> anyhow::Result<UnixNanos> {
    let date =
        NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| anyhow::anyhow!("Invalid date"))?;
    let current_weekday = date.weekday().number_from_monday();

    // Calculate the offset in days for closest weekday (Mon-Fri)
    let offset = i64::from(match current_weekday {
        1..=5 => 0, // Monday to Friday, no adjustment needed
        6 => 1,     // Saturday, adjust to previous Friday
        _ => 2,     // Sunday, adjust to previous Friday
    });

    // Calculate last closest weekday
    let last_closest = date - TimeDelta::try_days(offset).expect("Invalid offset");

    // Convert to UNIX nanoseconds
    let unix_timestamp_ns = last_closest
        .and_hms_nano_opt(0, 0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("Failed `and_hms_nano_opt`"))?;

    Ok(unix_timestamp_ns
        .and_utc()
        .timestamp_nanos_opt()
        .ok_or_else(|| anyhow::anyhow!("Failed `timestamp_nanos_opt`"))? as UnixNanos)
}

pub fn is_within_last_24_hours(timestamp_ns: UnixNanos) -> anyhow::Result<bool> {
    let seconds = timestamp_ns / NANOSECONDS_IN_SECOND;
    let nanoseconds = (timestamp_ns % NANOSECONDS_IN_SECOND) as u32;
    let timestamp = DateTime::from_timestamp(seconds as i64, nanoseconds)
        .ok_or_else(|| anyhow::anyhow!("Invalid timestamp {timestamp_ns}"))?;
    let now = Utc::now();

    Ok(now.signed_duration_since(timestamp) <= TimeDelta::try_days(1).unwrap())
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

    #[rstest]
    #[case(2023, 12, 15, 1_702_598_400_000_000_000)] // Fri
    #[case(2023, 12, 16, 1_702_598_400_000_000_000)] // Sat
    #[case(2023, 12, 17, 1_702_598_400_000_000_000)] // Sun
    #[case(2023, 12, 18, 1_702_857_600_000_000_000)] // Mon
    fn test_last_closest_weekday_nanos_with_valid_date(
        #[case] year: i32,
        #[case] month: u32,
        #[case] day: u32,
        #[case] expected: u64,
    ) {
        let result = last_weekday_nanos(year, month, day).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_last_closest_weekday_nanos_with_invalid_date() {
        let result = last_weekday_nanos(2023, 4, 31);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_last_closest_weekday_nanos_with_nonexistent_date() {
        let result = last_weekday_nanos(2023, 2, 30);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_last_closest_weekday_nanos_with_invalid_conversion() {
        let result = last_weekday_nanos(9999, 12, 31);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_is_within_last_24_hours_when_now() {
        let now_ns = Utc::now().timestamp_nanos_opt().unwrap();
        assert!(is_within_last_24_hours(now_ns as UnixNanos).unwrap());
    }

    #[rstest]
    fn test_is_within_last_24_hours_when_two_days_ago() {
        let past_ns = (Utc::now() - TimeDelta::try_days(2).unwrap())
            .timestamp_nanos_opt()
            .unwrap();
        assert!(!is_within_last_24_hours(past_ns as UnixNanos).unwrap());
    }
}

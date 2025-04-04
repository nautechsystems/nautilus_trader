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

//! Common data and time functions.

use chrono::{DateTime, Datelike, NaiveDate, SecondsFormat, TimeDelta, Utc, Weekday};

use crate::UnixNanos;

/// Number of milliseconds in one second.
pub const MILLISECONDS_IN_SECOND: u64 = 1_000;

/// Number of nanoseconds in one second.
pub const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;

/// Number of nanoseconds in one millisecond.
pub const NANOSECONDS_IN_MILLISECOND: u64 = 1_000_000;

/// Number of nanoseconds in one microsecond.
pub const NANOSECONDS_IN_MICROSECOND: u64 = 1_000;

/// List of weekdays (Monday to Friday).
pub const WEEKDAYS: [Weekday; 5] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
];

/// Converts seconds to nanoseconds (ns).
#[unsafe(no_mangle)]
pub extern "C" fn secs_to_nanos(secs: f64) -> u64 {
    (secs * NANOSECONDS_IN_SECOND as f64) as u64
}

/// Converts seconds to milliseconds (ms).
#[unsafe(no_mangle)]
pub extern "C" fn secs_to_millis(secs: f64) -> u64 {
    (secs * MILLISECONDS_IN_SECOND as f64) as u64
}

/// Converts milliseconds (ms) to nanoseconds (ns).
#[unsafe(no_mangle)]
pub extern "C" fn millis_to_nanos(millis: f64) -> u64 {
    (millis * NANOSECONDS_IN_MILLISECOND as f64) as u64
}

/// Converts microseconds (μs) to nanoseconds (ns).
#[unsafe(no_mangle)]
pub extern "C" fn micros_to_nanos(micros: f64) -> u64 {
    (micros * NANOSECONDS_IN_MICROSECOND as f64) as u64
}

/// Converts nanoseconds (ns) to seconds.
#[unsafe(no_mangle)]
pub extern "C" fn nanos_to_secs(nanos: u64) -> f64 {
    nanos as f64 / NANOSECONDS_IN_SECOND as f64
}

/// Converts nanoseconds (ns) to milliseconds (ms).
#[unsafe(no_mangle)]
pub const extern "C" fn nanos_to_millis(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MILLISECOND
}

/// Converts nanoseconds (ns) to microseconds (μs).
#[unsafe(no_mangle)]
pub const extern "C" fn nanos_to_micros(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MICROSECOND
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format string.
#[inline]
#[must_use]
pub fn unix_nanos_to_iso8601(unix_nanos: UnixNanos) -> String {
    let datetime = unix_nanos.to_datetime_utc();
    datetime.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format string
/// with millisecond precision.
#[inline]
#[must_use]
pub fn unix_nanos_to_iso8601_millis(unix_nanos: UnixNanos) -> String {
    let datetime = unix_nanos.to_datetime_utc();
    datetime.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Floor the given UNIX nanoseconds to the nearest microsecond.
#[must_use]
pub const fn floor_to_nearest_microsecond(unix_nanos: u64) -> u64 {
    (unix_nanos / NANOSECONDS_IN_MICROSECOND) * NANOSECONDS_IN_MICROSECOND
}

/// Calculates the last weekday (Mon-Fri) from the given `year`, `month` and `day`.
///
/// # Errors
///
/// Returns an error if the date is invalid.
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
    let last_closest = date - TimeDelta::days(offset);

    // Convert to UNIX nanoseconds
    let unix_timestamp_ns = last_closest
        .and_hms_nano_opt(0, 0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("Failed `and_hms_nano_opt`"))?;

    Ok(UnixNanos::from(
        unix_timestamp_ns
            .and_utc()
            .timestamp_nanos_opt()
            .ok_or_else(|| anyhow::anyhow!("Failed `timestamp_nanos_opt`"))? as u64,
    ))
}

/// Check whether the given UNIX nanoseconds timestamp is within the last 24 hours.
///
/// # Errors
///
/// Returns an error if the timestamp is invalid.
pub fn is_within_last_24_hours(timestamp_ns: UnixNanos) -> anyhow::Result<bool> {
    let timestamp_ns = timestamp_ns.as_u64();
    let seconds = timestamp_ns / NANOSECONDS_IN_SECOND;
    let nanoseconds = (timestamp_ns % NANOSECONDS_IN_SECOND) as u32;
    let timestamp = DateTime::from_timestamp(seconds as i64, nanoseconds)
        .ok_or_else(|| anyhow::anyhow!("Invalid timestamp {timestamp_ns}"))?;
    let now = Utc::now();

    Ok(now.signed_duration_since(timestamp) <= TimeDelta::days(1))
}

/// Subtract `n` months from a chrono `DateTime<Utc>`.
#[must_use]
pub fn subtract_n_months(datetime: DateTime<Utc>, n: u32) -> DateTime<Utc> {
    datetime
        .checked_sub_months(chrono::Months::new(n))
        .expect("Failed to subtract months")
}

/// Add `n` months to a chrono `DateTime<Utc>`.
#[must_use]
pub fn add_n_months(datetime: DateTime<Utc>, n: u32) -> DateTime<Utc> {
    datetime
        .checked_add_months(chrono::Months::new(n))
        .expect("Failed to add months")
}

/// Subtract `n` months from a given UNIX nanoseconds timestamp.
#[must_use]
pub fn subtract_n_months_nanos(unix_nanos: UnixNanos, n: u32) -> UnixNanos {
    let datetime = unix_nanos.to_datetime_utc();
    (subtract_n_months(datetime, n)
        .timestamp_nanos_opt()
        .expect("Months should be within 584 years") as u64)
        .into()
}

/// Add `n` months to a given UNIX nanoseconds timestamp.
#[must_use]
pub fn add_n_months_nanos(unix_nanos: UnixNanos, n: u32) -> UnixNanos {
    let date = unix_nanos.to_datetime_utc();
    (add_n_months(date, n)
        .timestamp_nanos_opt()
        .expect("Months should be within 584 years") as u64)
        .into()
}

/// Returns the last valid day of `(year, month)`.
#[must_use]
pub const fn last_day_of_month(year: i32, month: u32) -> u32 {
    // E.g., for February, check leap year logic
    match month {
        1 => 31,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 31, // fallback
    }
}

/// Basic leap-year check
#[must_use]
pub const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
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
    #[case(0, "1970-01-01T00:00:00.000000000Z")] // Unix epoch
    #[case(1, "1970-01-01T00:00:00.000000001Z")] // 1 nanosecond
    #[case(1_000, "1970-01-01T00:00:00.000001000Z")] // 1 microsecond
    #[case(1_000_000, "1970-01-01T00:00:00.001000000Z")] // 1 millisecond
    #[case(1_000_000_000, "1970-01-01T00:00:01.000000000Z")] // 1 second
    #[case(1_702_857_600_000_000_000, "2023-12-18T00:00:00.000000000Z")] // Specific date
    fn test_unix_nanos_to_iso8601(#[case] nanos: u64, #[case] expected: &str) {
        let result = unix_nanos_to_iso8601(UnixNanos::from(nanos));
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0, "1970-01-01T00:00:00.000Z")] // Unix epoch
    #[case(1_000_000, "1970-01-01T00:00:00.001Z")] // 1 millisecond
    #[case(1_000_000_000, "1970-01-01T00:00:01.000Z")] // 1 second
    #[case(1_702_857_600_123_456_789, "2023-12-18T00:00:00.123Z")] // With millisecond precision
    fn test_unix_nanos_to_iso8601_millis(#[case] nanos: u64, #[case] expected: &str) {
        let result = unix_nanos_to_iso8601_millis(UnixNanos::from(nanos));
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
        let result = last_weekday_nanos(year, month, day).unwrap().as_u64();
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
        assert!(is_within_last_24_hours(UnixNanos::from(now_ns as u64)).unwrap());
    }

    #[rstest]
    fn test_is_within_last_24_hours_when_two_days_ago() {
        let past_ns = (Utc::now() - TimeDelta::try_days(2).unwrap())
            .timestamp_nanos_opt()
            .unwrap();
        assert!(!is_within_last_24_hours(UnixNanos::from(past_ns as u64)).unwrap());
    }

    #[rstest]
    #[case(Utc.with_ymd_and_hms(2024, 3, 31, 12, 0, 0).unwrap(), 1, Utc.with_ymd_and_hms(2024, 2, 29, 12, 0, 0).unwrap())] // Leap year February
    #[case(Utc.with_ymd_and_hms(2024, 3, 31, 12, 0, 0).unwrap(), 12, Utc.with_ymd_and_hms(2023, 3, 31, 12, 0, 0).unwrap())] // One year earlier
    #[case(Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap(), 1, Utc.with_ymd_and_hms(2023, 12, 31, 12, 0, 0).unwrap())] // Wrapping to previous year
    #[case(Utc.with_ymd_and_hms(2024, 3, 31, 12, 0, 0).unwrap(), 2, Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap())] // Multiple months back
    fn test_subtract_n_months(
        #[case] input: DateTime<Utc>,
        #[case] months: u32,
        #[case] expected: DateTime<Utc>,
    ) {
        let result = subtract_n_months(input, months);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(Utc.with_ymd_and_hms(2023, 2, 28, 12, 0, 0).unwrap(), 1, Utc.with_ymd_and_hms(2023, 3, 28, 12, 0, 0).unwrap())] // Simple month addition
    #[case(Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap(), 1, Utc.with_ymd_and_hms(2024, 2, 29, 12, 0, 0).unwrap())] // Leap year February
    #[case(Utc.with_ymd_and_hms(2023, 12, 31, 12, 0, 0).unwrap(), 1, Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap())] // Wrapping to next year
    #[case(Utc.with_ymd_and_hms(2023, 1, 31, 12, 0, 0).unwrap(), 13, Utc.with_ymd_and_hms(2024, 2, 29, 12, 0, 0).unwrap())] // Crossing year boundary with multiple months
    fn test_add_n_months(
        #[case] input: DateTime<Utc>,
        #[case] months: u32,
        #[case] expected: DateTime<Utc>,
    ) {
        let result = add_n_months(input, months);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(2024, 2, 29)] // Leap year February
    #[case(2023, 2, 28)] // Non-leap year February
    #[case(2024, 12, 31)] // December
    #[case(2023, 11, 30)] // November
    fn test_last_day_of_month(#[case] year: i32, #[case] month: u32, #[case] expected: u32) {
        let result = last_day_of_month(year, month);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(2024, true)] // Leap year divisible by 4
    #[case(1900, false)] // Not leap year, divisible by 100 but not 400
    #[case(2000, true)] // Leap year, divisible by 400
    #[case(2023, false)] // Non-leap year
    fn test_is_leap_year(#[case] year: i32, #[case] expected: bool) {
        let result = is_leap_year(year);
        assert_eq!(result, expected);
    }
}

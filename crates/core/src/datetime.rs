// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
use std::{convert::TryFrom, sync::LazyLock};

use jiff::{
    Span, Timestamp,
    civil::{Date, Weekday},
    tz::{TimeZone, TimeZoneDatabase},
};

use crate::{UnixNanos, time::nanos_since_unix_epoch};

/// Number of milliseconds in one second.
pub const MILLISECONDS_IN_SECOND: u64 = 1_000;

/// Number of nanoseconds in one second.
pub const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;

/// Number of nanoseconds in one millisecond.
pub const NANOSECONDS_IN_MILLISECOND: u64 = 1_000_000;
const NANOSECONDS_IN_MILLISECOND_U32: u32 = 1_000_000;

/// Number of nanoseconds in one microsecond.
pub const NANOSECONDS_IN_MICROSECOND: u64 = 1_000;

/// Number of nanoseconds in one minute.
pub const NANOSECONDS_IN_MINUTE: u64 = 60 * NANOSECONDS_IN_SECOND;

/// Number of nanoseconds in one day.
pub const NANOSECONDS_IN_DAY: u64 = 24 * 60 * NANOSECONDS_IN_MINUTE;

/// Number of seconds in one minute.
pub const SECONDS_IN_MINUTE: u64 = 60;

/// Number of seconds in one hour.
pub const SECONDS_IN_HOUR: u64 = 60 * SECONDS_IN_MINUTE;

/// Number of seconds in one day.
pub const SECONDS_IN_DAY: u64 = 24 * SECONDS_IN_HOUR;

#[expect(
    clippy::cast_precision_loss,
    reason = "u64::MAX rounds to the exact exclusive 2^64 upper bound"
)]
pub(crate) const U64_UPPER_BOUND_F64: f64 = u64::MAX as f64;

static BUNDLED_TIME_ZONE_DATABASE: LazyLock<TimeZoneDatabase> =
    LazyLock::new(TimeZoneDatabase::bundled);

// Compile-time checks for time constants to prevent accidental modification
const _: () = {
    assert!(NANOSECONDS_IN_SECOND == 1_000_000_000);
    assert!(NANOSECONDS_IN_MILLISECOND == 1_000_000);
    assert!(NANOSECONDS_IN_MICROSECOND == 1_000);
    assert!(MILLISECONDS_IN_SECOND == 1_000);
    assert!(NANOSECONDS_IN_SECOND == MILLISECONDS_IN_SECOND * NANOSECONDS_IN_MILLISECOND);
    assert!(NANOSECONDS_IN_MILLISECOND == NANOSECONDS_IN_MICROSECOND * 1_000);
    assert!(NANOSECONDS_IN_SECOND / NANOSECONDS_IN_MILLISECOND == 1_000);
    assert!(NANOSECONDS_IN_SECOND / NANOSECONDS_IN_MICROSECOND == 1_000_000);
    assert!(SECONDS_IN_MINUTE == 60);
    assert!(SECONDS_IN_HOUR == 3_600);
    assert!(SECONDS_IN_DAY == 86_400);
    assert!(NANOSECONDS_IN_MINUTE == 60 * NANOSECONDS_IN_SECOND);
    assert!(NANOSECONDS_IN_DAY == 24 * 60 * NANOSECONDS_IN_MINUTE);
};

/// Resolves an IANA time zone from the bundled database.
///
/// The bundled database is intentional: it keeps time zone behavior deterministic across hosts
/// and avoids system time zone I/O in latency-sensitive paths.
///
/// # Errors
///
/// Returns an error if `name` is not present in the bundled IANA database.
pub fn get_timezone(name: &str) -> Result<TimeZone, jiff::Error> {
    BUNDLED_TIME_ZONE_DATABASE.get(name)
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    // Howard Hinnant's civil calendar algorithm maps UTC epoch days to a
    // Gregorian date using integer arithmetic only. The input is already UTC,
    // so no timezone or leap-second rules are involved in this formatter.
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);

    (
        i32::try_from(year).expect("year fits in i32"),
        u32::try_from(month).expect("month is positive"),
        u32::try_from(day).expect("day is positive"),
    )
}

struct DateTimeParts {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    subsec_nanos: u32,
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "digit helpers only receive values in 0..=9"
)]
fn push_digit(out: &mut String, digit: u32) {
    out.push(char::from(b'0' + digit as u8));
}

fn push_2_digits(out: &mut String, value: u32) {
    debug_assert!(value < 100);
    push_digit(out, value / 10);
    push_digit(out, value % 10);
}

fn push_3_digits(out: &mut String, value: u32) {
    debug_assert!(value < 1_000);
    push_digit(out, value / 100);
    push_2_digits(out, value % 100);
}

fn push_4_digits(out: &mut String, value: i32) {
    debug_assert!((0..=9_999).contains(&value));
    let value = u32::try_from(value).expect("year is non-negative");
    push_digit(out, value / 1_000);
    push_digit(out, (value / 100) % 10);
    push_2_digits(out, value % 100);
}

fn push_9_digits(out: &mut String, value: u32) {
    debug_assert!(value < 1_000_000_000);
    let mut divisor = 100_000_000;
    while divisor > 0 {
        push_digit(out, value / divisor % 10);
        divisor /= 10;
    }
}

fn split_unix_nanos(unix_nanos: UnixNanos) -> DateTimeParts {
    let nanos = unix_nanos.as_u64();
    let total_seconds = nanos / NANOSECONDS_IN_SECOND;
    let subsec_nanos = u32::try_from(nanos % NANOSECONDS_IN_SECOND).expect("subsecond fits u32");
    let days = total_seconds / SECONDS_IN_DAY;
    let seconds_of_day = total_seconds % SECONDS_IN_DAY;
    let (year, month, day) =
        civil_from_days(i64::try_from(days).expect("days since epoch fits i64"));
    let hour = u32::try_from(seconds_of_day / SECONDS_IN_HOUR).expect("hour fits u32");
    let minute =
        u32::try_from((seconds_of_day % SECONDS_IN_HOUR) / SECONDS_IN_MINUTE).expect("minute fits");
    let second = u32::try_from(seconds_of_day % SECONDS_IN_MINUTE).expect("second fits");

    DateTimeParts {
        year,
        month,
        day,
        hour,
        minute,
        second,
        subsec_nanos,
    }
}

fn push_iso8601_prefix(
    out: &mut String,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) {
    push_4_digits(out, year);
    out.push('-');
    push_2_digits(out, month);
    out.push('-');
    push_2_digits(out, day);
    out.push('T');
    push_2_digits(out, hour);
    out.push(':');
    push_2_digits(out, minute);
    out.push(':');
    push_2_digits(out, second);
    out.push('.');
}

/// List of weekdays (Monday to Friday).
pub const WEEKDAYS: [Weekday; 5] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
];

/// Converts seconds to nanoseconds (ns).
///
/// # Errors
///
/// Returns an error if `secs` is non-finite or cannot be represented as `u64` nanoseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "Intentional for unit conversion, may lose precision after clamping"
)]
pub fn secs_to_nanos(secs: f64) -> anyhow::Result<u64> {
    anyhow::ensure!(secs.is_finite(), "seconds must be finite, was {secs}");
    if secs <= 0.0 {
        return Ok(0);
    }
    let nanos = secs * NANOSECONDS_IN_SECOND as f64;
    anyhow::ensure!(
        nanos < U64_UPPER_BOUND_F64,
        "seconds {secs} is out of range for `u64` nanoseconds"
    );
    Ok(nanos.trunc() as u64)
}

/// Converts seconds to milliseconds (ms).
///
/// # Errors
///
/// Returns an error if `secs` is non-finite or cannot be represented as `u64` milliseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "Intentional for unit conversion, may lose precision after clamping"
)]
pub fn secs_to_millis(secs: f64) -> anyhow::Result<u64> {
    anyhow::ensure!(secs.is_finite(), "seconds must be finite, was {secs}");
    if secs <= 0.0 {
        return Ok(0);
    }
    let millis = secs * MILLISECONDS_IN_SECOND as f64;
    anyhow::ensure!(
        millis < U64_UPPER_BOUND_F64,
        "seconds {secs} is out of range for `u64` milliseconds"
    );
    Ok(millis.trunc() as u64)
}

/// Converts seconds to nanoseconds (ns), panicking on invalid input.
///
/// This is a convenience wrapper around [`secs_to_nanos`] when the caller expects
/// the input to be trusted and in-range.
///
/// # Panics
///
/// Panics if [`secs_to_nanos`] would return an error for `secs`.
#[must_use]
pub fn secs_to_nanos_unchecked(secs: f64) -> u64 {
    secs_to_nanos(secs).expect("secs_to_nanos_unchecked: invalid or overflowing input")
}

/// Converts minutes to seconds.
///
/// # Panics
///
/// Panics if the result cannot be represented as `u64` seconds.
#[must_use]
pub const fn mins_to_secs(mins: u64) -> u64 {
    checked_mins_to_secs(mins).expect("minutes to seconds conversion overflow")
}

/// Converts minutes to seconds, returning `None` on overflow.
#[must_use]
pub const fn checked_mins_to_secs(mins: u64) -> Option<u64> {
    mins.checked_mul(SECONDS_IN_MINUTE)
}

/// Converts minutes to nanoseconds.
///
/// # Panics
///
/// Panics if the result cannot be represented as `u64` nanoseconds.
#[must_use]
pub const fn mins_to_nanos(mins: u64) -> u64 {
    checked_mins_to_nanos(mins).expect("minutes to nanoseconds conversion overflow")
}

/// Converts minutes to nanoseconds, returning `None` on overflow.
#[must_use]
pub const fn checked_mins_to_nanos(mins: u64) -> Option<u64> {
    mins.checked_mul(NANOSECONDS_IN_MINUTE)
}

/// Converts milliseconds (ms) to nanoseconds (ns).
///
/// Casting f64 to u64 by truncating the fractional part is intentional for unit conversion,
/// which may lose precision and drop negative values after clamping.
///
/// # Errors
///
/// Returns an error if `millis` is non-finite or cannot be represented as `u64` nanoseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "Intentional for unit conversion, may lose precision after clamping"
)]
pub fn millis_to_nanos(millis: f64) -> anyhow::Result<u64> {
    anyhow::ensure!(
        millis.is_finite(),
        "milliseconds must be finite, was {millis}"
    );

    if millis <= 0.0 {
        return Ok(0);
    }
    let nanos = millis * NANOSECONDS_IN_MILLISECOND as f64;
    anyhow::ensure!(
        nanos < U64_UPPER_BOUND_F64,
        "milliseconds {millis} is out of range for `u64` nanoseconds"
    );
    Ok(nanos.trunc() as u64)
}

/// Converts milliseconds (ms) to nanoseconds (ns), panicking on invalid input.
///
/// # Panics
///
/// Panics if [`millis_to_nanos`] would return an error for `millis`.
#[must_use]
pub fn millis_to_nanos_unchecked(millis: f64) -> u64 {
    millis_to_nanos(millis).expect("millis_to_nanos_unchecked: invalid or overflowing input")
}

/// Converts microseconds (μs) to nanoseconds (ns).
///
/// Casting f64 to u64 by truncating the fractional part is intentional for unit conversion,
/// which may lose precision and drop negative values after clamping.
///
/// # Errors
///
/// Returns an error if `micros` is non-finite or cannot be represented as `u64` nanoseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "Intentional for unit conversion, may lose precision after clamping"
)]
pub fn micros_to_nanos(micros: f64) -> anyhow::Result<u64> {
    anyhow::ensure!(
        micros.is_finite(),
        "microseconds must be finite, was {micros}"
    );

    if micros <= 0.0 {
        return Ok(0);
    }
    let nanos = micros * NANOSECONDS_IN_MICROSECOND as f64;
    anyhow::ensure!(
        nanos < U64_UPPER_BOUND_F64,
        "microseconds {micros} is out of range for `u64` nanoseconds"
    );
    Ok(nanos.trunc() as u64)
}

/// Converts microseconds (μs) to nanoseconds (ns), panicking on invalid input.
///
/// # Panics
///
/// Panics if [`micros_to_nanos`] would return an error for `micros`.
#[must_use]
pub fn micros_to_nanos_unchecked(micros: f64) -> u64 {
    micros_to_nanos(micros).expect("micros_to_nanos_unchecked: invalid or overflowing input")
}

/// Converts nanoseconds (ns) to seconds.
///
/// Casting u64 to f64 may lose precision for large values,
/// but is acceptable when computing fractional seconds.
#[expect(
    clippy::cast_precision_loss,
    reason = "Precision loss acceptable for time conversion"
)]
#[must_use]
pub fn nanos_to_secs(nanos: u64) -> f64 {
    let seconds = nanos / NANOSECONDS_IN_SECOND;
    let rem_nanos = nanos % NANOSECONDS_IN_SECOND;
    (seconds as f64) + (rem_nanos as f64) / (NANOSECONDS_IN_SECOND as f64)
}

/// Converts nanoseconds (ns) to milliseconds (ms).
#[must_use]
pub const fn nanos_to_millis(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MILLISECOND
}

/// Converts nanoseconds (ns) to microseconds (μs).
#[must_use]
pub const fn nanos_to_micros(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MICROSECOND
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format string.
///
/// All [`UnixNanos`] values are representable by this formatter.
#[inline]
#[must_use]
pub fn unix_nanos_to_iso8601(unix_nanos: UnixNanos) -> String {
    let parts = split_unix_nanos(unix_nanos);

    let mut out = String::with_capacity(30);
    push_iso8601_prefix(
        &mut out,
        parts.year,
        parts.month,
        parts.day,
        parts.hour,
        parts.minute,
        parts.second,
    );
    push_9_digits(&mut out, parts.subsec_nanos);
    out.push('Z');
    out
}

/// Converts an ISO 8601 (RFC 3339) format string to UNIX nanoseconds timestamp.
///
/// This function accepts various ISO 8601 formats including:
/// - Full RFC 3339 with nanosecond precision: "2024-02-10T14:58:43.456789Z"
/// - RFC 3339 without fractional seconds: "2024-02-10T14:58:43Z"
/// - Simple date format: "2024-02-10" (interpreted as midnight UTC)
///
/// # Parameters
///
/// - `date_string`: The ISO 8601 formatted date string to parse
///
/// # Returns
///
/// Returns `Ok(UnixNanos)` if the string is successfully parsed, or an error if the format
/// is invalid or the timestamp is out of range.
///
/// # Errors
///
/// Returns an error if:
/// - The string format is not a valid ISO 8601 format
/// - The timestamp is out of range for `UnixNanos`
/// - The date/time values are invalid
#[inline]
pub fn iso8601_to_unix_nanos(date_string: &str) -> anyhow::Result<UnixNanos> {
    date_string
        .parse::<UnixNanos>()
        .map_err(|e| anyhow::anyhow!("Failed to parse ISO 8601 string '{date_string}': {e}"))
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format string
/// with millisecond precision.
///
/// All [`UnixNanos`] values are representable by this formatter.
#[inline]
#[must_use]
pub fn unix_nanos_to_iso8601_millis(unix_nanos: UnixNanos) -> String {
    let parts = split_unix_nanos(unix_nanos);

    let mut out = String::with_capacity(24);
    push_iso8601_prefix(
        &mut out,
        parts.year,
        parts.month,
        parts.day,
        parts.hour,
        parts.minute,
        parts.second,
    );
    push_3_digits(
        &mut out,
        parts.subsec_nanos / NANOSECONDS_IN_MILLISECOND_U32,
    );
    out.push('Z');
    out
}

/// Floor the given UNIX nanoseconds to the nearest microsecond.
#[must_use]
pub const fn floor_to_nearest_microsecond(unix_nanos: u64) -> u64 {
    (unix_nanos / NANOSECONDS_IN_MICROSECOND) * NANOSECONDS_IN_MICROSECOND
}

/// Calculates the last weekday (Mon-Fri) from the given `year`, `month`, and `day`.
///
/// # Errors
///
/// Returns an error if the date is invalid.
pub fn last_weekday_nanos(year: i32, month: u32, day: u32) -> anyhow::Result<UnixNanos> {
    let date = Date::new(
        i16::try_from(year).map_err(|_| anyhow::anyhow!("Invalid date"))?,
        i8::try_from(month).map_err(|_| anyhow::anyhow!("Invalid date"))?,
        i8::try_from(day).map_err(|_| anyhow::anyhow!("Invalid date"))?,
    )
    .map_err(|_| anyhow::anyhow!("Invalid date"))?;
    let current_weekday = date.weekday().to_monday_one_offset();

    // Calculate the offset in days for closest weekday (Mon-Fri)
    let offset = match current_weekday {
        1..=5 => 0, // Monday to Friday, no adjustment needed
        6 => 1,     // Saturday, adjust to previous Friday
        _ => 2,     // Sunday, adjust to previous Friday
    };
    // Calculate last closest weekday
    let last_closest = date.checked_sub(Span::new().days(offset))?;

    // Convert to UNIX nanoseconds
    let unix_timestamp_ns = last_closest
        .at(0, 0, 0, 0)
        .to_zoned(TimeZone::UTC)?
        .timestamp()
        .as_nanosecond();

    let ns_u64 = u64::try_from(unix_timestamp_ns)
        .map_err(|_| anyhow::anyhow!("Negative timestamp: {unix_timestamp_ns}"))?;
    Ok(UnixNanos::from(ns_u64))
}

/// Check whether the given UNIX nanoseconds timestamp is within the last 24 hours.
///
/// # Errors
///
/// Returns an error if the timestamp is invalid.
pub fn is_within_last_24_hours(timestamp_ns: UnixNanos) -> anyhow::Result<bool> {
    // Use the time seam so the comparison is deterministic under
    // `simulation` + `cfg(madsim)` and we avoid a wall-clock call that
    // would otherwise bypass the DST contract.
    let timestamp_ns = timestamp_ns.as_u64();
    let now_ns = nanos_since_unix_epoch();

    // Future timestamps are not within the last 24 hours
    if timestamp_ns > now_ns {
        return Ok(false);
    }

    Ok(now_ns - timestamp_ns <= NANOSECONDS_IN_DAY)
}

fn shift_months(datetime: Timestamp, months: i64) -> anyhow::Result<Timestamp> {
    let span = Span::new().try_months(months)?;
    let result = datetime.to_zoned(TimeZone::UTC).checked_add(span)?;
    Ok(result.timestamp())
}

/// Subtract `n` months from a Jiff [`Timestamp`].
///
/// # Errors
///
/// Returns an error if the resulting date would be invalid or out of range.
pub fn subtract_n_months(datetime: Timestamp, n: u32) -> anyhow::Result<Timestamp> {
    shift_months(datetime, -i64::from(n))
        .map_err(|_| anyhow::anyhow!("Failed to subtract {n} months from {datetime}"))
}

/// Add `n` months to a Jiff [`Timestamp`].
///
/// # Errors
///
/// Returns an error if the resulting date would be invalid or out of range.
pub fn add_n_months(datetime: Timestamp, n: u32) -> anyhow::Result<Timestamp> {
    shift_months(datetime, i64::from(n))
        .map_err(|_| anyhow::anyhow!("Failed to add {n} months to {datetime}"))
}

/// Subtract `n` months from a given UNIX nanoseconds timestamp.
///
/// # Errors
///
/// Returns an error if the resulting timestamp is out of range or invalid.
pub fn subtract_n_months_nanos(unix_nanos: UnixNanos, n: u32) -> anyhow::Result<UnixNanos> {
    let datetime = unix_nanos.to_datetime_utc();
    let result = subtract_n_months(datetime, n)?;
    let timestamp = result.as_nanosecond();

    let nanos =
        u64::try_from(timestamp).map_err(|_| anyhow::anyhow!("Negative timestamp not allowed"))?;
    Ok(UnixNanos::from(nanos))
}

/// Add `n` months to a given UNIX nanoseconds timestamp.
///
/// # Errors
///
/// Returns an error if the resulting timestamp is out of range or invalid.
pub fn add_n_months_nanos(unix_nanos: UnixNanos, n: u32) -> anyhow::Result<UnixNanos> {
    let datetime = unix_nanos.to_datetime_utc();
    let result = add_n_months(datetime, n)?;
    let timestamp = result.as_nanosecond();

    let nanos =
        u64::try_from(timestamp).map_err(|_| anyhow::anyhow!("Negative timestamp not allowed"))?;
    Ok(UnixNanos::from(nanos))
}

/// Add `n` years to a Jiff [`Timestamp`].
///
/// # Errors
///
/// Returns an error if the resulting date would be invalid or out of range.
pub fn add_n_years(datetime: Timestamp, n: u32) -> anyhow::Result<Timestamp> {
    let months = n.checked_mul(12).ok_or_else(|| {
        anyhow::anyhow!("Failed to add {n} years to {datetime}: month count overflow")
    })?;

    shift_months(datetime, i64::from(months))
        .map_err(|_| anyhow::anyhow!("Failed to add {n} years to {datetime}"))
}

/// Subtract `n` years from a Jiff [`Timestamp`].
///
/// # Errors
///
/// Returns an error if the resulting date would be invalid or out of range.
pub fn subtract_n_years(datetime: Timestamp, n: u32) -> anyhow::Result<Timestamp> {
    let months = n.checked_mul(12).ok_or_else(|| {
        anyhow::anyhow!("Failed to subtract {n} years from {datetime}: month count overflow")
    })?;

    shift_months(datetime, -i64::from(months))
        .map_err(|_| anyhow::anyhow!("Failed to subtract {n} years from {datetime}"))
}

/// Add `n` years to a given UNIX nanoseconds timestamp.
///
/// # Errors
///
/// Returns an error if the resulting timestamp is out of range or invalid.
pub fn add_n_years_nanos(unix_nanos: UnixNanos, n: u32) -> anyhow::Result<UnixNanos> {
    let datetime = unix_nanos.to_datetime_utc();
    let result = add_n_years(datetime, n)?;
    let timestamp = result.as_nanosecond();

    let nanos =
        u64::try_from(timestamp).map_err(|_| anyhow::anyhow!("Negative timestamp not allowed"))?;
    Ok(UnixNanos::from(nanos))
}

/// Subtract `n` years from a given UNIX nanoseconds timestamp.
///
/// # Errors
///
/// Returns an error if the resulting timestamp is out of range or invalid.
pub fn subtract_n_years_nanos(unix_nanos: UnixNanos, n: u32) -> anyhow::Result<UnixNanos> {
    let datetime = unix_nanos.to_datetime_utc();
    let result = subtract_n_years(datetime, n)?;
    let timestamp = result.as_nanosecond();

    let nanos =
        u64::try_from(timestamp).map_err(|_| anyhow::anyhow!("Negative timestamp not allowed"))?;
    Ok(UnixNanos::from(nanos))
}

/// Convert an optional [`Timestamp`] to an optional [`UnixNanos`] timestamp.
pub fn datetime_to_unix_nanos(value: Option<Timestamp>) -> Option<UnixNanos> {
    value
        .map(Timestamp::as_nanosecond)
        .and_then(|nanos| u64::try_from(nanos).ok())
        .map(UnixNanos::from)
}

/// Converts a `Timestamp` to `UnixNanos`.
///
/// Unlike `UnixNanos::from(Timestamp)` which panics, this returns an error.
///
/// # Errors
///
/// Returns an error if the timestamp is before the UNIX epoch or out of range for `UnixNanos`.
pub fn try_datetime_to_unix_nanos(value: Timestamp) -> anyhow::Result<UnixNanos> {
    let nanos = value.as_nanosecond();

    if nanos < 0 {
        anyhow::bail!("DateTime timestamp cannot be negative: {nanos}");
    }
    let nanos = u64::try_from(nanos)
        .map_err(|_| anyhow::anyhow!("DateTime timestamp out of range for UnixNanos: {nanos}"))?;

    Ok(UnixNanos::from(nanos))
}

#[cfg(test)]
// `allow` not `expect`: nightly clippy does not fire `float_cmp` inside `assert_eq!`
#[allow(
    clippy::float_cmp,
    reason = "Exact float comparisons acceptable in tests"
)]
mod tests {
    use jiff::SignedDuration;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn timestamp(value: &str) -> Timestamp {
        value.parse().unwrap()
    }

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
        let result = secs_to_nanos(value).unwrap();
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
        let result = secs_to_millis(value).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_secs_to_nanos_unchecked_matches_checked() {
        assert_eq!(secs_to_nanos_unchecked(1.1), secs_to_nanos(1.1).unwrap());
    }

    #[rstest]
    fn test_secs_to_nanos_non_finite_errors() {
        let err = secs_to_nanos(f64::NAN).unwrap_err();
        assert!(err.to_string().contains("finite"));
    }

    #[rstest]
    fn test_secs_to_millis_non_finite_errors() {
        let err = secs_to_millis(f64::INFINITY).unwrap_err();
        assert!(err.to_string().contains("finite"));
    }

    #[rstest]
    fn test_millis_to_nanos_non_finite_errors() {
        let err = millis_to_nanos(f64::NEG_INFINITY).unwrap_err();
        assert!(err.to_string().contains("finite"));
    }

    #[rstest]
    fn test_micros_to_nanos_non_finite_errors() {
        let err = micros_to_nanos(f64::NAN).unwrap_err();
        assert!(err.to_string().contains("finite"));
    }

    #[rstest]
    #[case(0, 0)]
    #[case(1, 60)]
    #[case(5, 300)]
    #[case(60, 3600)]
    #[case(1440, 86400)]
    fn test_mins_to_secs(#[case] mins: u64, #[case] expected: u64) {
        assert_eq!(mins_to_secs(mins), expected);
    }

    #[rstest]
    #[case(0, 0)]
    #[case(1, 60_000_000_000)]
    #[case(5, 300_000_000_000)]
    #[case(60, 3_600_000_000_000)]
    fn test_mins_to_nanos(#[case] mins: u64, #[case] expected: u64) {
        assert_eq!(mins_to_nanos(mins), expected);
    }

    #[rstest]
    #[case(
        checked_mins_to_secs,
        307_445_734_561_825_860,
        18_446_744_073_709_551_600
    )]
    #[case(checked_mins_to_nanos, 307_445_734, 18_446_744_040_000_000_000)]
    fn test_checked_minutes_conversion_boundary(
        #[case] convert: fn(u64) -> Option<u64>,
        #[case] max: u64,
        #[case] expected: u64,
    ) {
        assert_eq!(convert(max), Some(expected));
        assert_eq!(convert(max + 1), None);
    }

    #[rstest]
    #[should_panic(expected = "minutes to seconds conversion overflow")]
    fn test_mins_to_secs_overflow_panics() {
        let _ = mins_to_secs(307_445_734_561_825_861);
    }

    #[rstest]
    #[should_panic(expected = "minutes to nanoseconds conversion overflow")]
    fn test_mins_to_nanos_overflow_panics() {
        let _ = mins_to_nanos(307_445_735);
    }

    #[rstest]
    #[case(
        secs_to_nanos,
        18_446_744_073.709_553,
        18_446_744_073.709_55,
        18_446_744_073_709_549_568
    )]
    #[case(
        secs_to_millis,
        18_446_744_073_709_550.0,
        18_446_744_073_709_548.0,
        18_446_744_073_709_547_520
    )]
    #[case(
        millis_to_nanos,
        18_446_744_073_709.55,
        18_446_744_073_709.547,
        18_446_744_073_709_547_520
    )]
    #[case(
        micros_to_nanos,
        18_446_744_073_709_550.0,
        18_446_744_073_709_548.0,
        18_446_744_073_709_547_520
    )]
    fn test_float_conversion_u64_boundary(
        #[case] convert: fn(f64) -> anyhow::Result<u64>,
        #[case] invalid: f64,
        #[case] previous: f64,
        #[case] expected: u64,
    ) {
        let err = convert(invalid).unwrap_err();
        assert!(err.to_string().contains("out of range"));
        assert_eq!(convert(previous).unwrap(), expected);
    }

    #[rstest]
    fn test_secs_to_nanos_negative_infinity_errors() {
        let result = secs_to_nanos(f64::NEG_INFINITY);
        assert!(result.is_err());
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
        let result = millis_to_nanos(value).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_millis_to_nanos_unchecked_matches_checked() {
        assert_eq!(
            millis_to_nanos_unchecked(1.1),
            millis_to_nanos(1.1).unwrap()
        );
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
        let result = micros_to_nanos(value).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_micros_to_nanos_unchecked_matches_checked() {
        assert_eq!(
            micros_to_nanos_unchecked(1.1),
            micros_to_nanos(1.1).unwrap()
        );
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
    #[case(951_782_400_000_000_000, "2000-02-29T00:00:00.000000000Z")] // Leap day
    #[case(1_609_459_199_999_999_999, "2020-12-31T23:59:59.999999999Z")] // Year boundary
    #[case(1_702_857_600_000_000_000, "2023-12-18T00:00:00.000000000Z")] // Specific date
    fn test_unix_nanos_to_iso8601(#[case] nanos: u64, #[case] expected: &str) {
        let result = unix_nanos_to_iso8601(UnixNanos::from(nanos));
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(951_782_400_123_456_789)]
    #[case(1_609_459_199_999_999_999)]
    #[case(i64::MAX as u64)]
    fn test_unix_nanos_to_iso8601_matches_jiff_oracle(#[case] nanos: u64) {
        let expected = format!(
            "{:.9}",
            Timestamp::from_nanosecond(i128::from(nanos)).unwrap()
        );
        let result = unix_nanos_to_iso8601(UnixNanos::from(nanos));
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case((i64::MAX as u64) + 1)]
    #[case(u64::MAX)]
    fn test_unix_nanos_to_iso8601_supports_full_unix_nanos_range(#[case] nanos: u64) {
        let expected = format!(
            "{:.9}",
            Timestamp::from_nanosecond(i128::from(nanos)).unwrap()
        );
        let result = unix_nanos_to_iso8601(UnixNanos::from(nanos));
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0, "1970-01-01T00:00:00.000Z")] // Unix epoch
    #[case(1_000_000, "1970-01-01T00:00:00.001Z")] // 1 millisecond
    #[case(1_000_000_000, "1970-01-01T00:00:01.000Z")] // 1 second
    #[case(951_782_400_123_456_789, "2000-02-29T00:00:00.123Z")] // Leap day
    #[case(1_609_459_199_999_999_999, "2020-12-31T23:59:59.999Z")] // Year boundary
    #[case(1_702_857_600_123_456_789, "2023-12-18T00:00:00.123Z")] // With millisecond precision
    fn test_unix_nanos_to_iso8601_millis(#[case] nanos: u64, #[case] expected: &str) {
        let result = unix_nanos_to_iso8601_millis(UnixNanos::from(nanos));
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(0)]
    #[case(951_782_400_123_456_789)]
    #[case(1_609_459_199_999_999_999)]
    #[case(i64::MAX as u64)]
    fn test_unix_nanos_to_iso8601_millis_matches_jiff_oracle(#[case] nanos: u64) {
        let expected = format!(
            "{:.3}",
            Timestamp::from_nanosecond(i128::from(nanos)).unwrap()
        );
        let result = unix_nanos_to_iso8601_millis(UnixNanos::from(nanos));
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case((i64::MAX as u64) + 1)]
    #[case(u64::MAX)]
    fn test_unix_nanos_to_iso8601_millis_supports_full_unix_nanos_range(#[case] nanos: u64) {
        let expected = format!(
            "{:.3}",
            Timestamp::from_nanosecond(i128::from(nanos)).unwrap()
        );
        let result = unix_nanos_to_iso8601_millis(UnixNanos::from(nanos));
        assert_eq!(result, expected);
    }

    // Sweep the full representable range against Jiff, complementing the fixed-point oracle
    // cases above; any divergence in the integer date math surfaces as a mismatch here.
    proptest! {
        #[rstest]
        fn prop_unix_nanos_to_iso8601_matches_jiff(nanos in any::<u64>()) {
            let expected = format!(
                "{:.9}",
                Timestamp::from_nanosecond(i128::from(nanos)).unwrap(),
            );
            let actual = unix_nanos_to_iso8601(UnixNanos::from(nanos));
            prop_assert_eq!(actual, expected);
        }

        #[rstest]
        fn prop_unix_nanos_to_iso8601_millis_matches_jiff(nanos in any::<u64>()) {
            let expected = format!(
                "{:.3}",
                Timestamp::from_nanosecond(i128::from(nanos)).unwrap(),
            );
            let actual = unix_nanos_to_iso8601_millis(UnixNanos::from(nanos));
            prop_assert_eq!(actual, expected);
        }
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
        let now_ns = Timestamp::now().as_nanosecond();
        assert!(is_within_last_24_hours(UnixNanos::from(u64::try_from(now_ns).unwrap())).unwrap());
    }

    #[rstest]
    fn test_is_within_last_24_hours_when_two_days_ago() {
        let past_ns = (Timestamp::now() - SignedDuration::from_hours(48)).as_nanosecond();
        assert!(
            !is_within_last_24_hours(UnixNanos::from(u64::try_from(past_ns).unwrap())).unwrap()
        );
    }

    #[rstest]
    fn test_is_within_last_24_hours_when_future() {
        // Future timestamps should return false
        let future_ns = (Timestamp::now() + SignedDuration::from_hours(1)).as_nanosecond();
        assert!(
            !is_within_last_24_hours(UnixNanos::from(u64::try_from(future_ns).unwrap())).unwrap()
        );

        // One day in the future should also return false
        let future_ns = (Timestamp::now() + SignedDuration::from_hours(24)).as_nanosecond();
        assert!(
            !is_within_last_24_hours(UnixNanos::from(u64::try_from(future_ns).unwrap())).unwrap()
        );
    }

    #[rstest]
    #[case(
        timestamp("2024-03-31T12:00:00Z"),
        1,
        timestamp("2024-02-29T12:00:00Z")
    )]
    #[case(
        timestamp("2024-03-31T12:00:00Z"),
        12,
        timestamp("2023-03-31T12:00:00Z")
    )]
    #[case(
        timestamp("2024-01-31T12:00:00Z"),
        1,
        timestamp("2023-12-31T12:00:00Z")
    )]
    #[case(
        timestamp("2024-03-31T12:00:00Z"),
        2,
        timestamp("2024-01-31T12:00:00Z")
    )]
    fn test_subtract_n_months(
        #[case] input: Timestamp,
        #[case] months: u32,
        #[case] expected: Timestamp,
    ) {
        let result = subtract_n_months(input, months).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(
        timestamp("2023-02-28T12:00:00Z"),
        1,
        timestamp("2023-03-28T12:00:00Z")
    )]
    #[case(
        timestamp("2024-01-31T12:00:00Z"),
        1,
        timestamp("2024-02-29T12:00:00Z")
    )]
    #[case(
        timestamp("2023-12-31T12:00:00Z"),
        1,
        timestamp("2024-01-31T12:00:00Z")
    )]
    #[case(
        timestamp("2023-01-31T12:00:00Z"),
        13,
        timestamp("2024-02-29T12:00:00Z")
    )]
    fn test_add_n_months(
        #[case] input: Timestamp,
        #[case] months: u32,
        #[case] expected: Timestamp,
    ) {
        let result = add_n_months(input, months).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_add_n_years_overflow() {
        let datetime = timestamp("2024-01-01T00:00:00Z");
        let err = add_n_years(datetime, u32::MAX).unwrap_err();
        assert!(err.to_string().contains("month count overflow"));
    }

    #[rstest]
    fn test_subtract_n_years_overflow() {
        let datetime = timestamp("2024-01-01T00:00:00Z");
        let err = subtract_n_years(datetime, u32::MAX).unwrap_err();
        assert!(err.to_string().contains("month count overflow"));
    }

    #[rstest]
    fn test_add_n_years_nanos_overflow() {
        let nanos = UnixNanos::from(0);
        let err = add_n_years_nanos(nanos, u32::MAX).unwrap_err();
        assert!(err.to_string().contains("month count overflow"));
    }

    #[rstest]
    #[case("1970-01-01T00:00:00.000000000Z", 0)] // Unix epoch
    #[case("1970-01-01T00:00:00.000000001Z", 1)] // 1 nanosecond
    #[case("1970-01-01T00:00:00.001000000Z", 1_000_000)] // 1 millisecond
    #[case("1970-01-01T00:00:01.000000000Z", 1_000_000_000)] // 1 second
    #[case("2023-12-18T00:00:00.000000000Z", 1_702_857_600_000_000_000)] // Specific date
    #[case("2024-02-10T14:58:43.456789Z", 1_707_577_123_456_789_000)] // RFC3339 with fractions
    #[case("2024-02-10T14:58:43Z", 1_707_577_123_000_000_000)] // RFC3339 without fractions
    #[case("2024-02-10", 1_707_523_200_000_000_000)] // Simple date format
    fn test_iso8601_to_unix_nanos(#[case] input: &str, #[case] expected: u64) {
        let result = iso8601_to_unix_nanos(input).unwrap();
        assert_eq!(result.as_u64(), expected);
    }

    #[rstest]
    #[case("invalid-date")] // Invalid format
    #[case("2024-02-30")] // Invalid date
    #[case("2024-13-01")] // Invalid month
    #[case("not a timestamp")] // Random string
    fn test_iso8601_to_unix_nanos_invalid(#[case] input: &str) {
        let result = iso8601_to_unix_nanos(input);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_iso8601_roundtrip() {
        let original_nanos = UnixNanos::from(1_707_577_123_456_789_000);
        let iso8601_string = unix_nanos_to_iso8601(original_nanos);
        let parsed_nanos = iso8601_to_unix_nanos(&iso8601_string).unwrap();
        assert_eq!(parsed_nanos, original_nanos);
    }

    #[rstest]
    fn test_add_n_years_nanos_normal_case() {
        // Test adding 1 year from 2020-01-01
        let start = UnixNanos::from(timestamp("2020-01-01T00:00:00Z"));
        let result = add_n_years_nanos(start, 1).unwrap();
        let expected = UnixNanos::from(timestamp("2021-01-01T00:00:00Z"));
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_add_n_years_nanos_prevents_negative_timestamp() {
        // Edge case: ensure we catch if somehow a negative timestamp would be produced
        // This is a defensive check - in practice, adding years shouldn't produce negative
        // timestamps from valid UnixNanos, but we verify the check is in place
        let start = UnixNanos::from(0); // Epoch
        // Adding years to epoch should never produce negative, but the check is there
        let result = add_n_years_nanos(start, 1);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_datetime_to_unix_nanos_at_epoch() {
        // Unix epoch (1970-01-01 00:00:00 UTC) should return 0 nanoseconds
        let epoch = Timestamp::UNIX_EPOCH;
        let result = datetime_to_unix_nanos(Some(epoch));
        assert_eq!(result, Some(UnixNanos::from(0)));
    }

    #[rstest]
    fn test_datetime_to_unix_nanos_typical_datetime() {
        let dt = timestamp("2024-01-15T13:30:45.123456789Z");
        let result = datetime_to_unix_nanos(Some(dt));

        // Expected: 1705325445123456789 nanoseconds
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_u64(), 1_705_325_445_123_456_789);
    }

    #[rstest]
    fn test_datetime_to_unix_nanos_before_epoch() {
        // Pre-epoch datetime (1969-12-31 23:59:59 UTC) should return None
        // because negative timestamps can't be converted to u64
        let before_epoch = timestamp("1969-12-31T23:59:59Z");
        let result = datetime_to_unix_nanos(Some(before_epoch));
        assert_eq!(result, None);
    }

    #[rstest]
    fn test_datetime_to_unix_nanos_one_second_after_epoch() {
        // 1970-01-01 00:00:01 UTC = 1_000_000_000 nanoseconds
        let dt = Timestamp::from_second(1).unwrap();
        let result = datetime_to_unix_nanos(Some(dt));
        assert_eq!(result, Some(UnixNanos::from(1_000_000_000)));
    }

    #[rstest]
    fn test_datetime_to_unix_nanos_with_subsecond_precision() {
        // Test with microseconds: 1970-01-01 00:00:00.000001 UTC
        let dt = Timestamp::new(0, 1_000).unwrap(); // 1 microsecond = 1000 nanos
        let result = datetime_to_unix_nanos(Some(dt));
        assert_eq!(result, Some(UnixNanos::from(1_000)));
    }

    #[rstest]
    fn test_try_datetime_to_unix_nanos_valid() {
        let dt = Timestamp::new(0, 1_000).unwrap();
        assert_eq!(
            try_datetime_to_unix_nanos(dt).unwrap(),
            UnixNanos::from(1_000)
        );
    }

    #[rstest]
    fn test_try_datetime_to_unix_nanos_before_epoch_errors() {
        let before_epoch = timestamp("1969-12-31T23:59:59Z");
        let err = try_datetime_to_unix_nanos(before_epoch).unwrap_err();
        assert!(
            err.to_string().contains("cannot be negative"),
            "unexpected error: {err}"
        );
    }

    #[rstest]
    fn test_try_datetime_to_unix_nanos_out_of_range_errors() {
        let err = try_datetime_to_unix_nanos(Timestamp::MAX).unwrap_err();
        assert!(
            err.to_string().contains("out of range"),
            "unexpected error: {err}"
        );
    }

    #[rstest]
    fn test_nanos_helpers_support_values_above_i64_max() {
        let large = UnixNanos::from(u64::MAX);
        assert!(subtract_n_months_nanos(large, 1).is_ok());
        assert!(add_n_months_nanos(large, 1).is_err());
        assert!(add_n_years_nanos(large, 1).is_err());
        assert!(subtract_n_years_nanos(large, 1).is_ok());
    }

    #[rstest]
    fn test_subtract_n_months_nanos_pre_epoch_result_errors() {
        let epoch = UnixNanos::from(0);
        let err = subtract_n_months_nanos(epoch, 1).unwrap_err();
        assert_eq!(err.to_string(), "Negative timestamp not allowed");
    }

    #[rstest]
    fn test_subtract_n_years_nanos_pre_epoch_result_errors() {
        let epoch = UnixNanos::from(0);
        let err = subtract_n_years_nanos(epoch, 1).unwrap_err();
        assert_eq!(err.to_string(), "Negative timestamp not allowed");
    }

    #[rstest]
    fn test_subtract_n_months_nanos_at_epoch_boundary() {
        let epoch = UnixNanos::from(0);
        assert_eq!(subtract_n_months_nanos(epoch, 0).unwrap(), epoch);
    }
}

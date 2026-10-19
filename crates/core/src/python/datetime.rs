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

//! Date/time utility wrappers exposed to Python.

use pyo3::prelude::*;
use pyo3_stub_gen::derive::gen_stub_pyfunction;

use super::to_pyvalue_err;
use crate::{
    UnixNanos,
    datetime::{
        is_within_last_24_hours, last_weekday_nanos, micros_to_nanos, millis_to_nanos,
        nanos_to_micros, nanos_to_millis, nanos_to_secs, secs_to_millis, secs_to_nanos,
        unix_nanos_to_iso8601, unix_nanos_to_iso8601_millis,
    },
};

/// Converts seconds to nanoseconds (ns).
///
/// # Errors
///
/// Returns an error if `secs` is non-finite or cannot be represented as `u64` nanoseconds.
#[pyfunction(name = "secs_to_nanos")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_secs_to_nanos(secs: f64) -> PyResult<u64> {
    secs_to_nanos(secs).map_err(to_pyvalue_err)
}

/// Converts seconds to milliseconds (ms).
///
/// # Errors
///
/// Returns an error if `secs` is non-finite or cannot be represented as `u64` milliseconds.
#[pyfunction(name = "secs_to_millis")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_secs_to_millis(secs: f64) -> PyResult<u64> {
    secs_to_millis(secs).map_err(to_pyvalue_err)
}

/// Converts milliseconds (ms) to nanoseconds (ns).
///
/// Casting f64 to u64 by truncating the fractional part is intentional for unit conversion,
/// which may lose precision and drop negative values after clamping.
///
/// # Errors
///
/// Returns an error if `millis` is non-finite or cannot be represented as `u64` nanoseconds.
#[pyfunction(name = "millis_to_nanos")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_millis_to_nanos(millis: f64) -> PyResult<u64> {
    millis_to_nanos(millis).map_err(to_pyvalue_err)
}

/// Converts microseconds (μs) to nanoseconds (ns).
///
/// Casting f64 to u64 by truncating the fractional part is intentional for unit conversion,
/// which may lose precision and drop negative values after clamping.
///
/// # Errors
///
/// Returns an error if `micros` is non-finite or cannot be represented as `u64` nanoseconds.
#[pyfunction(name = "micros_to_nanos")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_micros_to_nanos(micros: f64) -> PyResult<u64> {
    micros_to_nanos(micros).map_err(to_pyvalue_err)
}

/// Converts nanoseconds (ns) to seconds.
///
/// Casting u64 to f64 may lose precision for large values,
/// but is acceptable when computing fractional seconds.
#[must_use]
#[pyfunction(name = "nanos_to_secs")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_nanos_to_secs(nanos: u64) -> f64 {
    nanos_to_secs(nanos)
}

/// Converts nanoseconds (ns) to milliseconds (ms).
#[must_use]
#[pyfunction(name = "nanos_to_millis")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub const fn py_nanos_to_millis(nanos: u64) -> u64 {
    nanos_to_millis(nanos)
}

/// Converts nanoseconds (ns) to microseconds (μs).
#[must_use]
#[pyfunction(name = "nanos_to_micros")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub const fn py_nanos_to_micros(nanos: u64) -> u64 {
    nanos_to_micros(nanos)
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format string.
///
/// All `UnixNanos` values are representable by this formatter.
#[pyfunction(
    name = "unix_nanos_to_iso8601",
    signature = (timestamp_ns, nanos_precision=Some(true))
)]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_unix_nanos_to_iso8601(
    timestamp_ns: u64,
    nanos_precision: Option<bool>,
) -> PyResult<String> {
    let unix_nanos = UnixNanos::from(timestamp_ns);
    let formatted = if nanos_precision.unwrap_or(true) {
        unix_nanos_to_iso8601(unix_nanos)
    } else {
        unix_nanos_to_iso8601_millis(unix_nanos)
    };

    Ok(formatted)
}

/// Calculates the last weekday (Mon-Fri) from the given `year`, `month`, and `day`.
///
/// # Errors
///
/// Returns an error if the date is invalid.
#[pyfunction(name = "last_weekday_nanos")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_last_weekday_nanos(year: i32, month: u32, day: u32) -> PyResult<u64> {
    Ok(last_weekday_nanos(year, month, day)
        .map_err(to_pyvalue_err)?
        .as_u64())
}

/// Check whether the given UNIX nanoseconds timestamp is within the last 24 hours.
///
/// # Errors
///
/// Returns an error if the timestamp is invalid.
#[pyfunction(name = "is_within_last_24_hours")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_is_within_last_24_hours(timestamp_ns: u64) -> PyResult<bool> {
    is_within_last_24_hours(UnixNanos::from(timestamp_ns)).map_err(to_pyvalue_err)
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use pyo3::ffi::c_str;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Some(true), 30)]
    #[case(Some(false), 24)]
    fn test_py_unix_nanos_to_iso8601_supports_full_range(
        #[case] nanos_precision: Option<bool>,
        #[case] expected_len: usize,
    ) {
        let result = py_unix_nanos_to_iso8601(u64::MAX, nanos_precision);
        assert_eq!(result.unwrap().len(), expected_len);
    }

    #[rstest]
    fn test_py_unix_nanos_to_iso8601_formats_valid_timestamp() {
        let output = py_unix_nanos_to_iso8601(0, Some(false)).unwrap();
        assert_eq!(output, "1970-01-01T00:00:00.000Z");
    }

    #[rstest]
    fn test_jiff_timestamp_python_input_accepts_aware_non_utc_datetime() {
        Python::initialize();
        Python::attach(|py| {
            let datetime = py
                .eval(
                    c_str!(
                        "__import__('datetime').datetime(2024, 1, 15, 13, 30, 45, tzinfo=__import__('datetime').timezone(__import__('datetime').timedelta(hours=-5)))"
                    ),
                    None,
                    None,
                )
                .unwrap();

            let timestamp = datetime.extract::<Timestamp>().unwrap();
            assert_eq!(timestamp, "2024-01-15T18:30:45Z".parse().unwrap());
        });
    }

    #[rstest]
    fn test_jiff_timestamp_python_input_accepts_zoneinfo_datetime() {
        Python::initialize();
        Python::attach(|py| {
            let datetime = py
                .eval(
                    c_str!(
                        "__import__('datetime').datetime(2024, 1, 15, 13, 30, 45, tzinfo=__import__('zoneinfo').ZoneInfo('America/New_York'))"
                    ),
                    None,
                    None,
                )
                .unwrap();

            let timestamp = datetime.extract::<Timestamp>().unwrap();
            assert_eq!(timestamp, "2024-01-15T18:30:45Z".parse().unwrap());
        });
    }

    #[rstest]
    fn test_jiff_timestamp_python_input_rejects_naive_datetime() {
        Python::initialize();
        Python::attach(|py| {
            let datetime = py
                .eval(
                    c_str!("__import__('datetime').datetime(2024, 1, 15, 13, 30, 45)"),
                    None,
                    None,
                )
                .unwrap();

            assert!(datetime.extract::<Timestamp>().is_err());
        });
    }
}

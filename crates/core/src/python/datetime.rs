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

use pyo3::prelude::*;

use super::to_pyvalue_err;
use crate::{
    UnixNanos,
    datetime::{
        is_within_last_24_hours, last_weekday_nanos, micros_to_nanos, millis_to_nanos,
        nanos_to_micros, nanos_to_millis, nanos_to_secs, secs_to_millis, secs_to_nanos,
        unix_nanos_to_iso8601, unix_nanos_to_iso8601_millis,
    },
};

/// Return round nanoseconds (ns) converted from the given seconds.
///
/// Parameters
/// ----------
/// secs : float
///     The seconds to convert.
///
/// Returns
/// -------
/// int
#[must_use]
#[pyfunction(name = "secs_to_nanos")]
pub fn py_secs_to_nanos(secs: f64) -> u64 {
    secs_to_nanos(secs)
}

/// Return round milliseconds (ms) converted from the given seconds.
///
/// Parameters
/// ----------
/// secs : float
///     The seconds to convert.
///
/// Returns
/// -------
/// int
#[must_use]
#[pyfunction(name = "secs_to_millis")]
pub fn py_secs_to_millis(secs: f64) -> u64 {
    secs_to_millis(secs)
}

/// Return round nanoseconds (ns) converted from the given milliseconds (ms).
///
/// Parameters
/// ----------
/// millis : float
///     The milliseconds to convert.
///
/// Returns
/// -------
/// int
#[must_use]
#[pyfunction(name = "millis_to_nanos")]
pub fn py_millis_to_nanos(millis: f64) -> u64 {
    millis_to_nanos(millis)
}

/// Return round nanoseconds (ns) converted from the given microseconds (μs).
///
/// Parameters
/// ----------
/// micros : float
///     The microseconds to convert.
///
/// Returns
/// -------
/// int
#[must_use]
#[pyfunction(name = "micros_to_nanos")]
pub fn py_micros_to_nanos(micros: f64) -> u64 {
    micros_to_nanos(micros)
}

/// Return seconds converted from the given nanoseconds (ns).
///
/// Parameters
/// ----------
/// nanos : int
///     The nanoseconds to convert.
///
/// Returns
/// -------
/// float
#[must_use]
#[pyfunction(name = "nanos_to_secs")]
pub fn py_nanos_to_secs(nanos: u64) -> f64 {
    nanos_to_secs(nanos)
}

/// Return round milliseconds (ms) converted from the given nanoseconds (ns).
///
/// Parameters
/// ----------
/// nanos : int
///     The nanoseconds to convert.
///
/// Returns
/// -------
/// int
#[must_use]
#[pyfunction(name = "nanos_to_millis")]
pub const fn py_nanos_to_millis(nanos: u64) -> u64 {
    nanos_to_millis(nanos)
}

/// Return round microseconds (μs) converted from the given nanoseconds (ns).
///
/// Parameters
/// ----------
/// nanos : int
///     The nanoseconds to convert.
///
/// Returns
/// -------
/// int
#[must_use]
#[pyfunction(name = "nanos_to_micros")]
pub const fn py_nanos_to_micros(nanos: u64) -> u64 {
    nanos_to_micros(nanos)
}

/// Return UNIX nanoseconds as an ISO 8601 (RFC 3339) format string.
///
/// Parameters
/// ----------
/// timestamp_ns : int
///     The UNIX timestamp (nanoseconds).
/// nanos_precision : bool, default True
///     If True, use nanosecond precision. If False, use millisecond precision.
///
/// Returns
/// -------
/// str
///
/// Raises
/// ------
/// ValueError
///     If `timestamp_ns` is invalid.
#[must_use]
#[pyfunction(name = "unix_nanos_to_iso8601", signature = (timestamp_ns, nanos_precision=true))]
pub fn py_unix_nanos_to_iso8601(timestamp_ns: u64, nanos_precision: Option<bool>) -> String {
    let unix_nanos = timestamp_ns.into();
    if nanos_precision.unwrap_or(true) {
        unix_nanos_to_iso8601(unix_nanos)
    } else {
        unix_nanos_to_iso8601_millis(unix_nanos)
    }
}

/// Return UNIX nanoseconds at midnight (UTC) of the last weekday (Mon-Fri).
///
/// Parameters
/// ----------
/// year : int
///     The year from the datum date.
/// month : int
///     The month from the datum date.
/// day : int
///     The day from the datum date.
///
/// Returns
/// -------
/// int
///
/// Raises
/// ------
/// ValueError
///     If given an invalid date.
#[pyfunction(name = "last_weekday_nanos")]
#[allow(clippy::missing_errors_doc)]
pub fn py_last_weekday_nanos(year: i32, month: u32, day: u32) -> PyResult<u64> {
    Ok(last_weekday_nanos(year, month, day)
        .map_err(to_pyvalue_err)?
        .as_u64())
}

/// Return whether the given UNIX nanoseconds timestamp is within the last 24 hours.
///
/// Parameters
/// ----------
/// timestamp_ns : int
///     The UNIX nanoseconds timestamp datum.
///
/// Returns
/// -------
/// bool
///
/// Raises
/// ------
/// ValueError
///     If `timestamp` is invalid.
#[pyfunction(name = "is_within_last_24_hours")]
#[allow(clippy::missing_errors_doc)]
pub fn py_is_within_last_24_hours(timestamp_ns: u64) -> PyResult<bool> {
    is_within_last_24_hours(UnixNanos::from(timestamp_ns)).map_err(to_pyvalue_err)
}

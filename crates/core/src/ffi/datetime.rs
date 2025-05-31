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

use std::ffi::c_char;

use crate::{
    datetime::{unix_nanos_to_iso8601, unix_nanos_to_iso8601_millis},
    ffi::string::str_to_cstr,
};

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format C string pointer.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn unix_nanos_to_iso8601_cstr(timestamp_ns: u64) -> *const c_char {
    str_to_cstr(&unix_nanos_to_iso8601(timestamp_ns.into()))
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format C string pointer
/// with millisecond precision.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn unix_nanos_to_iso8601_millis_cstr(timestamp_ns: u64) -> *const c_char {
    str_to_cstr(&unix_nanos_to_iso8601_millis(timestamp_ns.into()))
}
/// Converts seconds to nanoseconds (ns).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn secs_to_nanos(secs: f64) -> u64 {
    crate::datetime::secs_to_nanos(secs)
}

/// Converts seconds to milliseconds (ms).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn secs_to_millis(secs: f64) -> u64 {
    crate::datetime::secs_to_millis(secs)
}

/// Converts milliseconds (ms) to nanoseconds (ns).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn millis_to_nanos(millis: f64) -> u64 {
    crate::datetime::millis_to_nanos(millis)
}

/// Converts microseconds (μs) to nanoseconds (ns).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn micros_to_nanos(micros: f64) -> u64 {
    crate::datetime::micros_to_nanos(micros)
}

/// Converts nanoseconds (ns) to seconds.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn nanos_to_secs(nanos: u64) -> f64 {
    crate::datetime::nanos_to_secs(nanos)
}

/// Converts nanoseconds (ns) to milliseconds (ms).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub const extern "C" fn nanos_to_millis(nanos: u64) -> u64 {
    crate::datetime::nanos_to_millis(nanos)
}

/// Converts nanoseconds (ns) to microseconds (μs).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub const extern "C" fn nanos_to_micros(nanos: u64) -> u64 {
    crate::datetime::nanos_to_micros(nanos)
}

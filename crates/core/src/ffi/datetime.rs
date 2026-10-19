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

//! Thin FFI wrappers around the date/time conversion utilities in `nautilus-core`.
//!
//! The Rust implementation lives in `crate::datetime`; this module exposes the conversions to C.
//! Each exported function forwards directly to its Rust counterpart and inherits the same semantics
//! and safety guarantees.

use std::ffi::c_char;

use crate::{
    datetime::{unix_nanos_to_iso8601, unix_nanos_to_iso8601_millis},
    ffi::{abort_on_panic, string::str_to_cstr},
};

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format C string pointer.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn unix_nanos_to_iso8601_cstr(timestamp_ns: u64) -> *const c_char {
    abort_on_panic(|| str_to_cstr(&unix_nanos_to_iso8601(timestamp_ns.into())))
}

/// Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) format C string pointer
/// with millisecond precision.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn unix_nanos_to_iso8601_millis_cstr(timestamp_ns: u64) -> *const c_char {
    abort_on_panic(|| str_to_cstr(&unix_nanos_to_iso8601_millis(timestamp_ns.into())))
}

/// Converts seconds to nanoseconds (ns).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn secs_to_nanos(secs: f64) -> u64 {
    abort_on_panic(|| crate::datetime::secs_to_nanos_unchecked(secs))
}

/// Converts seconds to milliseconds (ms).
///
/// # Panics
///
/// Panics if [`crate::datetime::secs_to_millis`] returns an error for `secs`.
/// The panic is caught by [`abort_on_panic`] and converted into a process abort
/// across the FFI boundary.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn secs_to_millis(secs: f64) -> u64 {
    abort_on_panic(|| {
        crate::datetime::secs_to_millis(secs).expect("secs_to_millis: invalid or overflowing input")
    })
}

/// Converts milliseconds (ms) to nanoseconds (ns).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn millis_to_nanos(millis: f64) -> u64 {
    abort_on_panic(|| crate::datetime::millis_to_nanos_unchecked(millis))
}

/// Converts microseconds (μs) to nanoseconds (ns).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn micros_to_nanos(micros: f64) -> u64 {
    abort_on_panic(|| crate::datetime::micros_to_nanos_unchecked(micros))
}

/// Converts nanoseconds (ns) to seconds.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn nanos_to_secs(nanos: u64) -> f64 {
    abort_on_panic(|| crate::datetime::nanos_to_secs(nanos))
}

/// Converts nanoseconds (ns) to milliseconds (ms).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn nanos_to_millis(nanos: u64) -> u64 {
    abort_on_panic(|| crate::datetime::nanos_to_millis(nanos))
}

/// Converts nanoseconds (ns) to microseconds (μs).
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn nanos_to_micros(nanos: u64) -> u64 {
    abort_on_panic(|| crate::datetime::nanos_to_micros(nanos))
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use rstest::rstest;

    use super::*;
    use crate::ffi::string::cstr_drop;

    #[rstest]
    fn test_unix_nanos_to_iso8601_cstr_conversions() {
        let timestamp_ns = 1_702_857_600_123_456_789;

        let nanos = take_cstr(unix_nanos_to_iso8601_cstr(timestamp_ns));
        let millis = take_cstr(unix_nanos_to_iso8601_millis_cstr(timestamp_ns));

        assert_eq!(nanos, "2023-12-18T00:00:00.123456789Z");
        assert_eq!(millis, "2023-12-18T00:00:00.123Z");
    }

    #[rstest]
    fn test_time_unit_conversions() {
        assert_eq!(secs_to_nanos(1.25), 1_250_000_000);
        assert_eq!(secs_to_millis(1.25), 1_250);
        assert_eq!(millis_to_nanos(1.25), 1_250_000);
        assert_eq!(micros_to_nanos(1.25), 1_250);
        assert_eq!(
            nanos_to_secs(42_897_123_111).to_bits(),
            42.897_123_111_f64.to_bits()
        );
        assert_eq!(nanos_to_millis(1_234_567_890), 1_234);
        assert_eq!(nanos_to_micros(1_234_567_890), 1_234_567);
    }

    fn take_cstr(ptr: *const c_char) -> String {
        // SAFETY: The FFI conversion functions return valid pointers allocated by `str_to_cstr`
        let value = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        // SAFETY: The pointer was allocated by `str_to_cstr` and has not been freed
        unsafe { cstr_drop(ptr) };
        value
    }
}

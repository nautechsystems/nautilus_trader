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

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

use nautilus_core::ffi::string::cstr_as_str;

use crate::{identifiers::Venue, venues::VENUE_MAP};

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn venue_new(ptr: *const c_char) -> Venue {
    let value = unsafe { cstr_as_str(ptr) };
    Venue::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn venue_hash(id: &Venue) -> u64 {
    id.inner().precomputed_hash()
}

#[unsafe(no_mangle)]
pub extern "C" fn venue_is_synthetic(venue: &Venue) -> u8 {
    u8::from(venue.is_synthetic())
}

/// # Safety
///
/// - Assumes `code_ptr` is borrowed from a valid Python UTF-8 `str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn venue_code_exists(code_ptr: *const c_char) -> u8 {
    let code = unsafe { cstr_as_str(code_ptr) };
    u8::from(VENUE_MAP.lock().unwrap().contains_key(code))
}

/// # Safety
///
/// - Assumes `code_ptr` is borrowed from a valid Python UTF-8 `str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn venue_from_cstr_code(code_ptr: *const c_char) -> Venue {
    let code = unsafe { cstr_as_str(code_ptr) };
    Venue::from_code(code).unwrap()
}

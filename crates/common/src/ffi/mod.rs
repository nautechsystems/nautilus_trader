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

//! C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).

#![allow(unsafe_code)]

use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_to_bytes;

use crate::msgbus::matching::is_matching;

pub mod clock;
pub mod enums;
pub mod logging;
pub mod timer;

/// Match a topic and a string pattern using iterative backtracking algorithm
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
///
/// # Safety
///
/// Passing `NULL` pointers will result in a panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn is_matching_ffi(topic: *const c_char, pattern: *const c_char) -> u8 {
    let topic = unsafe { cstr_to_bytes(topic) };
    let pattern = unsafe { cstr_to_bytes(pattern) };
    is_matching(topic, pattern) as u8
}

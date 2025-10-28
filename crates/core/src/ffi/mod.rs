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
//!
//! All exported functions route through `abort_on_panic` so that any panic inside the
//! Rust implementation aborts immediately instead of unwinding across the foreign boundary.
//! Unwinding into C/Python is undefined behaviour, so this keeps the existing fail-fast
//! semantics while avoiding subtle stack corruption during debugging.

#![allow(unsafe_code)]
#![allow(unsafe_attr_outside_unsafe)]

pub mod cvec;
pub mod datetime;
pub mod parsing;
pub mod string;
pub mod uuid;

use std::{
    panic::{self, AssertUnwindSafe},
    process,
};

/// Executes `f`, aborting the process if it panics.
///
/// FFI exports always call this helper so a panic never unwinds across the
/// `extern "C"` boundary. Unwinding into C/Python is undefined behaviour and
/// can silently corrupt the foreign stack; aborting instead preserves the
/// fail-fast guarantee with effectively no debugging downside (the panic
/// message is still logged before the abort).
#[inline]
pub(crate) fn abort_on_panic<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => process::abort(),
    }
}

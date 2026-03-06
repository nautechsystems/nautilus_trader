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
pub fn abort_on_panic<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => process::abort(),
    }
}

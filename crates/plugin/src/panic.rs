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

//! Catch-unwind wrapper used by every plug-in `extern "C"` thunk.
//!
//! Unwinding across an FFI boundary is undefined behaviour, so every host-bound
//! call from a plug-in must be wrapped to convert a panic into a returned
//! [`PluginError`] with code [`PluginErrorCode::Panic`].

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::boundary::{PluginError, PluginErrorCode, PluginResult};

/// Wraps a closure in `catch_unwind` and maps a panic to a `PluginError`.
///
/// Macro-generated thunks call this so plug-in panics surface as errors instead
/// of unwinding through the FFI.
pub fn guard<T>(f: impl FnOnce() -> Result<T, PluginError>) -> PluginResult<T> {
    let result = catch_unwind(AssertUnwindSafe(f));
    match result {
        Ok(Ok(t)) => PluginResult::Ok(t),
        Ok(Err(e)) => PluginResult::Err(e),
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            drop_payload(payload);
            PluginResult::Err(PluginError::new(PluginErrorCode::Panic, message))
        }
    }
}

/// Runs a closure under `catch_unwind` for thunks whose return type cannot
/// carry a `PluginError` (e.g. `extern "C" fn(...) -> u64`).
///
/// On panic, logs the message and aborts the process. Aborting is the only
/// sound option once a panic reaches this point: returning a sentinel would
/// silently corrupt downstream computation, and unwinding across the FFI
/// boundary is undefined behaviour.
pub fn guard_infallible<T>(thunk_name: &str, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(t) => t,
        Err(payload) => {
            let msg = panic_message(payload.as_ref());
            drop_payload(payload);
            log::error!(
                target: "nautilus_plugin",
                "plug-in panicked in `{thunk_name}` thunk; aborting process: {msg}",
            );
            std::process::abort();
        }
    }
}

/// Drops a panic payload while suppressing any unwind from its `Drop` impl.
///
/// `std::panic::catch_unwind` catches the original panic, but if the payload
/// itself panics on drop the second panic unwinds the caller. For an
/// `extern "C"` thunk that is undefined behaviour. Wrapping the drop in
/// another `catch_unwind` keeps the surface around the FFI boundary
/// unwind-free even with adversarial payloads (e.g. `panic_any(T)` where
/// `T: Drop` panics).
pub fn drop_payload(payload: Box<dyn std::any::Any + Send>) {
    let _ = catch_unwind(AssertUnwindSafe(move || drop(payload)));
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "plug-in panicked with non-string payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn returns_ok_on_success() {
        let r = guard(|| Ok::<u32, PluginError>(7));
        assert_eq!(r.into_result().unwrap(), 7);
    }

    #[rstest]
    fn returns_err_on_returned_error() {
        let r = guard(|| Err::<u32, _>(PluginError::generic("boom")));
        let e = r.into_result().unwrap_err();
        assert_eq!(e.code, PluginErrorCode::Generic);
        assert_eq!(e.message_string(), "boom");
    }

    #[rstest]
    fn returns_err_on_string_panic() {
        let r = guard(|| -> Result<u32, PluginError> { panic!("oops") });
        let e = r.into_result().unwrap_err();
        assert_eq!(e.code, PluginErrorCode::Panic);
        assert!(e.message_string().contains("oops"));
    }

    #[rstest]
    fn returns_err_on_non_string_panic() {
        let r = guard(|| -> Result<u32, PluginError> {
            std::panic::panic_any(42_u32);
        });
        let e = r.into_result().unwrap_err();
        assert_eq!(e.code, PluginErrorCode::Panic);
        assert!(e.message_string().contains("non-string"));
    }

    #[rstest]
    fn guard_infallible_returns_inner_on_success() {
        let v = guard_infallible("test", || 42u64);
        assert_eq!(v, 42);
    }

    #[rstest]
    fn drop_payload_swallows_panicking_drop() {
        // `drop_payload` runs the payload Drop inside an inner `catch_unwind`
        // so a panicking Drop does not propagate out of the function. This
        // test asserts the call returns normally even when the payload
        // panics on drop.
        use std::{
            any::Any,
            sync::atomic::{AtomicUsize, Ordering},
        };

        static DROPS_OBSERVED: AtomicUsize = AtomicUsize::new(0);
        struct Bomb;
        impl Drop for Bomb {
            fn drop(&mut self) {
                DROPS_OBSERVED.fetch_add(1, Ordering::SeqCst);
                panic!("drop panic");
            }
        }
        DROPS_OBSERVED.store(0, Ordering::SeqCst);

        let payload: Box<dyn Any + Send> = Box::new(Bomb);
        drop_payload(payload);
        assert_eq!(DROPS_OBSERVED.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    fn guard_survives_panic_any_with_panicking_drop() {
        // Regression: a panic payload whose Drop also panics must not unwind
        // past `catch_unwind`. `drop_payload` wraps the payload drop in a
        // second `catch_unwind`; without it the second panic aborts the host
        // or causes UB in the `extern "C"` thunk.
        static DROPS_OBSERVED: AtomicUsize = AtomicUsize::new(0);
        struct Bomb;
        impl Drop for Bomb {
            fn drop(&mut self) {
                DROPS_OBSERVED.fetch_add(1, Ordering::SeqCst);
                panic!("drop panic");
            }
        }

        DROPS_OBSERVED.store(0, Ordering::SeqCst);
        let r = guard(|| -> Result<u32, PluginError> {
            std::panic::panic_any(Bomb);
        });
        let e = r.into_result().unwrap_err();
        assert_eq!(e.code, PluginErrorCode::Panic);

        // Drop ran inside the inner catch_unwind; observed exactly once
        assert_eq!(DROPS_OBSERVED.load(Ordering::SeqCst), 1);
    }
}

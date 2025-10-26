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

use std::{
    collections::HashMap,
    ffi::c_char,
    sync::{Mutex, OnceLock},
};

#[cfg(feature = "python")]
use nautilus_core::python::clone_py_object;
use nautilus_core::{
    MUTEX_POISONED, UUID4,
    ffi::string::{cstr_to_ustr, str_to_cstr},
};
#[cfg(feature = "python")]
use pyo3::prelude::*;

use crate::timer::{TimeEvent, TimeEventCallback, TimeEventHandlerV2};

#[repr(C)]
#[derive(Debug)]
/// Legacy time event handler for Cython/FFI inter-operatbility
///
/// TODO: Remove once Cython is deprecated
///
/// `TimeEventHandler` associates a `TimeEvent` with a callback function that is triggered
/// when the event's timestamp is reached.
pub struct TimeEventHandler {
    /// The time event.
    pub event: TimeEvent,
    /// The callable raw pointer.
    pub callback_ptr: *mut c_char,
}

// -----------------------------------------------------------------------------
// Internal registry that owns the Python callback objects
// -----------------------------------------------------------------------------
//
// The legacy `TimeEventHandler` handed to Cython stores only a borrowed
// `Py<PyAny>*` (`callback_ptr`).  To make sure the pointed-to Python object
// stays alive while *any* handler referencing it exists we keep a single
// `Arc<Py<PyAny>>` per raw pointer in this registry together with a manual
// reference counter.
//
// Why a registry instead of extra fields:
//   • The C ABI must remain `struct { TimeEvent, char * }` – adding bytes to
//     the struct would break all generated headers.
//   • `Arc<Py<..>>` guarantees GIL-safe INC/DEC but cannot be represented in
//     C.  Storing it externally preserves layout while retaining safety.
//
// Drop strategy:
//   1. Cloning a handler increments the per-pointer counter.
//   2. Dropping a handler decrements it; if the count hits zero we remove the
//      entry *then* release the `Arc` under `Python::attach`.
//      The drop happens *outside* the mutex guard to avoid dead-locking when
//      Python finalisers re-enter the registry.
//
// This design removes all manual INCREF/DECREF on `callback_ptr`, eliminates
// leaks, and is safe on any thread.

#[cfg(feature = "python")]
type CallbackEntry = (Py<PyAny>, usize); // (object, ref_count)

#[cfg(feature = "python")]
fn registry() -> &'static Mutex<HashMap<usize, CallbackEntry>> {
    static REG: OnceLock<Mutex<HashMap<usize, CallbackEntry>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

// Helper to obtain the registry lock, tolerant to poisoning so Drop cannot panic
#[cfg(feature = "python")]
fn registry_lock() -> std::sync::MutexGuard<'static, HashMap<usize, CallbackEntry>> {
    match registry().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(feature = "python")]
pub fn registry_size() -> usize {
    registry_lock().len()
}

#[cfg(feature = "python")]
pub fn cleanup_callback_registry() {
    // Drain entries while locked, then drop callbacks with the GIL outside
    let callbacks: Vec<Py<PyAny>> = {
        let mut map = registry_lock();
        map.drain().map(|(_, (obj, _))| obj).collect()
    };

    Python::attach(|_| {
        for cb in callbacks {
            drop(cb);
        }
    });
}

// Legacy conversion from TimeEventHandlerV2 to pure-C TimeEventHandler
// Only supports Python callbacks; available when `python` feature is enabled
#[cfg(feature = "python")]
impl From<TimeEventHandlerV2> for TimeEventHandler {
    /// # Panics
    ///
    /// Panics if the provided `TimeEventHandlerV2` contains a Rust callback,
    /// since only Python callbacks are supported by the legacy `TimeEventHandler`.
    fn from(value: TimeEventHandlerV2) -> Self {
        match value.callback {
            TimeEventCallback::Python(callback_arc) => {
                let raw_ptr = callback_arc.as_ptr().cast::<c_char>();

                // Keep an explicit ref-count per raw pointer in the registry.
                let key = raw_ptr as usize;
                let mut map = registry_lock();
                match map.entry(key) {
                    std::collections::hash_map::Entry::Occupied(mut e) => {
                        e.get_mut().1 += 1;
                    }
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert((clone_py_object(&callback_arc), 1));
                    }
                }

                Self {
                    event: value.event,
                    callback_ptr: raw_ptr,
                }
            }
            TimeEventCallback::Rust(_) => {
                panic!("Legacy time event handler is not supported for Rust callbacks")
            }
        }
    }
}

// Remove the callback from the registry when the last handler using the raw
// pointer is about to disappear.  We only drop the Arc if its strong count is
// 1 (i.e. this handler owns the final reference).  Dropping happens while
// holding the GIL so it is always safe.

#[cfg(feature = "python")]
impl Drop for TimeEventHandler {
    fn drop(&mut self) {
        if self.callback_ptr.is_null() {
            return;
        }

        let key = self.callback_ptr as usize;
        let mut map = registry().lock().expect(MUTEX_POISONED);
        if let Some(entry) = map.get_mut(&key) {
            if entry.1 > 1 {
                entry.1 -= 1;
                return;
            }
            // This was the final handler – remove entry and drop Arc under GIL
            let (arc, _) = map.remove(&key).unwrap();
            Python::attach(|_| drop(arc));
        }
    }
}

impl Clone for TimeEventHandler {
    fn clone(&self) -> Self {
        #[cfg(feature = "python")]
        {
            if !self.callback_ptr.is_null() {
                let key = self.callback_ptr as usize;
                let mut map = registry_lock();
                if let Some(entry) = map.get_mut(&key) {
                    entry.1 += 1;
                }
            }
        }

        Self {
            event: self.event.clone(),
            callback_ptr: self.callback_ptr,
        }
    }
}

#[cfg(not(feature = "python"))]
impl Drop for TimeEventHandler {
    fn drop(&mut self) {}
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use nautilus_core::UUID4;
    use pyo3::{Py, Python, types::PyList};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::timer::{TimeEvent, TimeEventCallback};

    #[rstest]
    fn registry_clears_after_handler_drop() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let callback = TimeEventCallback::from(Py::from(py_list.getattr("append").unwrap()));

            let handler_v2 = TimeEventHandlerV2::new(
                TimeEvent::new(Ustr::from("TEST"), UUID4::new(), 1.into(), 1.into()),
                callback,
            );

            // Wrap in block so handler drops before we assert size
            {
                let _legacy: TimeEventHandler = handler_v2.into();
                assert_eq!(registry_size(), 1);
            }

            // After drop registry should be empty
            assert_eq!(registry_size(), 0);
        });
    }
}

// Fallback conversion for non-Python callbacks: Rust callbacks only
#[cfg(not(feature = "python"))]
impl From<TimeEventHandlerV2> for TimeEventHandler {
    fn from(value: TimeEventHandlerV2) -> Self {
        // Only Rust callbacks are supported in non-python builds
        match value.callback {
            TimeEventCallback::Rust(_) => TimeEventHandler {
                event: value.event,
                callback_ptr: std::ptr::null_mut(),
            },
            #[cfg(feature = "python")]
            TimeEventCallback::Python(_) => {
                unreachable!("Python callback not supported without python feature")
            }
        }
    }
}

/// # Safety
///
/// Assumes `name_ptr` is borrowed from a valid Python UTF-8 `str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn time_event_new(
    name_ptr: *const c_char,
    event_id: UUID4,
    ts_event: u64,
    ts_init: u64,
) -> TimeEvent {
    TimeEvent::new(
        unsafe { cstr_to_ustr(name_ptr) },
        event_id,
        ts_event.into(),
        ts_init.into(),
    )
}

/// Returns a [`TimeEvent`] as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn time_event_to_cstr(event: &TimeEvent) -> *const c_char {
    str_to_cstr(&event.to_string())
}

// This function only exists so that `TimeEventHandler` is included in the definitions
#[unsafe(no_mangle)]
pub const extern "C" fn dummy(v: TimeEventHandler) -> TimeEventHandler {
    v
}

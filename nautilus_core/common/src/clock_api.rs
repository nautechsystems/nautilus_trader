// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    ffi::c_char,
    ops::{Deref, DerefMut},
};

use nautilus_core::{
    ffi::{cvec::CVec, string::cstr_to_string},
    time::UnixNanos,
};
use pyo3::{
    ffi,
    prelude::*,
    types::{PyList, PyString},
    AsPyPointer,
};

use crate::{
    clock::{Clock, LiveClock, TestClock},
    timer::{TimeEvent, TimeEventHandler},
};

/// Provides a C compatible Foreign Function Interface (FFI) for an underlying [`TestClock`].
///
/// This struct wraps `TestClock` in a way that makes it compatible with C function
/// calls, enabling interaction with `TestClock` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `TestClock_API` to be
/// dereferenced to `TestClock`, providing access to `TestClock`'s methods without
/// having to manually access the underlying `TestClock` instance.
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct TestClock_API(Box<TestClock>);

impl Deref for TestClock_API {
    type Target = TestClock;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TestClock_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[no_mangle]
pub extern "C" fn test_clock_new() -> TestClock_API {
    TestClock_API(Box::new(TestClock::new()))
}

#[no_mangle]
pub extern "C" fn test_clock_drop(clock: TestClock_API) {
    drop(clock); // Memory freed here
}

/// # Safety
/// - Assumes `callback_ptr` is a valid `PyCallable` pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_register_default_handler(
    clock: &mut TestClock_API,
    callback_ptr: *mut ffi::PyObject,
) {
    assert!(!callback_ptr.is_null());
    assert!(ffi::Py_None() != callback_ptr);

    let callback_py = Python::with_gil(|py| PyObject::from_borrowed_ptr(py, callback_ptr));
    clock.register_default_handler_py(callback_py);
}

#[no_mangle]
pub extern "C" fn test_clock_set_time(clock: &mut TestClock_API, to_time_ns: u64) {
    clock.set_time(to_time_ns);
}

#[no_mangle]
pub extern "C" fn test_clock_timestamp(clock: &mut TestClock_API) -> f64 {
    clock.timestamp()
}

#[no_mangle]
pub extern "C" fn test_clock_timestamp_ms(clock: &mut TestClock_API) -> u64 {
    clock.timestamp_ms()
}

#[no_mangle]
pub extern "C" fn test_clock_timestamp_us(clock: &mut TestClock_API) -> u64 {
    clock.timestamp_us()
}

#[no_mangle]
pub extern "C" fn test_clock_timestamp_ns(clock: &mut TestClock_API) -> u64 {
    clock.timestamp_ns()
}

#[no_mangle]
pub extern "C" fn test_clock_timer_names(clock: &TestClock_API) -> *mut ffi::PyObject {
    Python::with_gil(|py| -> Py<PyList> {
        let names: Vec<Py<PyString>> = clock
            .get_timers()
            .keys()
            .map(|k| PyString::new(py, k).into())
            .collect();
        PyList::new(py, names).into()
    })
    .as_ptr()
}

#[no_mangle]
pub extern "C" fn test_clock_timer_count(clock: &mut TestClock_API) -> usize {
    clock.timer_count()
}

/// # Safety
///
/// - Assumes `name_ptr` is a valid C string pointer.
/// - Assumes `callback_ptr` is a valid `PyCallable` pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_set_time_alert_ns(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
    alert_time_ns: UnixNanos,
    callback_ptr: *mut ffi::PyObject,
) {
    assert!(!callback_ptr.is_null());

    let name = cstr_to_string(name_ptr);
    let callback_py = Python::with_gil(|py| match callback_ptr {
        ptr if ptr != ffi::Py_None() => Some(PyObject::from_borrowed_ptr(py, ptr)),
        _ => None,
    });
    clock.set_time_alert_ns_py(name, alert_time_ns, callback_py);
}

/// # Safety
///
/// - Assumes `name_ptr` is a valid C string pointer.
/// - Assumes `callback_ptr` is a valid `PyCallable` pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_set_timer_ns(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
    interval_ns: u64,
    start_time_ns: UnixNanos,
    stop_time_ns: UnixNanos,
    callback_ptr: *mut ffi::PyObject,
) {
    assert!(!callback_ptr.is_null());

    let name = cstr_to_string(name_ptr);
    let stop_time_ns = match stop_time_ns {
        0 => None,
        _ => Some(stop_time_ns),
    };
    let callback_py = Python::with_gil(|py| match callback_ptr {
        ptr if ptr != ffi::Py_None() => Some(PyObject::from_borrowed_ptr(py, ptr)),
        _ => None,
    });
    clock.set_timer_ns_py(name, interval_ns, start_time_ns, stop_time_ns, callback_py);
}

/// # Safety
///
/// - Assumes `set_time` is a correct `uint8_t` of either 0 or 1.
#[no_mangle]
pub unsafe extern "C" fn test_clock_advance_time(
    clock: &mut TestClock_API,
    to_time_ns: u64,
    set_time: u8,
) -> CVec {
    let events: Vec<TimeEvent> = clock.advance_time(to_time_ns, set_time != 0);
    clock.match_handlers_py(events).into()
}

// TODO: This struct implementation potentially leaks memory
// TODO: Skip clippy check for now since it requires large modification
#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn vec_time_event_handlers_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<TimeEventHandler> =
        unsafe { Vec::from_raw_parts(ptr.cast::<TimeEventHandler>(), len, cap) };
    drop(data); // Memory freed here
}

/// # Safety
///
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_next_time_ns(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
) -> UnixNanos {
    let name = cstr_to_string(name_ptr);
    clock.next_time_ns(&name)
}

/// # Safety
///
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_cancel_timer(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
) {
    let name = cstr_to_string(name_ptr);
    clock.cancel_timer(&name);
}

#[no_mangle]
pub extern "C" fn test_clock_cancel_timers(clock: &mut TestClock_API) {
    clock.cancel_timers();
}

/// Provides a C compatible Foreign Function Interface (FFI) for an underlying [`LiveClock`].
///
/// This struct wraps `LiveClock` in a way that makes it compatible with C function
/// calls, enabling interaction with `LiveClock` in a C environment.
///
/// It implements the `Deref` and `DerefMut` traits, allowing instances of `LiveClock_API` to be
/// dereferenced to `LiveClock`, providing access to `LiveClock`'s methods without
/// having to manually access the underlying `LiveClock` instance. This includes
/// both mutable and immutable access.
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct LiveClock_API(Box<LiveClock>);

impl Deref for LiveClock_API {
    type Target = LiveClock;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LiveClock_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[no_mangle]
pub extern "C" fn live_clock_new() -> LiveClock_API {
    LiveClock_API(Box::new(LiveClock::new()))
}

#[no_mangle]
pub extern "C" fn live_clock_drop(clock: LiveClock_API) {
    drop(clock); // Memory freed here
}

#[no_mangle]
pub extern "C" fn live_clock_timestamp(clock: &mut LiveClock_API) -> f64 {
    clock.timestamp()
}

#[no_mangle]
pub extern "C" fn live_clock_timestamp_ms(clock: &mut LiveClock_API) -> u64 {
    clock.timestamp_ms()
}

#[no_mangle]
pub extern "C" fn live_clock_timestamp_us(clock: &mut LiveClock_API) -> u64 {
    clock.timestamp_us()
}

#[no_mangle]
pub extern "C" fn live_clock_timestamp_ns(clock: &mut LiveClock_API) -> u64 {
    clock.timestamp_ns()
}

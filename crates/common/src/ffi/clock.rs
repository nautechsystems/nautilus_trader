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
    ffi::c_char,
    ops::{Deref, DerefMut},
};

use nautilus_core::{
    UnixNanos,
    correctness::FAILED,
    ffi::{
        cvec::CVec,
        parsing::u8_as_bool,
        string::{cstr_as_str, str_to_cstr},
    },
};
#[cfg(feature = "python")]
use pyo3::{ffi, prelude::*};

use super::timer::TimeEventHandler;
use crate::{
    clock::{Clock, LiveClock, TestClock},
    timer::{TimeEvent, TimeEventCallback},
};

/// C compatible Foreign Function Interface (FFI) for an underlying [`TestClock`].
///
/// This struct wraps `TestClock` in a way that makes it compatible with C function
/// calls, enabling interaction with `TestClock` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `TestClock_API` to be
/// dereferenced to `TestClock`, providing access to `TestClock`'s methods without
/// having to manually access the underlying `TestClock` instance.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Debug)]
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

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_new() -> TestClock_API {
    TestClock_API(Box::default())
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_drop(clock: TestClock_API) {
    drop(clock); // Memory freed here
}

/// Registers the default callback handler for TestClock.
///
/// # Safety
///
/// Assumes `callback_ptr` is a valid `PyCallable` pointer.
///
/// # Panics
///
/// Panics if the `callback_ptr` is null or represents the Python `None` object.
#[cfg(feature = "python")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_clock_register_default_handler(
    clock: &mut TestClock_API,
    callback_ptr: *mut ffi::PyObject,
) {
    assert!(!callback_ptr.is_null());
    assert!(unsafe { ffi::Py_None() } != callback_ptr);

    let callback = Python::attach(|py| unsafe { Py::<PyAny>::from_borrowed_ptr(py, callback_ptr) });
    let callback = TimeEventCallback::from(callback);

    clock.register_default_handler(callback);
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_set_time(clock: &TestClock_API, to_time_ns: u64) {
    clock.set_time(to_time_ns.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_timestamp(clock: &TestClock_API) -> f64 {
    clock.get_time()
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_timestamp_ms(clock: &TestClock_API) -> u64 {
    clock.get_time_ms()
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_timestamp_us(clock: &TestClock_API) -> u64 {
    clock.get_time_us()
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_timestamp_ns(clock: &TestClock_API) -> u64 {
    clock.get_time_ns().as_u64()
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_timer_names(clock: &TestClock_API) -> *const c_char {
    // For simplicity we join a string with a reasonably unique delimiter.
    // This is a temporary solution pending the removal of Cython.
    str_to_cstr(&clock.timer_names().join("<,>"))
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_timer_count(clock: &mut TestClock_API) -> usize {
    clock.timer_count()
}

/// # Safety
///
/// This function assumes:
/// - `name_ptr` is a valid C string pointer.
/// - `callback_ptr` is a valid `PyCallable` pointer.
///
/// # Panics
///
/// Panics if `callback_ptr` is null or if setting the timer fails.
#[cfg(feature = "python")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_clock_set_time_alert(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
    alert_time_ns: UnixNanos,
    callback_ptr: *mut ffi::PyObject,
    allow_past: u8,
) {
    assert!(!callback_ptr.is_null());

    let name = unsafe { cstr_as_str(name_ptr) };
    let callback = if callback_ptr == unsafe { ffi::Py_None() } {
        None
    } else {
        let callback =
            Python::attach(|py| unsafe { Py::<PyAny>::from_borrowed_ptr(py, callback_ptr) });
        Some(TimeEventCallback::from(callback))
    };

    clock
        .set_time_alert_ns(name, alert_time_ns, callback, Some(allow_past != 0))
        .expect(FAILED);
}

/// # Safety
///
/// This function assumes:
/// - `name_ptr` is a valid C string pointer.
/// - `callback_ptr` is a valid `PyCallable` pointer.
///
/// # Parameters
///
/// - `start_time_ns`: UNIX timestamp in nanoseconds. Use `0` to indicate "use current time".
/// - `stop_time_ns`: UNIX timestamp in nanoseconds. Use `0` to indicate "no stop time".
///
/// # Panics
///
/// Panics if `callback_ptr` is null or represents the Python `None` object.
#[cfg(feature = "python")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_clock_set_timer(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
    interval_ns: u64,
    start_time_ns: UnixNanos,
    stop_time_ns: UnixNanos,
    callback_ptr: *mut ffi::PyObject,
    allow_past: u8,
    fire_immediately: u8,
) {
    assert!(!callback_ptr.is_null());

    let name = unsafe { cstr_as_str(name_ptr) };
    // C API convention: 0 means None (use defaults)
    let start_time_ns = (start_time_ns != 0).then_some(start_time_ns);
    let stop_time_ns = (stop_time_ns != 0).then_some(stop_time_ns);
    let callback = if callback_ptr == unsafe { ffi::Py_None() } {
        None
    } else {
        let callback =
            Python::attach(|py| unsafe { Py::<PyAny>::from_borrowed_ptr(py, callback_ptr) });
        Some(TimeEventCallback::from(callback))
    };

    clock
        .set_timer_ns(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            callback,
            Some(allow_past != 0),
            Some(fire_immediately != 0),
        )
        .expect(FAILED);
}

/// # Safety
///
/// Assumes `set_time` is a correct `uint8_t` of either 0 or 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_clock_advance_time(
    clock: &mut TestClock_API,
    to_time_ns: u64,
    set_time: u8,
) -> CVec {
    let events: Vec<TimeEvent> = clock.advance_time(to_time_ns.into(), u8_as_bool(set_time));
    let t: Vec<TimeEventHandler> = clock
        .match_handlers(events)
        .into_iter()
        .map(Into::into)
        .collect();
    t.into()
}

// TODO: This drop helper may leak Python callbacks when handlers own Python objects.
//       We need to mirror the `ffi::timer` registry so reference counts are decremented properly.
#[allow(clippy::drop_non_drop)]
#[unsafe(no_mangle)]
pub extern "C" fn vec_time_event_handlers_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<TimeEventHandler> =
        unsafe { Vec::from_raw_parts(ptr.cast::<TimeEventHandler>(), len, cap) };
    drop(data); // Memory freed here
}

/// # Safety
///
/// Assumes `name_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_clock_next_time(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
) -> UnixNanos {
    let name = unsafe { cstr_as_str(name_ptr) };
    clock.next_time_ns(name).unwrap_or_default()
}

/// # Safety
///
/// Assumes `name_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_clock_cancel_timer(
    clock: &mut TestClock_API,
    name_ptr: *const c_char,
) {
    let name = unsafe { cstr_as_str(name_ptr) };
    clock.cancel_timer(name);
}

#[unsafe(no_mangle)]
pub extern "C" fn test_clock_cancel_timers(clock: &mut TestClock_API) {
    clock.cancel_timers();
}

/// C compatible Foreign Function Interface (FFI) for an underlying [`LiveClock`].
///
/// This struct wraps `LiveClock` in a way that makes it compatible with C function
/// calls, enabling interaction with `LiveClock` in a C environment.
///
/// It implements the `Deref` and `DerefMut` traits, allowing instances of `LiveClock_API` to be
/// dereferenced to `LiveClock`, providing access to `LiveClock`'s methods without
/// having to manually access the underlying `LiveClock` instance. This includes
/// both mutable and immutable access.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Debug)]
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

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_new() -> LiveClock_API {
    // Initialize a live clock without a time event sender
    LiveClock_API(Box::new(LiveClock::new(None)))
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_drop(clock: LiveClock_API) {
    drop(clock); // Memory freed here
}

/// # Safety
///
/// Assumes `callback_ptr` is a valid `PyCallable` pointer.
///
/// # Panics
///
/// Panics if `callback_ptr` is null or represents the Python `None` object.
#[cfg(feature = "python")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn live_clock_register_default_handler(
    clock: &mut LiveClock_API,
    callback_ptr: *mut ffi::PyObject,
) {
    assert!(!callback_ptr.is_null());
    assert!(unsafe { ffi::Py_None() } != callback_ptr);

    let callback = Python::attach(|py| unsafe { Py::<PyAny>::from_borrowed_ptr(py, callback_ptr) });
    let callback = TimeEventCallback::from(callback);

    clock.register_default_handler(callback);
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_timestamp(clock: &mut LiveClock_API) -> f64 {
    clock.get_time()
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_timestamp_ms(clock: &mut LiveClock_API) -> u64 {
    clock.get_time_ms()
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_timestamp_us(clock: &mut LiveClock_API) -> u64 {
    clock.get_time_us()
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_timestamp_ns(clock: &mut LiveClock_API) -> u64 {
    clock.get_time_ns().as_u64()
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_timer_names(clock: &LiveClock_API) -> *const c_char {
    // For simplicity we join a string with a reasonably unique delimiter.
    // This is a temporary solution pending the removal of Cython.
    str_to_cstr(&clock.timer_names().join("<,>"))
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_timer_count(clock: &mut LiveClock_API) -> usize {
    clock.timer_count()
}

/// # Safety
///
/// This function assumes:
/// - `name_ptr` is a valid C string pointer.
/// - `callback_ptr` is a valid `PyCallable` pointer.
///
/// # Panics
///
/// This function panics if:
/// - `name` is not a valid string.
/// - `callback_ptr` is NULL and no default callback has been assigned on the clock.
#[cfg(feature = "python")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn live_clock_set_time_alert(
    clock: &mut LiveClock_API,
    name_ptr: *const c_char,
    alert_time_ns: UnixNanos,
    callback_ptr: *mut ffi::PyObject,
    allow_past: u8,
) {
    assert!(!callback_ptr.is_null());

    let name = unsafe { cstr_as_str(name_ptr) };
    let callback = if callback_ptr == unsafe { ffi::Py_None() } {
        None
    } else {
        let callback =
            Python::attach(|py| unsafe { Py::<PyAny>::from_borrowed_ptr(py, callback_ptr) });
        Some(TimeEventCallback::from(callback))
    };

    clock
        .set_time_alert_ns(name, alert_time_ns, callback, Some(allow_past != 0))
        .expect(FAILED);
}

/// # Safety
///
/// This function assumes:
/// - `name_ptr` is a valid C string pointer.
/// - `callback_ptr` is a valid `PyCallable` pointer.
///
/// # Parameters
///
/// - `start_time_ns`: UNIX timestamp in nanoseconds. Use `0` to indicate "use current time".
/// - `stop_time_ns`: UNIX timestamp in nanoseconds. Use `0` to indicate "no stop time".
///
/// # Panics
///
/// This function panics if:
/// - `name` is not a valid string.
/// - `callback_ptr` is NULL and no default callback has been assigned on the clock.
#[cfg(feature = "python")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn live_clock_set_timer(
    clock: &mut LiveClock_API,
    name_ptr: *const c_char,
    interval_ns: u64,
    start_time_ns: UnixNanos,
    stop_time_ns: UnixNanos,
    callback_ptr: *mut ffi::PyObject,
    allow_past: u8,
    fire_immediately: u8,
) {
    assert!(!callback_ptr.is_null());

    let name = unsafe { cstr_as_str(name_ptr) };
    // C API convention: 0 means None (use defaults)
    let start_time_ns = (start_time_ns != 0).then_some(start_time_ns);
    let stop_time_ns = (stop_time_ns != 0).then_some(stop_time_ns);
    let callback = if callback_ptr == unsafe { ffi::Py_None() } {
        None
    } else {
        let callback =
            Python::attach(|py| unsafe { Py::<PyAny>::from_borrowed_ptr(py, callback_ptr) });
        Some(TimeEventCallback::from(callback))
    };

    clock
        .set_timer_ns(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            callback,
            Some(allow_past != 0),
            Some(fire_immediately != 0),
        )
        .expect(FAILED);
}

/// # Safety
///
/// Assumes `name_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn live_clock_next_time(
    clock: &mut LiveClock_API,
    name_ptr: *const c_char,
) -> UnixNanos {
    let name = unsafe { cstr_as_str(name_ptr) };
    clock.next_time_ns(name).unwrap_or_default()
}

/// # Safety
///
/// Assumes `name_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn live_clock_cancel_timer(
    clock: &mut LiveClock_API,
    name_ptr: *const c_char,
) {
    let name = unsafe { cstr_as_str(name_ptr) };
    clock.cancel_timer(name);
}

#[unsafe(no_mangle)]
pub extern "C" fn live_clock_cancel_timers(clock: &mut LiveClock_API) {
    clock.cancel_timers();
}

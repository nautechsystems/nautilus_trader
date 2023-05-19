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

use std::collections::HashMap;
use std::ffi::c_char;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

use nautilus_core::correctness;
use nautilus_core::cvec::CVec;
use nautilus_core::datetime::{nanos_to_micros, nanos_to_millis, nanos_to_secs};
use nautilus_core::string::cstr_to_string;
use nautilus_core::time::{duration_since_unix_epoch, UnixNanos};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use pyo3::{ffi, AsPyPointer};

use crate::timer::{TestTimer, TimeEvent, TimeEventHandler};

const ONE_NANOSECOND_DURATION: Duration = Duration::from_nanos(1);

pub struct MonotonicClock {
    /// The last recorded duration value from the clock.
    last: Duration,
}

/// Provides a monotonic clock.
///
/// Always produces unique and monotonically increasing timestamps.
impl MonotonicClock {
    /// Returns a monotonic `Duration` since the UNIX epoch, ensuring that the returned value is
    /// always greater than the previously returned value.
    fn monotonic_duration_since_unix_epoch(&mut self) -> Duration {
        let now = duration_since_unix_epoch();
        let output = if now <= self.last {
            self.last + ONE_NANOSECOND_DURATION
        } else {
            now
        };
        self.last = output;
        output
    }

    /// Initializes a new `MonotonicClock` instance.
    #[must_use]
    pub fn new() -> Self {
        MonotonicClock {
            last: duration_since_unix_epoch(),
        }
    }

    /// Returns the current seconds since the UNIX epoch (unique and monotonic).
    pub fn unix_timestamp_secs(&mut self) -> f64 {
        self.monotonic_duration_since_unix_epoch().as_secs_f64()
    }

    /// Returns the current milliseconds since the UNIX epoch (unique and monotonic).
    pub fn unix_timestamp_millis(&mut self) -> u64 {
        self.monotonic_duration_since_unix_epoch().as_millis() as u64
    }

    /// Returns the current microseconds since the UNIX epoch (unique and monotonic).
    pub fn unix_timestamp_micros(&mut self) -> u64 {
        self.monotonic_duration_since_unix_epoch().as_micros() as u64
    }

    /// Returns the current nanoseconds since the UNIX epoch (unique and monotonic).
    pub fn unix_timestamp_nanos(&mut self) -> u64 {
        self.monotonic_duration_since_unix_epoch().as_nanos() as u64
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        MonotonicClock::new()
    }
}

/// Represents a type of clock.
///
/// # Notes
/// An active timer is one which has not expired (`timer.is_expired == False`).
trait Clock {
    /// Return a new [Clock].
    fn new() -> Self;

    /// Return the current UNIX time in seconds.
    fn timestamp(&mut self) -> f64;

    /// Return the current UNIX time in milliseconds (ms).
    fn timestamp_ms(&mut self) -> u64;

    /// Return the current UNIX time in microseconds (us).
    fn timestamp_us(&mut self) -> u64;

    /// Return the current UNIX time in nanoseconds (ns).
    fn timestamp_ns(&mut self) -> u64;

    /// Return the names of active timers in the clock.
    fn timer_names(&self) -> Vec<&str>;

    /// Return the count of active timers in the clock.
    fn timer_count(&self) -> usize;

    /// Register a default event handler for the clock. If a [Timer]
    /// does not have an event handler, then this handler is used.
    fn register_default_handler(&mut self, callback: Box<dyn Fn(TimeEvent)>);

    fn register_default_handler_py(&mut self, callback_py: PyObject);

    /// Set a [Timer] to alert at a particular time. Optional
    /// callback gets used to handle generated events.
    fn set_time_alert_ns_py(
        &mut self,
        name: String,
        alert_time_ns: UnixNanos,
        callback_py: Option<PyObject>,
    );

    /// Set a [Timer] to start alerting at every interval
    /// between start and stop time. Optional callback gets
    /// used to handle generated event.
    fn set_timer_ns_py(
        &mut self,
        name: String,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback_py: Option<PyObject>,
    );

    fn next_time_ns(&mut self, name: &str) -> UnixNanos;
    fn cancel_timer(&mut self, name: &str);
    fn cancel_timers(&mut self);
}

pub struct TestClock {
    time_ns: UnixNanos,
    timers: HashMap<String, TestTimer>,
    default_callback: Option<Box<dyn Fn(TimeEvent)>>,
    default_callback_py: Option<PyObject>,
    _callbacks: HashMap<String, Box<dyn Fn(TimeEvent)>>,
    callbacks_py: HashMap<String, PyObject>,
}

impl TestClock {
    #[allow(dead_code)] // Temporary
    fn set_time(&mut self, to_time_ns: UnixNanos) {
        self.time_ns = to_time_ns
    }

    pub fn advance_time(&mut self, to_time_ns: UnixNanos, set_time: bool) -> Vec<TimeEvent> {
        // Time should increase monotonically
        assert!(
            to_time_ns >= self.time_ns,
            "`to_time_ns` was < `self._time_ns`"
        );

        if set_time {
            self.time_ns = to_time_ns;
        }

        let mut timers: Vec<TimeEvent> = self
            .timers
            .iter_mut()
            .filter(|(_, timer)| !timer.is_expired)
            .flat_map(|(_, timer)| timer.advance(to_time_ns))
            .collect();

        timers.sort_by(|a, b| a.ts_event.cmp(&b.ts_event));
        timers
    }

    /// Assumes time events are sorted by their `ts_event`.
    pub fn match_handlers_py(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandler> {
        events
            .into_iter()
            .map(|event| {
                let callback_py = self
                    .callbacks_py
                    .get(event.name.as_str())
                    .cloned()
                    .unwrap_or_else(|| {
                        // If callback_py is None, use the default_callback_py
                        // TODO: clone for now
                        self.default_callback_py.clone().unwrap()
                    });
                TimeEventHandler {
                    event,
                    callback_ptr: callback_py.as_ptr(),
                }
            })
            .collect()
    }
}

impl Clock for TestClock {
    fn new() -> TestClock {
        TestClock {
            time_ns: 0,
            timers: HashMap::new(),
            default_callback: None,
            default_callback_py: None,
            _callbacks: HashMap::new(), // TBC
            callbacks_py: HashMap::new(),
        }
    }

    fn timestamp(&mut self) -> f64 {
        nanos_to_secs(self.time_ns)
    }

    fn timestamp_ms(&mut self) -> u64 {
        nanos_to_millis(self.time_ns)
    }

    fn timestamp_us(&mut self) -> u64 {
        nanos_to_micros(self.time_ns)
    }

    fn timestamp_ns(&mut self) -> u64 {
        self.time_ns
    }

    fn timer_names(&self) -> Vec<&str> {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    fn timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired)
            .count()
    }

    fn register_default_handler(&mut self, callback: Box<dyn Fn(TimeEvent)>) {
        self.default_callback = Some(callback);
    }

    fn register_default_handler_py(&mut self, callback_py: PyObject) {
        self.default_callback_py = Some(callback_py)
    }

    fn set_time_alert_ns_py(
        &mut self,
        name: String,
        alert_time_ns: UnixNanos,
        callback_py: Option<PyObject>,
    ) {
        correctness::valid_string(&name, "`Timer` name");
        assert!(
            callback_py.is_some() | self.default_callback_py.is_some(),
            "All Python callbacks were `None`"
        );

        match callback_py {
            Some(callback_py) => self.callbacks_py.insert(name.clone(), callback_py),
            None => None,
        };

        let timer = TestTimer::new(
            name.clone(),
            alert_time_ns - self.time_ns,
            self.time_ns,
            Some(alert_time_ns),
        );
        self.timers.insert(name, timer);
    }

    fn set_timer_ns_py(
        &mut self,
        name: String,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback_py: Option<PyObject>,
    ) {
        correctness::valid_string(&name, "`Timer` name");
        assert!(
            callback_py.is_some() | self.default_callback_py.is_some(),
            "All Python callbacks were `None`"
        );

        match callback_py {
            Some(callback_py) => self.callbacks_py.insert(name.clone(), callback_py),
            None => None,
        };

        let timer = TestTimer::new(name.clone(), interval_ns, start_time_ns, stop_time_ns);
        self.timers.insert(name, timer);
    }

    fn next_time_ns(&mut self, name: &str) -> UnixNanos {
        let timer = self.timers.get(name);
        match timer {
            None => 0,
            Some(timer) => timer.next_time_ns,
        }
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(name);
        match timer {
            None => {}
            Some(mut timer) => timer.cancel(),
        }
    }

    fn cancel_timers(&mut self) {
        for (_, timer) in self.timers.iter_mut() {
            timer.cancel()
        }
        self.timers = HashMap::new();
    }
}

pub struct LiveClock {
    internal: MonotonicClock,
    timers: HashMap<String, TestTimer>,
    default_callback: Option<Box<dyn Fn(TimeEvent)>>,
    default_callback_py: Option<PyObject>,
    _callbacks: HashMap<String, Box<dyn Fn(TimeEvent)>>, // TBC
    callbacks_py: HashMap<String, PyObject>,
}

impl Clock for LiveClock {
    fn new() -> LiveClock {
        LiveClock {
            internal: MonotonicClock::default(),
            timers: HashMap::new(),
            default_callback: None,
            default_callback_py: None,
            _callbacks: HashMap::new(),
            callbacks_py: HashMap::new(),
        }
    }

    fn timestamp(&mut self) -> f64 {
        self.internal.unix_timestamp_secs()
    }

    fn timestamp_ms(&mut self) -> u64 {
        self.internal.unix_timestamp_millis()
    }

    fn timestamp_us(&mut self) -> u64 {
        self.internal.unix_timestamp_micros()
    }

    fn timestamp_ns(&mut self) -> u64 {
        self.internal.unix_timestamp_nanos()
    }

    fn timer_names(&self) -> Vec<&str> {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    fn timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired)
            .count()
    }

    fn register_default_handler(&mut self, handler: Box<dyn Fn(TimeEvent)>) {
        self.default_callback = Some(handler);
    }

    fn register_default_handler_py(&mut self, callback_py: PyObject) {
        self.default_callback_py = Some(callback_py)
    }

    fn set_time_alert_ns_py(
        &mut self,
        name: String,
        mut alert_time_ns: UnixNanos,
        callback_py: Option<PyObject>,
    ) {
        correctness::valid_string(&name, "`Timer` name");
        assert!(
            callback_py.is_some() | self.default_callback_py.is_some(),
            "All Python callbacks were `None`"
        );

        match callback_py {
            Some(callback_py) => self.callbacks_py.insert(name.clone(), callback_py),
            None => None,
        };

        let ts_now = self.timestamp_ns();
        alert_time_ns = std::cmp::max(alert_time_ns, ts_now);
        let timer = TestTimer::new(
            name.clone(),
            alert_time_ns - ts_now,
            ts_now,
            Some(alert_time_ns),
        );
        self.timers.insert(name, timer);
    }

    fn set_timer_ns_py(
        &mut self,
        name: String,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback_py: Option<PyObject>,
    ) {
        correctness::valid_string(&name, "`Timer` name");
        assert!(
            callback_py.is_some() | self.default_callback_py.is_some(),
            "All Python callbacks were `None`"
        );

        match callback_py {
            Some(callback_py) => self.callbacks_py.insert(name.clone(), callback_py),
            None => None,
        };

        let timer = TestTimer::new(name.clone(), interval_ns, start_time_ns, stop_time_ns);
        self.timers.insert(name, timer);
    }

    fn next_time_ns(&mut self, name: &str) -> UnixNanos {
        let timer = self.timers.get(name);
        match timer {
            None => 0,
            Some(timer) => timer.next_time_ns,
        }
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(name);
        match timer {
            None => {}
            Some(mut timer) => timer.cancel(),
        }
    }

    fn cancel_timers(&mut self) {
        for (_, timer) in self.timers.iter_mut() {
            timer.cancel()
        }
        self.timers = HashMap::new();
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API - TestClock
////////////////////////////////////////////////////////////////////////////////
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
/// - Assumes `callback_ptr` is a valid PyCallable pointer.
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
            .timers
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
/// - Assumes `name_ptr` is a valid C string pointer.
/// - Assumes `callback_ptr` is a valid PyCallable pointer.
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
/// - Assumes `name_ptr` is a valid C string pointer.
/// - Assumes `callback_ptr` is a valid PyCallable pointer.
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
        unsafe { Vec::from_raw_parts(ptr as *mut TimeEventHandler, len, cap) };
    drop(data); // Memory freed here
}

/// # Safety
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

////////////////////////////////////////////////////////////////////////////////
// C API - LiveClock
////////////////////////////////////////////////////////////////////////////////
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monotonic_clock_increasing() {
        let mut clock = MonotonicClock::new();
        let secs1 = clock.unix_timestamp_secs();
        let secs2 = clock.unix_timestamp_secs();
        assert!(secs2 >= secs1);

        let millis1 = clock.unix_timestamp_millis();
        let millis2 = clock.unix_timestamp_millis();
        assert!(millis2 >= millis1);

        let micros1 = clock.unix_timestamp_micros();
        let micros2 = clock.unix_timestamp_micros();
        assert!(micros2 >= micros1);

        let nanos1 = clock.unix_timestamp_nanos();
        let nanos2 = clock.unix_timestamp_nanos();
        assert!(nanos2 >= nanos1);
    }

    #[test]
    fn test_monotonic_clock_beyond_unix_epoch() {
        let mut clock = MonotonicClock::new();

        assert!(clock.unix_timestamp_secs() > 1_650_000_000.0);
        assert!(clock.unix_timestamp_millis() > 1_650_000_000_000);
        assert!(clock.unix_timestamp_micros() > 1_650_000_000_000_000);
        assert!(clock.unix_timestamp_nanos() > 1_650_000_000_000_000_000);
    }

    #[test]
    fn test_set_timer_ns_py() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let mut clock = TestClock::new();
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            clock.register_default_handler_py(py_append);

            clock.set_timer_ns_py(String::from("TEST_TIME1"), 10, 0, None, None);

            assert_eq!(clock.timer_names(), ["TEST_TIME1"]);
            assert_eq!(clock.timer_count(), 1);
        });
    }

    #[test]
    fn test_cancel_timer() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let mut clock = TestClock::new();
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            clock.register_default_handler_py(py_append);

            clock.set_timer_ns_py(String::from("TEST_TIME1"), 10, 0, None, None);
            clock.cancel_timer(String::from("TEST_TIME1").as_str());

            assert!(clock.timer_names().is_empty());
            assert_eq!(clock.timer_count(), 0);
        });
    }

    #[test]
    fn test_cancel_timers() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let mut clock = TestClock::new();
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            clock.register_default_handler_py(py_append);

            clock.set_timer_ns_py(String::from("TEST_TIME1"), 10, 0, None, None);
            clock.cancel_timers();

            assert!(clock.timer_names().is_empty());
            assert_eq!(clock.timer_count(), 0);
        });
    }

    #[test]
    fn test_advance_within_stop_time_py() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let mut clock = TestClock::new();
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            clock.register_default_handler_py(py_append);

            clock.set_timer_ns_py(String::from("TEST_TIME1"), 1, 1, Some(3), None);
            clock.advance_time(2, true);

            assert_eq!(clock.timer_names(), ["TEST_TIME1"]);
            assert_eq!(clock.timer_count(), 1);
        });
    }

    #[test]
    fn test_advance_time_to_stop_time_with_set_time_true() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let mut clock = TestClock::new();
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            clock.register_default_handler_py(py_append);

            clock.set_timer_ns_py(String::from("TEST_TIME1"), 2, 0, Some(3), None);
            clock.advance_time(3, true);

            assert_eq!(clock.timer_names().len(), 1);
            assert_eq!(clock.timer_count(), 1);
            assert_eq!(clock.time_ns, 3);
        });
    }

    #[test]
    fn test_advance_time_to_stop_time_with_set_time_false() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let mut clock = TestClock::new();
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            clock.register_default_handler_py(py_append);

            clock.set_timer_ns_py(String::from("TEST_TIME1"), 2, 0, Some(3), None);
            clock.advance_time(3, false);

            assert_eq!(clock.timer_names().len(), 1);
            assert_eq!(clock.timer_count(), 1);
            assert_eq!(clock.time_ns, 0);
        });
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::timer::{TimeEvent, TimeEventHandler};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use super::timer::TestTimer;
use nautilus_core::datetime::{nanos_to_millis, nanos_to_secs};
use nautilus_core::string::pystr_to_string;
use nautilus_core::time::{Timedelta, Timestamp};
use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3::AsPyPointer;

trait Clock {
    fn new() -> Self;

    /// Return the current UNIX time in seconds.
    fn timestamp(&self) -> f64;

    /// Return the current UNIX time in milliseconds (ms).
    fn timestamp_ms(&self) -> u64;

    /// Return the current UNIX time in nanoseconds (ns).
    fn timestamp_ns(&self) -> u64;

    /// If the clock is a test clock.
    fn is_test_clock(&self) -> bool;

    /// If the clock has a default handler registered.
    fn is_default_handler_registered(&self) -> bool;

    /// Register a default event handler for the clock. If a [Timer]
    /// does not have an event handler, then this handler is used.
    fn register_default_handler_py(&mut self, handler: PyObject);

    /// Return a list of timer names as a list of Python strings.
    fn timer_names_py(&self) -> PyObject;

    /// Set a [Timer] to alert at a particular time. Optional
    /// callback gets used to handle generated events.
    fn set_time_alert_ns(
        &mut self,
        // Both representations of name
        name: String,
        alert_time_ns: Timestamp,
        callback: Option<PyObject>,
    );

    /// Set a [Timer] to start alerting at every interval
    /// between start and stop time. Optional callback gets
    /// used to handle generated event.
    fn set_timer_ns(
        &mut self,
        name: String,
        interval_ns: Timedelta,
        start_time_ns: Timestamp,
        stop_time_ns: Timestamp,
        callback: Option<PyObject>,
    );
}

#[pyclass]
pub struct TestClock {
    pub time_ns: Timestamp,
    pub timers: HashMap<String, TestTimer>,
    pub handlers: HashMap<String, PyObject>,
    pub default_handler: Option<PyObject>,
}

impl TestClock {
    #[allow(dead_code)] // Temporary
    fn set_time(&mut self, to_time_ns: Timestamp) {
        self.time_ns = to_time_ns
    }

    #[inline]
    pub fn advance_time(&mut self, to_time_ns: Timestamp) -> Vec<TimeEvent> {
        // Time should increase monotonically
        assert!(
            to_time_ns >= self.time_ns,
            "`to_time_ns` was < `self._time_ns`"
        );

        self.timers
            .iter_mut()
            .filter(|(_, timer)| !timer.is_expired)
            .flat_map(|(_, timer)| timer.advance(to_time_ns))
            .collect()
    }

    #[inline]
    pub fn match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandler> {
        events
            .iter()
            .map(|event| {
                TimeEventHandler {
                    event: event.clone(),                                // Clone for now
                    handler: self.handlers[event.name.as_str()].clone(), // Clone for now
                }
            })
            .collect()
    }
}

impl Clock for TestClock {
    #[inline]
    fn new() -> TestClock {
        TestClock {
            time_ns: 0,
            timers: HashMap::new(),
            handlers: HashMap::new(),
            default_handler: None,
        }
    }

    #[allow(dead_code)] // Temporary
    #[inline]
    fn timestamp(&self) -> f64 {
        nanos_to_secs(self.time_ns)
    }

    #[allow(dead_code)] // Temporary
    #[inline]
    fn timestamp_ms(&self) -> u64 {
        nanos_to_millis(self.time_ns)
    }

    #[allow(dead_code)] // Temporary
    fn timestamp_ns(&self) -> u64 {
        self.time_ns
    }

    fn is_test_clock(&self) -> bool {
        true
    }

    fn is_default_handler_registered(&self) -> bool {
        self.default_handler.is_some()
    }

    #[inline]
    fn register_default_handler_py(&mut self, handler: PyObject) {
        let _ = self.default_handler.insert(handler);
    }

    fn timer_names_py(&self) -> PyObject {
        let timer_names = self.timers.keys().clone();
        Python::with_gil(|py| PyList::new(py, timer_names).into())
    }

    #[inline]
    fn set_time_alert_ns(
        &mut self,
        name: String,
        alert_time_ns: Timestamp,
        callback: Option<PyObject>,
    ) {
        assert!(
            callback.is_some() | self.default_handler.is_some(),
            "`callback` and `default_handler` were none"
        );
        let callback = match callback {
            None => self.default_handler.clone().unwrap(),
            Some(callback) => callback,
        };

        let timer = TestTimer::new(
            name.clone(),
            (alert_time_ns - self.time_ns) as Timedelta,
            self.time_ns,
            Some(alert_time_ns),
        );
        self.timers.insert(name.clone(), timer);
        self.handlers.insert(name, callback);
    }

    #[inline]
    fn set_timer_ns(
        &mut self,
        name: String,
        interval_ns: Timedelta,
        start_time_ns: Timestamp,
        stop_time_ns: Timestamp,
        callback: Option<PyObject>,
    ) {
        assert!(
            callback.is_some() | self.default_handler.is_some(),
            "`callback` and `default_handler` were none"
        );
        let callback = match callback {
            None => self.default_handler.clone().unwrap(),
            Some(callback) => callback,
        };

        let timer = TestTimer::new(name.clone(), interval_ns, start_time_ns, Some(stop_time_ns));
        self.timers.insert(name.clone(), timer);
        self.handlers.insert(name, callback);
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

#[repr(C)]
pub struct CTestClock(Box<TestClock>);

impl Deref for CTestClock {
    type Target = TestClock;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CTestClock {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[no_mangle]
pub extern "C" fn test_clock_new() -> CTestClock {
    CTestClock(Box::new(TestClock::new()))
}

#[no_mangle]
pub extern "C" fn test_clock_register_default_handler(clock: &mut CTestClock, handler: PyObject) {
    clock.register_default_handler_py(handler);
}

#[no_mangle]
pub extern "C" fn test_clock_set_time(clock: &mut CTestClock, to_time_ns: u64) {
    clock.set_time(to_time_ns);
}

/// # Safety
/// - `name` must be borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn test_clock_set_time_alert_ns(
    clock: &mut CTestClock,
    name: PyObject,
    alert_time_ns: Timestamp,
    callback: Option<PyObject>,
) {
    let name = pystr_to_string(name.as_ptr());
    clock.set_time_alert_ns(name, alert_time_ns, callback);
}

/// # Safety
/// - `name` must be borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn test_clock_set_timer_ns(
    clock: &mut CTestClock,
    name: PyObject,
    interval_ns: Timedelta,
    start_time_ns: Timestamp,
    stop_time_ns: Timestamp,
    callback: Option<PyObject>,
) {
    let name = pystr_to_string(name.as_ptr());
    clock.set_timer_ns(name, interval_ns, start_time_ns, stop_time_ns, callback);
}

#[no_mangle]
pub extern "C" fn test_clock_advance_time(clock: &mut CTestClock, to_time_ns: u64) -> PyObject {
    let events = clock.advance_time(to_time_ns);
    Python::with_gil(|py| {
        PyList::new(py, events.into_iter().map(|v| Py::new(py, v).unwrap())).into()
    })
}

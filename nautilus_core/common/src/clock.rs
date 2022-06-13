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

use crate::timer::TimeEvent;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use super::timer::TestTimer;
use nautilus_core::datetime::{nanos_to_millis, nanos_to_secs};
use nautilus_core::string::pystr_to_string;
use nautilus_core::time::{Timedelta, Timestamp};
use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3::AsPyPointer;

#[pyclass]
pub struct TestClock {
    time_ns: Timestamp,
    next_time_ns: Timestamp,
    timers: HashMap<String, TestTimer>,
    handlers: HashMap<String, PyObject>,
    default_handler: PyObject,
}

impl TestClock {
    #[inline]
    fn new(initial_ns: Timestamp, default_handler: PyObject) -> TestClock {
        TestClock {
            time_ns: initial_ns,
            next_time_ns: 0,
            timers: HashMap::new(),
            handlers: HashMap::new(),
            default_handler,
        }
    }

    #[inline]
    fn timestamp(&self) -> f64 {
        nanos_to_secs(self.time_ns as f64)
    }

    #[inline]
    fn timestamp_ms(&self) -> u64 {
        nanos_to_millis(self.time_ns)
    }

    #[inline]
    fn timestamp_ns(&self) -> u64 {
        self.time_ns
    }

    fn set_time(&mut self, to_time_ns: Timestamp) {
        self.time_ns = to_time_ns
    }

    #[inline]
    fn advance_time(&mut self, to_time_ns: Timestamp) -> Vec<TimeEventHandler> {
        // Time should increase monotonically
        assert!(
            to_time_ns >= self.time_ns,
            "Time to advance to should be greater than current clock time"
        );

        let events = self
            .timers
            .iter_mut()
            .filter(|(_, timer)| !timer.is_expired)
            .flat_map(|(name_id, timer)| {
                let handler = self.handlers.get(name_id).unwrap_or(&self.default_handler);
                timer.advance(to_time_ns).map(|event| TimeEventHandler {
                    event,
                    handler: handler.clone(),
                })
            })
            .collect();

        // Update next event time for clock with minimum next event time
        // between all timers.
        self.next_time_ns = self
            .timers
            .values()
            .filter(|timer| !timer.is_expired)
            .map(|timer| timer.next_time_ns)
            .min()
            .unwrap_or(0);
        self.time_ns = to_time_ns;
        events
    }
}

trait Clock {
    /// Register a default event handler for the clock. If a [Timer]
    /// does not have an event handler this handler is used.
    fn register_default_handler(&mut self, handler: PyObject);
    /// Set a [Timer] to alert at a particular time. Optional
    /// callback gets used to handle generated event.
    fn set_time_alert_ns(
        &mut self,
        // both representation of of name
        name: (String, PyObject),
        alert_time_ns: Timestamp,
        callback: Option<PyObject>,
    );
    /// Set a [Timer] to start alerting at every interval
    /// between start and stop time. Optional callback gets
    /// used to handle generated event.
    fn set_timer_ns(
        &mut self,
        // both representation of of name
        name: (String, PyObject),
        interval_ns: Timedelta,
        start_time_ns: Timestamp,
        stop_time_ns: Timestamp,
        callback: Option<PyObject>,
    );
}

impl Clock for TestClock {
    #[inline]
    fn register_default_handler(&mut self, handler: PyObject) {
        self.default_handler = handler
    }

    #[inline]
    fn set_time_alert_ns(
        &mut self,
        name: (String, PyObject),
        alert_time_ns: Timestamp,
        callback: Option<PyObject>,
    ) {
        let callback = callback.unwrap_or_else(|| self.default_handler.clone());
        let timer = TestTimer::new(
            name.1,
            (alert_time_ns - self.time_ns) as Timedelta,
            self.time_ns,
            Some(alert_time_ns),
        );
        self.timers.insert(name.0.clone(), timer);
        self.handlers.insert(name.0, callback);
    }

    #[inline]
    fn set_timer_ns(
        &mut self,
        name: (String, PyObject),
        interval_ns: Timedelta,
        start_time_ns: Timestamp,
        stop_time_ns: Timestamp,
        callback: Option<PyObject>,
    ) {
        let callback = callback.unwrap_or_else(|| self.default_handler.clone());
        let timer = TestTimer::new(name.1, interval_ns, start_time_ns, Some(stop_time_ns));
        self.timers.insert(name.0.clone(), timer);
        self.handlers.insert(name.0, callback);
    }
}

/// Represents a bundled event and it's handler
#[repr(C)]
#[pyclass]
#[derive(Clone)]
pub struct TimeEventHandler {
    // A [TimeEvent] generated by a timer
    pub event: TimeEvent,
    /// A callable handler for this time event
    pub handler: PyObject,
}

impl TimeEventHandler {
    #[inline]
    fn handle_py(self) {
        Python::with_gil(|py| {
            let _ = self.handler.call0(py);
        });
    }

    #[inline]
    fn handle(self) {
        Python::with_gil(|py| {
            let _ = self.handler.call1(py, (self.event,));
        });
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

pub extern "C" fn new_test_clock(initial_ns: Timestamp, default_handler: PyObject) -> CTestClock {
    CTestClock(Box::new(TestClock::new(initial_ns, default_handler)))
}

pub extern "C" fn register_default_handler(clock: &mut CTestClock, handler: PyObject) {
    clock.register_default_handler(handler);
}

pub unsafe extern "C" fn set_time_alert_ns(
    clock: &mut CTestClock,
    name: PyObject,
    alert_time_ns: Timestamp,
    callback: Option<PyObject>,
) {
    let name = (pystr_to_string(name.as_ptr()), name);
    clock.set_time_alert_ns(name, alert_time_ns, callback);
}

pub unsafe extern "C" fn set_timer_ns(
    clock: &mut CTestClock,
    name: PyObject,
    interval_ns: Timedelta,
    start_time_ns: Timestamp,
    stop_time_ns: Timestamp,
    callback: Option<PyObject>,
) {
    let name = (pystr_to_string(name.as_ptr()), name);
    clock.set_timer_ns(name, interval_ns, start_time_ns, stop_time_ns, callback);
}

pub extern "C" fn advance_time(clock: &mut CTestClock, to_time_ns: u64) -> PyObject {
    let events = clock.advance_time(to_time_ns);
    Python::with_gil(|py| {
        PyList::new(py, events.into_iter().map(|v| Py::new(py, v).unwrap())).into()
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use pyo3::{prelude::*, types::*};

    use crate::clock::set_time_alert_ns;

    use super::new_test_clock;

    #[test]
    fn test_test_clock() {
        pyo3::prepare_freethreaded_python();
        let mut test_clock = Python::with_gil(|py| {
            let dummy = PyDict::new(py).into();
            new_test_clock(0, dummy)
        });

        assert_eq!(test_clock.time_ns, 0);
        let timer_name = "tringtring";

        let (name, callback) = Python::with_gil(|py| {
            let name = PyString::new(py, timer_name).into();
            let dummy = Some(PyDict::new(py).into());
            (name, dummy)
        });

        unsafe {
            set_time_alert_ns(&mut test_clock, name, 2_000, callback);
        }

        assert_eq!(test_clock.timers.len(), 1);
        assert_eq!(
            test_clock.timers.keys().next().unwrap().as_str(),
            timer_name
        );

        let events = test_clock.advance_time(3_000);

        assert_eq!(test_clock.timers.values().next().unwrap().is_expired, true);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events.iter().next().unwrap().event.name.to_string(),
            String::from_str(timer_name).unwrap()
        );
    }

    #[test]
    fn test_event_callback() {
        pyo3::prepare_freethreaded_python();
        let mut test_clock = Python::with_gil(|py| {
            let dummy = PyDict::new(py).into();
            new_test_clock(0, dummy)
        });

        let (name, callback, pymod): (PyObject, PyObject, PyObject) = Python::with_gil(|py| {
            let code = include_str!("./test_data/callback.py");
            let pymod = PyModule::from_code(py, &code, "humpty", "dumpty").unwrap();
            let name = PyString::new(py, "brrrringbrrring");
            let callback = pymod.getattr("increment").unwrap();
            (name.into(), callback.into(), pymod.into())
        });

        unsafe {
            set_time_alert_ns(&mut test_clock, name, 2_000, Some(callback));
        }

        let events = test_clock.advance_time(3_000);
        events
            .into_iter()
            .for_each(|time_event_handler| time_event_handler.handle());

        let count: u64 =
            Python::with_gil(|py| pymod.getattr(py, "count").unwrap().extract(py).unwrap());

        assert_eq!(count, 1);
    }
}

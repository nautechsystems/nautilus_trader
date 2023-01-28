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
use std::ptr::null;

use nautilus_core::correctness;
use nautilus_core::datetime::{nanos_to_millis, nanos_to_secs};
use nautilus_core::string::cstr_to_string;
use nautilus_core::time::UnixNanos;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use pyo3::{ffi, AsPyPointer};

use crate::timer::{TestTimer, TimeEvent, Vec_TimeEvent};

/// Represents a type of clock.
/// # Notes
/// An active timer is one which has not expired (`timer.is_expired == False`).
trait Clock {
    /// Return a new [Clock].
    fn new() -> Self;

    /// Return the current UNIX time in seconds.
    fn timestamp(&self) -> f64;

    /// Return the current UNIX time in milliseconds (ms).
    fn timestamp_ms(&self) -> u64;

    /// Return the current UNIX time in nanoseconds (ns).
    fn timestamp_ns(&self) -> u64;

    /// Return the names of active timers in the clock.
    fn timer_names(&self) -> Vec<&str>;

    /// Return the count of active timers in the clock.
    fn timer_count(&self) -> usize;

    /// Register a default event handler for the clock. If a [Timer]
    /// does not have an event handler, then this handler is used.
    fn register_default_handler(&mut self, handler: Box<dyn Fn(TimeEvent)>);

    /// Set a [Timer] to alert at a particular time. Optional
    /// callback gets used to handle generated events.
    fn set_time_alert_ns(
        &mut self,
        name: String,
        alert_time_ns: UnixNanos,
        callback: Option<Box<dyn Fn(TimeEvent)>>,
    );

    /// Set a [Timer] to start alerting at every interval
    /// between start and stop time. Optional callback gets
    /// used to handle generated event.
    fn set_timer_ns(
        &mut self,
        name: String,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<Box<dyn Fn(TimeEvent)>>,
    );

    fn next_time_ns(&mut self, name: &str) -> UnixNanos;
    fn cancel_timer(&mut self, name: &str);
    fn cancel_timers(&mut self);
}

pub struct TestClock {
    pub time_ns: UnixNanos,
    pub timers: HashMap<String, TestTimer>,
    pub handlers: HashMap<String, Box<dyn Fn(TimeEvent)>>,
    pub default_handler: Option<Box<dyn Fn(TimeEvent)>>,
}

impl TestClock {
    #[allow(dead_code)] // Temporary
    fn set_time(&mut self, to_time_ns: UnixNanos) {
        self.time_ns = to_time_ns
    }

    #[inline]
    pub fn advance_time(&mut self, to_time_ns: UnixNanos, set_time: bool) -> Vec<TimeEvent> {
        // Time should increase monotonically
        assert!(
            to_time_ns >= self.time_ns,
            "`to_time_ns` was < `self._time_ns`"
        );

        if set_time {
            self.time_ns = to_time_ns;
        }

        self.timers
            .iter_mut()
            .filter(|(_, timer)| !timer.is_expired)
            .flat_map(|(_, timer)| timer.advance(to_time_ns))
            .collect()
    }

    // #[inline]
    // pub fn match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandler> {
    //     events
    //         .into_iter()
    //         .map(|event| {
    //             TimeEventHandler {
    //                 event: event.clone(), // Clone for now
    //                 handler: &self.handlers[event.name.as_str()],
    //             }
    //         })
    //         .collect()
    // }
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

    #[inline]
    fn timestamp(&self) -> f64 {
        nanos_to_secs(self.time_ns)
    }

    #[inline]
    fn timestamp_ms(&self) -> u64 {
        nanos_to_millis(self.time_ns)
    }

    fn timestamp_ns(&self) -> u64 {
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

    #[inline]
    fn register_default_handler(&mut self, handler: Box<dyn Fn(TimeEvent)>) {
        self.default_handler = Some(handler);
    }

    #[inline]
    fn set_time_alert_ns(
        &mut self,
        name: String,
        alert_time_ns: UnixNanos,
        callback: Option<Box<dyn Fn(TimeEvent)>>,
    ) {
        correctness::valid_string(&name, "`Timer` name");
        // assert!(
        //     callback.is_some() | self.default_handler.is_some(),
        //     "`callback` and `default_handler` were none"
        // );

        match callback {
            Some(callback) => self.handlers.insert(name.clone(), callback),
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

    #[inline]
    fn set_timer_ns(
        &mut self,
        name: String,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<Box<dyn Fn(TimeEvent)>>,
    ) {
        correctness::valid_string(&name, "`Timer` name");
        // assert!(
        //     callback.is_some() | self.default_handler.is_some(),
        //     "`callback` and `default_handler` were none"
        // );
        match callback {
            None => None,
            Some(callback) => self.handlers.insert(name.clone(), callback),
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

    #[inline]
    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(name);
        match timer {
            None => {}
            Some(mut timer) => timer.cancel(),
        }
    }

    #[inline]
    fn cancel_timers(&mut self) {
        for (_, timer) in self.timers.iter_mut() {
            timer.cancel()
        }
        self.timers = HashMap::new();
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
pub extern "C" fn test_clock_free(clock: CTestClock) {
    drop(clock); // Memory freed here
}

#[no_mangle]
pub extern "C" fn test_clock_set_time(clock: &mut CTestClock, to_time_ns: u64) {
    clock.set_time(to_time_ns);
}

#[no_mangle]
pub extern "C" fn test_clock_time_ns(clock: &CTestClock) -> u64 {
    clock.time_ns
}

#[no_mangle]
pub extern "C" fn test_clock_timer_names(clock: &CTestClock) -> *mut ffi::PyObject {
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
pub extern "C" fn test_clock_timer_count(clock: &mut CTestClock) -> usize {
    clock.timer_count()
}

/// # Safety
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_set_time_alert_ns(
    clock: &mut CTestClock,
    name_ptr: *const c_char,
    alert_time_ns: UnixNanos,
) {
    let name = cstr_to_string(name_ptr);
    clock.set_time_alert_ns(name, alert_time_ns, None);
}

/// # Safety
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_set_timer_ns(
    clock: &mut CTestClock,
    name_ptr: *const c_char,
    interval_ns: u64,
    start_time_ns: UnixNanos,
    stop_time_ns: UnixNanos,
) {
    let name = cstr_to_string(name_ptr);
    let stop_time_ns = match stop_time_ns {
        0 => None,
        _ => Some(stop_time_ns),
    };
    clock.set_timer_ns(name, interval_ns, start_time_ns, stop_time_ns, None);
}

/// # Safety
/// - Assumes `set_time` is a correct `uint8_t` of either 0 or 1.
#[no_mangle]
pub unsafe extern "C" fn test_clock_advance_time(
    clock: &mut CTestClock,
    to_time_ns: u64,
    set_time: u8,
) -> Vec_TimeEvent {
    let events: Vec<TimeEvent> = clock.advance_time(to_time_ns, set_time != 0);
    let len = events.len();
    let data = match events.is_empty() {
        true => null() as *const TimeEvent,
        false => &events.leak()[0],
    };
    Vec_TimeEvent {
        ptr: data as *const TimeEvent,
        len,
    }
}

#[no_mangle]
pub extern "C" fn vec_time_events_drop(v: Vec_TimeEvent) {
    drop(v); // Memory freed here
}

/// # Safety
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_next_time_ns(
    clock: &mut CTestClock,
    name_ptr: *const c_char,
) -> UnixNanos {
    let name = cstr_to_string(name_ptr);
    clock.next_time_ns(&name)
}

/// # Safety
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn test_clock_cancel_timer(clock: &mut CTestClock, name_ptr: *const c_char) {
    let name = cstr_to_string(name_ptr);
    clock.cancel_timer(&name);
}

#[no_mangle]
pub extern "C" fn test_clock_cancel_timers(clock: &mut CTestClock) {
    clock.cancel_timers();
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::clock::{Clock, TestClock};

    #[test]
    fn test_set_timer_ns() {
        let mut clock = TestClock::new();
        clock.set_timer_ns(String::from("TEST_TIME1"), 10, 0, None, None);
        assert_eq!(clock.timer_names(), ["TEST_TIME1"]);
        assert_eq!(clock.timer_count(), 1);
    }

    #[test]
    fn test_advance_within_stop_time() {
        let mut clock = TestClock::new();
        clock.set_timer_ns(String::from("TEST_TIME1"), 1, 1, Some(3), None);
        clock.advance_time(2, true);
        assert_eq!(clock.timer_names(), ["TEST_TIME1"]);
        assert_eq!(clock.timer_count(), 1);
    }

    #[test]
    fn test_advance_time_to_stop_time_with_set_time_true() {
        let mut clock = TestClock::new();
        clock.set_timer_ns(String::from("TEST_TIME1"), 2, 0, Some(3), None);
        clock.advance_time(3, true);
        assert_eq!(clock.timer_names().len(), 1);
        assert_eq!(clock.timer_count(), 1);
        assert_eq!(clock.time_ns, 3);
    }

    #[test]
    fn test_advance_time_to_stop_time_with_set_time_false() {
        let mut clock = TestClock::new();
        clock.set_timer_ns(String::from("TEST_TIME1"), 2, 0, Some(3), None);
        clock.advance_time(3, false);
        assert_eq!(clock.timer_names().len(), 1);
        assert_eq!(clock.timer_count(), 1);
        assert_eq!(clock.time_ns, 0);
    }
}

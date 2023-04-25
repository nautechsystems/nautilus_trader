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

use std::cmp::Ordering;
use std::ops::{Deref, DerefMut};
use std::ptr::null;

use nautilus_common::clock::{TestClock, TestClockAPI};
use nautilus_common::timer::{TimeEventHandler, Vec_TimeEventHandler};
use nautilus_core::time::UnixNanos;

/// Provides a means of accumulating and draining time event handlers.
pub struct TimeEventAccumulator {
    event_handlers: Vec<TimeEventHandler>,
}

impl TimeEventAccumulator {
    /// Initializes a new `TimeEventAccumulator` instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            event_handlers: Vec::new(),
        }
    }

    /// Advance the given clock to the `to_time_ns`.
    pub fn advance_clock(&mut self, clock: &mut TestClock, to_time_ns: UnixNanos, set_time: bool) {
        let events = clock.advance_time(to_time_ns, set_time);
        let handlers = clock.match_handlers_py(events);
        self.event_handlers.extend(handlers);
    }

    /// Drain the accumulated time event handlers in sorted order (by the events `ts_event`).
    pub fn drain(&mut self) -> Vec<TimeEventHandler> {
        self.event_handlers.sort_by(|a, b| {
            a.event
                .ts_event
                .partial_cmp(&b.event.ts_event)
                .unwrap_or(Ordering::Equal)
        });
        self.event_handlers.drain(..).collect()
    }
}

impl Default for TimeEventAccumulator {
    fn default() -> Self {
        TimeEventAccumulator::new()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

#[repr(C)]
pub struct TimeEventAccumulatorAPI(Box<TimeEventAccumulator>);

impl Deref for TimeEventAccumulatorAPI {
    type Target = TimeEventAccumulator;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TimeEventAccumulatorAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[no_mangle]
pub extern "C" fn time_event_accumulator_new() -> TimeEventAccumulatorAPI {
    TimeEventAccumulatorAPI(Box::new(TimeEventAccumulator::new()))
}

#[no_mangle]
pub extern "C" fn time_event_accumulator_free(accumulator: TimeEventAccumulatorAPI) {
    drop(accumulator); // Memory freed here
}

#[no_mangle]
pub extern "C" fn time_event_accumulator_advance_clock(
    accumulator: &mut TimeEventAccumulatorAPI,
    clock: &mut TestClockAPI,
    to_time_ns: UnixNanos,
    set_time: u8,
) {
    accumulator.advance_clock(clock, to_time_ns, set_time != 0);
}

#[no_mangle]
pub extern "C" fn time_event_accumulator_drain(
    accumulator: &mut TimeEventAccumulatorAPI,
) -> Vec_TimeEventHandler {
    let handlers = accumulator.drain();
    let len = handlers.len();
    let data = match handlers.is_empty() {
        true => null() as *const TimeEventHandler,
        false => &handlers.leak()[0],
    };
    Vec_TimeEventHandler {
        ptr: data as *const TimeEventHandler,
        len,
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;

    use nautilus_common::timer::TimeEvent;
    use nautilus_core::uuid::UUID4;
    use pyo3::ffi::{self, Py_INCREF};
    use pyo3::types::PyList;
    use pyo3::{prelude::*, AsPyPointer};

    #[test]
    fn test_accumulator_drain_sorted() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());

            let mut accumulator = TimeEventAccumulator::new();

            let time_event1 = TimeEvent::new(String::from("TEST_EVENT_1"), UUID4::new(), 100, 100);
            let time_event2 = TimeEvent::new(String::from("TEST_EVENT_2"), UUID4::new(), 300, 300);
            let time_event3 = TimeEvent::new(String::from("TEST_EVENT_3"), UUID4::new(), 200, 200);

            let callback_ptr = py_append.as_ptr() as *mut ffi::PyObject;
            unsafe { Py_INCREF(callback_ptr) };

            let handler1 = TimeEventHandler {
                event: time_event1.clone(),
                callback_ptr,
            };

            unsafe { Py_INCREF(callback_ptr) };
            let handler2 = TimeEventHandler {
                event: time_event2.clone(),
                callback_ptr,
            };

            unsafe { Py_INCREF(callback_ptr) };
            let handler3 = TimeEventHandler {
                event: time_event3.clone(),
                callback_ptr,
            };

            accumulator.event_handlers.push(handler1.clone());
            accumulator.event_handlers.push(handler2.clone());
            accumulator.event_handlers.push(handler3.clone());

            let drained_handlers = accumulator.drain();

            assert_eq!(drained_handlers.len(), 3);
            assert_eq!(drained_handlers[0].event.ts_event, time_event1.ts_event);
            assert_eq!(drained_handlers[1].event.ts_event, time_event3.ts_event);
            assert_eq!(drained_handlers[2].event.ts_event, time_event2.ts_event);
        });
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::ops::{Deref, DerefMut};

use nautilus_common::{clock::TestClock, ffi::clock::TestClock_API, timer::TimeEventHandler};
use nautilus_core::{
    ffi::{cvec::CVec, parsing::u8_as_bool},
    time::UnixNanos,
};

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
        let handlers = clock.match_handlers(events);
        self.event_handlers.extend(handlers);
    }

    /// Drain the accumulated time event handlers in sorted order (by the events `ts_event`).
    pub fn drain(&mut self) -> Vec<TimeEventHandler> {
        // stable sort is not necessary since there is no relation between
        // events of the same clock. Only time based ordering is needed.
        self.event_handlers
            .sort_unstable_by_key(|v| v.event.ts_event);
        self.event_handlers.drain(..).collect()
    }
}

impl Default for TimeEventAccumulator {
    fn default() -> Self {
        Self::new()
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
pub extern "C" fn time_event_accumulator_drop(accumulator: TimeEventAccumulatorAPI) {
    drop(accumulator); // Memory freed here
}

#[no_mangle]
pub extern "C" fn time_event_accumulator_advance_clock(
    accumulator: &mut TimeEventAccumulatorAPI,
    clock: &mut TestClock_API,
    to_time_ns: UnixNanos,
    set_time: u8,
) {
    accumulator.advance_clock(clock, to_time_ns, u8_as_bool(set_time));
}

#[no_mangle]
pub extern "C" fn time_event_accumulator_drain(accumulator: &mut TimeEventAccumulatorAPI) -> CVec {
    accumulator.drain().into()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::c_char;

    use nautilus_common::timer::TimeEvent;
    use nautilus_core::uuid::UUID4;
    use pyo3::{types::PyList, Py, Python};
    use rstest::*;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_accumulator_drain_sorted() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());

            let mut accumulator = TimeEventAccumulator::new();

            let time_event1 = TimeEvent::new(Ustr::from("TEST_EVENT_1"), UUID4::new(), 100, 100);
            let time_event2 = TimeEvent::new(Ustr::from("TEST_EVENT_2"), UUID4::new(), 300, 300);
            let time_event3 = TimeEvent::new(Ustr::from("TEST_EVENT_3"), UUID4::new(), 200, 200);

            // Note: as_ptr returns a borrowed pointer. It is valid as long
            // as the object is in scope. In this case `callback_ptr` is valid
            // as long as `py_append` is in scope.
            let callback_ptr = py_append
                .as_ptr()
                .cast::<pyo3::ffi::PyObject>()
                .cast::<c_char>();

            let handler1 = TimeEventHandler {
                event: time_event1.clone(),
                callback_ptr,
            };

            let handler2 = TimeEventHandler {
                event: time_event2.clone(),
                callback_ptr,
            };

            let handler3 = TimeEventHandler {
                event: time_event3.clone(),
                callback_ptr,
            };

            accumulator.event_handlers.push(handler1);
            accumulator.event_handlers.push(handler2);
            accumulator.event_handlers.push(handler3);

            let drained_handlers = accumulator.drain();

            assert_eq!(drained_handlers.len(), 3);
            assert_eq!(drained_handlers[0].event.ts_event, time_event1.ts_event);
            assert_eq!(drained_handlers[1].event.ts_event, time_event3.ts_event);
            assert_eq!(drained_handlers[2].event.ts_event, time_event2.ts_event);
        });
    }
}

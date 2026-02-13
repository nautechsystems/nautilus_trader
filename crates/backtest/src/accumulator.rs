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

use std::{cmp::Ordering, collections::BinaryHeap};

use nautilus_common::{clock::TestClock, timer::TimeEventHandler};
use nautilus_core::UnixNanos;

/// Wrapper for heap ordering (earlier timestamps = higher priority).
///
/// Uses reverse ordering so the BinaryHeap (max-heap) behaves as a min-heap.
#[derive(Clone, Debug)]
pub struct ScheduledTimeEventHandler(pub TimeEventHandler);

impl PartialEq for ScheduledTimeEventHandler {
    fn eq(&self, other: &Self) -> bool {
        self.0.event.ts_event == other.0.event.ts_event
    }
}

impl Eq for ScheduledTimeEventHandler {}

impl PartialOrd for ScheduledTimeEventHandler {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledTimeEventHandler {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap behavior (earlier timestamps = higher priority)
        other.0.event.ts_event.cmp(&self.0.event.ts_event)
    }
}

/// Provides a means of accumulating and draining time event handlers using a priority queue.
///
/// Events are maintained in timestamp order using a binary heap, allowing efficient
/// retrieval of the next event to process.
#[derive(Debug)]
pub struct TimeEventAccumulator {
    heap: BinaryHeap<ScheduledTimeEventHandler>,
}

impl TimeEventAccumulator {
    /// Creates a new [`TimeEventAccumulator`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    /// Advance the given clock to the `to_time_ns` and push events to the heap.
    pub fn advance_clock(&mut self, clock: &mut TestClock, to_time_ns: UnixNanos, set_time: bool) {
        let events = clock.advance_time(to_time_ns, set_time);
        let handlers = clock.match_handlers(events);
        for handler in handlers {
            self.heap.push(ScheduledTimeEventHandler(handler));
        }
    }

    /// Peek at the next event timestamp without removing it.
    ///
    /// Returns `None` if the heap is empty.
    #[must_use]
    pub fn peek_next_time(&self) -> Option<UnixNanos> {
        self.heap.peek().map(|h| h.0.event.ts_event)
    }

    /// Pop the next event if its timestamp is at or before `ts`.
    ///
    /// Returns `None` if the heap is empty or the next event is after `ts`.
    pub fn pop_next_at_or_before(&mut self, ts: UnixNanos) -> Option<TimeEventHandler> {
        if self.heap.peek().is_some_and(|h| h.0.event.ts_event <= ts) {
            self.heap.pop().map(|h| h.0)
        } else {
            None
        }
    }

    /// Check if the heap is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Get the number of events in the heap.
    #[must_use]
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Clear all events from the heap.
    pub fn clear(&mut self) {
        self.heap.clear();
    }

    /// Drain all events from the heap in timestamp order.
    ///
    /// This is provided for backwards compatibility with code that expects
    /// batch processing. For iterative processing, prefer `pop_next_at_or_before`.
    pub fn drain(&mut self) -> Vec<TimeEventHandler> {
        let mut handlers = Vec::with_capacity(self.heap.len());
        while let Some(scheduled) = self.heap.pop() {
            handlers.push(scheduled.0);
        }
        handlers
    }
}

impl Default for TimeEventAccumulator {
    /// Creates a new default [`TimeEventAccumulator`] instance.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use nautilus_common::timer::{TimeEvent, TimeEventCallback};
    use nautilus_core::UUID4;
    use pyo3::{Py, Python, prelude::*, types::PyList};
    use rstest::*;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_accumulator_pop_in_order() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());

            let mut accumulator = TimeEventAccumulator::new();

            let time_event1 = TimeEvent::new(
                Ustr::from("TEST_EVENT_1"),
                UUID4::new(),
                100.into(),
                100.into(),
            );
            let time_event2 = TimeEvent::new(
                Ustr::from("TEST_EVENT_2"),
                UUID4::new(),
                300.into(),
                300.into(),
            );
            let time_event3 = TimeEvent::new(
                Ustr::from("TEST_EVENT_3"),
                UUID4::new(),
                200.into(),
                200.into(),
            );

            let callback = TimeEventCallback::from(py_append.into_any());

            let handler1 = TimeEventHandler::new(time_event1.clone(), callback.clone());
            let handler2 = TimeEventHandler::new(time_event2.clone(), callback.clone());
            let handler3 = TimeEventHandler::new(time_event3.clone(), callback);

            accumulator.heap.push(ScheduledTimeEventHandler(handler1));
            accumulator.heap.push(ScheduledTimeEventHandler(handler2));
            accumulator.heap.push(ScheduledTimeEventHandler(handler3));
            assert_eq!(accumulator.len(), 3);

            let popped1 = accumulator.pop_next_at_or_before(1000.into()).unwrap();
            assert_eq!(popped1.event.ts_event, time_event1.ts_event);

            let popped2 = accumulator.pop_next_at_or_before(1000.into()).unwrap();
            assert_eq!(popped2.event.ts_event, time_event3.ts_event);

            let popped3 = accumulator.pop_next_at_or_before(1000.into()).unwrap();
            assert_eq!(popped3.event.ts_event, time_event2.ts_event);

            assert!(accumulator.is_empty());
        });
    }

    #[rstest]
    fn test_accumulator_pop_respects_timestamp() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());

            let mut accumulator = TimeEventAccumulator::new();

            let time_event1 = TimeEvent::new(
                Ustr::from("TEST_EVENT_1"),
                UUID4::new(),
                100.into(),
                100.into(),
            );
            let time_event2 = TimeEvent::new(
                Ustr::from("TEST_EVENT_2"),
                UUID4::new(),
                300.into(),
                300.into(),
            );

            let callback = TimeEventCallback::from(py_append.into_any());

            accumulator
                .heap
                .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                    time_event1.clone(),
                    callback.clone(),
                )));
            accumulator
                .heap
                .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                    time_event2.clone(),
                    callback,
                )));

            let popped1 = accumulator.pop_next_at_or_before(200.into()).unwrap();
            assert_eq!(popped1.event.ts_event, time_event1.ts_event);

            // Event at 300 should not be returned with ts=200
            assert!(accumulator.pop_next_at_or_before(200.into()).is_none());

            let popped2 = accumulator.pop_next_at_or_before(300.into()).unwrap();
            assert_eq!(popped2.event.ts_event, time_event2.ts_event);
        });
    }

    #[rstest]
    fn test_peek_next_time() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let callback = TimeEventCallback::from(py_append.into_any());

            let mut accumulator = TimeEventAccumulator::new();
            assert!(accumulator.peek_next_time().is_none());

            let time_event1 = TimeEvent::new(
                Ustr::from("TEST_EVENT_1"),
                UUID4::new(),
                200.into(),
                200.into(),
            );
            let time_event2 = TimeEvent::new(
                Ustr::from("TEST_EVENT_2"),
                UUID4::new(),
                100.into(),
                100.into(),
            );

            accumulator
                .heap
                .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                    time_event1,
                    callback.clone(),
                )));
            assert_eq!(accumulator.peek_next_time(), Some(200.into()));

            accumulator
                .heap
                .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                    time_event2,
                    callback,
                )));
            assert_eq!(accumulator.peek_next_time(), Some(100.into()));
        });
    }

    #[rstest]
    fn test_drain_returns_in_order() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let callback = TimeEventCallback::from(py_append.into_any());

            let mut accumulator = TimeEventAccumulator::new();

            for ts in [300u64, 100, 200] {
                let event = TimeEvent::new(Ustr::from("TEST"), UUID4::new(), ts.into(), ts.into());
                accumulator
                    .heap
                    .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                        event,
                        callback.clone(),
                    )));
            }

            let handlers = accumulator.drain();

            assert_eq!(handlers.len(), 3);
            assert_eq!(handlers[0].event.ts_event.as_u64(), 100);
            assert_eq!(handlers[1].event.ts_event.as_u64(), 200);
            assert_eq!(handlers[2].event.ts_event.as_u64(), 300);
            assert!(accumulator.is_empty());
        });
    }

    #[rstest]
    fn test_interleaved_push_pop_maintains_order() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let callback = TimeEventCallback::from(py_append.into_any());

            let mut accumulator = TimeEventAccumulator::new();
            let mut popped_timestamps: Vec<u64> = Vec::new();

            for ts in [100u64, 300] {
                let event = TimeEvent::new(Ustr::from("TEST"), UUID4::new(), ts.into(), ts.into());
                accumulator
                    .heap
                    .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                        event,
                        callback.clone(),
                    )));
            }

            let handler = accumulator.pop_next_at_or_before(1000.into()).unwrap();
            popped_timestamps.push(handler.event.ts_event.as_u64());

            // Simulate callback scheduling new event at 150 (between popped 100 and pending 300)
            let event = TimeEvent::new(Ustr::from("NEW"), UUID4::new(), 150.into(), 150.into());
            accumulator
                .heap
                .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                    event, callback,
                )));

            while let Some(handler) = accumulator.pop_next_at_or_before(1000.into()) {
                popped_timestamps.push(handler.event.ts_event.as_u64());
            }

            assert_eq!(popped_timestamps, vec![100, 150, 300]);
        });
    }

    #[rstest]
    fn test_same_timestamp_events() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let callback = TimeEventCallback::from(py_append.into_any());

            let mut accumulator = TimeEventAccumulator::new();

            for i in 0..3 {
                let event = TimeEvent::new(
                    Ustr::from(&format!("EVENT_{i}")),
                    UUID4::new(),
                    100.into(),
                    100.into(),
                );
                accumulator
                    .heap
                    .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                        event,
                        callback.clone(),
                    )));
            }

            let mut count = 0;
            while let Some(handler) = accumulator.pop_next_at_or_before(100.into()) {
                assert_eq!(handler.event.ts_event.as_u64(), 100);
                count += 1;
            }
            assert_eq!(count, 3);
        });
    }

    #[rstest]
    fn test_pop_at_exact_timestamp_boundary() {
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let callback = TimeEventCallback::from(py_append.into_any());

            let mut accumulator = TimeEventAccumulator::new();

            let event = TimeEvent::new(Ustr::from("TEST"), UUID4::new(), 100.into(), 100.into());
            accumulator
                .heap
                .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                    event, callback,
                )));

            let handler = accumulator.pop_next_at_or_before(100.into());
            assert!(handler.is_some());
            assert_eq!(handler.unwrap().event.ts_event.as_u64(), 100);

            let event2 = TimeEvent::new(Ustr::from("TEST2"), UUID4::new(), 200.into(), 200.into());
            accumulator
                .heap
                .push(ScheduledTimeEventHandler(TimeEventHandler::new(
                    event2,
                    TimeEventCallback::from(
                        Py::from(py_list.getattr("append").unwrap()).into_any(),
                    ),
                )));

            assert!(accumulator.pop_next_at_or_before(199.into()).is_none());
            assert!(accumulator.pop_next_at_or_before(200.into()).is_some());
        });
    }
}

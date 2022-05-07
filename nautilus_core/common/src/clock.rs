use crate::timer::{TimeEvent};
use std::collections::HashMap;

use super::timer::{NameID, TestTimer};
use nautilus_core::datetime::{nanos_to_millis, nanos_to_secs};
use pyo3::prelude::*;

struct TestClock {
    time_ns: u64,
    next_time_ns: u64,
    timers: HashMap<NameID, TestTimer>,
    handlers: HashMap<NameID, PyObject>,
    default_handler: PyObject,
}

impl TestClock {
    fn new(initial_ns: u64, default_handler: PyObject) -> TestClock {
        TestClock {
            time_ns: initial_ns,
            next_time_ns: 0,
            timers: HashMap::new(),
            handlers: HashMap::new(),
            default_handler,
        }
    }

    fn timestamp(&self) -> f64 {
        nanos_to_secs(self.time_ns as f64)
    }

    fn timestamp_ms(&self) -> u64 {
        nanos_to_millis(self.time_ns)
    }

    fn set_time(&mut self, to_time_ns: u64) {
        self.time_ns = to_time_ns
    }

    fn advance_time(&mut self, to_time_ns: u64) -> Vec<(Vec<TimeEvent>, &PyObject)> {
        // time should increase monotonically
        assert!(
            self.next_time_ns >= to_time_ns,
            "Time to advance to should be greater than current clock time"
        );

        let events = self
            .timers
            .iter_mut()
            .filter(|(_, timer)| !timer.is_expired)
            .map(|(name_id, timer)| {
                let handler = self.handlers.get(name_id).unwrap_or(&self.default_handler);
                let events: Vec<TimeEvent> = timer.advance(to_time_ns).collect();
                (events, handler)
            })
            .collect();

        // update next event time for clock with
        // minimum next event time between all timers
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

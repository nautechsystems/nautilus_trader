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

use super::timer::{NameID, TestTimer};
use nautilus_core::datetime::{nanos_to_millis, nanos_to_secs};
use nautilus_core::time::{Timedelta, Timestamp};
use pyo3::prelude::*;

#[allow(dead_code)]
struct TestClock {
    time_ns: Timestamp,
    next_time_ns: Timestamp,
    timers: HashMap<NameID, TestTimer>,
    handlers: HashMap<NameID, PyObject>,
    default_handler: PyObject,
}

#[allow(dead_code)]
impl TestClock {
    fn new(initial_ns: Timestamp, default_handler: PyObject) -> TestClock {
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

    fn timestamp_ms(&self) -> i64 {
        nanos_to_millis(self.time_ns)
    }

    fn set_time(&mut self, to_time_ns: Timestamp) {
        self.time_ns = to_time_ns
    }

    fn advance_time(&mut self, to_time_ns: Timestamp) -> Vec<(Vec<TimeEvent>, &PyObject)> {
        // Time should increase monotonically
        assert!(
            to_time_ns >= self.time_ns,
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
    fn register_default_handler(&mut self, handler: PyObject);
    fn set_time_alert_ns(
        &mut self,
        name: NameID,
        alert_time_ns: Timestamp,
        callback: Option<PyObject>,
    );
    fn set_timer_ns(
        &mut self,
        name: NameID,
        interval_ns: Timedelta,
        start_time_ns: Timestamp,
        stop_time_ns: Timestamp,
        callback: Option<PyObject>,
    );
}

impl Clock for TestClock {
    fn register_default_handler(&mut self, handler: PyObject) {
        self.default_handler = handler
    }

    fn set_time_alert_ns(
        &mut self,
        name: NameID,
        alert_time_ns: Timestamp,
        callback: Option<PyObject>,
    ) {
        let callback = callback.unwrap_or_else(|| self.default_handler.clone());
        let timer = TestTimer::new(
            name,
            alert_time_ns - self.time_ns,
            self.time_ns,
            Some(alert_time_ns),
        );
        self.timers.insert(name, timer);
        self.handlers.insert(name, callback);
    }

    fn set_timer_ns(
        &mut self,
        name: NameID,
        interval_ns: Timedelta,
        start_time_ns: Timestamp,
        stop_time_ns: Timestamp,
        callback: Option<PyObject>,
    ) {
        let callback = callback.unwrap_or_else(|| self.default_handler.clone());
        let timer = TestTimer::new(name, interval_ns, start_time_ns, Some(stop_time_ns));
        self.timers.insert(name, timer);
        self.handlers.insert(name, callback);
    }
}

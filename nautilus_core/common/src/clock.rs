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

use std::{collections::HashMap, ops::Deref};

use nautilus_core::{
    correctness::check_valid_string,
    time::{get_atomic_clock_realtime, AtomicTime, UnixNanos},
};
use ustr::Ustr;

use crate::{
    handlers::EventHandler,
    timer::{LiveTimer, TestTimer, TimeEvent, TimeEventHandler},
};

/// Represents a type of clock.
///
/// # Notes
/// An active timer is one which has not expired (`timer.is_expired == False`).
pub trait Clock {
    /// Return the names of active timers in the clock.
    fn timer_names(&self) -> Vec<&str>;

    /// Return the count of active timers in the clock.
    fn timer_count(&self) -> usize;

    /// Register a default event handler for the clock. If a `Timer`
    /// does not have an event handler, then this handler is used.
    fn register_default_handler(&mut self, callback: EventHandler);

    /// Set a `Timer` to alert at a particular time. Optional
    /// callback gets used to handle generated events.
    fn set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: UnixNanos,
        callback: Option<EventHandler>,
    );

    /// Set a `Timer` to start alerting at every interval
    /// between start and stop time. Optional callback gets
    /// used to handle generated event.
    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<EventHandler>,
    );

    fn next_time_ns(&self, name: &str) -> UnixNanos;
    fn cancel_timer(&mut self, name: &str);
    fn cancel_timers(&mut self);
}

pub struct TestClock {
    time: AtomicTime,
    timers: HashMap<Ustr, TestTimer>,
    default_callback: Option<EventHandler>,
    callbacks: HashMap<Ustr, EventHandler>,
}

impl TestClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: AtomicTime::new(false, 0),
            timers: HashMap::new(),
            default_callback: None,
            callbacks: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_timers(&self) -> &HashMap<Ustr, TestTimer> {
        &self.timers
    }

    pub fn advance_time(&mut self, to_time_ns: UnixNanos, set_time: bool) -> Vec<TimeEvent> {
        // Time should increase monotonically
        assert!(
            to_time_ns >= self.time.get_time_ns(),
            "`to_time_ns` was < `self.time.get_time_ns()`"
        );

        if set_time {
            self.time.set_time(to_time_ns);
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
    #[must_use]
    pub fn match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandler> {
        events
            .into_iter()
            .map(|event| {
                let handler = self.callbacks.get(&event.name).cloned().unwrap_or_else(|| {
                    // If callback_py is None, use the default_callback_py
                    // TODO: clone for now
                    self.default_callback.clone().unwrap()
                });
                create_time_event_handler(event, &handler)
            })
            .collect()
    }
}

#[cfg(not(feature = "python"))]
fn create_time_event_handler(_event: TimeEvent, _handler: &EventHandler) -> TimeEventHandler {
    panic!("`python` feature is not enabled")
}

#[cfg(feature = "python")]
fn create_time_event_handler(event: TimeEvent, handler: &EventHandler) -> TimeEventHandler {
    use std::ffi::c_char;

    TimeEventHandler {
        event,
        callback_ptr: handler.callback.as_ptr() as *mut c_char,
    }
}

impl Default for TestClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for TestClock {
    type Target = AtomicTime;

    fn deref(&self) -> &Self::Target {
        &self.time
    }
}

impl Clock for TestClock {
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

    fn register_default_handler(&mut self, callback: EventHandler) {
        self.default_callback = Some(callback);
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: UnixNanos,
        callback: Option<EventHandler>,
    ) {
        check_valid_string(name, stringify!(name)).unwrap();
        assert!(
            callback.is_some() | self.default_callback.is_some(),
            "All Python callbacks were `None`"
        );

        let name_ustr = Ustr::from(name);
        match callback {
            Some(callback_py) => self.callbacks.insert(name_ustr, callback_py),
            None => None,
        };

        // TODO: should the atomic clock be shared
        // currently share timestamp nanoseconds
        let time_ns = self.time.get_time_ns();
        let timer = TestTimer::new(name, alert_time_ns - time_ns, time_ns, Some(alert_time_ns));
        self.timers.insert(name_ustr, timer);
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<EventHandler>,
    ) {
        check_valid_string(name, "name").unwrap();
        assert!(
            callback.is_some() | self.default_callback.is_some(),
            "All Python callbacks were `None`"
        );

        let name_ustr = Ustr::from(name);
        match callback {
            Some(callback_py) => self.callbacks.insert(name_ustr, callback_py),
            None => None,
        };

        let timer = TestTimer::new(name, interval_ns, start_time_ns, stop_time_ns);
        self.timers.insert(name_ustr, timer);
    }

    fn next_time_ns(&self, name: &str) -> UnixNanos {
        let timer = self.timers.get(&Ustr::from(name));
        match timer {
            None => 0,
            Some(timer) => timer.next_time_ns,
        }
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        match timer {
            None => {}
            Some(mut timer) => timer.cancel(),
        }
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }
        self.timers = HashMap::new();
    }
}

pub struct LiveClock {
    time: &'static AtomicTime,
    timers: HashMap<Ustr, LiveTimer>,
    default_callback: Option<EventHandler>,
}

impl LiveClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: get_atomic_clock_realtime(),
            timers: HashMap::new(),
            default_callback: None,
        }
    }

    #[must_use]
    pub fn get_timers(&self) -> &HashMap<Ustr, LiveTimer> {
        &self.timers
    }
}

impl Default for LiveClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for LiveClock {
    type Target = AtomicTime;

    fn deref(&self) -> &Self::Target {
        self.time
    }
}

impl Clock for LiveClock {
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

    fn register_default_handler(&mut self, handler: EventHandler) {
        self.default_callback = Some(handler);
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        mut alert_time_ns: UnixNanos,
        callback: Option<EventHandler>,
    ) {
        check_valid_string(name, stringify!(name)).unwrap();
        assert!(
            callback.is_some() | self.default_callback.is_some(),
            "All Python callbacks were `None`"
        );

        let callback = match callback {
            Some(callback) => callback,
            None => self.default_callback.clone().unwrap(),
        };

        let ts_now = self.get_time_ns();
        alert_time_ns = std::cmp::max(alert_time_ns, ts_now);
        let mut timer = LiveTimer::new(
            name,
            alert_time_ns - ts_now,
            ts_now,
            Some(alert_time_ns),
            callback,
        );
        timer.start();
        self.timers.insert(Ustr::from(name), timer);
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<EventHandler>,
    ) {
        check_valid_string(name, stringify!(name)).unwrap();
        assert!(
            callback.is_some() | self.default_callback.is_some(),
            "All Python callbacks were `None`"
        );

        let callback = match callback {
            Some(callback) => callback,
            None => self.default_callback.clone().unwrap(),
        };

        let mut timer = LiveTimer::new(name, interval_ns, start_time_ns, stop_time_ns, callback);
        timer.start();
        self.timers.insert(Ustr::from(name), timer);
    }

    fn next_time_ns(&self, name: &str) -> UnixNanos {
        let timer = self.timers.get(&Ustr::from(name));
        match timer {
            None => 0,
            Some(timer) => timer.next_time_ns,
        }
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        match timer {
            None => {}
            Some(mut timer) => timer.cancel(),
        }
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }
        self.timers = HashMap::new();
    }
}

// TODO: Rust specific clock tests

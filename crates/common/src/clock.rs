// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Real-time and static `Clock` implementations.

use std::{
    collections::{BTreeMap, BinaryHeap, HashMap},
    fmt::Debug,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use chrono::{DateTime, Utc};
use futures::Stream;
use nautilus_core::{
    AtomicTime, UnixNanos,
    correctness::{check_positive_u64, check_predicate_true, check_valid_string},
    time::get_atomic_clock_realtime,
};
use tokio::sync::Mutex;
use ustr::Ustr;

use crate::timer::{
    LiveTimer, TestTimer, TimeEvent, TimeEventCallback, TimeEventHandlerV2, create_valid_interval,
};

/// Represents a type of clock.
///
/// # Notes
///
/// An active timer is one which has not expired (`timer.is_expired == False`).
pub trait Clock: Debug {
    /// Returns the current date and time as a timezone-aware `DateTime<UTC>`.
    fn utc_now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(self.timestamp_ns().as_i64())
    }

    /// Returns the current UNIX timestamp in nanoseconds (ns).
    fn timestamp_ns(&self) -> UnixNanos;

    /// Returns the current UNIX timestamp in microseconds (μs).
    fn timestamp_us(&self) -> u64;

    /// Returns the current UNIX timestamp in milliseconds (ms).
    fn timestamp_ms(&self) -> u64;

    /// Returns the current UNIX timestamp in seconds.
    fn timestamp(&self) -> f64;

    /// Returns the names of active timers in the clock.
    fn timer_names(&self) -> Vec<&str>;

    /// Returns the count of active timers in the clock.
    fn timer_count(&self) -> usize;

    /// Register a default event handler for the clock. If a `Timer`
    /// does not have an event handler, then this handler is used.
    fn register_default_handler(&mut self, callback: TimeEventCallback);

    /// Get handler for [`TimeEvent`].
    ///
    /// Note: Panics if the event does not have an associated handler
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandlerV2;

    /// Set a `Timer` to alert at the specified time.
    /// Optional callback gets used to handle generated events.
    ///
    /// # Errors
    ///
    /// Returns an error if `name` is invalid, `alert_time_ns` is non-positive when not allowed,
    /// or any predicate check fails.
    #[allow(clippy::too_many_arguments)]
    fn set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: UnixNanos,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()>;

    /// Set a `Timer` to fire time events at every interval
    /// between start and stop time. Optional callback gets
    /// used to handle the generated events.
    ///
    /// | `allow_past` | `fire_immediately` | Behavior                                                                              |
    /// |--------------|--------------------|---------------------------------------------------------------------------------------|
    /// | `true`       | `true`             | First event fires immediately at start time, even if start time is in the past.       |
    /// | `true`       | `false`            | First event fires at start_time + interval, even if start time is in the past.        |
    /// | `false`      | `true`             | Returns error if start_time is in the past (first event would be immediate but past). |
    /// | `false`      | `false`            | Returns error if start_time + interval is in the past.                                |
    ///
    /// # Errors
    ///
    /// Returns an error if `name` is invalid, `interval_ns` is not positive,
    /// or if any predicate check fails.
    #[allow(clippy::too_many_arguments)]
    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()>;

    /// Returns the time interval in which the timer `name` is triggered.
    ///
    /// If the timer doesn't exist `None` is returned.
    fn next_time_ns(&self, name: &str) -> Option<UnixNanos>;

    /// Cancels the timer with `name`.
    fn cancel_timer(&mut self, name: &str);

    /// Cancels all timers.
    fn cancel_timers(&mut self);

    /// Resets the clock by clearing it's internal state.
    fn reset(&mut self);
}

/// A static test clock.
///
/// Stores the current timestamp internally which can be advanced.
#[derive(Debug)]
pub struct TestClock {
    time: AtomicTime,
    // Use btree map to ensure stable ordering when scanning for timers in `advance_time`
    timers: BTreeMap<Ustr, TestTimer>,
    default_callback: Option<TimeEventCallback>,
    callbacks: HashMap<Ustr, TimeEventCallback>,
    heap: BinaryHeap<TimeEvent>,
}

impl TestClock {
    /// Creates a new [`TestClock`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: AtomicTime::new(false, UnixNanos::default()),
            timers: BTreeMap::new(),
            default_callback: None,
            callbacks: HashMap::new(),
            heap: BinaryHeap::new(),
        }
    }

    /// Returns a reference to the internal timers for the clock.
    #[must_use]
    pub const fn get_timers(&self) -> &BTreeMap<Ustr, TestTimer> {
        &self.timers
    }

    /// Advances the internal clock to the specified `to_time_ns` and optionally sets the clock to that time.
    ///
    /// This function ensures that the clock behaves in a non-decreasing manner. If `set_time` is `true`,
    /// the internal clock will be updated to the value of `to_time_ns`. Otherwise, the clock will advance
    /// without explicitly setting the time.
    ///
    /// The method processes active timers, advancing them to `to_time_ns`, and collects any `TimeEvent`
    /// objects that are triggered as a result. Only timers that are not expired are processed.
    ///
    /// # Panics
    ///
    /// Panics if `to_time_ns` is less than the current internal clock time.
    pub fn advance_time(&mut self, to_time_ns: UnixNanos, set_time: bool) -> Vec<TimeEvent> {
        // Time should be non-decreasing
        assert!(
            to_time_ns >= self.time.get_time_ns(),
            "`to_time_ns` {to_time_ns} was < `self.time.get_time_ns()` {}",
            self.time.get_time_ns()
        );

        if set_time {
            self.time.set_time(to_time_ns);
        }

        // Iterate and advance timers and collect events. Only retain alive timers.
        let mut events: Vec<TimeEvent> = Vec::new();
        self.timers.retain(|_, timer| {
            timer.advance(to_time_ns).for_each(|event| {
                events.push(event);
            });

            !timer.is_expired()
        });

        events.sort_by(|a, b| a.ts_event.cmp(&b.ts_event));
        events
    }

    /// Advances the internal clock to the specified `to_time_ns` and optionally sets the clock to that time.
    ///
    /// Pushes the [`TimeEvent`]s on the heap to ensure ordering
    ///
    /// Note: `set_time` is not used but present to keep backward compatible api call
    ///
    /// # Panics
    ///
    /// Panics if `to_time_ns` is less than the current internal clock time.
    pub fn advance_to_time_on_heap(&mut self, to_time_ns: UnixNanos) {
        // Time should be non-decreasing
        assert!(
            to_time_ns >= self.time.get_time_ns(),
            "`to_time_ns` {to_time_ns} was < `self.time.get_time_ns()` {}",
            self.time.get_time_ns()
        );

        self.time.set_time(to_time_ns);

        // Iterate and advance timers and push events to heap. Only retain alive timers.
        self.timers.retain(|_, timer| {
            timer.advance(to_time_ns).for_each(|event| {
                self.heap.push(event);
            });

            !timer.is_expired()
        });
    }

    /// Matches `TimeEvent` objects with their corresponding event handlers.
    ///
    /// This function takes an `events` vector of `TimeEvent` objects, assumes they are already sorted
    /// by their `ts_event`, and matches them with the appropriate callback handler from the internal
    /// registry of callbacks. If no specific callback is found for an event, the default callback is used.
    ///
    /// # Panics
    ///
    /// Panics if the default callback is not set for the clock when matching handlers.
    #[must_use]
    pub fn match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandlerV2> {
        events
            .into_iter()
            .map(|event| {
                let callback = self.callbacks.get(&event.name).cloned().unwrap_or_else(|| {
                    // If callback_py is None, use the default_callback_py
                    // TODO: clone for now
                    self.default_callback
                        .clone()
                        .expect("Default callback should exist")
                });
                TimeEventHandlerV2::new(event, callback)
            })
            .collect()
    }
}

impl Iterator for TestClock {
    type Item = TimeEventHandlerV2;

    fn next(&mut self) -> Option<Self::Item> {
        self.heap.pop().map(|event| self.get_handler(event))
    }
}

impl Default for TestClock {
    /// Creates a new default [`TestClock`] instance.
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
    fn timestamp_ns(&self) -> UnixNanos {
        self.time.get_time_ns()
    }

    fn timestamp_us(&self) -> u64 {
        self.time.get_time_us()
    }

    fn timestamp_ms(&self) -> u64 {
        self.time.get_time_ms()
    }

    fn timestamp(&self) -> f64 {
        self.time.get_time()
    }

    fn timer_names(&self) -> Vec<&str> {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    fn timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .count()
    }

    fn register_default_handler(&mut self, callback: TimeEventCallback) {
        self.default_callback = Some(callback);
    }

    /// Returns the handler for the given `TimeEvent`.
    ///
    /// # Panics
    ///
    /// Panics if no event-specific or default callback has been registered for the event.
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandlerV2 {
        // Get the callback from either the event-specific callbacks or default callback
        let callback = self
            .callbacks
            .get(&event.name)
            .cloned()
            .or_else(|| self.default_callback.clone())
            .unwrap_or_else(|| panic!("Event '{}' should have associated handler", event.name));

        TimeEventHandlerV2::new(event, callback)
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        mut alert_time_ns: UnixNanos, // mut allows adjustment based on allow_past
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);

        check_predicate_true(
            callback.is_some()
                | self.callbacks.contains_key(&name)
                | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        match callback {
            Some(callback_py) => self.callbacks.insert(name, callback_py),
            None => None,
        };

        // This allows to reuse a time alert without updating the callback, for example for non regular monthly alerts
        self.cancel_timer(name.as_str());

        let ts_now = self.get_time_ns();

        if alert_time_ns < ts_now {
            if allow_past {
                alert_time_ns = ts_now;
                log::warn!(
                    "Timer '{name}' alert time {} was in the past, adjusted to current time for immediate firing",
                    alert_time_ns.to_rfc3339(),
                );
            } else {
                anyhow::bail!(
                    "Timer '{name}' alert time {} was in the past (current time is {})",
                    alert_time_ns.to_rfc3339(),
                    ts_now.to_rfc3339(),
                );
            }
        }

        // Safe to calculate interval now that we've ensured alert_time_ns >= ts_now
        let interval_ns = create_valid_interval((alert_time_ns - ts_now).into());

        let timer = TestTimer::new(name, interval_ns, ts_now, Some(alert_time_ns), false);
        self.timers.insert(name, timer);

        Ok(())
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;
        check_positive_u64(interval_ns, stringify!(interval_ns))?;
        check_predicate_true(
            callback.is_some() | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);
        let fire_immediately = fire_immediately.unwrap_or(false);

        match callback {
            Some(callback_py) => self.callbacks.insert(name, callback_py),
            None => None,
        };

        let mut start_time_ns = start_time_ns;
        let ts_now = self.get_time_ns();

        if start_time_ns == 0 {
            // Zero start time indicates no explicit start; we use the current time
            start_time_ns = self.timestamp_ns();
        } else if !allow_past {
            // Calculate the next event time based on fire_immediately flag
            let next_event_time = if fire_immediately {
                start_time_ns
            } else {
                start_time_ns + interval_ns
            };

            // Check if the next event would be in the past
            if next_event_time < ts_now {
                anyhow::bail!(
                    "Timer '{name}' next event time {} would be in the past (current time is {})",
                    next_event_time.to_rfc3339(),
                    ts_now.to_rfc3339(),
                );
            }
        }

        if let Some(stop_time) = stop_time_ns {
            if stop_time <= start_time_ns {
                anyhow::bail!(
                    "Timer '{name}' stop time {} must be after start time {}",
                    stop_time.to_rfc3339(),
                    start_time_ns.to_rfc3339(),
                );
            }
        }

        let interval_ns = create_valid_interval(interval_ns);

        let timer = TestTimer::new(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            fire_immediately,
        );
        self.timers.insert(name, timer);

        Ok(())
    }

    fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        self.timers
            .get(&Ustr::from(name))
            .map(|timer| timer.next_time_ns())
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        if let Some(mut timer) = timer {
            timer.cancel();
        }
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }

        self.timers.clear();
    }

    fn reset(&mut self) {
        self.time = AtomicTime::new(false, UnixNanos::default());
        self.timers = BTreeMap::new();
        self.heap = BinaryHeap::new();
        self.callbacks = HashMap::new();
    }
}

/// A real-time clock which uses system time.
///
/// Timestamps are guaranteed to be unique and monotonically increasing.
#[derive(Debug)]
pub struct LiveClock {
    time: &'static AtomicTime,
    timers: HashMap<Ustr, LiveTimer>,
    default_callback: Option<TimeEventCallback>,
    pub heap: Arc<Mutex<BinaryHeap<TimeEvent>>>,
    #[allow(dead_code)]
    callbacks: HashMap<Ustr, TimeEventCallback>,
}

impl LiveClock {
    /// Creates a new [`LiveClock`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: get_atomic_clock_realtime(),
            timers: HashMap::new(),
            default_callback: None,
            heap: Arc::new(Mutex::new(BinaryHeap::new())),
            callbacks: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_event_stream(&self) -> TimeEventStream {
        TimeEventStream::new(self.heap.clone())
    }

    #[must_use]
    pub const fn get_timers(&self) -> &HashMap<Ustr, LiveTimer> {
        &self.timers
    }

    // Clean up expired timers. Retain only live ones
    fn clear_expired_timers(&mut self) {
        self.timers.retain(|_, timer| !timer.is_expired());
    }
}

impl Default for LiveClock {
    /// Creates a new default [`LiveClock`] instance.
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
    fn timestamp_ns(&self) -> UnixNanos {
        self.time.get_time_ns()
    }

    fn timestamp_us(&self) -> u64 {
        self.time.get_time_us()
    }

    fn timestamp_ms(&self) -> u64 {
        self.time.get_time_ms()
    }

    fn timestamp(&self) -> f64 {
        self.time.get_time()
    }

    fn timer_names(&self) -> Vec<&str> {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    fn timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .count()
    }

    fn register_default_handler(&mut self, handler: TimeEventCallback) {
        self.default_callback = Some(handler);
    }

    /// # Panics
    ///
    /// This function panics if:
    /// - The `clock_v2` feature is not enabled.
    /// - The event does not have an associated handler (see trait documentation).
    #[allow(unused_variables)]
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandlerV2 {
        #[cfg(not(feature = "clock_v2"))]
        panic!("Cannot get live clock handler without 'clock_v2' feature");

        // Get the callback from either the event-specific callbacks or default callback
        #[cfg(feature = "clock_v2")]
        {
            let callback = self
                .callbacks
                .get(&event.name)
                .cloned()
                .or_else(|| self.default_callback.clone())
                .unwrap_or_else(|| panic!("Event '{}' should have associated handler", event.name));

            TimeEventHandlerV2::new(event, callback)
        }
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        mut alert_time_ns: UnixNanos, // mut allows adjustment based on allow_past
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);

        check_predicate_true(
            callback.is_some()
                | self.callbacks.contains_key(&name)
                | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        #[cfg(feature = "clock_v2")]
        {
            match callback.clone() {
                Some(callback) => self.callbacks.insert(name, callback),
                None => None,
            };
        }

        let callback = match callback {
            Some(callback) => callback,
            None => {
                if self.callbacks.contains_key(&name) {
                    self.callbacks.get(&name).unwrap().clone()
                } else {
                    self.default_callback.clone().unwrap()
                }
            }
        };

        // This allows to reuse a time alert without updating the callback, for example for non regular monthly alerts
        self.cancel_timer(name.as_str());

        let ts_now = self.get_time_ns();

        // Handle past timestamps based on flag
        if alert_time_ns < ts_now {
            if allow_past {
                alert_time_ns = ts_now;
                log::warn!(
                    "Timer '{name}' alert time {} was in the past, adjusted to current time for immediate firing",
                    alert_time_ns.to_rfc3339(),
                );
            } else {
                anyhow::bail!(
                    "Timer '{name}' alert time {} was in the past (current time is {})",
                    alert_time_ns.to_rfc3339(),
                    ts_now.to_rfc3339(),
                );
            }
        }

        // Safe to calculate interval now that we've ensured alert_time_ns >= ts_now
        let interval_ns = create_valid_interval((alert_time_ns - ts_now).into());

        #[cfg(not(feature = "clock_v2"))]
        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            ts_now,
            Some(alert_time_ns),
            callback,
            false,
        );

        #[cfg(feature = "clock_v2")]
        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            ts_now,
            Some(alert_time_ns),
            callback,
            self.heap.clone(),
            false,
        );

        timer.start();

        self.clear_expired_timers();
        self.timers.insert(name, timer);

        Ok(())
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;
        check_positive_u64(interval_ns, stringify!(interval_ns))?;
        check_predicate_true(
            callback.is_some() | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);
        let fire_immediately = fire_immediately.unwrap_or(false);

        let callback = match callback {
            Some(callback) => callback,
            None => self.default_callback.clone().unwrap(),
        };

        #[cfg(feature = "clock_v2")]
        {
            self.callbacks.insert(name, callback.clone());
        }

        let mut start_time_ns = start_time_ns;
        let ts_now = self.get_time_ns();

        if start_time_ns == 0 {
            // Zero start time indicates no explicit start; we use the current time
            start_time_ns = self.timestamp_ns();
        } else if start_time_ns < ts_now && !allow_past {
            anyhow::bail!(
                "Timer '{name}' start time {} was in the past (current time is {})",
                start_time_ns.to_rfc3339(),
                ts_now.to_rfc3339(),
            );
        }

        if let Some(stop_time) = stop_time_ns {
            if stop_time <= start_time_ns {
                anyhow::bail!(
                    "Timer '{name}' stop time {} must be after start time {}",
                    stop_time.to_rfc3339(),
                    start_time_ns.to_rfc3339(),
                );
            }
        }

        let interval_ns = create_valid_interval(interval_ns);

        #[cfg(not(feature = "clock_v2"))]
        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            callback,
            fire_immediately,
        );

        #[cfg(feature = "clock_v2")]
        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            callback,
            self.heap.clone(),
            fire_immediately,
        );
        timer.start();

        self.clear_expired_timers();
        self.timers.insert(name, timer);

        Ok(())
    }

    fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        self.timers
            .get(&Ustr::from(name))
            .map(|timer| timer.next_time_ns())
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        if let Some(mut timer) = timer {
            timer.cancel();
        }
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }

        self.timers.clear();
    }

    fn reset(&mut self) {
        self.timers = HashMap::new();
        self.heap = Arc::new(Mutex::new(BinaryHeap::new()));
        self.callbacks = HashMap::new();
    }
}

// Helper struct to stream events from the heap
#[derive(Debug)]
pub struct TimeEventStream {
    heap: Arc<Mutex<BinaryHeap<TimeEvent>>>,
}

impl TimeEventStream {
    pub const fn new(heap: Arc<Mutex<BinaryHeap<TimeEvent>>>) -> Self {
        Self { heap }
    }
}

impl Stream for TimeEventStream {
    type Item = TimeEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut heap = match self.heap.try_lock() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::error!("Unable to get LiveClock heap lock: {e}");
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };

        if let Some(event) = heap.pop() {
            Poll::Ready(Some(event))
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use rstest::{fixture, rstest};

    use super::*;

    #[derive(Default)]
    struct TestCallback {
        called: Rc<RefCell<bool>>,
    }

    impl TestCallback {
        const fn new(called: Rc<RefCell<bool>>) -> Self {
            Self { called }
        }
    }

    impl From<TestCallback> for TimeEventCallback {
        fn from(callback: TestCallback) -> Self {
            Self::Rust(Rc::new(move |_event: TimeEvent| {
                *callback.called.borrow_mut() = true;
            }))
        }
    }

    #[fixture]
    pub fn test_clock() -> TestClock {
        let mut clock = TestClock::new();
        clock.register_default_handler(TestCallback::default().into());
        clock
    }

    #[rstest]
    fn test_time_monotonicity(mut test_clock: TestClock) {
        let initial_time = test_clock.timestamp_ns();
        test_clock.advance_time((*initial_time + 1000).into(), true);
        assert!(test_clock.timestamp_ns() > initial_time);
    }

    #[rstest]
    fn test_timer_registration(mut test_clock: TestClock) {
        test_clock
            .set_time_alert_ns(
                "test_timer",
                (*test_clock.timestamp_ns() + 1000).into(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_names(), vec!["test_timer"]);
    }

    #[rstest]
    fn test_timer_expiration(mut test_clock: TestClock) {
        let alert_time = (*test_clock.timestamp_ns() + 1000).into();
        test_clock
            .set_time_alert_ns("test_timer", alert_time, None, None)
            .unwrap();
        let events = test_clock.advance_time(alert_time, true);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name.as_str(), "test_timer");
    }

    #[rstest]
    fn test_timer_cancellation(mut test_clock: TestClock) {
        test_clock
            .set_time_alert_ns(
                "test_timer",
                (*test_clock.timestamp_ns() + 1000).into(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(test_clock.timer_count(), 1);
        test_clock.cancel_timer("test_timer");
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_time_advancement(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        test_clock
            .set_timer_ns("test_timer", 1000, start_time, None, None, None, None)
            .unwrap();
        let events = test_clock.advance_time((*start_time + 2500).into(), true);
        assert_eq!(events.len(), 2);
        assert_eq!(*events[0].ts_event, *start_time + 1000);
        assert_eq!(*events[1].ts_event, *start_time + 2000);
    }

    #[rstest]
    fn test_default_and_custom_callbacks() {
        let mut clock = TestClock::new();
        let default_called = Rc::new(RefCell::new(false));
        let custom_called = Rc::new(RefCell::new(false));

        let default_callback = TestCallback::new(Rc::clone(&default_called));
        let custom_callback = TestCallback::new(Rc::clone(&custom_called));

        clock.register_default_handler(TimeEventCallback::from(default_callback));
        clock
            .set_time_alert_ns(
                "default_timer",
                (*clock.timestamp_ns() + 1000).into(),
                None,
                None,
            )
            .unwrap();
        clock
            .set_time_alert_ns(
                "custom_timer",
                (*clock.timestamp_ns() + 1000).into(),
                Some(TimeEventCallback::from(custom_callback)),
                None,
            )
            .unwrap();

        let events = clock.advance_time((*clock.timestamp_ns() + 1000).into(), true);
        let handlers = clock.match_handlers(events);

        for handler in handlers {
            handler.callback.call(handler.event);
        }

        assert!(*default_called.borrow());
        assert!(*custom_called.borrow());
    }

    #[rstest]
    fn test_multiple_timers(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        test_clock
            .set_timer_ns("timer1", 1000, start_time, None, None, None, None)
            .unwrap();
        test_clock
            .set_timer_ns("timer2", 2000, start_time, None, None, None, None)
            .unwrap();
        let events = test_clock.advance_time((*start_time + 2000).into(), true);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].name.as_str(), "timer1");
        assert_eq!(events[1].name.as_str(), "timer1");
        assert_eq!(events[2].name.as_str(), "timer2");
    }

    #[rstest]
    fn test_allow_past_parameter_true(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(2000));
        let current_time = test_clock.timestamp_ns();
        let past_time = UnixNanos::from(current_time.as_u64() - 1000);

        // With allow_past=true (default), should adjust to current time and succeed
        test_clock
            .set_time_alert_ns("past_timer", past_time, None, Some(true))
            .unwrap();

        // Verify timer was created with adjusted time
        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_names(), vec!["past_timer"]);

        // Next time should be at or after current time, not in the past
        let next_time = test_clock.next_time_ns("past_timer").unwrap();
        assert!(next_time >= current_time);
    }

    #[rstest]
    fn test_allow_past_parameter_false(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(2000));
        let current_time = test_clock.timestamp_ns();
        let past_time = current_time - 1000;

        // With allow_past=false, should fail for past times
        let result = test_clock.set_time_alert_ns("past_timer", past_time, None, Some(false));

        // Verify the operation failed with appropriate error
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("was in the past"));

        // Verify no timer was created
        assert_eq!(test_clock.timer_count(), 0);
        assert!(test_clock.timer_names().is_empty());
    }

    #[rstest]
    fn test_invalid_stop_time_validation(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(2000));
        let current_time = test_clock.timestamp_ns();
        let start_time = current_time + 1000;
        let stop_time = current_time + 500; // Stop time before start time

        // Should fail because stop_time < start_time
        let result = test_clock.set_timer_ns(
            "invalid_timer",
            100,
            start_time,
            Some(stop_time),
            None,
            None,
            None,
        );

        // Verify the operation failed with appropriate error
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("must be after start time"));

        // Verify no timer was created
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_set_timer_ns_fire_immediately_true(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        let interval_ns = 1000;

        test_clock
            .set_timer_ns(
                "fire_immediately_timer",
                interval_ns,
                start_time,
                None,
                None,
                None,
                Some(true),
            )
            .unwrap();

        // Advance time to check immediate firing and subsequent intervals
        let events = test_clock.advance_time((start_time + 2500).into(), true);

        // Should fire immediately at start_time (0), then at start_time+1000, then at start_time+2000
        assert_eq!(events.len(), 3);
        assert_eq!(*events[0].ts_event, *start_time); // Fires immediately
        assert_eq!(*events[1].ts_event, *start_time + 1000); // Then after interval
        assert_eq!(*events[2].ts_event, *start_time + 2000); // Then after second interval
    }

    #[rstest]
    fn test_set_timer_ns_fire_immediately_false(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        let interval_ns = 1000;

        test_clock
            .set_timer_ns(
                "normal_timer",
                interval_ns,
                start_time,
                None,
                None,
                None,
                Some(false),
            )
            .unwrap();

        // Advance time to check normal behavior
        let events = test_clock.advance_time((start_time + 2500).into(), true);

        // Should fire after first interval, not immediately
        assert_eq!(events.len(), 2);
        assert_eq!(*events[0].ts_event, *start_time + 1000); // Fires after first interval
        assert_eq!(*events[1].ts_event, *start_time + 2000); // Then after second interval
    }

    #[rstest]
    fn test_set_timer_ns_fire_immediately_default_is_false(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        let interval_ns = 1000;

        // Don't specify fire_immediately (should default to false)
        test_clock
            .set_timer_ns(
                "default_timer",
                interval_ns,
                start_time,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let events = test_clock.advance_time((start_time + 1500).into(), true);

        // Should behave the same as fire_immediately=false
        assert_eq!(events.len(), 1);
        assert_eq!(*events[0].ts_event, *start_time + 1000); // Fires after first interval
    }

    #[rstest]
    fn test_set_timer_ns_fire_immediately_with_zero_start_time(mut test_clock: TestClock) {
        test_clock.set_time(5000.into());
        let interval_ns = 1000;

        test_clock
            .set_timer_ns(
                "zero_start_timer",
                interval_ns,
                0.into(),
                None,
                None,
                None,
                Some(true),
            )
            .unwrap();

        let events = test_clock.advance_time(7000.into(), true);

        // With zero start time, should use current time as start
        // Fire immediately at current time (5000), then at 6000, 7000
        assert_eq!(events.len(), 3);
        assert_eq!(*events[0].ts_event, 5000); // Immediate fire at current time
        assert_eq!(*events[1].ts_event, 6000);
        assert_eq!(*events[2].ts_event, 7000);
    }

    #[rstest]
    fn test_multiple_timers_different_fire_immediately_settings(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        let interval_ns = 1000;

        // One timer with fire_immediately=true
        test_clock
            .set_timer_ns(
                "immediate_timer",
                interval_ns,
                start_time,
                None,
                None,
                None,
                Some(true),
            )
            .unwrap();

        // One timer with fire_immediately=false
        test_clock
            .set_timer_ns(
                "normal_timer",
                interval_ns,
                start_time,
                None,
                None,
                None,
                Some(false),
            )
            .unwrap();

        let events = test_clock.advance_time((start_time + 1500).into(), true);

        // Should have 3 events total: immediate_timer fires at start & 1000, normal_timer fires at 1000
        assert_eq!(events.len(), 3);

        // Sort events by timestamp to check order
        let mut event_times: Vec<u64> = events.iter().map(|e| e.ts_event.as_u64()).collect();
        event_times.sort();

        assert_eq!(event_times[0], start_time.as_u64()); // immediate_timer fires immediately
        assert_eq!(event_times[1], start_time.as_u64() + 1000); // both timers fire at 1000
        assert_eq!(event_times[2], start_time.as_u64() + 1000); // both timers fire at 1000
    }

    #[rstest]
    fn test_timer_name_collision_overwrites(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        // Set first timer
        test_clock
            .set_timer_ns("collision_timer", 1000, start_time, None, None, None, None)
            .unwrap();

        // Setting timer with same name should overwrite the existing one
        let result =
            test_clock.set_timer_ns("collision_timer", 2000, start_time, None, None, None, None);

        assert!(result.is_ok());
        // Should still only have one timer (overwritten)
        assert_eq!(test_clock.timer_count(), 1);

        // The timer should have the new interval
        let next_time = test_clock.next_time_ns("collision_timer").unwrap();
        // With interval 2000 and start at start_time, next time should be start_time + 2000
        assert_eq!(next_time, start_time + 2000);
    }

    #[rstest]
    fn test_timer_zero_interval_error(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        // Attempt to set timer with zero interval should fail
        let result =
            test_clock.set_timer_ns("zero_interval", 0, start_time, None, None, None, None);

        assert!(result.is_err());
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_timer_empty_name_error(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        // Attempt to set timer with empty name should fail
        let result = test_clock.set_timer_ns("", 1000, start_time, None, None, None, None);

        assert!(result.is_err());
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_timer_fire_immediately_at_exact_stop_time(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        let interval_ns = 1000;
        let stop_time = start_time + interval_ns; // Stop exactly at first interval

        test_clock
            .set_timer_ns(
                "exact_stop",
                interval_ns,
                start_time,
                Some(stop_time.into()),
                None,
                None,
                Some(true),
            )
            .unwrap();

        let events = test_clock.advance_time(stop_time.into(), true);

        // Should fire immediately at start, then at stop time (which equals first interval)
        assert_eq!(events.len(), 2);
        assert_eq!(*events[0].ts_event, *start_time); // Immediate fire
        assert_eq!(*events[1].ts_event, *stop_time); // Fire at stop time
    }

    #[rstest]
    fn test_timer_advance_to_exact_next_time(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        let interval_ns = 1000;

        test_clock
            .set_timer_ns(
                "exact_advance",
                interval_ns,
                start_time,
                None,
                None,
                None,
                Some(false),
            )
            .unwrap();

        // Advance to exactly the next fire time
        let next_time = test_clock.next_time_ns("exact_advance").unwrap();
        let events = test_clock.advance_time(next_time, true);

        assert_eq!(events.len(), 1);
        assert_eq!(*events[0].ts_event, *next_time);
    }

    #[rstest]
    fn test_allow_past_bar_aggregation_use_case(mut test_clock: TestClock) {
        // Simulate bar aggregation scenario: current time is in middle of a bar window
        test_clock.set_time(UnixNanos::from(100_500)); // 100.5 seconds

        let bar_start_time = UnixNanos::from(100_000); // 100 seconds (0.5 sec ago)
        let interval_ns = 1000; // 1 second bars

        // With allow_past=false and fire_immediately=false:
        // start_time is in past (100 sec) but next event (101 sec) is in future
        // This should be ALLOWED for bar aggregation
        let result = test_clock.set_timer_ns(
            "bar_timer",
            interval_ns,
            bar_start_time,
            None,
            None,
            Some(false), // allow_past = false
            Some(false), // fire_immediately = false
        );

        // Should succeed because next event time (100_000 + 1000 = 101_000) > current time (100_500)
        assert!(result.is_ok());
        assert_eq!(test_clock.timer_count(), 1);

        // Next event should be at bar_start_time + interval = 101_000
        let next_time = test_clock.next_time_ns("bar_timer").unwrap();
        assert_eq!(*next_time, 101_000);
    }

    #[rstest]
    fn test_allow_past_false_rejects_when_next_event_in_past(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(102_000)); // 102 seconds

        let past_start_time = UnixNanos::from(100_000); // 100 seconds (2 sec ago)
        let interval_ns = 1000; // 1 second interval

        // With allow_past=false and fire_immediately=false:
        // Next event would be 100_000 + 1000 = 101_000, which is < current time (102_000)
        // This should be REJECTED
        let result = test_clock.set_timer_ns(
            "past_event_timer",
            interval_ns,
            past_start_time,
            None,
            None,
            Some(false), // allow_past = false
            Some(false), // fire_immediately = false
        );

        // Should fail because next event time (101_000) < current time (102_000)
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("would be in the past")
        );
    }

    #[rstest]
    fn test_allow_past_false_with_fire_immediately_true(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(100_500)); // 100.5 seconds

        let past_start_time = UnixNanos::from(100_000); // 100 seconds (0.5 sec ago)
        let interval_ns = 1000;

        // With fire_immediately=true, next event = start_time (which is in past)
        // This should be REJECTED with allow_past=false
        let result = test_clock.set_timer_ns(
            "immediate_past_timer",
            interval_ns,
            past_start_time,
            None,
            None,
            Some(false), // allow_past = false
            Some(true),  // fire_immediately = true
        );

        // Should fail because next event time (100_000) < current time (100_500)
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("would be in the past")
        );
    }

    #[rstest]
    fn test_cancel_timer_during_execution(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        test_clock
            .set_timer_ns("cancel_test", 1000, start_time, None, None, None, None)
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);

        // Cancel the timer
        test_clock.cancel_timer("cancel_test");

        assert_eq!(test_clock.timer_count(), 0);

        // Advance time - should get no events from cancelled timer
        let events = test_clock.advance_time((start_time + 2000).into(), true);
        assert_eq!(events.len(), 0);
    }

    #[rstest]
    fn test_cancel_all_timers(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        // Create multiple timers
        test_clock
            .set_timer_ns("timer1", 1000, start_time, None, None, None, None)
            .unwrap();
        test_clock
            .set_timer_ns("timer2", 1500, start_time, None, None, None, None)
            .unwrap();
        test_clock
            .set_timer_ns("timer3", 2000, start_time, None, None, None, None)
            .unwrap();

        assert_eq!(test_clock.timer_count(), 3);

        // Cancel all timers
        test_clock.cancel_timers();

        assert_eq!(test_clock.timer_count(), 0);

        // Advance time - should get no events
        let events = test_clock.advance_time((start_time + 5000).into(), true);
        assert_eq!(events.len(), 0);
    }

    #[rstest]
    fn test_clock_reset_clears_timers(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        test_clock
            .set_timer_ns("reset_test", 1000, start_time, None, None, None, None)
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);

        // Reset the clock
        test_clock.reset();

        assert_eq!(test_clock.timer_count(), 0);
        assert_eq!(test_clock.timestamp_ns(), UnixNanos::default()); // Time reset to zero
    }
}

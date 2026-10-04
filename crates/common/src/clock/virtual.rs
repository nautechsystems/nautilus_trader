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

//! Deterministic clock for controlled time advancement.

use std::{
    collections::{BTreeMap, BinaryHeap},
    ops::Deref,
};

use nautilus_core::{
    AtomicTime, DurationNanos, UUID4, UnixNanos, correctness::check_predicate_true,
    string::formatting::Separable,
};
use ustr::Ustr;

use super::{
    CallbackRegistry, Clock, replace_existing_timer, validate_and_prepare_time_alert,
    validate_and_prepare_timer,
};
use crate::timer::{
    ScheduledTimeEvent, TimeEvent, TimeEventCallback, TimeEventHandler, VirtualTimer,
    create_valid_interval,
};

/// A deterministic clock for controlled time advancement.
///
/// The clock stores a manual timestamp, schedules [`VirtualTimer`] instances, and returns due events
/// without waiting for wall-clock time.
///
/// # Threading
///
/// This clock is thread-affine; use it only from the thread that created it.
#[derive(Debug)]
pub struct VirtualClock {
    time: AtomicTime,
    timers: BTreeMap<Ustr, VirtualTimer>,
    pub(super) timer_queue: BinaryHeap<ScheduledTimeEvent>,
    callbacks: CallbackRegistry,
}

impl VirtualClock {
    /// Creates a virtual clock at the UNIX epoch with no timers or callbacks.
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: AtomicTime::new(false, UnixNanos::default()),
            timers: BTreeMap::new(),
            timer_queue: BinaryHeap::new(),
            callbacks: CallbackRegistry::new(),
        }
    }

    /// Advances active timers through `to_time_ns` and returns their due events.
    ///
    /// Events at `to_time_ns` are included and returned in ascending order by event timestamp and
    /// timer name. If `set_time` is `true`, the stored clock timestamp is also set to `to_time_ns`;
    /// otherwise the stored timestamp remains unchanged.
    ///
    /// # Warnings
    ///
    /// Logs a warning if at least 1,000,000 time events are allocated during advancement.
    ///
    /// # Panics
    ///
    /// Panics if `to_time_ns` is less than the current internal clock time.
    pub fn advance_time(&mut self, to_time_ns: UnixNanos, set_time: bool) -> Vec<TimeEvent> {
        const WARN_TIME_EVENTS_THRESHOLD: usize = 1_000_000;

        let from_time_ns = self.time.get_time_ns();

        assert!(
            to_time_ns >= from_time_ns,
            "Invariant: time must be non-decreasing, `to_time_ns` {to_time_ns} < `from_time_ns` {from_time_ns}"
        );

        if set_time {
            self.time.set_time(to_time_ns);
        }

        let mut events: Vec<TimeEvent> = Vec::new();

        while self
            .timer_queue
            .peek()
            .is_some_and(|entry| entry.0.ts_event <= to_time_ns)
        {
            let entry = self
                .timer_queue
                .pop()
                .expect("timer queue peeked Some but pop returned None");

            let Some((event, next_event)) = self.advance_timer_from_entry(&entry.0) else {
                continue;
            };

            events.push(event);

            if let Some(next_event) = next_event {
                self.timer_queue.push(next_event);
            }
        }

        self.compact_timer_queue_if_needed();

        if events.len() >= WARN_TIME_EVENTS_THRESHOLD {
            log::warn!(
                "Allocated {} time events during clock advancement from {} to {}, \
                 consider stopping the timer between large time ranges with no data points",
                events.len().separate_with_commas(),
                from_time_ns,
                to_time_ns
            );
        }

        events.sort_by(|a, b| {
            a.ts_event
                .cmp(&b.ts_event)
                .then_with(|| a.name.cmp(&b.name))
        });

        events
    }

    /// Matches time events with their registered callbacks, preserving input order.
    ///
    /// A named callback takes precedence over the default callback for each event.
    ///
    /// # Panics
    ///
    /// Panics if an event has neither a named nor default callback.
    #[must_use]
    pub fn match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandler> {
        events
            .into_iter()
            .map(|event| self.callbacks.get_handler(event))
            .collect()
    }

    fn replace_existing_timer_if_needed(&mut self, name: &Ustr) {
        replace_existing_timer(&mut self.timers, name);
        self.compact_timer_queue_if_needed();
    }

    fn insert_timer(&mut self, timer: VirtualTimer) {
        self.timer_queue.push(Self::scheduled_event(&timer));
        self.timers.insert(timer.name, timer);
        self.compact_timer_queue_if_needed();
    }

    fn advance_timer_from_entry(
        &mut self,
        entry: &TimeEvent,
    ) -> Option<(TimeEvent, Option<ScheduledTimeEvent>)> {
        let timer = self.timers.get_mut(&entry.name)?;
        if timer.next_time_ns() != entry.ts_event {
            return None;
        }

        let Some((event, _)) = timer.next() else {
            self.timers.remove(&entry.name);
            return None;
        };

        let next_entry = if timer.is_expired() {
            self.timers.remove(&entry.name);
            None
        } else {
            Some(Self::scheduled_event(timer))
        };

        Some((event, next_entry))
    }

    fn compact_timer_queue_if_needed(&mut self) {
        if self.timer_queue.len() > self.timers.len().saturating_mul(2) {
            self.compact_timer_queue();
        }
    }

    fn compact_timer_queue(&mut self) {
        self.timer_queue = self.timers.values().map(Self::scheduled_event).collect();
    }

    fn scheduled_event(timer: &VirtualTimer) -> ScheduledTimeEvent {
        ScheduledTimeEvent::new(TimeEvent::new(
            timer.name,
            UUID4::new(),
            timer.next_time_ns(),
            timer.next_time_ns(),
        ))
    }
}

impl Default for VirtualClock {
    /// Creates a new default [`VirtualClock`] instance.
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for VirtualClock {
    type Target = AtomicTime;

    fn deref(&self) -> &Self::Target {
        &self.time
    }
}

impl Clock for VirtualClock {
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

    fn timer_exists(&self, name: &Ustr) -> bool {
        self.timers
            .get(name)
            .is_some_and(|timer| !timer.is_expired())
    }

    fn register_default_handler(&mut self, callback: TimeEventCallback) {
        self.callbacks.register_default_handler(callback);
    }

    fn cancel_default_handler(&mut self) {
        self.callbacks.cancel_default_handler();
    }

    fn cancel_callbacks(&mut self) {
        self.callbacks.clear();
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: UnixNanos,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        let ts_now = self.get_time_ns();
        let (name, alert_time_ns) =
            validate_and_prepare_time_alert(name, alert_time_ns, allow_past, ts_now)?;

        check_predicate_true(
            callback.is_some() | self.callbacks.has_any_callback(&name),
            "No callbacks provided",
        )?;

        self.replace_existing_timer_if_needed(&name);

        if let Some(callback) = callback {
            self.callbacks.register_callback(name, callback);
        }

        // Safe to calculate interval now that we've ensured alert_time_ns >= ts_now
        let interval_ns = create_valid_interval(alert_time_ns - ts_now);
        let fire_immediately = alert_time_ns == ts_now;

        let timer = VirtualTimer::new(
            name,
            interval_ns,
            ts_now,
            Some(alert_time_ns),
            fire_immediately,
        );
        self.insert_timer(timer);

        Ok(())
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: DurationNanos,
        start_time_ns: Option<UnixNanos>,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()> {
        let ts_now = self.get_time_ns();
        let (name, start_time_ns, stop_time_ns, _allow_past, fire_immediately) =
            validate_and_prepare_timer(
                name,
                interval_ns,
                start_time_ns,
                stop_time_ns,
                allow_past,
                fire_immediately,
                ts_now,
            )?;

        check_predicate_true(
            callback.is_some() | self.callbacks.has_any_callback(&name),
            "No callbacks provided",
        )?;

        self.replace_existing_timer_if_needed(&name);

        if let Some(callback) = callback {
            self.callbacks.register_callback(name, callback);
        }

        let interval_ns = create_valid_interval(interval_ns);

        let timer = VirtualTimer::new(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            fire_immediately,
        );
        self.insert_timer(timer);

        Ok(())
    }

    fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        self.timers
            .get(&Ustr::from(name))
            .filter(|timer| !timer.is_expired())
            .map(VirtualTimer::next_time_ns)
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        if let Some(mut timer) = timer {
            timer.cancel();
        }

        self.compact_timer_queue_if_needed();
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }

        self.timers.clear();
        self.timer_queue.clear();
    }

    fn reset(&mut self) {
        self.time = AtomicTime::new(false, UnixNanos::default());
        self.timers = BTreeMap::new();
        self.timer_queue = BinaryHeap::new();
        self.callbacks.clear();
    }
}

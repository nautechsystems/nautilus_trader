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

//! Real-time and static `Clock` implementations.
//!
//! Defines the [`Clock`] contract, the user-facing [`ClockApi`] facade, and the deterministic
//! [`TestClock`] used for controlled time advancement. Shared validation and callback registration
//! support test and live clock implementations.

use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BinaryHeap},
    fmt::Debug,
    ops::Deref,
    time::Duration,
};

use ahash::AHashMap;
use jiff::Timestamp;
use nautilus_core::{
    AtomicTime, UUID4, UnixNanos,
    correctness::{check_positive_u64, check_predicate_true, check_valid_string_utf8},
    datetime::{NANOSECONDS_IN_SECOND, try_datetime_to_unix_nanos},
    string::formatting::Separable,
};
use ustr::Ustr;

use crate::timer::{
    ScheduledTimeEvent, TestTimer, TimeEvent, TimeEventCallback, TimeEventHandler, Timer,
    create_valid_interval,
};

/// Provides time access, timer scheduling, and callback registration.
///
/// An active timer is one that has not expired.
pub trait Clock: Debug + Any {
    /// Returns the current UTC timestamp.
    fn utc_now(&self) -> Timestamp {
        self.timestamp_ns().to_datetime_utc()
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

    /// Returns whether an active timer named `name` exists.
    fn timer_exists(&self, name: &Ustr) -> bool;

    /// Registers the callback used when a timer has no named callback.
    fn register_default_handler(&mut self, callback: TimeEventCallback);

    /// Cancels the registered default event handler, if any.
    ///
    /// Releases the held callback so any Python object owned by it can be dropped.
    /// `Trader::release_component` calls this at component retirement to break the cycle
    /// between a Python component and its clock: the clock holds the callback as a
    /// `Py<PyAny>` that Python's cycle collector cannot reach through.
    fn cancel_default_handler(&mut self);

    /// Cancels all registered named event callbacks, preserving the default handler.
    ///
    /// Releases callbacks registered via [`Clock::set_time_alert_ns`] or
    /// [`Clock::set_timer_ns`] with an explicit `callback` argument.
    /// `Trader::release_component` calls this at component retirement, breaking the same
    /// cycle as [`Clock::cancel_default_handler`].
    fn cancel_callbacks(&mut self);

    /// Sets a timer to alert at the specified time.
    ///
    /// See [`Clock::set_time_alert_ns`] for flag semantics.
    ///
    /// # Callback
    ///
    /// - `Some(callback)` registers and uses `callback` for the named alert.
    /// - `None` uses a callback registered under `name`, falling back to the default callback.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `name` is invalid.
    /// - `alert_time` is before the UNIX epoch or outside the [`UnixNanos`] range.
    /// - The alert is in the past and `allow_past` is `Some(false)`.
    /// - No explicit, named, or default callback is available.
    fn set_time_alert(
        &mut self,
        name: &str,
        alert_time: Timestamp,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        self.set_time_alert_ns(
            name,
            try_datetime_to_unix_nanos(alert_time)?,
            callback,
            allow_past,
        )
    }

    /// Sets a timer to alert at the specified time.
    ///
    /// Any active timer registered under the same `name` is canceled with a warning before the
    /// new alert is scheduled. `allow_past` defaults to `true`.
    ///
    /// # Flags
    ///
    /// | `allow_past` | Behavior                                                               |
    /// | ------------ | ---------------------------------------------------------------------- |
    /// | `true`       | A past alert is moved to the current time and fires immediately.       |
    /// | `false`      | An alert earlier than the current time returns an error.               |
    ///
    /// # Callback
    ///
    /// - `Some(callback)` registers and uses `callback` for the named alert.
    /// - `None` uses a callback registered under `name`, falling back to the default callback.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `name` is invalid.
    /// - `alert_time_ns` is earlier than now and `allow_past` is `Some(false)`.
    /// - No explicit, named, or default callback is available.
    fn set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: UnixNanos,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()>;

    /// Sets a timer to fire time events at every interval between the start and stop times.
    ///
    /// Any active timer registered under the same `name` is canceled with a warning before the
    /// new timer is scheduled.
    ///
    /// See [`Clock::set_timer_ns`] for flag semantics.
    ///
    /// # Callback
    ///
    /// - `Some(callback)` registers and uses `callback` for the named timer.
    /// - `None` uses a callback registered under `name`, falling back to the default callback.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `name` is invalid.
    /// - `interval` is zero or exceeds `u64::MAX` nanoseconds.
    /// - `start_time` or `stop_time` is before the UNIX epoch or out of range for `UnixNanos`.
    /// - The first event timestamp is out of range for `UnixNanos`.
    /// - The first event is in the past when past times are disallowed.
    /// - The stop time is not after the start time.
    /// - The stop time is not after the current time when past times are disallowed.
    /// - No explicit, named, or default callback is available.
    #[expect(clippy::too_many_arguments)]
    fn set_timer(
        &mut self,
        name: &str,
        interval: Duration,
        start_time: Option<Timestamp>,
        stop_time: Option<Timestamp>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()> {
        self.set_timer_ns(
            name,
            duration_to_nanos(interval)?,
            start_time.map(try_datetime_to_unix_nanos).transpose()?,
            stop_time.map(try_datetime_to_unix_nanos).transpose()?,
            callback,
            allow_past,
            fire_immediately,
        )
    }

    /// Sets a timer to fire time events at every interval between the start and stop times.
    ///
    /// Any active timer registered under the same `name` is canceled with a warning before the
    /// new timer is scheduled. `allow_past` defaults to `true`, and `fire_immediately` defaults to
    /// `false`.
    ///
    /// # Start Time
    ///
    /// - `None` or `Some(0)`: Uses the current time as start time.
    /// - `Some(non_zero)`: Uses the specified timestamp as start time.
    ///
    /// # Flags
    ///
    /// | `allow_past` | `fire_immediately` | First event behavior                                |
    /// | ------------ | ------------------ | --------------------------------------------------- |
    /// | `true`       | `true`             | Fires at the start time, including a past start.    |
    /// | `true`       | `false`            | Fires one interval after the start, including past. |
    /// | `false`      | `true`             | A past start time returns an error.                 |
    /// | `false`      | `false`            | A past first event returns an error.                |
    ///
    /// # Callback
    ///
    /// - `Some(callback)` registers and uses `callback` for the named timer.
    /// - `None` uses a callback registered under `name`, falling back to the default callback.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `name` is invalid.
    /// - `interval_ns` is zero.
    /// - `start_time_ns + interval_ns` is out of range for `UnixNanos` when not firing immediately.
    /// - The first event is in the past when past times are disallowed.
    /// - The stop time is not after the start time.
    /// - The stop time is not after the current time when past times are disallowed.
    /// - No explicit, named, or default callback is available.
    #[expect(clippy::too_many_arguments)]
    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: Option<UnixNanos>,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()>;

    /// Returns the next trigger timestamp for the active timer named `name`.
    ///
    /// Returns `None` if no active timer with that name exists.
    fn next_time_ns(&self, name: &str) -> Option<UnixNanos>;

    /// Cancels the timer named `name`, if it exists.
    fn cancel_timer(&mut self, name: &str);

    /// Cancels all timers.
    fn cancel_timers(&mut self);

    /// Resets scheduling state while preserving the default callback.
    ///
    /// The reset clears all timers and named callbacks. Static clocks also reset their stored time.
    fn reset(&mut self);
}

impl dyn Clock {
    /// Returns a reference to this clock as `Any` for downcasting.
    pub fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Returns a mutable reference to this clock as `Any` for downcasting.
    pub fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Provides a user-facing facade over clock operations.
///
/// Calls delegate to either a borrowed [`Clock`] or a set of operation handlers.
/// Panics from supplied operation handlers propagate to the caller.
#[derive(Debug)]
pub struct ClockApi<'a> {
    backing: ClockApiBacking<'a>,
}

impl<'a> ClockApi<'a> {
    pub(crate) fn new(clock: &'a RefCell<dyn Clock>) -> Self {
        Self {
            backing: ClockApiBacking::Native(clock),
        }
    }

    /// Creates a clock API backed by the supplied operation handlers.
    ///
    /// The nanosecond timestamp handler also supplies the derived UTC, second, millisecond, and
    /// microsecond values. Timestamp-based scheduling methods convert their inputs before invoking
    /// the corresponding nanosecond handler.
    #[doc(hidden)]
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "clock API backing mirrors the full ClockApi surface"
    )]
    pub fn from_handlers<
        TimestampNs,
        SetTimeAlertNs,
        SetTimerNs,
        TimerNames,
        TimerCount,
        TimerExists,
        NextTimeNs,
        CancelTimer,
        CancelTimers,
    >(
        timestamp_ns: TimestampNs,
        set_time_alert_ns: SetTimeAlertNs,
        set_timer_ns: SetTimerNs,
        timer_names: TimerNames,
        timer_count: TimerCount,
        timer_exists: TimerExists,
        next_time_ns: NextTimeNs,
        cancel_timer: CancelTimer,
        cancel_timers: CancelTimers,
    ) -> Self
    where
        TimestampNs: Fn() -> UnixNanos + 'a,
        SetTimeAlertNs:
            Fn(&str, UnixNanos, Option<TimeEventCallback>, Option<bool>) -> anyhow::Result<()> + 'a,
        SetTimerNs: Fn(
                &str,
                u64,
                Option<UnixNanos>,
                Option<UnixNanos>,
                Option<TimeEventCallback>,
                Option<bool>,
                Option<bool>,
            ) -> anyhow::Result<()>
            + 'a,
        TimerNames: Fn() -> Vec<String> + 'a,
        TimerCount: Fn() -> usize + 'a,
        TimerExists: Fn(&str) -> bool + 'a,
        NextTimeNs: Fn(&str) -> Option<UnixNanos> + 'a,
        CancelTimer: Fn(&str) + 'a,
        CancelTimers: Fn() + 'a,
    {
        Self {
            backing: ClockApiBacking::Handlers(ClockApiHandlers {
                timestamp_ns: Box::new(timestamp_ns),
                set_time_alert_ns: Box::new(set_time_alert_ns),
                set_timer_ns: Box::new(set_timer_ns),
                timer_names: Box::new(timer_names),
                timer_count: Box::new(timer_count),
                timer_exists: Box::new(timer_exists),
                next_time_ns: Box::new(next_time_ns),
                cancel_timer: Box::new(cancel_timer),
                cancel_timers: Box::new(cancel_timers),
            }),
        }
    }

    /// Returns the current UNIX timestamp in nanoseconds.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn timestamp_ns(&self) -> UnixNanos {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().timestamp_ns(),
            ClockApiBacking::Handlers(handlers) => (handlers.timestamp_ns)(),
        }
    }

    /// Returns the current UNIX timestamp in microseconds.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn timestamp_us(&self) -> u64 {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().timestamp_us(),
            ClockApiBacking::Handlers(handlers) => (handlers.timestamp_ns)().as_micros(),
        }
    }

    /// Returns the current UNIX timestamp in milliseconds.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn timestamp_ms(&self) -> u64 {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().timestamp_ms(),
            ClockApiBacking::Handlers(handlers) => (handlers.timestamp_ns)().as_millis(),
        }
    }

    /// Returns the current UNIX timestamp in seconds.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn timestamp(&self) -> f64 {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().timestamp(),
            ClockApiBacking::Handlers(handlers) => {
                (handlers.timestamp_ns)().as_f64() / (NANOSECONDS_IN_SECOND as f64)
            }
        }
    }

    /// Returns the current UTC timestamp.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn utc_now(&self) -> Timestamp {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().utc_now(),
            ClockApiBacking::Handlers(handlers) => (handlers.timestamp_ns)().to_datetime_utc(),
        }
    }

    // panics-doc-ok
    /// Sets a time alert for the specified UTC timestamp.
    ///
    /// See [`Clock::set_time_alert`] for timing and callback selection semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the timestamp cannot be converted to [`UnixNanos`] or the backing clock
    /// rejects the alert.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already borrowed.
    pub fn set_time_alert(
        &self,
        name: &str,
        alert_time: Timestamp,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock
                .borrow_mut()
                .set_time_alert(name, alert_time, callback, allow_past),
            ClockApiBacking::Handlers(handlers) => (handlers.set_time_alert_ns)(
                name,
                try_datetime_to_unix_nanos(alert_time)?,
                callback,
                allow_past,
            ),
        }
    }

    // panics-doc-ok
    /// Sets a time alert for the specified UNIX nanosecond timestamp.
    ///
    /// See [`Clock::set_time_alert_ns`] for timing and callback selection semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing clock rejects the alert.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already borrowed.
    pub fn set_time_alert_ns(
        &self,
        name: &str,
        alert_time_ns: UnixNanos,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        match &self.backing {
            ClockApiBacking::Native(clock) => {
                clock
                    .borrow_mut()
                    .set_time_alert_ns(name, alert_time_ns, callback, allow_past)
            }
            ClockApiBacking::Handlers(handlers) => {
                (handlers.set_time_alert_ns)(name, alert_time_ns, callback, allow_past)
            }
        }
    }

    // panics-doc-ok
    /// Sets an interval timer using UTC timestamps.
    ///
    /// See [`Clock::set_timer`] for scheduling and callback selection semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the interval exceeds `u64::MAX` nanoseconds, a timestamp cannot be
    /// converted to [`UnixNanos`], or the backing clock rejects the timer.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already borrowed.
    #[expect(clippy::too_many_arguments, reason = "timer scheduling mirrors Clock")]
    pub fn set_timer(
        &self,
        name: &str,
        interval: Duration,
        start_time: Option<Timestamp>,
        stop_time: Option<Timestamp>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()> {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow_mut().set_timer(
                name,
                interval,
                start_time,
                stop_time,
                callback,
                allow_past,
                fire_immediately,
            ),
            ClockApiBacking::Handlers(handlers) => (handlers.set_timer_ns)(
                name,
                duration_to_nanos(interval)?,
                start_time.map(try_datetime_to_unix_nanos).transpose()?,
                stop_time.map(try_datetime_to_unix_nanos).transpose()?,
                callback,
                allow_past,
                fire_immediately,
            ),
        }
    }

    // panics-doc-ok
    /// Sets an interval timer using UNIX nanosecond timestamps.
    ///
    /// See [`Clock::set_timer_ns`] for scheduling and callback selection semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing clock rejects the timer.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already borrowed.
    #[expect(clippy::too_many_arguments, reason = "timer scheduling mirrors Clock")]
    pub fn set_timer_ns(
        &self,
        name: &str,
        interval_ns: u64,
        start_time_ns: Option<UnixNanos>,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()> {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow_mut().set_timer_ns(
                name,
                interval_ns,
                start_time_ns,
                stop_time_ns,
                callback,
                allow_past,
                fire_immediately,
            ),
            ClockApiBacking::Handlers(handlers) => (handlers.set_timer_ns)(
                name,
                interval_ns,
                start_time_ns,
                stop_time_ns,
                callback,
                allow_past,
                fire_immediately,
            ),
        }
    }

    /// Returns the names of active timers.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn timer_names(&self) -> Vec<String> {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock
                .borrow()
                .timer_names()
                .into_iter()
                .map(str::to_string)
                .collect(),
            ClockApiBacking::Handlers(handlers) => (handlers.timer_names)(),
        }
    }

    /// Returns the count of active timers.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn timer_count(&self) -> usize {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().timer_count(),
            ClockApiBacking::Handlers(handlers) => (handlers.timer_count)(),
        }
    }

    /// Returns whether an active timer named `name` exists.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn timer_exists(&self, name: &str) -> bool {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().timer_exists(&Ustr::from(name)),
            ClockApiBacking::Handlers(handlers) => (handlers.timer_exists)(name),
        }
    }

    /// Returns the next trigger timestamp for the active timer named `name`.
    ///
    /// Returns `None` if no active timer with that name exists.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already mutably borrowed.
    #[must_use]
    pub fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow().next_time_ns(name),
            ClockApiBacking::Handlers(handlers) => (handlers.next_time_ns)(name),
        }
    }

    /// Cancels the timer named `name`, if it exists.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already borrowed.
    pub fn cancel_timer(&self, name: &str) {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow_mut().cancel_timer(name),
            ClockApiBacking::Handlers(handlers) => (handlers.cancel_timer)(name),
        }
    }

    /// Cancels all timers.
    ///
    /// # Panics
    ///
    /// With native backing, panics if the clock is already borrowed.
    pub fn cancel_timers(&self) {
        match &self.backing {
            ClockApiBacking::Native(clock) => clock.borrow_mut().cancel_timers(),
            ClockApiBacking::Handlers(handlers) => (handlers.cancel_timers)(),
        }
    }
}

enum ClockApiBacking<'a> {
    Native(&'a RefCell<dyn Clock>),
    Handlers(ClockApiHandlers<'a>),
}

struct ClockApiHandlers<'a> {
    timestamp_ns: Box<dyn Fn() -> UnixNanos + 'a>,
    set_time_alert_ns: Box<SetTimeAlertNsHandler<'a>>,
    set_timer_ns: Box<SetTimerNsHandler<'a>>,
    timer_names: Box<dyn Fn() -> Vec<String> + 'a>,
    timer_count: Box<dyn Fn() -> usize + 'a>,
    timer_exists: Box<dyn Fn(&str) -> bool + 'a>,
    next_time_ns: Box<NextTimeNsHandler<'a>>,
    cancel_timer: Box<dyn Fn(&str) + 'a>,
    cancel_timers: Box<dyn Fn() + 'a>,
}

impl Debug for ClockApiBacking<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(_) => f.write_str("Native"),
            Self::Handlers(_) => f.write_str("Handlers"),
        }
    }
}

type SetTimeAlertNsHandler<'a> =
    dyn Fn(&str, UnixNanos, Option<TimeEventCallback>, Option<bool>) -> anyhow::Result<()> + 'a;
type NextTimeNsHandler<'a> = dyn Fn(&str) -> Option<UnixNanos> + 'a;
type SetTimerNsHandler<'a> = dyn Fn(
        &str,
        u64,
        Option<UnixNanos>,
        Option<UnixNanos>,
        Option<TimeEventCallback>,
        Option<bool>,
        Option<bool>,
    ) -> anyhow::Result<()>
    + 'a;

fn duration_to_nanos(duration: Duration) -> anyhow::Result<u64> {
    u64::try_from(duration.as_nanos())
        .map_err(|_| anyhow::anyhow!("Interval exceeds u64 nanoseconds"))
}

/// Registry for timer event callbacks.
///
/// Provides shared callback registration and retrieval logic used by both
/// `TestClock` and `LiveClock`.
#[derive(Debug, Default)]
pub struct CallbackRegistry {
    default_callback: Option<TimeEventCallback>,
    callbacks: AHashMap<Ustr, TimeEventCallback>,
}

impl CallbackRegistry {
    /// Creates an empty callback registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the callback used when no callback exists for a timer name.
    pub fn register_default_handler(&mut self, callback: TimeEventCallback) {
        self.default_callback = Some(callback);
    }

    /// Removes the default callback, preserving all named callbacks.
    pub fn cancel_default_handler(&mut self) {
        self.default_callback = None;
    }

    /// Registers a callback for `name`, replacing any existing callback for that name.
    pub fn register_callback(&mut self, name: Ustr, callback: TimeEventCallback) {
        self.callbacks.insert(name, callback);
    }

    /// Returns whether a named or default callback is available for `name`.
    #[must_use]
    pub fn has_any_callback(&self, name: &Ustr) -> bool {
        self.callbacks.contains_key(name) || self.default_callback.is_some()
    }

    /// Returns the callback for `name`, falling back to the default callback.
    #[must_use]
    pub fn get_callback(&self, name: &Ustr) -> Option<TimeEventCallback> {
        self.callbacks
            .get(name)
            .cloned()
            .or_else(|| self.default_callback.clone())
    }

    /// Creates a handler for `event` using its named callback or the default callback.
    ///
    /// # Panics
    ///
    /// Panics if neither a named nor default callback exists for the event.
    #[must_use]
    pub fn get_handler(&self, event: TimeEvent) -> TimeEventHandler {
        let callback = self
            .get_callback(&event.name)
            .unwrap_or_else(|| panic!("Event '{}' should have associated handler", event.name));

        TimeEventHandler::new(event, callback)
    }

    /// Clears all named callbacks, preserving the default callback.
    pub fn clear(&mut self) {
        self.callbacks.clear();
    }
}

/// Validates and normalizes parameters for a time alert.
///
/// `allow_past` defaults to `true`. When enabled, a past alert timestamp is replaced with
/// `ts_now`. Returns the interned name and normalized alert timestamp.
///
/// # Errors
///
/// Returns an error if `name` is invalid or the alert is in the past when past alerts are
/// disallowed.
pub fn validate_and_prepare_time_alert(
    name: &str,
    mut alert_time_ns: UnixNanos,
    allow_past: Option<bool>,
    ts_now: UnixNanos,
) -> anyhow::Result<(Ustr, UnixNanos)> {
    check_valid_string_utf8(name, stringify!(name))?;

    let name = Ustr::from(name);
    let allow_past = allow_past.unwrap_or(true);

    if alert_time_ns < ts_now {
        if allow_past {
            log::warn!(
                "Timer '{name}' alert time {} was in the past, adjusted to current time for immediate firing",
                alert_time_ns.to_rfc3339(),
            );
            alert_time_ns = ts_now;
        } else {
            anyhow::bail!(
                "Timer '{name}' alert time {} was in the past (current time is {ts_now})",
                alert_time_ns.to_rfc3339(),
            );
        }
    }

    Ok((name, alert_time_ns))
}

/// Validates and normalizes parameters for an interval timer.
///
/// A missing or zero `start_time_ns` resolves to `ts_now`. `allow_past` defaults to `true`, and
/// `fire_immediately` defaults to `false`. Returns the interned name, normalized start and stop
/// times, and resolved flag values.
///
/// # Errors
///
/// Returns an error if:
/// - `name` is invalid.
/// - `interval_ns` is zero.
/// - `start_time_ns + interval_ns` is out of range for `UnixNanos` when not firing immediately.
/// - The first event is in the past when past times are disallowed.
/// - The stop time is not after the normalized start time.
/// - The stop time is not after `ts_now` when past times are disallowed.
pub fn validate_and_prepare_timer(
    name: &str,
    interval_ns: u64,
    start_time_ns: Option<UnixNanos>,
    stop_time_ns: Option<UnixNanos>,
    allow_past: Option<bool>,
    fire_immediately: Option<bool>,
    ts_now: UnixNanos,
) -> anyhow::Result<(Ustr, UnixNanos, Option<UnixNanos>, bool, bool)> {
    check_valid_string_utf8(name, stringify!(name))?;
    check_positive_u64(interval_ns, stringify!(interval_ns))?;

    let name = Ustr::from(name);
    let allow_past = allow_past.unwrap_or(true);
    let fire_immediately = fire_immediately.unwrap_or(false);

    let start_time_ns = start_time_ns
        .filter(|start_time_ns| *start_time_ns != 0)
        .unwrap_or(ts_now);

    let next_event_time = if fire_immediately {
        start_time_ns
    } else {
        start_time_ns.checked_add(interval_ns).ok_or_else(|| {
            anyhow::anyhow!("Timer '{name}' first event time exceeds UnixNanos range")
        })?
    };

    if !allow_past && next_event_time < ts_now {
        anyhow::bail!(
            "Timer '{name}' next event time {} would be in the past (current time is {ts_now})",
            next_event_time.to_rfc3339(),
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

        if !allow_past && stop_time <= ts_now {
            anyhow::bail!(
                "Timer '{name}' stop time {} is in the past (current time is {ts_now})",
                stop_time.to_rfc3339(),
            );
        }
    }

    Ok((
        name,
        start_time_ns,
        stop_time_ns,
        allow_past,
        fire_immediately,
    ))
}

/// A deterministic clock for controlled time advancement.
///
/// The clock stores a manual timestamp, schedules [`TestTimer`] instances, and returns due events
/// without waiting for wall-clock time.
///
/// # Threading
///
/// This clock is thread-affine; use it only from the thread that created it.
#[derive(Debug)]
pub struct TestClock {
    time: AtomicTime,
    timers: BTreeMap<Ustr, TestTimer>,
    timer_queue: BinaryHeap<ScheduledTimeEvent>,
    callbacks: CallbackRegistry,
}

impl TestClock {
    /// Creates a test clock at the UNIX epoch with no timers or callbacks.
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

    fn insert_timer(&mut self, timer: TestTimer) {
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

    fn scheduled_event(timer: &TestTimer) -> ScheduledTimeEvent {
        ScheduledTimeEvent::new(TimeEvent::new(
            timer.name,
            UUID4::new(),
            timer.next_time_ns(),
            timer.next_time_ns(),
        ))
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
        let interval_ns = create_valid_interval((alert_time_ns - ts_now).into());
        let fire_immediately = alert_time_ns == ts_now;

        let timer = TestTimer::new(
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
        interval_ns: u64,
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

        let timer = TestTimer::new(
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
            .map(TestTimer::next_time_ns)
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

pub(crate) fn replace_existing_timer<T: Timer>(timers: &mut BTreeMap<Ustr, T>, name: &Ustr) {
    let Some(mut timer) = timers.remove(name) else {
        return;
    };

    if timer.is_expired() {
        return;
    }

    timer.cancel();
    log::warn!("Timer '{name}' replaced");
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use nautilus_core::{MUTEX_POISONED, UnixNanos};
    use proptest::{prelude::*, test_runner::TestCaseResult};
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::*;
    use crate::timer::{TimeEvent, TimeEventCallback};

    #[derive(Debug, Default)]
    struct TestCallback {
        /// Shared flag updated from within the timer callback; Mutex keeps the closure `Send` for tests.
        called: Arc<Mutex<bool>>,
    }

    impl TestCallback {
        fn new(called: Arc<Mutex<bool>>) -> Self {
            Self { called }
        }
    }

    impl From<TestCallback> for TimeEventCallback {
        fn from(callback: TestCallback) -> Self {
            Self::from(move |_event: TimeEvent| {
                if let Ok(mut called) = callback.called.lock() {
                    *called = true;
                }
            })
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
        test_clock.advance_time(UnixNanos::from(*initial_time + 1000), true);
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
            .set_timer_ns("test_timer", 1000, Some(start_time), None, None, None, None)
            .unwrap();
        let events = test_clock.advance_time(UnixNanos::from(*start_time + 2500), true);
        assert_eq!(events.len(), 2);
        assert_eq!(*events[0].ts_event, *start_time + 1000);
        assert_eq!(*events[1].ts_event, *start_time + 2000);
    }

    #[rstest]
    fn test_default_and_custom_callbacks() {
        let mut clock = TestClock::new();
        let default_called = Arc::new(Mutex::new(false));
        let custom_called = Arc::new(Mutex::new(false));

        let default_callback = TestCallback::new(Arc::clone(&default_called));
        let custom_callback = TestCallback::new(Arc::clone(&custom_called));

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

        let events = clock.advance_time(UnixNanos::from(*clock.timestamp_ns() + 1000), true);
        let handlers = clock.match_handlers(events);

        for handler in handlers {
            handler.callback.call(handler.event);
        }

        assert!(*default_called.lock().expect(MUTEX_POISONED));
        assert!(*custom_called.lock().expect(MUTEX_POISONED));
    }

    #[rstest]
    fn test_timer_with_rust_local_callback() {
        use std::{cell::RefCell, rc::Rc};

        let mut clock = TestClock::new();
        let call_count = Rc::new(RefCell::new(0_u32));
        let call_count_clone = Rc::clone(&call_count);

        // Create RustLocal callback using Rc (not Send/Sync)
        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event: TimeEvent| {
            *call_count_clone.borrow_mut() += 1;
        });

        clock
            .set_time_alert_ns(
                "local_timer",
                (*clock.timestamp_ns() + 1000).into(),
                Some(TimeEventCallback::from(callback)),
                None,
            )
            .unwrap();

        let events = clock.advance_time(UnixNanos::from(*clock.timestamp_ns() + 1000), true);
        let handlers = clock.match_handlers(events);

        for handler in handlers {
            handler.callback.call(handler.event);
        }

        assert_eq!(*call_count.borrow(), 1);
    }

    #[rstest]
    fn test_multiple_timers(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        test_clock
            .set_timer_ns("timer1", 1000, Some(start_time), None, None, None, None)
            .unwrap();
        test_clock
            .set_timer_ns("timer2", 2000, Some(start_time), None, None, None, None)
            .unwrap();
        let events = test_clock.advance_time(UnixNanos::from(*start_time + 2000), true);
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
            Some(start_time),
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
                Some(start_time),
                None,
                None,
                None,
                Some(true),
            )
            .unwrap();

        // Advance time to check immediate firing and subsequent intervals
        let events = test_clock.advance_time(start_time + 2500, true);

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
                Some(start_time),
                None,
                None,
                None,
                Some(false),
            )
            .unwrap();

        // Advance time to check normal behavior
        let events = test_clock.advance_time(start_time + 2500, true);

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
                Some(start_time),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let events = test_clock.advance_time(start_time + 1500, true);

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
                None,
                None,
                None,
                None,
                Some(true),
            )
            .unwrap();

        let events = test_clock.advance_time(UnixNanos::from(7000), true);

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
                Some(start_time),
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
                Some(start_time),
                None,
                None,
                None,
                Some(false),
            )
            .unwrap();

        let events = test_clock.advance_time(start_time + 1500, true);

        // Should have 3 events total: immediate_timer fires at start & 1000, normal_timer fires at 1000
        assert_eq!(events.len(), 3);

        // Sort events by timestamp to check order
        let mut event_times: Vec<u64> = events.iter().map(|e| e.ts_event.as_u64()).collect();
        event_times.sort_unstable();

        assert_eq!(event_times[0], start_time.as_u64()); // immediate_timer fires immediately
        assert_eq!(event_times[1], start_time.as_u64() + 1000); // both timers fire at 1000
        assert_eq!(event_times[2], start_time.as_u64() + 1000); // both timers fire at 1000
    }

    #[rstest]
    fn test_timer_name_collision_overwrites(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        // Set first timer
        test_clock
            .set_timer_ns(
                "collision_timer",
                1000,
                Some(start_time),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        // Setting timer with same name should overwrite the existing one
        let result = test_clock.set_timer_ns(
            "collision_timer",
            2000,
            Some(start_time),
            None,
            None,
            None,
            None,
        );

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
            test_clock.set_timer_ns("zero_interval", 0, Some(start_time), None, None, None, None);

        assert!(result.is_err());
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_timer_empty_name_error(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();

        // Attempt to set timer with empty name should fail
        let result = test_clock.set_timer_ns("", 1000, Some(start_time), None, None, None, None);

        assert!(result.is_err());
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_timer_exists(mut test_clock: TestClock) {
        let name = Ustr::from("exists_timer");
        assert!(!test_clock.timer_exists(&name));

        test_clock
            .set_time_alert_ns(
                name.as_str(),
                (*test_clock.timestamp_ns() + 1_000).into(),
                None,
                None,
            )
            .unwrap();

        assert!(test_clock.timer_exists(&name));
    }

    #[rstest]
    fn test_timer_exists_consistent_with_names_and_count_after_expiry(mut test_clock: TestClock) {
        let name = Ustr::from("expiring_timer");
        let start_time = test_clock.timestamp_ns();

        test_clock
            .set_timer_ns(
                name.as_str(),
                1_000,
                Some(start_time),
                Some(start_time + 2_500),
                None,
                None,
                None,
            )
            .unwrap();

        assert!(test_clock.timer_exists(&name));
        assert_eq!(test_clock.timer_count(), 1);

        test_clock.advance_time(start_time + 10_000, true);

        // All three introspection surfaces must agree the timer is gone
        assert!(!test_clock.timer_exists(&name));
        assert_eq!(test_clock.timer_count(), 0);
        assert!(test_clock.timer_names().is_empty());
    }

    #[rstest]
    fn test_timer_rejects_past_stop_time_when_not_allowed(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(10_000));
        let current = test_clock.timestamp_ns();

        let result = test_clock.set_timer_ns(
            "past_stop",
            10_000,
            Some(current - 500),
            Some(current - 100),
            None,
            Some(false),
            None,
        );

        let err = result.expect_err("expected stop time validation error");
        let err_msg = err.to_string();
        assert!(err_msg.contains("stop time"));
        assert!(err_msg.contains("in the past"));
    }

    #[rstest]
    fn test_timer_accepts_future_stop_time(mut test_clock: TestClock) {
        let current = test_clock.timestamp_ns();

        let result = test_clock.set_timer_ns(
            "future_stop",
            1_000,
            Some(current),
            Some(current + 10_000),
            None,
            Some(false),
            None,
        );

        assert!(result.is_ok());
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
                Some(start_time),
                Some(stop_time),
                None,
                None,
                Some(true),
            )
            .unwrap();

        let events = test_clock.advance_time(stop_time, true);

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
                Some(start_time),
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
            Some(bar_start_time),
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
            Some(past_start_time),
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
            Some(past_start_time),
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
            .set_timer_ns(
                "cancel_test",
                1000,
                Some(start_time),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);

        // Cancel the timer
        test_clock.cancel_timer("cancel_test");

        assert_eq!(test_clock.timer_count(), 0);

        // Advance time - should get no events from cancelled timer
        let events = test_clock.advance_time(start_time + 2000, true);
        assert_eq!(events.len(), 0);
    }

    #[rstest]
    fn test_cancelled_timer_queue_entry_is_skipped(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        test_clock
            .set_time_alert_ns("cancelled", start_time + 1000, None, None)
            .unwrap();
        test_clock
            .set_time_alert_ns("active", start_time + 2000, None, None)
            .unwrap();

        test_clock.cancel_timer("cancelled");
        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_queue.len(), 2);

        let events = test_clock.advance_time(start_time + 1000, true);
        assert!(events.is_empty());
        assert_eq!(test_clock.timer_names(), vec!["active"]);

        let events = test_clock.advance_time(start_time + 2000, true);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name.as_str(), "active");
    }

    #[rstest]
    fn test_timer_queue_compacts_stale_entries(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        test_clock
            .set_time_alert_ns("active", start_time + 1000, None, None)
            .unwrap();
        test_clock
            .set_time_alert_ns("cancelled-1", start_time + 2000, None, None)
            .unwrap();
        test_clock
            .set_time_alert_ns("cancelled-2", start_time + 3000, None, None)
            .unwrap();

        test_clock.cancel_timer("cancelled-1");
        assert_eq!(test_clock.timer_queue.len(), 3);

        test_clock.cancel_timer("cancelled-2");
        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_queue.len(), 1);
    }

    #[rstest]
    fn test_cancel_all_timers(mut test_clock: TestClock) {
        // Create multiple timers
        test_clock
            .set_timer_ns("timer1", 1000, None, None, None, None, None)
            .unwrap();
        test_clock
            .set_timer_ns("timer2", 1500, None, None, None, None, None)
            .unwrap();
        test_clock
            .set_timer_ns("timer3", 2000, None, None, None, None, None)
            .unwrap();

        assert_eq!(test_clock.timer_count(), 3);

        // Cancel all timers
        test_clock.cancel_timers();

        assert_eq!(test_clock.timer_count(), 0);

        // Advance time - should get no events
        let events = test_clock.advance_time(UnixNanos::from(5000), true);
        assert_eq!(events.len(), 0);
    }

    #[rstest]
    fn test_clock_reset_clears_timers(mut test_clock: TestClock) {
        test_clock
            .set_timer_ns("reset_test", 1000, None, None, None, None, None)
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);

        // Reset the clock
        test_clock.reset();

        assert_eq!(test_clock.timer_count(), 0);
        assert_eq!(test_clock.timestamp_ns(), UnixNanos::default()); // Time reset to zero
    }

    #[rstest]
    fn test_cancel_default_handler_clears_default(mut test_clock: TestClock) {
        // Default handler is registered by the fixture
        test_clock.cancel_default_handler();

        // Without a default and without an explicit callback, scheduling fails
        let alert_time: UnixNanos = (*test_clock.timestamp_ns() + 1000).into();
        let err = test_clock
            .set_time_alert_ns("alert", alert_time, None, None)
            .unwrap_err();
        assert!(
            err.to_string().contains("No callbacks provided"),
            "unexpected error: {err}"
        );
    }

    #[rstest]
    fn test_cancel_default_handler_is_idempotent_on_empty_registry() {
        // Fresh clock with no handler registered: cancel must not panic
        let mut clock = TestClock::new();
        clock.cancel_default_handler();
        clock.cancel_default_handler();
    }

    #[rstest]
    fn test_cancel_callbacks_clears_named(mut test_clock: TestClock) {
        let alert_time: UnixNanos = (*test_clock.timestamp_ns() + 1000).into();
        let callback = TimeEventCallback::from(TestCallback::default());
        test_clock
            .set_time_alert_ns("named_alert", alert_time, Some(callback), None)
            .unwrap();
        test_clock.cancel_timer("named_alert");

        // Cancel both default and named callbacks; rescheduling without a callback fails
        test_clock.cancel_default_handler();
        test_clock.cancel_callbacks();

        let err = test_clock
            .set_time_alert_ns("named_alert", alert_time, None, None)
            .unwrap_err();
        assert!(
            err.to_string().contains("No callbacks provided"),
            "unexpected error: {err}"
        );
    }

    #[rstest]
    fn test_failed_set_time_alert_ns_preserves_existing_timer() {
        // Fresh clock with no default handler
        let mut clock = TestClock::new();
        let alert_time: UnixNanos = (*clock.timestamp_ns() + 1000).into();
        let callback = TimeEventCallback::from(TestCallback::default());
        clock
            .set_time_alert_ns("alert", alert_time, Some(callback), None)
            .unwrap();
        assert_eq!(clock.next_time_ns("alert"), Some(alert_time));

        // Callbacks released (e.g. partial component teardown) while the alert still lives
        clock.cancel_callbacks();

        // Rescheduling without a callback fails the predicate check; the error
        // return must not have destroyed the previously scheduled alert
        let err = clock
            .set_time_alert_ns("alert", (*alert_time + 1000).into(), None, None)
            .unwrap_err();
        assert!(
            err.to_string().contains("No callbacks provided"),
            "unexpected error: {err}"
        );
        assert_eq!(clock.timer_count(), 1);
        assert_eq!(clock.next_time_ns("alert"), Some(alert_time));
    }

    #[rstest]
    fn test_cancel_default_handler_preserves_named_callbacks(mut test_clock: TestClock) {
        let alert_time: UnixNanos = (*test_clock.timestamp_ns() + 1000).into();
        let callback = TimeEventCallback::from(TestCallback::default());
        test_clock
            .set_time_alert_ns("alert", alert_time, Some(callback), None)
            .unwrap();
        test_clock.cancel_timer("alert");

        test_clock.cancel_default_handler();

        // Named callback survives: rescheduling under the same name without a callback works
        test_clock
            .set_time_alert_ns("alert", alert_time, None, None)
            .unwrap();
    }

    #[rstest]
    fn test_cancel_callbacks_preserves_default_handler(mut test_clock: TestClock) {
        // Default handler from fixture remains available
        test_clock.cancel_callbacks();

        let alert_time: UnixNanos = (*test_clock.timestamp_ns() + 1000).into();
        test_clock
            .set_time_alert_ns("alert", alert_time, None, None)
            .unwrap();
    }

    #[rstest]
    fn test_set_time_alert_default_impl(mut test_clock: TestClock) {
        let current_time = test_clock.utc_now();
        let alert_time = current_time + jiff::SignedDuration::from_secs(1);

        // Test the default implementation that delegates to set_time_alert_ns
        test_clock
            .set_time_alert("alert_test", alert_time, None, None)
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_names(), vec!["alert_test"]);

        // Verify the timer is set for the correct time
        let expected_ns = UnixNanos::from(alert_time);
        let next_time = test_clock.next_time_ns("alert_test").unwrap();

        // Should be very close (within a few nanoseconds due to conversion)
        let diff = if next_time >= expected_ns {
            next_time.as_u64() - expected_ns.as_u64()
        } else {
            expected_ns.as_u64() - next_time.as_u64()
        };
        assert!(
            diff < 1000,
            "Timer should be set within 1 microsecond of expected time"
        );
    }

    #[rstest]
    fn test_set_timer_default_impl(mut test_clock: TestClock) {
        let current_time = test_clock.utc_now();
        let start_time = current_time + jiff::SignedDuration::from_secs(1);
        let interval = Duration::from_millis(500);

        // Test the default implementation that delegates to set_timer_ns
        test_clock
            .set_timer(
                "timer_test",
                interval,
                Some(start_time),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_names(), vec!["timer_test"]);

        // Advance time and verify timer fires at correct intervals
        let start_ns = UnixNanos::from(start_time);
        let interval_ns = interval.as_nanos() as u64;

        let events = test_clock.advance_time(start_ns + interval_ns * 3, true);
        assert_eq!(events.len(), 3); // Should fire 3 times

        // Verify timing
        assert_eq!(*events[0].ts_event, *start_ns + interval_ns);
        assert_eq!(*events[1].ts_event, *start_ns + interval_ns * 2);
        assert_eq!(*events[2].ts_event, *start_ns + interval_ns * 3);
    }

    #[rstest]
    fn test_set_timer_with_stop_time_default_impl(mut test_clock: TestClock) {
        let current_time = test_clock.utc_now();
        let start_time = current_time + jiff::SignedDuration::from_secs(1);
        let stop_time = current_time + jiff::SignedDuration::from_secs(3);
        let interval = Duration::from_secs(1);

        // Test with stop time
        test_clock
            .set_timer(
                "timer_with_stop",
                interval,
                Some(start_time),
                Some(stop_time),
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);

        // Advance beyond stop time
        let stop_ns = UnixNanos::from(stop_time);
        let events = test_clock.advance_time(stop_ns + 1000, true);

        // Should fire twice: at start_time + 1s and start_time + 2s, but not at start_time + 3s since that would be at stop_time
        assert_eq!(events.len(), 2);

        let start_ns = UnixNanos::from(start_time);
        let interval_ns = interval.as_nanos() as u64;
        assert_eq!(*events[0].ts_event, *start_ns + interval_ns);
        assert_eq!(*events[1].ts_event, *start_ns + interval_ns * 2);
    }

    #[rstest]
    fn test_set_timer_fire_immediately_default_impl(mut test_clock: TestClock) {
        let current_time = test_clock.utc_now();
        let start_time = current_time + jiff::SignedDuration::from_secs(1);
        let interval = Duration::from_millis(500);

        // Test with fire_immediately=true
        test_clock
            .set_timer(
                "immediate_timer",
                interval,
                Some(start_time),
                None,
                None,
                None,
                Some(true),
            )
            .unwrap();

        let start_ns = UnixNanos::from(start_time);
        let interval_ns = interval.as_nanos() as u64;

        // Advance to start time + 1 interval
        let events = test_clock.advance_time(start_ns + interval_ns, true);

        // Should fire immediately at start_time, then again at start_time + interval
        assert_eq!(events.len(), 2);
        assert_eq!(*events[0].ts_event, *start_ns); // Immediate fire
        assert_eq!(*events[1].ts_event, *start_ns + interval_ns); // Regular interval
    }

    #[rstest]
    fn test_set_time_alert_when_alert_time_equals_current_time(mut test_clock: TestClock) {
        let current_time = test_clock.timestamp_ns();

        // Set time alert for exactly the current time
        test_clock
            .set_time_alert_ns("alert_at_current_time", current_time, None, None)
            .unwrap();

        assert_eq!(test_clock.timer_count(), 1);

        // Advance time by exactly 0 (to current time) - should fire immediately
        let events = test_clock.advance_time(current_time, true);

        // Should fire immediately since alert_time_ns == ts_now
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name.as_str(), "alert_at_current_time");
        assert_eq!(*events[0].ts_event, *current_time);
    }

    #[rstest]
    fn test_cancel_and_reschedule_same_name(mut test_clock: TestClock) {
        let start = test_clock.timestamp_ns();

        test_clock
            .set_time_alert_ns("timer", UnixNanos::from(*start + 1000), None, None)
            .unwrap();
        assert_eq!(test_clock.timer_count(), 1);

        test_clock.cancel_timer("timer");
        assert_eq!(test_clock.timer_count(), 0);

        test_clock
            .set_time_alert_ns("timer", UnixNanos::from(*start + 2000), None, None)
            .unwrap();
        assert_eq!(test_clock.timer_count(), 1);

        let events = test_clock.advance_time(UnixNanos::from(*start + 1500), true);
        assert!(events.is_empty());

        let events = test_clock.advance_time(UnixNanos::from(*start + 2000), true);
        assert_eq!(events.len(), 1);
        assert_eq!(*events[0].ts_event, *start + 2000);
    }

    #[rstest]
    fn test_multiple_timers_same_timestamp_all_fire(mut test_clock: TestClock) {
        let fire_time = UnixNanos::from(*test_clock.timestamp_ns() + 1000);

        for i in 0..5 {
            test_clock
                .set_time_alert_ns(&format!("timer_{i}"), fire_time, None, None)
                .unwrap();
        }
        assert_eq!(test_clock.timer_count(), 5);

        let events = test_clock.advance_time(fire_time, true);
        assert_eq!(events.len(), 5);
        for event in &events {
            assert_eq!(*event.ts_event, *fire_time);
        }
    }

    #[rstest]
    fn test_events_ordered_by_timestamp_after_advance() {
        let mut clock = TestClock::new();
        clock.register_default_handler(TestCallback::default().into());
        let start = clock.timestamp_ns();

        clock
            .set_time_alert_ns("third", UnixNanos::from(*start + 300), None, None)
            .unwrap();
        clock
            .set_time_alert_ns("first", UnixNanos::from(*start + 100), None, None)
            .unwrap();
        clock
            .set_time_alert_ns("second", UnixNanos::from(*start + 200), None, None)
            .unwrap();

        let events = clock.advance_time(UnixNanos::from(*start + 400), true);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].name.as_str(), "first");
        assert_eq!(events[1].name.as_str(), "second");
        assert_eq!(events[2].name.as_str(), "third");
    }

    #[rstest]
    fn test_large_interval_does_not_overflow(mut test_clock: TestClock) {
        let start = test_clock.timestamp_ns();
        let large_interval: u64 = 1_000_000_000 * 60 * 60 * 24 * 365; // ~1 year in ns

        test_clock
            .set_timer_ns(
                "large_interval",
                large_interval,
                Some(start),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let events = test_clock.advance_time(UnixNanos::from(*start + large_interval), true);
        assert_eq!(events.len(), 1);
        assert_eq!(*events[0].ts_event, *start + large_interval);
    }

    #[rstest]
    fn test_near_zero_interval_fires_correctly(mut test_clock: TestClock) {
        let start = test_clock.timestamp_ns();

        test_clock
            .set_timer_ns("tiny", 1, Some(start), None, None, None, None)
            .unwrap();

        let events = test_clock.advance_time(UnixNanos::from(*start + 10), true);
        assert_eq!(events.len(), 10);

        for i in 1..events.len() {
            assert!(events[i].ts_event >= events[i - 1].ts_event);
        }
    }

    #[rstest]
    fn test_repeated_advance_to_same_time_no_double_fire(mut test_clock: TestClock) {
        let fire_time = UnixNanos::from(*test_clock.timestamp_ns() + 1000);

        test_clock
            .set_time_alert_ns("once", fire_time, None, None)
            .unwrap();

        let events1 = test_clock.advance_time(fire_time, true);
        assert_eq!(events1.len(), 1);

        let events2 = test_clock.advance_time(fire_time, true);
        assert!(events2.is_empty());
    }

    #[rstest]
    fn test_advance_with_no_timers(mut test_clock: TestClock) {
        let start = test_clock.timestamp_ns();

        let events = test_clock.advance_time(UnixNanos::from(*start + 1000), true);
        assert!(events.is_empty());
        assert_eq!(*test_clock.timestamp_ns(), *start + 1000);
    }

    #[rstest]
    fn test_set_time_alert_rejects_unconvertible_datetime(mut test_clock: TestClock) {
        let pre_epoch = Timestamp::from_nanosecond(-1).unwrap();

        let err = test_clock
            .set_time_alert("pre_epoch_alert", pre_epoch, None, None)
            .unwrap_err();
        assert!(
            err.to_string().contains("cannot be negative"),
            "unexpected error: {err}"
        );

        let err = test_clock
            .set_time_alert("out_of_range_alert", Timestamp::MAX, None, None)
            .unwrap_err();
        assert!(
            err.to_string().contains("out of range"),
            "unexpected error: {err}"
        );

        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_set_timer_rejects_unconvertible_datetime(mut test_clock: TestClock) {
        let pre_epoch = Timestamp::from_nanosecond(-1).unwrap();
        let valid_start = test_clock.utc_now() + jiff::SignedDuration::from_secs(1);

        let err = test_clock
            .set_timer(
                "pre_epoch_start",
                Duration::from_secs(1),
                Some(pre_epoch),
                None,
                None,
                None,
                None,
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("cannot be negative"),
            "unexpected error: {err}"
        );

        let err = test_clock
            .set_timer(
                "pre_epoch_stop",
                Duration::from_secs(1),
                Some(valid_start),
                Some(pre_epoch),
                None,
                None,
                None,
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("cannot be negative"),
            "unexpected error: {err}"
        );

        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_set_timer_rejects_interval_exceeding_u64_nanos(mut test_clock: TestClock) {
        let interval = Duration::from_secs(u64::MAX / NANOSECONDS_IN_SECOND + 1);

        let err = test_clock
            .set_timer("overflow", interval, None, None, None, None, None)
            .unwrap_err();

        assert_eq!(err.to_string(), "Interval exceeds u64 nanoseconds");
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_set_timer_ns_rejects_unrepresentable_first_event_without_replacing_timer(
        mut test_clock: TestClock,
    ) {
        test_clock.set_time(UnixNanos::from(1));
        test_clock
            .set_timer_ns("overflow", 1, None, None, None, None, None)
            .unwrap();

        let err = test_clock
            .set_timer_ns("overflow", u64::MAX, None, None, None, None, None)
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Timer 'overflow' first event time exceeds UnixNanos range"
        );
        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(
            test_clock.next_time_ns("overflow"),
            Some(UnixNanos::from(2))
        );
    }

    #[rstest]
    fn test_clock_api_handlers_reject_invalid_time_inputs() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_alert = Arc::clone(&calls);
        let calls_for_timer = Arc::clone(&calls);

        let clock = ClockApi::from_handlers(
            || UnixNanos::from(1_700_000_000_000_000_000),
            move |name, _, _, _| {
                calls_for_alert
                    .lock()
                    .expect(MUTEX_POISONED)
                    .push(name.to_string());
                Ok(())
            },
            move |name, _, _, _, _, _, _| {
                calls_for_timer
                    .lock()
                    .expect(MUTEX_POISONED)
                    .push(name.to_string());
                Ok(())
            },
            Vec::new,
            || 0,
            |_| false,
            |_| None,
            |_| {},
            || {},
        );

        let pre_epoch = Timestamp::from_nanosecond(-1).unwrap();
        clock
            .set_time_alert("alert", pre_epoch, None, None)
            .unwrap_err();
        clock
            .set_timer(
                "timer",
                Duration::from_secs(1),
                Some(pre_epoch),
                None,
                None,
                None,
                None,
            )
            .unwrap_err();
        let interval = Duration::from_secs(u64::MAX / NANOSECONDS_IN_SECOND + 1);
        let err = clock
            .set_timer("overflow", interval, None, None, None, None, None)
            .unwrap_err();

        assert_eq!(err.to_string(), "Interval exceeds u64 nanoseconds");
        assert!(calls.lock().expect(MUTEX_POISONED).is_empty());
    }

    #[rstest]
    fn test_clock_api_new_uses_native_backing(test_clock: TestClock) {
        let clock = RefCell::new(test_clock);
        let api = ClockApi::new(&clock);

        api.set_timer_ns(
            "native-timer",
            1_000,
            None,
            None,
            None,
            Some(true),
            Some(false),
        )
        .unwrap();

        assert_eq!(api.timer_count(), 1);
        assert_eq!(api.timer_names(), vec!["native-timer".to_string()]);
        assert_eq!(
            api.next_time_ns("native-timer"),
            Some(UnixNanos::from(1_000))
        );
    }

    #[rstest]
    fn test_clock_api_handlers_back_full_surface() {
        let alerts = Arc::new(Mutex::new(Vec::new()));
        let timers = Arc::new(Mutex::new(Vec::new()));
        let cancellations = Arc::new(Mutex::new(Vec::new()));
        let cancel_all = Arc::new(Mutex::new(false));

        let alerts_for_handler = Arc::clone(&alerts);
        let timers_for_handler = Arc::clone(&timers);
        let cancellations_for_handler = Arc::clone(&cancellations);
        let cancel_all_for_handler = Arc::clone(&cancel_all);

        let clock = ClockApi::from_handlers(
            || UnixNanos::from(1_700_000_000_123_456_789),
            move |name, alert_time_ns, _callback, allow_past| {
                alerts_for_handler.lock().expect(MUTEX_POISONED).push((
                    name.to_string(),
                    alert_time_ns,
                    allow_past,
                ));
                Ok(())
            },
            move |name,
                  interval_ns,
                  start_time_ns,
                  stop_time_ns,
                  _callback,
                  allow_past,
                  fire_immediately| {
                timers_for_handler.lock().expect(MUTEX_POISONED).push((
                    name.to_string(),
                    interval_ns,
                    start_time_ns,
                    stop_time_ns,
                    allow_past,
                    fire_immediately,
                ));
                Ok(())
            },
            || vec!["alpha".to_string(), "beta".to_string()],
            || 2,
            |name| name == "alpha",
            |name| (name == "alpha").then(|| UnixNanos::from(1_700_000_000_999_000_000)),
            move |name| {
                cancellations_for_handler
                    .lock()
                    .expect(MUTEX_POISONED)
                    .push(name.to_string());
            },
            move || {
                *cancel_all_for_handler.lock().expect(MUTEX_POISONED) = true;
            },
        );

        let alert_time = Timestamp::from_nanosecond(1_700_000_000_333_000_000).unwrap();
        let start_time = Timestamp::from_nanosecond(1_700_000_000_444_000_000).unwrap();
        let stop_time = Timestamp::from_nanosecond(1_700_000_001_444_000_000).unwrap();
        clock
            .set_time_alert("alert", alert_time, None, Some(false))
            .unwrap();
        clock
            .set_time_alert_ns(
                "alert-ns",
                UnixNanos::from(1_700_000_000_555_000_000),
                None,
                Some(true),
            )
            .unwrap();
        clock
            .set_timer(
                "timer",
                Duration::from_millis(250),
                Some(start_time),
                Some(stop_time),
                None,
                Some(true),
                Some(false),
            )
            .unwrap();
        clock
            .set_timer_ns(
                "timer-ns",
                500_000_000,
                Some(UnixNanos::from(1_700_000_000_666_000_000)),
                Some(UnixNanos::from(1_700_000_001_666_000_000)),
                None,
                Some(false),
                Some(true),
            )
            .unwrap();
        clock.cancel_timer("alpha");
        clock.cancel_timers();

        assert_eq!(
            clock.timestamp_ns(),
            UnixNanos::from(1_700_000_000_123_456_789)
        );
        assert_eq!(clock.timestamp_us(), 1_700_000_000_123_456);
        assert_eq!(clock.timestamp_ms(), 1_700_000_000_123);
        assert_eq!(clock.timestamp(), 1_700_000_000.123_456_7);
        assert_eq!(
            clock.utc_now(),
            Timestamp::from_nanosecond(1_700_000_000_123_456_789).unwrap()
        );
        assert_eq!(clock.timer_names(), vec!["alpha", "beta"]);
        assert_eq!(clock.timer_count(), 2);
        assert!(clock.timer_exists("alpha"));
        assert!(!clock.timer_exists("gamma"));
        assert_eq!(
            clock.next_time_ns("alpha"),
            Some(UnixNanos::from(1_700_000_000_999_000_000))
        );
        assert_eq!(
            alerts.lock().expect(MUTEX_POISONED).as_slice(),
            &[
                (
                    "alert".to_string(),
                    UnixNanos::from(1_700_000_000_333_000_000),
                    Some(false)
                ),
                (
                    "alert-ns".to_string(),
                    UnixNanos::from(1_700_000_000_555_000_000),
                    Some(true)
                )
            ]
        );
        assert_eq!(
            timers.lock().expect(MUTEX_POISONED).as_slice(),
            &[
                (
                    "timer".to_string(),
                    250_000_000,
                    Some(UnixNanos::from(1_700_000_000_444_000_000)),
                    Some(UnixNanos::from(1_700_000_001_444_000_000)),
                    Some(true),
                    Some(false)
                ),
                (
                    "timer-ns".to_string(),
                    500_000_000,
                    Some(UnixNanos::from(1_700_000_000_666_000_000)),
                    Some(UnixNanos::from(1_700_000_001_666_000_000)),
                    Some(false),
                    Some(true)
                )
            ]
        );
        assert_eq!(
            cancellations.lock().expect(MUTEX_POISONED).as_slice(),
            &["alpha".to_string()]
        );
        assert!(*cancel_all.lock().expect(MUTEX_POISONED));
    }

    proptest! {
        #[rstest]
        fn prop_test_clock_operations_match_reference(
            initial_time_ns in clock_time_strategy(),
            operations in prop::collection::vec(clock_operation_strategy(), 1..=50),
        ) {
            check_clock_operations(initial_time_ns, operations)?;
        }

        #[rstest]
        fn prop_test_clock_max_time_alert(initial_time_ns in clock_time_strategy()) {
            check_clock_max_time_alert(initial_time_ns)?;
        }
    }

    #[derive(Clone, Debug)]
    enum ClockOperation {
        Set {
            name_index: usize,
            interval_ns: u64,
            stop_after_ns: Option<u64>,
            fire_immediately: bool,
        },
        Cancel(usize),
        Advance {
            delta_ns: u64,
            set_time: bool,
        },
    }

    #[derive(Clone, Debug)]
    struct TimerModel {
        interval: u64,
        next: u64,
        stop: Option<u64>,
    }

    fn clock_operation_strategy() -> impl Strategy<Value = ClockOperation> {
        prop_oneof![
            5 => (
                0usize..CLOCK_TIMER_NAMES.len(),
                1u64..=15,
                prop::option::of(1u64..=60),
                prop::bool::ANY,
            )
                .prop_map(
                    |(name_index, interval_ns, stop_after_ns, fire_immediately)| {
                        ClockOperation::Set {
                            name_index,
                            interval_ns,
                            stop_after_ns,
                            fire_immediately,
                        }
                    },
                ),
            2 => (0usize..CLOCK_TIMER_NAMES.len()).prop_map(ClockOperation::Cancel),
            5 => (0u64..=30, prop::bool::ANY)
                .prop_map(|(delta_ns, set_time)| ClockOperation::Advance { delta_ns, set_time }),
        ]
    }

    fn clock_time_strategy() -> impl Strategy<Value = u64> {
        prop_oneof![
            6 => 0u64..=u64::MAX - CLOCK_TIME_HEADROOM,
            2 => 0u64..=1_000_000,
            1 => Just(1_700_000_000_000_000_000),
            1 => Just(u64::MAX - CLOCK_TIME_HEADROOM),
        ]
    }

    fn check_clock_operations(
        initial_time_ns: u64,
        operations: Vec<ClockOperation>,
    ) -> TestCaseResult {
        let mut clock = TestClock::new();
        clock.register_default_handler(TestCallback::default().into());
        clock.set_time(UnixNanos::from(initial_time_ns));

        let mut time_ns = initial_time_ns;
        let mut timers = BTreeMap::new();

        for operation in operations {
            match operation {
                ClockOperation::Set {
                    name_index,
                    interval_ns,
                    stop_after_ns,
                    fire_immediately,
                } => {
                    let name = clock_timer_name(name_index);
                    let stop_time_ns = stop_after_ns.map(|offset| time_ns + offset);
                    clock
                        .set_timer_ns(
                            name.as_str(),
                            interval_ns,
                            Some(UnixNanos::from(time_ns)),
                            stop_time_ns.map(UnixNanos::from),
                            None,
                            None,
                            Some(fire_immediately),
                        )
                        .expect("generated timer configuration should be valid");
                    timers.insert(
                        name,
                        TimerModel {
                            interval: interval_ns,
                            next: if fire_immediately {
                                time_ns
                            } else {
                                time_ns + interval_ns
                            },
                            stop: stop_time_ns,
                        },
                    );
                }
                ClockOperation::Cancel(name_index) => {
                    let name = clock_timer_name(name_index);
                    clock.cancel_timer(name.as_str());
                    timers.remove(&name);
                }
                ClockOperation::Advance { delta_ns, set_time } => {
                    let to_time_ns = time_ns + delta_ns;
                    let actual: Vec<(u64, Ustr, u64)> = clock
                        .advance_time(UnixNanos::from(to_time_ns), set_time)
                        .into_iter()
                        .map(|event| (event.ts_event.as_u64(), event.name, event.ts_init.as_u64()))
                        .collect();
                    let expected = advance_clock_timers(&mut timers, to_time_ns);

                    prop_assert_eq!(actual, expected);

                    if set_time {
                        time_ns = to_time_ns;
                    }
                }
            }

            assert_clock_state(&clock, &timers, time_ns)?;
        }

        Ok(())
    }

    fn check_clock_max_time_alert(initial_time_ns: u64) -> TestCaseResult {
        let mut clock = TestClock::new();
        clock.register_default_handler(TestCallback::default().into());
        clock.set_time(UnixNanos::from(initial_time_ns));
        let name = Ustr::from("terminal-alert");
        clock
            .set_time_alert_ns(name.as_str(), UnixNanos::max(), None, None)
            .expect("maximum timestamp should be a valid time alert");

        let events: Vec<(u64, Ustr, u64)> = clock
            .advance_time(UnixNanos::max(), true)
            .into_iter()
            .map(|event| (event.ts_event.as_u64(), event.name, event.ts_init.as_u64()))
            .collect();

        prop_assert_eq!(events, vec![(u64::MAX, name, u64::MAX)]);
        prop_assert_eq!(clock.timestamp_ns(), UnixNanos::max());
        prop_assert_eq!(clock.timer_count(), 0);
        prop_assert!(clock.timer_names().is_empty());
        prop_assert!(!clock.timer_exists(&name));
        prop_assert_eq!(clock.next_time_ns(name.as_str()), None);

        Ok(())
    }

    fn advance_clock_timers(
        timers: &mut BTreeMap<Ustr, TimerModel>,
        to_time_ns: u64,
    ) -> Vec<(u64, Ustr, u64)> {
        let mut events = Vec::new();

        timers.retain(|name, timer| {
            while timer.next <= to_time_ns {
                if timer
                    .stop
                    .is_some_and(|stop_time_ns| timer.next > stop_time_ns)
                {
                    return false;
                }

                let event_time_ns = timer.next;
                events.push((event_time_ns, *name, event_time_ns));
                let Some(following_time_ns) = event_time_ns.checked_add(timer.interval) else {
                    return false;
                };
                timer.next = following_time_ns;
                if timer.stop == Some(event_time_ns) {
                    return false;
                }
            }

            true
        });

        events.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        events
    }

    fn assert_clock_state(
        clock: &TestClock,
        timers: &BTreeMap<Ustr, TimerModel>,
        time_ns: u64,
    ) -> TestCaseResult {
        let expected_names: Vec<&str> = timers.keys().map(Ustr::as_str).collect();

        prop_assert_eq!(clock.timestamp_ns(), UnixNanos::from(time_ns));
        prop_assert_eq!(clock.timer_count(), timers.len());
        prop_assert_eq!(clock.timer_names(), expected_names);

        for name in CLOCK_TIMER_NAMES.map(Ustr::from) {
            let expected = timers.get(&name);
            prop_assert_eq!(clock.timer_exists(&name), expected.is_some());
            prop_assert_eq!(
                clock
                    .next_time_ns(name.as_str())
                    .map(|next_time_ns| next_time_ns.as_u64()),
                expected.map(|timer| timer.next),
            );
        }

        Ok(())
    }

    const CLOCK_TIMER_NAMES: [&str; 4] = ["timer-0", "timer-1", "timer-2", "timer-3"];
    const CLOCK_TIME_HEADROOM: u64 = 100_000;

    fn clock_timer_name(index: usize) -> Ustr {
        Ustr::from(CLOCK_TIMER_NAMES[index])
    }
}

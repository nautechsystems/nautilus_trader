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

//! User-facing facade over clock operations.

use std::{cell::RefCell, fmt::Debug, time::Duration};

use jiff::Timestamp;
use nautilus_core::{
    DurationNanos, UnixNanos,
    datetime::{NANOSECONDS_IN_SECOND, try_datetime_to_unix_nanos},
};
use ustr::Ustr;

use super::{Clock, duration_to_nanos};
use crate::timer::TimeEventCallback;

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
                DurationNanos,
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
        interval_ns: DurationNanos,
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
        DurationNanos,
        Option<UnixNanos>,
        Option<UnixNanos>,
        Option<TimeEventCallback>,
        Option<bool>,
        Option<bool>,
    ) -> anyhow::Result<()>
    + 'a;

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

use std::{
    cmp::Ordering,
    ffi::c_char,
    fmt::{Display, Formatter},
    time::Duration,
};

use nautilus_core::{
    correctness::check_valid_string,
    time::{get_atomic_clock_realtime, TimedeltaNanos, UnixNanos},
    uuid::UUID4,
};
#[cfg(feature = "python")]
use pyo3::{types::PyCapsule, IntoPy, PyObject, Python};
use tokio::sync::oneshot;
use ustr::Ustr;

use crate::{handlers::EventHandler, runtime::get_runtime};

#[repr(C)]
#[derive(Clone, Debug)]
#[allow(clippy::redundant_allocation)] // C ABI compatibility
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
/// Represents a time event occurring at the event timestamp.
pub struct TimeEvent {
    /// The event name.
    pub name: Ustr,
    /// The event ID.
    pub event_id: UUID4,
    /// The message category
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: UnixNanos,
}

/// Assumes `name` is a valid string.
impl TimeEvent {
    pub fn new(name: Ustr, event_id: UUID4, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        Self {
            name,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

impl Display for TimeEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TimeEvent(name={}, event_id={}, ts_event={}, ts_init={})",
            self.name, self.event_id, self.ts_event, self.ts_init
        )
    }
}

impl PartialEq for TimeEvent {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.ts_event == other.ts_event
    }
}

#[repr(C)]
#[derive(Clone, Debug)]
/// Represents a time event and its associated handler.
pub struct TimeEventHandler {
    /// The event.
    pub event: TimeEvent,
    /// The callable raw pointer.
    pub callback_ptr: *mut c_char,
}

impl PartialOrd for TimeEventHandler {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TimeEventHandler {
    fn eq(&self, other: &Self) -> bool {
        self.event.ts_event == other.event.ts_event
    }
}

impl Eq for TimeEventHandler {}

impl Ord for TimeEventHandler {
    fn cmp(&self, other: &Self) -> Ordering {
        self.event.ts_event.cmp(&other.event.ts_event)
    }
}

pub trait Timer {
    fn new(
        name: Ustr,
        interval_ns: TimedeltaNanos,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
    ) -> Self;
    fn pop_event(&self, event_id: UUID4, ts_init: UnixNanos) -> TimeEvent;
    fn iterate_next_time(&mut self, ts_now: UnixNanos);
    fn cancel(&mut self);
}

/// Provides a test timer for user with a `TestClock`.
#[derive(Clone, Copy, Debug)]
pub struct TestTimer {
    pub name: Ustr,
    pub interval_ns: u64,
    pub start_time_ns: UnixNanos,
    pub stop_time_ns: Option<UnixNanos>,
    pub next_time_ns: UnixNanos,
    pub is_expired: bool,
}

impl TestTimer {
    #[must_use]
    pub fn new(
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
    ) -> Self {
        check_valid_string(name, stringify!(name)).unwrap();

        Self {
            name: Ustr::from(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: start_time_ns + interval_ns,
            is_expired: false,
        }
    }

    #[must_use]
    pub fn pop_event(&self, event_id: UUID4, ts_init: UnixNanos) -> TimeEvent {
        TimeEvent {
            name: self.name,
            event_id,
            ts_event: self.next_time_ns,
            ts_init,
        }
    }

    /// Advance the test timer forward to the given time, generating a sequence
    /// of events. A [`TimeEvent`] is appended for each time a next event is
    /// <= the given `to_time_ns`.
    pub fn advance(&mut self, to_time_ns: UnixNanos) -> impl Iterator<Item = TimeEvent> + '_ {
        let advances =
            to_time_ns.saturating_sub(self.next_time_ns - self.interval_ns) / self.interval_ns;
        self.take(advances as usize).map(|(event, _)| event)
    }

    /// Cancels the timer (the timer will not generate an event).
    pub fn cancel(&mut self) {
        self.is_expired = true;
    }
}

impl Iterator for TestTimer {
    type Item = (TimeEvent, UnixNanos);

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_expired {
            None
        } else {
            let item = (
                TimeEvent {
                    name: self.name,
                    event_id: UUID4::new(),
                    ts_event: self.next_time_ns,
                    ts_init: self.next_time_ns,
                },
                self.next_time_ns,
            );

            // If current next event time has exceeded stop time, then expire timer
            if let Some(stop_time_ns) = self.stop_time_ns {
                if self.next_time_ns >= stop_time_ns {
                    self.is_expired = true;
                }
            }

            self.next_time_ns += self.interval_ns;

            Some(item)
        }
    }
}

/// Provides a live timer for use with a `LiveClock`.
///
/// Note: `next_time_ns` is only accurate when initially starting the timer
/// and will not incrementally update as the timer runs.
pub struct LiveTimer {
    pub name: Ustr,
    pub interval_ns: u64,
    pub start_time_ns: UnixNanos,
    pub stop_time_ns: Option<UnixNanos>,
    pub next_time_ns: UnixNanos,
    pub is_expired: bool,
    callback: EventHandler,
    canceler: Option<oneshot::Sender<()>>,
}

impl LiveTimer {
    #[must_use]
    pub fn new(
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: EventHandler,
    ) -> Self {
        check_valid_string(name, stringify!(name)).unwrap();

        Self {
            name: Ustr::from(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: start_time_ns + interval_ns,
            is_expired: false,
            callback,
            canceler: None,
        }
    }

    pub fn start(&mut self) {
        let event_name = self.name;
        let mut start_time_ns = self.start_time_ns;
        let stop_time_ns = self.stop_time_ns;
        let interval_ns = self.interval_ns;

        let callback = self.callback.clone();

        // Setup oneshot channel for cancelling timer task
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.canceler = Some(cancel_tx);

        get_runtime().spawn(async move {
            let clock = get_atomic_clock_realtime();
            if start_time_ns == 0 {
                start_time_ns = clock.get_time_ns();
            }

            let mut next_time_ns = start_time_ns + interval_ns;

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_nanos(next_time_ns.saturating_sub(clock.get_time_ns()))) => {
                        // TODO: Remove this clone
                        let callback = callback.clone();
                        call_python_with_time_event(event_name, next_time_ns, clock.get_time_ns(), callback);

                        // Prepare next time interval
                        next_time_ns += interval_ns;

                        // Check if expired
                        if let Some(stop_time_ns) = stop_time_ns {
                            if next_time_ns >= stop_time_ns {
                                break; // Timer expired
                            }
                        }
                    },
                    _ = (&mut cancel_rx) => {
                        break; // Timer canceled
                    },
                }
            }

            Ok::<(), anyhow::Error>(())
        });

        self.is_expired = true;
    }

    /// Cancels the timer (the timer will not generate an event).
    pub fn cancel(&mut self) {
        if let Some(sender) = self.canceler.take() {
            let _ = sender.send(());
        }
    }
}

#[cfg(feature = "python")]
fn call_python_with_time_event(
    name: Ustr,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    handler: EventHandler,
) {
    Python::with_gil(|py| {
        // Create new time event
        let event = TimeEvent::new(name, UUID4::new(), ts_event, ts_init);
        let capsule: PyObject = PyCapsule::new(py, event, None)
            .expect("Error creating `PyCapsule`")
            .into_py(py);

        match handler.callback.call1(py, (capsule,)) {
            Ok(_) => {}
            Err(e) => eprintln!("Error on callback: {:?}", e),
        };
    })
}

#[cfg(not(feature = "python"))]
fn call_python_with_time_event(
    _name: Ustr,
    _ts_event: UnixNanos,
    _ts_init: UnixNanos,
    _handler: EventHandler,
) {
    panic!("`python` feature is not enabled");
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(not(feature = "python"))]
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::{TestTimer, TimeEvent};

    #[rstest]
    fn test_test_timer_pop_event() {
        let mut timer = TestTimer::new("test_timer", 0, 1, None);

        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[rstest]
    fn test_test_timer_advance_within_next_time_ns() {
        let mut timer = TestTimer::new("test_timer", 5, 0, None);
        let _: Vec<TimeEvent> = timer.advance(1).collect();
        let _: Vec<TimeEvent> = timer.advance(2).collect();
        let _: Vec<TimeEvent> = timer.advance(3).collect();
        assert_eq!(timer.advance(4).count(), 0);
        assert_eq!(timer.next_time_ns, 5);
        assert!(!timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_up_to_next_time_ns() {
        let mut timer = TestTimer::new("test_timer", 1, 0, None);
        assert_eq!(timer.advance(1).count(), 1);
        assert!(!timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_up_to_next_time_ns_with_stop_time() {
        let mut timer = TestTimer::new("test_timer", 1, 0, Some(2));
        assert_eq!(timer.advance(2).count(), 2);
        assert!(timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_beyond_next_time_ns() {
        let mut timer = TestTimer::new("test_timer", 1, 0, Some(5));
        assert_eq!(timer.advance(5).count(), 5);
        assert!(timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_beyond_stop_time() {
        let mut timer = TestTimer::new("test_timer", 1, 0, Some(5));
        assert_eq!(timer.advance(10).count(), 5);
        assert!(timer.is_expired);
    }
}

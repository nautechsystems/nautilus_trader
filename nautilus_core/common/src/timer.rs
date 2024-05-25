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

//! Real-time and test timers for use with `Clock` implementations.

use std::{
    cmp::Ordering,
    ffi::c_char,
    fmt::{Display, Formatter},
    num::NonZeroU64,
    sync::{
        atomic::{self, AtomicBool, AtomicU64},
        Arc,
    },
};

use nautilus_core::{
    correctness::check_valid_string, datetime::floor_to_nearest_microsecond, nanos::UnixNanos,
    time::get_atomic_clock_realtime, uuid::UUID4,
};
#[cfg(feature = "python")]
use pyo3::{types::PyCapsule, IntoPy, PyObject, Python};
use tokio::{
    sync::oneshot,
    time::{Duration, Instant},
};
use tracing::{debug, error, trace};
use ustr::Ustr;

use crate::{handlers::EventHandler, runtime::get_runtime};

#[repr(C)]
#[derive(Clone, Debug)]
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

impl TimeEvent {
    /// Creates a new [`TimeEvent`] instance.
    ///
    /// Assumes `name` is a valid string.
    #[must_use]
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
        self.event_id == other.event_id
    }
}

#[repr(C)]
#[derive(Clone, Debug)]
/// Represents a time event and its associated handler.
pub struct TimeEventHandler {
    /// The time event.
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

/// A test timer for user with a `TestClock`.
#[derive(Clone, Copy, Debug)]
pub struct TestTimer {
    pub name: Ustr,
    pub interval_ns: NonZeroU64,
    pub start_time_ns: UnixNanos,
    pub stop_time_ns: Option<UnixNanos>,
    next_time_ns: UnixNanos,
    is_expired: bool,
}

impl TestTimer {
    /// Creates a new [`TestTimer`] instance.
    pub fn new(
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
    ) -> anyhow::Result<Self> {
        check_valid_string(name, stringify!(name))?;
        // SAFETY: Guaranteed to be non-zero
        let interval_ns = NonZeroU64::new(std::cmp::max(interval_ns, 1)).unwrap();

        Ok(Self {
            name: Ustr::from(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: start_time_ns + interval_ns.get(),
            is_expired: false,
        })
    }

    /// Returns the next time in UNIX nanoseconds when the timer will fire.
    #[must_use]
    pub fn next_time_ns(&self) -> UnixNanos {
        self.next_time_ns
    }

    /// Returns whether the timer is expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired
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
        let advances = to_time_ns
            .saturating_sub(self.next_time_ns.as_u64() - self.interval_ns.get())
            / self.interval_ns.get();
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

/// A live timer for use with a `LiveClock`.
pub struct LiveTimer {
    pub name: Ustr,
    pub interval_ns: NonZeroU64,
    pub start_time_ns: UnixNanos,
    pub stop_time_ns: Option<UnixNanos>,
    next_time_ns: Arc<AtomicU64>,
    is_expired: Arc<AtomicBool>,
    callback: EventHandler,
    canceler: Option<oneshot::Sender<()>>,
}

impl LiveTimer {
    /// Creates a new [`LiveTimer`] instance.
    pub fn new(
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: EventHandler,
    ) -> anyhow::Result<Self> {
        check_valid_string(name, stringify!(name))?;
        // SAFETY: Guaranteed to be non-zero
        let interval_ns = NonZeroU64::new(std::cmp::max(interval_ns, 1)).unwrap();

        debug!("Creating timer '{}'", name);
        Ok(Self {
            name: Ustr::from(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: Arc::new(AtomicU64::new(start_time_ns.as_u64() + interval_ns.get())),
            is_expired: Arc::new(AtomicBool::new(false)),
            callback,
            canceler: None,
        })
    }

    /// Returns the next time in UNIX nanoseconds when the timer will fire.
    #[must_use]
    pub fn next_time_ns(&self) -> UnixNanos {
        UnixNanos::from(self.next_time_ns.load(atomic::Ordering::SeqCst))
    }

    /// Returns whether the timer is expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired.load(atomic::Ordering::SeqCst)
    }

    /// Starts the timer.
    pub fn start(&mut self) {
        let event_name = self.name;
        let stop_time_ns = self.stop_time_ns;
        let next_time_ns = self.next_time_ns.load(atomic::Ordering::SeqCst);
        let next_time_atomic = self.next_time_ns.clone();
        let interval_ns = self.interval_ns.get();
        let is_expired = self.is_expired.clone();
        let callback = self.callback.clone();

        // Floor the next time to the nearest microsecond which is within the timers accuracy
        let mut next_time_ns = UnixNanos::from(floor_to_nearest_microsecond(next_time_ns));

        // Setup oneshot channel for cancelling timer task
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.canceler = Some(cancel_tx);

        let rt = get_runtime();
        rt.spawn(async move {
            let clock = get_atomic_clock_realtime();
            let now_ns = clock.get_time_ns();

            let start = if next_time_ns <= now_ns {
                Instant::now()
            } else {
                // Timer initialization delay
                let delay = Duration::from_millis(1);
                let diff: u64 = (next_time_ns - now_ns).into();
                Instant::now() + Duration::from_nanos(diff) - delay
            };

            let mut timer = tokio::time::interval_at(start, Duration::from_nanos(interval_ns));

            loop {
                // SAFETY: `timer.tick` is cancellation safe, if the cancel branch completes
                // first then no tick has been consumed (no event was ready).
                tokio::select! {
                    _ = timer.tick() => {
                        let now_ns = clock.get_time_ns();
                        call_python_with_time_event(event_name, next_time_ns, now_ns, &callback);

                        // Prepare next time interval
                        next_time_ns += interval_ns;
                        next_time_atomic.store(next_time_ns.as_u64(), atomic::Ordering::SeqCst);

                        // Check if expired
                        if let Some(stop_time_ns) = stop_time_ns {
                            if std::cmp::max(next_time_ns, now_ns) >= stop_time_ns {
                                break; // Timer expired
                            }
                        }
                    },
                    _ = (&mut cancel_rx) => {
                        trace!("Received timer cancel");
                        break; // Timer canceled
                    },
                }
            }

            is_expired.store(true, atomic::Ordering::SeqCst);

            Ok::<(), anyhow::Error>(())
        });
    }

    /// Cancels the timer (the timer will not generate a final event).
    pub fn cancel(&mut self) -> anyhow::Result<()> {
        debug!("Cancel timer '{}'", self.name);
        if !self.is_expired.load(atomic::Ordering::SeqCst) {
            if let Some(sender) = self.canceler.take() {
                // Send cancellation signal
                sender.send(()).map_err(|e| anyhow::anyhow!("{:?}", e))?;
            }
        }
        Ok(())
    }
}

#[cfg(feature = "python")]
fn call_python_with_time_event(
    name: Ustr,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    handler: &EventHandler,
) {
    Python::with_gil(|py| {
        // Create new time event
        let event = TimeEvent::new(name, UUID4::new(), ts_event, ts_init);
        let capsule: PyObject = PyCapsule::new(py, event, None)
            .expect("Error creating `PyCapsule`")
            .into_py(py);

        match handler.callback.call1(py, (capsule,)) {
            Ok(_) => {}
            Err(e) => error!("Error on callback: {:?}", e),
        };
    });
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
#[cfg(test)]
mod tests {
    use nautilus_core::{
        datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos, time::get_atomic_clock_realtime,
    };
    use pyo3::prelude::*;
    use rstest::*;
    use tokio::time::Duration;

    use super::{LiveTimer, TestTimer, TimeEvent};
    use crate::{handlers::EventHandler, testing::wait_until};

    #[pyfunction]
    fn receive_event(_py: Python, _event: TimeEvent) -> PyResult<()> {
        // TODO: Assert the length of a handler vec
        Ok(())
    }

    #[rstest]
    fn test_test_timer_pop_event() {
        let mut timer = TestTimer::new("test_timer", 1, UnixNanos::from(1), None).unwrap();

        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[rstest]
    fn test_test_timer_advance_within_next_time_ns() {
        let mut timer = TestTimer::new("test_timer", 5, UnixNanos::default(), None).unwrap();
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(1)).collect();
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(2)).collect();
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(3)).collect();
        assert_eq!(timer.advance(UnixNanos::from(4)).count(), 0);
        assert_eq!(timer.next_time_ns, 5);
        assert!(!timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_up_to_next_time_ns() {
        let mut timer = TestTimer::new("test_timer", 1, UnixNanos::default(), None).unwrap();
        assert_eq!(timer.advance(UnixNanos::from(1)).count(), 1);
        assert!(!timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_up_to_next_time_ns_with_stop_time() {
        let mut timer = TestTimer::new(
            "test_timer",
            1,
            UnixNanos::default(),
            Some(UnixNanos::from(2)),
        )
        .unwrap();
        assert_eq!(timer.advance(UnixNanos::from(2)).count(), 2);
        assert!(timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_beyond_next_time_ns() {
        let mut timer = TestTimer::new(
            "test_timer",
            1,
            UnixNanos::default(),
            Some(UnixNanos::from(5)),
        )
        .unwrap();
        assert_eq!(timer.advance(UnixNanos::from(5)).count(), 5);
        assert!(timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_beyond_stop_time() {
        let mut timer = TestTimer::new(
            "test_timer",
            1,
            UnixNanos::default(),
            Some(UnixNanos::from(5)),
        )
        .unwrap();
        assert_eq!(timer.advance(UnixNanos::from(10)).count(), 5);
        assert!(timer.is_expired);
    }

    #[tokio::test]
    async fn test_live_timer_starts_and_stops() {
        pyo3::prepare_freethreaded_python();

        let handler = Python::with_gil(|py| {
            let callable = wrap_pyfunction!(receive_event, py).unwrap();
            EventHandler::new(callable.into_py(py))
        });

        // Create a new LiveTimer with no stop time
        let clock = get_atomic_clock_realtime();
        let start_time = clock.get_time_ns();
        let interval_ns = 100 * NANOSECONDS_IN_MILLISECOND;
        let mut timer =
            LiveTimer::new("TEST_TIMER", interval_ns, start_time, None, handler).unwrap();
        let next_time_ns = timer.next_time_ns();
        timer.start();

        // Wait for timer to run
        tokio::time::sleep(Duration::from_millis(300)).await;

        timer.cancel().unwrap();
        wait_until(|| timer.is_expired(), Duration::from_secs(2));
        assert!(timer.next_time_ns() > next_time_ns);
    }

    #[tokio::test]
    async fn test_live_timer_with_stop_time() {
        pyo3::prepare_freethreaded_python();

        let handler = Python::with_gil(|py| {
            let callable = wrap_pyfunction!(receive_event, py).unwrap();
            EventHandler::new(callable.into_py(py))
        });

        // Create a new LiveTimer with a stop time
        let clock = get_atomic_clock_realtime();
        let start_time = clock.get_time_ns();
        let interval_ns = 100 * NANOSECONDS_IN_MILLISECOND;
        let stop_time = start_time + 500 * NANOSECONDS_IN_MILLISECOND;
        let mut timer = LiveTimer::new(
            "TEST_TIMER",
            interval_ns,
            start_time,
            Some(stop_time),
            handler,
        )
        .unwrap();
        let next_time_ns = timer.next_time_ns();
        timer.start();

        // Wait for a longer time than the stop time
        tokio::time::sleep(Duration::from_secs(1)).await;

        wait_until(|| timer.is_expired(), Duration::from_secs(2));
        assert!(timer.next_time_ns() > next_time_ns);
    }

    #[tokio::test]
    async fn test_live_timer_with_zero_interval_and_immediate_stop_time() {
        pyo3::prepare_freethreaded_python();

        let handler = Python::with_gil(|py| {
            let callable = wrap_pyfunction!(receive_event, py).unwrap();
            EventHandler::new(callable.into_py(py))
        });

        // Create a new LiveTimer with a stop time
        let clock = get_atomic_clock_realtime();
        let start_time = UnixNanos::default();
        let interval_ns = 0;
        let stop_time = clock.get_time_ns();
        let mut timer = LiveTimer::new(
            "TEST_TIMER",
            interval_ns,
            start_time,
            Some(stop_time),
            handler,
        )
        .unwrap();
        timer.start();

        wait_until(|| timer.is_expired(), Duration::from_secs(2));
    }
}

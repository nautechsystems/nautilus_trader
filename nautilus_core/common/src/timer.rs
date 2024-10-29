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
    fmt::{Debug, Display},
    num::NonZeroU64,
    rc::Rc,
    sync::{
        atomic::{self, AtomicBool, AtomicU64},
        Arc,
    },
};

use nautilus_core::{
    correctness::{check_valid_string, FAILED},
    datetime::floor_to_nearest_microsecond,
    nanos::UnixNanos,
    time::get_atomic_clock_realtime,
    uuid::UUID4,
};
#[cfg(feature = "python")]
use pyo3::{PyObject, Python};
use tokio::{
    sync::oneshot,
    time::{Duration, Instant},
};
use ustr::Ustr;

use crate::runtime::get_runtime;

#[repr(C)]
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
/// Represents a time event occurring at the event timestamp.
///
/// A `TimeEvent` carries metadata such as the event's name, a unique event ID,
/// and timestamps indicating when the event was scheduled to occur and when it was initialized.
pub struct TimeEvent {
    /// The event name, identifying the nature or purpose of the event.
    pub name: Ustr,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl TimeEvent {
    /// Creates a new [`TimeEvent`] instance.
    ///
    /// # Safety
    ///
    /// - Assumes `name` is a valid string.
    #[must_use]
    pub const fn new(name: Ustr, event_id: UUID4, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        Self {
            name,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

impl Display for TimeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

pub type RustTimeEventCallback = dyn Fn(TimeEvent);

#[derive(Clone)]
pub enum TimeEventCallback {
    #[cfg(feature = "python")]
    Python(Arc<PyObject>),
    Rust(Rc<RustTimeEventCallback>),
}

impl Debug for TimeEventCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "python")]
            Self::Python(_) => f.write_str("Python callback"),
            Self::Rust(_) => f.write_str("Rust callback"),
        }
    }
}

impl TimeEventCallback {
    pub fn call(&self, event: TimeEvent) {
        match self {
            #[cfg(feature = "python")]
            Self::Python(callback) => {
                Python::with_gil(|py| {
                    callback.call1(py, (event,)).unwrap();
                });
            }
            Self::Rust(callback) => callback(event),
        }
    }
}

impl From<Rc<RustTimeEventCallback>> for TimeEventCallback {
    fn from(value: Rc<RustTimeEventCallback>) -> Self {
        Self::Rust(value)
    }
}

#[cfg(feature = "python")]
impl From<PyObject> for TimeEventCallback {
    fn from(value: PyObject) -> Self {
        Self::Python(Arc::new(value))
    }
}

// SAFETY: Message handlers cannot be sent across thread boundaries
unsafe impl Send for TimeEventCallback {}
unsafe impl Sync for TimeEventCallback {}

#[repr(C)]
#[derive(Clone, Debug)]
/// Represents a time event and its associated handler.
///
/// `TimeEventHandler` associates a `TimeEvent` with a callback function that is triggered
/// when the event's timestamp is reached.
pub struct TimeEventHandlerV2 {
    /// The time event.
    pub event: TimeEvent,
    /// The callable handler for the event.
    pub callback: TimeEventCallback,
}

impl TimeEventHandlerV2 {
    /// Creates a new [`TimeEventHandlerV2`] instance.
    #[must_use]
    pub const fn new(event: TimeEvent, callback: TimeEventCallback) -> Self {
        Self { event, callback }
    }
}

impl PartialOrd for TimeEventHandlerV2 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TimeEventHandlerV2 {
    fn eq(&self, other: &Self) -> bool {
        self.event.ts_event == other.event.ts_event
    }
}

impl Eq for TimeEventHandlerV2 {}

impl Ord for TimeEventHandlerV2 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.event.ts_event.cmp(&other.event.ts_event)
    }
}

/// A test timer for user with a `TestClock`.
///
/// `TestTimer` simulates time progression in a controlled environment,
/// allowing for precise control over event generation in test scenarios.
#[derive(Clone, Copy, Debug)]
pub struct TestTimer {
    /// The name of the timer.
    pub name: Ustr,
    /// The interval between timer events in nanoseconds.
    pub interval_ns: NonZeroU64,
    /// The start time of the timer in UNIX nanoseconds.
    pub start_time_ns: UnixNanos,
    /// The optional stop time of the timer in UNIX nanoseconds.
    pub stop_time_ns: Option<UnixNanos>,
    next_time_ns: UnixNanos,
    is_expired: bool,
}

impl TestTimer {
    /// Creates a new [`TestTimer`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `name` is not a valid string.
    #[must_use]
    pub fn new(
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
    ) -> Self {
        check_valid_string(name, stringify!(name)).expect(FAILED);
        // SAFETY: Guaranteed to be non-zero
        let interval_ns = NonZeroU64::new(std::cmp::max(interval_ns, 1)).unwrap();

        Self {
            name: Ustr::from(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: start_time_ns + interval_ns.get(),
            is_expired: false,
        }
    }

    /// Returns the next time in UNIX nanoseconds when the timer will fire.
    #[must_use]
    pub const fn next_time_ns(&self) -> UnixNanos {
        self.next_time_ns
    }

    /// Returns whether the timer is expired.
    #[must_use]
    pub const fn is_expired(&self) -> bool {
        self.is_expired
    }

    #[must_use]
    pub const fn pop_event(&self, event_id: UUID4, ts_init: UnixNanos) -> TimeEvent {
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
    ///
    /// This allows testing of multiple time intervals within a single step.
    pub fn advance(&mut self, to_time_ns: UnixNanos) -> impl Iterator<Item = TimeEvent> + '_ {
        let advances = to_time_ns
            .saturating_sub(self.next_time_ns.as_u64() - self.interval_ns.get())
            / self.interval_ns.get();
        self.take(advances as usize).map(|(event, _)| event)
    }

    /// Cancels the timer (the timer will not generate an event).
    ///
    /// Used to stop the timer before its scheduled stop time.
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
///
/// `LiveTimer` triggers events at specified intervals in a real-time environment,
/// using Tokio's async runtime to handle scheduling and execution.
pub struct LiveTimer {
    /// The name of the timer.
    pub name: Ustr,
    /// The start time of the timer in UNIX nanoseconds.
    pub interval_ns: NonZeroU64,
    /// The start time of the timer in UNIX nanoseconds.
    pub start_time_ns: UnixNanos,
    /// The optional stop time of the timer in UNIX nanoseconds.
    pub stop_time_ns: Option<UnixNanos>,
    next_time_ns: Arc<AtomicU64>,
    is_expired: Arc<AtomicBool>,
    callback: TimeEventCallback,
    canceler: Option<oneshot::Sender<()>>,
}

impl LiveTimer {
    /// Creates a new [`LiveTimer`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `name` is not a valid string.
    /// - If `interval_ns` is zero.
    #[must_use]
    pub fn new(
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: TimeEventCallback,
    ) -> Self {
        check_valid_string(name, stringify!(name)).expect(FAILED);
        // SAFETY: Guaranteed to be non-zero
        let interval_ns = NonZeroU64::new(std::cmp::max(interval_ns, 1)).unwrap();

        log::debug!("Creating timer '{name}'");
        Self {
            name: Ustr::from(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: Arc::new(AtomicU64::new(start_time_ns.as_u64() + interval_ns.get())),
            is_expired: Arc::new(AtomicBool::new(false)),
            callback,
            canceler: None,
        }
    }

    /// Returns the next time in UNIX nanoseconds when the timer will fire.
    ///
    /// Provides the scheduled time for the next event based on the current state of the timer.
    #[must_use]
    pub fn next_time_ns(&self) -> UnixNanos {
        UnixNanos::from(self.next_time_ns.load(atomic::Ordering::SeqCst))
    }

    /// Returns whether the timer is expired.
    ///
    /// An expired timer will not trigger any further events.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired.load(atomic::Ordering::SeqCst)
    }

    /// Starts the timer.
    ///
    /// Time events will begin triggering at the specified intervals.
    /// The generated events are handled by the provided callback function.
    pub fn start(&mut self) {
        let event_name = self.name;
        let stop_time_ns = self.stop_time_ns;
        let next_time_ns = self.next_time_ns.load(atomic::Ordering::SeqCst);
        let next_time_atomic = self.next_time_ns.clone();
        let interval_ns = self.interval_ns.get();
        let is_expired = self.is_expired.clone();

        // TODO: Live timer is currently multi-threaded
        // and only supports the python event handler
        #[cfg(feature = "python")]
        let callback = match self.callback.clone() {
            TimeEventCallback::Python(callback) => callback,
            TimeEventCallback::Rust(_) => {
                panic!("Live timer does not support Rust callbacks right now")
            }
        };

        // Floor the next time to the nearest microsecond which is within the timers accuracy
        let mut next_time_ns = UnixNanos::from(floor_to_nearest_microsecond(next_time_ns));

        // Set up oneshot channel for canceling timer task
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
                        #[cfg(feature = "python")]
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
                        tracing::trace!("Received timer cancel");
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
        log::debug!("Cancel timer '{}'", self.name);
        if !self.is_expired.load(atomic::Ordering::SeqCst) {
            if let Some(sender) = self.canceler.take() {
                // Send cancellation signal
                sender.send(()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
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
    callback: &PyObject,
) {
    use pyo3::{types::PyCapsule, IntoPy};

    Python::with_gil(|py| {
        // Create new time event
        let event = TimeEvent::new(name, UUID4::new(), ts_event, ts_init);
        let capsule: PyObject = PyCapsule::new_bound(py, event, None)
            .expect("Error creating `PyCapsule`")
            .into_py(py);

        match callback.call1(py, (capsule,)) {
            Ok(_) => {}
            Err(e) => tracing::error!("Error on callback: {e:?}"),
        };
    });
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::nanos::UnixNanos;
    use rstest::*;

    use super::{TestTimer, TimeEvent};

    #[rstest]
    fn test_test_timer_pop_event() {
        let mut timer = TestTimer::new("test_timer", 1, UnixNanos::from(1), None);

        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[rstest]
    fn test_test_timer_advance_within_next_time_ns() {
        let mut timer = TestTimer::new("test_timer", 5, UnixNanos::default(), None);
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(1)).collect();
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(2)).collect();
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(3)).collect();
        assert_eq!(timer.advance(UnixNanos::from(4)).count(), 0);
        assert_eq!(timer.next_time_ns, 5);
        assert!(!timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_up_to_next_time_ns() {
        let mut timer = TestTimer::new("test_timer", 1, UnixNanos::default(), None);
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
        );
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
        );
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
        );
        assert_eq!(timer.advance(UnixNanos::from(10)).count(), 5);
        assert!(timer.is_expired);
    }
}

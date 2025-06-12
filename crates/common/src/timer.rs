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

//! Real-time and test timers for use with `Clock` implementations.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    num::NonZeroU64,
    rc::Rc,
    sync::{
        Arc,
        atomic::{self, AtomicU64},
    },
};

use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{FAILED, check_valid_string},
    datetime::floor_to_nearest_microsecond,
    time::get_atomic_clock_realtime,
};
#[cfg(feature = "python")]
use pyo3::{PyObject, Python};
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};
use ustr::Ustr;

use crate::{runner::TimeEventSender, runtime::get_runtime};

/// Creates a valid nanoseconds interval that is guaranteed to be positive.
///
/// # Panics
///
/// Panics if `interval_ns` is zero.
#[must_use]
pub fn create_valid_interval(interval_ns: u64) -> NonZeroU64 {
    NonZeroU64::new(std::cmp::max(interval_ns, 1)).expect("`interval_ns` must be positive")
}

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
#[derive(Eq)]
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

/// Reverse order for `TimeEvent` comparison to be used in max heap.
impl PartialOrd for TimeEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Reverse order for `TimeEvent` comparison to be used in max heap.
impl Ord for TimeEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        other.ts_event.cmp(&self.ts_event)
    }
}

impl TimeEvent {
    /// Creates a new [`TimeEvent`] instance.
    ///
    /// # Safety
    ///
    /// Assumes `name` is a valid string.
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
    /// Invokes the callback for the given `TimeEvent`.
    ///
    /// # Panics
    ///
    /// Panics if the underlying Python callback invocation fails (e.g., raises an exception).
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

// TimeEventCallback supports both single-threaded and async use cases:
// - Python variant uses Arc<PyObject> for cross-thread compatibility with Python's GIL
// - Rust variant uses Rc<dyn Fn(TimeEvent)> for efficient single-threaded callbacks
// SAFETY: The async timer tasks only use Python callbacks, and Rust callbacks are never
// sent across thread boundaries in practice. This unsafe implementation allows the enum
// to be moved into async tasks while maintaining the efficient Rc for single-threaded use.
#[allow(unsafe_code)]
unsafe impl Send for TimeEventCallback {}
#[allow(unsafe_code)]
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

    /// Executes the handler by invoking its callback for the associated event.
    ///
    /// # Panics
    ///
    /// Panics if the underlying callback invocation fails (e.g., a Python callback raises an exception).
    pub fn run(self) {
        let Self { event, callback } = self;
        callback.call(event);
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
    /// If the timer should fire immediately at start time.
    pub fire_immediately: bool,
    next_time_ns: UnixNanos,
    is_expired: bool,
}

impl TestTimer {
    /// Creates a new [`TestTimer`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a valid string.
    #[must_use]
    pub fn new(
        name: Ustr,
        interval_ns: NonZeroU64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        fire_immediately: bool,
    ) -> Self {
        check_valid_string(name, stringify!(name)).expect(FAILED);

        let next_time_ns = if fire_immediately {
            start_time_ns
        } else {
            start_time_ns + interval_ns.get()
        };

        Self {
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            fire_immediately,
            next_time_ns,
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
        // Calculate how many events should fire up to and including to_time_ns
        let advances = if self.next_time_ns <= to_time_ns {
            (to_time_ns.as_u64() - self.next_time_ns.as_u64()) / self.interval_ns.get() + 1
        } else {
            0
        };
        self.take(advances as usize).map(|(event, _)| event)
    }

    /// Cancels the timer (the timer will not generate an event).
    ///
    /// Used to stop the timer before its scheduled stop time.
    pub const fn cancel(&mut self) {
        self.is_expired = true;
    }
}

impl Iterator for TestTimer {
    type Item = (TimeEvent, UnixNanos);

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_expired {
            None
        } else {
            // Check if current event would exceed stop time before creating the event
            if let Some(stop_time_ns) = self.stop_time_ns {
                if self.next_time_ns > stop_time_ns {
                    self.is_expired = true;
                    return None;
                }
            }

            let item = (
                TimeEvent {
                    name: self.name,
                    event_id: UUID4::new(),
                    ts_event: self.next_time_ns,
                    ts_init: self.next_time_ns,
                },
                self.next_time_ns,
            );

            // Check if we should expire after this event (for repeating timers at stop boundary)
            if let Some(stop_time_ns) = self.stop_time_ns {
                if self.next_time_ns == stop_time_ns {
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
#[derive(Debug)]
pub struct LiveTimer {
    /// The name of the timer.
    pub name: Ustr,
    /// The start time of the timer in UNIX nanoseconds.
    pub interval_ns: NonZeroU64,
    /// The start time of the timer in UNIX nanoseconds.
    pub start_time_ns: UnixNanos,
    /// The optional stop time of the timer in UNIX nanoseconds.
    pub stop_time_ns: Option<UnixNanos>,
    /// If the timer should fire immediately at start time.
    pub fire_immediately: bool,
    next_time_ns: Arc<AtomicU64>,
    callback: TimeEventCallback,
    task_handle: Option<JoinHandle<()>>,
    sender: Option<Arc<dyn TimeEventSender>>,
}

impl LiveTimer {
    /// Creates a new [`LiveTimer`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a valid string.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        name: Ustr,
        interval_ns: NonZeroU64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: TimeEventCallback,
        fire_immediately: bool,
        sender: Option<Arc<dyn TimeEventSender>>,
    ) -> Self {
        check_valid_string(name, stringify!(name)).expect(FAILED);

        let next_time_ns = if fire_immediately {
            start_time_ns.as_u64()
        } else {
            start_time_ns.as_u64() + interval_ns.get()
        };

        log::debug!("Creating timer '{name}'");

        Self {
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            fire_immediately,
            next_time_ns: Arc::new(AtomicU64::new(next_time_ns)),
            callback,
            task_handle: None,
            sender,
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
    /// A timer that has not been started is not expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.task_handle
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
    }

    /// Starts the timer.
    ///
    /// Time events will begin triggering at the specified intervals.
    /// The generated events are handled by the provided callback function.
    ///
    /// # Panics
    ///
    /// Panics if Rust-based callback system active and no time event sender has been set.
    #[allow(unused_variables)] // callback is used
    pub fn start(&mut self) {
        let event_name = self.name;
        let stop_time_ns = self.stop_time_ns;
        let interval_ns = self.interval_ns.get();
        let callback = self.callback.clone();

        // Get current time
        let clock = get_atomic_clock_realtime();
        let now_ns = clock.get_time_ns();

        // Check if the timer's alert time is in the past and adjust if needed
        let mut next_time_ns = self.next_time_ns.load(atomic::Ordering::SeqCst);
        if next_time_ns <= now_ns {
            log::warn!(
                "Timer '{event_name}' alert time {next_time_ns} was in the past, adjusted to current time for immediate fire"
            );
            next_time_ns = now_ns.into();
            self.next_time_ns
                .store(now_ns.as_u64(), atomic::Ordering::SeqCst);
        }

        // Floor the next time to the nearest microsecond which is within the timers accuracy
        let mut next_time_ns = UnixNanos::from(floor_to_nearest_microsecond(next_time_ns));
        let next_time_atomic = self.next_time_ns.clone();

        let sender = self.sender.clone();

        let rt = get_runtime();
        let handle = rt.spawn(async move {
            let clock = get_atomic_clock_realtime();

            // 1-millisecond delay to account for the overhead of initializing a tokio timer
            let overhead = Duration::from_millis(1);
            let delay_ns = next_time_ns.saturating_sub(now_ns.as_u64());
            let delay = Duration::from_nanos(delay_ns).saturating_sub(overhead);
            let start = Instant::now() + delay;

            let mut timer = tokio::time::interval_at(start, Duration::from_nanos(interval_ns));

            loop {
                // SAFETY: `timer.tick` is cancellation safe, if the cancel branch completes
                // first then no tick has been consumed (no event was ready).
                timer.tick().await;
                let now_ns = clock.get_time_ns();

                let event = TimeEvent::new(event_name, UUID4::new(), next_time_ns, now_ns);

                match callback {
                    #[cfg(feature = "python")]
                    TimeEventCallback::Python(ref callback) => {
                        call_python_with_time_event(event, callback);
                    }
                    TimeEventCallback::Rust(_) => {
                        let sender = sender
                            .as_ref()
                            .expect("timer event sender was unset for Rust callback system");
                        let handler = TimeEventHandlerV2::new(event, callback.clone());
                        sender.send(handler);
                    }
                }

                // Prepare next time interval
                next_time_ns += interval_ns;
                next_time_atomic.store(next_time_ns.as_u64(), atomic::Ordering::SeqCst);

                // Check if expired
                if let Some(stop_time_ns) = stop_time_ns {
                    if std::cmp::max(next_time_ns, now_ns) >= stop_time_ns {
                        break; // Timer expired
                    }
                }
            }
        });

        self.task_handle = Some(handle);
    }

    /// Cancels the timer.
    ///
    /// The timer will not generate a final event.
    pub fn cancel(&mut self) {
        log::debug!("Cancel timer '{}'", self.name);
        if let Some(ref handle) = self.task_handle {
            handle.abort();
        }
    }
}

#[cfg(feature = "python")]
fn call_python_with_time_event(event: TimeEvent, callback: &PyObject) {
    use nautilus_core::python::IntoPyObjectNautilusExt;
    use pyo3::types::PyCapsule;

    Python::with_gil(|py| {
        // Create new time event
        let capsule: PyObject = PyCapsule::new(py, event, None)
            .expect("Error creating `PyCapsule`")
            .into_py_any_unwrap(py);

        match callback.call1(py, (capsule,)) {
            Ok(_) => {}
            Err(e) => tracing::error!("Error on callback: {e:?}"),
        }
    });
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, rc::Rc};

    use nautilus_core::UnixNanos;
    use rstest::*;
    use ustr::Ustr;

    use super::{LiveTimer, TestTimer, TimeEvent, TimeEventCallback};

    #[rstest]
    fn test_test_timer_pop_event() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1).unwrap(),
            UnixNanos::from(1),
            None,
            false,
        );

        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[rstest]
    fn test_test_timer_advance_within_next_time_ns() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(5).unwrap(),
            UnixNanos::default(),
            None,
            false,
        );
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(1)).collect();
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(2)).collect();
        let _: Vec<TimeEvent> = timer.advance(UnixNanos::from(3)).collect();
        assert_eq!(timer.advance(UnixNanos::from(4)).count(), 0);
        assert_eq!(timer.next_time_ns, 5);
        assert!(!timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_up_to_next_time_ns() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1).unwrap(),
            UnixNanos::default(),
            None,
            false,
        );
        assert_eq!(timer.advance(UnixNanos::from(1)).count(), 1);
        assert!(!timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_up_to_next_time_ns_with_stop_time() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1).unwrap(),
            UnixNanos::default(),
            Some(UnixNanos::from(2)),
            false,
        );
        assert_eq!(timer.advance(UnixNanos::from(2)).count(), 2);
        assert!(timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_beyond_next_time_ns() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1).unwrap(),
            UnixNanos::default(),
            Some(UnixNanos::from(5)),
            false,
        );
        assert_eq!(timer.advance(UnixNanos::from(5)).count(), 5);
        assert!(timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_beyond_stop_time() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1).unwrap(),
            UnixNanos::default(),
            Some(UnixNanos::from(5)),
            false,
        );
        assert_eq!(timer.advance(UnixNanos::from(10)).count(), 5);
        assert!(timer.is_expired);
    }

    #[rstest]
    fn test_test_timer_advance_exact_boundary() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(5).unwrap(),
            UnixNanos::from(0),
            None,
            false,
        );
        let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(5)).collect();
        assert_eq!(events.len(), 1, "Expected one event at the 5 ns boundary");

        let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(10)).collect();
        assert_eq!(events.len(), 1, "Expected one event at the 10 ns boundary");
    }

    #[rstest]
    fn test_test_timer_fire_immediately_true() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(5).unwrap(),
            UnixNanos::from(10),
            None,
            true, // fire_immediately = true
        );

        // With fire_immediately=true, next_time_ns should be start_time_ns
        assert_eq!(timer.next_time_ns(), UnixNanos::from(10));

        // Advance to start time should produce an event
        let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(10)).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].ts_event, UnixNanos::from(10));

        // Next event should be at start_time + interval
        assert_eq!(timer.next_time_ns(), UnixNanos::from(15));
    }

    #[rstest]
    fn test_test_timer_fire_immediately_false() {
        let mut timer = TestTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(5).unwrap(),
            UnixNanos::from(10),
            None,
            false, // fire_immediately = false
        );

        // With fire_immediately=false, next_time_ns should be start_time_ns + interval
        assert_eq!(timer.next_time_ns(), UnixNanos::from(15));

        // Advance to start time should produce no events
        let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(10)).collect();
        assert_eq!(events.len(), 0);

        // Advance to first interval should produce an event
        let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(15)).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].ts_event, UnixNanos::from(15));
    }

    #[rstest]
    fn test_live_timer_fire_immediately_field() {
        let timer = LiveTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1000).unwrap(),
            UnixNanos::from(100),
            None,
            TimeEventCallback::Rust(Rc::new(|_| {})),
            true, // fire_immediately = true
            None, // time_event_sender
        );

        // Verify the field is set correctly
        assert!(timer.fire_immediately);

        // With fire_immediately=true, next_time_ns should be start_time_ns
        assert_eq!(timer.next_time_ns(), UnixNanos::from(100));
    }

    #[rstest]
    fn test_live_timer_fire_immediately_false_field() {
        let timer = LiveTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1000).unwrap(),
            UnixNanos::from(100),
            None,
            TimeEventCallback::Rust(Rc::new(|_| {})),
            false, // fire_immediately = false
            None,  // time_event_sender
        );

        // Verify the field is set correctly
        assert!(!timer.fire_immediately);

        // With fire_immediately=false, next_time_ns should be start_time_ns + interval
        assert_eq!(timer.next_time_ns(), UnixNanos::from(1100));
    }

    ////////////////////////////////////////////////////////////////////////////////
    // Property-based testing
    ////////////////////////////////////////////////////////////////////////////////

    use proptest::prelude::*;

    #[derive(Clone, Debug)]
    enum TimerOperation {
        AdvanceTime(u64),
        Cancel,
    }

    fn timer_operation_strategy() -> impl Strategy<Value = TimerOperation> {
        prop_oneof![
            8 => prop::num::u64::ANY.prop_map(|v| TimerOperation::AdvanceTime(v % 1000 + 1)),
            2 => Just(TimerOperation::Cancel),
        ]
    }

    fn timer_config_strategy() -> impl Strategy<Value = (u64, u64, Option<u64>, bool)> {
        (
            1u64..=100u64,                    // interval_ns (1-100)
            0u64..=50u64,                     // start_time_ns (0-50)
            prop::option::of(51u64..=200u64), // stop_time_ns (51-200 or None)
            prop::bool::ANY,                  // fire_immediately
        )
    }

    fn timer_test_strategy()
    -> impl Strategy<Value = (Vec<TimerOperation>, (u64, u64, Option<u64>, bool))> {
        (
            prop::collection::vec(timer_operation_strategy(), 5..=50),
            timer_config_strategy(),
        )
    }

    fn test_timer_with_operations(
        operations: Vec<TimerOperation>,
        (interval_ns, start_time_ns, stop_time_ns, fire_immediately): (u64, u64, Option<u64>, bool),
    ) {
        let mut timer = TestTimer::new(
            Ustr::from("PROP_TEST_TIMER"),
            NonZeroU64::new(interval_ns).unwrap(),
            UnixNanos::from(start_time_ns),
            stop_time_ns.map(UnixNanos::from),
            fire_immediately,
        );

        let mut current_time = start_time_ns;

        for operation in operations {
            if timer.is_expired() {
                break;
            }

            match operation {
                TimerOperation::AdvanceTime(delta) => {
                    let to_time = current_time + delta;
                    let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(to_time)).collect();
                    current_time = to_time;

                    // Verify event ordering and timing
                    for (i, event) in events.iter().enumerate() {
                        // Event timestamps should be in order
                        if i > 0 {
                            assert!(
                                event.ts_event >= events[i - 1].ts_event,
                                "Events should be in chronological order"
                            );
                        }

                        // Event timestamp should be within reasonable bounds
                        assert!(
                            event.ts_event.as_u64() >= start_time_ns,
                            "Event timestamp should not be before start time"
                        );

                        assert!(
                            event.ts_event.as_u64() <= to_time,
                            "Event timestamp should not be after advance time"
                        );

                        // If there's a stop time, event should not exceed it
                        if let Some(stop_time_ns) = stop_time_ns {
                            assert!(
                                event.ts_event.as_u64() <= stop_time_ns,
                                "Event timestamp should not exceed stop time"
                            );
                        }
                    }
                }
                TimerOperation::Cancel => {
                    timer.cancel();
                    assert!(timer.is_expired(), "Timer should be expired after cancel");
                }
            }

            // Timer invariants
            if !timer.is_expired() {
                // Next time should be properly spaced
                let expected_interval_multiple = if fire_immediately {
                    timer.next_time_ns().as_u64() >= start_time_ns
                } else {
                    timer.next_time_ns().as_u64() >= start_time_ns + interval_ns
                };
                assert!(
                    expected_interval_multiple,
                    "Next time should respect interval spacing"
                );

                // If timer has stop time, check if it should be considered logically expired
                // Note: Timer only becomes actually expired when advance() or next() is called
                if let Some(stop_time_ns) = stop_time_ns {
                    if timer.next_time_ns().as_u64() > stop_time_ns {
                        // The timer should expire on the next advance/iteration
                        let mut test_timer = timer.clone();
                        let events: Vec<TimeEvent> = test_timer
                            .advance(UnixNanos::from(stop_time_ns + 1))
                            .collect();
                        assert!(
                            events.is_empty() || test_timer.is_expired(),
                            "Timer should not generate events beyond stop time"
                        );
                    }
                }
            }
        }

        // Final consistency check: if timer is not expired and we haven't hit stop time,
        // advancing far enough should eventually expire it
        if !timer.is_expired() && stop_time_ns.is_some() {
            let events: Vec<TimeEvent> = timer
                .advance(UnixNanos::from(stop_time_ns.unwrap() + 1000))
                .collect();
            assert!(
                timer.is_expired() || events.is_empty(),
                "Timer should eventually expire or stop generating events"
            );
        }
    }

    proptest! {
        #[test]
        fn prop_timer_advance_operations((operations, config) in timer_test_strategy()) {
            test_timer_with_operations(operations, config);
        }

        #[test]
        fn prop_timer_interval_consistency(
            interval_ns in 1u64..=100u64,
            start_time_ns in 0u64..=50u64,
            fire_immediately in prop::bool::ANY,
            advance_count in 1usize..=20usize,
        ) {
            let mut timer = TestTimer::new(
                Ustr::from("CONSISTENCY_TEST"),
                NonZeroU64::new(interval_ns).unwrap(),
                UnixNanos::from(start_time_ns),
                None, // No stop time for this test
                fire_immediately,
            );

            let mut previous_event_time = if fire_immediately { start_time_ns } else { start_time_ns + interval_ns };

            for _ in 0..advance_count {
                let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(previous_event_time)).collect();

                if !events.is_empty() {
                    // Should get exactly one event at the expected time
                    prop_assert_eq!(events.len(), 1);
                    prop_assert_eq!(events[0].ts_event.as_u64(), previous_event_time);
                }

                previous_event_time += interval_ns;
            }
        }
    }
}

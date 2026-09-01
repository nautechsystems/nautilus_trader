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

//! Real-time and test timers for use with `Clock` implementations.
//!
//! Defines [`TimeEvent`] values, callback and handler types, heap scheduling order, and the
//! deterministic [`TestTimer`] iterator. The event and callback primitives are shared by test and
//! live clock implementations.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    num::NonZeroU64,
    rc::Rc,
    sync::Arc,
};

use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{FAILED, check_valid_string_utf8},
};
#[cfg(feature = "python")]
use pyo3::{Py, PyAny, Python};
use ustr::Ustr;

/// Returns a positive nanosecond interval, coercing zero to one nanosecond.
#[must_use]
pub fn create_valid_interval(interval_ns: u64) -> NonZeroU64 {
    NonZeroU64::new(interval_ns).unwrap_or(NonZeroU64::MIN)
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.common", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
/// Represents a named timer event.
///
/// `ts_event` records the scheduled event time, while `ts_init` records
/// when the event instance was initialized.
pub struct TimeEvent {
    /// The timer event name.
    pub name: Ustr,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event is scheduled to occur.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl TimeEvent {
    /// Creates a time event with the supplied identity and timestamps.
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
            "{}(name={}, event_id={}, ts_event={}, ts_init={})",
            stringify!(TimeEvent),
            self.name,
            self.event_id,
            self.ts_event,
            self.ts_init
        )
    }
}

/// Orders a [`TimeEvent`] for earliest-first scheduling in a
/// [`BinaryHeap`](std::collections::BinaryHeap).
///
/// The reversed ordering makes the heap pop events in ascending order by `ts_event`, then `name`,
/// `ts_init`, and `event_id`.
#[repr(transparent)] // Guarantees zero-cost abstraction with identical memory layout
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledTimeEvent(
    /// The time event to schedule.
    pub TimeEvent,
);

impl ScheduledTimeEvent {
    /// Creates a scheduled wrapper for `event`.
    #[must_use]
    pub const fn new(event: TimeEvent) -> Self {
        Self(event)
    }

    /// Returns the wrapped time event.
    #[must_use]
    pub fn into_inner(self) -> TimeEvent {
        self.0
    }
}

impl PartialOrd for ScheduledTimeEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledTimeEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse order for max heap: earlier timestamps have higher priority
        cmp_time_events(&other.0, &self.0)
    }
}

#[cfg(feature = "python")]
/// Wraps a Python callable that handles time events.
pub struct PythonTimeEventCallback {
    callback: Py<PyAny>,
}

#[cfg(feature = "python")]
impl PythonTimeEventCallback {
    /// Wraps a Python callable as a time event callback.
    #[must_use]
    pub const fn new(callback: Py<PyAny>) -> Self {
        Self { callback }
    }

    /// Invokes the Python callback for `event`.
    ///
    /// Logs and suppresses any exception raised by the callback.
    pub fn call(&self, event: TimeEvent) {
        Python::attach(|py| {
            if let Err(e) = self.callback.call1(py, (event,)) {
                log::error!("Python time event callback raised exception: {e}");
            }
        });
    }
}

#[cfg(feature = "python")]
impl Debug for PythonTimeEventCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PythonTimeEventCallback))
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
/// Represents a callback invoked for time events.
///
/// # Variants
///
/// - `Python`: For Python callbacks (requires `python` feature).
/// - `Rust`: Thread-safe callbacks using `Arc`. Use when the closure is `Send + Sync`.
/// - `RustLocal`: Single-threaded callbacks using `Rc`. Use when capturing `Rc<RefCell<...>>`.
///
/// # Choosing Between `Rust` and `RustLocal`
///
/// Use `Rust` (thread-safe) when:
/// - The callback does not capture `Rc<RefCell<...>>` or other non-`Send` types.
/// - The closure is `Send + Sync` (most simple closures qualify).
///
/// Use `RustLocal` when:
/// - The callback captures `Rc<RefCell<...>>` for shared mutable state.
/// - Thread safety constraints prevent using `Arc`.
///
/// `RustLocal` works with `TestClock` and with `LiveClock` when its event channel
/// is drained on the callback's originating thread.
///
/// # Automatic Conversion
///
/// - Closures that are `Fn + Send + Sync + 'static` automatically convert to `Rust`.
/// - `Rc<dyn Fn(TimeEvent)>` converts to `RustLocal`.
/// - `Arc<dyn Fn(TimeEvent) + Send + Sync>` converts to `Rust`.
pub enum TimeEventCallback {
    /// Python callable for use from Python via PyO3.
    #[cfg(feature = "python")]
    Python(Arc<PythonTimeEventCallback>),
    /// Thread-safe Rust callback using `Arc` (`Send + Sync`).
    Rust(Arc<dyn Fn(TimeEvent) + Send + Sync>),
    /// Local Rust callback using `Rc` (not `Send`/`Sync`).
    RustLocal(Rc<dyn Fn(TimeEvent)>),
}

impl Debug for TimeEventCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "python")]
            Self::Python(_) => f.write_str("Python callback"),
            Self::Rust(_) => f.write_str("Rust callback (thread-safe)"),
            Self::RustLocal(_) => f.write_str("Rust callback (local)"),
        }
    }
}

impl TimeEventCallback {
    /// Returns `true` if this is a local (non-thread-safe) Rust callback.
    ///
    /// Local callbacks use `Rc` internally and require creation, cloning, dropping,
    /// and invocation to stay on the originating thread.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        matches!(self, Self::RustLocal(_))
    }

    /// Invokes the callback for the given `TimeEvent`.
    ///
    /// For Python callbacks, exceptions are logged as errors rather than panicking.
    ///
    /// # Panics
    ///
    /// Panics from Rust callbacks propagate to the caller.
    pub fn call(&self, event: TimeEvent) {
        match self {
            #[cfg(feature = "python")]
            Self::Python(callback) => callback.call(event),
            Self::Rust(callback) => callback(event),
            Self::RustLocal(callback) => callback(event),
        }
    }
}

impl<F> From<F> for TimeEventCallback
where
    F: Fn(TimeEvent) + Send + Sync + 'static,
{
    fn from(value: F) -> Self {
        Self::Rust(Arc::new(value))
    }
}

impl From<Arc<dyn Fn(TimeEvent) + Send + Sync>> for TimeEventCallback {
    fn from(value: Arc<dyn Fn(TimeEvent) + Send + Sync>) -> Self {
        Self::Rust(value)
    }
}

impl From<Rc<dyn Fn(TimeEvent)>> for TimeEventCallback {
    fn from(value: Rc<dyn Fn(TimeEvent)>) -> Self {
        Self::RustLocal(value)
    }
}

#[cfg(feature = "python")]
impl From<Py<PyAny>> for TimeEventCallback {
    fn from(value: Py<PyAny>) -> Self {
        Self::from_python_time_event(value)
    }
}

#[cfg(feature = "python")]
impl TimeEventCallback {
    /// Creates a Python callback that receives a PyO3 `TimeEvent`.
    #[must_use]
    pub fn from_python_time_event(callback: Py<PyAny>) -> Self {
        Self::Python(Arc::new(PythonTimeEventCallback::new(callback)))
    }
}

#[repr(C)]
#[derive(Clone, Debug)]
/// Pairs a [`TimeEvent`] with its callback for ordered dispatch.
///
/// Natural ordering is ascending by `ts_event`, then `name`, `ts_init`, and `event_id`.
pub struct TimeEventHandler {
    /// The time event.
    pub event: TimeEvent,
    /// The callable handler for the event.
    pub callback: TimeEventCallback,
}

impl TimeEventHandler {
    /// Creates a handler for `event` and `callback`.
    #[must_use]
    pub const fn new(event: TimeEvent, callback: TimeEventCallback) -> Self {
        Self { event, callback }
    }

    /// Dispatches the event to the installed message-bus tap, then invokes its callback.
    ///
    /// # Panics
    ///
    /// Panics from the message-bus tap or a Rust callback propagate to the caller.
    pub fn run(self) {
        let Self { event, callback } = self;
        crate::msgbus::dispatch_tap_time_event(&event);
        callback.call(event);
    }
}

impl PartialOrd for TimeEventHandler {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TimeEventHandler {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for TimeEventHandler {}

impl Ord for TimeEventHandler {
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_time_events(&self.event, &other.event)
    }
}

fn cmp_time_events(left: &TimeEvent, right: &TimeEvent) -> Ordering {
    left.ts_event
        .cmp(&right.ts_event)
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.ts_init.cmp(&right.ts_init))
        .then_with(|| left.event_id.as_str().cmp(right.event_id.as_str()))
}

pub(crate) trait Timer {
    fn is_expired(&self) -> bool;
    fn cancel(&mut self);
}

/// A deterministic interval timer for use with a [`TestClock`](crate::clock::TestClock).
///
/// The timer generates scheduled events through an optional inclusive stop time as its iterator is
/// consumed.
#[derive(Clone, Debug)]
pub struct TestTimer {
    /// The name of the timer.
    pub name: Ustr,
    /// The interval between timer events in nanoseconds.
    pub interval_ns: NonZeroU64,
    /// The start time of the timer in UNIX nanoseconds.
    pub start_time_ns: UnixNanos,
    /// The optional inclusive stop time of the timer in UNIX nanoseconds.
    pub stop_time_ns: Option<UnixNanos>,
    /// Whether the first event fires at the start time instead of after one interval.
    pub fire_immediately: bool,
    next_time_ns: UnixNanos,
    is_expired: bool,
}

impl TestTimer {
    /// Creates a test timer with the supplied schedule.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `name` is not a valid string.
    /// - `fire_immediately` is `false` and `start_time_ns + interval_ns` exceeds the
    ///   [`UnixNanos`] range.
    #[must_use]
    pub fn new(
        name: Ustr,
        interval_ns: NonZeroU64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        fire_immediately: bool,
    ) -> Self {
        check_valid_string_utf8(name, stringify!(name)).expect(FAILED);

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

    /// Returns a lazy iterator over events scheduled at or before `to_time_ns`.
    ///
    /// Consuming the iterator advances the timer. Events at `to_time_ns` and at the configured stop
    /// time are included.
    pub fn advance(&mut self, to_time_ns: UnixNanos) -> impl Iterator<Item = TimeEvent> + '_ {
        // Calculate how many events should fire up to and including to_time_ns
        let advances = if self.next_time_ns <= to_time_ns {
            ((to_time_ns.as_u64() - self.next_time_ns.as_u64()) / self.interval_ns.get())
                .saturating_add(1)
        } else {
            0
        };
        self.take(advances as usize).map(|(event, _)| event)
    }

    /// Cancels the timer so it produces no further events.
    pub const fn cancel(&mut self) {
        self.is_expired = true;
    }
}

impl Timer for TestTimer {
    fn is_expired(&self) -> bool {
        Self::is_expired(self)
    }

    fn cancel(&mut self) {
        Self::cancel(self);
    }
}

impl Iterator for TestTimer {
    type Item = (TimeEvent, UnixNanos);

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_expired {
            return None;
        }

        // Check if current event would exceed stop time before creating the event
        if let Some(stop_time_ns) = self.stop_time_ns
            && self.next_time_ns > stop_time_ns
        {
            self.is_expired = true;
            return None;
        }

        let event_time_ns = self.next_time_ns;

        let item = (
            TimeEvent {
                name: self.name,
                event_id: UUID4::new(),
                ts_event: event_time_ns,
                ts_init: event_time_ns,
            },
            event_time_ns,
        );

        if let Some(following_time_ns) = event_time_ns.checked_add(self.interval_ns.get()) {
            self.next_time_ns = following_time_ns;
        } else {
            self.is_expired = true;
        }

        if self.stop_time_ns == Some(event_time_ns) {
            self.is_expired = true;
        }

        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BinaryHeap, num::NonZeroU64, rc::Rc};

    use nautilus_core::{UUID4, UnixNanos};
    #[cfg(feature = "python")]
    use pyo3::{
        Bound, PyResult, Python,
        types::{
            PyAnyMethods, PyCFunction, PyDict, PyList, PyListMethods, PyTuple, PyTupleMethods,
            PyTypeMethods,
        },
    };
    use rstest::*;
    use ustr::Ustr;

    use super::{
        ScheduledTimeEvent, TestTimer, TimeEvent, TimeEventCallback, TimeEventHandler,
        create_valid_interval,
    };
    use crate::msgbus::{
        BusTap, Endpoint, MStr, MessagingSwitchboard, Topic, clear_bus_tap, set_bus_tap,
    };

    #[rstest]
    #[case(0, 1)]
    #[case(1, 1)]
    #[case(25, 25)]
    fn test_create_valid_interval(#[case] interval_ns: u64, #[case] expected: u64) {
        assert_eq!(create_valid_interval(interval_ns).get(), expected);
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
        assert_eq!(
            timer.advance(UnixNanos::from(5)).count(),
            1,
            "Expected one event at the 5 ns boundary"
        );
        assert_eq!(
            timer.advance(UnixNanos::from(10)).count(),
            1,
            "Expected one event at the 10 ns boundary"
        );
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
        assert_eq!(timer.advance(UnixNanos::from(10)).count(), 0);

        // Advance to first interval should produce an event
        let events: Vec<TimeEvent> = timer.advance(UnixNanos::from(15)).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].ts_event, UnixNanos::from(15));
    }

    #[rstest]
    fn test_time_event_handler_ordering_uses_tie_breakers() {
        let callback = TimeEventCallback::from(|_: TimeEvent| {});

        let later_name = TimeEventHandler::new(
            TimeEvent::new(
                Ustr::from("TIME_BAR_ESM4-2-MINUTE-ASK-INTERNAL"),
                UUID4::from("00000000-0000-4000-8000-000000000003"),
                100.into(),
                100.into(),
            ),
            callback.clone(),
        );
        let earlier_name = TimeEventHandler::new(
            TimeEvent::new(
                Ustr::from("SPREAD_QUOTE_ESM4"),
                UUID4::from("00000000-0000-4000-8000-000000000002"),
                100.into(),
                100.into(),
            ),
            callback.clone(),
        );
        let later_init = TimeEventHandler::new(
            TimeEvent::new(
                Ustr::from("SPREAD_QUOTE_ESM4"),
                UUID4::from("00000000-0000-4000-8000-000000000004"),
                100.into(),
                101.into(),
            ),
            callback.clone(),
        );
        let later_id = TimeEventHandler::new(
            TimeEvent::new(
                Ustr::from("SPREAD_QUOTE_ESM4"),
                UUID4::from("00000000-0000-4000-8000-000000000005"),
                100.into(),
                100.into(),
            ),
            callback,
        );

        assert!(earlier_name < later_name);
        assert!(earlier_name < later_init);
        assert!(earlier_name < later_id);
        assert_ne!(earlier_name, later_id);
    }

    #[rstest]
    fn test_scheduled_time_event_ordering_laws() {
        let base = ScheduledTimeEvent::new(TimeEvent::new(
            Ustr::from("ALPHA"),
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            100.into(),
            10.into(),
        ));
        let variants = [
            base.clone(),
            ScheduledTimeEvent::new(TimeEvent::new(
                Ustr::from("BETA"),
                base.0.event_id,
                base.0.ts_event,
                base.0.ts_init,
            )),
            ScheduledTimeEvent::new(TimeEvent::new(
                base.0.name,
                UUID4::from("00000000-0000-4000-8000-000000000002"),
                base.0.ts_event,
                base.0.ts_init,
            )),
            ScheduledTimeEvent::new(TimeEvent::new(
                base.0.name,
                base.0.event_id,
                101.into(),
                base.0.ts_init,
            )),
            ScheduledTimeEvent::new(TimeEvent::new(
                base.0.name,
                base.0.event_id,
                base.0.ts_event,
                11.into(),
            )),
        ];

        for a in &variants {
            for b in &variants {
                assert_eq!(a == b, a.cmp(b).is_eq());
                assert_eq!(a.partial_cmp(b), Some(a.cmp(b)));
                assert_eq!(a.cmp(b), b.cmp(a).reverse());
            }
        }
    }

    #[rstest]
    fn test_scheduled_time_event_heap_ordering() {
        let expected = [
            TimeEvent::new(
                Ustr::from("ALPHA"),
                UUID4::from("00000000-0000-4000-8000-000000000001"),
                100.into(),
                10.into(),
            ),
            TimeEvent::new(
                Ustr::from("ALPHA"),
                UUID4::from("00000000-0000-4000-8000-000000000002"),
                100.into(),
                10.into(),
            ),
            TimeEvent::new(
                Ustr::from("ALPHA"),
                UUID4::from("00000000-0000-4000-8000-000000000003"),
                100.into(),
                11.into(),
            ),
            TimeEvent::new(
                Ustr::from("BETA"),
                UUID4::from("00000000-0000-4000-8000-000000000004"),
                100.into(),
                10.into(),
            ),
            TimeEvent::new(
                Ustr::from("ALPHA"),
                UUID4::from("00000000-0000-4000-8000-000000000005"),
                101.into(),
                10.into(),
            ),
        ];
        let insertion_order = [4, 1, 3, 0, 2];
        let mut heap = BinaryHeap::new();

        for index in insertion_order {
            heap.push(ScheduledTimeEvent::new(expected[index].clone()));
        }

        let popped = std::iter::from_fn(|| heap.pop().map(ScheduledTimeEvent::into_inner))
            .collect::<Vec<_>>();
        assert_eq!(popped, expected);
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_python_callback_passes_time_event() {
        Python::initialize();

        Python::attach(|py| {
            let seen = PyList::empty(py);
            let seen_obj = seen.clone().unbind().into_any();
            let callback = new_sync_py_callback(
                py,
                move |args: &Bound<'_, PyTuple>,
                      _kwargs: Option<&Bound<'_, PyDict>>|
                      -> PyResult<()> {
                    let arg = args.get_item(0)?;
                    let type_name = arg.get_type().name()?.to_string();
                    Python::attach(|py| seen_obj.call_method1(py, "append", (type_name,)))?;
                    Ok(())
                },
            )
            .expect("callback should create")
            .into_any()
            .unbind();

            let event = TimeEvent::new(
                Ustr::from("PY_CALLBACK_MODE"),
                UUID4::from("00000000-0000-4000-8000-000000000007"),
                UnixNanos::from(100),
                UnixNanos::from(99),
            );

            TimeEventCallback::from_python_time_event(callback).call(event);

            assert_eq!(seen.len(), 1);
            assert_eq!(
                seen.get_item(0).unwrap().extract::<String>().unwrap(),
                "TimeEvent"
            );
        });
    }

    #[cfg(feature = "python")]
    fn new_sync_py_callback<F>(py: Python<'_>, closure: F) -> PyResult<Bound<'_, PyCFunction>>
    where
        F: Fn(&Bound<'_, PyTuple>, Option<&Bound<'_, PyDict>>) -> PyResult<()>
            + Send
            + Sync
            + 'static,
    {
        PyCFunction::new_closure(py, None, None, closure)
    }

    #[derive(Default)]
    struct RecordingTimeEventTap {
        time_events: RefCell<Vec<(String, TimeEvent)>>,
    }

    impl RecordingTimeEventTap {
        fn time_events(&self) -> Vec<(String, TimeEvent)> {
            self.time_events.borrow().clone()
        }
    }

    impl BusTap for RecordingTimeEventTap {
        fn on_publish(&self, topic: MStr<Topic>, message: &dyn std::any::Any) {
            if let Some(event) = message.downcast_ref::<TimeEvent>() {
                self.time_events
                    .borrow_mut()
                    .push((topic.to_string(), event.clone()));
            }
        }

        fn on_send(&self, _endpoint: MStr<Endpoint>, _message: &dyn std::any::Any) {}
    }

    #[rstest]
    fn test_time_event_handler_run_dispatches_tap_before_callback() {
        let event = TimeEvent::new(
            Ustr::from("strategy.heartbeat"),
            UUID4::from("00000000-0000-4000-8000-000000000006"),
            UnixNanos::from(100),
            UnixNanos::from(99),
        );
        let tap = Rc::new(RecordingTimeEventTap::default());
        let callback_seen: Rc<RefCell<Vec<TimeEvent>>> = Rc::new(RefCell::new(Vec::new()));
        let expected_topic = MessagingSwitchboard::time_event_topic().to_string();
        let callback_expected = event.clone();
        let callback_expected_topic = expected_topic.clone();
        let callback_tap = Rc::clone(&tap);
        let callback_seen_ref = Rc::clone(&callback_seen);
        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |callback_event| {
            assert_eq!(
                callback_tap.time_events(),
                vec![(callback_expected_topic.clone(), callback_expected.clone())],
            );
            callback_seen_ref.borrow_mut().push(callback_event);
        });

        set_bus_tap(tap.clone());
        TimeEventHandler::new(event.clone(), TimeEventCallback::from(callback)).run();
        clear_bus_tap();

        assert_eq!(tap.time_events(), vec![(expected_topic, event.clone())]);
        assert_eq!(*callback_seen.borrow(), vec![event]);
    }

    use proptest::{prelude::*, test_runner::TestCaseResult};

    #[derive(Clone, Debug)]
    enum TimerOperation {
        AdvanceTime(u64),
        Cancel,
    }

    fn timer_operation_strategy() -> impl Strategy<Value = TimerOperation> {
        prop_oneof![
            8 => (0u64..=1000).prop_map(TimerOperation::AdvanceTime),
            2 => Just(TimerOperation::Cancel),
        ]
    }

    fn timer_config_strategy() -> impl Strategy<Value = (u64, u64, Option<u64>, bool)> {
        (
            1u64..=1000,
            timer_start_time_strategy(),
            prop::option::of(0u64..=20_000),
            prop::bool::ANY,
        )
            .prop_map(
                |(interval_ns, start_time_ns, stop_after_ns, fire_immediately)| {
                    (
                        interval_ns,
                        start_time_ns,
                        stop_after_ns.map(|offset| start_time_ns + offset),
                        fire_immediately,
                    )
                },
            )
    }

    fn timer_start_time_strategy() -> impl Strategy<Value = u64> {
        prop_oneof![
            6 => 0u64..=u64::MAX - TIMER_TIME_HEADROOM,
            2 => 0u64..=1_000_000,
            1 => Just(1_700_000_000_000_000_000),
            1 => Just(u64::MAX - TIMER_TIME_HEADROOM),
        ]
    }

    fn timer_test_strategy()
    -> impl Strategy<Value = (Vec<TimerOperation>, (u64, u64, Option<u64>, bool))> {
        (
            prop::collection::vec(timer_operation_strategy(), 5..=75),
            timer_config_strategy(),
        )
    }

    fn test_timer_with_operations(
        operations: Vec<TimerOperation>,
        (interval_ns, start_time_ns, stop_time_ns, fire_immediately): (u64, u64, Option<u64>, bool),
    ) -> TestCaseResult {
        let mut timer = TestTimer::new(
            Ustr::from("PROP_TEST_TIMER"),
            NonZeroU64::new(interval_ns).unwrap(),
            UnixNanos::from(start_time_ns),
            stop_time_ns.map(UnixNanos::from),
            fire_immediately,
        );

        let mut current_time = start_time_ns;
        let mut expected_next = if fire_immediately {
            start_time_ns
        } else {
            start_time_ns + interval_ns
        };
        let mut expected_expired = false;

        for operation in operations {
            match operation {
                TimerOperation::AdvanceTime(delta) => {
                    let to_time = current_time + delta;
                    let actual: Vec<(Ustr, u64, u64)> = timer
                        .advance(UnixNanos::from(to_time))
                        .map(|event| time_event_state(&event))
                        .collect();
                    let expected = expected_event_states(
                        expected_event_times(
                            to_time,
                            interval_ns,
                            stop_time_ns,
                            &mut expected_next,
                            &mut expected_expired,
                        ),
                        Ustr::from("PROP_TEST_TIMER"),
                    );
                    current_time = to_time;

                    prop_assert_eq!(actual, expected);
                }
                TimerOperation::Cancel => {
                    timer.cancel();
                    expected_expired = true;
                }
            }

            prop_assert_eq!(timer.is_expired(), expected_expired);
            prop_assert_eq!(timer.next_time_ns().as_u64(), expected_next);
        }

        if !expected_expired && let Some(stop_time_ns) = stop_time_ns {
            let to_time = stop_time_ns.saturating_add(interval_ns);
            let actual: Vec<(Ustr, u64, u64)> = timer
                .advance(UnixNanos::from(to_time))
                .map(|event| time_event_state(&event))
                .collect();
            let expected = expected_event_states(
                expected_event_times(
                    to_time,
                    interval_ns,
                    Some(stop_time_ns),
                    &mut expected_next,
                    &mut expected_expired,
                ),
                Ustr::from("PROP_TEST_TIMER"),
            );
            prop_assert_eq!(actual, expected);
            prop_assert!(expected_expired);
            prop_assert!(timer.is_expired());
            prop_assert_eq!(timer.next_time_ns().as_u64(), expected_next);
        }

        Ok(())
    }

    fn expected_event_times(
        to_time: u64,
        interval_ns: u64,
        stop_time_ns: Option<u64>,
        next_time: &mut u64,
        is_expired: &mut bool,
    ) -> Vec<u64> {
        let mut events = Vec::new();

        while !*is_expired && *next_time <= to_time {
            if let Some(stop_time_ns) = stop_time_ns
                && *next_time > stop_time_ns
            {
                *is_expired = true;
                break;
            }

            let event_time = *next_time;
            events.push(event_time);
            let Some(following_time) = event_time.checked_add(interval_ns) else {
                *is_expired = true;
                break;
            };
            *next_time = following_time;

            if Some(event_time) == stop_time_ns {
                *is_expired = true;
                break;
            }
        }

        events
    }

    proptest! {
        #[rstest]
        fn prop_timer_advance_operations((operations, config) in timer_test_strategy()) {
            test_timer_with_operations(operations, config)?;
        }

        #[rstest]
        fn prop_timer_advance_batching_is_consistent(
            interval_ns in 1u64..=1000,
            start_time_ns in timer_start_time_strategy(),
            fire_immediately in prop::bool::ANY,
            advance_count in 1u64..=20,
        ) {
            let mut timer = TestTimer::new(
                Ustr::from("CONSISTENCY_TEST"),
                NonZeroU64::new(interval_ns).unwrap(),
                UnixNanos::from(start_time_ns),
                None, // No stop time for this test
                fire_immediately,
            );

            let first_event_time = if fire_immediately { start_time_ns } else { start_time_ns + interval_ns };
            let final_event_time = first_event_time + interval_ns * (advance_count - 1);
            let expected = expected_event_states(
                (0..advance_count)
                    .map(|index| first_event_time + interval_ns * index)
                    .collect(),
                Ustr::from("CONSISTENCY_TEST"),
            );

            let mut batched_timer = timer.clone();
            let batched: Vec<(Ustr, u64, u64)> = batched_timer
                .advance(UnixNanos::from(final_event_time))
                .map(|event| time_event_state(&event))
                .collect();

            let mut stepped = Vec::new();

            for event_time in
                (0..advance_count).map(|index| first_event_time + interval_ns * index)
            {
                stepped.extend(
                    timer
                        .advance(UnixNanos::from(event_time))
                        .map(|event| time_event_state(&event)),
                );
            }

            prop_assert_eq!(&batched, &expected);
            prop_assert_eq!(&stepped, &expected);
            prop_assert_eq!(timer.next_time_ns(), batched_timer.next_time_ns());
            prop_assert_eq!(timer.is_expired(), batched_timer.is_expired());
        }

        #[rstest]
        fn prop_timer_terminal_time_does_not_require_following_time(
            (interval_ns, event_headroom) in terminal_time_strategy(),
            fire_immediately in prop::bool::ANY,
            bounded in prop::bool::ANY,
        ) {
            let event_time_ns = u64::MAX - event_headroom;
            let start_time_ns = if fire_immediately {
                event_time_ns
            } else {
                event_time_ns - interval_ns
            };
            let mut timer = TestTimer::new(
                Ustr::from("TERMINAL_STOP_TEST"),
                NonZeroU64::new(interval_ns).unwrap(),
                UnixNanos::from(start_time_ns),
                bounded.then_some(UnixNanos::max()),
                fire_immediately,
            );

            let events: Vec<(Ustr, u64, u64)> = timer
                .advance(UnixNanos::max())
                .map(|event| time_event_state(&event))
                .collect();

            prop_assert_eq!(
                events,
                vec![(Ustr::from("TERMINAL_STOP_TEST"), event_time_ns, event_time_ns)]
            );
            prop_assert!(timer.is_expired());
            prop_assert_eq!(timer.next_time_ns(), UnixNanos::from(event_time_ns));
        }
    }

    const TIMER_TIME_HEADROOM: u64 = 100_000;

    fn time_event_state(event: &TimeEvent) -> (Ustr, u64, u64) {
        (event.name, event.ts_event.as_u64(), event.ts_init.as_u64())
    }

    fn expected_event_states(times: Vec<u64>, name: Ustr) -> Vec<(Ustr, u64, u64)> {
        times.into_iter().map(|time| (name, time, time)).collect()
    }

    fn terminal_time_strategy() -> impl Strategy<Value = (u64, u64)> {
        (1u64..=1000).prop_flat_map(|interval_ns| (Just(interval_ns), 0u64..interval_ns))
    }
}

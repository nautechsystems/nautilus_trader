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

//! Live timer implementation using Tokio for real-time scheduling.

use std::{
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{self, AtomicU64},
    },
};

use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{FAILED, check_valid_string_utf8},
    datetime::floor_to_nearest_microsecond,
    time::get_atomic_clock_realtime,
};
#[cfg(feature = "python")]
use pyo3::{Py, PyAny, Python};
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};
use ustr::Ustr;

use super::runtime::get_runtime;
use crate::{
    runner::TimeEventSender,
    timer::{TimeEvent, TimeEventCallback, TimeEventHandlerV2},
};

/// A live timer for use with a `LiveClock`.
///
/// `LiveTimer` triggers events at specified intervals in a real-time environment,
/// using Tokio's async runtime to handle scheduling and execution.
///
/// # Threading
///
/// The timer runs on the runtime thread that created it and dispatches events across threads as needed.
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
        check_valid_string_utf8(name, stringify!(name)).expect(FAILED);

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
    /// Panics if Rust-based callback system is active and no time event sender has been set.
    #[allow(unused_variables)]
    pub fn start(&mut self) {
        let event_name = self.name;
        let stop_time_ns = self.stop_time_ns;
        let interval_ns = self.interval_ns.get();
        let callback = self.callback.clone();

        // Get current time
        let clock = get_atomic_clock_realtime();
        let now_ns = clock.get_time_ns();

        // Check if the timer's alert time is in the past and adjust if needed
        let now_raw = now_ns.as_u64();
        let mut observed_next = self.next_time_ns.load(atomic::Ordering::SeqCst);

        if observed_next <= now_raw {
            loop {
                match self.next_time_ns.compare_exchange(
                    observed_next,
                    now_raw,
                    atomic::Ordering::SeqCst,
                    atomic::Ordering::SeqCst,
                ) {
                    Ok(_) => {
                        if observed_next < now_raw {
                            let original = UnixNanos::from(observed_next);
                            log::warn!(
                                "Timer '{event_name}' alert time {} was in the past, adjusted to current time for immediate fire",
                                original.to_rfc3339(),
                            );
                        }
                        observed_next = now_raw;
                        break;
                    }
                    Err(actual) => {
                        observed_next = actual;
                        if observed_next > now_raw {
                            break;
                        }
                    }
                }
            }
        }

        // Floor the next time to the nearest microsecond which is within the timers accuracy
        let mut next_time_ns = UnixNanos::from(floor_to_nearest_microsecond(observed_next));
        let next_time_atomic = self.next_time_ns.clone();
        next_time_atomic.store(next_time_ns.as_u64(), atomic::Ordering::SeqCst);

        let sender = self.sender.clone();

        let rt = get_runtime();
        let handle = rt.spawn(async move {
            let clock = get_atomic_clock_realtime();

            // 1-millisecond delay to account for the overhead of initializing a tokio timer
            let overhead = Duration::from_millis(1);
            let delay_ns = next_time_ns.saturating_sub(now_ns.as_u64());
            let mut delay = Duration::from_nanos(delay_ns);

            // Subtract the estimated startup overhead; saturating to zero for sub-ms delays
            if delay > overhead {
                delay -= overhead;
            } else {
                delay = Duration::from_nanos(0);
            }

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
                        debug_assert!(
                            sender.is_some(),
                            "LiveTimer with Rust callback requires TimeEventSender"
                        );
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
                if let Some(stop_time_ns) = stop_time_ns
                    && std::cmp::max(next_time_ns, now_ns) >= stop_time_ns
                {
                    break; // Timer expired
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
fn call_python_with_time_event(event: TimeEvent, callback: &Py<PyAny>) {
    use nautilus_core::python::IntoPyObjectNautilusExt;
    use pyo3::types::PyCapsule;

    Python::attach(|py| {
        // Create a new PyCapsule that owns `event` and registers a destructor so
        // the contained `TimeEvent` is properly freed once the capsule is
        // garbage-collected by Python. Without the destructor the memory would
        // leak because the capsule would not know how to drop the Rust value.

        // Register a destructor that simply drops the `TimeEvent` once the
        // capsule is freed on the Python side.
        let capsule: Py<PyAny> = PyCapsule::new_with_destructor(py, event, None, |_, _| {})
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
    use std::{num::NonZeroU64, sync::Arc};

    use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
    use rstest::*;
    use ustr::Ustr;

    use super::LiveTimer;
    use crate::{
        runner::TimeEventSender,
        timer::{TimeEventCallback, TimeEventHandlerV2},
    };

    #[rstest]
    fn test_live_timer_fire_immediately_field() {
        let timer = LiveTimer::new(
            Ustr::from("TEST_TIMER"),
            NonZeroU64::new(1000).unwrap(),
            UnixNanos::from(100),
            None,
            TimeEventCallback::from(|_| {}),
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
            TimeEventCallback::from(|_| {}),
            false, // fire_immediately = false
            None,  // time_event_sender
        );

        // Verify the field is set correctly
        assert!(!timer.fire_immediately);

        // With fire_immediately=false, next_time_ns should be start_time_ns + interval
        assert_eq!(timer.next_time_ns(), UnixNanos::from(1100));
    }

    #[rstest]
    fn test_live_timer_adjusts_past_due_start_time() {
        #[derive(Debug)]
        struct NoopSender;

        impl TimeEventSender for NoopSender {
            fn send(&self, _handler: TimeEventHandlerV2) {}
        }

        let sender = Arc::new(NoopSender);
        let mut timer = LiveTimer::new(
            Ustr::from("PAST_TIMER"),
            NonZeroU64::new(1).unwrap(),
            UnixNanos::from(0),
            None,
            TimeEventCallback::from(|_| {}),
            true,
            Some(sender),
        );

        let before = get_atomic_clock_realtime().get_time_ns();

        timer.start();

        assert!(timer.next_time_ns() >= before);

        timer.cancel();
    }
}

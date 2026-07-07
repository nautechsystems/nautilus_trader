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
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};
use ustr::Ustr;

use super::runtime::get_runtime;
use crate::{
    runner::TimeEventSender,
    timer::{TimeEvent, TimeEventCallback, TimeEventHandler, Timer},
};

const TIMER_STARTUP_OVERHEAD: Duration = Duration::from_millis(1);

fn should_fire_scheduled_time(next_time_ns: UnixNanos, stop_time_ns: Option<UnixNanos>) -> bool {
    stop_time_ns.is_none_or(|stop_time_ns| next_time_ns <= stop_time_ns)
}

fn expires_after_scheduled_time(next_time_ns: UnixNanos, stop_time_ns: Option<UnixNanos>) -> bool {
    stop_time_ns == Some(next_time_ns)
}

fn is_stop_boundary(next_time_ns: u64, stop_time_ns: Option<UnixNanos>) -> bool {
    stop_time_ns == Some(UnixNanos::from(next_time_ns))
}

fn should_adjust_past_due_time(
    observed_next: u64,
    now_ns: UnixNanos,
    stop_time_ns: Option<UnixNanos>,
) -> bool {
    observed_next <= now_ns.as_u64() && !is_stop_boundary(observed_next, stop_time_ns)
}

fn normalize_start_time_ns(
    observed_next: u64,
    now_ns: UnixNanos,
    stop_time_ns: Option<UnixNanos>,
) -> UnixNanos {
    if is_stop_boundary(observed_next, stop_time_ns) {
        return UnixNanos::from(observed_next);
    }

    let now_raw = now_ns.as_u64();
    let start_time_ns = if observed_next <= now_raw {
        now_raw
    } else {
        observed_next
    };

    UnixNanos::from(floor_to_nearest_microsecond(start_time_ns))
}

fn timer_start_delay(next_time_ns: UnixNanos, now_ns: UnixNanos) -> Duration {
    let delay = Duration::from_nanos(next_time_ns.saturating_sub(now_ns.as_u64()));

    // Subtract the estimated startup overhead, saturating to zero for sub-overhead delays.
    delay.saturating_sub(TIMER_STARTUP_OVERHEAD)
}

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
    canceled: bool,
    sender: Option<Arc<dyn TimeEventSender>>,
}

impl LiveTimer {
    /// Creates a new [`LiveTimer`] instance.
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

        log::trace!("Creating timer '{name}'");

        Self {
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            fire_immediately,
            next_time_ns: Arc::new(AtomicU64::new(next_time_ns)),
            callback,
            task_handle: None,
            canceled: false,
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
        self.canceled
            || self
                .task_handle
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
    /// Panics if using a Rust callback (`Rust` or `RustLocal`) without a `TimeEventSender`.
    #[allow(unused_variables)]
    pub fn start(&mut self) {
        let event_name = self.name;
        let stop_time_ns = self.stop_time_ns;
        let interval_ns = self.interval_ns.get();

        if self.callback.is_local() {
            log::debug!(
                "Timer '{event_name}' uses a RustLocal callback on a live Tokio timer; \
                 callback registry dispatch is needed to avoid cloning Rc on worker threads"
            );
        }

        let callback = self.callback.clone();

        // Get current time
        let clock = get_atomic_clock_realtime();
        let now_ns = clock.get_time_ns();

        // Check if the timer's alert time is in the past and adjust if needed
        let now_raw = now_ns.as_u64();
        let mut observed_next = self.next_time_ns.load(atomic::Ordering::SeqCst);

        if should_adjust_past_due_time(observed_next, now_ns, stop_time_ns) {
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
                        if !should_adjust_past_due_time(observed_next, now_ns, stop_time_ns) {
                            break;
                        }
                    }
                }
            }
        }

        // Floor the next time to the nearest microsecond which is within the timers accuracy
        let mut next_time_ns = normalize_start_time_ns(observed_next, now_ns, stop_time_ns);
        let next_time_atomic = self.next_time_ns.clone();
        next_time_atomic.store(next_time_ns.as_u64(), atomic::Ordering::SeqCst);

        let sender = self.sender.clone();

        let rt = get_runtime();
        let handle = rt.spawn(async move {
            let clock = get_atomic_clock_realtime();

            let start = Instant::now() + timer_start_delay(next_time_ns, now_ns);

            let mut timer = tokio::time::interval_at(start, Duration::from_nanos(interval_ns));

            loop {
                // Never fire an event scheduled past the stop time. The event's
                // `ts_event` is the scheduled `next_time_ns`, so the bound is
                // enforced on the scheduled time (matching `TestTimer`), not on
                // the wall-clock read used only for `ts_init`.
                if !should_fire_scheduled_time(next_time_ns, stop_time_ns) {
                    break; // Timer expired before this event
                }

                // `timer.tick` is cancellation safe, if the cancel branch completes
                // first then no tick has been consumed (no event was ready).
                timer.tick().await;
                let now_ns = clock.get_time_ns();

                let event = TimeEvent::new(event_name, UUID4::new(), next_time_ns, now_ns);

                if let Some(sender) = sender.as_ref() {
                    // TODO: `RustLocal` still clones an `Rc` on the timer worker.
                    // Move callbacks into an event-loop registry and send an id instead.
                    let handler = TimeEventHandler::new(event, callback.clone());
                    sender.send(handler);
                } else {
                    #[cfg(feature = "python")]
                    if matches!(&callback, TimeEventCallback::Python(_)) {
                        callback.call(event);
                    } else {
                        panic!("timer event sender was unset for Rust callback system");
                    }

                    #[cfg(not(feature = "python"))]
                    {
                        panic!("timer event sender was unset for Rust callback system");
                    }
                }

                // The event scheduled exactly at the stop time fires (inclusive
                // boundary), then the timer expires.
                let expires_after_fire = expires_after_scheduled_time(next_time_ns, stop_time_ns);

                // Prepare next time interval
                next_time_ns += interval_ns;
                next_time_atomic.store(next_time_ns.as_u64(), atomic::Ordering::SeqCst);

                if expires_after_fire {
                    break; // Timer expired at the stop boundary
                }
            }
        });

        self.task_handle = Some(handle);
        self.canceled = false;
    }

    /// Cancels the timer.
    ///
    /// The timer will not generate a final event.
    pub fn cancel(&mut self) {
        log::trace!("Cancel timer '{}'", self.name);

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        self.canceled = true;
    }
}

impl Timer for LiveTimer {
    fn is_expired(&self) -> bool {
        Self::is_expired(self)
    }

    fn cancel(&mut self) {
        Self::cancel(self);
    }
}

impl Drop for LiveTimer {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    #[cfg(feature = "python")]
    use std::{
        sync::{Arc, Mutex, mpsc},
        time::Duration as StdDuration,
    };

    use nautilus_core::UnixNanos;
    #[cfg(feature = "python")]
    use nautilus_core::time::get_atomic_clock_realtime;
    #[cfg(feature = "python")]
    use pyo3::{
        Python,
        types::{PyAnyMethods, PyList, PyListMethods},
    };
    use rstest::*;
    use ustr::Ustr;

    use super::LiveTimer;
    #[cfg(feature = "python")]
    use crate::runner::TimeEventSender;
    use crate::timer::TimeEventCallback;
    #[cfg(feature = "python")]
    use crate::timer::TimeEventHandler;

    #[rstest]
    fn test_live_timer_stop_bound_allows_unbounded_scheduled_time() {
        assert!(super::should_fire_scheduled_time(
            UnixNanos::from(100),
            None
        ));
        assert!(!super::expires_after_scheduled_time(
            UnixNanos::from(100),
            None
        ));
    }

    #[rstest]
    fn test_live_timer_stop_bound_skips_time_past_stop() {
        let next_time_ns = UnixNanos::from(110);
        let stop_time_ns = Some(UnixNanos::from(100));

        assert!(!super::should_fire_scheduled_time(
            next_time_ns,
            stop_time_ns
        ));
        assert!(!super::expires_after_scheduled_time(
            next_time_ns,
            stop_time_ns
        ));
    }

    #[rstest]
    fn test_live_timer_stop_bound_allows_time_before_stop_without_expiring() {
        let next_time_ns = UnixNanos::from(90);
        let stop_time_ns = Some(UnixNanos::from(100));

        assert!(super::should_fire_scheduled_time(
            next_time_ns,
            stop_time_ns
        ));
        assert!(!super::expires_after_scheduled_time(
            next_time_ns,
            stop_time_ns
        ));
    }

    #[rstest]
    fn test_live_timer_stop_bound_fires_time_at_stop_then_expires() {
        let next_time_ns = UnixNanos::from(100);
        let stop_time_ns = Some(UnixNanos::from(100));

        assert!(super::should_fire_scheduled_time(
            next_time_ns,
            stop_time_ns
        ));
        assert!(super::expires_after_scheduled_time(
            next_time_ns,
            stop_time_ns
        ));
    }

    #[rstest]
    fn test_live_timer_past_due_stop_boundary_is_not_adjusted_forward() {
        let observed_next = 100;
        let now = UnixNanos::from(110);
        let stop_time_ns = Some(UnixNanos::from(observed_next));

        assert!(!super::should_adjust_past_due_time(
            observed_next,
            now,
            stop_time_ns
        ));
    }

    #[rstest]
    fn test_live_timer_past_due_time_before_stop_is_adjusted_forward() {
        let observed_next = 90;
        let now = UnixNanos::from(110);
        let stop_time_ns = Some(UnixNanos::from(120));

        assert!(super::should_adjust_past_due_time(
            observed_next,
            now,
            stop_time_ns
        ));
    }

    #[rstest]
    fn test_live_timer_start_time_normalization_adjusts_past_due_time() {
        let observed_next = 1_234_567;
        let now = UnixNanos::from(2_345_678);

        assert_eq!(
            super::normalize_start_time_ns(observed_next, now, None),
            UnixNanos::from(2_345_000)
        );
    }

    #[rstest]
    fn test_live_timer_start_time_normalization_keeps_future_time() {
        let observed_next = 3_456_789;
        let now = UnixNanos::from(2_345_678);

        assert_eq!(
            super::normalize_start_time_ns(observed_next, now, None),
            UnixNanos::from(3_456_000)
        );
    }

    #[rstest]
    fn test_live_timer_start_time_normalization_keeps_stop_boundary_exact() {
        let observed_next = 1_234_567;
        let now = UnixNanos::from(2_345_678);
        let stop_time_ns = Some(UnixNanos::from(observed_next));

        assert_eq!(
            super::normalize_start_time_ns(observed_next, now, stop_time_ns),
            UnixNanos::from(observed_next)
        );
    }

    #[rstest]
    fn test_live_timer_start_delay_subtracts_startup_overhead() {
        let next_time_ns = UnixNanos::from(12_000_000);
        let now = UnixNanos::from(10_000_000);

        assert_eq!(
            super::timer_start_delay(next_time_ns, now),
            tokio::time::Duration::from_millis(1)
        );
    }

    #[rstest]
    fn test_live_timer_start_delay_saturates_below_startup_overhead() {
        let next_time_ns = UnixNanos::from(10_500_000);
        let now = UnixNanos::from(10_000_000);

        assert_eq!(
            super::timer_start_delay(next_time_ns, now),
            tokio::time::Duration::from_nanos(0)
        );
    }

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

    #[cfg(feature = "python")]
    #[rstest]
    fn test_live_timer_with_sender_defers_python_callback_to_handler() {
        #[derive(Debug)]
        struct ChannelSender {
            tx: Mutex<mpsc::Sender<TimeEventHandler>>,
        }

        impl TimeEventSender for ChannelSender {
            fn send(&self, handler: TimeEventHandler) {
                self.tx
                    .lock()
                    .expect("sender mutex should lock")
                    .send(handler)
                    .expect("handler should send");
            }
        }

        Python::initialize();

        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = py_list
                .getattr("append")
                .expect("append should exist")
                .unbind();
            let callback = TimeEventCallback::from(py_append);
            let (tx, rx) = mpsc::channel();
            let sender = Arc::new(ChannelSender { tx: Mutex::new(tx) });
            let now = get_atomic_clock_realtime().get_time_ns();

            let mut timer = LiveTimer::new(
                Ustr::from("PY_TIMER"),
                NonZeroU64::new(1_000_000).unwrap(),
                now,
                None,
                callback,
                true,
                Some(sender),
            );

            timer.start();
            let handler = rx
                .recv_timeout(StdDuration::from_secs(1))
                .expect("timer handler should arrive without acquiring the GIL on the worker");
            timer.cancel();

            assert_eq!(py_list.len(), 0);
            handler.run();
            assert_eq!(py_list.len(), 1);
        });
    }
}

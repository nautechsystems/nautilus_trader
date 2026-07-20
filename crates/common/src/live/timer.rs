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

//! Live timer scheduling and callback dispatch.
//!
//! # Scheduling
//!
//! The runtime interval starts `TIMER_STARTUP_OVERHEAD` before the nominal schedule to offset
//! task startup latency. Shorter delays saturate to immediate execution, while event timestamps
//! retain their nominal schedule. Stop times are inclusive.
//!
//! # Task lifecycle
//!
//! Each [`LiveTimer::start`] creates fresh public schedule and task-state atomics. Because aborting
//! a runtime task does not join it, task retirement linearizes restart, cancellation, and drop
//! against event reservation. Retirement either prevents the old task from dispatching or observes
//! the schedule advanced by its reserved event; fresh atomics keep that task from overwriting its
//! replacement's schedule.
//!
//! # Callback dispatch
//!
//! Thread-safe callbacks cross the worker boundary as [`TimeEventMessage`] values. `RustLocal`
//! callbacks remain in an owner-thread registry and cross the boundary only through tokens and
//! leases. Senderless Python callbacks run inline and publish their following schedule after the
//! callback returns.

use std::{
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{self, AtomicU8, AtomicU64},
    },
};

use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{FAILED, check_valid_string_utf8},
    datetime::floor_to_nearest_microsecond,
    time::get_atomic_clock_realtime,
};
use ustr::Ustr;

use super::dst::{
    self,
    task::JoinHandle,
    time::{Duration, Instant},
};
#[cfg(not(all(feature = "simulation", madsim)))]
use super::runtime::get_runtime;
use crate::{
    runner::{
        TimeEventCallbackLease, TimeEventCallbackToken, TimeEventMessage, TimeEventMessageFactory,
        TimeEventSender, register_time_event_callback,
    },
    timer::{TimeEvent, TimeEventCallback, Timer},
};

const TIMER_STARTUP_OVERHEAD: Duration = Duration::from_millis(1);
const TASK_ACTIVE: u8 = 0;
const TASK_FIRING: u8 = 1;
const TASK_RETIRED: u8 = 2;

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

    // Subtract the estimated startup overhead, saturating to zero for sub-overhead delays
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
    callback: OwnerCallback,
    task_handle: Option<JoinHandle<()>>,
    task_state: Option<Arc<TimerTaskState>>,
    canceled: bool,
    sender: Option<Arc<dyn TimeEventSender>>,
}

#[derive(Debug)]
struct TimerTaskState {
    status: AtomicU8,
    next_time_ns: AtomicU64,
}

#[derive(Debug)]
enum OwnerCallback {
    Direct(TimeEventMessageFactory),
    /// The callback is retained owner-side so a restarted timer (cancel or
    /// natural expiry closed the token) can register a fresh token.
    Registered {
        token: TimeEventCallbackToken,
        callback: TimeEventCallback,
    },
    Senderless(TimeEventCallback),
}

#[derive(Clone, Debug)]
enum WorkerDispatch {
    Direct(TimeEventMessageFactory),
    Registered(TimeEventCallbackToken),
    #[cfg(feature = "python")]
    SenderlessPython(Arc<crate::timer::PythonTimeEventCallback>),
    SenderlessRust,
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

        let owner_callback = if sender.is_some() {
            if callback.is_local() {
                OwnerCallback::Registered {
                    token: register_time_event_callback(callback.clone()),
                    callback,
                }
            } else {
                OwnerCallback::Direct(TimeEventMessageFactory::new(&callback))
            }
        } else {
            OwnerCallback::Senderless(callback)
        };

        Self {
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            fire_immediately,
            next_time_ns: Arc::new(AtomicU64::new(next_time_ns)),
            callback: owner_callback,
            task_handle: None,
            task_state: None,
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
                .is_some_and(JoinHandle::is_finished)
    }

    /// Starts the timer.
    ///
    /// Time events will begin triggering at the specified intervals.
    /// The generated events are handled by the provided callback function.
    ///
    /// Starting a timer whose task is still active aborts that task first
    /// (restart semantics); a previously fired event that is already queued
    /// still dispatches.
    ///
    /// # Panics
    ///
    /// Panics if using a Rust callback (`Rust` or `RustLocal`) without a `TimeEventSender`.
    #[allow(unused_variables)]
    pub fn start(&mut self) {
        let event_name = self.name;
        let stop_time_ns = self.stop_time_ns;
        let interval_ns = self.interval_ns.get();

        let mut observed_next = self
            .retire_task()
            .unwrap_or_else(|| self.next_time_ns.load(atomic::Ordering::SeqCst));

        // Close the old token before registering its replacement;
        // any lease acquired before retirement remains valid.
        if let Some(handle) = self.task_handle.take() {
            self.close_registered_callback();
            handle.abort();
        }

        let worker_dispatch = match &mut self.callback {
            OwnerCallback::Registered { token, callback } => {
                // A cancel or a final stop-time fire closed the token; a
                // restart needs a fresh registration or acquire() would
                // return None on every fire.
                if token.is_closed() {
                    *token = register_time_event_callback(callback.clone());
                }
                WorkerDispatch::Registered(token.clone())
            }
            OwnerCallback::Direct(factory) => WorkerDispatch::Direct(factory.clone()),
            OwnerCallback::Senderless(callback) => match callback {
                #[cfg(feature = "python")]
                TimeEventCallback::Python(callback) => {
                    WorkerDispatch::SenderlessPython(callback.clone())
                }
                TimeEventCallback::Rust(_) | TimeEventCallback::RustLocal(_) => {
                    WorkerDispatch::SenderlessRust
                }
            },
        };

        // Get current time
        let clock = get_atomic_clock_realtime();
        let now_ns = clock.get_time_ns();

        // Check if the timer's alert time is in the past and adjust if needed
        let now_raw = now_ns.as_u64();

        if should_adjust_past_due_time(observed_next, now_ns, stop_time_ns) {
            if observed_next < now_raw {
                let original = UnixNanos::from(observed_next);
                log::warn!(
                    "Timer '{event_name}' alert time {} was in the past, adjusted to current time for immediate fire",
                    original.to_rfc3339(),
                );
            }

            observed_next = now_raw;
        }

        // Floor the next time to the nearest microsecond which is within the timers accuracy
        let mut next_time_ns = normalize_start_time_ns(observed_next, now_ns, stop_time_ns);
        let next_time_atomic = Arc::new(AtomicU64::new(next_time_ns.as_u64()));
        let task_state = Arc::new(TimerTaskState::new(next_time_ns.as_u64()));
        self.next_time_ns = next_time_atomic.clone();
        self.task_state = Some(task_state.clone());

        let sender = self.sender.clone();

        let task = async move {
            let clock = get_atomic_clock_realtime();

            let start = Instant::now() + timer_start_delay(next_time_ns, now_ns);

            let mut timer = dst::time::interval_at(start, Duration::from_nanos(interval_ns));

            loop {
                // Never fire an event scheduled past the stop time. The event's
                // `ts_event` is the scheduled `next_time_ns`, so the bound is
                // enforced on the scheduled time (matching `TestTimer`), not on
                // the wall-clock read used only for `ts_init`.
                if !should_fire_scheduled_time(next_time_ns, stop_time_ns) {
                    if let (Some(sender), WorkerDispatch::Registered(token)) =
                        (sender.as_ref(), &worker_dispatch)
                        && let Some(lease) = token.acquire()
                    {
                        token.close();
                        let now_ns = clock.get_time_ns();
                        let event = TimeEvent::new(event_name, UUID4::new(), next_time_ns, now_ns);
                        sender.send(TimeEventMessage::cleanup(event, lease));
                    }
                    break; // Timer expired before this event
                }

                // `timer.tick` is cancellation safe, if the cancel branch completes
                // first then no tick has been consumed (no event was ready).
                timer.tick().await;
                let now_ns = clock.get_time_ns();

                let event = TimeEvent::new(event_name, UUID4::new(), next_time_ns, now_ns);

                // The event scheduled exactly at the stop time fires (inclusive
                // boundary), then the timer expires.
                let expires_after_fire = expires_after_scheduled_time(next_time_ns, stop_time_ns);
                let following_next_time_ns = next_time_ns + interval_ns;

                // Reserve this fire and its following schedule together. A
                // restart either observes the advanced schedule or retires
                // the task before it can dispatch. Registered callbacks
                // acquire their lease first so token closure cannot suppress
                // an event whose schedule already advanced.
                let registered_lease = if let WorkerDispatch::Registered(token) = &worker_dispatch {
                    match task_state.reserve_registered_fire(token, following_next_time_ns.as_u64())
                    {
                        Some(lease) => Some(lease),
                        None => break,
                    }
                } else {
                    if !task_state.reserve_fire(following_next_time_ns.as_u64()) {
                        break;
                    }
                    None
                };

                if sender.is_some() {
                    next_time_atomic
                        .store(following_next_time_ns.as_u64(), atomic::Ordering::SeqCst);
                }

                match (&sender, &worker_dispatch) {
                    (Some(sender), WorkerDispatch::Direct(factory)) => {
                        sender.send(factory.message(event));
                    }
                    (Some(sender), WorkerDispatch::Registered(token)) => {
                        let lease =
                            registered_lease.expect("registered callback lease was not acquired");

                        if expires_after_fire {
                            token.close();
                        }
                        sender.send(TimeEventMessage::registered(event, lease));
                    }
                    #[cfg(feature = "python")]
                    (None, WorkerDispatch::SenderlessPython(callback)) => callback.call(event),
                    (None, WorkerDispatch::SenderlessRust) => {
                        panic!("timer event sender was unset for Rust callback system");
                    }
                    _ => unreachable!("timer callback dispatch did not match its sender"),
                }

                if sender.is_none() {
                    next_time_atomic
                        .store(following_next_time_ns.as_u64(), atomic::Ordering::SeqCst);
                }

                next_time_ns = following_next_time_ns;

                if expires_after_fire {
                    break; // Timer expired at the stop boundary
                }
            }
        };

        #[cfg(all(feature = "simulation", madsim))]
        let handle = dst::task::spawn(task);
        #[cfg(not(all(feature = "simulation", madsim)))]
        let handle = get_runtime().spawn(task);

        self.task_handle = Some(handle);
        self.canceled = false;
    }

    /// Cancels the timer.
    ///
    /// The timer will not generate a final event.
    pub fn cancel(&mut self) {
        log::trace!("Cancel timer '{}'", self.name);

        self.close_registered_callback();

        self.retire_task();

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        self.canceled = true;
    }

    fn close_registered_callback(&self) {
        if let OwnerCallback::Registered { token, .. } = &self.callback {
            token.close();
        }
    }

    fn retire_task(&mut self) -> Option<u64> {
        let task_state = self.task_state.take()?;
        let next_time_ns = task_state.retire();
        self.next_time_ns
            .store(next_time_ns, atomic::Ordering::SeqCst);
        Some(next_time_ns)
    }
}

impl TimerTaskState {
    fn new(next_time_ns: u64) -> Self {
        Self {
            status: AtomicU8::new(TASK_ACTIVE),
            next_time_ns: AtomicU64::new(next_time_ns),
        }
    }

    fn reserve_registered_fire(
        &self,
        token: &TimeEventCallbackToken,
        following_next_time_ns: u64,
    ) -> Option<TimeEventCallbackLease> {
        let lease = token.acquire()?;
        self.reserve_fire(following_next_time_ns).then_some(lease)
    }

    fn reserve_fire(&self, following_next_time_ns: u64) -> bool {
        if self
            .status
            .compare_exchange(
                TASK_ACTIVE,
                TASK_FIRING,
                atomic::Ordering::SeqCst,
                atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return false;
        }

        self.next_time_ns
            .store(following_next_time_ns, atomic::Ordering::SeqCst);
        self.status.store(TASK_ACTIVE, atomic::Ordering::SeqCst);
        true
    }

    fn retire(&self) -> u64 {
        loop {
            match self.status.compare_exchange(
                TASK_ACTIVE,
                TASK_RETIRED,
                atomic::Ordering::SeqCst,
                atomic::Ordering::SeqCst,
            ) {
                Ok(_) | Err(TASK_RETIRED) => break,
                // The firing section contains only atomic schedule publication
                Err(TASK_FIRING) => std::hint::spin_loop(),
                Err(status) => unreachable!("invalid timer task state {status}"),
            }
        }

        self.next_time_ns.load(atomic::Ordering::SeqCst)
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
        self.close_registered_callback();
        self.retire_task();

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(all(feature = "simulation", madsim)))]
    use std::rc::Rc;
    #[cfg(all(feature = "python", not(all(feature = "simulation", madsim))))]
    use std::sync::atomic::AtomicU64;
    use std::{
        num::NonZeroU64,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    #[cfg(any(feature = "python", not(all(feature = "simulation", madsim))))]
    use std::{
        sync::{Mutex, mpsc},
        time::Duration as StdDuration,
    };

    use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
    #[cfg(feature = "python")]
    use pyo3::{
        Python,
        types::{PyAnyMethods, PyList, PyListMethods},
    };
    use rstest::*;
    use ustr::Ustr;

    use super::LiveTimer;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use crate::runner::register_time_event_callback;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use crate::testing::wait_until;
    use crate::{
        runner::{TimeEventMessage, TimeEventSender},
        timer::TimeEventCallback,
    };

    #[cfg(any(feature = "python", not(all(feature = "simulation", madsim))))]
    #[derive(Debug)]
    struct ChannelSender {
        tx: Mutex<mpsc::Sender<TimeEventMessage>>,
    }

    #[cfg(any(feature = "python", not(all(feature = "simulation", madsim))))]
    impl TimeEventSender for ChannelSender {
        fn send(&self, message: TimeEventMessage) {
            self.tx
                .lock()
                .expect("sender mutex should lock")
                .send(message)
                .expect("message should send");
        }
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[derive(Debug)]
    struct PausingChannelSender {
        tx: Mutex<mpsc::Sender<TimeEventMessage>>,
        release_rx: Mutex<mpsc::Receiver<()>>,
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    impl TimeEventSender for PausingChannelSender {
        fn send(&self, message: TimeEventMessage) {
            self.tx
                .lock()
                .expect("sender mutex should lock")
                .send(message)
                .expect("message should send");
            self.release_rx
                .lock()
                .expect("release mutex should lock")
                .recv()
                .expect("timer send should release");
        }
    }

    #[cfg(all(feature = "simulation", madsim))]
    #[derive(Debug)]
    struct CountingSender {
        count: Arc<AtomicUsize>,
    }

    #[cfg(all(feature = "simulation", madsim))]
    impl TimeEventSender for CountingSender {
        fn send(&self, _message: TimeEventMessage) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

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
    fn test_timer_task_retirement_prevents_a_late_fire() {
        let state = super::TimerTaskState::new(100);

        let restart_time_ns = state.retire();
        let reserved = state.reserve_fire(200);

        assert_eq!(restart_time_ns, 100);
        assert!(!reserved);
        assert_eq!(state.next_time_ns.load(Ordering::SeqCst), 100);
    }

    #[rstest]
    fn test_timer_task_retirement_preserves_a_reserved_fire() {
        let state = super::TimerTaskState::new(100);

        let reserved = state.reserve_fire(200);
        let restart_time_ns = state.retire();

        assert!(reserved);
        assert_eq!(restart_time_ns, 200);
        assert_eq!(state.next_time_ns.load(Ordering::SeqCst), 200);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_closed_registered_callback_does_not_reserve_fire() {
        let state = super::TimerTaskState::new(100);
        let callback = TimeEventCallback::RustLocal(Rc::new(|_| {}));
        let token = register_time_event_callback(callback);
        token.close();

        let lease = state.reserve_registered_fire(&token, 200);
        let restart_time_ns = state.retire();

        assert!(lease.is_none());
        assert_eq!(restart_time_ns, 100);
        assert_eq!(state.next_time_ns.load(Ordering::SeqCst), 100);
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

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_live_timer_uses_global_runtime() {
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(ChannelSender { tx: Mutex::new(tx) });
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("LIVE_TIMER"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            Some(now),
            TimeEventCallback::from(|_| {}),
            true,
            Some(sender),
        );

        timer.start();
        let message = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("timer message should arrive on the global runtime");
        wait_until(|| timer.is_expired(), StdDuration::from_secs(1));

        assert_eq!(message.event().ts_event, now);
        assert!(timer.is_expired());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_live_timer_dispatches_rust_local_callback_on_owner_thread() {
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(ChannelSender { tx: Mutex::new(tx) });
        let count = Rc::new(std::cell::Cell::new(0));
        let callback_count = count.clone();
        let callback: Rc<dyn Fn(crate::timer::TimeEvent)> =
            Rc::new(move |_| callback_count.set(callback_count.get() + 1));
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("LOCAL_TIMER"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            Some(now),
            TimeEventCallback::RustLocal(callback),
            true,
            Some(sender),
        );

        timer.start();
        let message = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("registered timer message should arrive");

        assert!(message.dispatch());
        assert_eq!(count.get(), 1);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_live_timer_cancel_preserves_queued_rust_local_callback_lease() {
        let (tx, rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let sender = Arc::new(PausingChannelSender {
            tx: Mutex::new(tx),
            release_rx: Mutex::new(release_rx),
        });
        let count = Rc::new(std::cell::Cell::new(0));
        let callback_count = count.clone();
        let callback: Rc<dyn Fn(crate::timer::TimeEvent)> =
            Rc::new(move |_| callback_count.set(callback_count.get() + 1));
        let callback_weak = Rc::downgrade(&callback);
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("CANCEL_QUEUED"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            None,
            TimeEventCallback::RustLocal(callback),
            true,
            Some(sender),
        );

        timer.start();
        let message = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("registered timer message should arrive");
        timer.cancel();
        release_tx.send(()).expect("timer send should release");

        assert!(callback_weak.upgrade().is_some());
        assert!(message.dispatch());
        assert_eq!(count.get(), 1);

        // The timer retains an owner-side clone for restart; only after it
        // drops must no registry copy remain.
        drop(timer);
        assert!(callback_weak.upgrade().is_none());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_live_timer_cancel_preserves_queued_direct_callback() {
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(ChannelSender { tx: Mutex::new(tx) });
        let count = Arc::new(AtomicUsize::new(0));
        let callback_count = count.clone();
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("CANCEL_QUEUED_DIRECT"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            None,
            TimeEventCallback::from(move |_| {
                callback_count.fetch_add(1, Ordering::Relaxed);
            }),
            true,
            Some(sender),
        );

        timer.start();
        let message = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("direct timer message should arrive");
        timer.cancel();

        assert!(message.dispatch());
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_live_timer_restart_after_cancel_re_registers_rust_local_callback() {
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(ChannelSender { tx: Mutex::new(tx) });
        let count = Rc::new(std::cell::Cell::new(0));
        let callback_count = count.clone();
        let callback: Rc<dyn Fn(crate::timer::TimeEvent)> =
            Rc::new(move |_| callback_count.set(callback_count.get() + 1));
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("RESTART_TIMER"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            None,
            TimeEventCallback::RustLocal(callback),
            true,
            Some(sender),
        );

        timer.start();
        let first = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("first registered timer message should arrive");
        timer.cancel();
        assert!(first.dispatch());

        timer.start();
        let second = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("restarted timer should re-register and fire");
        timer.cancel();

        assert!(second.dispatch());
        assert_eq!(count.get(), 2);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_live_timer_start_while_active_restarts_and_keeps_dispatching() {
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(ChannelSender { tx: Mutex::new(tx) });
        let count = Rc::new(std::cell::Cell::new(0));
        let callback_count = count.clone();
        let callback: Rc<dyn Fn(crate::timer::TimeEvent)> =
            Rc::new(move |_| callback_count.set(callback_count.get() + 1));
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("DOUBLE_START_TIMER"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            None,
            TimeEventCallback::RustLocal(callback),
            true,
            Some(sender),
        );

        timer.start();
        let first = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("first task message should arrive");

        // Second start with the first task still active: the old task is
        // aborted and the token stays live for the new one.
        timer.start();
        let second = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("restarted task should keep dispatching");
        timer.cancel();

        assert!(first.dispatch());
        assert!(second.dispatch());
        assert_eq!(count.get(), 2);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_live_timer_stop_before_first_fire_sends_cleanup_message() {
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(ChannelSender { tx: Mutex::new(tx) });
        let count = Rc::new(std::cell::Cell::new(0));
        let callback_count = count.clone();
        let callback: Rc<dyn Fn(crate::timer::TimeEvent)> =
            Rc::new(move |_| callback_count.set(callback_count.get() + 1));
        let callback_weak = Rc::downgrade(&callback);
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("CLEANUP_TIMER"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            Some(now),
            TimeEventCallback::RustLocal(callback),
            false,
            Some(sender),
        );

        timer.start();
        let cleanup = rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("cleanup message should arrive");

        assert!(callback_weak.upgrade().is_some());
        assert!(!cleanup.dispatch());
        assert_eq!(count.get(), 0);

        // The timer retains an owner-side clone for restart; only after it
        // drops must no registry copy remain.
        drop(timer);
        assert!(callback_weak.upgrade().is_none());
    }

    #[cfg(all(feature = "simulation", madsim))]
    #[madsim::test]
    async fn test_live_timer_uses_dst_runtime() {
        let count = Arc::new(AtomicUsize::new(0));
        let sender = Arc::new(CountingSender {
            count: count.clone(),
        });
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut timer = LiveTimer::new(
            Ustr::from("DST_TIMER"),
            NonZeroU64::new(1_000_000).unwrap(),
            now,
            Some(now),
            TimeEventCallback::from(|_| {}),
            true,
            Some(sender),
        );

        timer.start();
        crate::live::dst::time::sleep(crate::live::dst::time::Duration::from_millis(2)).await;
        crate::live::dst::task::yield_now().await;

        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(timer.is_expired());
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_live_timer_with_sender_defers_python_callback_to_handler() {
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
            let message = rx
                .recv_timeout(StdDuration::from_secs(1))
                .expect("timer message should arrive without acquiring the GIL on the worker");
            timer.cancel();

            assert_eq!(py_list.len(), 0);
            assert!(message.dispatch());
            assert_eq!(py_list.len(), 1);
        });
    }

    #[cfg(all(feature = "python", not(all(feature = "simulation", madsim))))]
    #[rstest]
    fn test_senderless_callback_observes_current_schedule() {
        Python::initialize();

        Python::attach(|py| {
            let schedule: Arc<Mutex<Option<Arc<AtomicU64>>>> = Arc::new(Mutex::new(None));
            let callback_schedule = schedule.clone();
            let (tx, rx) = mpsc::channel();
            let callback = pyo3::types::PyCFunction::new_closure(
                py,
                None,
                None,
                move |_args: &pyo3::Bound<'_, pyo3::types::PyTuple>,
                      _kwargs: Option<&pyo3::Bound<'_, pyo3::types::PyDict>>|
                      -> pyo3::PyResult<()> {
                    let next_time_ns = callback_schedule
                        .lock()
                        .expect("schedule mutex should lock")
                        .as_ref()
                        .expect("timer schedule should be available")
                        .load(Ordering::SeqCst);
                    tx.send(next_time_ns)
                        .expect("observed schedule should send");
                    Ok(())
                },
            )
            .expect("callback should create")
            .into_any()
            .unbind();
            let now = get_atomic_clock_realtime().get_time_ns();
            let interval_ns = 10_000_000;
            let start_time_ns = now + 50_000_000;
            let mut timer = LiveTimer::new(
                Ustr::from("SENDERLESS_SCHEDULE"),
                NonZeroU64::new(interval_ns).unwrap(),
                start_time_ns,
                None,
                TimeEventCallback::from(callback),
                true,
                None,
            );

            timer.start();
            let expected_time_ns = timer.next_time_ns().as_u64();
            schedule
                .lock()
                .expect("schedule mutex should lock")
                .replace(timer.next_time_ns.clone());
            let observed_time_ns = py
                .detach(move || rx.recv_timeout(StdDuration::from_secs(1)))
                .expect("senderless callback should observe the schedule");
            wait_until(
                || timer.next_time_ns().as_u64() == expected_time_ns + interval_ns,
                StdDuration::from_secs(1),
            );
            timer.cancel();

            assert_eq!(observed_time_ns, expected_time_ns);
        });
    }
}

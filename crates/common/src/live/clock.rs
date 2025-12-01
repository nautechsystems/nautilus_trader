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

//! Live clock implementation using Tokio for real-time operations.

use std::{
    collections::BinaryHeap,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use ahash::AHashMap;
use futures::Stream;
use nautilus_core::{
    AtomicTime, UnixNanos, correctness::check_predicate_true, time::get_atomic_clock_realtime,
};
use ustr::Ustr;

use super::timer::LiveTimer;
use crate::{
    clock::{CallbackRegistry, Clock, validate_and_prepare_time_alert, validate_and_prepare_timer},
    runner::{TimeEventSender, get_time_event_sender},
    timer::{
        ScheduledTimeEvent, TimeEvent, TimeEventCallback, TimeEventHandlerV2, create_valid_interval,
    },
};

/// A real-time clock which uses system time.
///
/// Timestamps are guaranteed to be unique and monotonically increasing.
///
/// # Threading
///
/// The clock holds thread-local runtime state and must remain on its originating thread.
#[derive(Debug)]
pub struct LiveClock {
    time: &'static AtomicTime,
    timers: AHashMap<Ustr, LiveTimer>,
    callbacks: CallbackRegistry,
    sender: Option<Arc<dyn TimeEventSender>>,
}

impl LiveClock {
    /// Creates a new [`LiveClock`] instance.
    #[must_use]
    pub fn new(sender: Option<Arc<dyn TimeEventSender>>) -> Self {
        Self {
            time: get_atomic_clock_realtime(),
            timers: AHashMap::new(),
            callbacks: CallbackRegistry::new(),
            sender,
        }
    }

    #[must_use]
    pub const fn get_timers(&self) -> &AHashMap<Ustr, LiveTimer> {
        &self.timers
    }

    fn clear_expired_timers(&mut self) {
        self.timers.retain(|_, timer| !timer.is_expired());
    }

    fn replace_existing_timer_if_needed(&mut self, name: &Ustr) {
        if self.timer_exists(name) {
            self.cancel_timer(name.as_str());
            log::warn!("Timer '{name}' replaced");
        }
    }
}

impl Default for LiveClock {
    /// Creates a new default [`LiveClock`] instance.
    fn default() -> Self {
        Self::new(Some(get_time_event_sender()))
    }
}

impl Deref for LiveClock {
    type Target = AtomicTime;

    fn deref(&self) -> &Self::Target {
        self.time
    }
}

impl Clock for LiveClock {
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
        self.timers.contains_key(name)
    }

    fn register_default_handler(&mut self, handler: TimeEventCallback) {
        self.callbacks.register_default_handler(handler);
    }

    /// # Panics
    ///
    /// This function panics if:
    /// - The event does not have an associated handler (see trait documentation).
    #[allow(unused_variables)]
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandlerV2 {
        self.callbacks.get_handler(event)
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

        self.replace_existing_timer_if_needed(&name);

        check_predicate_true(
            callback.is_some() | self.callbacks.has_any_callback(&name),
            "No callbacks provided",
        )?;

        let callback = if let Some(callback) = callback {
            self.callbacks.register_callback(name, callback.clone());
            callback
        } else {
            self.callbacks
                .get_callback(&name)
                .expect("Callback should exist")
        };

        // Safe to calculate interval now that we've ensured alert_time_ns >= ts_now
        let interval_ns = create_valid_interval((alert_time_ns - ts_now).into());

        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            ts_now,
            Some(alert_time_ns),
            callback,
            false,
            self.sender.clone(),
        );

        timer.start();

        self.clear_expired_timers();
        self.timers.insert(name, timer);

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

        let callback = if let Some(callback) = callback {
            self.callbacks.register_callback(name, callback.clone());
            callback
        } else {
            self.callbacks
                .get_callback(&name)
                .expect("Callback should exist")
        };

        let interval_ns = create_valid_interval(interval_ns);

        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            callback,
            fire_immediately,
            self.sender.clone(),
        );
        timer.start();

        self.clear_expired_timers();
        self.timers.insert(name, timer);

        Ok(())
    }

    fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        self.timers
            .get(&Ustr::from(name))
            .map(|timer| timer.next_time_ns())
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        if let Some(mut timer) = timer {
            timer.cancel();
        }
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }

        self.timers.clear();
    }

    fn reset(&mut self) {
        self.cancel_timers();
        self.callbacks.clear();
    }
}

// Helper struct to stream events from the heap
#[derive(Debug)]
pub struct TimeEventStream {
    heap: Arc<tokio::sync::Mutex<BinaryHeap<ScheduledTimeEvent>>>,
}

impl TimeEventStream {
    pub const fn new(heap: Arc<tokio::sync::Mutex<BinaryHeap<ScheduledTimeEvent>>>) -> Self {
        Self { heap }
    }
}

impl Stream for TimeEventStream {
    type Item = TimeEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut heap = match self.heap.try_lock() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::error!("Unable to get LiveClock heap lock: {e}");
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };

        if let Some(event) = heap.pop() {
            Poll::Ready(Some(event.into_inner()))
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use nautilus_core::{MUTEX_POISONED, UnixNanos, time::get_atomic_clock_realtime};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        clock::Clock,
        runner::TimeEventSender,
        testing::wait_until,
        timer::{TimeEvent, TimeEventCallback, TimeEventHandlerV2},
    };

    #[derive(Debug)]
    struct CollectingSender {
        events: Arc<Mutex<Vec<(TimeEvent, UnixNanos)>>>,
    }

    impl CollectingSender {
        fn new(events: Arc<Mutex<Vec<(TimeEvent, UnixNanos)>>>) -> Self {
            Self { events }
        }
    }

    impl TimeEventSender for CollectingSender {
        fn send(&self, handler: TimeEventHandlerV2) {
            let TimeEventHandlerV2 { event, callback } = handler;
            let now_ns = get_atomic_clock_realtime().get_time_ns();
            let event_clone = event.clone();
            callback.call(event);
            self.events
                .lock()
                .expect(MUTEX_POISONED)
                .push((event_clone, now_ns));
        }
    }

    fn wait_for_events(
        events: &Arc<Mutex<Vec<(TimeEvent, UnixNanos)>>>,
        target: usize,
        timeout: Duration,
    ) {
        wait_until(
            || events.lock().expect(MUTEX_POISONED).len() >= target,
            timeout,
        );
    }

    #[rstest]
    fn test_live_clock_timer_replacement_cancels_previous_task() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sender = Arc::new(CollectingSender::new(Arc::clone(&events)));

        let mut clock = LiveClock::new(Some(sender));
        clock.register_default_handler(TimeEventCallback::from(|_| {}));

        let fast_interval = Duration::from_millis(10).as_nanos() as u64;
        clock
            .set_timer_ns("replace", fast_interval, None, None, None, None, None)
            .unwrap();

        wait_for_events(&events, 2, Duration::from_millis(200));
        events.lock().expect(MUTEX_POISONED).clear();

        let slow_interval = Duration::from_millis(30).as_nanos() as u64;
        clock
            .set_timer_ns("replace", slow_interval, None, None, None, None, None)
            .unwrap();

        wait_for_events(&events, 3, Duration::from_millis(300));

        let snapshot = events.lock().expect(MUTEX_POISONED).clone();
        let diffs: Vec<u64> = snapshot
            .windows(2)
            .map(|pair| pair[1].0.ts_event.as_u64() - pair[0].0.ts_event.as_u64())
            .collect();

        assert!(!diffs.is_empty());
        for diff in diffs {
            assert_ne!(diff, fast_interval);
        }

        clock.cancel_timers();
    }

    #[rstest]
    fn test_live_clock_time_alert_persists_callback() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sender = Arc::new(CollectingSender::new(Arc::clone(&events)));

        let mut clock = LiveClock::new(Some(sender));
        clock.register_default_handler(TimeEventCallback::from(|_| {}));

        let now = clock.timestamp_ns();
        let alert_time = now + 1_000_u64;

        clock
            .set_time_alert_ns("alert-callback", alert_time, None, None)
            .unwrap();

        assert!(
            clock
                .callbacks
                .has_any_callback(&Ustr::from("alert-callback"))
        );

        clock.cancel_timers();
    }

    #[rstest]
    fn test_live_clock_reset_stops_active_timers() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sender = Arc::new(CollectingSender::new(Arc::clone(&events)));

        let mut clock = LiveClock::new(Some(sender));
        clock.register_default_handler(TimeEventCallback::from(|_| {}));

        clock
            .set_timer_ns(
                "reset-test",
                Duration::from_millis(15).as_nanos() as u64,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        wait_for_events(&events, 2, Duration::from_millis(250));

        clock.reset();

        // Wait for any in-flight events to arrive
        let start = std::time::Instant::now();
        wait_until(
            || start.elapsed() >= Duration::from_millis(50),
            Duration::from_secs(2),
        );

        // Clear any events that arrived before reset took effect
        events.lock().expect(MUTEX_POISONED).clear();

        // Verify no new events arrive (timer should be stopped)
        let start = std::time::Instant::now();
        wait_until(
            || start.elapsed() >= Duration::from_millis(50),
            Duration::from_secs(2),
        );
        assert!(events.lock().expect(MUTEX_POISONED).is_empty());
    }

    #[rstest]
    fn test_live_timer_short_delay_not_early() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sender = Arc::new(CollectingSender::new(Arc::clone(&events)));

        let mut clock = LiveClock::new(Some(sender));
        clock.register_default_handler(TimeEventCallback::from(|_| {}));

        let now = clock.timestamp_ns();
        let start_time = UnixNanos::from(*now + 500_000); // 0.5 ms in the future
        let interval_ns = 1_000_000;

        clock
            .set_timer_ns(
                "short-delay",
                interval_ns,
                Some(start_time),
                None,
                None,
                None,
                Some(true),
            )
            .unwrap();

        wait_for_events(&events, 1, Duration::from_millis(100));

        let snapshot = events.lock().expect(MUTEX_POISONED).clone();
        assert!(!snapshot.is_empty());

        for (event, actual_ts) in &snapshot {
            assert!(actual_ts.as_u64() >= event.ts_event.as_u64());
        }

        clock.cancel_timers();
    }
}

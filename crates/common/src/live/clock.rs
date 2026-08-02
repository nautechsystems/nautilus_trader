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

//! Live clock implementation using Tokio for real-time operations.

use std::{collections::BTreeMap, ops::Deref, sync::Arc};

use nautilus_core::{
    AtomicTime, UnixNanos, correctness::check_predicate_true, time::get_atomic_clock_realtime,
};
use ustr::Ustr;

use super::timer::LiveTimer;
use crate::{
    clock::{
        CallbackRegistry, Clock, replace_existing_timer, validate_and_prepare_time_alert,
        validate_and_prepare_timer,
    },
    runner::{TimeEventSender, purge_closed_time_event_callbacks, try_get_time_event_sender},
    timer::{TimeEvent, TimeEventCallback, TimeEventHandler, create_valid_interval},
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
    timers: BTreeMap<Ustr, LiveTimer>,
    callbacks: CallbackRegistry,
    sender: Option<Arc<dyn TimeEventSender>>,
}

impl LiveClock {
    /// Creates a new [`LiveClock`] instance.
    #[must_use]
    pub fn new(sender: Option<Arc<dyn TimeEventSender>>) -> Self {
        Self {
            time: get_atomic_clock_realtime(),
            timers: BTreeMap::new(),
            callbacks: CallbackRegistry::new(),
            sender,
        }
    }

    #[must_use]
    pub const fn get_timers(&self) -> &BTreeMap<Ustr, LiveTimer> {
        &self.timers
    }

    fn clear_expired_timers(&mut self) {
        self.timers.retain(|_, timer| !timer.is_expired());
        purge_closed_time_event_callbacks();
    }

    fn replace_existing_timer_if_needed(&mut self, name: &Ustr) {
        replace_existing_timer(&mut self.timers, name);
    }
}

impl Default for LiveClock {
    /// Creates a new default [`LiveClock`] instance.
    ///
    /// Uses `try_get_time_event_sender()` to allow creation before channels are initialized.
    fn default() -> Self {
        Self::new(try_get_time_event_sender())
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
        self.timers
            .get(name)
            .is_some_and(|timer| !timer.is_expired())
    }

    fn register_default_handler(&mut self, handler: TimeEventCallback) {
        self.callbacks.register_default_handler(handler);
    }

    fn cancel_default_handler(&mut self) {
        self.callbacks.cancel_default_handler();
    }

    fn cancel_callbacks(&mut self) {
        self.callbacks.clear();
    }

    /// # Panics
    ///
    /// This function panics if:
    /// - The event does not have an associated handler (see trait documentation).
    #[allow(unused_variables)]
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandler {
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

        // Safe to calculate interval now that we've ensured alert_time_ns >= ts_now
        let interval_ns = create_valid_interval((alert_time_ns - ts_now).into());
        let fire_immediately = alert_time_ns == ts_now;

        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            ts_now,
            Some(alert_time_ns),
            callback,
            fire_immediately,
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
            .filter(|timer| !timer.is_expired())
            .map(LiveTimer::next_time_ns)
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

#[cfg(test)]
#[cfg(not(all(feature = "simulation", madsim)))]
mod tests {
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };

    use nautilus_core::{MUTEX_POISONED, UnixNanos, time::get_atomic_clock_realtime};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        clock::Clock,
        runner::{TimeEventMessage, TimeEventSender},
        testing::wait_until,
        timer::{TimeEvent, TimeEventCallback},
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
        fn send(&self, message: TimeEventMessage) {
            let now_ns = get_atomic_clock_realtime().get_time_ns();
            let event = message.event().clone();
            message.dispatch();
            self.events
                .lock()
                .expect(MUTEX_POISONED)
                .push((event, now_ns));
        }
    }

    #[derive(Debug)]
    struct PausingCollectingSender {
        collector: CollectingSender,
        paused_tx: mpsc::Sender<()>,
        release_rx: Mutex<mpsc::Receiver<()>>,
        pause_once: AtomicBool,
    }

    impl TimeEventSender for PausingCollectingSender {
        fn send(&self, message: TimeEventMessage) {
            self.collector.send(message);

            if self.pause_once.swap(false, Ordering::SeqCst) {
                self.paused_tx.send(()).expect("timer send should pause");
                self.release_rx
                    .lock()
                    .expect("release mutex should lock")
                    .recv()
                    .expect("timer send should release");
            }
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
        let (paused_tx, paused_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let sender = Arc::new(PausingCollectingSender {
            collector: CollectingSender::new(Arc::clone(&events)),
            paused_tx,
            release_rx: Mutex::new(release_rx),
            pause_once: AtomicBool::new(true),
        });

        let mut clock = LiveClock::new(Some(sender));
        clock.register_default_handler(TimeEventCallback::from(|_| {}));

        let fast_interval = Duration::from_millis(10).as_nanos() as u64;
        clock
            .set_timer_ns("replace", fast_interval, None, None, None, None, None)
            .unwrap();

        paused_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("fast timer send should pause");
        events.lock().expect(MUTEX_POISONED).clear();

        let slow_interval = Duration::from_millis(30).as_nanos() as u64;
        clock
            .set_timer_ns("replace", slow_interval, None, None, None, None, None)
            .unwrap();
        release_tx.send(()).expect("fast timer send should release");

        wait_for_events(&events, 3, Duration::from_secs(2));

        let snapshot = events.lock().expect(MUTEX_POISONED).clone();
        let diffs: Vec<u64> = snapshot
            .array_windows()
            .map(|[a, b]| b.0.ts_event.as_u64() - a.0.ts_event.as_u64())
            .collect();

        assert!(!diffs.is_empty());
        for diff in diffs {
            assert_eq!(diff, slow_interval);
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
        let alert_time = now + Duration::from_mins(1).as_nanos() as u64;

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
    fn test_live_clock_timer_exists_consistent_after_expiry() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sender = Arc::new(CollectingSender::new(Arc::clone(&events)));

        let mut clock = LiveClock::new(Some(sender));
        clock.register_default_handler(TimeEventCallback::from(|_| {}));

        let name = Ustr::from("expiring");
        let now = clock.timestamp_ns();
        let interval_ns = Duration::from_millis(5).as_nanos() as u64;
        let stop_time = UnixNanos::from(*now + Duration::from_millis(12).as_nanos() as u64);

        clock
            .set_timer_ns(
                name.as_str(),
                interval_ns,
                None,
                Some(stop_time),
                None,
                None,
                None,
            )
            .unwrap();

        assert!(clock.timer_exists(&name));

        // Wait for the timer task to run past its stop time and finish
        wait_until(|| clock.timer_count() == 0, Duration::from_secs(2));

        // An expired timer is purged only lazily on the next set/cancel call,
        // so the entry still sits in the map; the introspection surfaces
        // must nevertheless agree it is gone
        assert!(clock.timers.contains_key(&name));
        assert!(!clock.timer_exists(&name));
        assert_eq!(clock.timer_count(), 0);
        assert!(clock.timer_names().is_empty());
        assert!(clock.next_time_ns(name.as_str()).is_none());
    }

    #[rstest]
    fn test_live_clock_failed_set_time_alert_ns_preserves_existing_timer() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sender = Arc::new(CollectingSender::new(Arc::clone(&events)));

        // No default handler registered
        let mut clock = LiveClock::new(Some(sender));

        let now = clock.timestamp_ns();
        let alert_time = now + Duration::from_mins(1).as_nanos() as u64;

        clock
            .set_time_alert_ns(
                "alert",
                alert_time,
                Some(TimeEventCallback::from(|_| {})),
                None,
            )
            .unwrap();
        assert_eq!(clock.next_time_ns("alert"), Some(alert_time));

        // Callbacks released (e.g. partial component teardown) while the alert still lives
        clock.cancel_callbacks();

        // Rescheduling without a callback fails the predicate check; the error
        // return must not have destroyed the previously scheduled alert
        let err = clock
            .set_time_alert_ns("alert", alert_time + 1000u64, None, None)
            .unwrap_err();
        assert!(
            err.to_string().contains("No callbacks provided"),
            "unexpected error: {err}"
        );
        assert!(clock.timer_exists(&Ustr::from("alert")));
        assert_eq!(clock.next_time_ns("alert"), Some(alert_time));

        clock.cancel_timers();
    }
}

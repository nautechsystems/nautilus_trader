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

//! Message throttling and rate limiting functionality.
//!
//! This module provides throttling capabilities to control the rate of message processing
//! and prevent system overload. The throttler can buffer, drop, or delay messages based
//! on configured rate limits and time intervals.

use std::{
    any::Any,
    cell::{RefCell, UnsafeCell},
    collections::VecDeque,
    fmt::Debug,
    marker::PhantomData,
    num::{NonZeroU64, NonZeroUsize},
    rc::Rc,
};

use nautilus_core::{UnixNanos, correctness::FAILED};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    actor::{
        Actor,
        registry::{register_actor, try_get_actor_unchecked, with_actor_registry},
    },
    clock::Clock,
    msgbus::{self, Endpoint, Handler, MStr, ShareableMessageHandler},
    timer::{TimeEvent, TimeEventCallback},
};

/// Represents a throttling limit per interval.
///
/// The non-zero field types make a degenerate rate limit unrepresentable: a zero `limit`
/// underflows the throttler's `limit - 1` indexing, and a zero `interval_ns` disables
/// throttling entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimit {
    limit: NonZeroUsize,
    interval_ns: NonZeroU64,
}

impl RateLimit {
    /// Creates a new [`RateLimit`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `limit` or `interval_ns` is zero.
    pub fn new_checked(limit: usize, interval_ns: u64) -> anyhow::Result<Self> {
        let limit = NonZeroUsize::new(limit)
            .ok_or_else(|| anyhow::anyhow!("Invalid limit: {limit} (must be non-zero)"))?;
        let interval_ns = NonZeroU64::new(interval_ns).ok_or_else(|| {
            anyhow::anyhow!("Invalid interval_ns: {interval_ns} (must be non-zero)")
        })?;
        Ok(Self { limit, interval_ns })
    }

    /// Creates a new [`RateLimit`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `limit` or `interval_ns` is zero.
    #[must_use]
    pub fn new(limit: usize, interval_ns: u64) -> Self {
        Self::new_checked(limit, interval_ns).expect(FAILED)
    }

    /// Maximum number of messages that can be sent within the interval.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit.get()
    }

    /// Interval between messages in nanoseconds.
    #[must_use]
    pub const fn interval_ns(&self) -> u64 {
        self.interval_ns.get()
    }
}

/// Throttler rate limits messages by dropping or buffering them.
///
/// Throttler takes messages of type T and callback of type F for dropping
/// or processing messages.
///
/// The throttler stores its limit and interval as non-zero values from
/// [`RateLimit`]. Internal counters, buffers, and timer state stay private so
/// callers can observe state without breaking rate-limit invariants.
///
/// # Callback contract
///
/// The `output_send` and `output_drop` callbacks are invoked inline from
/// [`Throttler::send`] and the drain handler. They must not reenter the
/// throttler (for example by calling `send` synchronously), since the
/// throttler mutates its buffer and window state through `UnsafeCell` without
/// borrow-check protection. Route side effects through an asynchronous queue
/// when in doubt.
pub struct Throttler<T, F> {
    clock: Rc<RefCell<dyn Clock>>,
    actor_id: Ustr,
    timer_name: Ustr,
    limit: NonZeroUsize,
    interval_ns: NonZeroU64,
    buffer: VecDeque<T>,
    timestamps: VecDeque<UnixNanos>,
    is_limiting: bool,
    recv_count: usize,
    sent_count: usize,
    output_send: F,
    output_drop: Option<F>,
}

impl<T, F> Actor for Throttler<T, F>
where
    T: 'static + Debug,
    F: Fn(T) + 'static,
{
    fn id(&self) -> Ustr {
        self.actor_id
    }

    fn handle(&mut self, _msg: &dyn Any) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T, F> Debug for Throttler<T, F>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Throttler))
            .field("actor_id", &self.actor_id)
            .field("timer_name", &self.timer_name)
            .field("limit", &self.limit())
            .field("interval_ns", &self.interval_ns())
            .field("buffer", &self.buffer)
            .field("timestamps", &self.timestamps)
            .field("is_limiting", &self.is_limiting)
            .field("recv_count", &self.recv_count)
            .field("sent_count", &self.sent_count)
            .finish()
    }
}

impl<T, F> Throttler<T, F>
where
    T: Debug,
{
    /// Creates a new [`Throttler`] instance.
    ///
    /// The timer is registered under a name namespaced by `actor_id` so multiple
    /// throttlers can share one clock.
    #[inline]
    pub fn new(
        rate_limit: RateLimit,
        clock: Rc<RefCell<dyn Clock>>,
        timer_name: &str,
        output_send: F,
        output_drop: Option<F>,
        actor_id: Ustr,
    ) -> Self {
        Self {
            clock,
            actor_id,
            timer_name: Ustr::from(format!("{timer_name}-{actor_id}").as_str()),
            limit: rate_limit.limit,
            interval_ns: rate_limit.interval_ns,
            buffer: VecDeque::new(),
            timestamps: VecDeque::with_capacity(rate_limit.limit.get().min(1024)),
            is_limiting: false,
            recv_count: 0,
            sent_count: 0,
            output_send,
            output_drop,
        }
    }

    /// Set timer with a callback to be triggered on next interval.
    ///
    /// Typically used to register callbacks:
    /// - to process buffered messages
    /// - to stop buffering
    ///
    /// `allow_past` is set explicitly so a zero `delta_next` clamps to the
    /// current time and fires immediately instead of returning an error.
    ///
    /// # Panics
    ///
    /// Panics if setting the time alert on the internal clock fails.
    #[inline]
    pub(crate) fn set_timer(&self, callback: Option<TimeEventCallback>) {
        let delta = self.delta_next();
        let mut clock = self.clock.borrow_mut();
        if clock.timer_exists(&self.timer_name) {
            clock.cancel_timer(&self.timer_name);
        }
        let alert_ts = clock.timestamp_ns() + delta;

        clock
            .set_time_alert_ns(self.timer_name.as_str(), alert_ts, callback, Some(true))
            .expect(FAILED);
    }

    /// Time delta when the next message can be sent.
    ///
    /// Uses saturating subtraction so a clock regression or a future-dated
    /// timestamp yields a zero delta instead of panicking.
    #[inline]
    pub fn delta_next(&self) -> u64 {
        match self.timestamps.get(self.limit.get() - 1) {
            Some(ts) => {
                let diff = self
                    .clock
                    .borrow()
                    .timestamp_ns()
                    .as_u64()
                    .saturating_sub(ts.as_u64());
                self.interval_ns.get().saturating_sub(diff)
            }
            None => 0,
        }
    }

    /// Reset the throttler which clears internal state and cancels any pending
    /// timer so no drain or resume callback fires after reset.
    #[inline]
    pub fn reset(&mut self) {
        self.cancel_timer_internal();
        self.buffer.clear();
        self.recv_count = 0;
        self.sent_count = 0;
        self.is_limiting = false;
        self.timestamps.clear();
    }
}

impl<T, F> Throttler<T, F> {
    /// Cancels the throttler's timer if one is pending. Silently does nothing
    /// when the clock is borrowed elsewhere or no timer exists (best-effort,
    /// e.g. from `Drop`).
    ///
    /// Lives in a boundless impl block so `Drop` (which has no `T: Debug` bound)
    /// can call it.
    fn cancel_timer_internal(&self) {
        if let Ok(mut clock) = self.clock.try_borrow_mut() {
            clock.cancel_timer(&self.timer_name);
        }
    }

    /// Counts sent messages whose timestamps fall inside the current interval
    /// window. Shared by [`Throttler::used`] and [`Throttler::try_reserve`].
    fn count_in_window(&self) -> usize {
        let interval_start =
            self.clock.borrow().timestamp_ns().as_i64() - self.interval_ns.get() as i64;
        self.timestamps
            .iter()
            .take_while(|&&ts| ts.as_i64() > interval_start)
            .count()
    }

    /// Maximum number of messages that can be sent within the interval.
    #[inline]
    pub const fn limit(&self) -> usize {
        self.limit.get()
    }

    /// Interval between messages in nanoseconds.
    #[inline]
    pub const fn interval_ns(&self) -> u64 {
        self.interval_ns.get()
    }

    /// Rate limit configured for this throttler.
    #[inline]
    pub const fn rate_limit(&self) -> RateLimit {
        RateLimit {
            limit: self.limit,
            interval_ns: self.interval_ns,
        }
    }

    /// Number of messages queued in buffer.
    #[inline]
    pub fn qsize(&self) -> usize {
        self.buffer.len()
    }

    /// Fractional value of rate limit consumed in current interval.
    #[inline]
    pub fn used(&self) -> f64 {
        if self.timestamps.is_empty() {
            return 0.0;
        }
        let messages_in_current_interval = self.count_in_window();
        (messages_in_current_interval as f64) / (self.limit.get() as f64)
    }

    /// Whether the throttler is currently limiting the message rate.
    #[inline]
    pub const fn is_limiting(&self) -> bool {
        self.is_limiting
    }

    /// Number of messages received.
    #[inline]
    pub const fn recv_count(&self) -> usize {
        self.recv_count
    }

    /// Number of messages sent.
    #[inline]
    pub const fn sent_count(&self) -> usize {
        self.sent_count
    }
}

impl<T, F> Throttler<T, F>
where
    T: 'static + Debug,
    F: Fn(T) + 'static,
{
    pub fn to_actor(self) -> Rc<UnsafeCell<Self>> {
        // Register process endpoint
        let process_handler = ThrottlerProcess::<T, F>::new(self.actor_id);
        msgbus::register_any(
            process_handler.id().as_str().into(),
            ShareableMessageHandler::from(Rc::new(process_handler) as Rc<dyn Handler<dyn Any>>),
        );

        // Register actor state and return the wrapped reference
        register_actor(self)
    }

    /// Disposes of the throttler by cancelling its timer, deregistering its
    /// process endpoint from the message bus, and removing it from the actor
    /// registry.
    ///
    /// Call this before dropping a throttler registered via [`Throttler::to_actor`]
    /// to avoid leaking the process endpoint. For embedded throttlers (not
    /// registered) this is still safe: the endpoint and registry removals are
    /// no-ops.
    pub fn dispose(&mut self) {
        self.cancel_timer_internal();
        msgbus::deregister_any(process_endpoint(self.actor_id));
        with_actor_registry(|registry| {
            registry.remove(&self.actor_id);
        });
    }

    #[inline]
    pub(crate) fn send_msg(&mut self, msg: T) {
        let now = self.clock.borrow().timestamp_ns();

        if self.timestamps.len() >= self.limit.get() {
            self.timestamps.pop_back();
        }
        self.timestamps.push_front(now);

        self.sent_count += 1;
        (self.output_send)(msg);
    }

    /// Reserves capacity for `count` messages without sending callbacks.
    ///
    /// Returns `false` when the current window cannot accept all messages. No partial
    /// reservation is made in that case. The resume timer is armed only when the
    /// window is genuinely full (`delta_next > 0`); when the window already slid
    /// (`delta_next == 0`) the next call re-evaluates without arming a zero-delta
    /// timer that would fire immediately and log spam.
    #[inline]
    pub fn try_reserve(&mut self, count: usize) -> bool {
        self.recv_count += count;

        if count == 0 {
            return true;
        }

        let delta = self.delta_next();
        if self.is_limiting && delta == 0 && self.buffer.is_empty() {
            self.is_limiting = false;
        }

        if self.is_limiting {
            return false;
        }

        let used = self.count_in_window();

        if self.limit.get().saturating_sub(used) < count {
            self.is_limiting = true;

            if delta > 0 {
                self.set_timer(Some(throttler_resume::<T, F>(self.actor_id)));
            }
            return false;
        }

        let now = self.clock.borrow().timestamp_ns();

        for _ in 0..count {
            if self.timestamps.len() >= self.limit.get() {
                self.timestamps.pop_back();
            }
            self.timestamps.push_front(now);
        }
        self.sent_count += count;
        true
    }

    #[inline]
    pub(crate) fn limit_msg(&mut self, msg: T) {
        if self.output_drop.is_none() {
            self.buffer.push_front(msg);
            log::debug!("Buffering {}", self.buffer.len());

            if !self.is_limiting {
                log::debug!("Limiting");
                let cb = Some(ThrottlerProcess::<T, F>::new(self.actor_id).get_timer_callback());
                self.set_timer(cb);
                self.is_limiting = true;
            }
        } else {
            log::debug!("Dropping");

            if let Some(drop) = &self.output_drop {
                drop(msg);
            }

            if !self.is_limiting {
                log::debug!("Limiting");
                self.set_timer(Some(throttler_resume::<T, F>(self.actor_id)));
                self.is_limiting = true;
            }
        }
    }

    #[inline]
    pub fn send(&mut self, msg: T)
    where
        T: 'static,
        F: Fn(T) + 'static,
    {
        self.recv_count += 1;

        let delta = self.delta_next();

        // Auto-reset when the rate window has passed but no timer callback
        // arrived (e.g. for embedded throttlers not registered as actors).
        // Gated on an empty buffer so buffered throttlers keep draining via
        // ThrottlerProcess; only drop-mode throttlers have an empty buffer here.
        if self.is_limiting && delta == 0 && self.buffer.is_empty() {
            self.is_limiting = false;
        }

        if self.is_limiting || delta > 0 {
            self.limit_msg(msg);
        } else {
            self.send_msg(msg);
        }
    }
}

/// Builds the message-bus endpoint used to drive the buffered drain handler.
/// Centralized so registration, `dispose`, and `Drop` agree on the name.
fn process_endpoint(actor_id: Ustr) -> MStr<Endpoint> {
    MStr::endpoint(format!("{actor_id}_process")).expect(FAILED)
}

/// Process buffered messages for throttler
///
/// When limit is reached, schedules a timer event to call self again. The handler
/// is registered as a separated endpoint on the message bus as `{actor_id}_process`.
struct ThrottlerProcess<T, F> {
    actor_id: Ustr,
    endpoint: MStr<Endpoint>,
    phantom_t: PhantomData<T>,
    phantom_f: PhantomData<F>,
}

impl<T, F> ThrottlerProcess<T, F>
where
    T: Debug,
{
    pub(crate) fn new(actor_id: Ustr) -> Self {
        Self {
            actor_id,
            endpoint: process_endpoint(actor_id),
            phantom_t: PhantomData,
            phantom_f: PhantomData,
        }
    }

    pub(crate) fn get_timer_callback(&self) -> TimeEventCallback {
        let endpoint = self.endpoint;
        TimeEventCallback::from(move |event: TimeEvent| {
            msgbus::send_any(endpoint, &(event));
        })
    }
}

impl<T, F> Handler<dyn Any> for ThrottlerProcess<T, F>
where
    T: 'static + Debug,
    F: Fn(T) + 'static,
{
    fn id(&self) -> Ustr {
        *self.endpoint
    }

    fn handle(&self, _message: &dyn Any) {
        // Use the fallible lookup so a late timer fire after teardown is a
        // no-op rather than a panic.
        let Some(mut throttler) = try_get_actor_unchecked::<Throttler<T, F>>(&self.actor_id) else {
            return;
        };

        while let Some(msg) = throttler.buffer.pop_back() {
            throttler.send_msg(msg);

            // Set timer to process more buffered messages
            // if interval limit reached and there are more
            // buffered messages to process
            if !throttler.buffer.is_empty() && throttler.delta_next() > 0 {
                throttler.is_limiting = true;
                throttler.set_timer(Some(self.get_timer_callback()));
                return;
            }
        }

        throttler.is_limiting = false;
    }
}

impl<T, F> Drop for Throttler<T, F> {
    fn drop(&mut self) {
        // Cancel any pending timer so drain/resume callbacks do not fire after
        // teardown. Best-effort: skip silently if the shared clock is currently
        // borrowed (e.g. during a nested drop).
        self.cancel_timer_internal();
    }
}

/// Sets throttler to resume sending messages.
///
/// Uses `try_get_actor_unchecked` so that embedded throttlers (not registered
/// in the actor registry) are handled gracefully. The `send()` auto-reset
/// ensures such throttlers recover once the rate window passes.
pub fn throttler_resume<T, F>(actor_id: Ustr) -> TimeEventCallback
where
    T: 'static + Debug,
    F: Fn(T) + 'static,
{
    TimeEventCallback::from(move |_event: TimeEvent| {
        if let Some(mut throttler) = try_get_actor_unchecked::<Throttler<T, F>>(&actor_id) {
            throttler.is_limiting = false;
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{RefCell, UnsafeCell},
        rc::Rc,
    };

    use nautilus_core::UUID4;
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::{RateLimit, Throttler, ThrottlerProcess};
    use crate::{
        clock::{Clock, TestClock},
        msgbus::{self, Handler},
    };
    type SharedThrottler = Rc<UnsafeCell<Throttler<u64, Box<dyn Fn(u64)>>>>;

    /// Test throttler with default values for testing
    ///
    /// - Rate limit is 5 messages in 10 intervals.
    /// - Message handling is decided by specific fixture
    #[derive(Clone)]
    struct TestThrottler {
        throttler: SharedThrottler,
        clock: Rc<RefCell<TestClock>>,
        interval: u64,
    }

    #[allow(unsafe_code)]
    impl TestThrottler {
        #[expect(clippy::mut_from_ref)]
        pub(crate) fn get_throttler(&self) -> &mut Throttler<u64, Box<dyn Fn(u64)>> {
            unsafe { &mut *self.throttler.get() }
        }
    }

    // Timer names are namespaced as `{base}-{actor_id}` with a random actor_id,
    // so tests match on the base prefix and the expected count instead of an
    // exact name.
    fn timer_count_with_prefix(
        throttler: &Throttler<u64, Box<dyn Fn(u64)>>,
        prefix: &str,
    ) -> usize {
        throttler
            .clock
            .borrow()
            .timer_names()
            .iter()
            .filter(|name| name.starts_with(prefix))
            .count()
    }

    #[allow(unsafe_code)]
    #[expect(clippy::mut_from_ref)]
    fn access_shared(t: &SharedThrottler) -> &mut Throttler<u64, Box<dyn Fn(u64)>> {
        unsafe { &mut *t.get() }
    }

    #[rstest]
    #[case(0, 1_000)]
    #[case(1_000, 0)]
    fn test_rate_limit_new_checked_rejects_zero(#[case] limit: usize, #[case] interval_ns: u64) {
        assert!(RateLimit::new_checked(limit, interval_ns).is_err());
    }

    #[rstest]
    #[case(0, 1_000)]
    #[case(1_000, 0)]
    #[should_panic]
    fn test_rate_limit_new_panics_on_zero(#[case] limit: usize, #[case] interval_ns: u64) {
        let _ = RateLimit::new(limit, interval_ns);
    }

    #[rstest]
    fn test_rate_limit_new_checked_accepts_positive() {
        let rate = RateLimit::new_checked(5, 10).unwrap();

        assert_eq!(rate.limit(), 5);
        assert_eq!(rate.interval_ns(), 10);
    }

    #[fixture]
    pub fn test_throttler_buffered() -> TestThrottler {
        let output_send: Box<dyn Fn(u64)> = Box::new(|msg: u64| {
            log::debug!("Sent: {msg}");
        });
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let inner_clock = Rc::clone(&clock);
        let rate_limit = RateLimit::new(5, 10);
        let interval = rate_limit.interval_ns();
        let actor_id = Ustr::from(UUID4::new().as_str());

        TestThrottler {
            throttler: Throttler::new(
                rate_limit,
                clock,
                "buffer_timer",
                output_send,
                None,
                actor_id,
            )
            .to_actor(),
            clock: inner_clock,
            interval,
        }
    }

    #[fixture]
    pub fn test_throttler_unbuffered() -> TestThrottler {
        let output_send: Box<dyn Fn(u64)> = Box::new(|msg: u64| {
            log::debug!("Sent: {msg}");
        });
        let output_drop: Box<dyn Fn(u64)> = Box::new(|msg: u64| {
            log::debug!("Dropped: {msg}");
        });
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let inner_clock = Rc::clone(&clock);
        let rate_limit = RateLimit::new(5, 10);
        let interval = rate_limit.interval_ns();
        let actor_id = Ustr::from(UUID4::new().as_str());

        TestThrottler {
            throttler: Throttler::new(
                rate_limit,
                clock,
                "dropper_timer",
                output_send,
                Some(output_drop),
                actor_id,
            )
            .to_actor(),
            clock: inner_clock,
            interval,
        }
    }

    #[rstest]
    fn test_buffering_send_to_limit_becomes_throttled(test_throttler_buffered: TestThrottler) {
        let throttler = test_throttler_buffered.get_throttler();
        for _ in 0..6 {
            throttler.send(42);
        }
        assert_eq!(throttler.qsize(), 1);

        assert!(throttler.is_limiting);
        assert_eq!(throttler.recv_count, 6);
        assert_eq!(throttler.sent_count, 5);
        assert_eq!(timer_count_with_prefix(throttler, "buffer_timer"), 1);
    }

    #[rstest]
    fn test_buffering_used_when_sent_to_limit_returns_one(test_throttler_buffered: TestThrottler) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..5 {
            throttler.send(42);
        }

        assert_eq!(throttler.used(), 1.0);
        assert_eq!(throttler.recv_count, 5);
        assert_eq!(throttler.sent_count, 5);
    }

    #[rstest]
    fn test_buffering_used_when_half_interval_from_limit_returns_one(
        test_throttler_buffered: TestThrottler,
    ) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..5 {
            throttler.send(42);
        }

        let half_interval = test_throttler_buffered.interval / 2;
        // Advance the clock by half the interval
        {
            let mut clock = test_throttler_buffered.clock.borrow_mut();
            clock.advance_time(half_interval.into(), true);
        }

        assert_eq!(throttler.used(), 1.0);
        assert_eq!(throttler.recv_count, 5);
        assert_eq!(throttler.sent_count, 5);
    }

    #[rstest]
    fn test_buffering_used_before_limit_when_halfway_returns_half(
        test_throttler_buffered: TestThrottler,
    ) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..3 {
            throttler.send(42);
        }

        assert_eq!(throttler.used(), 0.6);
        assert_eq!(throttler.recv_count, 3);
        assert_eq!(throttler.sent_count, 3);
    }

    #[rstest]
    fn test_try_reserve_counts_messages_without_output(test_throttler_buffered: TestThrottler) {
        let throttler = test_throttler_buffered.get_throttler();

        assert!(throttler.try_reserve(3));

        assert_eq!(throttler.used(), 0.6);
        assert_eq!(throttler.recv_count, 3);
        assert_eq!(throttler.sent_count, 3);
        assert_eq!(throttler.qsize(), 0);
    }

    #[rstest]
    fn test_try_reserve_rejects_when_full_batch_exceeds_limit(
        test_throttler_buffered: TestThrottler,
    ) {
        let throttler = test_throttler_buffered.get_throttler();

        assert!(throttler.try_reserve(3));
        assert!(!throttler.try_reserve(3));

        assert_eq!(throttler.used(), 0.6);
        assert_eq!(throttler.recv_count, 6);
        assert_eq!(throttler.sent_count, 3);
        assert_eq!(throttler.qsize(), 0);
        assert!(throttler.is_limiting);
        // delta_next == 0 here (only 3 of 5 slots used), so the resume timer is
        // not armed to avoid an immediate-fire zero-delta timer. The next call
        // re-evaluates via the auto-reset branch.
        assert_eq!(timer_count_with_prefix(throttler, "buffer_timer"), 0);
    }

    #[rstest]
    fn test_buffering_refresh_when_at_limit_sends_remaining_items(
        test_throttler_buffered: TestThrottler,
    ) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..6 {
            throttler.send(42);
        }

        // Advance time and process events
        {
            let mut clock = test_throttler_buffered.clock.borrow_mut();
            let time_events = clock.advance_time(test_throttler_buffered.interval.into(), true);
            for each_event in clock.match_handlers(time_events) {
                drop(clock); // Release the mutable borrow

                each_event.callback.call(each_event.event);

                // Re-borrow the clock for the next iteration
                clock = test_throttler_buffered.clock.borrow_mut();
            }
        }

        // Assert final state
        assert_eq!(throttler.used(), 0.2);
        assert_eq!(throttler.recv_count, 6);
        assert_eq!(throttler.sent_count, 6);
        assert_eq!(throttler.qsize(), 0);
    }

    #[rstest]
    fn test_buffering_send_message_after_buffering_message(test_throttler_buffered: TestThrottler) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..6 {
            throttler.send(43);
        }

        // Advance time and process events
        {
            let mut clock = test_throttler_buffered.clock.borrow_mut();
            let time_events = clock.advance_time(test_throttler_buffered.interval.into(), true);
            for each_event in clock.match_handlers(time_events) {
                drop(clock); // Release the mutable borrow

                each_event.callback.call(each_event.event);

                // Re-borrow the clock for the next iteration
                clock = test_throttler_buffered.clock.borrow_mut();
            }
        }

        for _ in 0..6 {
            throttler.send(42);
        }

        // Assert final state
        assert_eq!(throttler.used(), 1.0);
        assert_eq!(throttler.recv_count, 12);
        assert_eq!(throttler.sent_count, 10);
        assert_eq!(throttler.qsize(), 2);
    }

    #[rstest]
    fn test_buffering_send_message_after_halfway_after_buffering_message(
        test_throttler_buffered: TestThrottler,
    ) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..6 {
            throttler.send(42);
        }

        // Advance time and process events
        {
            let mut clock = test_throttler_buffered.clock.borrow_mut();
            let time_events = clock.advance_time(test_throttler_buffered.interval.into(), true);
            for each_event in clock.match_handlers(time_events) {
                drop(clock); // Release the mutable borrow

                each_event.callback.call(each_event.event);

                // Re-borrow the clock for the next iteration
                clock = test_throttler_buffered.clock.borrow_mut();
            }
        }

        for _ in 0..3 {
            throttler.send(42);
        }

        // Assert final state
        assert_eq!(throttler.used(), 0.8);
        assert_eq!(throttler.recv_count, 9);
        assert_eq!(throttler.sent_count, 9);
        assert_eq!(throttler.qsize(), 0);
    }

    #[rstest]
    fn test_dropping_send_sends_message_to_handler(test_throttler_unbuffered: TestThrottler) {
        let throttler = test_throttler_unbuffered.get_throttler();
        throttler.send(42);

        assert!(!throttler.is_limiting);
        assert_eq!(throttler.recv_count, 1);
        assert_eq!(throttler.sent_count, 1);
    }

    #[rstest]
    fn test_dropping_send_to_limit_drops_message(test_throttler_unbuffered: TestThrottler) {
        let throttler = test_throttler_unbuffered.get_throttler();
        for _ in 0..6 {
            throttler.send(42);
        }
        assert_eq!(throttler.qsize(), 0);

        assert!(throttler.is_limiting);
        assert_eq!(throttler.used(), 1.0);
        assert_eq!(throttler.clock.borrow().timer_count(), 1);
        assert_eq!(timer_count_with_prefix(throttler, "dropper_timer"), 1);
        assert_eq!(throttler.recv_count, 6);
        assert_eq!(throttler.sent_count, 5);
    }

    #[rstest]
    fn test_dropping_advance_time_when_at_limit_dropped_message(
        test_throttler_unbuffered: TestThrottler,
    ) {
        let throttler = test_throttler_unbuffered.get_throttler();
        for _ in 0..6 {
            throttler.send(42);
        }

        // Advance time and process events
        {
            let mut clock = test_throttler_unbuffered.clock.borrow_mut();
            let time_events = clock.advance_time(test_throttler_unbuffered.interval.into(), true);
            for each_event in clock.match_handlers(time_events) {
                drop(clock); // Release the mutable borrow

                each_event.callback.call(each_event.event);

                // Re-borrow the clock for the next iteration
                clock = test_throttler_unbuffered.clock.borrow_mut();
            }
        }

        assert_eq!(throttler.clock.borrow().timer_count(), 0);
        assert!(!throttler.is_limiting);
        assert_eq!(throttler.used(), 0.0);
        assert_eq!(throttler.recv_count, 6);
        assert_eq!(throttler.sent_count, 5);
    }

    #[rstest]
    fn test_dropping_send_message_after_dropping_message(test_throttler_unbuffered: TestThrottler) {
        let throttler = test_throttler_unbuffered.get_throttler();
        for _ in 0..6 {
            throttler.send(42);
        }

        // Advance time and process events
        {
            let mut clock = test_throttler_unbuffered.clock.borrow_mut();
            let time_events = clock.advance_time(test_throttler_unbuffered.interval.into(), true);
            for each_event in clock.match_handlers(time_events) {
                drop(clock); // Release the mutable borrow

                each_event.callback.call(each_event.event);

                // Re-borrow the clock for the next iteration
                clock = test_throttler_unbuffered.clock.borrow_mut();
            }
        }

        throttler.send(42);

        assert_eq!(throttler.used(), 0.2);
        assert_eq!(throttler.clock.borrow().timer_count(), 0);
        assert!(!throttler.is_limiting);
        assert_eq!(throttler.recv_count, 7);
        assert_eq!(throttler.sent_count, 6);
    }

    #[rstest]
    fn test_new_preserves_rate_limit() {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let rate_limit = RateLimit::new(5, 10);

        let throttler = Throttler::<u64, Box<dyn Fn(u64)>>::new(
            rate_limit,
            clock,
            "rate_limit",
            Box::new(|_| ()) as Box<dyn Fn(u64)>,
            None,
            Ustr::from("rate-limit-actor"),
        );

        assert_eq!(throttler.rate_limit(), rate_limit);
        assert_eq!(throttler.limit(), 5);
        assert_eq!(throttler.interval_ns(), 10);
    }

    #[rstest]
    fn test_debug_output_includes_identity_and_state() {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let actor_id = Ustr::from("debug-actor");
        let mut throttler = Throttler::<u64, Box<dyn Fn(u64)>>::new(
            RateLimit::new(5, 10),
            clock,
            "debug_timer",
            Box::new(|_| ()) as Box<dyn Fn(u64)>,
            None,
            actor_id,
        );

        throttler.send(42);

        let debug = format!("{throttler:?}");
        let timer_name = Ustr::from("debug_timer-debug-actor");

        assert!(debug.contains(&format!("actor_id: {actor_id:?}")));
        assert!(debug.contains(&format!("timer_name: {timer_name:?}")));
        assert!(debug.contains("limit: 5"));
        assert!(debug.contains("interval_ns: 10"));
        assert!(debug.contains("is_limiting: false"));
        assert!(debug.contains("recv_count: 1"));
        assert!(debug.contains("sent_count: 1"));
    }

    #[rstest]
    fn test_reset_clears_state_and_cancels_timer(test_throttler_buffered: TestThrottler) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..6 {
            throttler.send(42);
        }
        assert_eq!(timer_count_with_prefix(throttler, "buffer_timer"), 1);
        assert_eq!(throttler.qsize(), 1);

        throttler.reset();

        assert_eq!(throttler.qsize(), 0);
        assert_eq!(throttler.recv_count, 0);
        assert_eq!(throttler.sent_count, 0);
        assert!(!throttler.is_limiting);
        assert!(throttler.timestamps.is_empty());
        assert_eq!(timer_count_with_prefix(throttler, "buffer_timer"), 0);
        assert_eq!(throttler.clock.borrow().timer_count(), 0);
    }

    #[rstest]
    fn test_two_throttlers_share_clock_without_timer_collision() {
        let clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
        let interval = 10u64;

        let mk = |base: &str| -> SharedThrottler {
            let clock: Rc<RefCell<dyn Clock>> = Rc::clone(&clock) as Rc<RefCell<dyn Clock>>;
            Throttler::new(
                RateLimit::new(5, interval),
                clock,
                base,
                Box::new(|_| ()) as Box<dyn Fn(u64)>,
                None,
                Ustr::from(UUID4::new().as_str()),
            )
            .to_actor()
        };

        let t1 = mk("shared_timer");
        let t2 = mk("shared_timer");

        // Both throttlers use the same base timer name on a shared clock; the
        // namespaced names must keep both timers distinct.
        {
            let t1 = access_shared(&t1);

            for _ in 0..6 {
                t1.send(42);
            }
        }
        {
            let t2 = access_shared(&t2);

            for _ in 0..6 {
                t2.send(42);
            }
        }

        let clock_ref = clock.borrow();
        let names = clock_ref.timer_names();
        let shared_count = names
            .iter()
            .filter(|n| n.starts_with("shared_timer"))
            .count();
        assert_eq!(
            shared_count, 2,
            "two distinct timers expected, found {names:?}"
        );
    }

    #[rstest]
    fn test_try_reserve_then_send_interleaved(test_throttler_buffered: TestThrottler) {
        let throttler = test_throttler_buffered.get_throttler();

        // Reserve 3 of 5 slots, then send one more via the send path. Both
        // paths share the same window: 4 of 5 slots should be used.
        assert!(throttler.try_reserve(3));
        throttler.send(42);

        assert_eq!(throttler.recv_count, 4);
        assert_eq!(throttler.sent_count, 4);
        assert_eq!(throttler.used(), 0.8);
        assert!(!throttler.is_limiting);
    }

    #[rstest]
    fn test_dispose_cancels_timer_and_deregisters_endpoint(test_throttler_buffered: TestThrottler) {
        let throttler = test_throttler_buffered.get_throttler();

        for _ in 0..6 {
            throttler.send(42);
        }
        let actor_id = throttler.actor_id;
        let endpoint_name = format!("{actor_id}_process");
        assert_eq!(timer_count_with_prefix(throttler, "buffer_timer"), 1);
        assert!(msgbus::has_endpoint(&endpoint_name));

        throttler.dispose();

        assert_eq!(throttler.clock.borrow().timer_count(), 0);
        assert!(
            !msgbus::has_endpoint(&endpoint_name),
            "dispose must deregister the process endpoint"
        );
    }

    ////////////////////////////////////////////////////////////////////////////////
    // Property-based testing
    ////////////////////////////////////////////////////////////////////////////////

    use proptest::prelude::*;

    #[derive(Clone, Debug)]
    enum ThrottlerInput {
        SendMessage(u64),
        AdvanceClock(u8),
    }

    // Custom strategy for ThrottlerInput
    fn throttler_input_strategy() -> impl Strategy<Value = ThrottlerInput> {
        prop_oneof![
            2 => prop::bool::ANY.prop_map(|_| ThrottlerInput::SendMessage(42)),
            8 => prop::num::u8::ANY.prop_map(|v| ThrottlerInput::AdvanceClock(v % 5 + 5)),
        ]
    }

    // Custom strategy for ThrottlerTest
    fn throttler_test_strategy() -> impl Strategy<Value = Vec<ThrottlerInput>> {
        prop::collection::vec(throttler_input_strategy(), 10..=150)
    }

    fn test_throttler_with_inputs(inputs: Vec<ThrottlerInput>, test_throttler: &TestThrottler) {
        let test_clock = test_throttler.clock.clone();
        let interval = test_throttler.interval;
        let throttler = test_throttler.get_throttler();
        let mut sent_count = 0;

        for input in inputs {
            match input {
                ThrottlerInput::SendMessage(msg) => {
                    throttler.send(msg);
                    sent_count += 1;
                }
                ThrottlerInput::AdvanceClock(duration) => {
                    let mut clock_ref = test_clock.borrow_mut();
                    let current_time = clock_ref.get_time_ns();
                    let time_events =
                        clock_ref.advance_time(current_time + u64::from(duration), true);
                    for each_event in clock_ref.match_handlers(time_events) {
                        drop(clock_ref);
                        each_event.callback.call(each_event.event);
                        clock_ref = test_clock.borrow_mut();
                    }
                }
            }

            // Check the throttler rate limits on the appropriate conditions
            // * At least one message is buffered
            // * Timestamp queue is filled upto limit
            // * Least recent timestamp in queue exceeds interval
            let buffered_messages = throttler.qsize() > 0;
            let now = throttler.clock.borrow().timestamp_ns().as_u64();
            let limit_filled_within_interval = throttler
                .timestamps
                .get(throttler.limit() - 1)
                .is_some_and(|&ts| (now - ts.as_u64()) < interval);
            let expected_limiting = buffered_messages && limit_filled_within_interval;
            assert_eq!(throttler.is_limiting, expected_limiting);

            // Message conservation
            assert_eq!(sent_count, throttler.sent_count + throttler.qsize());
        }

        // Drain all buffered messages by repeatedly advancing the clock.
        // Each timer callback may send up to `limit` messages and schedule
        // a new timer for the next batch, so we must keep advancing.
        for i in 1..=100u64 {
            if throttler.qsize() == 0 {
                break;
            }
            let advance_to = interval * 100 * i;
            let time_events = test_clock
                .borrow_mut()
                .advance_time(advance_to.into(), true);
            let mut clock_ref = test_clock.borrow_mut();
            for each_event in clock_ref.match_handlers(time_events) {
                drop(clock_ref);
                each_event.callback.call(each_event.event);
                clock_ref = test_clock.borrow_mut();
            }
        }
        assert_eq!(throttler.qsize(), 0);
    }

    #[rstest]
    fn prop_test() {
        // Create a fresh throttler for each iteration to ensure clean state,
        // even when tests panic (which would skip the reset code)
        proptest!(|(inputs in throttler_test_strategy())| {
            let test_throttler = test_throttler_buffered();
            test_throttler_with_inputs(inputs, &test_throttler);
        });
    }

    #[rstest]
    fn prop_test_dropping() {
        // Drop-mode coverage: every received message is either sent or dropped,
        // and sent_count tracks the send callback exactly. Catches conservation
        // violations and panics under random send/advance sequences.
        proptest!(|(inputs in throttler_test_strategy())| {
            let clock = Rc::new(RefCell::new(TestClock::new()));
            let sent: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
            let dropped: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

            let sent_cb = {
                let sent = Rc::clone(&sent);
                Box::new(move |_| *sent.borrow_mut() += 1) as Box<dyn Fn(u64)>
            };
            let drop_cb = {
                let dropped = Rc::clone(&dropped);
                Box::new(move |_| *dropped.borrow_mut() += 1) as Box<dyn Fn(u64)>
            };

            let interval = 10u64;
            let throttler = Throttler::new(
                RateLimit::new(5, interval),
                Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
                "prop_drop_timer",
                sent_cb,
                Some(drop_cb),
                Ustr::from(UUID4::new().as_str()),
            )
            .to_actor();
            let throttler = access_shared(&throttler);

            for input in inputs {
                match input {
                    ThrottlerInput::SendMessage(msg) => {
                        throttler.send(msg);
                    }
                    ThrottlerInput::AdvanceClock(duration) => {
                        let mut clock_ref = clock.borrow_mut();
                        let current_time = clock_ref.get_time_ns();
                        let time_events =
                            clock_ref.advance_time(current_time + u64::from(duration), true);
                        for each_event in clock_ref.match_handlers(time_events) {
                            drop(clock_ref);
                            each_event.callback.call(each_event.event);
                            clock_ref = clock.borrow_mut();
                        }
                    }
                }

                let sent_now = *sent.borrow();
                let dropped_now = *dropped.borrow();
                // Conservation: every received message was sent or dropped.
                assert_eq!(sent_now + dropped_now, throttler.recv_count);
                assert_eq!(throttler.sent_count, sent_now);
                assert!(throttler.qsize() == 0, "drop mode must never buffer");
            }
        });
    }

    #[rstest]
    fn test_throttler_process_id_returns_ustr() {
        // This test verifies that ThrottlerProcess::id() correctly returns Ustr
        // by dereferencing MStr<Endpoint> (tests *self.endpoint -> Ustr conversion)
        let actor_id = Ustr::from("test_throttler");
        let process = ThrottlerProcess::<String, fn(String)>::new(actor_id);

        // Call id() which does *self.endpoint
        let handler_id: Ustr = process.id();

        // Verify it's a valid Ustr with expected format
        assert!(handler_id.as_str().contains("test_throttler_process"));
        assert!(!handler_id.is_empty());

        // Verify type - this wouldn't compile if id() didn't return Ustr
        let _type_check: Ustr = handler_id;
    }
}

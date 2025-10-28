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
    rc::Rc,
};

use nautilus_core::{UnixNanos, correctness::FAILED};
use ustr::Ustr;

use crate::{
    actor::{
        Actor,
        registry::{get_actor_unchecked, register_actor},
    },
    clock::Clock,
    msgbus::{
        self,
        handler::{MessageHandler, ShareableMessageHandler},
    },
    timer::{TimeEvent, TimeEventCallback},
};

/// Represents a throttling limit per interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: usize,
    pub interval_ns: u64,
}

impl RateLimit {
    /// Creates a new [`RateLimit`] instance.
    #[must_use]
    pub const fn new(limit: usize, interval_ns: u64) -> Self {
        Self { limit, interval_ns }
    }
}

/// Throttler rate limits messages by dropping or buffering them.
///
/// Throttler takes messages of type T and callback of type F for dropping
/// or processing messages.
pub struct Throttler<T, F> {
    /// The number of messages received.
    pub recv_count: usize,
    /// The number of messages sent.
    pub sent_count: usize,
    /// Whether the throttler is currently limiting the message rate.
    pub is_limiting: bool,
    /// The maximum number of messages that can be sent within the interval.
    pub limit: usize,
    /// The buffer of messages to be sent.
    pub buffer: VecDeque<T>,
    /// The timestamps of the sent messages.
    pub timestamps: VecDeque<UnixNanos>,
    /// The clock used to keep track of time.
    pub clock: Rc<RefCell<dyn Clock>>,
    /// The actor ID of the throttler.
    pub actor_id: Ustr,
    /// The interval between messages in nanoseconds.
    interval: u64,
    /// The name of the timer.
    timer_name: Ustr,
    /// The callback to send a message.
    output_send: F,
    /// The callback to drop a message.
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
        f.debug_struct(stringify!(InnerThrottler))
            .field("recv_count", &self.recv_count)
            .field("sent_count", &self.sent_count)
            .field("is_limiting", &self.is_limiting)
            .field("limit", &self.limit)
            .field("buffer", &self.buffer)
            .field("timestamps", &self.timestamps)
            .field("interval", &self.interval)
            .field("timer_name", &self.timer_name)
            .finish()
    }
}

impl<T, F> Throttler<T, F>
where
    T: Debug,
{
    #[inline]
    pub fn new(
        limit: usize,
        interval: u64,
        clock: Rc<RefCell<dyn Clock>>,
        timer_name: String,
        output_send: F,
        output_drop: Option<F>,
        actor_id: Ustr,
    ) -> Self {
        Self {
            recv_count: 0,
            sent_count: 0,
            is_limiting: false,
            limit,
            buffer: VecDeque::new(),
            timestamps: VecDeque::with_capacity(limit),
            clock,
            interval,
            timer_name: Ustr::from(&timer_name),
            output_send,
            output_drop,
            actor_id,
        }
    }

    /// Set timer with a callback to be triggered on next interval.
    ///
    /// Typically used to register callbacks:
    /// - to process buffered messages
    /// - to stop buffering
    ///
    /// # Panics
    ///
    /// Panics if setting the time alert on the internal clock fails.
    #[inline]
    pub fn set_timer(&mut self, callback: Option<TimeEventCallback>) {
        let delta = self.delta_next();
        let mut clock = self.clock.borrow_mut();
        if clock.timer_exists(&self.timer_name) {
            clock.cancel_timer(&self.timer_name);
        }
        let alert_ts = clock.timestamp_ns() + delta;

        clock
            .set_time_alert_ns(&self.timer_name, alert_ts, callback, None)
            .expect(FAILED);
    }

    /// Time delta when the next message can be sent.
    #[inline]
    pub fn delta_next(&mut self) -> u64 {
        match self.timestamps.get(self.limit - 1) {
            Some(ts) => {
                let diff = self.clock.borrow().timestamp_ns().as_u64() - ts.as_u64();
                self.interval.saturating_sub(diff)
            }
            None => 0,
        }
    }

    /// Reset the throttler which clears internal state.
    #[inline]
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.recv_count = 0;
        self.sent_count = 0;
        self.is_limiting = false;
        self.timestamps.clear();
    }

    /// Fractional value of rate limit consumed in current interval.
    #[inline]
    pub fn used(&self) -> f64 {
        if self.timestamps.is_empty() {
            return 0.0;
        }

        let now = self.clock.borrow().timestamp_ns().as_i64();
        let interval_start = now - self.interval as i64;

        let messages_in_current_interval = self
            .timestamps
            .iter()
            .take_while(|&&ts| ts.as_i64() > interval_start)
            .count();

        (messages_in_current_interval as f64) / (self.limit as f64)
    }

    /// Number of messages queued in buffer.
    #[inline]
    pub fn qsize(&self) -> usize {
        self.buffer.len()
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
        msgbus::register(
            process_handler.id().as_str().into(),
            ShareableMessageHandler::from(Rc::new(process_handler) as Rc<dyn MessageHandler>),
        );

        // Register actor state and return the wrapped reference
        register_actor(self)
    }

    #[inline]
    pub fn send_msg(&mut self, msg: T) {
        let now = self.clock.borrow().timestamp_ns();

        if self.timestamps.len() >= self.limit {
            self.timestamps.pop_back();
        }
        self.timestamps.push_front(now);

        self.sent_count += 1;
        (self.output_send)(msg);
    }

    #[inline]
    pub fn limit_msg(&mut self, msg: T) {
        let callback = if self.output_drop.is_none() {
            self.buffer.push_front(msg);
            log::debug!("Buffering {}", self.buffer.len());
            Some(ThrottlerProcess::<T, F>::new(self.actor_id).get_timer_callback())
        } else {
            log::debug!("Dropping");
            if let Some(drop) = &self.output_drop {
                drop(msg);
            }
            Some(throttler_resume::<T, F>(self.actor_id))
        };
        if !self.is_limiting {
            log::debug!("Limiting");
            self.set_timer(callback);
            self.is_limiting = true;
        }
    }

    #[inline]
    pub fn send(&mut self, msg: T)
    where
        T: 'static,
        F: Fn(T) + 'static,
    {
        self.recv_count += 1;

        if self.is_limiting || self.delta_next() > 0 {
            self.limit_msg(msg);
        } else {
            self.send_msg(msg);
        }
    }
}

/// Process buffered messages for throttler
///
/// When limit is reached, schedules a timer event to call self again. The handler
/// is registered as a separated endpoint on the message bus as `{actor_id}_process`.
struct ThrottlerProcess<T, F> {
    actor_id: Ustr,
    endpoint: Ustr,
    phantom_t: PhantomData<T>,
    phantom_f: PhantomData<F>,
}

impl<T, F> ThrottlerProcess<T, F>
where
    T: Debug,
{
    pub fn new(actor_id: Ustr) -> Self {
        let endpoint = Ustr::from(&format!("{actor_id}_process"));
        Self {
            actor_id,
            endpoint,
            phantom_t: PhantomData,
            phantom_f: PhantomData,
        }
    }

    pub fn get_timer_callback(&self) -> TimeEventCallback {
        let endpoint = self.endpoint.into(); // TODO: Optimize this
        TimeEventCallback::from(move |event: TimeEvent| {
            msgbus::send_any(endpoint, &(event));
        })
    }
}

impl<T, F> MessageHandler for ThrottlerProcess<T, F>
where
    T: 'static + Debug,
    F: Fn(T) + 'static,
{
    fn id(&self) -> Ustr {
        self.endpoint
    }

    fn handle(&self, _message: &dyn Any) {
        let throttler = get_actor_unchecked::<Throttler<T, F>>(&self.actor_id);
        while let Some(msg) = throttler.buffer.pop_back() {
            throttler.send_msg(msg);

            // Set timer to process more buffered messages
            // if interval limit reached and there are more
            // buffered messages to process
            if !throttler.buffer.is_empty() && throttler.delta_next() > 0 {
                throttler.is_limiting = true;

                let endpoint = self.endpoint.into(); // TODO: Optimize this

                // Send message to throttler process endpoint to resume
                throttler.set_timer(Some(TimeEventCallback::from(move |event: TimeEvent| {
                    msgbus::send_any(endpoint, &(event));
                })));
                return;
            }
        }

        throttler.is_limiting = false;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Sets throttler to resume sending messages
pub fn throttler_resume<T, F>(actor_id: Ustr) -> TimeEventCallback
where
    T: 'static + Debug,
    F: Fn(T) + 'static,
{
    TimeEventCallback::from(move |_event: TimeEvent| {
        let throttler = get_actor_unchecked::<Throttler<T, F>>(&actor_id);
        throttler.is_limiting = false;
    })
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        cell::{RefCell, UnsafeCell},
        rc::Rc,
    };

    use nautilus_core::UUID4;
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::{RateLimit, Throttler};
    use crate::clock::TestClock;
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
        #[allow(clippy::mut_from_ref)]
        pub fn get_throttler(&self) -> &mut Throttler<u64, Box<dyn Fn(u64)>> {
            unsafe { &mut *self.throttler.get() }
        }
    }

    #[fixture]
    pub fn test_throttler_buffered() -> TestThrottler {
        let output_send: Box<dyn Fn(u64)> = Box::new(|msg: u64| {
            log::debug!("Sent: {msg}");
        });
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let inner_clock = Rc::clone(&clock);
        let rate_limit = RateLimit::new(5, 10);
        let interval = rate_limit.interval_ns;
        let actor_id = Ustr::from(&UUID4::new().to_string());

        TestThrottler {
            throttler: Throttler::new(
                rate_limit.limit,
                rate_limit.interval_ns,
                clock,
                "buffer_timer".to_string(),
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
        let interval = rate_limit.interval_ns;
        let actor_id = Ustr::from(&UUID4::new().to_string());

        TestThrottler {
            throttler: Throttler::new(
                rate_limit.limit,
                rate_limit.interval_ns,
                clock,
                "dropper_timer".to_string(),
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
        assert_eq!(throttler.clock.borrow().timer_names(), vec!["buffer_timer"]);
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
        assert_eq!(
            throttler.clock.borrow().timer_names(),
            vec!["dropper_timer"]
        );
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

    fn test_throttler_with_inputs(inputs: Vec<ThrottlerInput>, test_throttler: TestThrottler) {
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
                .get(throttler.limit - 1)
                .is_some_and(|&ts| (now - ts.as_u64()) < interval);
            let expected_limiting = buffered_messages && limit_filled_within_interval;
            assert_eq!(throttler.is_limiting, expected_limiting);

            // Message conservation
            assert_eq!(sent_count, throttler.sent_count + throttler.qsize());
        }

        // Advance clock by a large amount to process all messages
        let time_events = test_clock
            .borrow_mut()
            .advance_time((interval * 100).into(), true);
        let mut clock_ref = test_clock.borrow_mut();
        for each_event in clock_ref.match_handlers(time_events) {
            drop(clock_ref);
            each_event.callback.call(each_event.event);
            clock_ref = test_clock.borrow_mut();
        }
        assert_eq!(throttler.qsize(), 0);
    }

    #[ignore = "Used for manually testing failing cases"]
    #[rstest]
    fn test_case() {
        let inputs = [
            ThrottlerInput::SendMessage(42),
            ThrottlerInput::AdvanceClock(5),
            ThrottlerInput::SendMessage(42),
            ThrottlerInput::SendMessage(42),
            ThrottlerInput::SendMessage(42),
            ThrottlerInput::SendMessage(42),
            ThrottlerInput::SendMessage(42),
            ThrottlerInput::AdvanceClock(5),
            ThrottlerInput::SendMessage(42),
            ThrottlerInput::SendMessage(42),
        ]
        .to_vec();

        let test_throttler = test_throttler_buffered();
        test_throttler_with_inputs(inputs, test_throttler);
    }

    #[rstest]
    #[allow(unsafe_code)]
    fn prop_test() {
        let test_throttler = test_throttler_buffered();

        proptest!(move |(inputs in throttler_test_strategy())| {
            test_throttler_with_inputs(inputs, test_throttler.clone());
            // Reset throttler state between runs
            let throttler = unsafe { &mut *test_throttler.throttler.get() };
            throttler.reset();
            throttler.clock.borrow_mut().reset();
        });
    }
}

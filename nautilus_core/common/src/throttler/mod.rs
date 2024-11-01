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

pub mod callbacks;
pub mod inner;

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use callbacks::{ThrottlerProcess, ThrottlerResume};
use inner::InnerThrottler;

use crate::clock::Clock;

/// Represents a throttling limit per interval.
pub struct RateLimit {
    pub limit: usize,
    pub interval_ns: u64,
}

impl RateLimit {
    #[must_use]
    pub const fn new(limit: usize, interval_ns: u64) -> Self {
        Self { limit, interval_ns }
    }
}

/// Shareable reference to an [`InnerThrottler`]
///
/// Throttler takes messages of type T and callback of type F for dropping
/// or processing messages.
#[derive(Clone)]
pub struct Throttler<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}

impl<T, F> Debug for Throttler<T, F>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Throttler))
            .field("inner", &self.inner)
            .finish()
    }
}

impl<T, F> Throttler<T, F> {
    pub fn new(
        rate_limit: RateLimit,
        clock: Rc<RefCell<dyn Clock>>,
        timer_name: String,
        output_send: F,
        output_drop: Option<F>,
    ) -> Self {
        let inner = InnerThrottler::new(
            rate_limit.limit,
            rate_limit.interval_ns,
            clock,
            timer_name,
            output_send,
            output_drop,
        );

        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    #[must_use]
    pub fn qsize(&self) -> usize {
        let inner = self.inner.borrow();
        inner.buffer.len()
    }

    pub fn reset(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.reset();
    }

    #[must_use]
    pub fn used(&self) -> f64 {
        let inner = self.inner.borrow();
        inner.used()
    }
}

impl<T, F> Throttler<T, F>
where
    F: Fn(T) + 'static,
    T: 'static,
{
    pub fn send(&self, msg: T) {
        let throttler_clone = Self {
            inner: self.inner.clone(),
        };
        let mut inner = self.inner.borrow_mut();
        inner.recv_count += 1;

        if inner.is_limiting || inner.delta_next() > 0 {
            inner.limit_msg(msg, throttler_clone);
        } else {
            inner.send_msg(msg);
        }
    }

    fn get_process_callback(&self) -> ThrottlerProcess<T, F> {
        ThrottlerProcess::new(self.inner.clone())
    }

    fn get_resume_callback(&self) -> ThrottlerResume<T, F> {
        ThrottlerResume::new(self.inner.clone())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use rstest::{fixture, rstest};

    use super::{RateLimit, Throttler};
    use crate::clock::TestClock;

    /// Test throttler with default values for testing
    ///
    /// - Rate limit is 5 messages in 10 intervals.
    /// - Message handling is decided by specific fixture
    struct TestThrottler {
        throttler: Throttler<u64, Box<dyn Fn(u64)>>,
        clock: Rc<RefCell<TestClock>>,
        interval: u64,
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

        TestThrottler {
            throttler: Throttler::new(
                rate_limit,
                clock,
                "buffer_timer".to_string(),
                output_send,
                None,
            ),
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

        TestThrottler {
            throttler: Throttler::new(
                rate_limit,
                clock,
                "dropper_timer".to_string(),
                output_send,
                Some(output_drop),
            ),
            clock: inner_clock,
            interval,
        }
    }

    #[rstest]
    fn test_buffering_send_to_limit_becomes_throttled(mut test_throttler_buffered: TestThrottler) {
        let throttler = &mut test_throttler_buffered.throttler;
        for _ in 0..6 {
            throttler.send(42);
        }
        assert_eq!(throttler.qsize(), 1);

        let inner = throttler.inner.borrow();
        assert!(inner.is_limiting);
        assert_eq!(inner.recv_count, 6);
        assert_eq!(inner.sent_count, 5);
        assert_eq!(inner.clock.borrow().timer_names(), vec!["buffer_timer"]);
    }

    #[rstest]
    fn test_buffering_used_when_sent_to_limit_returns_one(
        mut test_throttler_buffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_buffered.throttler;

        for _ in 0..5 {
            throttler.send(42);
        }

        let inner = throttler.inner.borrow();
        assert_eq!(inner.used(), 1.0);
        assert_eq!(inner.recv_count, 5);
        assert_eq!(inner.sent_count, 5);
    }

    #[rstest]
    fn test_buffering_used_when_half_interval_from_limit_returns_one(
        mut test_throttler_buffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_buffered.throttler;

        for _ in 0..5 {
            throttler.send(42);
        }

        let half_interval = test_throttler_buffered.interval / 2;
        // Advance the clock by half the interval
        {
            let mut clock = test_throttler_buffered.clock.borrow_mut();
            clock.advance_time(half_interval.into(), true);
        }

        let inner = throttler.inner.borrow();
        assert_eq!(inner.used(), 1.0);
        assert_eq!(inner.recv_count, 5);
        assert_eq!(inner.sent_count, 5);
    }

    #[rstest]
    fn test_buffering_used_before_limit_when_halfway_returns_half(
        mut test_throttler_buffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_buffered.throttler;

        for _ in 0..3 {
            throttler.send(42);
        }

        let inner = throttler.inner.borrow();
        assert_eq!(inner.used(), 0.6);
        assert_eq!(inner.recv_count, 3);
        assert_eq!(inner.sent_count, 3);
    }

    #[rstest]
    fn test_buffering_refresh_when_at_limit_sends_remaining_items(
        mut test_throttler_buffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_buffered.throttler;

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
        {
            let inner = throttler.inner.borrow();
            assert_eq!(inner.used(), 0.2);
            assert_eq!(inner.recv_count, 6);
            assert_eq!(inner.sent_count, 6);
            assert_eq!(inner.qsize(), 0);
        }
    }

    #[rstest]
    fn test_buffering_send_message_after_buffering_message(
        mut test_throttler_buffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_buffered.throttler;

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

        for _ in 0..6 {
            throttler.send(42);
        }

        // Assert final state
        {
            let inner = throttler.inner.borrow();
            assert_eq!(inner.used(), 1.0);
            assert_eq!(inner.recv_count, 12);
            assert_eq!(inner.sent_count, 10);
            assert_eq!(inner.qsize(), 2);
        }
    }

    #[rstest]
    fn test_buffering_send_message_after_halfway_after_buffering_message(
        mut test_throttler_buffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_buffered.throttler;

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
        {
            let inner = throttler.inner.borrow();
            assert_eq!(inner.used(), 0.8);
            assert_eq!(inner.recv_count, 9);
            assert_eq!(inner.sent_count, 9);
            assert_eq!(inner.qsize(), 0);
        }
    }

    #[rstest]
    fn test_dropping_send_sends_message_to_handler(mut test_throttler_unbuffered: TestThrottler) {
        let throttler = &mut test_throttler_unbuffered.throttler;
        throttler.send(42);
        let inner = throttler.inner.borrow();

        assert!(!inner.is_limiting);
        assert_eq!(inner.recv_count, 1);
        assert_eq!(inner.sent_count, 1);
    }

    #[rstest]
    fn test_dropping_send_to_limit_drops_message(mut test_throttler_unbuffered: TestThrottler) {
        let throttler = &mut test_throttler_unbuffered.throttler;
        for _ in 0..6 {
            throttler.send(42);
        }
        assert_eq!(throttler.qsize(), 0);

        let inner = throttler.inner.borrow();
        assert!(inner.is_limiting);
        assert_eq!(inner.used(), 1.0);
        assert_eq!(inner.clock.borrow().timer_count(), 1);
        assert_eq!(inner.clock.borrow().timer_names(), vec!["dropper_timer"]);
        assert_eq!(inner.recv_count, 6);
        assert_eq!(inner.sent_count, 5);
    }

    #[rstest]
    fn test_dropping_advance_time_when_at_limit_dropped_message(
        mut test_throttler_unbuffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_unbuffered.throttler;
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

        let inner = throttler.inner.borrow();
        assert_eq!(inner.clock.borrow().timer_count(), 0);
        assert!(!inner.is_limiting);
        assert_eq!(inner.used(), 0.0);
        assert_eq!(inner.recv_count, 6);
        assert_eq!(inner.sent_count, 5);
    }

    #[rstest]
    fn test_dropping_send_message_after_dropping_message(
        mut test_throttler_unbuffered: TestThrottler,
    ) {
        let throttler = &mut test_throttler_unbuffered.throttler;
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

        let inner = throttler.inner.borrow();
        assert_eq!(inner.used(), 0.2);
        assert_eq!(inner.clock.borrow().timer_count(), 0);
        assert!(!inner.is_limiting);
        assert_eq!(inner.recv_count, 7);
        assert_eq!(inner.sent_count, 6);
    }

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

    fn test_throttler_with_inputs(inputs: Vec<ThrottlerInput>) {
        let TestThrottler {
            throttler,
            clock: test_clock,
            interval,
        } = test_throttler_buffered();
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
            // * Atleast one message is buffered
            // * Timestamp queue is filled upto limit
            // * Least recent timestamp in queue exceeds interval
            let inner = throttler.inner.borrow();
            let buffered_messages = inner.qsize() > 0;
            let now = inner.clock.borrow().timestamp_ns().as_u64();
            let limit_filled_within_interval = inner
                .timestamps
                .get(inner.limit - 1)
                .map_or(false, |&ts| (now - ts.as_u64()) < interval);
            let expected_limiting = buffered_messages && limit_filled_within_interval;
            assert_eq!(inner.is_limiting, expected_limiting);

            // Message conservation
            let inner = throttler.inner.borrow();
            assert_eq!(sent_count, inner.sent_count + inner.qsize());
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

    #[test]
    #[ignore = "Used for manually testing failing cases"]
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

        test_throttler_with_inputs(inputs);
    }

    proptest! {
        #[test]
        fn test(inputs in throttler_test_strategy()) {
            test_throttler_with_inputs(inputs);
        }
    }
}

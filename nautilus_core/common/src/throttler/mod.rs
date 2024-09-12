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

use std::{cell::RefCell, cmp::max, collections::VecDeque, rc::Rc};

use nautilus_core::nanos::UnixNanos;

use crate::{
    clock::Clock,
    timer::{RustTimeEventCallback, TimeEvent, TimeEventCallback},
};

type ThrottlerWrapperCallbackFn<T, F> = Rc<dyn Fn(&mut Throttler<T, F>, TimeEvent)>;

pub struct ThrottlerWrapper<T, F: Fn(T)> {
    throttler: RefCell<Throttler<T, F>>,
    callback: ThrottlerWrapperCallbackFn<T, F>,
}

impl<T, F: Fn(T)> ThrottlerWrapper<T, F> {
    fn _new(
        throttler: Throttler<T, F>,
        callback: impl Fn(&mut Throttler<T, F>, TimeEvent) + 'static,
    ) -> Self {
        Self {
            throttler: RefCell::new(throttler),
            callback: Rc::new(callback),
        }
    }
}

impl<T, F: Fn(T)> RustTimeEventCallback for ThrottlerWrapper<T, F> {
    fn call(&self, event: TimeEvent) {
        let mut throttler = self.throttler.borrow_mut();
        (self.callback)(&mut throttler, event);
    }
}

pub struct Throttler<T, F: Fn(T)> {
    pub recv_count: usize,
    /// The number of messages sent.
    pub sent_count: usize,
    /// Whether the throttler is currently limiting the message rate.
    pub is_limiting: bool,
    /// The maximum number of messages that can be sent within the interval.
    pub limit: usize,
    /// The buffer of messages to be sent.
    buffer: VecDeque<T>,
    /// The timestamps of the sent messages.
    timestamps: VecDeque<UnixNanos>,
    /// Whether the throttler has warmed up.
    warm: bool,
    /// The interval between messages in nanoseconds.
    interval: u64,
    /// The clock used to keep track of time.
    clock: Box<dyn Clock>,
    /// The name of the timer.
    timer_name: String,
    /// The function to send a message.
    output_send: F,
    /// The function to drop a message.
    output_drop: Option<F>,
    wrapper: Option<Rc<ThrottlerWrapper<T, F>>>,
}

impl<T: 'static, F: Fn(T) + 'static> Throttler<T, F> {
    pub fn new(
        limit: usize,
        interval: u64,
        clock: Box<dyn Clock>,
        timer_name: String,
        output_send: F,
        output_drop: Option<F>,
    ) -> Self {
        Self {
            recv_count: 0,
            sent_count: 0,
            is_limiting: false,
            limit,
            buffer: VecDeque::new(),
            timestamps: VecDeque::with_capacity(limit),
            warm: false,
            interval,
            clock,
            timer_name,
            output_send,
            output_drop,
            wrapper: None,
        }
    }

    /// Returns the number of messages in the buffer.
    // #[must_use]
    pub fn qsize(&self) -> usize {
        self.buffer.len()
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.warm = false;
        self.recv_count = 0;
        self.sent_count = 0;
        self.is_limiting = false;
    }

    pub fn used(&self) -> f64 {
        if !self.warm && self.sent_count < 2 {
            return 0.0;
        }

        let diff = max(
            0,
            self.interval as i64
                - (self.clock.timestamp_ns().as_i64() - self.timestamps.back().unwrap().as_i64()),
        );
        let mut used = diff as f64 / self.interval as f64;

        if !self.warm {
            used *= self.sent_count as f64 / self.limit as f64;
        }

        used
    }

    pub fn send(&mut self, msg: T) {
        self.recv_count += 1;

        if self.is_limiting || self.delta_next() > 0 {
            self.limit_msg(msg);
        } else {
            self.send_msg(msg);
        }
    }

    pub fn delta_next(&mut self) -> i64 {
        if !self.warm && self.sent_count < self.limit {
            return 0;
        }
        self.warm = true;

        let diff = self.clock.timestamp_ns().as_u64()
            - self
                .timestamps
                .back()
                .unwrap_or_else(|| panic!("Failed to get timestamp"))
                .as_u64();
        self.interval as i64 - diff as i64
    }

    fn limit_msg(&mut self, msg: T) {
        if self.output_drop.is_none() {
            self.buffer.push_front(msg);
            self.set_callback(|throttler, event| throttler.process(event));
            log::debug!("Buffering {}", self.buffer.len());
        } else {
            log::debug!("Dropping");
            (self.output_send)(msg);
            self.set_callback(|throttler, event| throttler.resume(event));
        }

        if !self.is_limiting {
            log::debug!("Limiting");
            self.set_timer();
            self.is_limiting = true;
        }
    }

    fn set_timer(&mut self) {
        if self.clock.timer_names().contains(&self.timer_name.as_str()) {
            self.clock.cancel_timer(&self.timer_name);
        }

        let delta_next = self.delta_next();

        self.clock.set_time_alert_ns(
            &self.timer_name,
            self.clock.timestamp_ns() + delta_next as u64,
            Some(TimeEventCallback::Rust(self.wrapper.clone().expect("T"))),
        );
    }

    fn process(&mut self, _event: TimeEvent) {
        while let Some(msg) = self.buffer.pop_back() {
            self.send_msg(msg);

            if self.delta_next() > 0 {
                self.set_callback(|throttler, event| throttler.process(event));
                self.set_timer();
                return;
            }
        }

        self.is_limiting = false;
        self.wrapper = None;
    }

    fn resume(&mut self, _event: TimeEvent) {
        self.is_limiting = false;
    }

    fn send_msg(&mut self, msg: T) {
        self.timestamps.push_front(self.clock.timestamp_ns());
        (self.output_send)(msg);
        self.sent_count += 1;
    }

    fn set_callback<C>(&mut self, _f: C)
    where
        C: Fn(&mut Throttler<T, F>, TimeEvent) + 'static,
    {
        // self.wrapper = Some(Rc::new(ThrottlerWrapper::new(self, f)))
    }
}
////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{rc::Rc, thread::sleep, time::Duration};

    use rstest::*;

    use super::*;
    use crate::clock::TestClock;

    fn round_to_precision(value: f64) -> f64 {
        let precision = 1;
        let factor = 10f64.powi(precision as i32);
        (value * factor).round() / factor
    }

    // TODO: add into stubs.rs
    pub struct TestThrottler {
        pub inner: Throttler<Rc<String>, Box<dyn Fn(Rc<String>)>>,
    }

    // TODO: add comments
    impl TestThrottler {
        pub fn new(buffer: bool) -> Self {
            let output_send: Box<dyn Fn(Rc<String>)> = Box::new(|msg: Rc<String>| {
                log::debug!("Sent: {}", msg);
            });
            let clock = TestClock::new();
            clock.make_realtime();

            if buffer {
                TestThrottler {
                    inner: Throttler::new(
                        5,
                        1_000_000_000,
                        Box::new(clock),
                        "buffer_timer".to_string(),
                        output_send,
                        None,
                    ),
                }
            } else {
                let output_drop: Box<dyn Fn(Rc<String>)> = Box::new(|msg: Rc<String>| {
                    log::debug!("Dropped: {}", msg);
                });

                TestThrottler {
                    inner: Throttler::new(
                        5,
                        1_000_000_000,
                        Box::new(clock),
                        "dropper_timer".to_string(),
                        output_send,
                        Some(output_drop),
                    ),
                }
            }
        }

        pub fn advance_time(&self, time: UnixNanos) {
            sleep(Duration::from_nanos(time.as_u64()));
        }
    }

    #[rstest]
    fn test_buffering_throttler_instantiation() {
        let throttler = TestThrottler::new(true);

        assert_eq!(throttler.inner.recv_count, 0);
        assert_eq!(throttler.inner.sent_count, 0);
        assert_eq!(throttler.inner.used(), 0.0);
        assert_eq!(throttler.inner.qsize(), 0);
        assert!(!throttler.inner.is_limiting);
        assert!(!throttler.inner.warm);
        assert_eq!(throttler.inner.limit, 5);
        assert_eq!(throttler.inner.buffer.len(), 0);
        assert_eq!(throttler.inner.timestamps.len(), 0);
        assert_eq!(throttler.inner.interval, 1_000_000_000);
        assert_eq!(throttler.inner.timer_name, "buffer_timer".to_string());
    }

    #[rstest]
    fn test_buffering_send_sends_message_to_handler() {
        let mut throttler = TestThrottler::new(true);
        let msg = Rc::new("MESSAGE".to_string());

        throttler.inner.send(msg.clone());

        assert_eq!(throttler.inner.qsize(), 0);
        assert_eq!(throttler.inner.recv_count, 1);
        assert_eq!(throttler.inner.sent_count, 1);
    }

    #[rstest]
    fn test_buffering_send_to_limit_becomes_throttled() {
        let mut throttler = TestThrottler::new(true);
        let msg = Rc::new("MESSAGE".to_string());

        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());

        assert_eq!(throttler.inner.qsize(), 1);
        assert!(throttler.inner.is_limiting);
        assert_eq!(throttler.inner.recv_count, 6);
        assert_eq!(throttler.inner.sent_count, 5);
        assert_eq!(throttler.inner.clock.timer_names(), vec!["buffer_timer"]);
        // above is failing because in fn it is blank in wrapper
    }

    #[rstest]
    fn test_buffering_used_when_sent_to_limit_returns_one() {
        let mut throttler = TestThrottler::new(true);
        let msg = Rc::new("MESSAGE".to_string());

        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());

        assert_eq!(round_to_precision(throttler.inner.used()), 1.0);
        assert_eq!(throttler.inner.recv_count, 5);
        assert_eq!(throttler.inner.sent_count, 5);
    }

    #[rstest]
    fn test_buffering_used_when_half_interval_from_limit_returns_half() {
        let mut throttler = TestThrottler::new(true);
        let msg = Rc::new("MESSAGE".to_string());

        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());

        // Advance the clock by half the interval
        throttler.advance_time(500_000_000.into());

        //  Todo: Add comment why this
        assert_eq!(round_to_precision(throttler.inner.used()), 0.5);
        assert_eq!(throttler.inner.recv_count, 5);
        assert_eq!(throttler.inner.sent_count, 5);
    }

    #[rstest]
    fn test_buffering_used_before_limit_when_halfway_returns_half() {
        let mut throttler = TestThrottler::new(true);
        let msg = Rc::new("MESSAGE".to_string());

        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());

        assert_eq!(round_to_precision(throttler.inner.used()), 0.6);
        assert_eq!(throttler.inner.recv_count, 3);
        assert_eq!(throttler.inner.sent_count, 3);
    }

    // #[rstest]
    // fn test_buffering_refresh_when_at_limit_sends_remaining_items() {}

    // #[rstest]
    // fn test_buffering_send_message_after_dropping_message() {}

    // // Now, Dropping Messages
    #[rstest]
    fn test_dropping_throttler_instantiation() {
        let throttler = TestThrottler::new(false);

        assert_eq!(throttler.inner.recv_count, 0);
        assert_eq!(throttler.inner.sent_count, 0);
        assert_eq!(throttler.inner.used(), 0.0);
        assert_eq!(throttler.inner.qsize(), 0);
        assert!(!throttler.inner.is_limiting);
        assert!(!throttler.inner.warm);
        assert_eq!(throttler.inner.limit, 5);
        assert_eq!(throttler.inner.buffer.len(), 0);
        assert_eq!(throttler.inner.timestamps.len(), 0);
        assert_eq!(throttler.inner.interval, 1_000_000_000);
        assert_eq!(throttler.inner.timer_name, "dropper_timer".to_string());
    }

    #[rstest]
    fn test_dropping_send_sends_message_to_handler() {
        let mut throttler = TestThrottler::new(false);
        let msg = Rc::new("MESSAGE".to_string());

        throttler.inner.send(msg.clone());

        assert_eq!(throttler.inner.qsize(), 0);
        assert_eq!(throttler.inner.recv_count, 1);
        assert_eq!(throttler.inner.sent_count, 1);
    }

    #[rstest]
    fn test_send_to_limit_drops_message() {
        let mut throttler = TestThrottler::new(false);
        let msg = Rc::new("MESSAGE".to_string());

        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());
        throttler.inner.send(msg.clone());

        assert_eq!(throttler.inner.qsize(), 0);
        assert_eq!(throttler.inner.recv_count, 6);
        assert_eq!(throttler.inner.sent_count, 5);
        assert_eq!(round_to_precision(throttler.inner.used()), 1.0);
        assert!(throttler.inner.is_limiting);
        assert_eq!(throttler.inner.clock.timer_names(), vec!["dropper_timer"]);
    }

    // #[rstest]
    // fn test_advance_time_when_at_limit_dropped_message() {}

    // #[rstest]
    // fn test_send_message_after_dropping_message() {}
}

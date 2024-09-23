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

use std::{cell::RefCell, rc::Rc};

use callbacks::{ThrottlerProcess, ThrottlerResume};
use inner::InnerThrottler;

use crate::clock::Clock;

#[derive(Clone)]
pub struct Throttler<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}
impl<T, F> Throttler<T, F> {
    pub fn new(
        limit: usize,
        interval: u64,
        clock: Box<dyn Clock>,
        timer_name: String,
        output_send: F,
        output_drop: Option<F>,
    ) -> Self {
        let inner =
            InnerThrottler::new(limit, interval, clock, timer_name, output_send, output_drop);

        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    /// Returns the number of messages in the buffer.
    // #[must_use]
    pub fn qsize(&self) -> usize {
        let inner = self.inner.borrow();
        inner.buffer.len()
    }

    pub fn reset(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.reset();
    }

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
        let throttler_clone = Throttler {
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
// #[cfg(test)]
mod tests {
    use super::Throttler;

    use rstest::*;

    use crate::clock::TestClock;

    // fn round_to_precision(value: f64) -> f64 {
    //     let precision = 1;
    //     let factor = 10f64.powi(precision as i32);
    //     (value * factor).round() / factor
    // }

    type TestThrottler = Throttler<String, Box<dyn Fn(String)>>;

    #[fixture]
    pub fn test_throttler_buffered() -> TestThrottler {
        let output_send: Box<dyn Fn(String)> = Box::new(|msg: String| {
            log::debug!("Sent: {}", msg);
        });
        let clock = TestClock::new();

        Throttler::new(
            5,
            1_000_000_000,
            Box::new(clock),
            "buffer_timer".to_string(),
            output_send,
            None,
        )
    }

    #[fixture]
    pub fn test_throttler_unbuffered() -> TestThrottler {
        let output_drop: Box<dyn Fn(String)> = Box::new(|msg: String| {
            log::debug!("Dropped: {}", msg);
        });
        let clock = TestClock::new();

        Throttler::new(
            5,
            1_000_000_000,
            Box::new(clock),
            "buffer_timer".to_string(),
            output_drop,
            None,
        )
    }

    #[rstest]
    fn test_buffering_send_to_limit_becomes_throttled(
        test_throttler_buffered: Throttler<String, Box<dyn Fn(String)>>,
    ) {
        let throttler = test_throttler_buffered;
        throttler.send("MESSAGE".to_string());
        throttler.send("MESSAGE".to_string());
        throttler.send("MESSAGE".to_string());
        throttler.send("MESSAGE".to_string());
        throttler.send("MESSAGE".to_string());
        throttler.send("MESSAGE".to_string());
        assert_eq!(throttler.qsize(), 1);

        let inner = throttler.inner.borrow();
        assert!(inner.is_limiting);
        assert_eq!(inner.recv_count, 6);
        assert_eq!(inner.sent_count, 5);
        assert_eq!(inner.clock.timer_names(), vec!["buffer_timer"]);
    }

    //     #[rstest]
    //     fn test_buffering_used_when_sent_to_limit_returns_one() {
    //         let mut throttler = TestThrottler::new(true);
    //         let msg = Rc::new("MESSAGE".to_string());

    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());

    //         assert_eq!(round_to_precision(throttler.inner.used()), 1.0);
    //         assert_eq!(throttler.inner.recv_count, 5);
    //         assert_eq!(throttler.inner.sent_count, 5);
    //     }

    //     #[rstest]
    //     fn test_buffering_used_when_half_interval_from_limit_returns_half() {
    //         let mut throttler = TestThrottler::new(true);
    //         let msg = Rc::new("MESSAGE".to_string());

    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());

    //         // Advance the clock by half the interval
    //         throttler.advance_time(500_000_000.into());

    //         //  Todo: Add comment why this
    //         assert_eq!(round_to_precision(throttler.inner.used()), 0.5);
    //         assert_eq!(throttler.inner.recv_count, 5);
    //         assert_eq!(throttler.inner.sent_count, 5);
    //     }

    //     #[rstest]
    //     fn test_buffering_used_before_limit_when_halfway_returns_half() {
    //         let mut throttler = TestThrottler::new(true);
    //         let msg = Rc::new("MESSAGE".to_string());

    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());

    //         assert_eq!(round_to_precision(throttler.inner.used()), 0.6);
    //         assert_eq!(throttler.inner.recv_count, 3);
    //         assert_eq!(throttler.inner.sent_count, 3);
    //     }

    //     // #[rstest]
    //     // fn test_buffering_refresh_when_at_limit_sends_remaining_items() {}

    //     // #[rstest]
    //     // fn test_buffering_send_message_after_dropping_message() {}

    //     // // Now, Dropping Messages
    //     #[rstest]
    //     fn test_dropping_throttler_instantiation() {
    //         let throttler = TestThrottler::new(false);

    //         assert_eq!(throttler.inner.recv_count, 0);
    //         assert_eq!(throttler.inner.sent_count, 0);
    //         assert_eq!(throttler.inner.used(), 0.0);
    //         assert_eq!(throttler.inner.qsize(), 0);
    //         assert!(!throttler.inner.is_limiting);
    //         assert!(!throttler.inner.warm);
    //         assert_eq!(throttler.inner.limit, 5);
    //         assert_eq!(throttler.inner.buffer.len(), 0);
    //         assert_eq!(throttler.inner.timestamps.len(), 0);
    //         assert_eq!(throttler.inner.interval, 1_000_000_000);
    //         assert_eq!(throttler.inner.timer_name, "dropper_timer".to_string());
    //     }

    //     #[rstest]
    //     fn test_dropping_send_sends_message_to_handler() {
    //         let mut throttler = TestThrottler::new(false);
    //         let msg = Rc::new("MESSAGE".to_string());

    //         throttler.inner.send(msg.clone());

    //         assert_eq!(throttler.inner.qsize(), 0);
    //         assert_eq!(throttler.inner.recv_count, 1);
    //         assert_eq!(throttler.inner.sent_count, 1);
    //     }

    //     #[rstest]
    //     fn test_send_to_limit_drops_message() {
    //         let mut throttler = TestThrottler::new(false);
    //         let msg = Rc::new("MESSAGE".to_string());

    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());
    //         throttler.inner.send(msg.clone());

    //         assert_eq!(throttler.inner.qsize(), 0);
    //         assert_eq!(throttler.inner.recv_count, 6);
    //         assert_eq!(throttler.inner.sent_count, 5);
    //         assert_eq!(round_to_precision(throttler.inner.used()), 1.0);
    //         assert!(throttler.inner.is_limiting);
    //         assert_eq!(throttler.inner.clock.timer_names(), vec!["dropper_timer"]);
    //     }

    //     // #[rstest]
    //     // fn test_advance_time_when_at_limit_dropped_message() {}

    //     // #[rstest]
    //     // fn test_send_message_after_dropping_message() {}
}

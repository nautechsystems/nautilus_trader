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

use std::{cell::RefCell, collections::VecDeque, fmt::Debug, rc::Rc};

use nautilus_core::nanos::UnixNanos;

use super::Throttler;
use crate::{clock::Clock, timer::TimeEventCallback};

/// Throttler rate limits messages by dropping or buffering them.
///
/// Throttler takes messages of type T and callback of type F for dropping
/// or processing messages.
pub struct InnerThrottler<T, F> {
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
    /// The interval between messages in nanoseconds.
    interval: u64,
    /// The name of the timer.
    timer_name: String,
    /// The callback to send a message.
    output_send: F,
    /// The callback to drop a message.
    output_drop: Option<F>,
}

impl<T, F> Debug for InnerThrottler<T, F>
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

impl<T, F> InnerThrottler<T, F> {
    #[inline]
    pub fn new(
        limit: usize,
        interval: u64,
        clock: Rc<RefCell<dyn Clock>>,
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
            clock,
            interval,
            timer_name,
            output_send,
            output_drop,
        }
    }

    /// Set timer with a callback to be triggered on next interval.
    ///
    /// Typically used to register callbacks:
    /// - [`super::callbacks::ThrottlerProcess`] to process buffered messages
    /// - [`super::callbacks::ThrottlerResume`] to stop buffering
    #[inline]
    pub fn set_timer(&mut self, callback: Option<TimeEventCallback>) {
        let delta = self.delta_next();
        let mut clock = self.clock.borrow_mut();
        if clock.timer_names().contains(&self.timer_name.as_str()) {
            clock.cancel_timer(&self.timer_name);
        }
        let alert_ts = clock.timestamp_ns() + delta;

        clock.set_time_alert_ns(&self.timer_name, alert_ts, callback);
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

impl<T, F> InnerThrottler<T, F>
where
    F: Fn(T) + 'static,
    T: 'static,
{
    #[inline]
    pub fn send_msg(&mut self, msg: T) {
        let now = self.clock.borrow().timestamp_ns();

        if self.timestamps.len() >= self.limit {
            self.timestamps.pop_back();
        }
        self.timestamps.push_front(now);

        (self.output_send)(msg);
        self.sent_count += 1;
    }

    #[inline]
    pub fn limit_msg(&mut self, msg: T, throttler: Throttler<T, F>) {
        let callback = if self.output_drop.is_none() {
            self.buffer.push_front(msg);
            log::debug!("Buffering {}", self.buffer.len());
            Some(throttler.get_process_callback().into())
        } else {
            log::debug!("Dropping");
            if let Some(drop) = &self.output_drop {
                drop(msg);
            }
            Some(throttler.get_resume_callback().into())
        };
        if !self.is_limiting {
            log::debug!("Limiting");
            self.set_timer(callback);
            self.is_limiting = true;
        }
    }
}

use std::{cell::RefCell, cmp::max, collections::VecDeque, rc::Rc};

use nautilus_core::nanos::UnixNanos;

use super::Throttler;
use crate::{clock::Clock, timer::TimeEventCallback};

pub struct InnerThrottler<T, F> {
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
    timestamps: VecDeque<UnixNanos>,
    /// Whether the throttler has warmed up.
    warm: bool,
    /// The interval between messages in nanoseconds.
    interval: u64,
    /// The clock used to keep track of time.
    pub clock: Rc<RefCell<dyn Clock>>,
    /// The name of the timer.
    timer_name: String,
    /// The callback to send a message.
    output_send: F,
    /// The callback to drop a message.
    output_drop: Option<F>,
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
            warm: false,
            interval,
            clock,
            timer_name,
            output_send,
            output_drop,
        }
    }

    #[inline]
    pub fn set_timer(&mut self, callback: Option<TimeEventCallback>) {
        let delta = self.delta_next();
        let mut clock = self.clock.borrow_mut();
        // Cancel any existing timer
        if clock.timer_names().contains(&self.timer_name.as_str()) {
            clock.cancel_timer(&self.timer_name);
        }
        // self.clock.cancel_timer(&self.timer_name);
        let alert_ts = clock.timestamp_ns() + delta;

        clock.set_time_alert_ns(&self.timer_name, alert_ts, callback);
    }

    #[inline]
    pub fn delta_next(&mut self) -> u64 {
        if !self.warm && self.sent_count < self.limit {
            return 0;
        }
        self.warm = true;

        let diff = self.clock.borrow().timestamp_ns().as_u64()
            - self
                .timestamps
                .back()
                .unwrap_or_else(|| panic!("Failed to get timestamp"))
                .as_u64();
        self.interval - diff
    }

    #[inline]
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.warm = false;
        self.recv_count = 0;
        self.sent_count = 0;
        self.is_limiting = false;
    }

    #[inline]
    pub fn used(&self) -> f64 {
        if !self.warm && self.sent_count < 2 {
            return 0.0;
        }

        let diff = max(
            0,
            self.interval as i64
                - (self.clock.borrow().timestamp_ns().as_i64()
                    - self.timestamps.back().unwrap().as_i64()),
        );
        let mut used = diff as f64 / self.interval as f64;

        if !self.warm {
            used *= self.sent_count as f64 / self.limit as f64;
        }
        used
    }

    /// Returns the number of messages in the buffer.
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
        let ts = {
            let clock = self.clock.borrow();
            clock.timestamp_ns()
        };
        self.timestamps.push_front(ts);
        (self.output_send)(msg);
        self.sent_count += 1;
    }

    #[inline]
    pub fn limit_msg(&mut self, msg: T, throttler: Throttler<T, F>) {
        // TODO: turn off clippy lint because callback is used
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

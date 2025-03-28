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

//! Provides an implementation of an exponential backoff mechanism with jitter support.
//! It is used for managing reconnection delays in the socket clients.
//!
//! The backoff mechanism allows the delay to grow exponentially up to a configurable
//! maximum, optionally applying random jitter to avoid synchronized reconnection storms.
//! An "immediate first" flag is available so that the very first reconnect attempt
//! can occur without any delay.

use std::time::Duration;

use rand::Rng;

#[derive(Clone, Debug)]
pub struct ExponentialBackoff {
    /// The initial backoff delay.
    delay_initial: Duration,
    /// The maximum delay to cap the backoff.
    delay_max: Duration,
    /// The current backoff delay.
    delay_current: Duration,
    /// The factor to multiply the delay on each iteration.
    factor: f64,
    /// The maximum random jitter to add (in milliseconds).
    jitter_ms: u64,
    /// If true, the first call to `next()` returns zero delay (immediate reconnect).
    immediate_first: bool,
}

/// An exponential backoff mechanism with optional jitter and immediate-first behavior.
///
/// This struct computes successive delays for reconnect attempts.
/// It starts from an initial delay and multiplies it by a factor on each iteration,
/// capping the delay at a maximum value. Random jitter is added (up to a configured
/// maximum) to the delay. When `immediate_first` is true, the first call to `next_duration`
/// returns zero delay, triggering an immediate reconnect, after which the immediate flag is disabled.
impl ExponentialBackoff {
    /// Creates a new [`ExponentialBackoff]` instance.
    #[must_use]
    pub const fn new(
        delay_initial: Duration,
        delay_max: Duration,
        factor: f64,
        jitter_ms: u64,
        immediate_first: bool,
    ) -> Self {
        Self {
            delay_initial,
            delay_max,
            delay_current: delay_initial,
            factor,
            jitter_ms,
            immediate_first,
        }
    }

    /// Return the next backoff delay with jitter and update the internal state.
    ///
    /// If the `immediate_first` flag is set and this is the first call (i.e. the current
    /// delay equals the initial delay), it returns `Duration::ZERO` to trigger an immediate
    /// reconnect and disables the immediate behavior for subsequent calls.
    pub fn next_duration(&mut self) -> Duration {
        if self.immediate_first && self.delay_current == self.delay_initial {
            self.immediate_first = false;
            return Duration::ZERO;
        }

        // Generate random jitter
        let jitter = rand::rng().random_range(0..=self.jitter_ms);
        let delay_with_jitter = self.delay_current + Duration::from_millis(jitter);

        // Prepare the next delay
        let current_nanos = self.delay_current.as_nanos();
        let max_nanos = self.delay_max.as_nanos() as u64;
        let next_nanos = (current_nanos as f64 * self.factor) as u64;
        self.delay_current = Duration::from_nanos(std::cmp::min(next_nanos, max_nanos));

        delay_with_jitter
    }

    /// Reset the backoff to its initial state.
    pub const fn reset(&mut self) {
        self.delay_current = self.delay_initial;
    }

    /// Returns the current base delay without jitter.
    /// This represents the delay that would be used as the base for the next call to `next()`,
    /// before any jitter is applied.
    #[must_use]
    pub const fn current_delay(&self) -> Duration {
        self.delay_current
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_no_jitter_exponential_growth() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(1600);
        let factor = 2.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false);

        // 1st call returns the initial delay
        let d1 = backoff.next_duration();
        assert_eq!(d1, Duration::from_millis(100));

        // 2nd call: current becomes 200ms
        let d2 = backoff.next_duration();
        assert_eq!(d2, Duration::from_millis(200));

        // 3rd call: current becomes 400ms
        let d3 = backoff.next_duration();
        assert_eq!(d3, Duration::from_millis(400));

        // 4th call: current becomes 800ms
        let d4 = backoff.next_duration();
        assert_eq!(d4, Duration::from_millis(800));

        // 5th call: current would be 1600ms (800 * 2) which is within the cap
        let d5 = backoff.next_duration();
        assert_eq!(d5, Duration::from_millis(1600));

        // 6th call: should still be capped at 1600ms
        let d6 = backoff.next_duration();
        assert_eq!(d6, Duration::from_millis(1600));
    }

    #[rstest]
    fn test_reset() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(1600);
        let factor = 2.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false);

        // Call next() once so that the internal state updates
        let _ = backoff.next_duration(); // current_delay becomes 200ms
        backoff.reset();
        let d = backoff.next_duration();
        // After reset, the next delay should be the initial delay (100ms)
        assert_eq!(d, Duration::from_millis(100));
    }

    #[rstest]
    fn test_jitter_within_bounds() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(1000);
        let factor = 2.0;
        let jitter = 50;
        // Run several iterations to ensure that jitter stays within bounds
        for _ in 0..10 {
            let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false);
            // Capture the expected base delay before jitter is applied
            let base = backoff.delay_current;
            let delay = backoff.next_duration();
            // The returned delay must be at least the base delay and at most base + jitter
            let min_expected = base;
            let max_expected = base + Duration::from_millis(jitter);
            assert!(
                delay >= min_expected,
                "Delay {delay:?} is less than expected minimum {min_expected:?}"
            );
            assert!(
                delay <= max_expected,
                "Delay {delay:?} exceeds expected maximum {max_expected:?}"
            );
        }
    }

    #[rstest]
    fn test_factor_less_than_two() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(200);
        let factor = 1.5;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false);

        // First call returns 100ms
        let d1 = backoff.next_duration();
        assert_eq!(d1, Duration::from_millis(100));

        // Second call: current_delay becomes 100 * 1.5 = 150ms
        let d2 = backoff.next_duration();
        assert_eq!(d2, Duration::from_millis(150));

        // Third call: current_delay becomes 150 * 1.5 = 225ms, but capped to 200ms
        let d3 = backoff.next_duration();
        assert_eq!(d3, Duration::from_millis(200));

        // Fourth call: remains at the max of 200ms
        let d4 = backoff.next_duration();
        assert_eq!(d4, Duration::from_millis(200));
    }

    #[rstest]
    fn test_max_delay_is_respected() {
        let initial = Duration::from_millis(500);
        let max = Duration::from_millis(1000);
        let factor = 3.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false);

        // 1st call returns 500ms
        let d1 = backoff.next_duration();
        assert_eq!(d1, Duration::from_millis(500));

        // 2nd call: would be 500 * 3 = 1500ms but is capped to 1000ms
        let d2 = backoff.next_duration();
        assert_eq!(d2, Duration::from_millis(1000));

        // Subsequent calls should continue to return the max delay
        let d3 = backoff.next_duration();
        assert_eq!(d3, Duration::from_millis(1000));
    }

    #[rstest]
    fn test_current_delay_getter() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(1600);
        let factor = 2.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false);

        assert_eq!(backoff.current_delay(), initial);

        let _ = backoff.next_duration();
        assert_eq!(backoff.current_delay(), Duration::from_millis(200));

        let _ = backoff.next_duration();
        assert_eq!(backoff.current_delay(), Duration::from_millis(400));

        backoff.reset();
        assert_eq!(backoff.current_delay(), initial);
    }

    #[rstest]
    fn test_immediate_first() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(1600);
        let factor = 2.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, true);

        // The first call should yield an immediate (zero) delay
        let d1 = backoff.next_duration();
        assert_eq!(
            d1,
            Duration::ZERO,
            "Expected immediate reconnect (zero delay) on first call"
        );

        // The next call should return the current delay (i.e. the base initial delay)
        let d2 = backoff.next_duration();
        assert_eq!(
            d2, initial,
            "Expected the delay to be the initial delay after immediate reconnect"
        );

        // Subsequent calls should continue with the exponential growth
        let d3 = backoff.next_duration();
        let expected = initial * 2; // 100ms * 2 = 200ms
        assert_eq!(
            d3, expected,
            "Expected exponential growth from the initial delay"
        );
    }
}

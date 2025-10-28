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

use nautilus_core::correctness::{check_in_range_inclusive_f64, check_predicate_true};
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
    immediate_reconnect: bool,
    /// The original value of `immediate_reconnect` for reset purposes.
    immediate_reconnect_original: bool,
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
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `delay_initial` is zero.
    /// - `delay_max` is less than `delay_initial`.
    /// - `delay_max` exceeds `Duration::from_nanos(u64::MAX)` (≈584 years).
    /// - `factor` is not in the range [1.0, 100.0] (to prevent reconnect spam).
    pub fn new(
        delay_initial: Duration,
        delay_max: Duration,
        factor: f64,
        jitter_ms: u64,
        immediate_first: bool,
    ) -> anyhow::Result<Self> {
        check_predicate_true(!delay_initial.is_zero(), "delay_initial must be non-zero")?;
        check_predicate_true(
            delay_max >= delay_initial,
            "delay_max must be >= delay_initial",
        )?;
        check_predicate_true(
            delay_max.as_nanos() <= u128::from(u64::MAX),
            "delay_max exceeds maximum representable duration (≈584 years)",
        )?;
        check_in_range_inclusive_f64(factor, 1.0, 100.0, "factor")?;

        Ok(Self {
            delay_initial,
            delay_max,
            delay_current: delay_initial,
            factor,
            jitter_ms,
            immediate_reconnect: immediate_first,
            immediate_reconnect_original: immediate_first,
        })
    }

    /// Return the next backoff delay with jitter and update the internal state.
    ///
    /// If the `immediate_first` flag is set and this is the first call (i.e. the current
    /// delay equals the initial delay), it returns `Duration::ZERO` to trigger an immediate
    /// reconnect and disables the immediate behavior for subsequent calls.
    pub fn next_duration(&mut self) -> Duration {
        if self.immediate_reconnect && self.delay_current == self.delay_initial {
            self.immediate_reconnect = false;
            return Duration::ZERO;
        }

        // Generate random jitter
        let jitter = rand::rng().random_range(0..=self.jitter_ms);
        let delay_with_jitter = self.delay_current + Duration::from_millis(jitter);

        // Prepare the next delay with overflow protection
        // Keep all math in u128 to avoid silent truncation
        let current_nanos = self.delay_current.as_nanos();
        let max_nanos = self.delay_max.as_nanos();

        // Use checked floating point multiplication to prevent overflow
        let next_nanos_u128 = if current_nanos > u128::from(u64::MAX) {
            // Current is already at max representable value, cap to max
            max_nanos
        } else {
            let current_u64 = current_nanos as u64;
            let next_f64 = current_u64 as f64 * self.factor;

            // Check for overflow in the float result
            if next_f64 > u64::MAX as f64 {
                u128::from(u64::MAX)
            } else {
                u128::from(next_f64 as u64)
            }
        };

        let clamped = std::cmp::min(next_nanos_u128, max_nanos);
        let final_nanos = if clamped > u128::from(u64::MAX) {
            u64::MAX
        } else {
            clamped as u64
        };

        self.delay_current = Duration::from_nanos(final_nanos);

        delay_with_jitter
    }

    /// Reset the backoff to its initial state.
    pub const fn reset(&mut self) {
        self.delay_current = self.delay_initial;
        self.immediate_reconnect = self.immediate_reconnect_original;
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
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();

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
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();

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
            let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();
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
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();

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
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();

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
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();

        assert_eq!(backoff.current_delay(), initial);

        let _ = backoff.next_duration();
        assert_eq!(backoff.current_delay(), Duration::from_millis(200));

        let _ = backoff.next_duration();
        assert_eq!(backoff.current_delay(), Duration::from_millis(400));

        backoff.reset();
        assert_eq!(backoff.current_delay(), initial);
    }

    #[rstest]
    fn test_validation_zero_initial_delay() {
        let result =
            ExponentialBackoff::new(Duration::ZERO, Duration::from_millis(1000), 2.0, 0, false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("delay_initial must be non-zero")
        );
    }

    #[rstest]
    fn test_validation_max_less_than_initial() {
        let result = ExponentialBackoff::new(
            Duration::from_millis(1000),
            Duration::from_millis(500),
            2.0,
            0,
            false,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("delay_max must be >= delay_initial")
        );
    }

    #[rstest]
    fn test_validation_factor_too_small() {
        let result = ExponentialBackoff::new(
            Duration::from_millis(100),
            Duration::from_millis(1000),
            0.5,
            0,
            false,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("factor"));
    }

    #[rstest]
    fn test_validation_factor_too_large() {
        let result = ExponentialBackoff::new(
            Duration::from_millis(100),
            Duration::from_millis(1000),
            150.0,
            0,
            false,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("factor"));
    }

    #[rstest]
    fn test_validation_delay_max_exceeds_u64_max_nanos() {
        // Duration::from_nanos(u64::MAX) is approximately 584 years
        // Try to create a backoff with delay_max exceeding this
        let max_valid = Duration::from_nanos(u64::MAX);
        let too_large = max_valid + Duration::from_nanos(1);

        let result = ExponentialBackoff::new(Duration::from_millis(100), too_large, 2.0, 0, false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("delay_max exceeds maximum representable duration")
        );
    }

    #[rstest]
    fn test_immediate_first() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(1600);
        let factor = 2.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, true).unwrap();

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

    #[rstest]
    fn test_reset_restores_immediate_first() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_millis(1600);
        let factor = 2.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, true).unwrap();

        // Use immediate first
        let d1 = backoff.next_duration();
        assert_eq!(d1, Duration::ZERO);

        // Now immediate_first should be disabled
        let d2 = backoff.next_duration();
        assert_eq!(d2, initial);

        // Reset should restore immediate_first
        backoff.reset();
        let d3 = backoff.next_duration();
        assert_eq!(
            d3,
            Duration::ZERO,
            "Reset should restore immediate_first behavior"
        );
    }
}

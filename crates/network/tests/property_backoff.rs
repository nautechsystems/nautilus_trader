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

//! Property-based tests for exponential backoff mechanism.
//!
//! These tests verify mathematical properties and invariants that should hold
//! regardless of specific parameter combinations:
//! - Delays grow exponentially up to maximum
//! - Jitter is always within bounds
//! - Reset behavior is consistent
//! - Immediate-first behavior works correctly

use std::time::Duration;

use nautilus_network::backoff::ExponentialBackoff;
use proptest::prelude::*;
use rstest::rstest;

/// Generate valid backoff parameters.
fn backoff_params_strategy() -> impl Strategy<Value = (Duration, Duration, f64, u64, bool)> {
    (
        1u64..=5000u64,   // initial_ms: 1ms to 5s
        10u64..=60000u64, // max_ms: 10ms to 60s
        1.1f64..=10.0f64, // factor: reasonable exponential growth
        0u64..=1000u64,   // jitter_ms: 0 to 1s
        any::<bool>(),    // immediate_first
    )
        .prop_filter("max >= initial", |(initial_ms, max_ms, _, _, _)| {
            max_ms >= initial_ms
        })
        .prop_map(|(initial_ms, max_ms, factor, jitter_ms, immediate_first)| {
            (
                Duration::from_millis(initial_ms),
                Duration::from_millis(max_ms),
                factor,
                jitter_ms,
                immediate_first,
            )
        })
}

proptest! {
    /// Property: Backoff delays should grow exponentially up to the maximum.
    #[rstest]
    fn backoff_grows_exponentially_to_max(
        (initial, max, factor, jitter_ms, immediate_first) in backoff_params_strategy(),
        iterations in 1usize..=20
    ) {
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter_ms, immediate_first)
            .expect("Valid backoff parameters");

        let mut last_base_delay = Duration::ZERO;
        let mut reached_max = false;

        for i in 0..iterations {
            let base_delay_before = backoff.current_delay();
            let delay = backoff.next_duration();
            let base_delay_after = backoff.current_delay();

            // Handle immediate-first case
            if immediate_first && i == 0 {
                prop_assert_eq!(delay, Duration::ZERO, "First delay should be zero with immediate_first");
                continue;
            }

            // The returned delay should be based on the base delay BEFORE the call to next_duration
            // Delay should include jitter, so it should be >= base delay (before)
            prop_assert!(
                delay >= base_delay_before.saturating_sub(Duration::from_millis(jitter_ms)),
                "Delay {} should be >= base delay before {} minus jitter {}",
                delay.as_millis(),
                base_delay_before.as_millis(),
                jitter_ms
            );

            // Delay should not exceed base delay + jitter
            prop_assert!(
                delay <= base_delay_before + Duration::from_millis(jitter_ms),
                "Delay {} should be <= base delay before {} plus jitter {}",
                delay.as_millis(),
                base_delay_before.as_millis(),
                jitter_ms
            );

            // Base delay should not exceed maximum
            prop_assert!(
                base_delay_after <= max,
                "Base delay after {} should not exceed maximum {}",
                base_delay_after.as_millis(),
                max.as_millis(),
            );

            // If we haven't reached max, delay should grow (unless at max already)
            if !reached_max && last_base_delay > Duration::ZERO {
                let actual_growth = base_delay_after >= last_base_delay;

                prop_assert!(
                    actual_growth,
                    "Base delay should grow: {} -> {} (factor: {})",
                    last_base_delay.as_millis(),
                    base_delay_after.as_millis(),
                    factor
                );
            }

            if base_delay_after == max {
                reached_max = true;
            }

            last_base_delay = base_delay_after;
        }
    }

    /// Property: Jitter should always be within the specified bounds.
    #[rstest]
    fn jitter_within_bounds(
        (initial, max, factor, jitter_ms, immediate_first) in backoff_params_strategy(),
        iterations in 1usize..=50
    ) {
        // Only test with non-zero jitter
        prop_assume!(jitter_ms > 0);

        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter_ms, immediate_first)
            .expect("Valid backoff parameters");

        for i in 0..iterations {
            let delay = backoff.next_duration();
            let base_delay = backoff.current_delay();

            // Skip immediate-first case
            if immediate_first && i == 0 {
                continue;
            }

            // Jitter should be between 0 and jitter_ms
            let actual_jitter = delay.saturating_sub(base_delay);
            prop_assert!(
                actual_jitter <= Duration::from_millis(jitter_ms),
                "Actual jitter {} should not exceed maximum jitter {}",
                actual_jitter.as_millis(),
                jitter_ms
            );
        }
    }

    /// Property: Reset should restore initial state.
    #[rstest]
    fn reset_restores_initial_state(
        (initial, max, factor, jitter_ms, immediate_first) in backoff_params_strategy(),
        advance_iterations in 1usize..=10
    ) {
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter_ms, immediate_first)
            .expect("Valid backoff parameters");

        // Record initial state
        let initial_delay = backoff.current_delay();

        // Advance the backoff state
        for _ in 0..advance_iterations {
            backoff.next_duration();
        }

        // State should have changed when growth beyond the initial delay is
        // actually possible.  This is not the case when the initial delay is
        // already at the maximum because further calls will clamp to the
        // same maximum value.  We therefore only assert a change when
        //  * growth is possible (initial < max), and
        //  * we are not in the special immediate-first + single iteration case.
        if initial < max && !(immediate_first && advance_iterations == 1) {
            prop_assert_ne!(
                backoff.current_delay(),
                initial_delay,
                "Backoff state should have changed after {} iterations",
                advance_iterations
            );
        }

        // Reset and verify initial state is restored
        backoff.reset();
        prop_assert_eq!(
            backoff.current_delay(),
            initial_delay,
            "Current delay should be restored to initial after reset"
        );

        // Verify immediate_first behavior is restored if it was set
        if immediate_first {
            let first_delay_after_reset = backoff.next_duration();
            prop_assert_eq!(
                first_delay_after_reset,
                Duration::ZERO,
                "First delay after reset should be zero with immediate_first"
            );
        }
    }

    /// Property: Immediate-first behavior should work correctly.
    #[rstest]
    fn immediate_first_behavior(
        (initial, max, factor, jitter_ms, _) in backoff_params_strategy(),
        subsequent_calls in 1usize..=5
    ) {
        // Test with immediate_first = true
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter_ms, true)
            .expect("Valid backoff parameters");

        // First call should return zero
        let first_delay = backoff.next_duration();
        prop_assert_eq!(
            first_delay,
            Duration::ZERO,
            "First call should return zero delay with immediate_first"
        );

        // Subsequent calls should return non-zero delays
        for i in 0..subsequent_calls {
            let delay = backoff.next_duration();
            prop_assert!(
                delay >= initial,
                "Subsequent call {} should return delay >= initial ({}ms), was {}ms",
                i + 1,
                initial.as_millis(),
                delay.as_millis()
            );
        }
    }

    /// Property: Backoff should eventually reach and stay at maximum delay.
    #[rstest]
    fn eventually_reaches_maximum(
        (initial, max, factor, jitter_ms, immediate_first) in backoff_params_strategy(),
        excess_iterations in 1usize..=10
    ) {
        // Only test cases where growth is meaningful
        prop_assume!(factor > 1.1);
        prop_assume!(max > initial * 2);

        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter_ms, immediate_first)
            .expect("Valid backoff parameters");

        // Calculate expected iterations to reach max
        let growth_ratio = max.as_millis() as f64 / initial.as_millis() as f64;
        let expected_iterations = growth_ratio.log(factor).ceil() as usize + 5;

        // Run enough iterations to definitely reach max
        for _ in 0..expected_iterations {
            backoff.next_duration();
        }

        // Should have reached maximum
        prop_assert_eq!(
            backoff.current_delay(),
            max,
            "Should reach maximum delay after sufficient iterations"
        );

        // Additional iterations should stay at maximum
        for _ in 0..excess_iterations {
            backoff.next_duration();
            prop_assert_eq!(
                backoff.current_delay(),
                max,
                "Should stay at maximum delay"
            );
        }
    }

    /// Property: Backoff delays should be deterministic for same parameters (ignoring jitter).
    #[rstest]
    fn deterministic_base_progression(
        (initial, max, factor, _jitter_ms, immediate_first) in backoff_params_strategy(),
        iterations in 1usize..=10
    ) {
        // Test without jitter for deterministic behavior
        let mut backoff1 = ExponentialBackoff::new(initial, max, factor, 0, immediate_first)
            .expect("Valid backoff parameters");
        let mut backoff2 = ExponentialBackoff::new(initial, max, factor, 0, immediate_first)
            .expect("Valid backoff parameters");

        for _ in 0..iterations {
            let delay1 = backoff1.next_duration();
            let delay2 = backoff2.next_duration();

            prop_assert_eq!(
                delay1, delay2,
                "Backoff delays should be identical for same parameters without jitter"
            );

            prop_assert_eq!(
                backoff1.current_delay(),
                backoff2.current_delay(),
                "Current delays should be identical for same parameters"
            );
        }
    }

    /// Property: Factor bounds should be respected.
    #[rstest]
    fn factor_bounds_respected(
        initial_ms in 1u64..=1000u64,
        max_ms in 1000u64..=10000u64,
        jitter_ms in 0u64..=100u64,
        immediate_first in any::<bool>()
    ) {
        let initial = Duration::from_millis(initial_ms);
        let max = Duration::from_millis(max_ms);

        // Test boundary cases for factor
        let valid_factors = [1.0, 1.1, 2.0, 10.0, 50.0, 100.0];
        let invalid_factors = [0.0, 0.5, 0.99, 100.1, 150.0];

        for &factor in &valid_factors {
            let result = ExponentialBackoff::new(initial, max, factor, jitter_ms, immediate_first);
            prop_assert!(
                result.is_ok(),
                "Factor {} should be valid",
                factor
            );
        }

        for &factor in &invalid_factors {
            let result = ExponentialBackoff::new(initial, max, factor, jitter_ms, immediate_first);
            prop_assert!(
                result.is_err(),
                "Factor {} should be invalid",
                factor
            );
        }
    }
}

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

//! Exponential backoff with optional jitter for socket reconnection delays.
//!
//! Successive delays grow by a configurable factor up to a maximum. Random jitter reduces
//! synchronized reconnect storms. Immediate‑first mode allows the first reconnect attempt to run
//! without delay. A rolling‑window throttle enforces a minimum attempt spacing once reconnects flap,
//! bounding the attempt rate when the stability reset would otherwise restore an immediate reconnect.

use std::{collections::VecDeque, pin::pin, sync::atomic::AtomicU8, time::Duration};

use nautilus_core::correctness::{check_in_range_inclusive_f64, check_predicate_true};
use rand::RngExt;

use crate::{dst, mode::ConnectionMode};

// Keep public reconnect_max_attempts docs synchronized with this value
pub(crate) const RECONNECT_STABILITY_THRESHOLD: Duration = Duration::from_secs(10);

/// The minimum spacing between reconnect attempts once reconnects flap within a short window.
///
/// One second keeps a single client under the strictest new-connection rate among supported
/// venues (Binance permits 300 connections per 5 minutes per IP; OKX permits 3 per second).
pub(crate) const RECONNECT_MIN_DELAY: Duration = Duration::from_secs(1);

/// The number of reconnect attempts inside [`RECONNECT_MIN_DELAY_WINDOW`] that trips the minimum
/// delay.
pub(crate) const RECONNECT_MIN_DELAY_ATTEMPTS: usize = 3;

/// The rolling window over recent reconnect attempts.
///
/// Long enough to catch a venue cycling connections just past [`RECONNECT_STABILITY_THRESHOLD`],
/// where the stability reset would otherwise restore an immediate reconnect on every cycle.
pub(crate) const RECONNECT_MIN_DELAY_WINDOW: Duration = Duration::from_mins(2);

/// Tracks recent reconnect attempt times to enforce [`RECONNECT_MIN_DELAY`] while reconnects flap.
///
/// The stability reset restores immediate-first reconnection after
/// [`RECONNECT_STABILITY_THRESHOLD`], so the backoff delay alone cannot bound the attempt rate
/// against a venue that keeps replacement connections alive just past that threshold. Once
/// [`RECONNECT_MIN_DELAY_ATTEMPTS`] attempts occur inside [`RECONNECT_MIN_DELAY_WINDOW`], every
/// further attempt waits at least [`RECONNECT_MIN_DELAY`] until fewer than three remain.
#[derive(Debug, Default)]
pub(crate) struct ReconnectThrottle {
    recent_attempts: VecDeque<dst::time::Instant>,
}

impl ReconnectThrottle {
    /// Returns the delay to wait before the next reconnect attempt: `backoff_delay` raised to
    /// [`RECONNECT_MIN_DELAY`] while the rolling window holds at least
    /// [`RECONNECT_MIN_DELAY_ATTEMPTS`] attempts.
    pub(crate) fn gated_delay(&mut self, backoff_delay: Duration) -> Duration {
        self.prune_expired();

        if self.recent_attempts.len() >= RECONNECT_MIN_DELAY_ATTEMPTS {
            backoff_delay.max(RECONNECT_MIN_DELAY)
        } else {
            backoff_delay
        }
    }

    /// Records a reconnect attempt at the current time.
    pub(crate) fn record_attempt(&mut self) {
        self.prune_expired();
        self.recent_attempts.push_back(dst::time::Instant::now());
    }

    fn prune_expired(&mut self) {
        while self
            .recent_attempts
            .front()
            .is_some_and(|oldest| oldest.elapsed() > RECONNECT_MIN_DELAY_WINDOW)
        {
            self.recent_attempts.pop_front();
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExponentialBackoff {
    delay_initial: Duration,
    delay_max: Duration,
    delay_current: Duration,
    factor: f64,
    jitter_ms: u64,
    immediate_reconnect: bool,
    immediate_reconnect_original: bool,
}

/// An exponential backoff mechanism with optional jitter and immediate‑first behavior.
///
/// The backoff starts at an initial delay, multiplies that delay by a factor after each call, and
/// caps it at the configured maximum. Each result includes bounded random jitter. When
/// `immediate_first` is `true`, the first call to [`Self::next_duration`] returns zero. Calling
/// [`Self::reset`] restores both the initial delay and the original immediate‑first setting.
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

    /// Returns the next backoff delay with jitter and updates the internal state.
    ///
    /// If the `immediate_first` flag is set and this is the first call (i.e. the current
    /// delay equals the initial delay), it returns `Duration::ZERO` to trigger an immediate
    /// reconnect and disables the immediate behavior for subsequent calls.
    ///
    /// Near the cap the jittered base is lowered to `delay_max - jitter` so
    /// the spread survives saturation; the result is clamped into
    /// `[min(delay_initial, delay_max), delay_max]`.
    pub fn next_duration(&mut self) -> Duration {
        if self.immediate_reconnect && self.delay_current == self.delay_initial {
            self.immediate_reconnect = false;
            return Duration::ZERO;
        }

        // Generate random jitter
        let jitter = rand::rng().random_range(0..=self.jitter_ms); // dst-ok: transport-layer reconnect jitter, out of DST scope

        // Cap the jittered base below delay_max so the spread survives saturation at the cap
        let base = std::cmp::min(
            self.delay_current,
            self.delay_max
                .saturating_sub(Duration::from_millis(self.jitter_ms)),
        );
        let delay_with_jitter = base + Duration::from_millis(jitter);

        // The floor keeps a jitter range wider than delay_max from producing a zero delay
        let floor = std::cmp::min(self.delay_initial, self.delay_max);
        let clamped_delay = delay_with_jitter.clamp(floor, self.delay_max);

        // The constructor guarantees both values fit in u64 nanoseconds. Float-to-integer casts
        // saturate, so the final min preserves the configured cap even if multiplication overflows.
        let current_nanos = self.delay_current.as_nanos() as u64;
        let max_nanos = self.delay_max.as_nanos() as u64;
        let next_nanos = (current_nanos as f64 * self.factor) as u64;
        self.delay_current = Duration::from_nanos(next_nanos.min(max_nanos));

        clamped_delay
    }

    /// Resets the backoff to its initial state.
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

pub(crate) async fn wait_reconnect_delay(
    duration: Duration,
    connection_mode: &AtomicU8,
    state_notify: &tokio::sync::Notify,
) -> bool {
    if duration.is_zero() {
        return true;
    }

    tokio::select! {
        biased;
        () = dst::time::sleep(duration) => true,
        () = async {
            loop {
                let mut notified = pin!(state_notify.notified());
                notified.as_mut().enable();

                if !ConnectionMode::from_atomic(connection_mode).is_reconnect() {
                    break;
                }
                notified.await;
            }
        } => false,
    }
}

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
        let max = Duration::from_secs(1);
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
        let max = Duration::from_secs(1);
        let factor = 3.0;
        let jitter = 0;
        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();

        // 1st call returns 500ms
        let d1 = backoff.next_duration();
        assert_eq!(d1, Duration::from_millis(500));

        // 2nd call: would be 500 * 3 = 1500ms but is capped to 1000ms
        let d2 = backoff.next_duration();
        assert_eq!(d2, Duration::from_secs(1));

        // Subsequent calls should continue to return the max delay
        let d3 = backoff.next_duration();
        assert_eq!(d3, Duration::from_secs(1));
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
        let result = ExponentialBackoff::new(Duration::ZERO, Duration::from_secs(1), 2.0, 0, false);
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
            Duration::from_secs(1),
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
            Duration::from_secs(1),
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
            Duration::from_secs(1),
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

    #[rstest]
    fn test_jitter_never_exceeds_max_delay() {
        let initial = Duration::from_millis(100);
        let max = Duration::from_secs(1);
        let factor = 2.0;
        let jitter = 500;

        let mut backoff = ExponentialBackoff::new(initial, max, factor, jitter, false).unwrap();

        // Run backoff until it reaches the cap
        while backoff.current_delay() < max {
            backoff.next_duration();
        }

        // Now that we're at the cap, verify jitter doesn't push us over delay_max
        for _ in 0..100 {
            let delay = backoff.next_duration();
            assert!(
                delay <= max,
                "Delay with jitter {delay:?} exceeded max {max:?}"
            );
        }
    }

    #[rstest]
    fn test_jitter_spreads_delays_at_cap() {
        // Regression: clamping after adding jitter collapsed the spread to a
        // single value once the backoff saturated, re-synchronizing clients
        // exactly during extended outages
        let initial = Duration::from_millis(100);
        let max = Duration::from_secs(1);
        let mut backoff = ExponentialBackoff::new(initial, max, 2.0, 500, false).unwrap();

        while backoff.current_delay() < max {
            backoff.next_duration();
        }

        let mut distinct = std::collections::HashSet::new();
        for _ in 0..100 {
            distinct.insert(backoff.next_duration());
        }

        assert!(
            distinct.len() >= 2,
            "Jitter must keep spreading delays once the backoff saturates at the cap"
        );
    }

    #[rstest]
    fn test_jitter_wider_than_max_never_returns_zero_delay() {
        // A jitter range wider than delay_max collapses the base to zero; the
        // floor keeps non-immediate delays positive.
        let max = Duration::from_millis(50);
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(10), max, 2.0, 100, false).unwrap();

        for _ in 0..200 {
            let delay = backoff.next_duration();
            assert!(
                !delay.is_zero(),
                "Non-immediate backoff delay must be positive"
            );
            assert!(delay <= max, "Delay {delay:?} exceeded max {max:?}");
        }
    }

    // Time-dependent throttle tests need an exact paused clock; under the sim build
    // `dst::time` resolves to madsim, whose scheduler epsilon can move an attempt across
    // the window boundary. DST-level coverage comes from the turmoil storm tests.
    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn test_throttle_passes_delay_through_below_attempt_threshold() {
        let mut throttle = ReconnectThrottle::default();

        for _ in 0..RECONNECT_MIN_DELAY_ATTEMPTS {
            assert_eq!(
                throttle.gated_delay(Duration::ZERO),
                Duration::ZERO,
                "Cold window must not floor an immediate reconnect"
            );
            throttle.record_attempt();
        }
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn test_throttle_floors_delay_once_threshold_trips() {
        let mut throttle = ReconnectThrottle::default();

        for _ in 0..RECONNECT_MIN_DELAY_ATTEMPTS {
            throttle.record_attempt();
        }

        assert_eq!(
            throttle.gated_delay(Duration::ZERO),
            RECONNECT_MIN_DELAY,
            "Hot window must floor an immediate reconnect"
        );
        assert_eq!(
            throttle.gated_delay(Duration::from_millis(25)),
            RECONNECT_MIN_DELAY,
            "Hot window must raise a sub-floor backoff delay"
        );
        assert_eq!(
            throttle.gated_delay(Duration::from_secs(5)),
            Duration::from_secs(5),
            "Hot window must not lower a backoff delay above the floor"
        );
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn test_throttle_lifts_floor_after_window_expires() {
        let mut throttle = ReconnectThrottle::default();

        for _ in 0..RECONNECT_MIN_DELAY_ATTEMPTS {
            throttle.record_attempt();
        }

        assert_eq!(throttle.gated_delay(Duration::ZERO), RECONNECT_MIN_DELAY);

        dst::time::sleep(RECONNECT_MIN_DELAY_WINDOW + Duration::from_secs(1)).await;

        assert_eq!(
            throttle.gated_delay(Duration::ZERO),
            Duration::ZERO,
            "Floor must lift once the rolling window drains"
        );
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn test_throttle_window_is_purely_time_based() {
        let mut throttle = ReconnectThrottle::default();

        for _ in 0..RECONNECT_MIN_DELAY_ATTEMPTS {
            throttle.record_attempt();
        }

        // A stable connection can live between flapping attempts; the window must not
        // treat that uptime as recovery. Only time draining the window lifts the floor.
        dst::time::sleep(Duration::from_mins(1)).await;
        assert_eq!(
            throttle.gated_delay(Duration::ZERO),
            RECONNECT_MIN_DELAY,
            "Stable uptime inside the window must not lift the floor"
        );

        dst::time::sleep(RECONNECT_MIN_DELAY_WINDOW).await;
        assert_eq!(
            throttle.gated_delay(Duration::ZERO),
            Duration::ZERO,
            "Floor must lift once attempts drain from the window"
        );
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    mod throttle_proptests {
        use proptest::prelude::*;
        use rstest::rstest;

        use super::*;

        fn build_paused_runtime() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap()
        }

        #[derive(Clone, Debug)]
        enum ThrottleOp {
            Attempt,
            AdvanceMs(u64),
            Gate(u64),
        }

        fn throttle_op_strategy() -> impl Strategy<Value = ThrottleOp> {
            prop_oneof![
                3 => Just(ThrottleOp::Attempt),
                2 => (0u64..=180_000).prop_map(ThrottleOp::AdvanceMs),
                3 => (0u64..=10_000).prop_map(ThrottleOp::Gate),
            ]
        }

        proptest! {
            // Pin regression files to the crate directory
            #![proptest_config(ProptestConfig {
                failure_persistence: Some(Box::new(
                    proptest::test_runner::FileFailurePersistence::Direct(
                        concat!(env!("CARGO_MANIFEST_DIR"), "/proptest-regressions/backoff.txt")
                    )
                )),
                ..ProptestConfig::default()
            })]

            #[rstest]
            fn test_throttle_threshold_boundary(
                attempt_count in 0usize..=8,
                input_ms in 0u64..=5_000,
            ) {
                let runtime = build_paused_runtime();
                runtime.block_on(async move {
                    let mut throttle = ReconnectThrottle::default();

                    for _ in 0..attempt_count {
                        throttle.record_attempt();
                    }

                    let input = Duration::from_millis(input_ms);
                    let expected = if attempt_count >= RECONNECT_MIN_DELAY_ATTEMPTS {
                        input.max(RECONNECT_MIN_DELAY)
                    } else {
                        input
                    };
                    assert_eq!(
                        throttle.gated_delay(input),
                        expected,
                        "Threshold mismatch at {attempt_count} attempts in window"
                    );
                });
            }

            #[rstest]
            fn test_throttle_matches_rolling_window_model(
                ops in prop::collection::vec(throttle_op_strategy(), 1..=500),
            ) {
                let runtime = build_paused_runtime();
                runtime.block_on(async move {
                    let mut throttle = ReconnectThrottle::default();
                    let mut attempt_times_ms: Vec<u64> = Vec::new();
                    let mut now_ms = 0u64;
                    let window_ms = RECONNECT_MIN_DELAY_WINDOW.as_millis() as u64;

                    for op in ops {
                        match op {
                            ThrottleOp::Attempt => {
                                throttle.record_attempt();
                                attempt_times_ms.push(now_ms);
                            }
                            ThrottleOp::AdvanceMs(ms) => {
                                dst::time::sleep(Duration::from_millis(ms)).await;
                                now_ms += ms;
                            }
                            ThrottleOp::Gate(input_ms) => {
                                let input = Duration::from_millis(input_ms);
                                let kept = attempt_times_ms
                                    .iter()
                                    .filter(|t| now_ms - *t <= window_ms)
                                    .count();
                                let expected = if kept >= RECONNECT_MIN_DELAY_ATTEMPTS {
                                    input.max(RECONNECT_MIN_DELAY)
                                } else {
                                    input
                                };
                                let gated = throttle.gated_delay(input);
                                assert_eq!(
                                    gated, expected,
                                    "Model mismatch at {now_ms}ms with {kept} attempts in window"
                                );
                                assert!(
                                    gated >= input,
                                    "Floor must never lower the backoff delay"
                                );
                            }
                        }
                    }
                });
            }
        }
    }
}

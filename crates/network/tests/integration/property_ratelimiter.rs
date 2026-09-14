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

#![allow(
    clippy::cast_possible_truncation,
    reason = "test arithmetic with known-safe values"
)]

//! Property-based tests for rate limiting components.
//!
//! These tests verify fundamental properties that should hold regardless of specific input values:
//! - Rate limiter never allows more requests than quota permits
//! - GCRA algorithm maintains token bucket invariants
//! - Quota calculations respect mathematical bounds
//! - Key isolation works correctly

use std::{num::NonZeroU32, time::Duration};

use nautilus_network::ratelimiter::{RateLimiter, clock::FakeRelativeClock, quota::Quota};
use proptest::prelude::*;
use rstest::rstest;

#[derive(Debug, Clone)]
enum RateLimitOp {
    Check(usize),
    Advance(u64),
}

#[derive(Debug, Clone)]
struct GcraReference {
    now_ns: u128,
    cell_ns: u128,
    burst_ns: u128,
    tat_by_key: Vec<Option<u128>>,
}

impl GcraReference {
    fn new(quota: Quota, keys: usize) -> Self {
        let cell_ns = quota.replenish_interval().as_nanos();
        let burst_ns = cell_ns * u128::from(quota.burst_size().get());
        Self {
            now_ns: 0,
            cell_ns,
            burst_ns,
            tat_by_key: vec![None; keys],
        }
    }

    fn advance(&mut self, millis: u64) {
        self.now_ns += Duration::from_millis(millis).as_nanos();
    }

    fn check(&mut self, key_index: usize) -> bool {
        let tat = self.tat_by_key[key_index].unwrap_or(self.now_ns + self.cell_ns);
        let earliest_time = tat.saturating_sub(self.burst_ns);

        if self.now_ns < earliest_time {
            false
        } else {
            self.tat_by_key[key_index] = Some(tat.max(self.now_ns) + self.cell_ns);
            true
        }
    }
}

fn rate_limit_op_strategy() -> impl Strategy<Value = RateLimitOp> {
    prop_oneof![
        (0usize..5).prop_map(RateLimitOp::Check),
        (0u64..=2_000).prop_map(RateLimitOp::Advance),
    ]
}

proptest! {
    // Pin regression files to the crate directory: the default source-parallel
    // resolution has no `src` component for integration tests and lands at the
    // workspace root instead
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                concat!(env!("CARGO_MANIFEST_DIR"), "/proptest-regressions/ratelimiter.txt")
            )
        )),
        ..ProptestConfig::default()
    })]

    /// Property: exact GCRA decisions match a reference model under deterministic time.
    #[rstest]
    fn rate_limiter_matches_reference_trace(
        rate in 1u32..=20u32,
        key_count in 1usize..=5,
        ops in proptest::collection::vec(rate_limit_op_strategy(), 1..120)
    ) {
        let quota = Quota::per_second(NonZeroU32::new(rate).unwrap()).unwrap();
        let clock = FakeRelativeClock::default();
        let rate_limiter = RateLimiter::new_with_clock(Some(quota), vec![], clock);
        let keys = (0..key_count)
            .map(|index| format!("key-{index}"))
            .collect::<Vec<_>>();
        let mut reference = GcraReference::new(quota, key_count);

        for (step, op) in ops.iter().enumerate() {
            match *op {
                RateLimitOp::Check(raw_key_index) => {
                    let key_index = raw_key_index % key_count;
                    let actual = rate_limiter.check_key(&keys[key_index]).is_ok();
                    let expected = reference.check(key_index);
                    prop_assert_eq!(
                        actual,
                        expected,
                        "GCRA decision mismatch at step {}, op {:?}, rate={}, key_count={}",
                        step,
                        op,
                        rate,
                        key_count
                    );
                }
                RateLimitOp::Advance(millis) => {
                    rate_limiter.advance_clock(Duration::from_millis(millis));
                    reference.advance(millis);
                }
            }
        }
    }

    /// Property: admissions are bounded in every time window, under deterministic
    /// time and the production quota shape (`with_period` + `allow_burst`).
    ///
    /// Implementation-independent: in any window of length m*t (t = replenish
    /// interval) GCRA admits at most burst + m + 1 cells. Unlike the reference-
    /// trace property this cannot be fooled by a misconception shared between
    /// the implementation and a mirrored model.
    #[rstest]
    fn rate_limiter_never_exceeds_window_budget(
        period_ms in 1u64..=1_000,
        burst in 1u32..=10,
        ops in proptest::collection::vec(rate_limit_op_strategy(), 1..200)
    ) {
        let quota = Quota::with_period(Duration::from_millis(period_ms))
            .unwrap()
            .allow_burst(NonZeroU32::new(burst).unwrap());
        let clock = FakeRelativeClock::default();
        let rate_limiter = RateLimiter::new_with_clock(Some(quota), vec![], clock);

        let key = "window".to_string();
        let mut now_ns: u128 = 0;
        let mut admitted_ns: Vec<u128> = Vec::new();

        for op in &ops {
            match *op {
                RateLimitOp::Check(_) => {
                    if rate_limiter.check_key(&key).is_ok() {
                        admitted_ns.push(now_ns);
                    }
                }
                RateLimitOp::Advance(millis) => {
                    rate_limiter.advance_clock(Duration::from_millis(millis));
                    now_ns += Duration::from_millis(millis).as_nanos();
                }
            }
        }

        let t_ns = u128::from(period_ms) * 1_000_000;
        for window_cells in [1u128, u128::from(burst)] {
            let window_ns = window_cells * t_ns;
            let budget = usize::try_from(u128::from(burst) + window_cells + 1).unwrap();

            for (i, start) in admitted_ns.iter().enumerate() {
                let end = start + window_ns;
                let in_window = admitted_ns[i..].iter().take_while(|&&ts| ts < end).count();
                prop_assert!(
                    in_window <= budget,
                    "{} admissions within a {}-cell window (budget {}), period_ms={}, burst={}",
                    in_window,
                    window_cells,
                    budget,
                    period_ms,
                    burst
                );
            }
        }
    }

    /// Property: keyed quotas override the default quota under deterministic time.
    #[rstest]
    fn rate_limiter_keyed_quota_overrides_default_trace(
        default_rate in 1u32..=20u32,
        keyed_rate_offset in 1u32..20u32,
        ops in proptest::collection::vec(rate_limit_op_strategy(), 1..120)
    ) {
        let keyed_rate = 1 + ((default_rate + keyed_rate_offset - 1) % 20);
        let default_quota = Quota::per_second(NonZeroU32::new(default_rate).unwrap()).unwrap();
        let keyed_quota = Quota::per_second(NonZeroU32::new(keyed_rate).unwrap()).unwrap();
        let clock = FakeRelativeClock::default();
        let default_key = "default-key".to_string();
        let keyed_key = "keyed-key".to_string();
        let rate_limiter = RateLimiter::new_with_clock(
            Some(default_quota),
            vec![(keyed_key.clone(), keyed_quota)],
            clock,
        );
        let mut default_reference = GcraReference::new(default_quota, 1);
        let mut keyed_reference = GcraReference::new(keyed_quota, 1);

        for (step, op) in ops.iter().enumerate() {
            match *op {
                RateLimitOp::Check(raw_key_index) => {
                    let (key, reference, rate) = if raw_key_index % 2 == 0 {
                        (&keyed_key, &mut keyed_reference, keyed_rate)
                    } else {
                        (&default_key, &mut default_reference, default_rate)
                    };
                    let actual = rate_limiter.check_key(key).is_ok();
                    let expected = reference.check(0);
                    prop_assert_eq!(
                        actual,
                        expected,
                        "GCRA override mismatch at step {}, op {:?}, rate={}",
                        step,
                        op,
                        rate
                    );
                }
                RateLimitOp::Advance(millis) => {
                    rate_limiter.advance_clock(Duration::from_millis(millis));
                    default_reference.advance(millis);
                    keyed_reference.advance(millis);
                }
            }
        }
    }

    /// Property: Rate limiter should never allow more requests than quota permits initially.
    #[rstest]
    fn rate_limiter_respects_quota_bounds(
        rate in 1u32..=100u32,
        key in "[a-z]{1,10}",
        request_count in 1usize..=200
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero).unwrap();
        let clock = FakeRelativeClock::default();
        let rate_limiter = RateLimiter::new_with_clock(
            None,
            vec![(key.clone(), quota)],
            clock,
        );

        let mut successful_requests = 0;
        let burst_capacity = rate as usize;

        for i in 0..request_count {
            let allowed = rate_limiter.check_key(&key).is_ok();
            if allowed {
                successful_requests += 1;
            }

            prop_assert_eq!(
                allowed,
                i < burst_capacity,
                "Unexpected decision for request {} with burst {}",
                i,
                burst_capacity
            );
        }

        prop_assert_eq!(successful_requests, request_count.min(burst_capacity));
    }

    /// Property: Rate limiter behavior should be consistent across multiple keys.
    #[rstest]
    fn rate_limiter_consistent_across_keys(
        keys in prop::collection::hash_set("[a-z]{3,8}", 2..=10).prop_map(|s| s.into_iter().collect::<Vec<_>>()),
        rate in 1u32..=20u32
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero).unwrap();

        let keyed_quotas: Vec<(String, Quota)> = keys.iter()
            .map(|k| (k.clone(), quota))
            .collect();

        let rate_limiter = RateLimiter::new_with_quota(
            Some(quota),
            keyed_quotas
        );

        // Each key should behave independently - first request should always work
        for key in &keys {
            let allowed = rate_limiter.check_key(key).is_ok();
            prop_assert!(
                allowed,
                "First request for key '{}' should be allowed",
                key
            );
        }

        // Verify keys don't interfere with each other using a fresh limiter (avoid consuming key2)
        if keys.len() > 1 {
            let keyed_quotas2: Vec<(String, Quota)> = keys.iter().map(|k| (k.clone(), quota)).collect();
            let rate_limiter2 = RateLimiter::new_with_quota(Some(quota), keyed_quotas2);

            // Generate load on first key only
            for _ in 0..rate {
                let _ = rate_limiter2.check_key(&keys[0]);
            }

            // The second key's first request should still succeed (unaffected by first key).
            let second_key_fresh = rate_limiter2.check_key(&keys[1]).is_ok();
            prop_assert!(
                second_key_fresh,
                "Second key '{}' should be available and unaffected by '{}'",
                keys[1], keys[0]
            );
        }
    }

    /// Property: Quota calculations should respect mathematical bounds and not overflow.
    #[rstest]
    fn quota_calculations_bounded(
        rate in 1u32..=10000u32
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();

        // Should not panic on quota creation for different periods
        let quota_second = Quota::per_second(rate_nonzero).unwrap();
        let quota_minute = Quota::per_minute(rate_nonzero);
        let quota_hour = Quota::per_hour(rate_nonzero);

        let replenish_second = quota_second.replenish_interval().as_nanos();
        let replenish_minute = quota_minute.replenish_interval().as_nanos();
        let replenish_hour = quota_hour.replenish_interval().as_nanos();

        prop_assert_eq!(replenish_second, 1_000_000_000u128 / u128::from(rate));
        prop_assert_eq!(replenish_minute, 60_000_000_000u128 / u128::from(rate));
        prop_assert_eq!(replenish_hour, 3_600_000_000_000u128 / u128::from(rate));

        // Verify burst capacity equals rate
        prop_assert_eq!(
            quota_second.burst_size().get(),
            rate,
            "Burst capacity should equal rate for per-second quota"
        );

        prop_assert_eq!(
            quota_minute.burst_size().get(),
            rate,
            "Burst capacity should equal rate for per-minute quota"
        );

        prop_assert_eq!(
            quota_hour.burst_size().get(),
            rate,
            "Burst capacity should equal rate for per-hour quota"
        );

        // Verify replenish intervals make sense relative to each other
        prop_assert!(
            replenish_minute >= replenish_second,
            "Per-minute interval {} should be >= per-second interval {}",
            replenish_minute,
            replenish_second
        );

        prop_assert!(
            replenish_hour >= replenish_minute,
            "Per-hour interval {} should be >= per-minute interval {}",
            replenish_hour,
            replenish_minute
        );
    }

    /// Property: Rate limiter should handle rapid sequential requests consistently.
    #[rstest]
    fn rate_limiter_handles_rapid_requests(
        rate in 1u32..=50u32,
        request_count in 1usize..=150
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero).unwrap();
        let clock = FakeRelativeClock::default();
        let rate_limiter = RateLimiter::<String, _>::new_with_clock(
            Some(quota),
            vec![],
            clock,
        );

        let key = "rapid_test".to_string();
        let mut allowed_count = 0;
        let mut denied_count = 0;

        for i in 0..request_count {
            let allowed = rate_limiter.check_key(&key).is_ok();
            if allowed {
                allowed_count += 1;
            } else {
                denied_count += 1;
            }
            prop_assert_eq!(allowed, i < rate as usize);
        }

        let expected_allowed = request_count.min(rate as usize);
        prop_assert_eq!(allowed_count, expected_allowed);
        prop_assert_eq!(denied_count, request_count - expected_allowed);
    }

    /// Property: Default quota should work when no specific key quota is set.
    #[rstest]
    fn default_quota_behavior(
        default_rate in 1u32..=20u32,
        key_rate in 1u32..=20u32,
        key in "[a-z]{1,8}"
    ) {
        let default_quota = Quota::per_second(NonZeroU32::new(default_rate).unwrap()).unwrap();
        let key_quota = Quota::per_second(NonZeroU32::new(key_rate).unwrap()).unwrap();

        let clock = FakeRelativeClock::default();
        let rate_limiter = RateLimiter::new_with_clock(
            Some(default_quota),
            vec![(key.clone(), key_quota)],
            clock,
        );

        let specific_decisions = (0..=key_rate)
            .map(|_| rate_limiter.check_key(&key).is_ok())
            .collect::<Vec<_>>();

        let unknown_key = format!("{key}_unknown");
        let default_decisions = (0..=default_rate)
            .map(|_| rate_limiter.check_key(&unknown_key).is_ok())
            .collect::<Vec<_>>();

        let mut expected_specific = vec![true; key_rate as usize];
        expected_specific.push(false);
        let mut expected_default = vec![true; default_rate as usize];
        expected_default.push(false);
        prop_assert_eq!(specific_decisions, expected_specific);
        prop_assert_eq!(default_decisions, expected_default);
    }

    /// Property: Quota with custom period should work correctly.
    #[rstest]
    fn custom_period_quota_behavior(
        period_ms in 1u64..=5000u64,
        burst_size in 1u32..=10u32
    ) {
        let period = Duration::from_millis(period_ms);
        let burst_nonzero = NonZeroU32::new(burst_size).unwrap();

        let quota = Quota::with_period(period).unwrap().allow_burst(burst_nonzero);
        let clock = FakeRelativeClock::default();
        let rate_limiter = RateLimiter::<String, _>::new_with_clock(Some(quota), vec![], clock);
        let key = "custom_period_test".to_string();
        let decisions = (0..=burst_size)
            .map(|_| rate_limiter.check_key(&key).is_ok())
            .collect::<Vec<_>>();
        let mut expected = vec![true; burst_size as usize];
        expected.push(false);

        prop_assert_eq!(decisions, expected);
        rate_limiter.advance_clock(period);
        prop_assert!(rate_limiter.check_key(&key).is_ok());
        prop_assert!(rate_limiter.check_key(&key).is_err());
        prop_assert_eq!(quota.burst_size().get(), burst_size);
        prop_assert_eq!(quota.replenish_interval(), period);
    }

    /// Property: per_second succeeds for all max_burst <= 1_000_000_000
    /// and always produces a positive replenish interval.
    #[rstest]
    fn per_second_valid_range_invariants(
        max_burst in 1u32..=1_000_000_000u32,
    ) {
        let quota = Quota::per_second(NonZeroU32::new(max_burst).unwrap())
            .expect("max_burst <= 1_000_000_000 should always succeed");
        prop_assert_eq!(
            quota.replenish_interval().as_nanos(),
            1_000_000_000u128 / u128::from(max_burst)
        );
    }

    /// Property: per_minute never panics for any NonZeroU32 value.
    #[rstest]
    fn per_minute_full_range_never_panics(
        max_burst in 1u32..=u32::MAX,
    ) {
        let quota = Quota::per_minute(NonZeroU32::new(max_burst).unwrap());
        prop_assert_eq!(
            quota.replenish_interval().as_nanos(),
            60_000_000_000u128 / u128::from(max_burst)
        );
    }

    /// Property: per_hour never panics for any NonZeroU32 value.
    #[rstest]
    fn per_hour_full_range_never_panics(
        max_burst in 1u32..=u32::MAX,
    ) {
        let quota = Quota::per_hour(NonZeroU32::new(max_burst).unwrap());
        prop_assert_eq!(
            quota.replenish_interval().as_nanos(),
            3_600_000_000_000u128 / u128::from(max_burst)
        );
    }

    /// Property: GCRA boundary edge case where t0 equals earliest_time exactly.
    #[rstest]
    fn gcra_boundary_exact_replenishment(
        rate in 1u32..=20u32
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero).unwrap();
        let clock = FakeRelativeClock::default();
        let rate_limiter = RateLimiter::<String, _>::new_with_clock(Some(quota), vec![], clock);

        let key = "boundary_test".to_string();

        // Consume burst capacity completely
        for _ in 0..rate {
            prop_assert!(rate_limiter.check_key(&key).is_ok());
        }

        prop_assert!(rate_limiter.check_key(&key).is_err());
        rate_limiter.advance_clock(quota.replenish_interval());
        prop_assert!(rate_limiter.check_key(&key).is_ok());
        prop_assert!(rate_limiter.check_key(&key).is_err());
    }
}

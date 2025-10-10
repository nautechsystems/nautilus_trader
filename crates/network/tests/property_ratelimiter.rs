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

//! Property-based tests for rate limiting components.
//!
//! These tests verify fundamental properties that should hold regardless of specific input values:
//! - Rate limiter never allows more requests than quota permits
//! - GCRA algorithm maintains token bucket invariants
//! - Quota calculations respect mathematical bounds
//! - Key isolation works correctly

use std::{num::NonZeroU32, time::Duration};

use nautilus_network::ratelimiter::{RateLimiter, quota::Quota};
use proptest::prelude::*;
use rstest::rstest;

proptest! {
    /// Property: Rate limiter should never allow more requests than quota permits initially.
    #[rstest]
    fn rate_limiter_respects_quota_bounds(
        rate in 1u32..=100u32,
        key in "[a-z]{1,10}",
        request_count in 1usize..=200
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero);
        let rate_limiter = RateLimiter::new_with_quota(
            None,
            vec![(key.clone(), quota)]
        );

        let mut successful_requests = 0;
        let burst_capacity = rate as usize;
        let start = std::time::Instant::now();

        // Make rapid requests without any delay
        for i in 0..request_count {
            let allowed = rate_limiter.check_key(&key).is_ok();
            if allowed {
                successful_requests += 1;
            }

            // Within burst capacity, all requests should be allowed initially
            if i < burst_capacity {
                prop_assert!(allowed, "Request {} should be allowed within burst capacity", i);
            }
        }

        // Account for real time passing during the tight loop.
        // With a real clock, additional tokens may replenish while we iterate.
        let elapsed = start.elapsed();
        let replenish_interval = quota.replenish_interval();
        // Integer division floors the replenished count, which is conservative
        // (we may underestimate tokens replenished during the loop). This still
        // preserves the invariant we care about: we must not exceed what could
        // have been allowed by burst plus replenishment in the elapsed time.
        let replenished = (elapsed.as_nanos() / replenish_interval.as_nanos()) as usize;
        let max_allowed = burst_capacity.saturating_add(replenished);
        let bound = std::cmp::min(request_count, max_allowed);

        // Should not exceed burst + replenished capacity during the loop
        prop_assert!(
            successful_requests <= bound,
            "Successful requests {} exceeded allowed bound {} (burst {} + replenished {} in {:?})",
            successful_requests,
            bound,
            burst_capacity,
            replenished,
            elapsed
        );
    }

    /// Property: Rate limiter behavior should be consistent across multiple keys.
    #[rstest]
    fn rate_limiter_consistent_across_keys(
        keys in prop::collection::hash_set("[a-z]{3,8}", 2..=10).prop_map(|s| s.into_iter().collect::<Vec<_>>()),
        rate in 1u32..=20u32
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero);

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
        let quota_second = Quota::per_second(rate_nonzero);
        let quota_minute = Quota::per_minute(rate_nonzero);
        let quota_hour = Quota::per_hour(rate_nonzero);

        // Verify internal calculations don't overflow
        let replenish_second = quota_second.replenish_interval().as_nanos() as u64;
        let replenish_minute = quota_minute.replenish_interval().as_nanos() as u64;
        let replenish_hour = quota_hour.replenish_interval().as_nanos() as u64;

        prop_assert!(
            replenish_second > 0,
            "Per-second replenish interval should be positive: {}",
            replenish_second
        );

        prop_assert!(
            replenish_minute > 0,
            "Per-minute replenish interval should be positive: {}",
            replenish_minute
        );

        prop_assert!(
            replenish_hour > 0,
            "Per-hour replenish interval should be positive: {}",
            replenish_hour
        );

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
        let quota = Quota::per_second(rate_nonzero);
        let rate_limiter = RateLimiter::<String, _>::new_with_quota(
            Some(quota),
            vec![]
        );

        let key = "rapid_test".to_string();
        let mut allowed_count = 0;
        let mut denied_count = 0;

        // Make rapid sequential requests
        let start = std::time::Instant::now();
        for _ in 0..request_count {
            if rate_limiter.check_key(&key).is_ok() {
                allowed_count += 1;
            } else {
                denied_count += 1;
            }
        }

        // Account for real time passing during the tight loop: tokens may replenish
        let burst_capacity = rate as usize;
        let elapsed = start.elapsed();
        let replenish_interval = quota.replenish_interval();
        // Floors replenished count, which is conservative and safe
        let replenished = (elapsed.as_nanos() / replenish_interval.as_nanos()) as usize;
        let max_allowed = std::cmp::min(request_count, burst_capacity.saturating_add(replenished));

        prop_assert!(
            allowed_count <= max_allowed,
            "Allowed {} exceeded bound {} (burst {} + replenished {} in {:?})",
            allowed_count, max_allowed, burst_capacity, replenished, elapsed
        );

        prop_assert_eq!(
            allowed_count + denied_count,
            request_count,
            "Total requests should equal allowed + denied"
        );
    }

    /// Property: Default quota should work when no specific key quota is set.
    #[rstest]
    fn default_quota_behavior(
        default_rate in 1u32..=20u32,
        key_rate in 1u32..=20u32,
        key in "[a-z]{1,8}"
    ) {
        let default_quota = Quota::per_second(NonZeroU32::new(default_rate).unwrap());
        let key_quota = Quota::per_second(NonZeroU32::new(key_rate).unwrap());

        let rate_limiter = RateLimiter::new_with_quota(
            Some(default_quota),
            vec![(key.clone(), key_quota)]
        );

        // Test specific key uses its quota (time-aware bound)
        let mut specific_allowed = 0usize;
        let specific_attempts = key_rate as usize + 1; // inclusive in original test
        let start_specific = std::time::Instant::now();
        for _ in 0..specific_attempts {
            if rate_limiter.check_key(&key).is_ok() {
                specific_allowed += 1;
            }
        }
        let elapsed_specific = start_specific.elapsed();
        let burst_specific = key_rate as usize;
        let repl_specific = key_quota.replenish_interval();
        let replenished_specific = (elapsed_specific.as_nanos() / repl_specific.as_nanos()) as usize;
        let max_allowed_specific = std::cmp::min(specific_attempts, burst_specific.saturating_add(replenished_specific));
        prop_assert!(
            specific_allowed <= max_allowed_specific,
            "Specific key allowed {} exceeded bound {} (burst {} + replenished {} in {:?})",
            specific_allowed, max_allowed_specific, burst_specific, replenished_specific, elapsed_specific
        );

        // Test unknown key uses default quota (time-aware bound)
        let unknown_key = format!("{key}_unknown");
        let mut default_allowed = 0usize;
        let default_attempts = default_rate as usize + 1; // inclusive in original test
        let start_default = std::time::Instant::now();
        for _ in 0..default_attempts {
            if rate_limiter.check_key(&unknown_key).is_ok() {
                default_allowed += 1;
            }
        }
        let elapsed_default = start_default.elapsed();
        let burst_default = default_rate as usize;
        let repl_default = default_quota.replenish_interval();
        let replenished_default = (elapsed_default.as_nanos() / repl_default.as_nanos()) as usize;
        let max_allowed_default = std::cmp::min(default_attempts, burst_default.saturating_add(replenished_default));
        prop_assert!(
            default_allowed <= max_allowed_default,
            "Unknown key allowed {} exceeded bound {} (burst {} + replenished {} in {:?})",
            default_allowed, max_allowed_default, burst_default, replenished_default, elapsed_default
        );
    }

    /// Property: Quota with custom period should work correctly.
    #[rstest]
    fn custom_period_quota_behavior(
        period_ms in 1u64..=5000u64,
        burst_size in 1u32..=10u32
    ) {
        let period = Duration::from_millis(period_ms);
        let burst_nonzero = NonZeroU32::new(burst_size).unwrap();

        // Test with_period constructor
        if let Some(base_quota) = Quota::with_period(period) {
            let quota = base_quota.allow_burst(burst_nonzero);

            let rate_limiter = RateLimiter::<String, _>::new_with_quota(
                Some(quota),
                vec![]
            );

            let key = "custom_period_test".to_string();

            // Should allow up to burst size immediately (time-aware upper bound)
            let mut allowed = 0usize;
            let attempts = (burst_size * 2) as usize;
            let start = std::time::Instant::now();
            for _ in 0..attempts {
                if rate_limiter.check_key(&key).is_ok() {
                    allowed += 1;
                }
            }
            let elapsed = start.elapsed();
            let burst = burst_size as usize;
            let repl = quota.replenish_interval();
            // Floors replenishment count; conservative and safe
            let replenished = (elapsed.as_nanos() / repl.as_nanos()) as usize;
            let max_allowed = std::cmp::min(attempts, burst.saturating_add(replenished));
            prop_assert!(
                allowed <= max_allowed,
                "Allowed {} exceeded bound {} (burst {} + replenished {} in {:?})",
                allowed, max_allowed, burst, replenished, elapsed
            );

            // Verify quota properties
            prop_assert_eq!(
                quota.burst_size().get(),
                burst_size,
                "Max burst should match configured value"
            );

            prop_assert_eq!(
                quota.replenish_interval().as_nanos() as u64,
                period.as_nanos() as u64,
                "Replenish interval should match configured period"
            );
        }
    }

    /// Property: GCRA boundary edge case where t0 equals earliest_time exactly.
    #[rstest]
    fn gcra_boundary_exact_replenishment(
        rate in 1u32..=20u32
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero);
        let rate_limiter = RateLimiter::<String, _>::new_with_quota(Some(quota), vec![]);

        let key = "boundary_test".to_string();

        // Consume burst capacity completely
        for _ in 0..rate {
            let _ = rate_limiter.check_key(&key);
        }

        // Next request should be denied (rate limited)
        let denied = rate_limiter.check_key(&key).is_err();
        prop_assert!(denied, "Should be rate limited after consuming burst");
    }
}

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

proptest! {
    /// Property: Rate limiter should never allow more requests than quota permits initially.
    #[test]
    fn rate_limiter_respects_quota_bounds(
        rate in 1u32..=100u32,
        key in "[a-z]{1,10}",
        request_count in 1usize..=200
    ) {
        let rate_nonzero = NonZeroU32::new(rate).unwrap();
        let quota = Quota::per_second(rate_nonzero);
        let rate_limiter = RateLimiter::new_with_quota(
            Some(quota),
            vec![(key.clone(), quota)]
        );

        let mut successful_requests = 0;
        let burst_capacity = rate as usize;

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

        // Should not exceed burst capacity without time advancement
        prop_assert!(
            successful_requests <= burst_capacity,
            "Successful requests {} exceeded burst capacity {}",
            successful_requests,
            burst_capacity
        );

        // Should allow exactly the minimum of request_count and burst_capacity
        let expected_successful = std::cmp::min(request_count, burst_capacity);
        prop_assert_eq!(
            successful_requests,
            expected_successful,
            "Should allow exactly min(request_count, burst_capacity)"
        );
    }

    /// Property: Rate limiter behavior should be consistent across multiple keys.
    #[test]
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

        // Verify keys don't interfere with each other
        if keys.len() > 1 && rate > 1 {
            // Exhaust first key by making all its allowed requests
            for _ in 0..rate {
                let _ = rate_limiter.check_key(&keys[0]);
            }

            // First key should now be exhausted
            let first_key_exhausted = rate_limiter.check_key(&keys[0]).is_err();

            // Second key should still have quota available
            let second_key_fresh = rate_limiter.check_key(&keys[1]).is_ok();

            prop_assert!(
                first_key_exhausted,
                "First key '{}' should be exhausted after {} requests",
                keys[0], rate
            );

            prop_assert!(
                second_key_fresh,
                "Second key '{}' should still be available after first key exhaustion",
                keys[1]
            );
        }
    }

    /// Property: Quota calculations should respect mathematical bounds and not overflow.
    #[test]
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
    #[test]
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
        for _ in 0..request_count {
            if rate_limiter.check_key(&key).is_ok() {
                allowed_count += 1;
            } else {
                denied_count += 1;
            }
        }

        // Should allow exactly the burst capacity, deny the rest
        let burst_capacity = rate as usize;
        prop_assert_eq!(
            allowed_count,
            std::cmp::min(request_count, burst_capacity),
            "Allowed count should match burst capacity or request count"
        );

        if request_count > burst_capacity {
            prop_assert!(
                denied_count > 0,
                "Should deny some requests when exceeding burst capacity"
            );

            prop_assert_eq!(
                denied_count,
                request_count - burst_capacity,
                "Denied count should equal excess requests"
            );
        }

        prop_assert_eq!(
            allowed_count + denied_count,
            request_count,
            "Total requests should equal allowed + denied"
        );
    }

    /// Property: Default quota should work when no specific key quota is set.
    #[test]
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

        // Test specific key uses its quota
        let mut specific_allowed = 0;
        for _ in 0..=key_rate {
            if rate_limiter.check_key(&key).is_ok() {
                specific_allowed += 1;
            }
        }

        prop_assert_eq!(
            specific_allowed,
            key_rate as usize,
            "Specific key should use its own quota"
        );

        // Test unknown key uses default quota
        let unknown_key = format!("{key}_unknown");
        let mut default_allowed = 0;
        for _ in 0..=default_rate {
            if rate_limiter.check_key(&unknown_key).is_ok() {
                default_allowed += 1;
            }
        }

        prop_assert_eq!(
            default_allowed,
            default_rate as usize,
            "Unknown key should use default quota"
        );
    }

    /// Property: Quota with custom period should work correctly.
    #[test]
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

            // Should allow up to burst size immediately
            let mut allowed = 0;
            for _ in 0..burst_size * 2 {
                if rate_limiter.check_key(&key).is_ok() {
                    allowed += 1;
                }
            }

            prop_assert_eq!(
                allowed,
                burst_size as usize,
                "Should allow exactly burst size requests"
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
}

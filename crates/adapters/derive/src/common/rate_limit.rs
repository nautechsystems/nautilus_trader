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

//! Rate-limit quotas and keys for the Derive adapter.
//!
//! Derive runs a fixed-window limiter that replenishes the request allowance
//! every five seconds, splitting traffic into matching-engine actions (order
//! create/cancel/replace) and everything else ("non-matching": market-data
//! reads, channel subscriptions, login). Matching limits are per account and
//! tiered (Trader vs Market Maker); the non-matching limit is a flat per-IP
//! allowance. See <https://docs.derive.xyz/reference/rate-limits>.
//!
//! The `nautilus_network` limiter is GCRA-based, so each quota is expressed as a
//! sustained per-second rate with a burst capacity of five seconds' worth of
//! cells. That reproduces Derive's "5x burst, replenished every 5 seconds"
//! model: a full burst of `tps * 5` cells drains, then refills one cell every
//! `1 / tps` seconds (five seconds to refill the whole burst).

use std::num::NonZeroU32;

use nautilus_network::ratelimiter::quota::Quota;

/// Rate-limit key for matching-engine requests (order create/cancel/replace).
pub const DERIVE_MATCHING_RATE_KEY: &str = "derive:matching";

/// Rate-limit key for non-matching requests (reads, subscriptions, login).
pub const DERIVE_NON_MATCHING_RATE_KEY: &str = "derive:non-matching";

/// Default matching-engine allowance for a Trader-tier account (requests per
/// second). Market Maker accounts negotiate higher limits and raise this via
/// [`crate::config::DeriveExecClientConfig::max_matching_requests_per_second`].
pub const DERIVE_DEFAULT_MATCHING_TPS: u32 = 1;

/// Flat non-matching allowance per IP (requests per second).
pub const DERIVE_NON_MATCHING_TPS: u32 = 10;

/// Burst multiplier: Derive permits five seconds' worth of requests in a single
/// burst before the fixed window replenishes.
pub const DERIVE_RATE_BURST_MULTIPLIER: u32 = 5;

/// Builds the matching-engine quota for `max_requests_per_second`, falling back
/// to [`DERIVE_DEFAULT_MATCHING_TPS`] when unset or zero.
#[must_use]
pub fn matching_quota(max_requests_per_second: Option<u32>) -> Quota {
    let tps = max_requests_per_second
        .filter(|&v| v > 0)
        .unwrap_or(DERIVE_DEFAULT_MATCHING_TPS);
    quota_with_burst(tps)
}

/// Builds the flat non-matching quota ([`DERIVE_NON_MATCHING_TPS`]).
#[must_use]
pub fn non_matching_quota() -> Quota {
    quota_with_burst(DERIVE_NON_MATCHING_TPS)
}

fn quota_with_burst(tps: u32) -> Quota {
    let rate = NonZeroU32::new(tps).expect("tps must be non-zero");
    let burst = NonZeroU32::new(tps.saturating_mul(DERIVE_RATE_BURST_MULTIPLIER))
        .expect("burst must be non-zero");
    Quota::per_second(rate)
        .expect("per-second quota replenish interval must be non-zero")
        .allow_burst(burst)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_non_matching_quota_is_ten_per_second_with_five_second_burst() {
        let quota = non_matching_quota();
        assert_eq!(quota.burst_size().get(), 50);
        assert_eq!(quota.replenish_interval(), Duration::from_millis(100));
    }

    #[rstest]
    fn test_matching_quota_defaults_to_trader_tier() {
        let quota = matching_quota(None);
        assert_eq!(quota.burst_size().get(), 5);
        assert_eq!(quota.replenish_interval(), Duration::from_secs(1));
    }

    #[rstest]
    fn test_matching_quota_treats_zero_as_unset() {
        assert_eq!(matching_quota(Some(0)).burst_size().get(), 5);
    }

    #[rstest]
    fn test_matching_quota_honors_market_maker_override() {
        let quota = matching_quota(Some(500));
        assert_eq!(quota.burst_size().get(), 2500);
        assert_eq!(quota.replenish_interval(), Duration::from_millis(2));
    }
}

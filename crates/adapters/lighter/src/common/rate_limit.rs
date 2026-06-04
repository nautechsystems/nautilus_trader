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

//! Rate-limit keys, quotas, and the shared transaction limiter for the Lighter adapter.
//!
//! Lighter meters requests against both the caller IP and the account L1 address.
//! REST reads draw on a per-client read quota; transactions (`sendTx` /
//! `sendTxBatch`) are metered in one venue bucket per account regardless of
//! transport. Single orders are submitted over the WebSocket and batches over
//! HTTP, so both paths share one [`LighterTxRateLimiter`] to keep their combined
//! rate under the single venue transaction limit.

use std::{
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};

use nautilus_network::ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota};
use ustr::Ustr;

/// Conservative Lighter REST rate limit for standard accounts.
///
/// Lighter documents 60 REST requests per rolling minute for standard accounts. Builder and
/// premium accounts can authenticate requests to get higher weighted limits.
pub static LIGHTER_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_minute(NonZeroU32::new(60).expect("non-zero")));

/// Rate-limit bucket key shared by all REST read endpoints.
pub const LIGHTER_REST_BUCKET: &str = "lighter:rest";

/// Rate-limit bucket key for the venue transaction bucket.
///
/// Lighter meters `sendTx` and `sendTxBatch` (HTTP) and the WebSocket `sendTx`
/// path in one per-account bucket. Both transports share a
/// [`LighterTxRateLimiter`] keyed on this so their combined rate stays under the
/// single venue limit.
pub const LIGHTER_TX_BUCKET: &str = "lighter:tx";

/// Per-account transaction rate limiter, shared across the HTTP and WebSocket
/// `sendTx` paths so their combined rate honours the single venue bucket.
pub type LighterTxRateLimiter = RateLimiter<Ustr, MonotonicClock>;

/// Resolves a per-minute override to a quota, falling back to the conservative
/// standard-account quota when unset or zero.
#[must_use]
pub fn resolve_quota(per_min: Option<u32>) -> Quota {
    per_min
        .and_then(NonZeroU32::new)
        .map_or(*LIGHTER_REST_QUOTA, Quota::per_minute)
}

/// Builds the shared transaction limiter from a `sendtx_quota_per_min` override,
/// keyed on [`LIGHTER_TX_BUCKET`]. Unset or zero falls back to the standard
/// 60 req/min.
#[must_use]
pub fn build_tx_rate_limiter(sendtx_per_min: Option<u32>) -> Arc<LighterTxRateLimiter> {
    Arc::new(RateLimiter::new_with_quota(
        None,
        vec![(Ustr::from(LIGHTER_TX_BUCKET), resolve_quota(sendtx_per_min))],
    ))
}

/// Awaits transaction-bucket capacity before a `sendTx` on either transport.
///
/// Paces in the caller's task before the frame is enqueued, so neither the HTTP
/// client nor the WebSocket feed-handler task sleeps mid-loop.
pub async fn await_tx_quota(limiter: &LighterTxRateLimiter) {
    limiter
        .await_keys_ready(Some(&[Ustr::from(LIGHTER_TX_BUCKET)]))
        .await;
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_resolve_quota_defaults_when_unset_or_zero() {
        assert_eq!(resolve_quota(None), *LIGHTER_REST_QUOTA);
        assert_eq!(resolve_quota(Some(0)), *LIGHTER_REST_QUOTA);
    }

    #[rstest]
    fn test_resolve_quota_uses_override() {
        let expected = Quota::per_minute(NonZeroU32::new(24_000).unwrap());
        assert_eq!(resolve_quota(Some(24_000)), expected);
    }

    #[rstest]
    fn test_build_tx_rate_limiter_handles_unset_zero_and_override() {
        // Builds without panicking for unset/zero (NonZeroU32 guard) and keys on
        // the tx bucket: a fresh limiter admits the first transaction.
        let key = Ustr::from(LIGHTER_TX_BUCKET);

        for sendtx in [None, Some(0), Some(4_000)] {
            let limiter = build_tx_rate_limiter(sendtx);
            assert!(limiter.check_key(&key).is_ok());
        }
    }
}

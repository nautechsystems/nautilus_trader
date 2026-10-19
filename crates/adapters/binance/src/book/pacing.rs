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

//! Request-weight pacing for REST depth snapshots.
//!
//! Binance charges each depth snapshot a request weight that grows with its level limit, and
//! bans an IP that keeps exceeding the per-minute weight budget. [`SnapshotPacer`] makes a burst
//! of snapshots, such as every book resyncing after a reconnect, wait for budget instead.

use std::num::NonZeroU32;

use nautilus_network::ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota};

const SNAPSHOT_WEIGHT_KEY: &str = "depth-snapshot";

/// Paces REST depth snapshots against a per-minute request-weight budget.
///
/// Half the budget can be spent at once and half refills over each minute, so no 60-second window
/// admits more than the budget.
#[derive(Debug)]
pub(crate) struct SnapshotPacer {
    limiter: RateLimiter<&'static str, MonotonicClock>,
    burst: u32,
}

impl SnapshotPacer {
    pub(crate) fn new(weight_per_minute: NonZeroU32) -> Self {
        let half = NonZeroU32::new(weight_per_minute.get() / 2).unwrap_or(NonZeroU32::MIN);

        Self {
            limiter: RateLimiter::new_with_quota(
                Some(Quota::per_minute(half).allow_burst(half)),
                Vec::new(),
            ),
            burst: half.get(),
        }
    }

    /// Waits until `weight` fits the budget, then consumes it.
    pub(crate) async fn acquire(&self, weight: u32) {
        // A request heavier than the burst could never be admitted
        let weight = weight.min(self.burst) as usize;
        let keys = vec![SNAPSHOT_WEIGHT_KEY; weight];
        self.limiter.await_keys_ready(Some(&keys)).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[tokio::test]
    async fn acquire_waits_once_the_burst_is_spent() {
        let pacer = SnapshotPacer::new(NonZeroU32::new(600).unwrap());

        let burst = tokio::time::timeout(Duration::from_millis(50), pacer.acquire(300))
            .await
            .is_ok();
        let over_budget = tokio::time::timeout(Duration::from_millis(20), pacer.acquire(1))
            .await
            .is_err();
        let replenished = tokio::time::timeout(Duration::from_secs(1), pacer.acquire(1))
            .await
            .is_ok();

        assert!(burst);
        assert!(over_budget);
        assert!(replenished);
    }

    #[rstest]
    #[tokio::test]
    async fn burst_is_half_the_budget() {
        let pacer = SnapshotPacer::new(NonZeroU32::new(1_200).unwrap());

        let burst = tokio::time::timeout(Duration::from_millis(50), pacer.acquire(600))
            .await
            .is_ok();
        let beyond_burst = tokio::time::timeout(Duration::from_millis(200), pacer.acquire(100))
            .await
            .is_err();

        assert!(burst);
        assert!(beyond_burst);
    }

    #[rstest]
    #[tokio::test]
    async fn acquire_caps_weight_at_the_burst() {
        let pacer = SnapshotPacer::new(NonZeroU32::new(10).unwrap());

        let admitted = tokio::time::timeout(Duration::from_millis(50), pacer.acquire(250))
            .await
            .is_ok();

        assert!(admitted);
    }
}

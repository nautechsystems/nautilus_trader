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

use std::{num::NonZeroU32, sync::Arc, time::Duration};

use nautilus_network::dst::time::{self, Instant};
use parking_lot::Mutex;

/// Paces REST depth snapshots against a per-minute request-weight budget.
///
/// Half the budget can be spent at once and half refills over each minute. Queued requests
/// reserve weight without spending it, so a cooldown cannot accumulate additional send credit.
#[derive(Debug)]
pub(crate) struct SnapshotPacer {
    state: Mutex<SnapshotPacingState>,
    changed: tokio::sync::Notify,
    interval: Duration,
    burst: u32,
}

impl SnapshotPacer {
    pub(crate) fn new(weight_per_minute: NonZeroU32) -> Self {
        let burst = (weight_per_minute.get() / 2).max(1);

        Self {
            state: Mutex::new(SnapshotPacingState {
                next: Instant::now(),
                reserved: 0,
            }),
            changed: tokio::sync::Notify::new(),
            interval: Duration::from_secs(60) / burst,
            burst,
        }
    }

    /// Waits until `weight` can be reserved, capped at the burst so admission remains possible.
    ///
    /// Committing the permit spends weight; dropping an unsent permit releases the reservation.
    pub(crate) async fn acquire(self: &Arc<Self>, weight: u32) -> SnapshotPermit {
        let weight = weight.min(self.burst);

        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();

            let wait = {
                let mut state = self.state.lock();
                let now = Instant::now();

                if weight > self.burst - state.reserved {
                    None
                } else {
                    let next = state.next.max(now) + self.interval * (state.reserved + weight);
                    let ready = now + self.interval * self.burst;

                    if next <= ready {
                        state.reserved += weight;

                        return SnapshotPermit {
                            pacer: self.clone(),
                            weight,
                        };
                    }

                    Some(next - ready)
                }
            };

            match wait {
                Some(wait) => tokio::select! {
                    () = &mut changed => {}
                    () = time::sleep(wait) => {}
                },
                None => changed.await,
            }
        }
    }
}

#[derive(Debug)]
struct SnapshotPacingState {
    next: Instant,
    reserved: u32,
}

#[derive(Debug)]
pub(crate) struct SnapshotPermit {
    pacer: Arc<SnapshotPacer>,
    weight: u32,
}

impl SnapshotPermit {
    pub(crate) fn commit(mut self) {
        self.release(true);
    }

    fn release(&mut self, sent: bool) {
        let weight = std::mem::take(&mut self.weight);
        if weight == 0 {
            return;
        }

        {
            let mut state = self.pacer.state.lock();
            state.reserved -= weight;
            if sent {
                state.next = state.next.max(Instant::now()) + self.pacer.interval * weight;
            }
        }

        self.pacer.changed.notify_waiters();
    }
}

impl Drop for SnapshotPermit {
    fn drop(&mut self) {
        self.release(false);
    }
}

pub(crate) struct SnapshotAdmission {
    pacer: Arc<SnapshotPacer>,
    weight: u32,
    permit: Mutex<Option<SnapshotPermit>>,
}

impl SnapshotAdmission {
    pub(crate) fn new(permit: SnapshotPermit) -> Self {
        Self {
            pacer: permit.pacer.clone(),
            weight: permit.weight,
            permit: Mutex::new(Some(permit)),
        }
    }

    pub(crate) async fn acquire(&self) -> SnapshotPermit {
        let permit = self.permit.lock().take();
        match permit {
            Some(permit) => permit,
            None => self.pacer.acquire(self.weight).await,
        }
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
        let pacer = Arc::new(SnapshotPacer::new(NonZeroU32::new(600).unwrap()));
        let burst = tokio::time::timeout(Duration::from_millis(50), async {
            pacer.acquire(300).await.commit();
        })
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
        let pacer = Arc::new(SnapshotPacer::new(NonZeroU32::new(1_200).unwrap()));
        let burst = tokio::time::timeout(Duration::from_millis(50), async {
            pacer.acquire(600).await.commit();
        })
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
        let pacer = Arc::new(SnapshotPacer::new(NonZeroU32::new(10).unwrap()));

        let admitted = tokio::time::timeout(Duration::from_millis(50), pacer.acquire(250))
            .await
            .is_ok();

        assert!(admitted);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn queued_reservations_do_not_refill_before_transmission() {
        let pacer = Arc::new(SnapshotPacer::new(NonZeroU32::new(3_000).unwrap()));
        let mut permits = Vec::new();
        for _ in 0..6 {
            permits.push(pacer.acquire(250).await);
        }

        let next = pacer.acquire(250);
        tokio::pin!(next);

        tokio::time::advance(Duration::from_secs(21)).await;
        assert!(futures_util::poll!(&mut next).is_pending());

        for permit in permits {
            permit.commit();
        }

        assert!(futures_util::poll!(&mut next).is_pending());
        tokio::time::advance(Duration::from_secs(10)).await;
        let permit = futures_util::poll!(&mut next).map(|permit| permit.commit());

        assert!(permit.is_ready());
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn cancellation_returns_reserved_weight_without_spending_it() {
        let pacer = Arc::new(SnapshotPacer::new(NonZeroU32::new(600).unwrap()));
        let permit = pacer.acquire(300).await;
        let next = pacer.acquire(300);
        tokio::pin!(next);
        assert!(futures_util::poll!(&mut next).is_pending());

        drop(permit);
        let next = futures_util::poll!(&mut next).map(|permit| permit.commit());

        assert!(next.is_ready());
        let next = pacer.acquire(1);
        tokio::pin!(next);
        assert!(futures_util::poll!(&mut next).is_pending());
    }
}

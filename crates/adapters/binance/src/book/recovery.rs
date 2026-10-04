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

//! Recovery tasks that fetch REST depth snapshots for Binance books.
//!
//! Each task supplies a snapshot fetch and error classification to the shared [`BookRecovery`]
//! runner, which owns backoff, the retry budget, and the retry ceiling that follows it. A fetched
//! snapshot completes recovery only when [`BookSyncTracker::accept_snapshot`] bridges it to the
//! buffered diffs. The data client supplies the fetch, its request weight, and the task scope.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nautilus_common::live::dst::time::{self, Duration};
use nautilus_live::{book::recovery::BookRecovery, task::TaskSpawner};
use nautilus_model::identifiers::InstrumentId;

use super::{
    BinanceBookError,
    sync::{BookSyncTracker, DepthSnapshot},
};

/// Spawns a task that fetches snapshots until `tracker` accepts one or the recovery is cancelled.
///
/// Each fetch first draws `weight` from the tracker's snapshot pacer. The task takes the first
/// permit before the runner starts, so waiting behind other books spends neither attempts nor
/// the recovery budget. The snapshot timeout bounds each fetch once paced; zero leaves the fetch
/// to the HTTP client's timeout.
pub(crate) fn spawn_recovery<F, Fut>(
    instrument_id: InstrumentId,
    recovery: Arc<BookRecovery<BinanceBookError>>,
    tracker: BookSyncTracker,
    fetch: F,
    weight: u32,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<DepthSnapshot, BinanceBookError>> + Send + 'static,
{
    let shutdown = tasks.cancellation_token();
    let recovery_guard = recovery.cancellation.clone().drop_guard();

    let task = async move {
        let _recovery_guard = recovery_guard;

        tokio::select! {
            biased;
            () = shutdown.cancelled() => return,
            () = recovery.cancellation.cancelled() => return,
            () = tracker.pacer().acquire(weight) => {}
        }

        let prepaid = AtomicBool::new(true);

        tokio::select! {
            biased;
            () = shutdown.cancelled() => {}
            () = recovery.run(
                instrument_id,
                snapshot_timeout,
                |_, gate| {
                    let prepaid = prepaid.swap(false, Ordering::Relaxed);
                    let snapshot = fetch_with_deadline(fetch(), snapshot_timeout);
                    let tracker = &tracker;
                    let recovery = &recovery;

                    async move {
                        if !prepaid {
                            tracker.pacer().acquire(weight).await;
                        }

                        snapshot
                            .await
                            .and_then(|snapshot| {
                                tracker.accept_snapshot(instrument_id, recovery, &gate, snapshot)
                            })
                            .inspect_err(|e| {
                                log::debug!("Book snapshot attempt for {instrument_id} failed: {e}");
                            })
                    }
                },
                BinanceBookError::is_retryable,
                BinanceBookError::Permanent,
                || BinanceBookError::Retryable("book snapshot deadline expired".to_string()),
            ) => {}
        }
    };

    if let Err(e) = tasks.spawn(task) {
        log::warn!("Skipping book recovery for {instrument_id} after shutdown began: {e}");
    }
}

async fn fetch_with_deadline(
    fetch: impl Future<Output = Result<DepthSnapshot, BinanceBookError>>,
    snapshot_timeout: Duration,
) -> Result<DepthSnapshot, BinanceBookError> {
    if snapshot_timeout.is_zero() {
        fetch.await
    } else {
        time::timeout(snapshot_timeout, fetch)
            .await
            .unwrap_or_else(|_| {
                Err(BinanceBookError::Retryable(format!(
                    "depth snapshot request exceeded {snapshot_timeout:?} deadline"
                )))
            })
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, sync::atomic::AtomicUsize};

    use nautilus_common::{
        live::sender::EventSender, messages::DataEvent, testing::wait_until_async,
    };
    use nautilus_core::UnixNanos;
    use nautilus_live::task::TaskGroup;
    use nautilus_model::data::{Data, OrderBookDelta, OrderBookDeltas};
    use rstest::rstest;

    use super::*;
    use crate::book::{
        pacing::SnapshotPacer,
        sync::{DepthSequencing, DepthUpdate},
    };

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTCUSDT.BINANCE")
    }

    fn snapshot(last_update_id: u64) -> DepthSnapshot {
        let clear = OrderBookDelta::clear(
            instrument_id(),
            last_update_id,
            UnixNanos::from(1_u64),
            UnixNanos::from(2_u64),
        );

        DepthSnapshot {
            last_update_id,
            deltas: OrderBookDeltas::new(instrument_id(), vec![clear]),
            has_event_time: true,
        }
    }

    fn update(update_id: u64) -> DepthUpdate {
        DepthUpdate {
            first_update_id: update_id,
            final_update_id: update_id,
            prev_final_update_id: None,
            deltas: None,
        }
    }

    fn setup(
        weight_per_minute: u32,
    ) -> (
        BookSyncTracker,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        TaskGroup,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pacer = SnapshotPacer::new(NonZeroU32::new(weight_per_minute).unwrap());

        let tracker = BookSyncTracker::new(
            DepthSequencing::Spot,
            EventSender::from(tx),
            Arc::new(pacer),
        );
        tracker.subscribe(instrument_id());
        (tracker, rx, TaskGroup::new())
    }

    #[rstest]
    #[tokio::test]
    async fn fetch_with_deadline_zero_waits_for_slow_fetch() {
        let fetch = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(snapshot(7))
        };

        let result = fetch_with_deadline(fetch, Duration::ZERO).await;

        assert_eq!(result.unwrap().last_update_id, 7);
    }

    #[rstest]
    #[tokio::test]
    async fn fetch_with_deadline_expires_pending_fetch() {
        let fetch = std::future::pending::<Result<DepthSnapshot, BinanceBookError>>();

        let result = fetch_with_deadline(fetch, Duration::from_millis(20)).await;

        assert_eq!(
            result.unwrap_err(),
            BinanceBookError::Retryable(
                "depth snapshot request exceeded 20ms deadline".to_string()
            )
        );
    }

    #[rstest]
    #[tokio::test]
    async fn recovery_retries_until_snapshot_bridges() {
        let (tracker, mut rx, tasks) = setup(60_000);
        let recovery = tracker.handle_update(instrument_id(), update(101)).unwrap();
        let fetches = Arc::new(AtomicUsize::new(0));
        let fetch_count = Arc::clone(&fetches);

        spawn_recovery(
            instrument_id(),
            Arc::clone(&recovery),
            tracker.clone(),
            move || {
                let attempt = fetch_count.fetch_add(1, Ordering::SeqCst);
                async move { Ok(snapshot(if attempt == 0 { 99 } else { 100 })) }
            },
            5,
            Duration::from_secs(1),
            &tasks.spawner().unwrap(),
        );

        wait_until_async(
            || {
                let accepted = recovery.is_accepted();
                async move { accepted }
            },
            Duration::from_secs(5),
        )
        .await;

        let event = rx.try_recv().unwrap();

        let DataEvent::Data(Data::BookDeltas(deltas)) = event else {
            panic!("expected order book deltas");
        };

        assert_eq!(deltas.sequence, 100);
        assert!(rx.try_recv().is_err());
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
    }

    // A permanent error skips the retry budget but keeps the book owned for ceiling retries
    #[rstest]
    #[tokio::test]
    async fn permanent_failure_keeps_book_owned() {
        let (tracker, mut rx, tasks) = setup(60_000);
        let recovery = tracker.handle_update(instrument_id(), update(101)).unwrap();
        let fetches = Arc::new(AtomicUsize::new(0));
        let fetch_count = Arc::clone(&fetches);

        spawn_recovery(
            instrument_id(),
            Arc::clone(&recovery),
            tracker.clone(),
            move || {
                fetch_count.fetch_add(1, Ordering::SeqCst);
                async { Err(BinanceBookError::Permanent("invalid symbol".to_string())) }
            },
            5,
            Duration::from_secs(1),
            &tasks.spawner().unwrap(),
        );

        wait_until_async(
            || {
                let fetched = fetches.load(Ordering::SeqCst) == 1;
                async move { fetched }
            },
            Duration::from_secs(5),
        )
        .await;

        tokio::time::sleep(Duration::from_millis(100)).await;

        let buffered = tracker.handle_update(instrument_id(), update(102));

        assert!(recovery.is_running());
        assert!(buffered.is_none());
        assert!(rx.try_recv().is_err());
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    #[tokio::test]
    async fn queued_recovery_waits_for_weight_without_expiring_its_deadline() {
        let (tracker, mut rx, tasks) = setup(600);
        tracker.pacer().acquire(600).await;
        let recovery = tracker.handle_update(instrument_id(), update(101)).unwrap();
        let fetches = Arc::new(AtomicUsize::new(0));
        let fetch_count = Arc::clone(&fetches);
        let started = std::time::Instant::now();

        spawn_recovery(
            instrument_id(),
            Arc::clone(&recovery),
            tracker.clone(),
            move || {
                fetch_count.fetch_add(1, Ordering::SeqCst);
                async { Ok(snapshot(100)) }
            },
            1,
            Duration::from_millis(20),
            &tasks.spawner().unwrap(),
        );

        wait_until_async(
            || {
                let accepted = recovery.is_accepted();
                async move { accepted }
            },
            Duration::from_secs(5),
        )
        .await;

        assert!(started.elapsed() >= Duration::from_millis(80));
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        assert!(matches!(
            rx.try_recv(),
            Ok(DataEvent::Data(Data::BookDeltas(_)))
        ));
    }

    #[rstest]
    #[tokio::test]
    async fn recovery_spanning_reconnect_completes_before_new_diffs() {
        let (tracker, mut rx, tasks) = setup(600);
        tracker.pacer().acquire(600).await;
        let recovery = tracker.handle_update(instrument_id(), update(101)).unwrap();
        let fetches = Arc::new(AtomicUsize::new(0));
        let fetch_count = Arc::clone(&fetches);

        spawn_recovery(
            instrument_id(),
            Arc::clone(&recovery),
            tracker.clone(),
            move || {
                fetch_count.fetch_add(1, Ordering::SeqCst);
                async { Ok(snapshot(100)) }
            },
            1,
            Duration::from_secs(1),
            &tasks.spawner().unwrap(),
        );

        // The reconnect lands while the recovery waits for weight, and the quiet book sends no
        // diff on the new stream
        tracker.reset_on_reconnect();

        wait_until_async(
            || {
                let accepted = recovery.is_accepted();
                async move { accepted }
            },
            Duration::from_secs(5),
        )
        .await;

        let event = rx.try_recv().unwrap();

        let DataEvent::Data(Data::BookDeltas(deltas)) = event else {
            panic!("expected order book deltas");
        };

        assert_eq!(deltas.sequence, 100);
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn unsubscribe_cancels_recovery_queued_for_weight() {
        let (tracker, mut rx, tasks) = setup(60);
        tracker.pacer().acquire(60).await;
        let recovery = tracker.handle_update(instrument_id(), update(101)).unwrap();
        let fetches = Arc::new(AtomicUsize::new(0));
        let fetch_count = Arc::clone(&fetches);

        spawn_recovery(
            instrument_id(),
            Arc::clone(&recovery),
            tracker.clone(),
            move || {
                fetch_count.fetch_add(1, Ordering::SeqCst);
                async { Ok(snapshot(100)) }
            },
            30,
            Duration::from_secs(1),
            &tasks.spawner().unwrap(),
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
        tracker.remove(instrument_id());
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(recovery.cancellation.is_cancelled());
        assert_eq!(fetches.load(Ordering::SeqCst), 0);
        assert!(rx.try_recv().is_err());
    }
}

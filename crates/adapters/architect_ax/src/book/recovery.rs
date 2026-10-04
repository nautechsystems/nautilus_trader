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

//! Subscription and recovery tasks for AX Exchange books.
//!
//! The tasks here write gated book subscriptions, monitor snapshot deadlines, and supply AX
//! replacement operations and error classification to the shared [`BookRecovery`] runner. The
//! runner owns snapshot waits, backoff, the retry budget, and the retry ceiling that follows it.
//! A successful write alone does not complete recovery: the tracker must accept a snapshot.
//!
//! Tasks claim ownership through [`BookSyncTracker`], which keeps shared state transitions under
//! its lock. Cancellation stops obsolete work after unsubscribe, replacement, or shutdown. The data
//! client supplies the WebSocket client and task scope.

use std::sync::Arc;

use nautilus_common::live::dst::time::{self, Duration, Instant};
use nautilus_live::{
    book::{
        recovery::BookRecovery as Recovery,
        snapshot::{SnapshotGate, snapshot_expired},
    },
    task::TaskSpawner,
};
use nautilus_model::identifiers::InstrumentId;

use super::sync::BookSyncTracker;
use crate::{
    common::enums::AxMarketDataLevel,
    websocket::data::{AxMdWebSocketClient, AxWsClientError, subscription::SubscriptionTurn},
};

pub(crate) type BookRecovery = Recovery<AxWsClientError>;

/// Subscribes an order book delta book and recovers it when its initial snapshot is missing.
///
/// The write waits for `turn`, so it follows every earlier subscription change. The book
/// suppresses frames until the subscription write completes, and the task claims recovery when
/// the write fails or no snapshot arrives within `snapshot_timeout`.
pub(crate) fn spawn_subscription_task(
    instrument_id: InstrumentId,
    level: AxMarketDataLevel,
    mut turn: SubscriptionTurn,
    book_sync: BookSyncTracker,
    ws_client: AxMdWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let gate = SnapshotGate::default();
    gate.lock().close();
    let cancel = book_sync.record_subscription(instrument_id, Instant::now(), gate.clone());
    let guard = cancel.clone().drop_guard();
    let tracker = book_sync.clone();
    let spawner = tasks.clone();

    if let Err(e) = tasks.spawn(async move {
        let _guard = guard;
        turn.wait().await;
        let result = ws_client
            .subscribe_book_deltas_gated(instrument_id.symbol.inner(), level, cancel.clone(), gate)
            .await;

        // Later subscription changes wait for the write, not for the snapshot
        drop(turn);

        if cancel.is_cancelled() {
            return;
        }

        match result {
            Ok(()) => {
                if !snapshot_expired(&cancel, snapshot_timeout).await {
                    return;
                }

                log::warn!(
                    "Initial book snapshot missing for {instrument_id}; requesting a fresh snapshot"
                );
            }
            Err(e) if is_retryable_error(&e) => {
                log::warn!("Initial book subscription write failed for {instrument_id}: {e}");
            }
            Err(e) => {
                // Nothing reached the handler, so recovery would have no stream to replace
                book_sync.remove_subscription(instrument_id, &cancel);
                log::warn!("Book subscription rejected for {instrument_id}: {e}");
                return;
            }
        }

        if let Some(recovery) = book_sync.claim_subscription_recovery(instrument_id, &cancel) {
            spawn_recovery_task(
                instrument_id,
                recovery,
                ws_client,
                snapshot_timeout,
                &spawner,
            );
        }
    }) {
        // A book whose write never runs would suppress every frame
        tracker.remove(instrument_id);
        log::debug!("Skipping AX book subscription after shutdown began: {e}");
    }
}

/// Restarts every book after a reconnect, arming snapshot deadlines that a one-shot monitor
/// enforces.
///
/// A zero `snapshot_timeout` arms nothing, so reconnected books wait for the replayed
/// subscriptions to deliver snapshots.
pub(crate) fn reset_on_reconnect(
    book_sync: &BookSyncTracker,
    ws_client: &AxMdWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    book_sync.reset_on_reconnect();

    if snapshot_timeout.is_zero() {
        return;
    }

    if book_sync.seed_pending_snapshots(snapshot_timeout, Instant::now()) > 0 {
        spawn_recovery_monitor(
            book_sync.clone(),
            ws_client.clone(),
            snapshot_timeout,
            tasks,
        );
    }
}

// Recovers every book whose armed snapshot deadline expires, once
fn spawn_recovery_monitor(
    book_sync: BookSyncTracker,
    ws_client: AxMdWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let task_cancel = tasks.cancellation_token();
    let spawner = tasks.clone();

    if let Err(e) = tasks.spawn(async move {
        tokio::select! {
            biased;
            () = task_cancel.cancelled() => {}
            () = time::sleep(snapshot_timeout) => {
                for signal in book_sync.take_expired_snapshots(Instant::now()) {
                    signal.log();

                    if let Some(recovery) = book_sync.claim_recovery(signal.instrument_id) {
                        spawn_recovery_task(
                            signal.instrument_id,
                            recovery,
                            ws_client.clone(),
                            snapshot_timeout,
                            &spawner,
                        );
                    }
                }
            }
        }
    }) {
        log::debug!("Skipping AX book recovery monitor after shutdown began: {e}");
    }
}

/// Starts recovery for a book whose frame could not be converted, or fails the running attempt
/// without waiting for its snapshot deadline.
pub(crate) fn reject_snapshot(
    instrument_id: InstrumentId,
    book_sync: &BookSyncTracker,
    ws_client: &AxMdWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let error = AxWsClientError::InvalidSnapshot(format!("book frame for {instrument_id}"));

    let Some(recovery) = book_sync.reject_snapshot(instrument_id, error) else {
        return;
    };

    spawn_recovery_task(
        instrument_id,
        recovery,
        ws_client.clone(),
        snapshot_timeout,
        tasks,
    );
}

fn spawn_recovery_task(
    instrument_id: InstrumentId,
    recovery: Arc<BookRecovery>,
    ws_client: AxMdWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();
    let recovery_guard = recovery.cancellation.clone().drop_guard();
    let symbol = instrument_id.symbol.inner();
    let timeout_ms = u64::try_from(snapshot_timeout.as_millis()).unwrap_or(u64::MAX);

    if let Err(e) = tasks.spawn(async move {
        let _recovery_guard = recovery_guard;

        tokio::select! {
            biased;
            () = shutdown.cancelled() => recovery.cancellation.cancel(),
            () = recovery.run(
                instrument_id,
                snapshot_timeout,
                |attempt_cancel, gate| ws_client.resubscribe_book_gated(symbol, attempt_cancel, gate),
                is_retryable_error,
                AxWsClientError::ClientError,
                || AxWsClientError::OperationTimeout { timeout_ms },
            ) => {}
        }
    }) {
        log::debug!("Skipping AX book recovery task after shutdown began: {e}");
    }
}

fn is_retryable_error(error: &AxWsClientError) -> bool {
    matches!(
        error,
        AxWsClientError::Transport(_)
            | AxWsClientError::OperationTimeout { .. }
            | AxWsClientError::InvalidSnapshot(_)
    )
}

#[cfg(test)]
mod tests {
    use nautilus_common::testing::wait_until_async;
    use nautilus_live::task::TaskGroup;
    use nautilus_network::websocket::TransportBackend;
    use rstest::rstest;

    use super::*;
    use crate::websocket::data::subscription::SubscriptionOrder;

    #[rstest]
    #[tokio::test]
    async fn rejected_subscription_starts_no_recovery() {
        let book_sync = BookSyncTracker::default();
        let tasks = TaskGroup::new();
        let spawner = tasks.spawner().expect("open task group");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");

        // An unconnected client has no handler, so the subscribe fails before any write
        spawn_subscription_task(
            instrument_id,
            AxMarketDataLevel::Level2,
            SubscriptionOrder::default().next(),
            book_sync.clone(),
            unconnected_client(),
            Duration::from_secs(10),
            &spawner,
        );

        // A recovery task would keep running at its retry ceiling
        wait_until_async(
            || async { tasks.all_finished() },
            std::time::Duration::from_secs(5),
        )
        .await;
        book_sync.reset_on_reconnect();
        let armed = book_sync.seed_pending_snapshots(Duration::from_secs(10), Instant::now());

        assert_eq!(armed, 0, "the rejected book must not stay tracked");
    }

    #[rstest]
    #[tokio::test]
    async fn zero_snapshot_timeout_arms_no_deadline_after_reconnect() {
        let book_sync = BookSyncTracker::default();
        let tasks = TaskGroup::new();
        let spawner = tasks.spawner().expect("open task group");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
        let now = Instant::now();
        book_sync.record_subscription(instrument_id, now, SnapshotGate::default());
        assert!(book_sync.record_snapshot(instrument_id, now));

        reset_on_reconnect(&book_sync, &unconnected_client(), Duration::ZERO, &spawner);
        let recovery = book_sync.claim_recovery(instrument_id);

        assert!(tasks.all_finished(), "no deadline monitor starts");
        assert!(
            recovery.is_some_and(|recovery| recovery.is_running()),
            "no deadline owns the reconnected book"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn refused_recovery_task_releases_book() {
        let book_sync = BookSyncTracker::default();
        let tasks = TaskGroup::new();
        let spawner = tasks.spawner().expect("open task group");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
        let now = Instant::now();
        book_sync.record_subscription(instrument_id, now, SnapshotGate::default());
        assert!(book_sync.record_snapshot(instrument_id, now));
        tasks.begin_shutdown();

        reject_snapshot(
            instrument_id,
            &book_sync,
            &unconnected_client(),
            Duration::from_secs(10),
            &spawner,
        );
        let reclaimed = book_sync.claim_recovery(instrument_id);

        assert!(
            reclaimed.is_some_and(|recovery| recovery.is_running()),
            "a refused task must not leave its episode owning the book"
        );
    }

    #[rstest]
    #[case::transport(AxWsClientError::Transport("e".into()), true)]
    #[case::operation_timeout(AxWsClientError::OperationTimeout { timeout_ms: 10 }, true)]
    #[case::invalid_snapshot(AxWsClientError::InvalidSnapshot("e".into()), true)]
    #[case::channel(AxWsClientError::ChannelError("e".into()), false)]
    #[case::client(AxWsClientError::ClientError("e".into()), false)]
    fn recovery_retry_classification(#[case] error: AxWsClientError, #[case] expected: bool) {
        assert_eq!(is_retryable_error(&error), expected);
    }

    fn unconnected_client() -> AxMdWebSocketClient {
        AxMdWebSocketClient::new(
            "ws://unconnected-book-client.test/md/ws".to_string(),
            "test_token".to_string(),
            30,
            TransportBackend::default(),
            None,
        )
    }
}

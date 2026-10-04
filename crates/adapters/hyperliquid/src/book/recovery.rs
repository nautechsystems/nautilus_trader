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

//! Subscription and recovery tasks for Hyperliquid books.
//!
//! The tasks here write gated `l2Book` subscriptions, monitor snapshot deadlines, and supply
//! Hyperliquid replacement operations and error classification to the shared [`BookRecovery`]
//! runner. The runner owns snapshot waits, backoff, the retry budget, and the retry ceiling that
//! follows it. A successful write alone does not complete recovery: the tracker must accept a
//! snapshot.
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
use crate::websocket::{client::HyperliquidWebSocketClient, error::HyperliquidWsError};

pub(crate) type BookRecovery = Recovery<HyperliquidWsError>;

/// Subscribes an order book delta book and recovers it when its initial snapshot is missing.
///
/// The book suppresses frames until the subscription write completes, and the task claims
/// recovery when the write fails or no snapshot arrives within `snapshot_timeout`.
#[allow(
    clippy::too_many_arguments,
    reason = "subscription needs shared adapter state"
)]
pub(crate) fn spawn_subscription_task(
    instrument_id: InstrumentId,
    n_sig_figs: Option<u32>,
    mantissa: Option<u32>,
    book_sync: BookSyncTracker,
    ws_client: HyperliquidWebSocketClient,
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
        let result = ws_client
            .subscribe_book_gated(instrument_id, n_sig_figs, mantissa, cancel.clone(), gate)
            .await;

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
                // Nothing reached the venue, so recovery would have no stream to replace
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
        log::debug!("Skipping Hyperliquid book subscription after shutdown began: {e}");
    }
}

/// Restarts every book after a reconnect, arming snapshot deadlines that a one-shot monitor
/// enforces.
///
/// A zero `snapshot_timeout` arms nothing, so reconnected books wait for the replayed
/// subscriptions to deliver snapshots.
pub(crate) fn reset_on_reconnect(
    book_sync: &BookSyncTracker,
    ws_client: &HyperliquidWebSocketClient,
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
    ws_client: HyperliquidWebSocketClient,
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
                    start_recovery(
                        signal.instrument_id,
                        &book_sync,
                        &ws_client,
                        snapshot_timeout,
                        &spawner,
                    );
                }
            }
        }
    }) {
        log::debug!("Skipping Hyperliquid book recovery monitor after shutdown began: {e}");
    }
}

/// Starts recovery for `instrument_id` unless another owner holds the book.
pub(crate) fn start_recovery(
    instrument_id: InstrumentId,
    book_sync: &BookSyncTracker,
    ws_client: &HyperliquidWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let Some(recovery) = book_sync.claim_recovery(instrument_id) else {
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

/// Starts recovery for a book whose frame could not be converted, or fails the running attempt
/// without waiting for its snapshot deadline.
pub(crate) fn reject_snapshot(
    instrument_id: InstrumentId,
    book_sync: &BookSyncTracker,
    ws_client: &HyperliquidWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let error = HyperliquidWsError::InvalidSnapshot(format!("l2Book frame for {instrument_id}"));

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
    ws_client: HyperliquidWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();
    let recovery_guard = recovery.cancellation.clone().drop_guard();

    if let Err(e) = tasks.spawn(async move {
        let _recovery_guard = recovery_guard;

        tokio::select! {
            biased;
            () = shutdown.cancelled() => recovery.cancellation.cancel(),
            () = recovery.run(
                instrument_id,
                snapshot_timeout,
                |attempt_cancel, gate| ws_client.resubscribe_book_gated(instrument_id, attempt_cancel, gate),
                is_retryable_error,
                HyperliquidWsError::ClientError,
                || HyperliquidWsError::OperationTimeout {
                    timeout_ms: snapshot_timeout.as_millis().min(u128::from(u64::MAX)) as u64,
                },
            ) => {}
        }
    }) {
        log::debug!("Skipping Hyperliquid book recovery task after shutdown began: {e}");
    }
}

fn is_retryable_error(error: &HyperliquidWsError) -> bool {
    matches!(
        error,
        HyperliquidWsError::Connection(_)
            | HyperliquidWsError::TungsteniteError(_)
            | HyperliquidWsError::TransportSend(_)
            | HyperliquidWsError::OperationTimeout { .. }
            | HyperliquidWsError::InvalidSnapshot(_)
    )
}

#[cfg(test)]
mod tests {
    use nautilus_common::testing::wait_until_async;
    use nautilus_live::task::TaskGroup;
    use nautilus_network::{error::SendError, websocket::TransportBackend};
    use rstest::rstest;

    use super::*;
    use crate::common::enums::HyperliquidEnvironment;

    #[rstest]
    #[tokio::test]
    async fn rejected_subscription_starts_no_recovery() {
        let book_sync = BookSyncTracker::default();
        let tasks = TaskGroup::new();
        let spawner = tasks.spawner().expect("open task group");
        let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");

        // No instrument is cached, so the subscribe fails before any write
        let ws_client = HyperliquidWebSocketClient::new(
            Some("wss://rejected-book-subscription.test/ws".to_string()),
            HyperliquidEnvironment::Testnet,
            None,
            TransportBackend::default(),
            None,
        );

        spawn_subscription_task(
            instrument_id,
            None,
            None,
            book_sync.clone(),
            ws_client,
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
    #[case::connection(HyperliquidWsError::Connection("e".into()), true)]
    #[case::tungstenite(HyperliquidWsError::TungsteniteError("e".into()), true)]
    #[case::transport_send(HyperliquidWsError::TransportSend(SendError::Closed), true)]
    #[case::operation_timeout(HyperliquidWsError::OperationTimeout { timeout_ms: 10 }, true)]
    #[case::invalid_snapshot(HyperliquidWsError::InvalidSnapshot("e".into()), true)]
    #[case::url_parsing(HyperliquidWsError::UrlParsing("e".into()), false)]
    #[case::message_serialization(HyperliquidWsError::MessageSerialization("e".into()), false)]
    #[case::message_deserialization(
        HyperliquidWsError::MessageDeserialization("e".into()),
        false
    )]
    #[case::channel_send(HyperliquidWsError::ChannelSend("e".into()), false)]
    #[case::client(HyperliquidWsError::ClientError("e".into()), false)]
    fn recovery_retry_classification(#[case] error: HyperliquidWsError, #[case] expected: bool) {
        assert_eq!(is_retryable_error(&error), expected);
    }
}
